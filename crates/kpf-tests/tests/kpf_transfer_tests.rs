//! KpfPair P3 transfer (DESIGN 3.4 b, 4.5 T2) and KpfOpen `take`.
//!
//! Shape: in 0 KpfPair, 1 entrant (a KpfOpen of the leaver's side, or a direct taker's P2PK input that funds the
//! transaction), 2 F (KpfOpen case). out 0 leaver payout, 1 entrant's order continuation-or-refund (or the direct
//! taker's change), 2 part, 3 remainder (partial fills), then F's change. Every accepted transaction is checked
//! for conservation.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::{Transaction, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf::paths::*;
use kpf::*;
use secp256k1::Keypair;

const PAIR_ID: Hash = Hash::from_bytes([0x5a; 32]);
const L_ID: Hash = Hash::from_bytes([0x41; 32]);
const S_ID: Hash = Hash::from_bytes([0x42; 32]);

struct K {
    long: Keypair,
    short: Keypair,
    other: Keypair,
    f: Keypair,
    entrant: Keypair,
    taker: Keypair,
}

fn keys() -> K {
    K { long: keypair(11), short: keypair(12), other: keypair(13), f: keypair(14), entrant: keypair(15), taker: keypair(16) }
}

/// The DESIGN 5 pair (1,000 tokens, bl 9, bs 12 KAS) with both sides listed: the long sells at >= 10.5 KAS in
/// parts >= 100 tokens, tip 0.01 KAS per token; the short buys back at <= 9.5 KAS, parts >= 100, tip 0.02.
fn listed(k: &K) -> Pair {
    let mut p = pair(&k.long, &k.short);
    (p.long_ask, p.long_min, p.long_tip) = (1_050_000_000, 100_000_000, 1_000_000);
    (p.short_bid, p.short_min, p.short_tip) = (950_000_000, 100_000_000, 2_000_000);
    p
}

fn live(p: &Pair) -> Live {
    Live { p: p.clone(), value: p.floor() as u64, id: PAIR_ID, op: outpoint(10) }
}

/// A long KpfOpen with rate 2 KAS (so bl' = x - 2 <= 9 at x <= 11), limit 11, accepting a short rate >= 1.5.
fn long_entrant(k: &K) -> LiveOpen {
    let mut o = long_order(&k.entrant);
    (o.limit, o.my_rate) = (1_100_000_000, 200_000_000);
    LiveOpen { value: o.escrow() as u64, o, id: L_ID, op: outpoint(1) }
}

/// A short KpfOpen with rate 3 KAS (bs' = x + 3 >= 12 at x >= 9), limit 9, accepting a long rate >= 0.5.
fn short_entrant(k: &K) -> LiveOpen {
    let mut o = short_order(&k.entrant);
    o.size_left = 2_000_000_000;
    (o.limit, o.my_rate) = (900_000_000, 300_000_000);
    LiveOpen { value: o.escrow() as u64, o, id: S_ID, op: outpoint(2) }
}

fn long_spec(k: &K, n: i64, x: i64) -> TransferSpec {
    TransferSpec { side: LONG, n, x, nb: x - 200_000_000, ek: xonly(&k.entrant), et: L_ID, k: 0 }
}

fn short_spec(k: &K, n: i64, x: i64) -> TransferSpec {
    TransferSpec { side: SHORT, n, x, nb: x + 300_000_000, ek: xonly(&k.entrant), et: S_ID, k: 0 }
}

/// A direct taker's spec: its own key, tag 0, the given bound.
fn direct_spec(k: &K, side: i64, n: i64, x: i64, nb: i64) -> TransferSpec {
    TransferSpec { side, n, x, nb, ek: xonly(&k.taker), et: ZERO, k: 0 }
}

fn accept(tx: &Transaction, e: &[UtxoEntry]) {
    assert_accepted(tx, e);
    let ins: u128 = e.iter().map(|u| u.amount as u128).sum();
    let outs: u128 = tx.outputs.iter().map(|o| o.value as u128).sum();
    assert!(outs <= ins, "outputs {outs} > inputs {ins}");
}

fn accept_d(d: &Draft) -> Transaction {
    let (tx, e) = d.build();
    accept(&tx, &e);
    tx
}

