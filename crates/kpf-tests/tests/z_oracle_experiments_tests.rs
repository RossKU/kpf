//! The experiments the first slice of oracle-only markets needs (docs/DESIGN-ORACLE-2.md 7: Z1, Z11 and the measurable
//! part of Z4), in the rusty-kaspa v2.1.0 script engine. Results are summarised in `docs/DESIGN-ORACLE-IMPL.md`.
//!
//! Z1, an underwriter's own input. The note (2 §1.5) has the pair accept a price only when input `j` of the claim is a
//! plain P2PK input of the underwriter's key. Probe `contracts/probes/Z1Spk.sil`:
//! - `tx.inputs[j].scriptPubKey` (`OpTxInputSpk`) returns input j's script public key, so a contract can require it to
//!   be exactly `0x20 || key || OP_CHECKSIG`: accepted for that key's input, refused for another key, for a P2SH input,
//!   and for the contract's own input. Consensus then requires that input's own script (the key's signature) to pass.
//! - What the key signed is visible too: input j's signature script is one 65-byte push whose last byte is the sighash
//!   type. A SIGHASH_NONE or ANYONECANPAY signature is valid for consensus (the transaction is accepted), so an
//!   underwriter that signed only its own input could be pulled into any transaction; `keyInputAll` refuses it by
//!   requiring the length 66, the push 0x41 and the type 0x01. The note's "its SIGHASH_ALL signature covers every
//!   outpoint" therefore holds only if the pair checks the type byte: **confirmed, with that extra check**.
//! - The raw read (`OpTxInputSpkLen`, `OpTxInputSpkSubstr`) returns the two version bytes (0x0000) before the script.
//! - Cost: `keyInput` 577 script units, `keyInputAll` 590 (no signature operation in the reading input; the one
//!   signature check, 100,000 units, is in the key's own input). Numbers printed by `z1_*`.
//!
//! Z11, a twin pair at genesis. **The note's first option is refuted**: in the creating transaction the new id is
//! invisible to the covenant counters (genesis outputs "do not populate covenant contexts", X1), so
//! `OpCovOutputCount(newId)` is 0 with one output of the id and still 0 with a twin; a rule "== 1" refuses every
//! honest genesis and a rule "<= 1" accepts every twin. **The second option holds**: a scan of every output with
//! `OpOutputCovenantId(j) != newId` for `j != pairOut` (which reads the binding directly), with the output count
//! bounded (8 here, unrolled as every KPF scan). A twin is necessarily in the same transaction: a genesis id is the
//! hash of its authorising outpoint and its group (X1), so no later transaction can author it. Cost of the scan of 8
//! outputs: 1,921 script units (printed by `z11_*`). Built into the stand-in order `KpfOpenStub.fill`.
//!
//! Z4 (the part measurable now) is in `kpf_bond_metrics_tests`.

mod common;

