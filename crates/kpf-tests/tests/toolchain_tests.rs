//! Toolchain proof: compile `contracts/Trivial.sil` with the vendored SilverScript compiler and execute a hand-built
//! transaction spending it in the rusty-kaspa v2.1.0 script engine.

mod common;

use common::*;

fn trivial() -> silverscript_abi::SilAbiArtifact {
    compile("Trivial.sil", &[int(42)])
}

fn spend(x: i64) -> (kaspa_consensus_core::tx::Transaction, Vec<kaspa_consensus_core::tx::UtxoEntry>) {
    let art = trivial();
    let payee = keypair(9);
    build(&[contract_in(outpoint(1), 10_000, None, &art, "unlock", vec![int(x)])], vec![out(9_000, p2pk_spk(&payee), None)])
}

#[test]
fn trivial_compiles_to_p2sh() {
    let art = trivial();
    assert!(!bytecode(&art).is_empty());
    // The P2SH script is `OpBlake2b OpData32 <hash> OpEqual` (version 0), as for every KOB order.
    assert_eq!(p2sh(&art).script().len(), 35);
}

#[test]
fn trivial_accepts_the_secret() {
    let (tx, entries) = spend(42);
    assert_accepted(&tx, &entries);
}

#[test]
fn trivial_refuses_a_wrong_value() {
    let (tx, entries) = spend(41);
    assert_input_refused(&tx, &entries, 0);
}