/// Input `idx` fails a require; every other listed input passes.
fn refused_only(d: &Draft, idx: usize, ok: &[usize]) {
    let (tx, e) = d.build();
    let r = run(&tx, &e).expect("bindings");
    for &o in ok {
        assert!(r[o].is_ok(), "input {o} should accept: {:?}", r[o]);
    }
    assert_input_refused(&tx, &e, idx);
}

fn pair_refused(d: &Draft) {
    let (tx, e) = d.build();
    assert_input_refused(&tx, &e, 0);
}

fn tx_d(l: &Live, t: &TransferSpec, ent: &Entrant, k: &K) -> Draft {
    transfer_draft(l, t, ent, &k.f, 3)
}

// ---------------------------------------------------------------- accepted

#[test]
fn p3_long_full_by_a_kpfopen() {
    let k = keys();
    let l = live(&listed(&k));
    let t = long_spec(&k, 1_000_000_000, 1_050_000_000);
    let d = tx_d(&l, &t, &Entrant::Open(long_entrant(&k)), &k);
    let tx = accept_d(&d);
    // The leaver gets q(n, 10.5 - 9) - q(n, 0.01) = 1,500 - 10 KAS.
    assert_eq!(tx.outputs[0].value, 149_000_000_000);
    assert_eq!(tx.outputs[0].script_public_key, p2pk_spk(&k.long));
    // The part is the only continuation: (1,000, bl' = 8.5, bs = 12, E = 10.5), the entrant's key and tag, its side
    // unlisted, the short (stayer) untouched including its listing; it inherits the old carrier.
    let part = &d.pairs[0].1;
    assert_eq!((part.size, part.bl, part.bs, part.entry), (1_000_000_000, 850_000_000, 1_200_000_000, 1_050_000_000));
    assert_eq!((part.long_key, part.tag_l, part.long_ask), (xonly(&k.entrant), L_ID, 0));
    assert_eq!((part.short_key, part.tag_s, part.short_bid), (l.p.short_key, l.p.tag_s, l.p.short_bid));
    assert_eq!(tx.outputs[2].value, part.floor() as u64);
    // The executor keeps both tips and the order's spare carrier.
    assert_eq!(tx.outputs[3].value + FEE - F_FUNDS, 2 * 1_000_000_000 + C as u64);
}

#[test]
fn p3_long_partial_by_a_kpfopen_either_ordinal() {
    let k = keys();
    let l = live(&listed(&k));
    let mut t = long_spec(&k, 400_000_000, 1_050_000_000);
    let d = tx_d(&l, &t, &Entrant::Open(long_entrant(&k)), &k);
    let tx = accept_d(&d);
    // The remainder (600 tokens) keeps the leaver, its listing and the old entry, and the old carrier.
    let rest = &d.pairs[1].1;
    assert_eq!((rest.size, rest.bl, rest.entry, rest.long_key, rest.long_ask), (600_000_000, l.p.bl, l.p.entry, l.p.long_key, l.p.long_ask));
    assert_eq!(tx.outputs[3].value as i64, l.value as i64 - l.p.range_of(400_000_000));
    // The new part's carrier comes from the order's escrow, not from the pair.
    assert_eq!(tx.outputs[2].value as i64, d.pairs[0].1.floor());
    // The order continues with 1,600 tokens left.
    assert_eq!(d.opens[0].1.size_left, 1_600_000_000);
    // The part as the second continuation (k = 1).
    t.k = 1;
    accept_d(&tx_d(&l, &t, &Entrant::Open(long_entrant(&k)), &k));
    // The wrong ordinal is refused.
    let mut d = tx_d(&l, &t, &Entrant::Open(long_entrant(&k)), &k);
    let mut a = d.args(0);
    a[6] = iv(0);
    d.set_args(0, a);
    pair_refused(&d);
}

#[test]
fn p3_short_full_and_partial_by_a_kpfopen() {
    let k = keys();
    let l = live(&listed(&k));
    for n in [1_000_000_000, 250_000_000] {
        let t = short_spec(&k, n, 950_000_000);
        let d = tx_d(&l, &t, &Entrant::Open(short_entrant(&k)), &k);
        let tx = accept_d(&d);
        // The short leaver: q(n, 12 - 9.5) - q(n, 0.02).
        assert_eq!(tx.outputs[0].value as i64, owed(n, 250_000_000, 2_000_000, SCALE));
        assert_eq!(tx.outputs[0].script_public_key, p2pk_spk(&k.short));
        let part = &d.pairs[0].1;
        assert_eq!((part.bl, part.bs, part.short_key, part.tag_s, part.short_bid), (l.p.bl, 1_250_000_000, xonly(&k.entrant), S_ID, 0));
        assert_eq!((part.long_key, part.tag_l, part.long_ask), (l.p.long_key, l.p.tag_l, l.p.long_ask));
    }
}

