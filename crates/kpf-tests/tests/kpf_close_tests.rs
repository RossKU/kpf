//! KpfPair P4: close-versus-close (DESIGN 3.4 c, 4.5 T3) and mutual close (T3').
//!
//! T3 shape: in 0 host P1 (its long listing is filled), 1 donor P2 (its short listing), 2 F. out 0 P1's long, 1
//! P2's short, 2 P3 = P1's continuation (n, BL2, BS1) with P2's long, then P1's and P2's remainders, then F's
//! change. T3' shape: in 0 pair, 1 F; out 0 long, remainder, change, short at M - 1. Every accepted transaction
//! is checked for conservation.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::{Transaction, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf::paths::*;
use kpf::*;
use secp256k1::Keypair;

const H_ID: Hash = Hash::from_bytes([0x5a; 32]);
const D_ID: Hash = Hash::from_bytes([0x5b; 32]);
const Q_ID: Hash = Hash::from_bytes([0x5c; 32]);

struct K {
    l1: Keypair,
    s1: Keypair,
    l2: Keypair,
    s2: Keypair,
    other: Keypair,
    f: Keypair,
}

fn keys() -> K {
    K { l1: keypair(11), s1: keypair(12), l2: keypair(21), s2: keypair(22), other: keypair(13), f: keypair(14) }
}

/// P1 = (9, 12) KAS, 1,000 tokens; its long sells at >= 10 KAS (parts >= 100, tip 0.01).
fn host_pair(k: &K, s: i64) -> Pair {
    let mut p = pair(&k.l1, &k.s1).with_size(s);
    (p.long_ask, p.long_min, p.long_tip) = (1_000_000_000, 100_000_000, 1_000_000);
    (p.short_bid, p.short_min, p.short_tip) = (800_000_000, 0, 0);
    p
}

/// P2 = (8.5, 11.5) KAS; its short buys back at <= 10.3 KAS (parts >= 100, tip 0.02); its long has a listing
/// of its own (13 KAS), which moves with it.
fn donor_pair(k: &K, s: i64) -> Pair {
    let mut p = pair(&k.l2, &k.s2).with_size(s);
    (p.bl, p.bs, p.entry) = (850_000_000, 1_150_000_000, 1_000_000_000);
    (p.long_ask, p.long_min, p.long_tip) = (1_300_000_000, 200_000_000, 0);
    (p.short_bid, p.short_min, p.short_tip) = (1_030_000_000, 100_000_000, 2_000_000);
    (p.tag_l, p.tag_s) = (Hash::from_bytes([0x2a; 32]), Hash::from_bytes([0x2b; 32]));
    p
}

fn live_as(p: &Pair, id: Hash, op: u8) -> Live {
    Live { p: p.clone(), value: p.floor() as u64, id, op: outpoint(op), daa: 0 }
}

fn host(k: &K, s: i64) -> Live {
    live_as(&host_pair(k, s), H_ID, 10)
}

fn donor(k: &K, s: i64) -> Live {
    live_as(&donor_pair(k, s), D_ID, 20)
}

const X: i64 = 1_020_000_000;
/// The entry price of both test pairs, and so the only P3 entry both accept (E(donor) <= e3 <= E(host)).
const E0: i64 = 1_000_000_000;