use common::*;
use kaspa_consensus_core::hashing::sighash::{calc_schnorr_signature_hash, SigHashReusedValuesUnsync};
use kaspa_consensus_core::hashing::sighash_type::{SigHashType, SIG_HASH_ALL, SIG_HASH_ANY_ONE_CAN_PAY, SIG_HASH_NONE};
use kaspa_consensus_core::tx::{MutableTransaction, Transaction, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::verify::{run_raw, Meter};
use secp256k1::{Message, Secp256k1};
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

fn z1() -> SilAbiArtifact {
    compile("probes/Z1Spk.sil", &[int(1)])
}

fn z11() -> SilAbiArtifact {
    compile("probes/Z11Twin.sil", &[int(11)])
}

fn key_of(seed: u8) -> ArtifactValue {
    ArtifactValue::Bytes(keypair(seed).x_only_public_key().0.serialize().to_vec())
}

/// Input 0 the probe calling `entry`, input 1 a P2PK input of key 5 signed with `hash_type`, input 2 another P2SH
/// input (the probe again, as a stand-in for any contract input).
fn z1_tx(entry: &'static str, args: Vec<ArtifactValue>, hash_type: SigHashType) -> (Transaction, Vec<UtxoEntry>) {
    let a = z1();
    let k = keypair(5);
    let ins = [
        contract_in(outpoint(1), 100_000_000, None, &a, entry, args.clone()),
        p2pk_in(outpoint(2), 50_000_000, None, &k),
        contract_in(outpoint(3), 10_000_000, None, &a, entry, args),
    ];
    let (mut tx, e) = build(&ins, vec![out(150_000_000, p2pk_spk(&keypair(9)), None)]);
    if hash_type.to_u8() != SIG_HASH_ALL.to_u8() {
        // Re-sign input 1 with the requested sighash type (signature scripts are not part of the sighash).
        let mt = MutableTransaction::with_entries(tx.clone(), e.clone());
        let reused = SigHashReusedValuesUnsync::new();
        let h = calc_schnorr_signature_hash(&mt.as_verifiable(), 1, hash_type, &reused);
        let mut s = Secp256k1::new().sign_schnorr_no_aux_rand(&Message::from_digest(h.as_bytes()), &k).as_ref().to_vec();
        s.push(hash_type.to_u8());
        tx.inputs[1].signature_script = push(&s);
        tx.finalize();
    }
    (tx, e)
}

fn results(t: &(Transaction, Vec<UtxoEntry>)) -> Vec<bool> {
    run(&t.0, &t.1).expect("bindings").iter().map(|r| r.is_ok()).collect()
}

#[test]
fn z1_a_contract_reads_another_inputs_script_and_its_sighash_type() {
    // The key's input is recognised; another key, a P2SH input and the probe's own input are not.
    assert_eq!(results(&z1_tx("keyInput", vec![int(1), key_of(5)], SIG_HASH_ALL)), vec![true, true, true]);
    assert!(!results(&z1_tx("keyInput", vec![int(1), key_of(6)], SIG_HASH_ALL))[0], "another key");
    assert!(!results(&z1_tx("keyInput", vec![int(2), key_of(5)], SIG_HASH_ALL))[0], "a P2SH input");
    assert!(!results(&z1_tx("keyInput", vec![int(0), key_of(5)], SIG_HASH_ALL))[0], "its own input");
    // The raw bytes: 0x20 || key || OP_CHECKSIG (0xac), with or without the two version bytes in front.
    let mut script = vec![0x20];
    script.extend_from_slice(&keypair(5).x_only_public_key().0.serialize());
    script.push(0xac);
    let versioned = [vec![0x00, 0x00], script.clone()].concat();
    let plain_ok = results(&z1_tx("spkBytes", vec![int(1), ArtifactValue::Bytes(script.clone())], SIG_HASH_ALL))[0];
    let versioned_ok = results(&z1_tx("spkBytes", vec![int(1), ArtifactValue::Bytes(versioned.clone())], SIG_HASH_ALL))[0];
    println!("Z1 OpTxInputSpkSubstr reads: script only {plain_ok}, version + script {versioned_ok}");
    assert!(plain_ok ^ versioned_ok, "exactly one layout");
    let mut bad = if versioned_ok { versioned } else { script };
    let n = bad.len();
    bad[n - 2] ^= 1;
    assert!(!results(&z1_tx("spkBytes", vec![int(1), ArtifactValue::Bytes(bad)], SIG_HASH_ALL))[0]);

    // A signature that commits to less is still valid for consensus (input 1 passes), so `keyInput` alone cannot tell
    // that the key consented to this transaction; `keyInputAll` reads the sighash byte and refuses it.
    for ht in [SIG_HASH_NONE, SigHashType::from_u8(SIG_HASH_ALL.to_u8() | SIG_HASH_ANY_ONE_CAN_PAY.to_u8()).unwrap()] {
        let t = z1_tx("keyInput", vec![int(1), key_of(5)], ht);
        assert_eq!(results(&t), vec![true, true, true], "the weaker signature is accepted by consensus and keyInput");
        let t = z1_tx("keyInputAll", vec![int(1), key_of(5)], ht);
        let r = results(&t);
        assert!(r[1], "input 1 itself is valid");
        assert!(!r[0], "keyInputAll refuses a signature that is not SIGHASH_ALL");
    }
    assert!(results(&z1_tx("keyInputAll", vec![int(1), key_of(5)], SIG_HASH_ALL))[0]);
}

#[test]
fn z1_the_cost_of_reading_an_underwriters_input() {
    for entry in ["keyInput", "keyInputAll"] {
        let t = z1_tx(entry, vec![int(1), key_of(5)], SIG_HASH_ALL);
        let priced = run_raw(&t.0, &t.1, Meter::Unlimited).expect("bindings")[0].clone().expect("passes");
        let free = run_raw(&t.0, &t.1, Meter::Free).expect("bindings")[0].clone().expect("passes");
        println!("Z1 {entry}: {priced} script units ({free} without sig-op charges)");
        assert_eq!(priced, free, "{entry}: no signature operation in the reading input");
        assert!(priced < 5_000, "{entry}: {priced}");
    }
}

/// Input 0 the probe calling `entry(args)`, input 1 the funder F (P2PK) that authors the genesis. Outputs: 0 change,
/// 1 the new "pair" (P2SH of the probe), and `twins` more outputs in F's genesis group (copies of the pair), plus
/// `extra` plain outputs. The group decides the id (X1): every output of the group shares it.
fn z11_tx(entry: &'static str, args: Vec<ArtifactValue>, twins: usize, extra: usize) -> (Transaction, Vec<UtxoEntry>) {
    let a = z11();
    let f = keypair(7);
    let ins = [contract_in(outpoint(1), 50_000_000, Some(Hash::from_bytes([0x0b; 32])), &a, entry, args), p2pk_in(outpoint(2), 100_000_000, None, &f)];
    let mut outs: Vec<TransactionOutput> = vec![out(40_000_000, p2pk_spk(&f), None), out(50_000_000, p2sh(&a), None)];
    for _ in 0..twins {
        outs.push(out(10_000_000, p2sh(&a), None));
    }
    for _ in 0..extra {
        outs.push(out(1_000_000, p2pk_spk(&keypair(9)), None));
    }
    let group: Vec<usize> = (1..2 + twins).collect();
    let g = genesis_id(outpoint(2), &outs, &group);
    for i in &group {
        outs[*i].covenant = bind(1, g);
    }
    build(&ins, outs)
}

#[test]
fn z11_the_counter_does_not_see_a_twin_at_genesis() {
    // OpCovOutputCount(newId) is 0 with no twin and with one or two: the counter cannot refuse a twin.
    for twins in [0usize, 1, 2] {
        assert!(results(&z11_tx("count", vec![int(1), int(0)], twins, 0))[0], "{twins} twins: count 0");
        assert!(!results(&z11_tx("count", vec![int(1), int(1 + twins as i64)], twins, 0))[0], "{twins} twins: not counted");
    }
    // So the rule "exactly one output of the new id" by the counter refuses the honest genesis as well.
    assert!(!results(&z11_tx("onlyOneByCount", vec![int(1)], 0, 0))[0], "the counter rule refuses every genesis");
}

#[test]
fn z11_an_output_scan_refuses_a_twin_at_genesis() {
    assert!(results(&z11_tx("onlyOneByScan", vec![int(1)], 0, 0))[0], "the honest genesis");
    assert!(results(&z11_tx("onlyOneByScan", vec![int(1)], 0, 6))[0], "8 outputs, no twin");
    for twins in [1usize, 2] {
        assert!(!results(&z11_tx("onlyOneByScan", vec![int(1)], twins, 0))[0], "{twins} twins refused");
    }
    // A twin at the last scanned index, and the bound on the output count.
    assert!(!results(&z11_tx("onlyOneByScan", vec![int(1)], 1, 5))[0]);
    assert!(!results(&z11_tx("onlyOneByScan", vec![int(1)], 0, 7))[0], "9 outputs exceed the scan");
    let t = z11_tx("onlyOneByScan", vec![int(1)], 0, 6);
    let units = run_raw(&t.0, &t.1, Meter::Unlimited).expect("bindings")[0].clone().expect("passes");
    println!("Z11 scan of 8 outputs: {units} script units");
    assert!(units < 10_000);
}