#[test]
fn p3_direct_taker_full_and_partial_both_sides() {
    let k = keys();
    let l = live(&listed(&k));
    for n in [1_000_000_000, 300_000_000] {
        accept_d(&tx_d(&l, &direct_spec(&k, LONG, n, 1_080_000_000, 880_000_000), &Entrant::Direct(k.taker), &k));
        accept_d(&tx_d(&l, &direct_spec(&k, SHORT, n, 930_000_000, 1_200_000_000), &Entrant::Direct(k.taker), &k));
    }
    // The no-downgrade bounds themselves are allowed: bl' = bl, bs' = bs.
    accept_d(&tx_d(&l, &direct_spec(&k, LONG, 500_000_000, 1_050_000_000, l.p.bl), &Entrant::Direct(k.taker), &k));
    accept_d(&tx_d(&l, &direct_spec(&k, SHORT, 500_000_000, 950_000_000, l.p.bs), &Entrant::Direct(k.taker), &k));
}

#[test]
fn p3_whole_pair_below_the_listing_minimum_is_allowed() {
    let k = keys();
    // 50 tokens, below longMin (100): taking all of it is allowed, a part of it is not.
    let p = listed(&k).with_size(50_000_000);
    let l = live(&p);
    accept_d(&tx_d(&l, &direct_spec(&k, LONG, 50_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.taker), &k));
    let p = listed(&k).with_size(150_000_000);
    let l = live(&p);
    pair_refused(&tx_d(&l, &direct_spec(&k, LONG, 50_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.taker), &k));
}

#[test]
fn p3_dust_payout_stays_in_the_remainder() {
    let k = keys();
    let mut p = listed(&k);
    (p.long_ask, p.long_tip) = (1, 0);
    let l = live(&p);
    // At x = bl + 0.00001 KAS the leaver is owed q(400, 1,000 sompi) = 400,000 sompi, below dust: no payout, and
    // the remainder holds it.
    let t = direct_spec(&k, LONG, 400_000_000, p.bl + 1_000, p.bl);
    let d = tx_d(&l, &t, &Entrant::Direct(k.taker), &k);
    let tx = accept_d(&d);
    assert_eq!(tx.outputs[0].value, 0);
    let keep = l.value as i64 - p.range_of(400_000_000) + 400_000;
    assert_eq!(tx.outputs[3].value as i64, keep);
    let mut d2 = d.clone();
    d2.outs[3].value -= 1;
    pair_refused(&d2);
}

// ---------------------------------------------------------------- listing rules

#[test]
fn p3_price_outside_the_listing_is_refused() {
    let k = keys();
    let l = live(&listed(&k));
    pair_refused(&tx_d(&l, &direct_spec(&k, LONG, 500_000_000, 1_049_999_999, 880_000_000), &Entrant::Direct(k.taker), &k));
    pair_refused(&tx_d(&l, &direct_spec(&k, SHORT, 500_000_000, 950_000_001, 1_200_000_000), &Entrant::Direct(k.taker), &k));
    // At the limits themselves both fill.
    accept_d(&tx_d(&l, &direct_spec(&k, LONG, 500_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.taker), &k));
    accept_d(&tx_d(&l, &direct_spec(&k, SHORT, 500_000_000, 950_000_000, 1_200_000_000), &Entrant::Direct(k.taker), &k));
    // Above priceMax.
    let mut p = listed(&k);
    p.m.price_max = 2_000_000_000;
    let l = live(&p);
    pair_refused(&tx_d(&l, &direct_spec(&k, LONG, 500_000_000, 2_000_000_001, 880_000_000), &Entrant::Direct(k.taker), &k));
}