fn cs(n: i64) -> CloseSpec {
    CloseSpec { n, x: X, e3: E0 }
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

fn refused_only(d: &Draft, idx: usize, ok: &[usize]) {
    let (tx, e) = d.build();
    let r = run(&tx, &e).expect("bindings");
    for &o in ok {
        assert!(r[o].is_ok(), "input {o} should accept: {:?}", r[o]);
    }
    assert_input_refused(&tx, &e, idx);
}

fn both_refused(d: &Draft) {
    let (tx, e) = d.build();
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
}

fn cd(h: &Live, d: &Live, c: &CloseSpec, k: &K) -> Draft {
    close_draft(h, d, c, &k.f, 3)
}

// ---------------------------------------------------------------- close-versus-close accepted

#[test]
fn p4_full_against_full_is_the_worked_example() {
    let k = keys();
    let (h, d) = (host(&k, 1_000_000_000), donor(&k, 1_000_000_000));
    let dr = cd(&h, &d, &cs(1_000_000_000), &k);
    let tx = accept_d(&dr);
    // DESIGN 5: L1 gets 1,200 KAS (less its tip of 10), S2 1,300 KAS (less 20, plus P2's freed carrier), and
    // P3 = (8.5, 12) holds 3,500 KAS plus P1's carrier.
    assert_eq!(tx.outputs[0].value, 120_000_000_000 - 1_000_000_000);
    assert_eq!(tx.outputs[1].value, 130_000_000_000 - 2_000_000_000 + C as u64);
    assert_eq!(tx.outputs[2].value, 350_000_000_000 + C as u64);
    let p3 = &dr.pairs[0].1;
    // The entry is e3 (DESIGN 4.5a), not the fill price x.
    assert_eq!((p3.size, p3.bl, p3.bs, p3.entry), (1_000_000_000, 850_000_000, 1_200_000_000, E0));
    // Stayers: S1 keeps key, bound and listing, its tag cleared; L2 keeps key, bound and listing, tagged by P2's id.
    assert_eq!((p3.short_key, p3.tag_s, p3.short_window()), (h.p.short_key, ZERO, h.p.short_window()));
    assert_eq!((p3.long_key, p3.tag_l, p3.long_window()), (d.p.long_key, D_ID, d.p.long_window()));
    // The executor keeps the two tips: 3,000 + 3,000 = 1,190 + 1,280 + 3,500 + 30.
    assert_eq!(tx.outputs[3].value + FEE - F_FUNDS, 3_000_000_000);
    // P3 is a live pair: its owners can close it.
    let lp = live_pairs(&tx, &dr);
    let ins = [pair_input(&lp[0], "coopClose", vec![Arg::Sig(k.l2), Arg::Sig(k.s1)])];
    let (ctx, ce) = build(&ins, vec![out(lp[0].value, p2pk_spk(&k.l2), None)]);
    accept(&ctx, &ce);
}

#[test]
fn p4_partial_fills_on_either_or_both_sides() {
    let k = keys();
    for (s1, s2, n) in [(1_000_000_000, 400_000_000, 400_000_000), (400_000_000, 1_000_000_000, 400_000_000), (1_000_000_000, 1_500_000_000, 400_000_000)] {
        let (h, d) = (host(&k, s1), donor(&k, s2));
        let dr = cd(&h, &d, &cs(n), &k);
        let tx = accept_d(&dr);
        // Remainders keep everything but the size and the tags (cleared); each keeps its pair's old carrier.
        for (idx, p) in dr.pairs.iter().skip(1) {
            let src = if p.long_key == h.p.long_key { &h } else { &d };
            assert_eq!(p, &src.p.rest_of(src.p.size - n), "remainder {idx}");
            assert_eq!(tx.outputs[*idx].value as i64, src.value as i64 - src.p.range_of(n));
        }
        // P3 holds its floor: a new carrier (the executor's) when the host splits, the host's own when it does not.
        let p3 = &dr.pairs[0].1;
        assert_eq!(tx.outputs[2].value as i64, p3.floor());
    }
}

#[test]
fn p4_donor_before_host_in_the_inputs() {
    let k = keys();
    let (h, d) = (host(&k, 1_000_000_000), donor(&k, 1_000_000_000));
    let c = cs(400_000_000);
    let base = cd(&h, &d, &c, &k);
    // in 0 donor, 1 host, 2 F; out 0 S2, 1 L1, 2 P3, 3 host rest, 4 donor rest, 5 change.
    let mut dr = base.clone();
    dr.ins.swap(0, 1);
    dr.outs.swap(0, 1);
    dr.outs[2].covenant = bind(1, H_ID);
    dr.outs[3].covenant = bind(1, H_ID);
    dr.outs[4].covenant = bind(0, D_ID);
    accept_d(&dr);
}

// ---------------------------------------------------------------- listing rules

#[test]
fn p4_price_outside_either_listing_is_refused() {
    let k = keys();
    let (h, d) = (host(&k, 1_000_000_000), donor(&k, 1_000_000_000));
    refused_only(&cd(&h, &d, &CloseSpec { n: 400_000_000, x: 999_999_999, e3: E0 }, &k), 0, &[1]);
    refused_only(&cd(&h, &d, &CloseSpec { n: 400_000_000, x: 1_030_000_001, e3: E0 }, &k), 1, &[0]);
    accept_d(&cd(&h, &d, &CloseSpec { n: 400_000_000, x: 1_000_000_000, e3: E0 }, &k));
    accept_d(&cd(&h, &d, &CloseSpec { n: 400_000_000, x: 1_030_000_000, e3: E0 }, &k));
}

#[test]
fn p4_unlisted_side_is_refused() {
    let k = keys();
    let mut hp = host_pair(&k, 1_000_000_000);
    hp.long_ask = 0;
    let mut dp = donor_pair(&k, 1_000_000_000);
    refused_only(&cd(&live_as(&hp, H_ID, 10), &donor(&k, 1_000_000_000), &cs(400_000_000), &k), 0, &[1]);
    dp.short_bid = 0;
    refused_only(&cd(&host(&k, 1_000_000_000), &live_as(&dp, D_ID, 20), &cs(400_000_000), &k), 1, &[0]);
}

#[test]
fn p4_part_and_remainder_sizes() {
    let k = keys();
    // Below the host's listing minimum (raised to 500 tokens), and below the donor's (100).
    let mut hp = host_pair(&k, 1_000_000_000);
    hp.long_min = 500_000_000;
    refused_only(&cd(&live_as(&hp, H_ID, 10), &donor(&k, 1_000_000_000), &cs(400_000_000), &k), 0, &[1]);
    refused_only(&cd(&host(&k, 1_000_000_000), &donor(&k, 1_000_000_000), &cs(99_999_999), &k), 1, &[]);
    // A host remainder below minSize; a donor remainder below minSize.
    refused_only(&cd(&host(&k, 1_000_000_000), &donor(&k, 2_000_000_000), &cs(999_999_999), &k), 0, &[1]);
    refused_only(&cd(&host(&k, 2_000_000_000), &donor(&k, 1_000_000_000), &cs(999_999_999), &k), 1, &[0]);
}

#[test]
fn p4_leavers_paid_short_or_tip_above_rate_are_refused() {
    let k = keys();
    for (s1, s2) in [(1_000_000_000, 1_000_000_000), (1_000_000_000, 2_000_000_000)] {
        let (h, d) = (host(&k, s1), donor(&k, s2));
        let n = 1_000_000_000;
        let base = cd(&h, &d, &cs(n), &k);
        let mut dr = base.clone();
        dr.outs[0].value -= 1;
        refused_only(&dr, 0, &[1]);
        let mut dr = base.clone();
        dr.outs[1].value -= 1;
        refused_only(&dr, 1, &[0]);
        // Tips taken at the listed rate + 1 sompi per token.
        let mut dr = base.clone();
        dr.outs[0].value = (q(n, X - h.p.bl, SCALE, 0) - q(n, h.p.long_tip + 1, SCALE, 0)) as u64;
        refused_only(&dr, 0, &[1]);
        let mut dr = base.clone();
        dr.outs[1].value -= q(n, 1, SCALE, 0) as u64;
        refused_only(&dr, 1, &[0]);
        // Payouts to the wrong keys.
        let mut dr = base.clone();
        dr.outs[0].script_public_key = p2pk_spk(&k.other);
        refused_only(&dr, 0, &[1]);
        let mut dr = base.clone();
        dr.outs[1].script_public_key = p2pk_spk(&k.l2);
        refused_only(&dr, 1, &[0]);
    }
}

// ---------------------------------------------------------------- the stayers

#[test]
fn p4_downgrade_by_one_is_refused() {
    let k = keys();
    // BL2 = BL1 + 1: S1's new counterparty would bankrupt earlier; the host refuses.
    let mut dp = donor_pair(&k, 1_000_000_000);
    dp.bl = 900_000_001;
    for n in [1_000_000_000, 400_000_000] {
        refused_only(&cd(&host(&k, 1_000_000_000), &live_as(&dp, D_ID, 20), &cs(n), &k), 0, &[1]);
    }
    // BS1 = BS2 - 1: L2's new counterparty would bankrupt earlier; the donor refuses.
    let mut hp = host_pair(&k, 1_000_000_000);
    hp.bs = 1_149_999_999;
    for n in [1_000_000_000, 400_000_000] {
        refused_only(&cd(&live_as(&hp, H_ID, 10), &donor(&k, 1_000_000_000), &cs(n), &k), 1, &[0]);
    }
    // Equal bounds are allowed.
    let mut dp = donor_pair(&k, 1_000_000_000);
    dp.bl = 900_000_000;
    dp.bs = 1_200_000_000;
    accept_d(&cd(&host(&k, 1_000_000_000), &live_as(&dp, D_ID, 20), &cs(400_000_000), &k));
}

/// Each pair checks only its own stayer's side of P3; changing the other side's fields is refused by the pair
/// that owns them, even when the other pair is told the changed value.
#[test]
fn p4_stayers_and_market_are_untouched() {
    let k = keys();
    let (h, d) = (host(&k, 1_000_000_000), donor(&k, 1_000_000_000));
    let c = cs(400_000_000);
    let base = cd(&h, &d, &c, &k);
    let p3 = base.pairs[0].1.clone();
    // (mutation of P3, what the host is told, what the donor is told, which input refuses)
    type M = Box<dyn Fn(&mut Pair)>;
    let cases: Vec<(&str, M, usize)> = vec![
        ("S1 key", Box::new(|p: &mut Pair| p.short_key = xonly(&keypair(13))), 0),
        ("S1 bound", Box::new(|p: &mut Pair| p.bs += 1), 0),
        ("S1 listing", Box::new(|p: &mut Pair| p.short_tip += 1), 0),
        ("L2 key", Box::new(|p: &mut Pair| p.long_key = xonly(&keypair(13))), 1),
        ("L2 bound", Box::new(|p: &mut Pair| p.bl -= 1), 1),
        ("L2 listing", Box::new(|p: &mut Pair| p.long_ask -= 1), 1),
        ("L2 tag", Box::new(|p: &mut Pair| p.tag_l = Hash::from_bytes([0x5d; 32])), 1),
    ];
    for (name, f, idx) in cases {
        let mut p = p3.clone();
        f(&mut p);
        let mut dr = base.clone();
        dr.outs[2].value += 1_000_000_000;
        dr.set_pair(2, &p);
        // Tell both pairs the changed P3 fields through their arguments.
        let w = p.long_window();
        dr.set_args(0, vec![iv(c.n), iv(c.x), iv(c.e3), iv(0), iv(p.bl), bv(&p.long_key), iv(w.0), iv(w.1), iv(w.2), bv(&p.tag_l.as_bytes())]);
        let w = p.short_window();
        dr.set_args(1, vec![iv(c.n), iv(c.x), iv(c.e3), iv(2), iv(p.bs), bv(&p.short_key), iv(w.0), iv(w.1), iv(w.2)]);
        let (tx, e) = dr.build();
        let r = run(&tx, &e).unwrap();
        assert!(r[idx].is_err(), "{name}: input {idx} accepted");
        assert!(r[1 - idx].is_ok(), "{name}: input {} refused: {:?}", 1 - idx, r[1 - idx]);
    }
    // Entry, size, market block, and a tag on S1's side (neither pair writes one there: S1 is not handed over):
    // both refuse.
    let cases: Vec<(&str, M)> = vec![
        ("entry", Box::new(|p: &mut Pair| p.entry += 1)),
        ("S1 tag", Box::new(|p: &mut Pair| p.tag_s = Hash::from_bytes([9; 32]))),
        ("S1 tag = P2's id", Box::new(|p: &mut Pair| p.tag_s = D_ID)),
        ("size", Box::new(|p: &mut Pair| p.size += 1)),
        ("carrier", Box::new(|p: &mut Pair| p.m.carrier = 0)),
        ("ext", Box::new(|p: &mut Pair| p.m.ext = Hash::from_bytes([9; 32]))),
    ];
    for (name, f) in cases {
        let mut p = p3.clone();
        f(&mut p);
        let mut dr = base.clone();
        dr.outs[2].value += 1_000_000_000;
        dr.set_pair(2, &p);
        let (tx, e) = dr.build();
        let r = run(&tx, &e).unwrap();
        assert!(r[0].is_err() && r[1].is_err(), "{name}: {r:?}");
    }
}

#[test]
fn p4_values_one_sompi_short_are_refused() {
    let k = keys();
    let (h, d) = (host(&k, 1_000_000_000), donor(&k, 1_500_000_000));
    let base = cd(&h, &d, &cs(400_000_000), &k);
    // P3 below its range + carrier: both refuse.
    let mut dr = base.clone();
    dr.outs[2].value -= 1;
    both_refused(&dr);
    // Host remainder, donor remainder.
    let mut dr = base.clone();
    dr.outs[3].value -= 1;
    refused_only(&dr, 0, &[1]);
    let mut dr = base.clone();
    dr.outs[4].value -= 1;
    refused_only(&dr, 1, &[0]);
    // Host full: P3 inherits the host's carrier and excess.
    let mut hl = host(&k, 400_000_000);
    hl.value += 7;
    let dr = cd(&hl, &d, &cs(400_000_000), &k);
    let tx = accept_d(&dr);
    assert_eq!(tx.outputs[2].value as i64, dr.pairs[0].1.floor() + 7);
    let mut dr2 = dr.clone();
    dr2.outs[2].value -= 1;
    refused_only(&dr2, 0, &[1]);
}

// ---------------------------------------------------------------- aliasing

#[test]
fn p4_hand_over_must_be_a_fresh_p3_of_another_id() {
    let k = keys();
    let (h, d) = (host(&k, 1_000_000_000), donor(&k, 1_000_000_000));
    let base = cd(&h, &d, &cs(400_000_000), &k);
    // The donor pointed at the host's remainder (a third pair's state, P1's own tags) instead of P3.
    let mut dr = base.clone();
    let mut a = dr.args(1);
    a[3] = iv(3);
    dr.set_args(1, a);
    refused_only(&dr, 1, &[0]);
    // P3 without a covenant id (unspendable): the donor refuses (and the host finds no continuation).
    let mut dr = base.clone();
    dr.outs[2].covenant = None;
    let (tx, e) = dr.build();
    assert_input_refused(&tx, &e, 1);
    // P3 under the donor's own id.
    let mut dr = base.clone();
    dr.outs[2].covenant = bind(1, D_ID);
    let (tx, e) = dr.build();
    assert_input_refused(&tx, &e, 1);
}

/// The stale-tag alias, P4 form. Q is the P3 of an earlier partial close of P2 (Q.tagL = id(P2), Q's long is
/// L2 at BL2 with L2's listing). P2's remainder is now a donor again, and the executor points it at the part of a
/// transfer of Q's short: if that part kept tagL = id(P2) it would satisfy the donor, so P2's L2 would be paid for
/// twice and the executor keep one. Rule that refuses it (DESIGN 4.4, no entry copies a tag): Q's transfer hands
/// over only its short, so the part carries tagL = 0. With the honest part the donor refuses (no tag of its id);
/// with the stale tag copied into the part Q refuses (it writes 0), the donor alone accepting.
#[test]
fn p4_third_party_pair_cannot_be_the_re_pair_target() {
    let k = keys();
    let d = donor(&k, 1_000_000_000);
    let n = 300_000_000;
    let mut qp = host_pair(&k, n);
    (qp.bl, qp.long_key, qp.tag_l) = (d.p.bl, d.p.long_key, D_ID);
    (qp.long_ask, qp.long_min, qp.long_tip) = d.p.long_window();
    (qp.short_bid, qp.short_min, qp.short_tip) = (1_030_000_000, 0, 0);
    let ql = live_as(&qp, Q_ID, 30);
    let taker = keypair(40);
    let nb = 1_250_000_000;
    let t = TransferSpec { side: SHORT, n, x: X, nb, ek: xonly(&taker), et: ZERO, k: 0 };
    let to = transfer_out(&ql, &t);
    let pd = CloseSpec { n, x: X, e3: E0 };
    assert_eq!(to.part.tag_l, ZERO, "Q's part does not copy its stale tag");
    let dargs = vec![iv(n), iv(X), iv(qp.entry), iv(2), iv(nb), bv(&xonly(&taker)), iv(0), iv(0), iv(0)];
    let dco = close_out(&live_as(&qp, Q_ID, 30), &d, &pd);
    let (dr_rest, dr_rest_v) = dco.donor_rest.clone().unwrap();
    let ins = vec![
        pair_input(&ql, "transfer", vec![iv(SHORT), iv(n), iv(X), iv(nb), bv(&xonly(&taker)), bv(&ZERO.as_bytes()), iv(0)]),
        pair_input(&d, "closeDonor", dargs),
        p2pk_in(fop(7), F_FUNDS, None, &taker),
    ];
    let outs = vec![
        out(to.pay as u64, p2pk_x(&qp.short_key), None),
        out(dco.donor_pay as u64, p2pk_x(&d.p.short_key), None),
        out(to.part_value as u64, p2sh(&to.part.art()), bind(0, Q_ID)),
        out(dr_rest_v as u64, p2sh(&dr_rest.art()), bind(1, D_ID)),
        out(0, p2pk_spk(&taker), None),
    ];
    // Both hand-overs wait for their pair's rate limit (DESIGN 4.5c).
    let dr = Draft { ins, outs, lock: ql.handover_from().max(d.handover_from()), seqs: vec![], genesis: vec![], change: Some(4), pairs: vec![], pendings: vec![], opens: vec![] };
    // The honest part (tagL = 0): Q accepts its own transfer, the donor finds no hand-over to it.
    refused_only(&dr, 1, &[0]);
    // The stale tag copied into the part: the donor alone would accept; Q refuses.
    let mut stale = to.part.clone();
    stale.tag_l = D_ID;
    let mut dr2 = dr.clone();
    dr2.outs[2].script_public_key = p2sh(&stale.art());
    refused_only(&dr2, 0, &[1]);
}

#[test]
fn p4_one_p3_cannot_be_claimed_twice() {
    let k = keys();
    let n = 1_000_000_000;
    let (h, d) = (host(&k, n), donor(&k, n));
    let base = cd(&h, &d, &cs(n), &k);
    // A second donor (another id, the same long) also claims P3: in 0 host, 1 donor, 2 donor', 3 F; out 0 L1, 1
    // S2, 2 S2', 3 P3, 4 change.
    let mut d2 = d.clone();
    (d2.id, d2.op) = (Hash::from_bytes([0x5e; 32]), outpoint(21));
    let mk = |d2: &Live| {
        let mut ins = base.ins.clone();
        ins.insert(2, pair_input(d2, "closeDonor", donor_args(&h, &cs(n), 3)));
        let mut outs = base.outs.clone();
        outs.insert(2, out(base.outs[1].value, p2pk_x(&d2.p.short_key), None));
        let mut dr = Draft { ins, outs, lock: base.lock, seqs: vec![], genesis: vec![], change: Some(4), pairs: vec![], pendings: vec![], opens: vec![] };
        let mut a = dr.args(1);
        a[3] = iv(3);
        dr.set_args(1, a);
        dr
    };
    refused_only(&mk(&d2), 2, &[0, 1]);
    // Two donors sharing the id: both refused by the single-input guard.
    let mut d3 = d.clone();
    d3.op = outpoint(22);
    let (tx, e) = mk(&d3).build();
    assert_input_refused(&tx, &e, 1);
    assert_input_refused(&tx, &e, 2);
    // Host and donor sharing one id (same-id twins of a split): both refused.
    let mut dt = d.clone();
    dt.id = H_ID;
    let mut dr = cd(&h, &dt, &cs(n), &k);
    dr.outs[2].covenant = bind(0, H_ID);
    both_refused(&dr);
}

// ---------------------------------------------------------------- mutual close

/// Both listings of one pair cross: the long sells at >= 10, the short buys back at <= 10.5.
fn crossing(k: &K) -> Pair {
    let mut p = pair(&k.l1, &k.s1);
    (p.long_ask, p.long_min, p.long_tip) = (1_000_000_000, 100_000_000, 1_000_000);
    (p.short_bid, p.short_min, p.short_tip) = (1_050_000_000, 100_000_000, 2_000_000);
    p
}

fn md(l: &Live, n: i64, x: i64, k: &K) -> Draft {
    mutual_draft(l, n, x, &k.f, 3)
}

fn first_refused(d: &Draft) {
    let (tx, e) = d.build();
    assert_input_refused(&tx, &e, 0);
}

#[test]
fn p4_mutual_close_full_and_partial() {
    let k = keys();
    let l = live_as(&crossing(&k), H_ID, 10);
    // Full at 10.2: long q(s, 1.2) - 10, short q(s, 1.8) - 20 plus the carrier.
    let d = md(&l, 1_000_000_000, X, &k);
    let tx = accept_d(&d);
    assert_eq!(tx.outputs[0].value, 120_000_000_000 - 1_000_000_000);
    assert_eq!(tx.outputs[2].value, 180_000_000_000 - 2_000_000_000 + C as u64);
    let mut d2 = d.clone();
    d2.outs[2].value -= 1;
    first_refused(&d2);
    // Partial: 400 tokens close, the remainder keeps both listings and the carrier.
    let d = md(&l, 400_000_000, X, &k);
    let tx = accept_d(&d);
    assert_eq!(d.pairs[0].1, l.p.rest_of(600_000_000));
    assert_eq!(tx.outputs[1].value as i64, l.value as i64 - l.p.range_of(400_000_000));
    // At both listing limits.
    accept_d(&md(&l, 400_000_000, 1_000_000_000, &k));
    accept_d(&md(&l, 400_000_000, 1_050_000_000, &k));
}

#[test]
fn p4_mutual_close_needs_crossing_listings() {
    let k = keys();
    let mut p = crossing(&k);
    p.long_ask = 1_050_000_001;
    let l = live_as(&p, H_ID, 10);
    for x in [1_050_000_000, 1_050_000_001, X] {
        first_refused(&md(&l, 400_000_000, x, &k));
    }
    // Price outside a crossing pair's listings.
    let l = live_as(&crossing(&k), H_ID, 10);
    first_refused(&md(&l, 400_000_000, 999_999_999, &k));
    first_refused(&md(&l, 400_000_000, 1_050_000_001, &k));
    // One side unlisted.
    for f in [|p: &mut Pair| p.long_ask = 0, |p: &mut Pair| p.short_bid = 0] {
        let mut p = crossing(&k);
        f(&mut p);
        first_refused(&md(&live_as(&p, H_ID, 10), 400_000_000, X, &k));
    }
}

#[test]
fn p4_mutual_close_payouts_slots_and_sizes() {
    let k = keys();
    let l = live_as(&crossing(&k), H_ID, 10);
    let base = md(&l, 400_000_000, X, &k);
    for idx in [0, 1, 3] {
        let mut d = base.clone();
        d.outs[idx].value -= 1;
        first_refused(&d);
    }
    // Long and short swapped.
    let mut d = base.clone();
    let (a, b) = (d.outs[0].script_public_key.clone(), d.outs[3].script_public_key.clone());
    (d.outs[0].script_public_key, d.outs[3].script_public_key) = (b, a);
    first_refused(&d);
    // The short's reverse slot below the input count: two outputs for two inputs.
    let mo = mutual_out(&live_as(&crossing(&k), H_ID, 10), 1_000_000_000, X);
    let ins = vec![pair_input(&l, "mutualClose", vec![iv(1_000_000_000), iv(X)]), funder(&k.f, 3, F_FUNDS)];
    let outs = vec![out(mo.lp as u64, p2pk_spk(&k.l1), None), out(mo.sp as u64, p2pk_spk(&k.s1), None)];
    first_refused(&Draft { ins, outs, lock: 0, seqs: vec![], genesis: vec![], change: None, pairs: vec![], pendings: vec![], opens: vec![] });
    // Below a listing minimum; a remainder below minSize.
    first_refused(&md(&l, 99_999_999, X, &k));
    first_refused(&md(&l, 999_999_999, X, &k));
    // Two continuations.
    let mut d = base.clone();
    let extra = d.outs[1].clone();
    d.outs.insert(2, extra);
    d.change = Some(3);
    first_refused(&d);
}

// ---------------------------------------------------------------- measurements

/// Sizes and script units of the listing paths (consensus flags). Run with `--nocapture` to print the table.
#[test]
fn metrics_listing_paths() {
    let k = keys();
    println!("KpfPair: script {} B, state {} B", bytecode(pair_base()).len(), host(&k, 1).p.state_bytes().len());
    println!("KpfOpen: script {} B, state {} B", bytecode(open_base()).len(), long_order(&k.l1).state_bytes().len());
    // P3: a long listing taken by a KpfOpen (partial), a short listing by a direct taker (full).
    let mut tp = crossing(&k);
    (tp.short_bid, tp.short_min) = (950_000_000, 0);
    // A long KpfOpen takes only at x <= E (KpfOpen.take); the pair's entry sits above the fill price.
    tp.entry = 1_100_000_000;
    let tl = live_as(&tp, H_ID, 10);
    let mut lo = long_order(&k.l2);
    (lo.limit, lo.my_rate) = (1_100_000_000, 200_000_000);
    let lo = LiveOpen { value: lo.escrow() as u64, o: lo, id: Hash::from_bytes([0x41; 32]), op: outpoint(1) };
    let t = TransferSpec { side: LONG, n: 400_000_000, x: 1_050_000_000, nb: 850_000_000, ek: xonly(&k.l2), et: lo.id, k: 0 };
    let t3 = transfer_draft(&tl, &t, &Entrant::Open(lo.clone()), &k.f, 3);
    let t3f = TransferSpec { side: LONG, n: 1_000_000_000, ..t.clone() };
    let t3f = transfer_draft(&tl, &t3f, &Entrant::Open(lo), &k.f, 3);
    let ts = TransferSpec { side: SHORT, n: 1_000_000_000, x: 950_000_000, nb: 1_250_000_000, ek: xonly(&k.s2), et: ZERO, k: 0 };
    let t3s = transfer_draft(&tl, &ts, &Entrant::Direct(k.s2), &k.f, 3);
    let c4 = cd(&host(&k, 1_000_000_000), &donor(&k, 1_500_000_000), &cs(400_000_000), &k);
    let c4f = cd(&host(&k, 1_000_000_000), &donor(&k, 1_000_000_000), &cs(1_000_000_000), &k);
    let m4 = md(&live_as(&crossing(&k), H_ID, 10), 400_000_000, X, &k);
    let m4f = md(&live_as(&crossing(&k), H_ID, 10), 1_000_000_000, X, &k);
    let rows: Vec<(&str, &Draft, usize)> = vec![
        ("P3 transfer (long, partial)", &t3, 0),
        ("P3 transfer (long, full)", &t3f, 0),
        ("P3 transfer (short, direct)", &t3s, 0),
        ("KpfOpen take (partial)", &t3, 1),
        ("KpfOpen take (filled out)", &t3f, 1),
        ("P4 closeHost (partial)", &c4, 0),
        ("P4 closeDonor (partial)", &c4, 1),
        ("P4 closeHost (full)", &c4f, 0),
        ("P4 closeDonor (full)", &c4f, 1),
        ("P4 mutualClose (partial)", &m4, 0),
        ("P4 mutualClose (full)", &m4f, 0),
    ];
    for (name, d, i) in rows {
        let (tx, e) = d.build();
        let r = &run_metered(&tx, &e)[i];
        assert!(r.result.is_ok(), "{name}: {:?}", r.result);
        assert!(r.max_stack < 244, "{name}: stack {}", r.max_stack);
        println!("{name:<30} sigscript {:>5} B  script units {:>7}  max stack {:>3}", tx.inputs[i].signature_script.len(), r.script_units, r.max_stack);
    }
}
