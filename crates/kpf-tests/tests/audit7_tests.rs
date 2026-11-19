//! Review 7 (docs/audit/REVIEW-7.md): the oracle-market family after the fresh-id change of review 6
//! (DESIGN-ORACLE-IMPL 24) and the maker fields that fix R7-F1 (DESIGN-ORACLE-IMPL 25). Every transaction here runs
//! every input in the real script engine with the covenant context (`common::run`). `exploit_` tests build an attack and
//! assert that the engine ACCEPTS it; `holds_` tests assert that an attack is refused. Worked numbers use the fixture
//! market of `oraclekit` (1,000 units at 10 KAS, long margin 1,000 KAS, short margin 2,000 KAS, cover 6,000 KAS, a
//! claim's bond 10 % of the cover).
//!
//! The root of R7-F1: `fresh(out)` proves that output `out` starts a new covenant id carried by no input and by no other
//! output, but nothing tied that output to the input that checks it, so two inputs whose entries computed one script
//! (two pairs filled at one price against one stayer, a restored half beside its remainder) accepted one output and the
//! builder kept the second input's value. The fix: every fresh output names its makers, one field per creating role,
//! each the creating input's outpoint commitment (`maker_of`, `KpfPairO.me`); a creating input requires its own role's
//! field to hold its own commitment and every role its transaction shape cannot share to hold zero. One field holds one
//! commitment, so two inputs in one role never accept one output, and the roles that may share an output are exactly
//! the documented compositions (two orders opening a pair, a pair and the order that takes its listing, host and donor).

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use common::*;
use kaspa_consensus_core::tx::TransactionOutput;
use kpf_protocol::bond::{BindArgs, Claim, Ctx};
use kpf_protocol::oracle::*;
use oraclekit::*;
use secp256k1::Keypair;

const KAS: i64 = 100_000_000;
const FALSE_P: i64 = 920_000_000;
const ASK: i64 = 1_000_000_000;

/// The attacker's own key (never the underwriter's).
fn attacker() -> Keypair {
    keypair(50)
}

fn liq(side: i64, n: i64, p: i64, t0: i64, lock: u64) -> ClaimSpec {
    ClaimSpec { kind: LIQ, side, n, p, t0, lock }
}

/// Two pairs opened by two long orders of the attacker against one resting short order of the victim (`short_kp`), at
/// the same price and in the same expiry step, under the same honest underwriter: what a market maker's resting order
/// gets whenever it is filled twice at one price. They differ in their covenant ids, their outpoints, the long's maker
/// and nothing a hand-over of the long carries into the entrant's part but the parent maker.
fn two_pairs_against_one_short() -> (LPair, LPair) {
    let a = attacker();
    let (lo1, lo2, so, sl) = (order(L, &a, 0x73, 0), order(L, &a, 0x74, 0), order(S, &short_kp(), 0x72, 0), slot(K));
    let sp = open_spec();
    let (t1, p1, b1) = open_tx(&lo1, &so, &sl, &sp, &|_, _| {});
    must("first open", &t1);
    let (so2, _) = order_after(&so, sp.n, so.s.fill_spend(sp.n, sp.premium));
    let so2 = live(&t1, 1, OPEN_DAA, &so2.expect("the short order rests on"));
    let sl2 = live(&t1, 3, OPEN_DAA, &b1);
    let (t2, p2, _) = open_tx(&lo2, &so2, &sl2, &sp, &|_, _| {});
    must("second open (the victim's order filled again at the same price)", &t2);
    let (l1, l2) = (live(&t1, 2, OPEN_DAA, &p1), live(&t2, 2, OPEN_DAA + 10, &p2));
    assert_ne!(l1.id, l2.id);
    assert_eq!(l1.s.short_key, x(&short_kp()));
    assert_eq!(l2.s.short_key, x(&short_kp()));
    (l1, l2)
}

/// The two pairs with both longs listed by the attacker at 10 KAS (the amends clear every maker: the states are equal).
fn two_listed_longs() -> (LPair, LPair) {
    let a = attacker();
    let (l1, l2) = two_pairs_against_one_short();
    let (at1, s1) = amend_tx(&l1, L, &a, ASK, 20);
    must("the attacker lists the long of pair 1", &at1);
    let (at2, s2) = amend_tx(&l2, L, &a, ASK, 20);
    must("the attacker lists the long of pair 2", &at2);
    assert_eq!(s1, s2, "the listed pairs have one state");
    (live(&at1, 0, 400, &s1), live(&at2, 0, 400, &s2))
}

/// The two pairs with both shorts listed by the victim at 10 KAS (for two donors of one P3).
fn two_listed_shorts() -> (LPair, LPair) {
    let (l1, l2) = two_pairs_against_one_short();
    let (at1, s1) = amend_tx(&l1, S, &short_kp(), ASK, 20);
    must("list short 1", &at1);
    let (at2, s2) = amend_tx(&l2, S, &short_kp(), ASK, 20);
    must("list short 2", &at2);
    (live(&at1, 0, 400, &s1), live(&at2, 0, 400, &s2))
}

/// The whole-pair transfer of the long of `lp` at `xp` to `ek`, bound `nb`, the part at output `c` (the args alone).
fn transfer_args(lp: &LPair, xp: i64, nb: i64, ek: &Keypair, c: i64) -> Vec<Arg> {
    vec![iv(L), iv(lp.s.size), iv(xp), iv(nb), bv(&x(ek)), bv(&[0; 32]), iv(c)]
}

