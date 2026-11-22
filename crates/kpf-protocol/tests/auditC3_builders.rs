//! Review C3 of the committee family (docs/audit/REVIEW-C3.md): the smallest objects the contracts allow, built by the
//! tool's builders on the pinned artifacts and passed through `verify::verify` (the storage mass of an output is about
//! 10^12 / value grams, four times that under a covenant id; a block holds 500,000).
//!
//! `c3_bounded_*`: measured, and what the measure means for a user.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::*;
use kpf_protocol::committee::*;
use kpf_protocol::committee_build::*;
use kpf_protocol::oracle::Makers;
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::state::{KIND_CLOSE, LONG, SHORT};
use kpf_protocol::tx::{KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};

const KAS1: i64 = 100_000_000;
const FEED: i64 = 1_000_000_000;

fn kp(seed: u8) -> Keypair {
    Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&[seed; 32]).expect("a secret key"))
}
fn xk(k: &Keypair) -> [u8; 32] {
    k.x_only_public_key().0.serialize()
}
fn op(n: u8) -> TransactionOutpoint {
    TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }
}
fn fund(key: [u8; 32], n: u8) -> KeyUtxo {
    KeyUtxo { outpoint: op(n), amount: 1_000 * KAS1 as u64, daa: 0, key }
}
/// Signs and verifies; the storage mass and whether every rule of `verify` passed (the failures otherwise).
fn measure(what: &str, b: &OBuilt, keys: &KeySet) -> (u64, bool, Option<OSigned>) {
    match b.sign(keys) {
        Ok(sg) => {
            let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
            println!("{what}: storage mass {} grams, fee {} sompi, {}", sg.mass.storage, sg.fee, if v.is_ok() { "verified".to_string() } else { format!("REFUSED: {:?}", v.errors()) });
            (sg.mass.storage, v.is_ok(), Some(sg))
        }
        Err(e) => {
            println!("{what}: the builder refuses to sign: {e}");
            (u64::MAX, false, None)
        }
    }
}
fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
    let o = &sg.tx.outputs[i];
    OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
}

fn order(m: &CMarket, side: i64, owner: [u8; 32], size: i64, rate: i64, extra: i64, n: u8) -> OLive<COpen> {
    let o = COpen { m: *m, side, owner, size_left: size, limit: FEED, my_rate: rate, min_counter_rate: 0, min_fill: size, tip: 0, active_from: 0, expiry_daa: 9_000_000 };
    OLive { value: (o.spend(size) + extra) as u64, id: Some(Hash::from_bytes([n; 32])), op: op(n), daa: 6_000, s: o }
}