#[test]
fn p3_unlisted_side_is_refused() {
    let k = keys();
    let mut p = listed(&k);
    (p.long_ask, p.short_bid) = (0, 0);
    let l = live(&p);
    pair_refused(&tx_d(&l, &direct_spec(&k, LONG, 500_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.taker), &k));
    pair_refused(&tx_d(&l, &direct_spec(&k, SHORT, 500_000_000, 950_000_000, 1_200_000_000), &Entrant::Direct(k.taker), &k));
    // A side code other than 1 or 2.
    let l = live(&listed(&k));
    pair_refused(&tx_d(&l, &direct_spec(&k, 3, 500_000_000, 950_000_000, 1_200_000_000), &Entrant::Direct(k.taker), &k));
}

#[test]
fn p3_part_and_remainder_sizes() {
    let k = keys();
    let l = live(&listed(&k));
    let go = |n: i64| tx_d(&l, &direct_spec(&k, LONG, n, 1_050_000_000, 880_000_000), &Entrant::Direct(k.taker), &k);
    // Below the listing minimum (100 tokens).
    pair_refused(&go(99_999_999));
    accept_d(&go(100_000_000));
    // A remainder below minSize (1 token): 999.5 tokens of 1,000.
    pair_refused(&go(999_500_000));
    accept_d(&go(999_000_000));
    // More than the pair, zero, negative.
    for n in [1_000_000_001, 0, -1] {
        let mut t = direct_spec(&k, LONG, 500_000_000, 1_050_000_000, 880_000_000);
        t.n = n;
        let mut d = go(500_000_000);
        let mut a = d.args(0);
        a[1] = iv(t.n);
        d.set_args(0, a);
        pair_refused(&d);
    }
    // A partial fill with only the part, or only one continuation of the id.
    let mut d = go(500_000_000);
    d.outs.remove(3);
    d.pairs.pop();
    pair_refused(&d);
}

// ---------------------------------------------------------------- the leaver's payout

#[test]
fn p3_leaver_paid_short_or_tip_above_its_rate_is_refused() {
    let k = keys();
    let l = live(&listed(&k));
    for (side, x, nb, tip) in [(LONG, 1_050_000_000, 880_000_000, 1_000_000), (SHORT, 950_000_000, 1_200_000_000, 2_000_000)] {
        for n in [1_000_000_000, 400_000_000] {
            let t = direct_spec(&k, side, n, x, nb);
            let mut d = tx_d(&l, &t, &Entrant::Direct(k.taker), &k);
            d.outs[0].value -= 1;
            pair_refused(&d);
            // The tip taken at the listed rate + 1 sompi per token.
            let r = if side == LONG { x - l.p.bl } else { l.p.bs - x };
            let mut d = tx_d(&l, &t, &Entrant::Direct(k.taker), &k);
            d.outs[0].value = (q(n, r, SCALE, 0) - q(n, tip + 1, SCALE, 0)) as u64;
            pair_refused(&d);
        }
    }
    // Paid to another key.
    let mut d = tx_d(&l, &direct_spec(&k, LONG, 500_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.taker), &k);
    d.outs[0].script_public_key = p2pk_spk(&k.other);
    pair_refused(&d);
    // The short's payout sent to the long.
    let mut d = tx_d(&l, &direct_spec(&k, SHORT, 500_000_000, 950_000_000, 1_200_000_000), &Entrant::Direct(k.taker), &k);
    d.outs[0].script_public_key = p2pk_spk(&k.long);
    pair_refused(&d);
}

// ---------------------------------------------------------------- the stayer

#[test]
fn p3_downgrade_by_one_is_refused() {
    let k = keys();
    let l = live(&listed(&k));
    // Direct taker: bl' = bl + 1 (the staying short's counterparty bankrupts earlier), bs' = bs - 1.
    pair_refused(&tx_d(&l, &direct_spec(&k, LONG, 500_000_000, 1_050_000_000, l.p.bl + 1), &Entrant::Direct(k.taker), &k));
    pair_refused(&tx_d(&l, &direct_spec(&k, SHORT, 500_000_000, 950_000_000, l.p.bs - 1), &Entrant::Direct(k.taker), &k));
    // KpfOpen taker whose own rate gives bl' = bl + 1: the order is satisfied, the pair refuses.
    let mut lo = long_entrant(&k);
    lo.o.my_rate = 1_050_000_000 - l.p.bl - 1;
    lo.value = lo.o.escrow() as u64;
    let mut t = long_spec(&k, 500_000_000, 1_050_000_000);
    t.nb = 1_050_000_000 - lo.o.my_rate;
    refused_only(&tx_d(&l, &t, &Entrant::Open(lo), &k), 0, &[1]);
    // Short side by a KpfOpen: bs' = bs - 1.
    let mut so = short_entrant(&k);
    so.o.my_rate = l.p.bs - 950_000_000 - 1;
    so.value = so.o.escrow() as u64;
    let mut t = short_spec(&k, 500_000_000, 950_000_000);
    t.nb = 950_000_000 + so.o.my_rate;
    refused_only(&tx_d(&l, &t, &Entrant::Open(so), &k), 0, &[1]);
}