/// The part a whole transfer of the long of `p` to `ek` at bound `nb` writes, naming `parent` as its maker.
fn long_part(p: &OPair, nb: i64, ek: &Keypair, parent: &LPair) -> OPair {
    let mut part = p.rest_of(p.size);
    part.bl = nb;
    part.long_key = x(ek);
    (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
    part.mk.parent = maker_of(&parent.op);
    part
}

// ---------------------------------------------------------------- R7-F1, the review's three routes

/// R7-F1, the party route, fixed. Two pairs share the victim as short; one transaction transfers both longs, each
/// whole, to the attacker's new key into ONE part. The part names one parent (it is one field): the pair it names
/// accepts, the other refuses, whichever is named. Before the fix both accepted and the builder kept pair 2's 3,000.2
/// KAS (the victim short lost 2,000 KAS of margin and half its position). The honest transfer of either pair alone is
/// accepted.
#[test]
fn holds_two_pairs_never_merge_into_one_part_by_transfer() {
    let a = attacker();
    let (l1, l2) = two_listed_longs();
    let ek = keypair(51);
    let nb = l1.s.bl;
    let pay = kpf_protocol::payoff::owed(l1.s.size, ASK - l1.s.bl, 0, l1.s.m.p.scale);
    let funding = 5_000 * KAS as u64;
    for (named, other) in [(0usize, 1usize), (1, 0)] {
        let parent = if named == 0 { &l1 } else { &l2 };
        let part = long_part(&l1.s, nb, &ek, parent);
        let part_v = part.range_of(part.size) + (l1.value as i64 - l1.s.range_of(l1.s.size));
        let ins = vec![
            pair_in(&l1, "transfer", transfer_args(&l1, ASK, nb, &ek, 2)),
            pair_in(&l2, "transfer", transfer_args(&l2, ASK, nb, &ek, 2)),
            key_in(outpoint(230), funding, 0, &a),
        ];
        let total_in = l1.value + l2.value + funding;
        let fixed = 2 * pay as u64 + part_v as u64;
        let mut outs = vec![
            out(pay as u64, key_spk(&l1.s.long_key), None),
            out(pay as u64, key_spk(&l2.s.long_key), None),
            out(part_v as u64, pair_spk(&part), None),
            out(total_in - fixed, p2pk_spk(&a), None),
        ];
        seal(&mut outs, 2, 0, l1.op);
        let t = build_ext(&ins, outs, 0, &[seq_quiet(&l1), seq_quiet(&l2), 0]);
        let v = verdicts(&t);
        assert!(v[named] && !v[other], "the part names input {named}: only it accepts ({v:?})");
    }
    must("pair 1 alone", &transfer_tx(&l1, L, ASK, nb, &ek, 20).0);
    must("pair 2 alone", &transfer_tx(&l2, L, ASK, nb, &ek, 20).0);
}

/// R7-F1 with partial hand-overs, fixed: 400 units of each long into one part (each remainder exact under its own id).
/// The pair the part does not name refuses.
#[test]
fn holds_partial_hand_overs_of_two_pairs_never_share_one_part() {
    let a = attacker();
    let (l1, l2) = two_listed_longs();
    let ek = keypair(51);
    let n = 400_000_000;
    let nb = l1.s.bl;
    let mut part = long_part(&l1.s, nb, &ek, &l1);
    part.size = n;
    let sc = l1.s.m.p.scale;
    let pay = kpf_protocol::payoff::owed(n, ASK - l1.s.bl, 0, sc);
    let part_v = part.range_of(n) + l1.s.m.p.carrier;
    let rest = |l: &LPair| (l.s.rest_of(l.s.size - n), l.s.rest_value(l.value, n));
    let (r1, v1) = rest(&l1);
    let (r2, v2) = rest(&l2);
    let args = |c: i64| vec![iv(L), iv(n), iv(ASK), iv(nb), bv(&x(&ek)), bv(&[0; 32]), iv(c)];
    let funding = 5_000 * KAS as u64;
    let ins = vec![pair_in(&l1, "transfer", args(2)), pair_in(&l2, "transfer", args(2)), key_in(outpoint(231), funding, 0, &a)];
    let total_in = l1.value + l2.value + funding;
    let fixed = 2 * pay as u64 + part_v as u64 + v1 as u64 + v2 as u64;
    let mut outs = vec![
        out(pay as u64, key_spk(&l1.s.long_key), None),
        out(pay as u64, key_spk(&l2.s.long_key), None),
        out(part_v as u64, pair_spk(&part), None),
        out(v1 as u64, pair_spk(&r1), bind(0, l1.id.unwrap())),
        out(v2 as u64, pair_spk(&r2), bind(1, l2.id.unwrap())),
        out(total_in - fixed, p2pk_spk(&a), None),
    ];
    seal(&mut outs, 2, 0, l1.op);
    let t = build_ext(&ins, outs, 0, &[seq_quiet(&l1), seq_quiet(&l2), 0]);
    refused("two partial hand-overs into one part", &t, 1);
    assert!(verdicts(&t)[0], "the named pair's own checks pass");
}

/// The restored half and its remainder of `exploit_underwriter...` (review 7): the underwriter's claim on half the pair,
/// refuted (review 11: a late claim is no longer void; a refuted one restores just as a void one did), restores the half
/// under the claim's id with the remainder's exact state.
fn restored_half_beside_its_remainder() -> (LPair, LPair) {
    let (lp, _, _) = opened();
    let half = lp.s.size / 2;
    let (t, w) = propose_tx(&lp, &liq(L, half, FALSE_P, 500, 500), &uw_kp(), None);
    must("a claim on half", &t);
    let lw = live(&t, 0, 500, &w);
    let rest = live(&t, 1, 500, &lp.s.rest_of(lp.s.size - half));
    let (ft, s, at) = refute(&lw, 501);
    let Settlement::Restore { part: back, defender, disputer, .. } = s else { panic!("{s:?}") };
    assert_eq!(defender + disputer, 300 * KAS, "the refuted claim's bond, to the long and the disputer");
    let back = live(&ft, 1, at, &back);
    assert_eq!(back.s, rest.s, "the restored half and the remainder have one state");
    assert_ne!(back.id, rest.id);
    (rest, back)
}

/// R7-F1, the underwriter's route, fixed: one liquidation of both halves vouched once with ONE pending output. The
/// pending part names one parent; the other half refuses, whichever is named. Before the fix both accepted and the
/// underwriter kept the other half's 1,500.2 KAS for a 300 KAS void bond. Each half alone is proposed as before.
#[test]
fn holds_underwriter_cannot_merge_two_halves_into_one_claim() {
    let (rest, back) = restored_half_beside_its_remainder();
    let half = rest.s.size;
    let k = kit();
    let (w2, fb) = rest.s.pending_of(k, half, LIQ, L, FALSE_P, 650).expect("a liquidation the pair accepts");
    assert_eq!(back.s.pending_of(k, half, LIQ, L, FALSE_P, 650).unwrap().0, w2, "both pairs would write one pending state but for the maker");
    let args = || vec![iv(half), iv(LIQ), iv(L), iv(FALSE_P), iv(650), iv(2), bv(&[0u8; 65]), iv(fb), iv(0), bv(&k.pending.prefix), bv(&k.pending.suffix)];
    for (named, other) in [(0usize, 1usize), (1, 0)] {
        let parent = if named == 0 { &rest } else { &back };
        let w = w2.made_by(&parent.op);
        let ins = vec![pair_in(&rest, "propose", args()), pair_in(&back, "propose", args()), key_in(outpoint(232), 500 * KAS as u64, 0, &uw_kp()), exec_in(233)];
        let pend_v = rest.value.max(back.value) + w.bond as u64;
        let loot = rest.value.min(back.value);
        let mut outs = vec![out(pend_v, pend_spk(&w), None), out(loot, key_spk(&x(&uw_kp())), None), change(90_000_000_000)];
        seal(&mut outs, 0, named as u16, parent.op);
        let t = build_ext(&ins, outs, 650, &[0, 0, 0, 0]);
        let v = verdicts(&t);
        assert!(v[named] && !v[other], "the pending part names input {named}: only it accepts ({v:?})");
    }
    must("the remainder alone", &propose_tx(&rest, &liq(L, half, FALSE_P, 650, 650), &uw_kp(), None).0);
    must("the restored half alone", &propose_tx(&back, &liq(L, half, FALSE_P, 650, 650), &uw_kp(), None).0);
}

// ---------------------------------------------------------------- the review's unproven routes

/// Close-versus-close with several hosts and donors naming one P3 at output `c3` (each host's long and each donor's
/// short paid at its own input index; the executor last): in hosts, then donors, then the executor; out the payouts,
/// P3 at the executor's index, change. P3's makers are `mk`; the hosts are handed `dm`, the donors `hm`.
fn cvc_many(hosts: &[&LPair], donors: &[&LPair], xp: i64, mk: Makers, dm: [u8; 32], hm: [u8; 32]) -> T {
    let (h, d) = (&hosts[0].s, &donors[0].s);
    let n = h.size;
    let e3 = h.entry;
    let mut p3 = h.rest_of(n);
    p3.bl = d.bl;
    p3.entry = e3;
    p3.long_key = d.long_key;
    (p3.long_ask, p3.long_min, p3.long_tip) = (d.long_ask, d.long_min, d.long_tip);
    p3.mk = mk;
    let sc = h.m.p.scale;
    let c3 = (hosts.len() + donors.len()) as i64;
    let host_args = vec![iv(n), iv(xp), iv(e3), iv(c3), iv(d.bl), bv(&d.long_key), iv(d.long_ask), iv(d.long_min), iv(d.long_tip), bv(&dm)];
    let donor_args = vec![iv(n), iv(xp), iv(e3), iv(c3), iv(h.bs), bv(&h.short_key), iv(h.short_bid), iv(h.short_min), iv(h.short_tip), bv(&hm)];
    let mut ins = vec![];
    let mut outs = vec![];
    let mut seqs = vec![];
    for lh in hosts {
        ins.push(pair_in(lh, "closeHost", host_args.clone()));
        outs.push(out(kpf_protocol::payoff::owed(n, xp - lh.s.bl, lh.s.long_tip, sc) as u64, key_spk(&lh.s.long_key), None));
        seqs.push(seq_quiet(lh));
    }
    for ld in donors {
        ins.push(pair_in(ld, "closeDonor", donor_args.clone()));
        let spay = kpf_protocol::payoff::owed(n, ld.s.bs - xp, ld.s.short_tip, sc) + ld.value as i64 - ld.s.range_of(n);
        outs.push(out(spay as u64, key_spk(&ld.s.short_key), None));
        seqs.push(seq_quiet(ld));
    }
    ins.push(exec_in(240));
    seqs.push(0);
    let keep_h = hosts[0].value as i64 - h.range_of(n);
    outs.push(out((p3.range_of(n) + keep_h) as u64, pair_spk(&p3), None));
    outs.push(change(90_000_000_000));
    seal(&mut outs, c3 as usize, 0, hosts[0].op);
    build_ext(&ins, outs, 0, &seqs)
}

/// A host with its long listed, and a donor with its short listed, against which the twins of this file close.
fn listed_host_and_donor() -> (LPair, LPair) {
    let (lp, _, _) = opened();
    let (at, h) = amend_tx(&lp, L, &long_kp(), ASK, 20);
    must("list the host's long", &at);
    let (lp2, _, _) = opened();
    let (at2, d) = amend_tx(&lp2, S, &short_kp(), ASK, 20);
    must("list the donor's short", &at2);
    (live(&at, 0, 400, &h), live(&at2, 0, 400, &d))
}

/// The review's unproven `closeHost` merge, built: two identical hosts (the attacker's two listed longs against one
/// victim short) and one donor writing ONE P3. P3 names one host (one field): the other host refuses. The honest
/// close-versus-close of one host and the donor is accepted.
#[test]
fn holds_two_hosts_never_share_one_p3() {
    let (h1, h2) = two_listed_longs();
    let (_, donor) = listed_host_and_donor();
    let mk = Makers { host: maker_of(&h1.op), donor: maker_of(&donor.op), ..Makers::default() };
    let t = cvc_many(&[&h1], &[&donor], ASK, mk, maker_of(&donor.op), maker_of(&h1.op));
    must("one host and the donor", &t);
    let t = cvc_many(&[&h1, &h2], &[&donor], ASK, mk, maker_of(&donor.op), maker_of(&h1.op));
    let v = verdicts(&t);
    assert!(v[0] && !v[1] && v[2], "P3 names host 1: host 2 refuses ({v:?})");
}

/// Two donors into one P3 with one host: P3 names one donor; the other refuses.
#[test]
fn holds_two_donors_never_share_one_p3() {
    let (d1, d2) = two_listed_shorts();
    let (host, _) = listed_host_and_donor();
    let mk = Makers { host: maker_of(&host.op), donor: maker_of(&d1.op), ..Makers::default() };
    must("the host and one donor", &cvc_many(&[&host], &[&d1], ASK, mk, maker_of(&d1.op), maker_of(&host.op)));
    let t = cvc_many(&[&host], &[&d1, &d2], ASK, mk, maker_of(&d1.op), maker_of(&host.op));
    let v = verdicts(&t);
    assert!(v[0] && v[1] && !v[2], "P3 names donor 1: donor 2 refuses ({v:?})");
}

/// T2 with extra pairs: in 0 the pair `lp` (transfer to the order), 1 the order (take), 2 the slot (bind of the part
/// at its expiry), then `extra` pairs each transferring its long into the same part, then the executor; out 0 the
/// leaver, 1 the order's continuation, 2 the part, then each extra pair's leaver at its input index, the slot, change.
fn take_with_extra(lp: &LPair, extra: &[&LPair], lo: &LOrder, sl: &LSlot, xp: i64, lock: u64) -> T {
    let k = kit();
    let (p, o) = (&lp.s, &lo.s);
    let n = p.size;
    let mut part = p.rest_of(n);
    part.bl = xp - o.my_rate;
    part.long_key = o.owner;
    (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
    part.mk.long = maker_of(&lo.op);
    part.mk.parent = maker_of(&lp.op);
    let pay = kpf_protocol::payoff::owed(n, xp - p.bl, p.long_tip, p.m.p.scale);
    let cover = p.cover_of(n);
    let a = BindArgs::new(lock as i64, p.b.expiry, 0, &[Claim::output(2, cover)]);
    let ctx = Ctx { lock: lock as i64, seq: 10, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true };
    let b2 = sl.s.bind(&ctx, &a, sl.value as i64).expect("the slot counts the part");
    let take_args = vec![
        iv(2), iv(n), iv(xp), iv(p.entry), iv(p.bs), bv(&p.short_key), bv(&maker_of(&lp.op)), iv(p.short_bid), iv(p.short_min), iv(p.short_tip),
        iv(2), iv(0), iv(p.b.expiry), iv(p.b.cover_rate), iv(p.b.premium), bv(&k.pair.prefix), bv(&k.pair.suffix),
    ];
    let targs = vec![iv(L), iv(n), iv(xp), iv(part.bl), bv(&o.owner), bv(&maker_of(&lo.op)), iv(2)];
    let mut ins = vec![pair_in(lp, "transfer", targs.clone()), order_in(lo, "take", take_args), slot_in(sl, "bind", bind_args(&a, &uw_kp()))];
    let mut seqs = vec![seq_quiet(lp), 0, 10];
    for e in extra {
        ins.push(pair_in(e, "transfer", targs.clone()));
        seqs.push(seq_quiet(e));
    }
    ins.push(exec_in(241));
    seqs.push(0);
    let (oc, ov) = order_after(lo, n, o.take_spend(n));
    let keep = lp.value as i64 - p.range_of(n);
    let mut outs = vec![
        out(pay as u64, key_spk(&p.long_key), None),
        out(ov, order_spk(&oc.expect("the order rests on")), bind(1, lo.id.unwrap())),
        out((part.range_of(n) + keep) as u64, pair_spk(&part), None),
    ];
    // Each extra pair's leaver at its own input index (3, 4, ...), so that only the maker can refuse it.
    for e in extra {
        outs.push(out(kpf_protocol::payoff::owed(n, xp - e.s.bl, e.s.long_tip, p.m.p.scale) as u64, key_spk(&e.s.long_key), None));
    }
    outs.push(out(sl.value, bond_spk(&b2), bind(2, sl.id.unwrap())));
    outs.push(change(90_000_000_000));
    seal(&mut outs, 2, 0, lp.op);
    build_ext(&ins, outs, lock, &seqs)
}

/// The review's unproven merge inside an order's `take`, built: two identical pairs transferring their longs into the
/// one part a resting long order takes (with the slot's bind). The part names one parent: the second pair refuses.
/// The honest take of one pair is accepted.
#[test]
fn holds_two_pairs_never_merge_into_the_part_an_order_takes() {
    let (l1, l2) = two_listed_longs();
    let lo = order(L, &keypair(46), 0x7a, 0);
    let mut sl = slot(K);
    sl.value = 3 * l1.s.cover_of(l1.s.size) as u64;
    must("the honest take", &take_with_extra(&l1, &[], &lo, &sl, ASK, 400));
    let t = take_with_extra(&l1, &[&l2], &lo, &sl, ASK, 400);
    let v = verdicts(&t);
    assert!(v[0] && v[1] && v[2] && !v[3], "the part names pair 1 and the order: pair 2 refuses ({v:?})");
}

/// The review's unproven share of an order's `fill` output with a pair's hand-over part, built: a long order fills a
/// pair at output 2 that is also the part a listed pair's long transfer writes to the order's owner (same position,
/// binding, expiry; the stayer's short unlisted). The two entries demand different makers (the fill: no parent; the
/// transfer: itself as parent), so with either one's makers the other refuses: the order's escrow can never be paid
/// into a pair that is already funded.
#[test]
fn holds_an_order_fill_never_shares_its_pair_with_a_hand_over_part() {
    let (lp, sl, _) = opened();
    let (at, listed) = amend_tx(&lp, L, &long_kp(), ASK, 20);
    must("list", &at);
    let lp = live(&at, 0, 400, &listed);
    let p = &lp.s;
    let lo = order(L, &keypair(46), 0x7a, 0);
    let o = &lo.s;
    let k = kit();
    let n = p.size;
    let bl = ASK - o.my_rate;
    let cover = p.cover_of(n);
    let a = BindArgs::new(400, p.b.expiry, 0, &[Claim::output(2, cover)]);
    let ctx = Ctx { lock: 400, seq: 10, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true };
    let b2 = sl.s.bind(&ctx, &a, sl.value as i64).expect("bind");
    let fill_args = vec![
        iv(2), iv(n), iv(ASK), iv(bl), iv(p.bs), bv(&p.short_key), bv(&[0; 32]), iv(2), iv(0), iv(p.b.expiry), iv(p.b.cover_rate), iv(p.b.premium),
        bv(&k.pair.prefix), bv(&k.pair.suffix),
    ];
    let targs = vec![iv(L), iv(n), iv(ASK), iv(bl), bv(&o.owner), bv(&maker_of(&lo.op)), iv(2)];
    let pay = kpf_protocol::payoff::owed(n, ASK - p.bl, p.long_tip, p.m.p.scale);
    let mut part = p.rest_of(n);
    part.bl = bl;
    part.long_key = o.owner;
    (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
    let fill_mk = Makers { long: maker_of(&lo.op), ..Makers::default() };
    let hand_mk = Makers { long: maker_of(&lo.op), parent: maker_of(&lp.op), ..Makers::default() };
    for (mk, refuses) in [(hand_mk, 1usize), (fill_mk, 0usize)] {
        part.mk = mk;
        let ins = vec![pair_in(&lp, "transfer", targs.clone()), order_in(&lo, "fill", fill_args.clone()), slot_in(&sl, "bind", bind_args(&a, &uw_kp())), exec_in(242)];
        let (oc, ov) = order_after(&lo, n, o.fill_spend(n, p.b.premium));
        let mut outs = vec![
            out(pay as u64, key_spk(&p.long_key), None),
            out(ov, order_spk(&oc.expect("rests on")), bind(1, lo.id.unwrap())),
            out((part.range_of(n) + lp.value as i64 - p.range_of(n)) as u64, pair_spk(&part), None),
            out(sl.value, bond_spk(&b2), bind(2, sl.id.unwrap())),
            change(90_000_000_000),
        ];
        seal(&mut outs, 2, 0, lp.op);
        let t = build_ext(&ins, outs, 400, &[seq_quiet(&lp), 0, 10, 0]);
        let v = verdicts(&t);
        assert!(!v[refuses] && v[1 - refuses], "with the {} makers the other entry refuses ({v:?})", if refuses == 1 { "hand-over's" } else { "fill's" });
    }
}

/// The review's unproven vouch question, explored: one vouch input cannot cover proposals of two underwriters (each
/// propose requires its own `uwKey`'s P2PK input); with each underwriter's own vouch both proposals land, each into its
/// own pending part naming its own pair, and each underwriter signed every output (harmless).
#[test]
fn holds_one_vouch_never_covers_a_proposal_of_another_underwriter() {
    let (lp, _, _) = opened();
    let mut other = lp.s.clone();
    other.b.uw_key = x(&keypair(32));
    let lb = genesis_pair(other, lp.value, 0x5d, 400);
    let k = kit();
    let c = liq(L, lp.s.size, FALSE_P, 450, 450);
    let (wa, fa) = lp.s.pending_of(k, c.n, c.kind, c.side, c.p, c.t0).unwrap();
    let (wb, fbb) = lb.s.pending_of(k, c.n, c.kind, c.side, c.p, c.t0).unwrap();
    let (wa, wb) = (wa.made_by(&lp.op), wb.made_by(&lb.op));
    let args = |j: i64, fb: i64, kk: i64| vec![iv(c.n), iv(c.kind), iv(c.side), iv(c.p), iv(c.t0), iv(j), bv(&[0u8; 65]), iv(fb), iv(kk), bv(&k.pending.prefix), bv(&k.pending.suffix)];
    let build = |jb: i64, second: Option<&Keypair>| {
        let mut ins = vec![pair_in(&lp, "propose", args(2, fa, 0)), pair_in(&lb, "propose", args(jb, fbb, 1)), key_in(outpoint(243), 50_000_000_000, 0, &uw_kp())];
        if let Some(kp) = second {
            ins.push(key_in(outpoint(244), 50_000_000_000, 0, kp));
        }
        ins.push(exec_in(245));
        let mut outs = vec![out(lp.s.pending_value(lp.value, &wa) as u64, pend_spk(&wa), None), out(lb.s.pending_value(lb.value, &wb) as u64, pend_spk(&wb), None), change(90_000_000_000)];
        seal(&mut outs, 0, 0, lp.op);
        seal(&mut outs, 1, 1, lb.op);
        let seqs = vec![0u64; ins.len()];
        build_ext(&ins, outs, 450, &seqs)
    };
    let v = verdicts(&build(2, None));
    assert!(v[0] && !v[1], "the one vouch is the first underwriter's: the second pair refuses ({v:?})");
    must("each underwriter's own vouch", &build(3, Some(&keypair(32))));
}

// ---------------------------------------------------------------- every role, twins, stale makers

/// The maker requirement of each creating entry, field by field (long, short, parent, host, donor): `Own` (this input's
/// commitment), `Co` (a co-maker's commitment given as an argument, checked by that co-maker), `Zero`. This is the
/// contract text of `KpfOpenO.fill`, `KpfOpenO.take`, `KpfPairO.transfer` (`handOver`), `closeHost`, `closeDonor`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Req {
    Own,
    Co,
    Zero,
}
use Req::*;

fn entry_reqs() -> Vec<(&'static str, [Req; 5])> {
    vec![
        ("fill by a long order", [Own, Co, Zero, Zero, Zero]),
        ("fill by a short order", [Co, Own, Zero, Zero, Zero]),
        ("take by a long order", [Own, Zero, Co, Zero, Zero]),
        ("take by a short order", [Zero, Own, Co, Zero, Zero]),
        ("transfer of the long", [Co, Zero, Own, Zero, Zero]),
        ("transfer of the short", [Zero, Co, Own, Zero, Zero]),
        ("closeHost", [Zero, Zero, Zero, Own, Co]),
        ("closeDonor", [Zero, Zero, Zero, Co, Own]),
    ]
}

/// Two distinct inputs can accept one output only if no field is `Own` for both (one field, two commitments) and no
/// field is `Own` for one and `Zero` for the other (a commitment is not zero).
fn compatible(a: &[Req; 5], b: &[Req; 5]) -> bool {
    a.iter().zip(b).all(|(x, y)| !matches!((x, y), (Own, Own) | (Own, Zero) | (Zero, Own)))
}

/// Every pair of creating entries (the same entry twice included: two inputs in one role) on one output: only the
/// documented compositions are compatible, and no three inputs ever are. The engine tests of this file and of
/// `oracle_tests`/`audit6_tests` exercise each row: two pairs by transfer (parent, parent), two hosts, two donors, two
/// long orders filling one pair, two orders taking one part, a fill sharing a hand-over part, the honest open, take and
/// close-versus-close.
#[test]
fn holds_two_inputs_of_any_two_roles_accept_one_output_only_in_the_documented_compositions() {
    let r = entry_reqs();
    let legit = [
        ("fill by a long order", "fill by a short order"),
        ("take by a long order", "transfer of the long"),
        ("take by a short order", "transfer of the short"),
        ("closeHost", "closeDonor"),
    ];
    for (i, (na, a)) in r.iter().enumerate() {
        for (nb, b) in r.iter().skip(i) {
            let ok = compatible(a, b);
            let documented = legit.iter().any(|(x, y)| (x == na && y == nb) || (x == nb && y == na));
            assert_eq!(ok, documented, "{na} + {nb}: compatible {ok}, documented {documented}");
        }
    }
    for a in &r {
        for b in &r {
            for c in &r {
                assert!(!(compatible(&a.1, &b.1) && compatible(&b.1, &c.1) && compatible(&a.1, &c.1)), "three inputs {} + {} + {}", a.0, b.0, c.0);
            }
        }
    }
}

/// Two long orders filling one pair output (one long maker field): the order the pair does not name refuses. Two orders
/// that are twins of one covenant id (one genesis group) cannot even be inputs together (`OpCovInputCount(selfId) == 1`).
#[test]
fn holds_two_long_orders_never_fill_one_pair() {
    let a = attacker();
    let (lo1, so, sl) = (order(L, &a, 0x73, 0), order(S, &short_kp(), 0x72, 0), slot(K));
    let lo2 = order(L, &a, 0x75, 0);
    let sp = open_spec();
    let (t, p, _) = open_tx(&lo1, &so, &sl, &sp, &|_, _| {});
    must("the honest open", &t);
    // Rebuild the open with lo2 as a third input running the same fill into output 2 (its continuation after the
    // executor).
    let k = kit();
    let fill_args = |ck: [u8; 32], ct: [u8; 32]| -> Vec<Arg> {
        vec![iv(2), iv(sp.n), iv(sp.x), iv(p.bl), iv(p.bs), bv(&ck), bv(&ct), iv(2), iv(0), iv(sp.xe), iv(sp.cover_rate), iv(sp.premium), bv(&k.pair.prefix), bv(&k.pair.suffix)]
    };
    let cover = q(sp.n, sp.cover_rate, p.m.p.scale);
    let ba = BindArgs::new(sp.lock as i64, sp.xe, sp.advance, &[Claim::output(2, cover)]);
    let ins = vec![
        order_in(&lo1, "fill", fill_args(so.s.owner, maker_of(&so.op))),
        order_in(&so, "fill", fill_args(lo1.s.owner, maker_of(&lo1.op))),
        slot_in(&sl, "bind", bind_args(&ba, &uw_kp())),
        exec_in(200),
        order_in(&lo2, "fill", fill_args(so.s.owner, maker_of(&so.op))),
    ];
    let mut outs = t.0.outputs.clone();
    let (c2, v2) = order_after(&lo2, sp.n, lo2.s.fill_spend(sp.n, sp.premium));
    outs.insert(4, out(v2, order_spk(&c2.unwrap()), bind(4, lo2.id.unwrap())));
    outs.truncate(7);
    let t2 = build_ext(&ins, outs, sp.lock, &[0, 0, 10, 0, 0]);
    let v = verdicts(&t2);
    assert!(v[0] && v[1] && !v[4], "the pair names long order 1: order 2 refuses ({v:?})");
    // Twins of one id: a genesis group of two identical orders.
    let gins = vec![key_in(outpoint(0x41), 4_100_000_000_000, 0, &a)];
    let mut gouts = vec![out(lo1.value, order_spk(&lo1.s), None), out(lo1.value, order_spk(&lo1.s), None), out(90_000_000_000, p2pk_spk(&a), None)];
    let g = genesis_id(outpoint(0x41), &gouts, &[0, 1]);
    gouts[0].covenant = bind(0, g);
    gouts[1].covenant = bind(0, g);
    let gt = build_ext(&gins, gouts, 0, &[0]);
    must("a genesis of two twin orders", &gt);
    let (tw1, tw2) = (live(&gt, 0, 0, &lo1.s), live(&gt, 1, 0, &lo1.s));
    assert_eq!(tw1.id, tw2.id);
    let (t3, _, _) = open_tx(&tw1, &so, &sl, &sp, &|_, _| {});
    must("one twin opens a pair", &t3);
    let ins = vec![
        order_in(&tw1, "fill", fill_args(so.s.owner, maker_of(&so.op))),
        order_in(&so, "fill", fill_args(tw1.s.owner, maker_of(&tw1.op))),
        slot_in(&sl, "bind", bind_args(&ba, &uw_kp())),
        exec_in(200),
        order_in(&tw2, "fill", fill_args(so.s.owner, maker_of(&so.op))),
    ];
    let mut outs = t3.0.outputs.clone();
    outs.truncate(6);
    let t4 = build_ext(&ins, outs, sp.lock, &[0, 0, 10, 0, 0]);
    assert!(!accepted(&t4), "twin orders of one id in one transaction");
}

/// Twins of one pair id (a genesis group of two identical look-alike pairs, the long listed) transferring into one
/// part: both inputs carry the id, so neither passes `OpCovInputCount(selfId) == 1`, and the part names one of them
/// anyway. (Split parts of one id do not exist in this family: every split's part starts a fresh id.)
#[test]
fn holds_twin_pairs_of_one_id_never_merge() {
    let a = attacker();
    let (lp, _, _) = opened();
    let mut gp = lp.s.clone();
    (gp.long_ask, gp.mk) = (ASK, Makers::default());
    let ins = vec![key_in(outpoint(0x42), 2 * lp.value + 1_000_000_000_000, 0, &a)];
    let mut outs = vec![out(lp.value, pair_spk(&gp), None), out(lp.value, pair_spk(&gp), None), out(900_000_000_000, p2pk_spk(&a), None)];
    let g = genesis_id(outpoint(0x42), &outs, &[0, 1]);
    outs[0].covenant = bind(0, g);
    outs[1].covenant = bind(0, g);
    let gt = build_ext(&ins, outs, 0, &[0]);
    must("a genesis of twin pairs", &gt);
    let (t1, t2) = (live(&gt, 0, 380, &gp), live(&gt, 1, 380, &gp));
    let ek = keypair(51);
    let part = long_part(&gp, gp.bl, &ek, &t1);
    let pay = kpf_protocol::payoff::owed(gp.size, ASK - gp.bl, 0, gp.m.p.scale);
    let ins = vec![pair_in(&t1, "transfer", transfer_args(&t1, ASK, gp.bl, &ek, 2)), pair_in(&t2, "transfer", transfer_args(&t2, ASK, gp.bl, &ek, 2)), key_in(outpoint(0x43), 900_000_000_000, 0, &a)];
    let mut outs = vec![out(pay as u64, key_spk(&gp.long_key), None), out(pay as u64, key_spk(&gp.long_key), None), out(t1.value, pair_spk(&part), None), out(100_000_000_000, p2pk_spk(&a), None)];
    seal(&mut outs, 2, 0, t1.op);
    let t = build_ext(&ins, outs, 0, &[20, 20, 0]);
    let v = verdicts(&t);
    assert!(!v[0] && !v[1], "twins of one id refuse together ({v:?})");
    must("one twin alone hands over", &transfer_tx(&t1, L, ASK, gp.bl, &ek, 20).0);
}

/// Makers live for one transaction. (1) No entry copies them: the part a transfer wrote (naming its parent) amends into
/// a continuation with every maker zero, and a continuation that keeps them is refused; the same for a contested
/// pending part (contest writes its maker zero). (2) A stale maker field never satisfies a later input: a look-alike
/// written earlier with the very part pair Q's transfer would write (naming Q) is no output of Q's transaction; spent
/// beside Q, its continuation carries its own id (not fresh) and zero makers, so Q refuses; and Q's transfer with no
/// part of its own is refused. A maker is checked only by its own input, in the transaction that spends it, against
/// that transaction's outputs; an outpoint is spent once, so its commitment can never be checked again.
#[test]
fn holds_a_stale_maker_field_never_satisfies_a_later_input() {
    let (lp, _, _) = opened();
    let (at, listed) = amend_tx(&lp, L, &long_kp(), ASK, 20);
    must("list", &at);
    let q = live(&at, 0, 400, &listed);
    let ek = keypair(51);
    let (tt, part) = transfer_tx(&q, L, ASK, q.s.bl, &ek, 20);
    must("the hand-over", &tt);
    assert_eq!(part.mk.parent, maker_of(&q.op));
    let np = live(&tt, 1, 450, &part);
    // (1) amend of the fresh part: makers zero in the continuation; keeping them is refused.
    let (am, c) = amend_tx(&np, L, &ek, 0, 20);
    must("amend clears the makers", &am);
    assert!(c.mk.is_zero());
    let kept = OPair { mk: part.mk, ..c.clone() };
    let ins = vec![pair_in(&np, "amend", vec![Arg::Sig(ek), iv(L), iv(0), iv(0), iv(0)]), exec_in(222)];
    let t = build_ext(&ins, vec![out(np.value, pair_spk(&kept), bind(0, np.id.unwrap())), change(90_000_000_000)], 0, &[20, 0]);
    refused("an amend copying the makers", &t, 0);
    // (2) Q's would-be part, written earlier by a genesis (a look-alike naming Q), spent beside Q by its own amend.
    let (at2, listed2) = amend_tx(&lp, L, &long_kp(), ASK, 20);
    let q2 = live(&at2, 0, 400, &listed2);
    let stale = long_part(&q2.s, q2.s.bl, &ek, &q2);
    let x_ = genesis_pair(stale.clone(), q2.value, 0x5e, 400);
    let mut xc = OPair { mk: Makers::default(), ..stale.clone() };
    xc.long_ask = 0;
    let pay = kpf_protocol::payoff::owed(q2.s.size, ASK - q2.s.bl, 0, q2.s.m.p.scale);
    let ins = vec![pair_in(&q2, "transfer", transfer_args(&q2, ASK, q2.s.bl, &ek, 1)), pair_in(&x_, "amend", vec![Arg::Sig(ek), iv(L), iv(0), iv(0), iv(0)]), key_in(outpoint(0x44), 900_000_000_000, 0, &ek)];
    let outs = vec![out(pay as u64, key_spk(&q2.s.long_key), None), out(x_.value, pair_spk(&xc), bind(1, x_.id.unwrap())), out(100_000_000_000, p2pk_spk(&ek), None)];
    let t = build_ext(&ins, outs, 0, &[20, 20, 0]);
    let v = verdicts(&t);
    assert!(!v[0] && v[1], "Q refuses the look-alike's continuation ({v:?})");
    // Q's transfer with no output of its own (the look-alike merely exists): refused.
    let ins = vec![pair_in(&q2, "transfer", transfer_args(&q2, ASK, q2.s.bl, &ek, 1)), key_in(outpoint(0x45), 900_000_000_000, 0, &ek)];
    let outs = vec![out(pay as u64, key_spk(&q2.s.long_key), None), out(100_000_000_000, p2pk_spk(&ek), None)];
    refused("a transfer relying on an earlier output", &build_ext(&ins, outs, 0, &[20, 0]), 0);
}

/// The judge's deliveries keep working, and the pending part's maker lives for one transaction: contest writes it
/// zero (a contested part that keeps its parent's commitment is refused), and the honest contest, defence, decide and
/// finalize run unchanged.
#[test]
fn holds_contest_clears_the_pending_parts_maker() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p: FALSE_P, t0: 450, lock: 450 }, 500);
    assert_eq!(lw.s.mk_parent, maker_of(&lp.op));
    let st = stake(70, 10 * kpf_protocol::judge::UNIT as u64, 0, 0x46);
    let (t, w2, _, _) = contest_tx(&lw, &st, 70, 520);
    must("the honest contest", &t);
    assert_eq!(w2.mk_parent, [0; 32]);
    // The same contest with the maker kept in the continuation (and a dispute naming that continuation).
    let id = lw.id.unwrap();
    let kept = OPending { mk_parent: lw.s.mk_parent, ..w2.clone() };
    let jm = lw.s.m.judge_market();
    let receipt = kpf_protocol::judge::Js::receipt(st.s.owner, jm, kpf_protocol::judge::NO, st.value as i64, 0, lw.daa as i64);
    let d = kpf_protocol::judge::Js::opened(jm, spk_hash(&pend_spk(&kept)), lw.daa as i64, st.value as i64);
    let ins = vec![pend_in(&lw, "contest", vec![b8(1), b8(2)]), stake_in(&st, "open", vec![b8(0), b8(0), b8(2), Arg::Sig(keypair(70))]), exec_in(212)];
    let outs = vec![out(lw.value, pend_spk(&kept), bind(0, id)), out(st.value, stake_spk(&receipt), bind(0, id)), out(DISPUTE_BASE, stake_spk(&d), bind(0, id)), change(90_000_000_000)];
    refused("a contested part keeping its maker", &build_ext(&ins, outs, 520, &[0, 0, 0]), 0);
}

