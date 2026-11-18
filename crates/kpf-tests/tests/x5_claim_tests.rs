//! X5-lite: hand-over outputs claimed by covenant id (DESIGN 4.4 "Hand-over outputs are claimed once", 8 X5;
//! THREAT_MODEL F2).
//!
//! rusty-kaspa paths are relative to the cargo checkout of v2.1.0 (commit 01b532e).

mod common;

use common::*;
use kaspa_consensus_core::tx::{Transaction, TransactionOutpoint, UtxoEntry};
use kaspa_consensus_core::Hash;
use silverscript_abi::SilAbiArtifact;

const ZERO: Hash = Hash::from_bytes([0; 32]);
const I1: Hash = Hash::from_bytes([0x11; 32]);
const I2: Hash = Hash::from_bytes([0x22; 32]);
const P: Hash = Hash::from_bytes([0x33; 32]);

/// A claimer instance (its own `tag` is irrelevant to the claim).
fn claimer() -> SilAbiArtifact {
    compile("probes/X5Claim.sil", &[bytes32(ZERO)])
}

/// The instance a claimer with id `id` requires at output j: same template, `tag = id`.
fn target(id: Hash) -> SilAbiArtifact {
    compile("probes/X5Claim.sil", &[bytes32(id)])
}

fn claim_in(op: TransactionOutpoint, utxo: UtxoEntry, art: &SilAbiArtifact, entry: &'static str) -> In {
    In { outpoint: op, utxo, spend: Spend::Contract { art: art.clone(), entry, args: vec![int(0)] } }
}

/// Two claimers on one output 0 whose script is `target(tag)`.
fn two_claims(a: In, b: In, tag: Hash) -> (Transaction, Vec<UtxoEntry>) {
    let total = a.utxo.amount + b.utxo.amount;
    build(&[a, b], vec![out(total / 2, p2sh(&target(tag)), None), out(total / 2 - 1_000_000, p2pk_spk(&keypair(8)), None)])
}

/// Twins produced by an X2-style split of one claimer with id P (both continuations carry P, as X2 shows).
fn split_twins(c: &SilAbiArtifact) -> [(TransactionOutpoint, UtxoEntry); 2] {
    let f = keypair(7);
    let ins = [contract_in(outpoint(1), 100_000_000, Some(P), c, "split2", vec![]), p2pk_in(outpoint(2), 10_000_000, None, &f)];
    let (tx, e) = build(&ins, vec![out(50_000_000, p2sh(c), bind(0, P)), out(50_000_000, p2sh(c), bind(0, P))]);
    assert_accepted(&tx, &e);
    [utxo_of(&tx, 0), utxo_of(&tx, 1)]
}

/// Distinct ids: the required output script is P2SH(template with tag = own id), a function of the id
/// (validateOutputState rebuilds prefix || new state || suffix and compares the P2SH,
/// `vendor/kob/vendor/silverscript/silverscript-lang/src/compiler/compile/state.rs:292-329`). One output has one
/// script, so it satisfies at most one of two claimers with different ids.
#[test]
fn x5_one_output_cannot_satisfy_two_distinct_ids() {
    let c = claimer();
    let mk = |entry| {
        two_claims(
            contract_in(outpoint(1), 50_000_000, Some(I1), &c, entry, vec![int(0)]),
            contract_in(outpoint(2), 50_000_000, Some(I2), &c, entry, vec![int(0)]),
            I1,
        )
    };
    for entry in ["claimUnguarded", "claim"] {
        let (tx, e) = mk(entry);
        let res = run(&tx, &e).expect("bindings valid");
        assert!(res[0].is_ok(), "{entry}: the claimer whose id is the tag passes");
        assert_input_refused(&tx, &e, 1);
    }
}

/// Twins from a split share one id (`covenants.rs:124-136`, X2), so the unguarded claim is satisfied twice by ONE
/// output: the aliasing DESIGN 4.4 says cannot happen. The guard OpCovInputCount(selfId) == 1 refuses both, because
/// the count is per transaction over all inputs carrying the id (`covenants.rs:107-111`).
#[test]
fn x5_split_twins_alias_one_output_unless_guarded() {
    let c = claimer();
    let [t0, t1] = split_twins(&c);
    assert_eq!(t0.1.covenant_id, t1.1.covenant_id);

    let (tx, e) = two_claims(claim_in(t0.0, t0.1.clone(), &c, "claimUnguarded"), claim_in(t1.0, t1.1.clone(), &c, "claimUnguarded"), P);
    assert_accepted(&tx, &e);

    let (tx, e) = two_claims(claim_in(t0.0, t0.1, &c, "claim"), claim_in(t1.0, t1.1, &c, "claim"), P);
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
}

/// A split is not the only source of shared ids: a multi-output genesis group binds all its outputs to one id
/// (`covenants.rs:142`, `covenant_id.rs:16-30`). Genesis twins alias the unguarded claim exactly as split twins do.
#[test]
fn x5_genesis_twins_also_share_an_id() {
    let c = claimer();
    let f = keypair(7);
    let ins = [p2pk_in(outpoint(2), 100_000_000, None, &f)];
    let mut outs = vec![out(49_000_000, p2sh(&c), None), out(49_000_000, p2sh(&c), None)];
    let g = genesis_id(outpoint(2), &outs, &[0, 1]);
    outs[0].covenant = bind(0, g);
    outs[1].covenant = bind(0, g);
    let (tx, e) = build(&ins, outs);
    assert_accepted(&tx, &e);
    let (t0, t1) = (utxo_of(&tx, 0), utxo_of(&tx, 1));

    let (tx, e) = two_claims(claim_in(t0.0, t0.1.clone(), &c, "claimUnguarded"), claim_in(t1.0, t1.1.clone(), &c, "claimUnguarded"), g);
    assert_accepted(&tx, &e);
    let (tx, e) = two_claims(claim_in(t0.0, t0.1, &c, "claim"), claim_in(t1.0, t1.1, &c, "claim"), g);
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
}