#[test]
fn p3_stayer_remainder_and_market_are_untouched() {
    let k = keys();
    let l = live(&listed(&k));
    let t = direct_spec(&k, LONG, 400_000_000, 1_050_000_000, 880_000_000);
    let base = tx_d(&l, &t, &Entrant::Direct(k.taker), &k);
    let part = base.pairs[0].1.clone();
    let rest = base.pairs[1].1.clone();
    let part_mut: Vec<(&str, Box<dyn Fn(&mut Pair)>)> = vec![
        ("stayer key", Box::new(|p: &mut Pair| p.short_key = xonly(&keypair(13)))),
        ("stayer tag", Box::new(|p: &mut Pair| p.tag_s = Hash::from_bytes([9; 32]))),
        ("stayer bound", Box::new(|p: &mut Pair| p.bs -= 1)),
        ("stayer listing", Box::new(|p: &mut Pair| p.short_bid += 1)),
        ("entrant key", Box::new(|p: &mut Pair| p.long_key = xonly(&keypair(13)))),
        ("entrant tag", Box::new(|p: &mut Pair| p.tag_l = L_ID)),
        ("entrant bound", Box::new(|p: &mut Pair| p.bl -= 1)),
        ("entry", Box::new(|p: &mut Pair| p.entry += 1)),
        ("size", Box::new(|p: &mut Pair| p.size -= 1)),
        ("carrier", Box::new(|p: &mut Pair| p.m.carrier += 1)),
        ("token", Box::new(|p: &mut Pair| p.m.token_cov_id = Hash::from_bytes([7; 32]))),
        ("idleDaa", Box::new(|p: &mut Pair| p.m.idle_daa = 0)),
    ];
    for (name, f) in part_mut {
        let mut p = part.clone();
        f(&mut p);
        let mut d = base.clone();
        d.outs[2].value += 1_000_000_000; // never short of value
        d.set_pair(2, &p);
        let (tx, e) = d.build();
        assert!(run(&tx, &e).unwrap()[0].is_err(), "part: {name} accepted");
    }
    let rest_mut: Vec<(&str, Box<dyn Fn(&mut Pair)>)> = vec![
        ("leaver key", Box::new(|p: &mut Pair| p.long_key = xonly(&keypair(13)))),
        ("leaver listing", Box::new(|p: &mut Pair| p.long_ask = 0)),
        ("entry", Box::new(|p: &mut Pair| p.entry = 1_050_000_000)),
        ("bound", Box::new(|p: &mut Pair| p.bl -= 1)),
        ("tag", Box::new(|p: &mut Pair| p.tag_l = ZERO)),
        ("size", Box::new(|p: &mut Pair| p.size += 1)),
    ];
    for (name, f) in rest_mut {
        let mut p = rest.clone();
        f(&mut p);
        let mut d = base.clone();
        d.outs[3].value += 1_000_000_000;
        d.set_pair(3, &p);
        let (tx, e) = d.build();
        assert!(run(&tx, &e).unwrap()[0].is_err(), "remainder: {name} accepted");
    }
}

