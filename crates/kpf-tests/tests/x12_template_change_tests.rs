//! X12-lite: one covenant id across two templates (DESIGN 3.7, 8 X12; `vendor/kob/docs/spec/kob1-payload.md:275`).
//!
//! rusty-kaspa paths are relative to the cargo checkout of v2.1.0 (commit 01b532e).

mod common;

use common::*;
use kaspa_consensus_core::Hash;

const M: Hash = Hash::from_bytes([0x0c; 32]);

/// Consensus classifies a binding as a continuation by comparing ids only: `Some(input_covenant_id) if
/// input_covenant_id == covenant_id` (`covenants.rs:124-136`). The output's script is never compared with the
/// authorising input's, so an input of template A continues its id into template B. Confirms
/// `vendor/kob/docs/spec/kob1-payload.md:275-276` ("consensus lets the authorising input continue its covenant id into
/// ANY script"); the only limit is what A's own script requires.
#[test]
fn x12_id_continues_from_template_a_into_template_b() {
    let a = compile("probes/X12A.sil", &[int(1_000)]);
    let b = compile("probes/X12B.sil", &[bytes32(Hash::from_bytes([0x01; 32]))]);
    assert_ne!(p2sh(&a), p2sh(&b));
    let ins = [contract_in(outpoint(1), 100_000_000, Some(M), &a, "migrate", vec![])];
    let (tx, e) = build(&ins, vec![out(99_000_000, p2sh(&b), bind(0, M))]);
    assert_accepted(&tx, &e);

    // The B instance carries M and is the only input of M when spent.
    let u = utxo_of(&tx, 0);
    assert_eq!(u.1.covenant_id, Some(M));
    let ins = [In { outpoint: u.0, utxo: u.1, spend: Spend::Contract { art: b.clone(), entry: "land", args: vec![bytes32(M)] } }];
    let (tx2, e2) = build(&ins, vec![out(98_000_000, p2pk_spk(&keypair(8)), None)]);
    assert_accepted(&tx2, &e2);
}

/// "Any script" includes a plain P2PK output. That P2PK UTXO then carries M and, being an input that carries M,
/// may itself continue M into any script (`covenants.rs:124-136` never looks at the authoriser's script either):
/// a lineage can pass through key-controlled UTXOs and come back into a template.
#[test]
fn x12_id_passes_through_a_p2pk_output_and_back() {
    let a = compile("probes/X12A.sil", &[int(1_000)]);
    let b = compile("probes/X12B.sil", &[bytes32(Hash::from_bytes([0x01; 32]))]);
    let holder = keypair(5);
    let ins = [contract_in(outpoint(1), 100_000_000, Some(M), &a, "migrate", vec![])];
    let (tx, e) = build(&ins, vec![out(99_000_000, p2pk_spk(&holder), bind(0, M))]);
    assert_accepted(&tx, &e);

    let (op, u) = utxo_of(&tx, 0);
    assert_eq!(u.covenant_id, Some(M));
    let ins = [In { outpoint: op, utxo: u, spend: Spend::P2pk(holder) }];
    let (tx2, e2) = build(&ins, vec![out(98_000_000, p2sh(&b), bind(0, M))]);
    assert_accepted(&tx2, &e2);
    assert_eq!(utxo_of(&tx2, 0).1.covenant_id, Some(M));
}

/// One genesis group may also hold outputs of two templates under one id: the id hashes every group output's script
/// (`covenant_id.rs:22-28`) without requiring them to match.
#[test]
fn x12_one_genesis_group_with_two_templates() {
    let a = compile("probes/X12A.sil", &[int(1_000)]);
    let b = compile("probes/X12B.sil", &[bytes32(Hash::from_bytes([0x01; 32]))]);
    let f = keypair(7);
    let ins = [p2pk_in(outpoint(2), 100_000_000, None, &f)];
    let mut outs = vec![out(50_000_000, p2sh(&a), None), out(49_000_000, p2sh(&b), None)];
    let g = genesis_id(outpoint(2), &outs, &[0, 1]);
    outs[0].covenant = bind(0, g);
    outs[1].covenant = bind(0, g);
    let (tx, e) = build(&ins, outs);
    assert_accepted(&tx, &e);
    assert_eq!(utxo_of(&tx, 0).1.covenant_id, utxo_of(&tx, 1).1.covenant_id);
}
