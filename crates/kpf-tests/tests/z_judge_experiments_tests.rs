//! Experiment Z13 of the stake judge (docs/DESIGN-ORACLE-IMPL.md, second slice): what a SIGHASH_SINGLE |
//! ANYONECANPAY signature (type 0x84) commits to. A staker joins a dispute by signing its own stake input and the
//! receipt output at the same index only, so that any aggregator can add the commitment to a deposit against
//! whatever the dispute UTXO is when it lands; the question is whether that signature also fixes the receipt's
//! covenant binding (which dispute it joins), or only its value and script.

mod common;

use common::*;
use kaspa_consensus_core::Hash;

const X: Hash = Hash::from_bytes([0x77; 32]);
const Y: Hash = Hash::from_bytes([0x78; 32]);
const SINGLE_ACP: u8 = 0x84;

/// In 0 a P2PK UTXO of key 5 carrying covenant id X, signed with 0x84 over the transaction as built; in 1 a funder;
/// in 1 carrying covenant id Y; out 0 a continuation of X, out 1 change. Returns whether input 0 passes after `edit` changed the signed transaction.
fn signed_then(edit: impl Fn(&mut kaspa_consensus_core::tx::Transaction)) -> bool {
    let k = keypair(5);
    let ins = [
        In { outpoint: outpoint(1), utxo: kaspa_consensus_core::tx::UtxoEntry::new(500_000_000, p2pk_spk(&k), 0, false, Some(X)), spend: Spend::P2pkType(k, SINGLE_ACP) },
        p2pk_in(outpoint(2), 500_000_000, Some(Y), &keypair(6)),
    ];
    let outs = vec![out(500_000_000, p2pk_spk(&k), bind(0, X)), out(400_000_000, p2pk_spk(&keypair(6)), None)];
    let (mut tx, e) = build(&ins, outs);
    let sig0 = tx.inputs[0].signature_script.clone();
    edit(&mut tx);
    tx.inputs[0].signature_script = sig0;
    tx.finalize();
    match run(&tx, &e) {
        Ok(v) => v[0].is_ok(),
        Err(_) => false,
    }
}

#[test]
fn z13_single_anyonecanpay_commits_to_its_own_output_and_its_covenant_binding() {
    assert!(signed_then(|_| {}), "as signed");
    // Other outputs and the lock time are not committed.
    assert!(signed_then(|t| t.outputs[1].value = 300_000_000), "another output changed");
    assert!(signed_then(|t| t.outputs.push(out(1_000_000, p2pk_spk(&keypair(7)), None))), "an output added");
    // Its own output is.
    assert!(!signed_then(|t| t.outputs[0].value = 499_999_999), "own output's value");
    assert!(!signed_then(|t| t.outputs[0].script_public_key = p2pk_spk(&keypair(7))), "own output's script");
    // The binding: is it part of the signed output?
    let unbound = signed_then(|t| t.outputs[0].covenant = None);
    println!("Z13: removing the covenant binding of the signed output {}", if unbound { "leaves the signature valid" } else { "invalidates the signature" });
    assert!(!unbound, "the binding is committed");
    // Bound to another existing id instead (a valid covenant context): the signature fails too.
    assert!(!signed_then(|t| t.outputs[0].covenant = bind(1, Y)), "own output moved to another covenant id");}
