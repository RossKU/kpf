//! Arithmetic boundary vectors of DESIGN 5 (experiment X11 for this slice): priceMax = 10^14, scale = 10^9, the
//! largest size. Inside the bounds every path computes without overflow; above them the entry refuses with a
//! failed require (never an arithmetic error midway).
//!
//! Size bound used by both contracts: floor(size / scale) <= (2^63 - 1 - carrier) / priceMax - 1.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::{Transaction, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf::*;
use silverscript_abi::ArtifactValue;

const BIG_SCALE: i64 = 1_000_000_000;
const PAIR_ID: Hash = Hash::from_bytes([0x5b; 32]);

fn big_market() -> Market {
    let mut m = market();
    m.scale = BIG_SCALE;
    m.price_max = PRICE_MAX;
    m.min_size = BIG_SCALE;
    m
}

/// The largest whole-token count the contracts accept for this carrier and priceMax.
fn max_whole(m: &Market) -> i64 {
    (i64::MAX - m.carrier) / m.price_max - 1
}

fn max_size(m: &Market) -> i64 {
    max_whole(m) * m.scale + m.scale - 1
}

fn accept(tx: &Transaction, e: &[UtxoEntry]) {
    assert_accepted(tx, e);
    let ins: u128 = e.iter().map(|u| u.amount as u128).sum();
    let outs: u128 = tx.outputs.iter().map(|o| o.value as u128).sum();
    assert!(outs <= ins, "outputs {outs} > inputs {ins}");
}

#[test]
fn bound_values() {
    let m = big_market();
    assert_eq!(max_whole(&m), 92_232);
    let s = max_size(&m);
    // q(s, priceMax) + carrier fits, and the low product (n mod scale) * (r mod scale) stays below scale^2.
    let top = q(s, m.price_max, m.scale, m.scale - 1) as i128 + m.carrier as i128;
    assert!(top <= i64::MAX as i128);
    assert!(((BIG_SCALE - 1) as i128) * ((BIG_SCALE - 1) as i128) < i64::MAX as i128);
    // One whole token more and q(s, priceMax) alone no longer fits in 63 bits with the carrier.
    let s2 = s + 2 * m.scale;
    let v = (s2 / m.scale) as i128 * m.price_max as i128 + m.carrier as i128;
    assert!(v > i64::MAX as i128);
}

fn big_pair(size: i64, bl: i64, bs: i64, m: Market) -> Pair {
    let mut p = pair(&keypair(11), &keypair(12));
    (p.m, p.size, p.bl, p.bs, p.entry) = (m, size, bl, bs, bs);
    p
}

fn add_margin(p: &Pair, delta: i64, in_value: u64, out_value: u64) -> (Transaction, Vec<UtxoEntry>) {
    let mut n = p.clone();
    n.bl -= delta;
    let ins = [
        In {
            outpoint: outpoint(10),
            utxo: UtxoEntry::new(in_value, p2sh(&p.art()), 0, false, Some(PAIR_ID)),
            spend: Spend::Signed { art: p.art(), entry: "addMargin", args: vec![Arg::Sig(keypair(11)), Arg::V(ArtifactValue::Int(1)), Arg::V(ArtifactValue::Int(delta))] },
        },
        p2pk_in(outpoint(11), out_value - in_value + 1_000, None, &keypair(14)),
    ];
    build(&ins, vec![out(out_value, p2sh(&n.art()), bind(0, PAIR_ID))])
}

#[test]
fn pair_at_the_largest_size_and_price_bound() {
    let m = big_market();
    let s = max_size(&m);
    // bl = scale - 1, bs = priceMax; add delta = scale - 1 (largest r mod scale) to reach bl = 0.
    let p = big_pair(s, BIG_SCALE - 1, PRICE_MAX, m.clone());
    let in_v = p.floor() as u64;
    let d = BIG_SCALE - 1;
    let need = q(s, d, BIG_SCALE, BIG_SCALE - 1) as u64;
    let mut n = p.clone();
    n.bl = 0;
    let out_v = (n.floor() as u64).max(in_v + need);
    assert!(out_v <= i64::MAX as u64);
    let (tx, e) = add_margin(&p, d, in_v, out_v);
    accept(&tx, &e);
    // P8 at E = priceMax: the long takes the whole range of the largest pair.
    let p0 = big_pair(s, 0, PRICE_MAX, m);
    let v = p0.floor() as u64;
    let lp = q(s, PRICE_MAX, BIG_SCALE, 0) as u64;
    let ins = [
        In { outpoint: outpoint(10), utxo: UtxoEntry::new(v, p2sh(&p0.art()), 0, false, Some(PAIR_ID)), spend: Spend::Signed { art: p0.art(), entry: "idleSettle", args: vec![] } },
        p2pk_in(outpoint(11), 1_000_000, None, &keypair(14)),
    ];
    let (tx, e) = build_ext(&ins, vec![out(lp, p2pk_spk(&keypair(11)), None), out(0, p2pk_spk(&keypair(14)), None), out(v - lp, p2pk_spk(&keypair(12)), None)], 0, &[IDLE as u64]);
    accept(&tx, &e);
}

