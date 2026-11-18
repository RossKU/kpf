//! X1: genesis of a pair output authored by a plain P2PK funder (DESIGN 4.5 T1, 8 X1; THREAT_MODEL G1).
//!
//! rusty-kaspa paths are relative to the cargo checkout of v2.1.0 (commit 01b532e, pinned by KOB's `Cargo.lock`):
//! `crypto/txscript/src/covenants.rs` (CovenantsContext::from_tx), `crypto/txscript/src/opcodes/mod.rs` (opcodes),
//! `consensus/core/src/hashing/covenant_id.rs` (the genesis id).

mod common;

use common::*;
use kaspa_consensus_core::tx::{Transaction, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kaspa_txscript_errors::CovenantsError;
use silverscript_abi::SilAbiArtifact;

const OPENER_ID: Hash = Hash::from_bytes([0x0a; 32]);

struct Fx {
    pair: SilAbiArtifact,
    opener: SilAbiArtifact,
    funder: secp256k1::Keypair,
}

fn fx() -> Fx {
    Fx { pair: compile("probes/X1Pair.sil", &[int(1_000)]), opener: compile("probes/X1Opener.sil", &[int(1)]), funder: keypair(7) }
}

/// T1 shape: in 0 opener (covenant input, entry `entry`), in 1 funder F (P2PK, no covenant id).
/// out 0 change to F, out 1 the pair, bound as decided by `bind_pair` (given the unbound outputs).
fn open_tx(f: &Fx, entry: &'static str, args: Vec<silverscript_abi::ArtifactValue>, bind_pair: impl Fn(&[TransactionOutput]) -> Option<kaspa_consensus_core::tx::CovenantBinding>) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [
        contract_in(outpoint(1), 50_000_000, Some(OPENER_ID), &f.opener, entry, args),
        p2pk_in(outpoint(2), 100_000_000, None, &f.funder),
    ];
    let mut outs = vec![out(40_000_000, p2pk_spk(&f.funder), None), out(100_000_000, p2sh(&f.pair), None)];
    outs[1].covenant = bind_pair(&outs);
    build(&ins, outs)
}

/// The id consensus accepts for "F (input 1) authors output 1 alone".
fn f_genesis(outs: &[TransactionOutput]) -> Hash {
    genesis_id(outpoint(2), outs, &[1])
}

fn spend_pair(f: &Fx, pair_utxo: (kaspa_consensus_core::tx::TransactionOutpoint, UtxoEntry)) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [In { outpoint: pair_utxo.0, utxo: pair_utxo.1, spend: Spend::Contract { art: f.pair.clone(), entry: "spend", args: vec![] } }];
    build(&ins, vec![out(99_000_000, p2pk_spk(&keypair(8)), None)])
}

/// A plain P2PK input can author the genesis of a covenant output, and the id is not chosen freely: it must equal
/// covenant_id(outpoint of the authorising input, (index, value, script) of every output of the group)
/// (`covenants.rs:137-160`, `covenant_id.rs:16-30`). The P2PK script is never asked; consensus only recomputes the
/// hash. So the builder of the transaction (whoever holds F's key) decides which input authors and which outputs
/// form the group, and F's SIGHASH_ALL signature commits to the binding (`consensus/core/src/hashing/sighash.rs:233-235`).
/// In the creating transaction the new id is invisible to the covenant counters: genesis outputs are "validated but
/// do not populate covenant contexts" (`covenants.rs:96-100`, `:137-143`), so OpCovInputCount(id) = OpCovOutputCount(id)
/// = 0 (`opcodes/mod.rs:1333-1372`, `covenants.rs:78-89`), while OpOutputCovenantId reads the binding directly
/// (`opcodes/mod.rs:1406-1419`).
#[test]
fn x1_p2pk_funder_authors_the_pair_genesis() {
    let f = fx();
    let (tx, entries) = open_tx(&f, "open", vec![int(1)], |o| bind(1, f_genesis(o)));
    assert_accepted(&tx, &entries);
    // The opener sees a non-zero id, counted under neither inputs nor outputs.
    let (tx, entries) = open_tx(&f, "observe", vec![int(1), int(0), int(0)], |o| bind(1, f_genesis(o)));
    assert_accepted(&tx, &entries);
    let (tx_fresh, e_fresh) = open_tx(&f, "openFresh", vec![int(1)], |o| bind(1, f_genesis(o)));
    assert_accepted(&tx_fresh, &e_fresh);

    // The pair UTXO carries the genesis id and is spendable on its `OpCovInputCount(selfId) == 1` path.
    let utxo = utxo_of(&tx, 1);
    assert_eq!(utxo.1.covenant_id, Some(f_genesis(&tx.outputs)));
    let (spend, se) = spend_pair(&f, utxo);
    assert_accepted(&spend, &se);
}