#[test]
fn p3_listing_is_not_replayed_on_the_part() {
    let k = keys();
    let l = live(&listed(&k));
    // Direct taker: the part keeps the leaver's long listing.
    let t = direct_spec(&k, LONG, 400_000_000, 1_050_000_000, 880_000_000);
    let mut d = tx_d(&l, &t, &Entrant::Direct(k.taker), &k);
    let mut p = d.pairs[0].1.clone();
    (p.long_ask, p.long_min, p.long_tip) = l.p.long_window();
    d.set_pair(2, &p);
    pair_refused(&d);
    // KpfOpen taker: both refuse (the order requires its side unlisted, too).
    let t = long_spec(&k, 400_000_000, 1_050_000_000);
    let mut d = tx_d(&l, &t, &Entrant::Open(long_entrant(&k)), &k);
    let mut p = d.pairs[0].1.clone();
    p.long_ask = 1_050_000_000;
    d.set_pair(2, &p);
    let (tx, e) = d.build();
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
    // The transferred part, now owned by the entrant, cannot be taken again at the old listing.
    let d = tx_d(&l, &direct_spec(&k, LONG, 400_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.taker), &k);
    let tx = accept_d(&d);
    let lp = live_pairs(&tx, &d);
    let again = tx_d(&lp[0], &direct_spec(&k, LONG, 400_000_000, 1_050_000_000, 800_000_000), &Entrant::Direct(k.other), &k);
    pair_refused(&again);
}

// ---------------------------------------------------------------- the KpfOpen entrant

#[test]
fn p3_kpfopen_checks_its_own_terms() {
    let k = keys();
    let l = live(&listed(&k));
    let ent = Entrant::Open(long_entrant(&k));
    // Its bound does not match its own rate (bl' one below x - myRate): the pair would accept, the order refuses.
    let mut t = long_spec(&k, 400_000_000, 1_050_000_000);
    t.nb -= 1;
    refused_only(&tx_d(&l, &t, &ent, &k), 1, &[0]);
    // Its key or tag replaced in the part.
    let t = long_spec(&k, 400_000_000, 1_050_000_000);
    let mut t2 = t.clone();
    t2.ek = xonly(&k.other);
    refused_only(&tx_d(&l, &t2, &ent, &k), 1, &[0]);
    let mut t2 = t.clone();
    t2.et = ZERO;
    refused_only(&tx_d(&l, &t2, &ent, &k), 1, &[0]);
    // Price above its limit (11 KAS).
    let mut p = listed(&k);
    p.bs = 1_400_000_000;
    let l2 = live(&p);
    let mut lo = long_entrant(&k);
    lo.o.my_rate = 250_000_000;
    lo.value = lo.o.escrow() as u64;
    let mut t = long_spec(&k, 400_000_000, 1_100_000_001);
    t.nb = t.x - lo.o.my_rate;
    refused_only(&tx_d(&l2, &t, &Entrant::Open(lo), &k), 1, &[0]);
    // The stayer's margin rate below its minCounterRate (1.5 KAS): bs - x = 1.4999.
    let mut p = listed(&k);
    p.bs = 1_050_000_000 + 149_999_999;
    let l3 = live(&p);
    refused_only(&tx_d(&l3, &long_spec(&k, 400_000_000, 1_050_000_000), &ent, &k), 1, &[0]);
    // Below its minFill (100 tokens), and its escrow continuation 1 sompi short.
    let mut lo = long_entrant(&k);
    lo.o.min_fill = 500_000_000;
    refused_only(&tx_d(&l, &long_spec(&k, 400_000_000, 1_050_000_000), &Entrant::Open(lo), &k), 1, &[0]);
    let mut d = tx_d(&l, &long_spec(&k, 400_000_000, 1_050_000_000), &ent, &k);
    d.outs[1].value -= 1;
    refused_only(&d, 1, &[0]);
}

#[test]
fn p3_part_or_remainder_one_sompi_under_its_range_is_refused() {
    let k = keys();
    let l = live(&listed(&k));
    // KpfOpen taker, partial: the part 1 below q(n, bs - bl') + C (both refuse), the remainder 1 below what it keeps.
    let d0 = tx_d(&l, &long_spec(&k, 400_000_000, 1_050_000_000), &Entrant::Open(long_entrant(&k)), &k);
    let mut d = d0.clone();
    d.outs[2].value -= 1;
    let (tx, e) = d.build();
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
    let mut d = d0.clone();
    d.outs[3].value -= 1;
    refused_only(&d, 0, &[1]);
    // Full: the part 1 below its range plus the inherited carrier.
    let mut d = tx_d(&l, &long_spec(&k, 1_000_000_000, 1_050_000_000), &Entrant::Open(long_entrant(&k)), &k);
    d.outs[2].value -= 1;
    pair_refused(&d);
    // Short side, direct taker.
    let mut d = tx_d(&l, &direct_spec(&k, SHORT, 400_000_000, 950_000_000, 1_210_000_000), &Entrant::Direct(k.taker), &k);
    d.outs[2].value -= 1;
    pair_refused(&d);
}

