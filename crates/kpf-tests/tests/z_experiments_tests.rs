//! The two experiments guaranteed fast settlement needs before its contracts (docs/DESIGN-ORACLE.md 6, Z1 and Z2;
//! results in DESIGN 4.5f), in the rusty-kaspa v2.1.0 script engine.
//!
//! Z1, the data-signature builtin. `checkMsgSig` compiles to `OpCheckSigFromStack`. Measured with consensus engine
//! flags (a signature check costs `SIGOP_SCRIPT_UNITS` = 100,000 script units) on the probe `contracts/probes/Z1MsgSig.sil`:
//!
//! | entry | signatures | script units | without the sig-op charge | sig-op charge |
//! |---|---|---|---|---|
//! | `none` (baseline) | 0 | 1,615 | 1,615 | 0 |
//! | `msg1` | 1 data | 101,720 | 1,720 | 100,000 |
//! | `msg2` | 2 data | 201,951 | 1,951 | 200,000 |
//! | `msg4` | 4 data | 402,409 | 2,409 | 400,000 |
//! | `msg8` | 8 data | 803,321 | 3,321 | 800,000 |
//! | `tx1` | 1 transaction | 101,673 | 1,673 | 100,000 |
//! | `tx8` | 8 transaction | 803,056 | 3,056 | 800,000 |
//!
//! (the charges are asserted below; `z1_*` prints the numbers). So a data signature is priced exactly like a
//! transaction signature in script units, one signature operation each (the note's pass criterion), about 115 units
//! of opcodes besides. The static sig-op bound of an input (`get_sig_op_count_upper_bound`, the 15-per-P2SH-input rule
//! a node applies) counts the template's 9 `OpCheckSig` and none of its 15 `OpCheckSigFromStack`: data signatures are
//! metered by the compute budget only, not by the static count, so the 15-signer cap of the note (O§1.3) does not bind
//! them; the script-unit budget does (8 data signatures: 803,321 units, 81 budget units of 10,000).
//!
//! Decision: guaranteed fast settlement does not use data signatures. A guarantor consents by spending its own KAS
//! into the lock output with an ordinary SIGHASH_ALL transaction signature; the claimant (`KpfPending.guarantee`)
//! reads the lock *output*, whose value is physical whatever funded it. A data signature would not spend anything, so
//! one stake could back many claims in parallel (DESIGN-ORACLE 1.3, attack 13); this stage needs neither its
//! reusability nor its one-round assembly.
//!
//! Z2, what a contract reads of time. `OpTxInputDaaScore(i)` returns the `block_daa_score` of input i's UTXO entry,
//! exactly (any value, including one far in the past), and -1 for the `u64::MAX` a mempool gives a parent that is not
//! yet accepted. CSV (`this.ageDaa >= a`) checks only the input's committed sequence and CLTV (`tx.daa >= t`) only the
//! lock time; the node enforces those against the real DAA score (sequence: `entry DAA + sequence <= the accepting
//! block's DAA`; lock time: `lock time <= the accepting block's DAA`), so both are lower bounds on the time of
//! acceptance and nothing a script reads is an upper bound. The UTXO's DAA score is set by consensus to the DAA score of
//! the chain block that accepts (merges) the creating transaction (`consensus/core/src/utxo/utxo_diff.rs`, called with
//! the merging block's DAA score), the same on every node and never earlier than the creating transaction's lock time.
//!
//! Is it exact enough for the late-submission rule of the note (O§1.4, `P <= visibleUntil`)? Yes: P is the pending
//! part's own entry DAA score, fixed once accepted; a contract that spends it reads exactly that number. Acceptance can
//! only come later than inclusion (a block merged a little later is accepted at the merging block's score), so lateness
//! of acceptance makes `P` larger and the rule stricter, never looser. What the rule can not do is bound the time of
//! the *guarantee* transaction itself (no upper bound on now exists); that is why the lock's liability is measured on
//! UTXO scores (`P`, and the lock's own `F`) and not on the lock time (`KpfLock.dispute`).

mod common;

use common::*;
use kaspa_consensus_core::hashing::sighash::SigHashReusedValuesUnsync;
use kaspa_consensus_core::tx::{PopulatedTransaction, UtxoEntry};
use kpf_protocol::verify::{run_raw, Meter};
use secp256k1::{Message, Secp256k1};
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

fn z1() -> SilAbiArtifact {
    compile("probes/Z1MsgSig.sil", &[int(1)])
}

fn z2() -> SilAbiArtifact {
    compile("probes/Z2Daa.sil", &[int(2)])
}

const D: [u8; 32] = [0x5a; 32];

fn datasig(seed: u8) -> ArtifactValue {
    let kp = keypair(seed);
    let s = Secp256k1::new().sign_schnorr(&Message::from_digest(D), &kp);
    ArtifactValue::Bytes(s.as_ref().to_vec())
}

fn key(seed: u8) -> ArtifactValue {
    ArtifactValue::Bytes(keypair(seed).x_only_public_key().0.serialize().to_vec())
}