/// The fresh-id scan and the maker are both required: a part under a fresh id naming another input (here: zero, as a
/// builder who drops the maker would write it) is refused, as is a part naming its maker but keeping the pair's id.
#[test]
fn holds_the_maker_and_the_fresh_scan_are_both_required() {
    let (lp, _, _) = opened();
    let (at, listed) = amend_tx(&lp, L, &long_kp(), ASK, 20);
    must("list", &at);
    let q = live(&at, 0, 400, &listed);
    let ek = keypair(51);
    refused("a part keeping the pair's id", &transfer_tx_with(&q, L, ASK, q.s.bl, &ek, 20, Lineage::Keep).0, 0);
    let mut nameless = long_part(&q.s, q.s.bl, &ek, &q);
    nameless.mk = Makers::default();
    let pay = kpf_protocol::payoff::owed(q.s.size, ASK - q.s.bl, 0, q.s.m.p.scale);
    let ins = vec![pair_in(&q, "transfer", transfer_args(&q, ASK, q.s.bl, &ek, 1)), key_in(outpoint(0x47), 900_000_000_000, 0, &ek)];
    let mut outs = vec![out(pay as u64, key_spk(&q.s.long_key), None), out(q.value, pair_spk(&nameless), None), out(100_000_000_000, p2pk_spk(&ek), None)];
    seal(&mut outs, 1, 0, q.op);
    refused("a fresh part naming no maker", &build_ext(&ins, outs, 0, &[20, 0]), 0);
}