#[test]
fn pair_above_the_bounds_is_refused() {
    let m = big_market();
    let s = max_size(&m);
    // One more whole token.
    let p = big_pair(s + 1, 1_000, PRICE_MAX, m.clone());
    let (tx, e) = add_margin(&p, 1, 10_000_000_000, 20_000_000_000);
    assert_input_refused(&tx, &e, 0);
    // priceMax above 10^14.
    let mut m2 = m.clone();
    m2.price_max = PRICE_MAX + 1;
    let p = big_pair(BIG_SCALE, 1_000, PRICE_MAX, m2);
    let (tx, e) = add_margin(&p, 1, 10_000_000_000, 20_000_000_000);
    assert_input_refused(&tx, &e, 0);
    // scale above 10^9, and scale 0.
    for sc in [BIG_SCALE + 1, 0] {
        let mut m3 = m.clone();
        m3.scale = sc;
        let p = big_pair(BIG_SCALE, 1_000, PRICE_MAX, m3);
        let (tx, e) = add_margin(&p, 1, 10_000_000_000, 20_000_000_000);
        assert_input_refused(&tx, &e, 0);
    }
    // bs above priceMax, negative carrier.
    let p = big_pair(BIG_SCALE, 1_000, PRICE_MAX + 1, m.clone());
    let (tx, e) = add_margin(&p, 1, 10_000_000_000, 20_000_000_000);
    assert_input_refused(&tx, &e, 0);
    let mut m4 = m.clone();
    m4.carrier = -1;
    let p = big_pair(BIG_SCALE, 1_000, PRICE_MAX, m4);
    let (tx, e) = add_margin(&p, 1, 10_000_000_000, 20_000_000_000);
    assert_input_refused(&tx, &e, 0);
}

/// T1 at the bounds: the largest size, bl = 0 and bs = priceMax.
fn big_open(n: i64) -> (Transaction, Vec<UtxoEntry>) {
    let m = big_market();
    let x = PRICE_MAX / 2;
    let mut l = long_order(&keypair(21));
    let mut s = short_order(&keypair(22));
    for o in [&mut l, &mut s] {
        o.m = m.clone();
        o.size_left = n;
        o.min_fill = BIG_SCALE;
        o.tip = 0;
    }
    (l.limit, l.my_rate, l.min_counter_rate) = (x, x, 0);
    (s.limit, s.my_rate, s.min_counter_rate) = (x, PRICE_MAX - x, 0);
    let (l_id, s_id) = (Hash::from_bytes([0x61; 32]), Hash::from_bytes([0x62; 32]));
    let mut p = pair(&keypair(21), &keypair(22));
    (p.m, p.size, p.bl, p.bs, p.entry, p.tag_l, p.tag_s) = (m.clone(), n, 0, PRICE_MAX, x, l_id, s_id);
    let pv = p.floor() as u64;
    let (pre, suf) = parts(pair_base());
    let a = |ck: &secp256k1::Keypair, ct: Hash| {
        args(&[
            ArtifactValue::Int(2),
            ArtifactValue::Int(n),
            ArtifactValue::Int(x),
            ArtifactValue::Int(0),
            ArtifactValue::Int(PRICE_MAX),
            bytes(&xonly(ck)),
            bytes(&ct.as_bytes()),
            bytes(&pre),
            bytes(&suf),
        ])
    };
    let spend = |o: &Open| (q(n, o.my_rate, BIG_SCALE, 0) + o.m.carrier) as u64;
    // Each escrow exactly covers its spend; the funder covers the pair's rounding.
    let (vl, vs) = (spend(&l), spend(&s));
    let ins = [
        In { outpoint: outpoint(1), utxo: UtxoEntry::new(vl, p2sh(&l.art()), 0, false, Some(l_id)), spend: Spend::Signed { art: l.art(), entry: "fill", args: a(&keypair(22), s_id) } },
        In { outpoint: outpoint(2), utxo: UtxoEntry::new(vs, p2sh(&s.art()), 0, false, Some(s_id)), spend: Spend::Signed { art: s.art(), entry: "fill", args: a(&keypair(21), l_id) } },
        p2pk_in(outpoint(3), 1_000_000, None, &keypair(24)),
    ];
    let mut outs = vec![out(0, p2pk_spk(&keypair(21)), None), out(0, p2pk_spk(&keypair(22)), None), out(pv, p2sh(&p.art()), None)];
    outs[2].covenant = bind(2, genesis_id(outpoint(3), &outs, &[2]));
    build(&ins, outs)
}

#[test]
fn t1_at_the_largest_size() {
    let s = max_size(&big_market());
    let (tx, e) = big_open(s);
    accept(&tx, &e);
}

#[test]
fn t1_above_the_largest_size_is_refused() {
    let s = max_size(&big_market());
    let (tx, e) = big_open(s + 1);
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
}
