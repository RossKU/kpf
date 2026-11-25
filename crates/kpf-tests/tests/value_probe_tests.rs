//! Research probe R3 (`docs/RESEARCH-UNIFIED-MARGIN.md`): an account of many legs valued by one small script
//! (`contracts/probes/R3Value.sil`) that the scripts of operations would rely on by being spent beside it. The
//! question is what a leg costs in bytes and whether the stack grows with the legs (the engine holds 244 items, and
//! the scripts that value two legs in line stand at that limit).

mod common;

use common::*;
use silverscript_abi::ArtifactValue;

const KAS1: u64 = 100_000_000;
const SCALE: i64 = 100_000_000;

#[derive(Clone, Copy)]
struct Leg {
    size: i64,
    entry: i64,
    price: i64,
    long: bool,
}

fn q(size: i64, x: i64) -> i64 {
    ((size as i128 * x as i128) / SCALE as i128) as i64
}

fn bytes(legs: &[Leg]) -> Vec<u8> {
    legs.iter().flat_map(|l| [l.size.to_le_bytes().to_vec(), SCALE.to_le_bytes().to_vec(), l.entry.to_le_bytes().to_vec(), l.price.to_le_bytes().to_vec(), vec![if l.long { 1 } else { 2 }]].concat()).collect()
}

fn blake2b(data: &[u8]) -> [u8; 32] {
    kaspa_txscript::pay_to_script_hash_script(data).script()[2..34].try_into().expect("P2SH script")
}

/// (A's gain, the maintenance at 5 %) as the model counts them.
fn valued(legs: &[Leg]) -> (i64, i64) {
    let gain: i64 = legs.iter().map(|l| (q(l.size, l.price) - q(l.size, l.entry)) * if l.long { 1 } else { -1 }).sum();
    let worth: i64 = legs.iter().map(|l| q(l.size, l.price)).sum();
    (gain, (worth / 10_000) * 500 + ((worth % 10_000) * 500 + 9_999) / 10_000)
}

fn args(legs: &[Leg], gain: i64, maint: i64) -> Vec<ArtifactValue> {
    let b = bytes(legs);
    let h = blake2b(&b);
    vec![ArtifactValue::Bytes(b), int(legs.len() as i64), int(500), int(gain), int(maint), ArtifactValue::Bytes(h.to_vec())]
}

fn run_value(a: Vec<ArtifactValue>) -> Metered {
    let art = compile("probes/R3Value.sil", &[int(1)]);
    let (tx, entries) = build(&[contract_in(outpoint(1), 10 * KAS1, None, &art, "value", a)], vec![out(9 * KAS1, p2pk_spk(&keypair(3)), None)]);
    run_metered(&tx, &entries).remove(0)
}

fn legs(n: usize) -> Vec<Leg> {
    (0..n).map(|i| Leg { size: (3 + i as i64) * SCALE + 12_345 * i as i64, entry: (10 + i as i64) * SCALE, price: (10 + i as i64) * SCALE + (i as i64 % 3 - 1) * 37_000_000 + 11, long: i % 2 == 0 }).collect()
}

#[test]
fn r3_an_account_of_many_legs_is_valued_by_one_small_script() {
    let art = compile("probes/R3Value.sil", &[int(1)]);
    let script = bytecode(&art).len();
    println!("R3Value: {script} bytes of script (a loop of at most 16 legs)");
    // The same script written for fewer legs at most: a transaction carries the one that fits its account.
    for k in [2usize, 4, 8] {
        let small = compile(&format!("probes/R3Value{k}.sil"), &[int(1)]);
        let l = legs(k);
        let (gain, maint) = valued(&l);
        let (tx, entries) = build(&[contract_in(outpoint(1), 10 * KAS1, None, &small, "value", args(&l, gain, maint))], vec![out(9 * KAS1, p2pk_spk(&keypair(3)), None)]);
        let m = run_metered(&tx, &entries).remove(0);
        assert!(m.result.is_ok(), "{k} legs at most: {:?}", m.result);
        println!("R3Value{k}: {} bytes of script; with {k} legs a signature script of {} bytes, a stack of {}", bytecode(&small).len(), tx.inputs[0].signature_script.len(), m.max_stack);
    }
    let mut last = 0usize;
    for n in [1usize, 2, 4, 8, 16] {
        let l = legs(n);
        let (gain, maint) = valued(&l);
        let a = args(&l, gain, maint);
        let art = compile("probes/R3Value.sil", &[int(1)]);
        let (tx, entries) = build(&[contract_in(outpoint(1), 10 * KAS1, None, &art, "value", a.clone())], vec![out(9 * KAS1, p2pk_spk(&keypair(3)), None)]);
        let sig = tx.inputs[0].signature_script.len();
        let m = run_metered(&tx, &entries).remove(0);
        assert!(m.result.is_ok(), "{n} legs: {:?}", m.result);
        println!("R3 {n:>2} legs: a signature script of {sig} bytes (the script among them), {} bytes of arguments, a stack of {} at most, {} script units", sig - script, m.max_stack, m.script_units);
        assert!(m.max_stack < 60, "the stack does not grow with the legs: {}", m.max_stack);
        last = m.max_stack.max(last);
        // What is not so is not held: a sompi of gain, a sompi of maintenance, a leg changed under the same hash.
        assert!(run_value(args(&l, gain + 1, maint)).result.is_err());
        assert!(run_value(args(&l, gain, maint - 1)).result.is_err());
        let mut forged = a.clone();
        if let ArtifactValue::Bytes(b) = &mut forged[0] {
            b[24] ^= 1;
        }
        assert!(run_value(forged).result.is_err(), "a price changed under the legs' hash");
    }
    assert!(last > 0);
}