#[test]
fn p3_extra_carrier_never_comes_from_the_pair() {
    let k = keys();
    // A pair holding one carrier of excess above its floor (an over-funded open).
    let p = listed(&k);
    let mut l = live(&p);
    l.value += C as u64;
    // Partial: the remainder must keep the old carrier and the excess. An executor that funds the new part's
    // carrier from that excess (remainder at its bare floor) is refused.
    let d0 = tx_d(&l, &direct_spec(&k, LONG, 400_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.taker), &k);
    accept_d(&d0);
    let mut d = d0.clone();
    d.outs[3].value = p.with_size(600_000_000).floor() as u64;
    pair_refused(&d);
    // Without excess: the new carrier moved out of the remainder into the part.
    let l = live(&p);
    let mut d = tx_d(&l, &direct_spec(&k, LONG, 400_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.taker), &k);
    d.outs[3].value -= C as u64;
    d.outs[2].value += C as u64;
    pair_refused(&d);
    // Full, with excess: the part inherits carrier and excess; keeping the excess is refused.
    let mut l = live(&p);
    l.value += C as u64;
    let d0 = tx_d(&l, &direct_spec(&k, LONG, 1_000_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.taker), &k);
    let tx = accept_d(&d0);
    assert_eq!(tx.outputs[2].value as i64, d0.pairs[0].1.floor() + C);
    let mut d = d0.clone();
    d.outs[2].value -= C as u64;
    pair_refused(&d);
}

// ---------------------------------------------------------------- aliasing

#[test]
fn p3_same_id_parts_cannot_be_spent_together() {
    let k = keys();
    let l = live(&listed(&k));
    let d = tx_d(&l, &direct_spec(&k, LONG, 400_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.taker), &k);
    let tx = accept_d(&d);
    let lp = live_pairs(&tx, &d);
    let (part, rest) = (&lp[0], &lp[1]);
    assert_eq!(part.id, rest.id);
    // Each alone works: the remainder's long listing fills again; the part's short listing fills.
    accept_d(&tx_d(rest, &direct_spec(&k, LONG, 300_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.other), &k));
    accept_d(&tx_d(part, &direct_spec(&k, SHORT, 400_000_000, 950_000_000, 1_200_000_000), &Entrant::Direct(k.other), &k));
    // Together (the remainder's transfer and the part's transfer in one transaction): both refused.
    let a = tx_d(rest, &direct_spec(&k, LONG, 300_000_000, 1_050_000_000, 880_000_000), &Entrant::Direct(k.other), &k);
    let b = tx_d(part, &direct_spec(&k, SHORT, 200_000_000, 950_000_000, 1_200_000_000), &Entrant::Direct(k.other), &k);
    let mut ins = vec![a.ins[0].clone(), b.ins[0].clone(), p2pk_in(fop(9), F_FUNDS, None, &k.other)];
    ins[1].outpoint = part.op;
    let mut outs = vec![a.outs[0].clone(), b.outs[0].clone()];
    for o in [&a.outs[2], &a.outs[3], &b.outs[2], &b.outs[3]] {
        let mut o = o.clone();
        o.covenant = bind(0, rest.id);
        outs.push(o);
    }
    outs.push(out(0, p2pk_spk(&k.other), None));
    let (tx, e) = Draft { ins, outs: outs.clone(), lock: 0, seqs: vec![], genesis: vec![], change: Some(outs.len() - 1), pairs: vec![], opens: vec![] }.build();
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
}

