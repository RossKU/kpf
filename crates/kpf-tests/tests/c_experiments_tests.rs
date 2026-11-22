//! Experiments of the committee price source (docs/DESIGN-COMMITTEE.md), in the rusty-kaspa v2.1.0 script engine.
//!
//! C1, the attested signed price. One input (`contracts/probes/C1Attest.sil`) holds 15 members of (key, weight) and accepts a
//! price p for a signed price t0 of a market only if members holding at least the quorum of the total weight each signed a price
//! of their own for that time (a data signature over blake2b("kpf-tick-1" || committee id || market || t0 || price_i))
//! and p is a weighted median of those prices. The numbers are printed by `c1_*` and asserted where a decision rests
//! on them. First measured with 16 members (a 3.8 kB script; 11 signed members 1,143,522 script units, 16 members
//! 1,658,993; stack 29); a node admits 15 signature operations in an input's script and counts these (live run 3), so
//! the probe and the family have 15 members. The median rule is the probe's only: review C1 broke it (C1-F1).

mod common;

use common::*;
use kaspa_consensus_core::tx::{Transaction, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::state::sm8;
use secp256k1::{Message, Secp256k1};
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

const N: usize = 15;
const MARKET: [u8; 32] = [0x4d; 32];
const FROM: i64 = 1_000;
const UNTIL: i64 = 4_000;
const VALUE: u64 = 20_000_000;

fn rid() -> Hash {
    Hash::from_bytes([0x52; 32])
}

fn member_kp(i: usize) -> secp256k1::Keypair {
    keypair(100 + i as u8)
}

fn blake2b(data: &[u8]) -> [u8; 32] {
    // P2SH's hash is blake2b-256 of the redeem script: the function the contracts' `blake2b` computes.
    kaspa_txscript::pay_to_script_hash_script(data).script()[2..34].try_into().expect("P2SH script")
}

fn price_digest(t0: i64, price: i64) -> [u8; 32] {
    let mut m = b"kpf-tick-1".to_vec();
    m.extend_from_slice(&rid().as_bytes());
    m.extend_from_slice(&MARKET);
    m.extend_from_slice(&sm8(t0));
    m.extend_from_slice(&sm8(price));
    blake2b(&m)
}

fn members_blob(weights: &[i64; N]) -> Vec<u8> {
    let mut b = vec![];
    for (i, w) in weights.iter().enumerate() {
        b.extend_from_slice(&member_kp(i).x_only_public_key().0.serialize());
        b.extend_from_slice(&sm8(*w));
    }
    b
}

fn epoch_table(weights: &[i64; N], quorum: i64) -> SilAbiArtifact {
    let total: i64 = weights.iter().sum();
    compile(
        "probes/C1Attest.sil",
        &[ArtifactValue::Bytes(members_blob(weights)), int(total), int(FROM), int(UNTIL), int(quorum)],
    )
}

/// One signed price per member: `Some(price)` signs, `None` leaves the member out.
fn attest_args(t0: i64, p: i64, votes: &[Option<i64>; N], forge: Option<usize>) -> Vec<ArtifactValue> {
    let mut bits = 0i64;
    let mut prices = vec![];
    let mut sigs = vec![];
    for (i, v) in votes.iter().enumerate() {
        match v {
            Some(pi) => {
                bits |= 1 << i;
                prices.extend_from_slice(&sm8(*pi));
                let signer = if forge == Some(i) { keypair(99) } else { member_kp(i) };
                let s = Secp256k1::new().sign_schnorr(&Message::from_digest(price_digest(t0, *pi)), &signer);
                sigs.extend_from_slice(s.as_ref());
            }
            None => {
                prices.extend_from_slice(&sm8(0));
                sigs.extend_from_slice(&[0u8; 64]);
            }
        }
    }
    let b = |v: Vec<u8>| ArtifactValue::Bytes(v);
    vec![b(MARKET.to_vec()), b(sm8(t0).to_vec()), b(sm8(p).to_vec()), b(sm8(bits).to_vec()), b(prices), b(sigs)]
}

fn attest_tx(art: &SilAbiArtifact, args: Vec<ArtifactValue>) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [
        contract_in(outpoint(1), VALUE, Some(rid()), art, "attest", args),
        p2pk_in(outpoint(2), 100_000_000, None, &keypair(60)),
    ];
    build(&ins, vec![out(VALUE, p2sh(art), bind(0, rid())), out(99_000_000, p2pk_spk(&keypair(60)), None)])
}

fn weights_top3() -> [i64; N] {
    // 30, 22 and 15 of 100: the three largest members hold exactly 67 %.
    let mut w = [1i64; N];
    w[0] = 30;
    w[1] = 22;
    w[2] = 15;
    w[3] = 12;
    w[4] = 8;
    w[5] = 2;
    // 30 + 22 + 15 + 12 + 8 + 2 + 9 x 1 = 98; one more unit on members 6 and 14.
    w[6] = 2;
    w[14] = 2;
    w.map(|x| x * 1_000_000_000_000)
}

fn votes(list: &[(usize, i64)]) -> [Option<i64>; N] {
    let mut v = [None; N];
    for (i, p) in list {
        v[*i] = Some(*p);
    }
    v
}