// ---------------------------------------------------------------- kept from the review

/// The fresh-id scan itself: a ninth output is refused.
#[test]
fn holds_the_fresh_scan_refuses_a_ninth_output() {
    let a = attacker();
    let (l1, _) = two_listed_longs();
    let (t, _) = transfer_tx(&l1, L, ASK, l1.s.bl, &keypair(51), 20);
    must("an ordinary transfer", &t);
    let mut outs = t.0.outputs.clone();
    for _ in 0..6 {
        outs.push(out(2_000_000, p2pk_spk(&a), None));
    }
    seal(&mut outs, 1, 0, l1.op);
    let ins = vec![pair_in(&l1, "transfer", vec![iv(L), iv(l1.s.size), iv(ASK), iv(l1.s.bl), bv(&x(&keypair(51))), bv(&[0; 32]), iv(1)]), key_in(outpoint(223), 500_000_000_000, 0, &keypair(51))];
    let t9 = build_ext(&ins, outs, 0, &[20, 0]);
    refused("a hand-over with nine outputs", &t9, 0);
}

/// Two orders of one side never compute one new pair: each names its own outpoint as the pair's maker.
#[test]
fn holds_two_orders_never_share_one_new_pair() {
    let a = attacker();
    let (lo1, lo2, so, sl) = (order(L, &a, 0x73, 0), order(L, &a, 0x74, 0), order(S, &short_kp(), 0x72, 0), slot(K));
    let p = opened_pair(&lo1, &so, &sl, &open_spec());
    let p2 = opened_pair(&lo2, &so, &sl, &open_spec());
    assert_ne!(pair_spk(&p), pair_spk(&p2), "the long order's outpoint commitment is in the pair it writes");
    assert_eq!((p.mk.long, p.mk.short), (maker_of(&lo1.op), maker_of(&so.op)));
}

/// Renewal after the lapse settle is due (point 5 of the review's brief): the underwriter's counted renewal moves the
/// expiry of a lapsed pair and so cancels a due lapse settle (documented in DESIGN-ORACLE-IMPL 24.2 (c)); a party cannot.
#[test]
fn holds_only_the_underwriters_counted_renewal_cancels_a_due_lapse_settle() {
    let (lp, sl, _) = opened();
    let due = lp.s.lapse_from();
    assert_eq!(due, 1_200 + 200 + 1_000);
    must("lapse settle due", &lapse_tx(&lp, due, 0));
    let (t, r, _) = renew_tx(&lp, &sl, due, 10);
    must("a renewal at the DAA the lapse settle is due (documented 24.2 (c))", &t);
    let rl = live(&t, 0, due, &r);
    assert_eq!(rl.s.b.expiry, 2_000);
    assert!(!accepted(&lapse_tx(&rl, due, 0)), "after the renewal the lapse settle is not due");
    let (at, c) = amend_tx(&lp, S, &short_kp(), 0, 20);
    must("amend", &at);
    must("lapse settle after an amend", &lapse_tx(&live(&at, 0, due, &c), due, 0));
}

#[allow(dead_code)]
fn _unused(_: TransactionOutput) {}