#[test]
fn c3_bounded_the_smallest_pair_order_part_and_exit_are_mined_and_two_bare_refunds_together_are_not() {
    let rid = Hash::from_bytes([0x52; 32]);
    let m = CMarket::testnet(rid.as_bytes(), [0xfe; 32]);
    let (long, short, taker) = (kp(41), kp(42), kp(77));
    let mut keys = KeySet::new();
    for k in [long, short, taker] {
        keys.add_keypair(k);
    }
    let me = xk(&taker);
    // The smallest order the contracts take: 0.2 KAS of margin (0.2 unit at 1 KAS a unit). Its counterparty posts the
    // maintenance (0.5 KAS a unit). The pair holds 0.3 + 0.2 KAS.
    let size = 20_000_000;
    let lo = order(&m, LONG, xk(&long), size, 100_000_000, KAS1, 11);
    assert!(lo.s.part_ok(size) && !lo.s.part_ok(size - 1));
    let (b, p) = open_taker(&lo, size, FEED, 50_000_000, me, 6_054_000, 6_100, &[fund(me, 60)], &me, MIN_FEE_RATE).expect("the smallest open");
    assert_eq!(p.floor(), 50_000_000);
    let (mass, ok, sg) = measure("the smallest pair, the order's refund 1 KAS", &b, &keys);
    assert!(ok && mass < 500_000);
    let pair = nth(&sg.unwrap(), 1, p.clone(), 6_100);

    // The same order escrowed to the sompi (`c-order --escrow`): its refund is nothing, which the builder writes as
    // the least viable output, 0.025 KAS, at the taker's cost: 400,000 grams of the block's 500,000 for that output.
    let bare = order(&m, LONG, xk(&long), size, 100_000_000, 0, 12);
    let (b, _) = open_taker(&bare, size, FEED, 50_000_000, me, 6_054_000, 6_100, &[fund(me, 61)], &me, MIN_FEE_RATE).expect("the open of a bare order");
    let (mass_bare, ok_bare, _) = measure("the smallest pair, the order's refund topped up to 0.025 KAS", &b, &keys);
    // Two such orders opened against each other: two refunds of 0.025 KAS in one transaction.
    let bare_short = order(&m, SHORT, xk(&short), size, 100_000_000, 0, 13);
    let two = open(&bare, &bare_short, size, FEED, 6_054_000, 6_100, &[fund(me, 62)], &me, MIN_FEE_RATE);
    let (mass_two, ok_two) = match &two {
        Ok((b, _)) => {
            let (mass, ok, _) = measure("two bare orders opened against each other", b, &keys);
            (mass, ok)
        }
        Err(e) => {
            println!("two bare orders opened against each other: the builder refuses: {e}");
            (u64::MAX, false)
        }
    };
    println!("bare order alone: {mass_bare} grams ({ok_bare}); two bare orders: {mass_two} grams ({ok_two})");
    assert!(!ok_two, "two refunds of the least viable size in one transaction exceed a block");
    // With the tool's default escrow (1 KAS above a full fill) both are mined.
    let so = order(&m, SHORT, xk(&short), size, 100_000_000, KAS1, 14);
    let (b, _) = open(&lo, &so, size, FEED, 6_054_000, 6_100, &[fund(me, 63)], &me, MIN_FEE_RATE).expect("the open of two default orders");
    assert!(measure("two default orders opened against each other", &b, &keys).1);

    // The smallest split of a pair: a close of a part that leaves each side 0.2 KAS at the entry. 100 units at 10 KAS,
    // bounds 9 and 12; the part is 0.2 unit (0.6 KAS and the carrier the closer funds).
    let big = CPair { m, size: 100 * m.p.scale, bl: 900_000_000, bs: 1_200_000_000, entry: FEED, long_key: xk(&long), short_key: xk(&short), expiry: 6_054_100, long_ask: 0, long_min: 0, long_tip: 0, short_bid: 0, short_min: 0, short_tip: 0, mk: Makers::default() };
    assert!(big.split_ok(size) && !big.split_ok(size - 1));
    let lp = OLive { value: big.floor() as u64, id: Some(Hash::from_bytes([0x50; 32])), op: op(10), daa: 6_100, s: big.clone() };
    let members = [kp(101), kp(102), kp(103)];
    let mut table = EMPTY_TABLE;
    for (i, w) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: w * KAS1 };
    }
    let rp = CommitteeParams::testnet();
    let epoch_table = OLive { s: EpochTable { members: table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(2), daa: 6_000 };
    let signed_prices: Vec<SignedPrice> = members.iter().map(|k| sign_price(k, &rid.as_bytes(), &m.id(), 6_500, FEED)).collect();
    let a = Attestation::collect(&epoch_table.s, &rid.as_bytes(), &m.id(), 6_500, &signed_prices).expect("a quorum");
    let (b, w) = propose(&lp, &epoch_table, &a, KIND_CLOSE, LONG, size, xk(&long), &[fund(xk(&long), 64)], &xk(&long), MIN_FEE_RATE).expect("the smallest close");
    let (mass, ok, sg) = measure("a close of the smallest part (pending part 0.8 KAS, epoch table 1 KAS, remainder)", &b, &keys);
    assert!(ok && mass < 500_000);
    let pending = nth(&sg.unwrap(), 0, w, 6_510);
    let (b, _) = finalize(&pending, &[fund(me, 65)], &me, MIN_FEE_RATE).expect("its settlement");
    assert!(measure("the settlement of the smallest close (the long 0.39 KAS, the short 0.41 KAS)", &b, &keys).1);
    // The smallest pair's own exits.
    let (b, _) = assign(&pair, LONG, size, FEED, 900_000_000, me, &[fund(me, 66)], &me, MIN_FEE_RATE).expect("an assign of the smallest pair");
    assert!(measure("an assign of the smallest pair (the leaver 0.2 KAS)", &b, &keys).1);
    let b = lapse_settle(&pair, &[fund(me, 67)], &me, MIN_FEE_RATE).expect("its lapse settle");
    assert!(measure("the lapse settle of the smallest pair (0.2 and 0.3 KAS)", &b, &keys).1);

    // A committee: a join of the least member (10 KAS), its leave (an exit of 10 KAS), a roll (four epoch tables of 1 KAS).
    let r0 = OLive { s: Committee::empty(rp, 3_000), value: COMMITTEE_CARRIER, id: Some(rid), op: op(1), daa: 3_000 };
    let mut r = r0.clone();
    for (i, k) in members.iter().enumerate() {
        let (b, next, _) = join(&r, i, xk(k), 10 * KAS1, &[fund(me, 70 + i as u8)], &me, MIN_FEE_RATE).expect("a join");
        let (_, ok, sg) = measure("a join of 10 KAS", &b, &keys);
        assert!(ok);
        r = nth(&sg.unwrap(), 0, next, 3_100 + 20 * i as u64);
    }
    let (b, _, _) = roll(&r, 1, &[fund(me, 80)], &me, MIN_FEE_RATE).expect("a roll");
    let (mass, ok, _) = measure("a roll (four epoch tables of 1 KAS)", &b, &keys);
    assert!(ok && mass > 100_000);
    let mut member_keys = KeySet::new();
    member_keys.add_keypair(members[0]);
    member_keys.add_keypair(taker);
    let (b, _, e) = leave(&r, 0, &[fund(me, 81)], &me, MIN_FEE_RATE).expect("a leave");
    let (_, ok, sg) = measure("a leave (an exit of 10 KAS)", &b, &member_keys);
    assert!(ok);
    let exit = nth(&sg.unwrap(), 1, e, 3_200);
    let b = withdraw(&exit, &[fund(me, 82)], &me, MIN_FEE_RATE).expect("the withdraw");
    assert!(measure("the withdraw of 10 KAS", &b, &keys).1);
}