#[test]
fn p3_two_orders_cannot_claim_one_part() {
    let k = keys();
    let l = live(&listed(&k));
    let t = long_spec(&k, 400_000_000, 1_050_000_000);
    let d = tx_d(&l, &t, &Entrant::Open(long_entrant(&k)), &k);
    // A second long order (another id, same owner and terms) also points at the part at output 2.
    let mut lo2 = long_entrant(&k);
    (lo2.id, lo2.op) = (Hash::from_bytes([0x43; 32]), outpoint(5));
    let mut ins = d.ins.clone();
    ins.insert(2, open_input(&lo2, "take", d.args(1)));
    let mut outs = d.outs.clone();
    let (o2, _) = order_out(&lo2, t.n, 2);
    outs.insert(2, o2);
    for o in outs.iter_mut().skip(3) {
        if o.covenant.is_some() {
            o.covenant = bind(0, l.id);
        }
    }
    let ch = outs.len() - 1;
    let dd = Draft { ins, outs, lock: 0, seqs: vec![], genesis: vec![], change: Some(ch), pairs: vec![], opens: vec![] };
    // The take arguments name partOut = 2; the part moved to 3, so fix both orders' argument.
    let mut dd2 = dd.clone();
    for i in [1, 2] {
        let mut a = dd2.args(i);
        a[0] = iv(3);
        dd2.set_args(i, a);
    }
    refused_only(&dd2, 2, &[0, 1]);

    // Two long orders sharing one covenant id (one multi-output genesis): neither may claim.
    let lo = long_entrant(&k);
    let v = lo.value;
    let mut g_outs = vec![out(v, p2sh(&lo.o.art()), None), out(v, p2sh(&lo.o.art()), None)];
    let g = genesis_id(outpoint(9), &g_outs, &[0, 1]);
    g_outs[0].covenant = bind(0, g);
    g_outs[1].covenant = bind(0, g);
    let (gtx, ge) = build(&[p2pk_in(outpoint(9), 3 * v, None, &k.f)], g_outs);
    accept(&gtx, &ge);
    let tw = |i: usize| {
        let (op, _) = utxo_of(&gtx, i);
        LiveOpen { o: lo.o.clone(), value: v, id: g, op }
    };
    let mut t = long_spec(&k, 400_000_000, 1_050_000_000);
    t.et = g;
    let d = tx_d(&l, &t, &Entrant::Open(tw(0)), &k);
    let mut ins = d.ins.clone();
    ins.insert(2, open_input(&tw(1), "take", d.args(1)));
    let mut outs = d.outs.clone();
    let (o2, _) = order_out(&tw(1), t.n, 2);
    outs.insert(2, o2);
    let ch = outs.len() - 1;
    let mut dd = Draft { ins, outs, lock: 0, seqs: vec![], genesis: vec![], change: Some(ch), pairs: vec![], opens: vec![] };
    for i in [1, 2] {
        let mut a = dd.args(i);
        a[0] = iv(3);
        dd.set_args(i, a);
    }
    let (tx, e) = dd.build();
    assert_input_refused(&tx, &e, 1);
    assert_input_refused(&tx, &e, 2);
}

/// The stale-tag alias the live-tag guard closes. Pair Q was opened earlier by the long order L' (Q.tagL = id(L'));
/// L' is still alive. Q's short owner amends its own listing, and the executor points L''s `take` at Q's
/// continuation, which already holds L''s owner at L''s rate: L' would pay a second margin for a position it
/// already has. Q's amend refuses because a tag of Q names an input of the transaction; the order alone accepts.
#[test]
fn p3_stale_tag_cannot_alias_a_hand_over() {
    let k = keys();
    let lo = long_entrant(&k);
    let x0 = 1_000_000_000;
    let mut qp = pair(&k.entrant, &k.short);
    (qp.size, qp.bl, qp.bs, qp.entry, qp.tag_l) = (400_000_000, x0 - lo.o.my_rate, 1_200_000_000, x0, L_ID);
    let ql = Live { p: qp.clone(), value: qp.floor() as u64, id: Hash::from_bytes([0x77; 32]), op: outpoint(30) };
    let w = (950_000_000, 100_000_000, 0);
    let mut d = amend_draft(&ql, &k.short, SHORT, w, &k.f, 4);
    let (pre, suf) = parts(pair_base());
    let ta = vec![iv(0), iv(qp.size), iv(x0), iv(qp.bs), bv(&qp.short_key), bv(&qp.tag_s.as_bytes()), iv(w.0), iv(w.1), iv(w.2), bv(&pre), bv(&suf)];
    d.ins.insert(1, open_input(&lo, "take", ta));
    let (oo, _) = order_out(&lo, qp.size, 1);
    d.outs.insert(1, oo);
    d.change = Some(2);
    refused_only(&d, 0, &[1]);
    // The same amend without L' in the transaction is fine.
    accept_d(&amend_draft(&ql, &k.short, SHORT, w, &k.f, 4));
}