/// Who decides: the id is a function of the group. Any other id is refused for the whole transaction
/// (`covenants.rs:157-159`, WrongGenesisCovenantId), including an existing id F does not carry: an input that does
/// not carry id X can never author an output with X except by a hash collision, because a binding whose id differs
/// from the authorising input's id is always treated as a genesis (`covenants.rs:124-143`).
#[test]
fn x1_genesis_id_is_fixed_by_outpoint_and_group() {
    let f = fx();
    for wrong in [Hash::from_bytes([0x55; 32]), OPENER_ID] {
        let (tx, entries) = open_tx(&f, "open", vec![int(1)], |_| bind(1, wrong));
        assert!(matches!(run(&tx, &entries), Err(CovenantsError::WrongGenesisCovenantId(1, _))), "{wrong}");
    }
    // The same pair output authored by the opener input instead of F gets another (equally valid) id: the builder
    // chooses the authoriser.
    let (tx, entries) = open_tx(&f, "open", vec![int(1)], |o| bind(0, genesis_id(outpoint(1), o, &[1])));
    assert_accepted(&tx, &entries);
    assert_ne!(tx.outputs[1].covenant.unwrap().covenant_id, f_genesis(&tx.outputs));
    // A group with an extra output of F's (a twin, here a P2PK output) yields one shared id for both outputs
    // (`covenants.rs:142`); the opener's checks cannot see the twin.
    let ins = [
        contract_in(outpoint(1), 50_000_000, Some(OPENER_ID), &f.opener, "openFresh", vec![int(1)]),
        p2pk_in(outpoint(2), 100_000_000, None, &f.funder),
    ];
    let mut outs = vec![out(40_000_000, p2pk_spk(&f.funder), None), out(100_000_000, p2sh(&f.pair), None)];
    let g = genesis_id(outpoint(2), &outs, &[0, 1]);
    outs[0].covenant = bind(1, g);
    outs[1].covenant = bind(1, g);
    let (tx, entries) = build(&ins, outs);
    assert_accepted(&tx, &entries);
    assert_eq!(utxo_of(&tx, 0).1.covenant_id, utxo_of(&tx, 1).1.covenant_id);
}

/// An id-less pair output: consensus accepts it (outputs without a binding are skipped, `covenants.rs:113-116`), but
/// the opener refuses it, because OpOutputCovenantId returns 32 zero bytes for an output without a binding
/// (`opcodes/mod.rs:1412`). DESIGN 8 X1 pass criterion "id-less output is refused by the opening orders".
#[test]
fn x1_idless_pair_output_is_refused_by_the_opener() {
    let f = fx();
    let (tx, entries) = open_tx(&f, "open", vec![int(1)], |_| None);
    let res = run(&tx, &entries).expect("consensus accepts an output without a binding");
    assert!(res[1].is_ok(), "the funder's P2PK input is fine");
    assert_input_refused(&tx, &entries, 0);
}

/// If an id-less pair output is created anyway (no opener checks it), it is unspendable on every path that requires
/// OpCovInputCount(selfId) == 1, forever:
/// - OpInputCovenantId returns ZERO_HASH for a UTXO without an id (`opcodes/mod.rs:1324`);
/// - OpCovInputCount(ZERO_HASH) counts the inputs whose UTXO carries Some(ZERO_HASH) (`covenants.rs:107-111`,
///   `:78-79`); no UTXO ever carries it, because an output id is either the authoriser's own id (inductively never
///   zero) or a genesis hash, and a genesis binding to ZERO_HASH is refused (shown below);
/// - a UTXO's covenant id is fixed when it is created (`consensus/core/src/utxo/utxo_diff.rs:239`), so nothing can add
///   an id later. A template with any path that does not use its id would still be spendable on that path.
#[test]
fn x1_idless_pair_is_locked_forever() {
    let f = fx();
    let payer = keypair(3);
    let ins = [p2pk_in(outpoint(2), 100_000_000, None, &payer)];
    let (create, ce) = build(&ins, vec![out(99_000_000, p2sh(&f.pair), None)]);
    assert_accepted(&create, &ce);
    let utxo = utxo_of(&create, 0);
    assert_eq!(utxo.1.covenant_id, None);
    let (spend, se) = spend_pair(&f, utxo.clone());
    assert_input_refused(&spend, &se, 0);
    // Company does not help: another covenant input in the same transaction leaves the count at zero.
    let ins2 = [
        In { outpoint: utxo.0, utxo: utxo.1, spend: Spend::Contract { art: f.pair.clone(), entry: "spend", args: vec![] } },
        contract_in(outpoint(4), 1_000_000, Some(Hash::from_bytes([0x44; 32])), &f.pair, "spend", vec![]),
    ];
    let (spend2, se2) = build(&ins2, vec![out(99_000_000, p2pk_spk(&keypair(8)), None)]);
    assert_input_refused(&spend2, &se2, 0);
    // A binding to the zero id cannot be authored.
    let ins3 = [p2pk_in(outpoint(5), 100_000_000, None, &payer)];
    let (zero, ze) = build(&ins3, vec![out(99_000_000, p2sh(&f.pair), bind(0, Hash::from_bytes([0; 32])))]);
    assert!(matches!(run(&zero, &ze), Err(CovenantsError::WrongGenesisCovenantId(0, _))));
}

/// OpOutputCovenantId != 0 does not distinguish a genesis from a continuation. A funder whose P2PK UTXO already
/// carries an id X (a stray of some lineage) continues X into the pair output (`covenants.rs:124-136`): the plain
/// non-zero check passes, the counters show the lineage (1 input, 1 output of X), and `openFresh`
/// (OpCovInputCount(id) == 0) refuses it.
#[test]
fn x1_nonzero_check_accepts_an_inherited_lineage() {
    let f = fx();
    let x = Hash::from_bytes([0x77; 32]);
    let mk = |entry: &'static str, args: Vec<silverscript_abi::ArtifactValue>| {
        let ins = [
            contract_in(outpoint(1), 50_000_000, Some(OPENER_ID), &f.opener, entry, args),
            p2pk_in(outpoint(2), 100_000_000, Some(x), &f.funder),
        ];
        build(&ins, vec![out(40_000_000, p2pk_spk(&f.funder), None), out(100_000_000, p2sh(&f.pair), bind(1, x))])
    };
    let (tx, e) = mk("open", vec![int(1)]);
    assert_accepted(&tx, &e);
    let (tx, e) = mk("observe", vec![int(1), int(1), int(1)]);
    assert_accepted(&tx, &e);
    let (tx, e) = mk("openFresh", vec![int(1)]);
    assert_input_refused(&tx, &e, 0);
}
