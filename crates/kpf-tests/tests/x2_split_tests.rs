//! X2: split continuation (DESIGN 4.3 "at most 2 continuations", 8 X2; THREAT_MODEL F3).
//!
//! rusty-kaspa paths are relative to the cargo checkout of v2.1.0 (commit 01b532e).

mod common;

use common::*;
use kaspa_consensus_core::tx::{Transaction, TransactionOutpoint, UtxoEntry};
use kaspa_consensus_core::Hash;
use silverscript_abi::SilAbiArtifact;

const PARENT_ID: Hash = Hash::from_bytes([0x0b; 32]);

fn art() -> SilAbiArtifact {
    compile("probes/X2Split.sil", &[int(1_000)])
}

/// in 0 parent (id P, `split`), in 1 funder; out 0 and 1 the two parts, both bound to (input 0, P); out 2 change.
fn split_tx(a: &SilAbiArtifact) -> (Transaction, Vec<UtxoEntry>) {
    let f = keypair(7);
    let ins = [contract_in(outpoint(1), 100_000_000, Some(PARENT_ID), a, "split", vec![]), p2pk_in(outpoint(2), 10_000_000, None, &f)];
    build(
        &ins,
        vec![
            out(60_000_000, p2sh(a), bind(0, PARENT_ID)),
            out(40_000_000, p2sh(a), bind(0, PARENT_ID)),
            out(9_000_000, p2pk_spk(&f), None),
        ],
    )
}

fn solo_in(a: &SilAbiArtifact, u: (TransactionOutpoint, UtxoEntry)) -> In {
    In { outpoint: u.0, utxo: u.1, spend: Spend::Contract { art: a.clone(), entry: "solo", args: vec![] } }
}

fn spend(ins: Vec<In>) -> (Transaction, Vec<UtxoEntry>) {
    let total: u64 = ins.iter().map(|i| i.utxo.amount).sum();
    build(&ins, vec![out(total - 1_000_000, p2pk_spk(&keypair(8)), None)])
}

/// A binding whose id equals the authorising input's id is a continuation, and one input may authorise any number of
/// them (`covenants.rs:124-136`: each such output is pushed to the input's auth_outputs and to the id's shared
/// output list), so OpCovOutputCount(P) == 2 here. Each part's UTXO takes the binding id (`utxo_diff.rs:239`): BOTH
/// parts carry the parent's id P. Ids are not fresh per output.
#[test]
fn x2_split_parts_keep_the_parent_id() {
    let a = art();
    let (tx, e) = split_tx(&a);
    assert_accepted(&tx, &e);
    let (p0, p1) = (utxo_of(&tx, 0), utxo_of(&tx, 1));
    assert_eq!(p0.1.covenant_id, Some(PARENT_ID));
    assert_eq!(p1.1.covenant_id, Some(PARENT_ID));
}

/// With only one of the two outputs bound, OpCovOutputCount(P) == 1 and `split` refuses.
#[test]
fn x2_split_with_one_bound_output_is_refused() {
    let a = art();
    let f = keypair(7);
    let ins = [contract_in(outpoint(1), 100_000_000, Some(PARENT_ID), &a, "split", vec![]), p2pk_in(outpoint(2), 10_000_000, None, &f)];
    let (tx, e) = build(&ins, vec![out(60_000_000, p2sh(&a), bind(0, PARENT_ID)), out(40_000_000, p2sh(&a), None)]);
    assert_input_refused(&tx, &e, 0);
}

/// Each part alone: OpCovInputCount(P) counts the inputs of THIS transaction whose UTXO carries P
/// (`covenants.rs:107-111`), which is 1. Both parts in one transaction: the count is 2 and the single-input path
/// refuses both (DESIGN 8 X2 pass criterion).
#[test]
fn x2_parts_spend_alone_but_not_together() {
    let a = art();
    let (tx, e) = split_tx(&a);
    assert_accepted(&tx, &e);
    let (p0, p1) = (utxo_of(&tx, 0), utxo_of(&tx, 1));

    let (s0, e0) = spend(vec![solo_in(&a, p0.clone())]);
    assert_accepted(&s0, &e0);
    let (s1, e1) = spend(vec![solo_in(&a, p1.clone())]);
    assert_accepted(&s1, &e1);

    let (both, eb) = spend(vec![solo_in(&a, p0), solo_in(&a, p1)]);
    assert_input_refused(&both, &eb, 0);
    assert_input_refused(&both, &eb, 1);
}

/// The alternative: the parent authorises each part as its own one-output genesis group. A binding whose id differs
/// from the authoriser's is a genesis (`covenants.rs:137-143`), so each part gets a fresh id derived from the parent's
/// outpoint and that part alone (`covenant_id.rs:16-30`). The parent's contract can see them only through
/// OpOutputCovenantId (genesis outputs are not in the covenant contexts, `covenants.rs:96-100`), and it proves
/// freshness with OpCovInputCount(id) == 0. The two parts can then be co-spent on single-input paths.
#[test]
fn x2_fresh_id_split_gives_distinct_ids() {
    let a = art();
    let f = keypair(7);
    let ins = [
        contract_in(outpoint(1), 100_000_000, Some(PARENT_ID), &a, "splitFresh", vec![int(0), int(1)]),
        p2pk_in(outpoint(2), 10_000_000, None, &f),
    ];
    let mut outs = vec![out(60_000_000, p2sh(&a), None), out(40_000_000, p2sh(&a), None), out(9_000_000, p2pk_spk(&f), None)];
    let (g0, g1) = (genesis_id(outpoint(1), &outs, &[0]), genesis_id(outpoint(1), &outs, &[1]));
    outs[0].covenant = bind(0, g0);
    outs[1].covenant = bind(0, g1);
    let (tx, e) = build(&ins, outs);
    assert_accepted(&tx, &e);
    assert_ne!(g0, g1);
    assert_ne!(g0, PARENT_ID);

    let (both, eb) = spend(vec![solo_in(&a, utxo_of(&tx, 0)), solo_in(&a, utxo_of(&tx, 1))]);
    assert_accepted(&both, &eb);
}

/// splitFresh refuses a same-id split (OpCovOutputCount(selfId) != 0), and `split` refuses a fresh-id split
/// (OpCovOutputCount(selfId) == 0 != 2): the contract, not consensus, chooses which kind of split it allows.
#[test]
fn x2_each_split_path_refuses_the_other_kind() {
    let a = art();
    let f = keypair(7);
    let same = |entry: &'static str, args| {
        let ins = [contract_in(outpoint(1), 100_000_000, Some(PARENT_ID), &a, entry, args), p2pk_in(outpoint(2), 10_000_000, None, &f)];
        build(&ins, vec![out(60_000_000, p2sh(&a), bind(0, PARENT_ID)), out(40_000_000, p2sh(&a), bind(0, PARENT_ID))])
    };
    let (tx, e) = same("splitFresh", vec![int(0), int(1)]);
    assert_input_refused(&tx, &e, 0);

    let ins = [contract_in(outpoint(1), 100_000_000, Some(PARENT_ID), &a, "split", vec![]), p2pk_in(outpoint(2), 10_000_000, None, &f)];
    let mut outs = vec![out(60_000_000, p2sh(&a), None), out(40_000_000, p2sh(&a), None)];
    let (g0, g1) = (genesis_id(outpoint(1), &outs, &[0]), genesis_id(outpoint(1), &outs, &[1]));
    outs[0].covenant = bind(0, g0);
    outs[1].covenant = bind(0, g1);
    let (tx, e) = build(&ins, outs);
    assert_input_refused(&tx, &e, 0);
}