#[test]
fn c1_a_quorum_of_signed_prices_accepts_their_weighted_median() {
    let w = weights_top3();
    assert_eq!(w.iter().sum::<i64>(), 100_000_000_000_000);
    let art = epoch_table(&w, 6_700);
    // Members 0, 1, 2 (67 %) sign 80,010, 80,000 and 79,990: the weighted median is 80,000 (member 0's 30 at or above
    // 80,010 is less than half of 67; with member 1 it is 52).
    let v = votes(&[(0, 80_010), (1, 80_000), (2, 79_990)]);
    let (tx, e) = attest_tx(&art, attest_args(2_000, 80_000, &v, None));
    assert_accepted(&tx, &e);
    // Neither signed neighbour is a median.
    for p in [80_010, 79_990, 80_001, 79_999] {
        let (tx, e) = attest_tx(&art, attest_args(2_000, p, &v, None));
        assert_input_refused(&tx, &e, 0);
    }
}

#[test]
fn c1_below_the_quorum_nothing_is_accepted() {
    let art = epoch_table(&weights_top3(), 6_700);
    // Members 0, 3 and 4 hold 50 %; members 0, 1 and 3 hold 64 %.
    for set in [[0usize, 3, 4], [0, 1, 3]] {
        let v = votes(&[(set[0], 80_000), (set[1], 80_000), (set[2], 80_000)]);
        let (tx, e) = attest_tx(&art, attest_args(2_000, 80_000, &v, None));
        assert_input_refused(&tx, &e, 0);
    }
    // A quorum of one unit more than the three largest members hold refuses them.
    let art = epoch_table(&weights_top3(), 6_701);
    let v = votes(&[(0, 80_000), (1, 80_000), (2, 80_000)]);
    let (tx, e) = attest_tx(&art, attest_args(2_000, 80_000, &v, None));
    assert_input_refused(&tx, &e, 0);
}

#[test]
fn c1_a_forged_signature_a_foreign_price_and_a_price_outside_the_window_are_refused() {
    let art = epoch_table(&weights_top3(), 6_700);
    let v = votes(&[(0, 80_000), (1, 80_000), (2, 80_000)]);
    // Member 1's signature made by another key.
    let (tx, e) = attest_tx(&art, attest_args(2_000, 80_000, &v, Some(1)));
    assert_input_fails(&tx, &e, 0);
    // Signatures of signed price 2,000 presented for signed price 2,001.
    let mut args = attest_args(2_000, 80_000, &v, None);
    args[1] = ArtifactValue::Bytes(sm8(2_001).to_vec());
    let (tx, e) = attest_tx(&art, args);
    assert_input_fails(&tx, &e, 0);
    // Signatures of this market presented for another.
    let mut args = attest_args(2_000, 80_000, &v, None);
    args[0] = ArtifactValue::Bytes([0x4e; 32].to_vec());
    let (tx, e) = attest_tx(&art, args);
    assert_input_fails(&tx, &e, 0);
    // The window is [from, until).
    for (t0, ok) in [(FROM - 1, false), (FROM, true), (UNTIL - 1, true), (UNTIL, false)] {
        let (tx, e) = attest_tx(&art, attest_args(t0, 80_000, &v, None));
        if ok {
            assert_accepted(&tx, &e);
        } else {
            assert_input_refused(&tx, &e, 0);
        }
    }
    // A bit beyond the fifteenth member.
    let mut args = attest_args(2_000, 80_000, &v, None);
    args[3] = ArtifactValue::Bytes(sm8((1 << 15) | 7).to_vec());
    let (tx, e) = attest_tx(&art, args);
    assert_input_refused(&tx, &e, 0);
}

#[test]
fn c1_cost_of_an_attested_price() {
    let equal = [1_000_000_000_000i64; N];
    let art = epoch_table(&equal, 6_700);
    println!("C1 script: {} bytes", bytecode(&art).len());
    for k in [11usize, 15] {
        let list: Vec<(usize, i64)> = (0..k).map(|i| (i, 80_000)).collect();
        let (tx, e) = attest_tx(&art, attest_args(2_000, 80_000, &votes(&list), None));
        let m = &run_metered(&tx, &e)[0];
        m.result.clone().expect("accepted");
        println!(
            "C1 {k} of 15 members: {} script units, max stack {}, signature script {} bytes",
            m.script_units,
            m.max_stack,
            tx.inputs[0].signature_script.len()
        );
        // One signature operation per signed member and little besides.
        assert!(m.script_units >= k as u64 * kpf_protocol::tx::SIGOP_SCRIPT_UNITS);
        assert!(m.script_units < k as u64 * kpf_protocol::tx::SIGOP_SCRIPT_UNITS + 200_000);
    }
    let top = epoch_table(&weights_top3(), 6_700);
    let (tx, e) = attest_tx(&top, attest_args(2_000, 80_000, &votes(&[(0, 80_000), (1, 80_000), (2, 80_000)]), None));
    let m = &run_metered(&tx, &e)[0];
    m.result.clone().expect("accepted");
    println!("C1 3 of 15 members (67 % of the weight): {} script units, max stack {}", m.script_units, m.max_stack);
}