/// (script units with consensus sig-op pricing, script units with free signatures, the static sig-op bound).
fn measure(art: &SilAbiArtifact, entry: &'static str, args: Vec<Arg>) -> (u64, u64, u32) {
    let ins = [In { outpoint: outpoint(1), utxo: UtxoEntry::new(100_000_000, p2sh(art), 0, false, None), spend: Spend::Signed { art: art.clone(), entry, args } }];
    let (tx, e) = build(&ins, vec![out(99_000_000, p2pk_spk(&keypair(9)), None)]);
    let priced = run_raw(&tx, &e, Meter::Unlimited).expect("bindings")[0].clone().expect("input passes");
    let free = run_raw(&tx, &e, Meter::Free).expect("bindings")[0].clone().expect("input passes");
    let bound = kaspa_txscript::get_sig_op_count_upper_bound::<PopulatedTransaction, SigHashReusedValuesUnsync>(&tx.inputs[0].signature_script, &e[0].script_public_key) as u32;
    (priced, free, bound)
}

fn msg_args(n: u8) -> Vec<Arg> {
    let mut a: Vec<Arg> = (0..n).map(|i| Arg::V(datasig(40 + i))).collect();
    a.push(Arg::V(ArtifactValue::Bytes(D.to_vec())));
    a.extend((0..n).map(|i| Arg::V(key(40 + i))));
    a
}

fn tx_args(n: u8) -> Vec<Arg> {
    let mut a: Vec<Arg> = (0..n).map(|i| Arg::Sig(keypair(40 + i))).collect();
    a.extend((0..n).map(|i| Arg::V(key(40 + i))));
    a
}

#[test]
fn z1_check_msg_sig_is_priced_like_check_sig_but_not_counted_as_a_sig_op() {
    let a = z1();
    let (base, base_free, base_ops) = measure(&a, "none", vec![Arg::V(ArtifactValue::Bytes(D.to_vec())), Arg::V(key(40))]);
    assert_eq!(base, base_free, "no signature, no sig-op charge");
    // The static bound is a property of the whole redeem script, not of the entry taken: it counts the 9 OpCheckSig of
    // tx1 and tx8 and none of the 15 OpCheckSigFromStack of msg1..msg8.
    assert_eq!(base_ops, 9);
    println!("Z1 none: {base} script units, static sig-op bound {base_ops}");
    for (entry, n) in [("msg1", 1u8), ("msg2", 2), ("msg4", 4), ("msg8", 8)] {
        let (u, f, ops) = measure(&a, entry, msg_args(n));
        println!("Z1 {entry}: {u} script units ({f} without the sig-op charge), static sig-op bound {ops}");
        // Each data signature is charged exactly one signature operation (100,000 units), like a transaction signature.
        assert_eq!(u - f, n as u64 * kpf_protocol::tx::SIGOP_SCRIPT_UNITS, "{entry}: sig-op charge");
        // The static bound a node applies to a P2SH input does not see OpCheckSigFromStack.
        assert_eq!(ops, 9, "{entry}: static sig-op bound (the template's OpCheckSig only)");
    }
    for (entry, n) in [("tx1", 1u8), ("tx8", 8)] {
        let (u, f, ops) = measure(&a, entry, tx_args(n));
        println!("Z1 {entry}: {u} script units ({f} without the sig-op charge), static sig-op bound {ops}");
        assert_eq!(u - f, n as u64 * kpf_protocol::tx::SIGOP_SCRIPT_UNITS, "{entry}: sig-op charge");
        assert_eq!(ops, 9, "{entry}: static sig-op bound");
    }
    // The non-signature part of one check is the same for both builtins (within the cost of the extra 32-byte push).
    let (m1, m1f, _) = measure(&a, "msg1", msg_args(1));
    let (t1, t1f, _) = measure(&a, "tx1", tx_args(1));
    assert!(m1f.abs_diff(t1f) < 1_000, "msg1 {m1f} vs tx1 {t1f} without the charge");
    assert!(m1.abs_diff(t1) < 1_000);
}

#[test]
fn z1_a_wrong_data_signature_is_refused() {
    let a = z1();
    let mut args = msg_args(1);
    args[0] = Arg::V(datasig(99));
    let ins = [In { outpoint: outpoint(1), utxo: UtxoEntry::new(100_000_000, p2sh(&a), 0, false, None), spend: Spend::Signed { art: a.clone(), entry: "msg1", args } }];
    let (tx, e) = build(&ins, vec![out(99_000_000, p2pk_spk(&keypair(9)), None)]);
    assert_input_fails(&tx, &e, 0);
}

fn z2_tx(entry: &'static str, args: Vec<Arg>, utxo_daa: u64, lock: u64, seq: u64) -> (kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>) {
    let a = z2();
    let f = keypair(8);
    let ins = [
        In { outpoint: outpoint(1), utxo: UtxoEntry::new(100_000_000, p2sh(&a), utxo_daa, false, None), spend: Spend::Signed { art: a.clone(), entry, args } },
        In { outpoint: outpoint(2), utxo: UtxoEntry::new(50_000_000, p2pk_spk(&f), 7_777, false, None), spend: Spend::P2pk(f) },
    ];
    build_ext(&ins, vec![out(149_000_000, p2pk_spk(&keypair(9)), None)], lock, &[seq, 0])
}

fn ok(t: (kaspa_consensus_core::tx::Transaction, Vec<UtxoEntry>)) -> bool {
    run(&t.0, &t.1).expect("bindings")[0].is_ok()
}

#[test]
fn z2_a_contract_reads_the_utxo_entry_daa_score_exactly() {
    for daa in [0u64, 1, 600, 10_000_000, 87_759_350, 499_999_999_999] {
        assert!(ok(z2_tx("own", vec![Arg::V(ArtifactValue::Int(daa as i64))], daa, 0, 0)), "reads {daa}");
        assert!(!ok(z2_tx("own", vec![Arg::V(ArtifactValue::Int(daa as i64 + 1))], daa, 0, 0)), "not {daa} + 1");
    }
    // Another input's score, read by index.
    assert!(ok(z2_tx("other", vec![Arg::V(ArtifactValue::Int(1)), Arg::V(ArtifactValue::Int(7_777))], 5, 0, 0)));
    // A parent the mempool has not seen accepted: u64::MAX, read as -1 (the reason every KPF clock refuses a negative one).
    assert!(ok(z2_tx("own", vec![Arg::V(ArtifactValue::Int(-1))], u64::MAX, 0, 0)));
}

#[test]
fn z2_the_late_submission_rule_is_a_comparison_of_the_entry_score() {
    let vis = 87_759_350i64;
    assert!(ok(z2_tx("atMost", vec![Arg::V(ArtifactValue::Int(vis))], vis as u64, 0, 0)), "accepted at the limit");
    assert!(!ok(z2_tx("atMost", vec![Arg::V(ArtifactValue::Int(vis))], vis as u64 + 1, 0, 0)), "one DAA late is refused");
    assert!(!ok(z2_tx("atMost", vec![Arg::V(ArtifactValue::Int(vis))], u64::MAX, 0, 0)), "an unaccepted parent is refused");
}

#[test]
fn z2_csv_and_cltv_check_only_what_the_transaction_commits() {
    // CSV: the input's sequence, whatever the UTXO's DAA score (the node then requires entry DAA + sequence <= now).
    assert!(ok(z2_tx("age", vec![Arg::V(ArtifactValue::Int(600))], 10_000_000, 0, 600)));
    assert!(!ok(z2_tx("age", vec![Arg::V(ArtifactValue::Int(600))], 10_000_000, 0, 599)));
    assert!(ok(z2_tx("age", vec![Arg::V(ArtifactValue::Int(600))], 0, 0, 600)), "the entry score plays no part in the script");
    // CLTV: the lock time (the node then requires lock time <= now).
    assert!(ok(z2_tx("after", vec![Arg::V(ArtifactValue::Int(10_000_000))], 5, 10_000_000, 0)));
    assert!(!ok(z2_tx("after", vec![Arg::V(ArtifactValue::Int(10_000_000))], 5, 9_999_999, 0)));
}

/// The burn output of `KpfLock.dispute` (DESIGN-ORACLE Z7): P2SH of the one-byte redeem script `OP_RETURN` (0x6a). It is
/// an ordinary P2SH script public key (the standard script class a node relays), and no spend of it can succeed: the
/// redeem script hashing to it fails as soon as it runs, and finding another script with the same blake2b hash is a
/// preimage attack. Its hash is the constant `BURN` of `KpfLock`.
#[test]
fn z7_the_burn_output_is_standard_p2sh_and_unspendable() {
    use kaspa_txscript::script_class::ScriptClass;
    let spk = kaspa_txscript::pay_to_script_hash_script(&[0x6a]);
    let h: Vec<u8> = spk.script()[2..34].to_vec();
    println!("BURN {}", h.iter().map(|b| format!("{b:02x}")).collect::<String>());
    assert_eq!(ScriptClass::from_script(&spk), ScriptClass::ScriptHash);
    assert_eq!(h, kpf_protocol::lock::burn_hash().to_vec(), "the library's burn hash");
    // A spend that reveals the redeem script fails.
    let f = keypair(8);
    let ins = [In { outpoint: outpoint(1), utxo: UtxoEntry::new(100_000_000, spk.clone(), 0, false, None), spend: Spend::P2pk(f) }];
    let (mut tx, e) = build(&ins, vec![out(99_000_000, p2pk_spk(&keypair(9)), None)]);
    tx.inputs[0].signature_script = push(&[0x6a]);
    assert!(run(&tx, &e).expect("bindings")[0].is_err(), "the burn output is unspendable");
    // Nor with an empty redeem script, or another one.
    for rs in [vec![], vec![0x51], vec![0x6a, 0x51]] {
        tx.inputs[0].signature_script = push(&rs);
        assert!(run(&tx, &e).expect("bindings")[0].is_err());
    }
}
