//! Review 9 (docs/audit/REVIEW-9.md): the oracle-market family at commit `7d9024e` (after the fix of review 8). Every
//! transaction here runs every input in the real script engine with the covenant context (`common::run`).
//! `r9_exploit_` tests execute an attack that the contracts accept; `r9_holds_` tests assert that an attack is refused
//! (or that a shape keeps the rule it is meant to keep). Worked numbers use the fixture market of `oraclekit` (term 800,
//! step 100, tail 200, idle 1,000, W 200, quiet 20, mmr 5 %, g 2 %, a claim's bond 10 % of the cover, carrier 0.2 KAS).
//!
//! R9-F1: `KpfPairO.renew` relates the expiry to the slot's window only (`x2 = X + term <= t + term + step`, a lower
//! bound on the present), never to the present itself, so a pair whose expiry lies in the past is renewed back to
//! back, one premium per renewal, until its margins are gone. `KpfOpenO.take` without a pair puts any expiry the slot's
//! ring holds into the part (no lower bound), and `fill` only bounds it by the order's own age; a slot written by a
//! genesis with ring position 0 holds the expiries 100 .. 1,200 for ever.
//!
//! R9-F2: the same missing rule on an honest pair: a silent underwriter renews late, twice at one DAA (charging a term
//! in which no claim was valid), or after the lapse settle became due, which then is no longer due.
//!
//! R9-F3: the headroom between the range and the cover rate is shared: either party (or an entrant) takes it whole,
//! after which the other side can neither add margin nor be offered a cure.
//!
//! R9-N1: a valid partial liquidation does not refund the underwriter's carrier (DESIGN-ORACLE-IMPL 26.4 says it does).
//!
//! The seeded walk (lead 5) is at the end of the file.

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use common::*;
use kaspa_consensus_core::Hash;
use kpf_protocol::bond::{BindArgs, Bond, Claim, Ctx};
use kpf_protocol::judge::{Js, NO, STATUS_RESOLVED, UNIT, YES};
use kpf_protocol::oracle::*;
use oraclekit::*;
use secp256k1::Keypair;

const KAS: i64 = 100_000_000;

fn slot_ctx(sl: &LSlot, lock: u64) -> Ctx {
    Ctx { lock: lock as i64, seq: 10, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true }
}

/// `KpfOpenO.take` with no pair in the transaction (DESIGN-ORACLE-IMPL 26.2 "without a pair: binds only the order"):
/// in 0 the order (take, all it has left), 1 the slot (bind, claim OUT_REF + 1 at `xe`), 2 the counterparty's funding
/// (P2PK of `ck`, it authors the part's genesis); out 0 the order's refund, 1 the part (fresh id), 2 the slot, 3 the
/// counterparty's change. The counterparty's side is `bound` (a long order: the short's bound), the entry `e`.
#[allow(clippy::too_many_arguments)]
fn take_alone(lo: &LOrder, sl: &LSlot, ck: &Keypair, xp: i64, e: i64, bound: i64, xe: i64, cr: i64, prem: i64, lock: u64) -> (T, OPair, Bond) {
    let k = kit();
    let o = &lo.s;
    let n = o.size_left;
    let sc = o.m.p.scale;
    let (bl, bs, lk, sk) = if o.side == L { (xp - o.my_rate, bound, o.owner, x(ck)) } else { (bound, xp + o.my_rate, x(ck), o.owner) };
    let mut part = OPair {
        m: o.m,
        size: n,
        bl,
        bs,
        entry: e,
        long_key: lk,
        short_key: sk,
        b: OBinding { uw_id: sl.idb(), uw_key: sl.s.owner, cover_rate: cr, expiry: xe, premium: prem },
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers::default(),
    };
    if o.side == L {
        part.mk.long = maker_of(&lo.op);
    } else {
        part.mk.short = maker_of(&lo.op);
    }
    let a = BindArgs::new(lock as i64, xe, 0, &[Claim::output(1, q(n, cr, sc))]);
    let b2 = sl.s.bind(&slot_ctx(sl, lock), &a, sl.value as i64).unwrap_or_else(|_| sl.s.clone());
    let args = vec![
        iv(1),
        iv(n),
        iv(xp),
        iv(e),
        iv(bound),
        bv(&x(ck)),
        bv(&[0; 32]),
        iv(0),
        iv(0),
        iv(0),
        iv(1),
        iv(0),
        iv(xe),
        iv(cr),
        iv(prem),
        bv(&k.pair.prefix),
        bv(&k.pair.suffix),
    ];
    let fv = 1_000_000_000_000i64;
    let fund = key_in(outpoint(0x91), fv as u64, 0, ck);
    let fop = fund.outpoint;
    let ins = vec![order_in(lo, "take", args), slot_in(sl, "bind", bind_args(&a, &uw_kp())), fund];
    let spend = o.take_spend(n);
    let pv = part.floor();
    let mut outs = vec![
        out(lo.value - spend as u64, key_spk(&o.owner), None),
        out(pv as u64, pair_spk(&part), None),
        out(sl.value, bond_spk(&b2), bind(1, sl.id.unwrap())),
        out((fv - (pv - spend)) as u64, p2pk_spk(ck), None),
    ];
    seal(&mut outs, 1, 2, fop);
    (build_ext(&ins, outs, lock, &[0, 10, 0]), part, b2)
}

/// The underwriter renews `lp` as often as the contracts let it, every renewal at the same lock time `lock` (no
/// calendar time passes between them), advancing its ring just enough each time. Returns the last pair, the slot and
/// what each renewal took from the pair (`q(s, bs - bl) - q(s, new range)`, sent where the underwriter's bind says).
fn renew_burst(mut lp: LPair, mut sl: LSlot, lock: u64) -> (LPair, LSlot, Vec<i64>) {
    let mut pays = vec![];
    while let Some((_, pay)) = lp.s.renewed() {
        let x2 = lp.s.b.expiry + lp.s.m.bond.term;
        let d = (x2 / lp.s.m.bond.step - 12 - sl.s.j0).max(0);
        let (t, r, b2) = renew_tx(&lp, &sl, lock, d);
        must(&format!("renewal {} (X {} -> {x2}) at DAA {lock}", pays.len() + 1, lp.s.b.expiry), &t);
        pays.push(pay);
        lp = live(&t, 0, lock, &r);
        sl = live(&t, 1, lock, &b2);
    }
    let x2 = lp.s.b.expiry + lp.s.m.bond.term;
    let d = (x2 / lp.s.m.bond.step - 12 - sl.s.j0).max(0);
    refused("the renewal after the last (the range no longer wider than both bands)", &renew_tx(&lp, &sl, lock, d).0, 0);
    (lp, sl, pays)
}

// ---------------------------------------------------------------- R9-F1: renewals of a pair whose expiry lies in the past

/// R9-F1, the drain of an order through a take with no pair. An order rests (long, 2,000 units, limit 10.50, margin rate
/// 1 KAS, premium accepted up to 0.10 KAS per unit and term, any slot). At DAA 30,000 the attacker, holding a slot whose
/// ring was never advanced (it holds the expiries 100 .. 1,200) and a second key as the counterparty, takes the order at
/// 10.00 into a part with expiry 1,200 (more than 28,000 DAA in the past), short bound 12.20, entry 10.00, cover rate
/// 3.20 (the range) and the order's maximal premium. Then, still at DAA 30,000, its slot renews the part 21 times in a
/// row: every renewal moves the expiry by one term (1,200 -> 18,000), the long's bound up by 0.05 and takes 200 KAS out
/// of the pair to the underwriter. After the 21st the long's bound (10.05) is above the entry and the lapse settle pays
/// the order's owner nothing: it paid 2,000.2 KAS and leaves with 0. The design's claim (DESIGN-ORACLE-IMPL 16, the
/// renewal row; KpfPairO.renew's comment) is "the premium is paid at most once per term of calendar time".
#[test]
fn r9_exploit_a_take_without_a_pair_and_back_to_back_renewals_drain_the_orders_margin() {
    let now = 30_000u64;
    let lo = order(L, &long_kp(), 0x71, now - 100);
    let o = lo.s.clone();
    let n = o.size_left;
    let sc = o.m.p.scale;
    let sl = slot(K);
    assert_eq!(sl.s.j0, 0, "the attacker's slot: ring position 0, buckets for the expiries 100 .. 1,200");
    let att = keypair(77);
    let (t, part, b2) = take_alone(&lo, &sl, &att, 1_000_000_000, 1_000_000_000, 1_220_000_000, 1_200, 320_000_000, o.prem_max, now);
    must("the take with no pair, expiry 1,200 at DAA 30,000", &t);
    let paid_in = o.take_spend(n);
    assert_eq!(paid_in, 200_020_000_000, "the order pays 2,000 KAS of margin and a carrier");
    let lp = live(&t, 1, now, &part);
    let sl = live(&t, 2, now, &b2);
    // Without the renewals the part lapses at once and the order gets its margin back (review 8, note 1).
    assert!(lp.s.lapse_from() <= now);
    assert_eq!(lp.s.lapse_out(lp.value).0, q(n, o.my_rate, sc), "control: the immediate lapse settle pays back the margin");
    let (lp2, _, pays) = renew_burst(lp, sl, now);
    assert_eq!(pays.len(), 21, "21 renewals at one DAA");
    assert_eq!(lp2.s.b.expiry, 1_200 + 21 * 800);
    let taken: i64 = pays.iter().sum();
    assert_eq!(taken, 21 * 200 * KAS, "4,200 KAS of premiums, half of each from the order's side");
    assert_eq!((lp2.s.bl, lp2.s.bs), (1_005_000_000, 1_115_000_000));
    let (l, s) = lp2.s.lapse_out(lp2.value);
    assert_eq!(l, 0, "the order's owner gets nothing at the lapse settle");
    must("the lapse settle of the drained part", &lapse_tx(&lp2, now, 0));
    // The attacker's books: it funded the part's short side and the slot's KAS (returned); it receives the premiums and
    // the short's lapse payout. Its gain is the order's loss.
    let funded = part.floor() - paid_in;
    let gain = taken + s - funded;
    assert_eq!(gain, paid_in, "the attacker gains exactly what the order paid in: {gain}");
}

/// R9-F1 without any colluding party: the underwriter alone. Two honest resting orders (a long and a short of the
/// fixture, both accepted at DAA 0, any slot) are filled at DAA 30,000 by an outsider's slot with expiry 800 (the fill's
/// only bound, `xe >= the order's DAA + term`, holds: 0 + 800) and the orders' maximal premium. The slot then renews
/// the pair 19 times at DAA 30,000. The long's equity at the entry falls from 1,000 to 50 KAS, the short's from 2,000 to
/// 1,050 KAS: the underwriter takes 1,900 KAS (plus the 100 KAS first premium) in no calendar time, and nothing a
/// party signs or can contest is involved.
#[test]
fn r9_exploit_an_underwriter_alone_fills_two_aged_orders_into_a_past_expiry_and_renews_them_dry() {
    let now = 30_000u64;
    let (lo, so, sl) = (order(L, &long_kp(), 0x71, 0), order(S, &short_kp(), 0x72, 0), slot(K));
    let sp = OpenSpec { n: 1_000_000_000, x: 1_000_000_000, cover_rate: 600_000_000, premium: lo.s.prem_max, xe: 800, lock: now, advance: 0 };
    let (t, p, b2) = open_tx(&lo, &so, &sl, &sp, &|_, _| {});
    must("the fill of two aged orders with expiry 800 at DAA 30,000", &t);
    let lp = live(&t, 2, now, &p);
    let sl = live(&t, 3, now, &b2);
    let (l0, s0) = lp.s.lapse_out(lp.value);
    assert_eq!((l0, s0 - p.m.p.carrier), (1_000 * KAS, 2_000 * KAS), "control: at the open, long 1,000 KAS, short 2,000 KAS at the entry");
    let (lp2, _, pays) = renew_burst(lp, sl, now);
    assert_eq!(pays.len(), 19, "19 renewals at one DAA");
    let taken: i64 = pays.iter().sum();
    assert_eq!(taken, 1_900 * KAS);
    let (l, s) = lp2.s.lapse_out(lp2.value);
    assert_eq!((l, s - p.m.p.carrier), (50 * KAS, 1_050 * KAS), "after the burst: long 50 KAS, short 1,050 KAS at the entry");
    must("the lapse settle after the burst", &lapse_tx(&lp2, now, 0));
}

/// R9-F2, the bounded form on an honest pair: an underwriter silent past the expiry renews late, twice in a row, just
/// before the lapse settle is due. The fixture pair (opened at 300, X = 1,200, lapse settle due at 2,400) is renewed
/// at DAA 2,300 to 2,000 and again to 2,800: two premiums at one DAA, the first for the term [1,200, 2,000] in which no
/// claim on the pair could be valid (a claim needs P + W <= X).
#[test]
fn r9_exploit_a_silent_underwriter_charges_the_lapsed_term_by_a_late_double_renewal() {
    let (lp, sl, _) = opened();
    let lock = 2_300u64;
    assert!(lp.s.lapse_from() > lock, "the lapse settle is not yet due at 2,300");
    let (t1, r1, b1) = renew_tx(&lp, &sl, lock, 8);
    must("the late renewal 1,200 -> 2,000 at 2,300", &t1);
    let (l1, s1) = (live(&t1, 0, lock, &r1), live(&t1, 1, lock, &b1));
    let (t2, r2, _) = renew_tx(&l1, &s1, lock, 8);
    must("the second renewal 2,000 -> 2,800 at the same DAA", &t2);
    assert_eq!(r2.b.expiry, 2_800);
    assert_eq!(r2.bl - lp.s.bl, 2 * (lp.s.b.premium - lp.s.b.premium / 2), "the long paid two premium halves at DAA 2,300");
}

/// R9-F2, the exit "nobody can postpone" (DESIGN-ORACLE-IMPL 22, R6-F3): once the lapse settle is due (2,400 for the
/// fixture pair) the underwriter can still renew, at any later DAA, and the settle is no longer due. At 2,500 the
/// lapse settle and a renewal of the same pair conflict: whichever the miner lands decides whether the parties are
/// settled at the entry or kept in the pair (for one more premium, then again).
#[test]
fn r9_exploit_a_renewal_after_the_lapse_settle_is_due_cancels_it() {
    let (lp, sl, _) = opened();
    let lock = 2_500u64;
    assert!(lp.s.lapse_from() <= lock);
    must("the lapse settle is due at 2,500", &lapse_tx(&lp, lock, 0));
    let (t1, r1, b1) = renew_tx(&lp, &sl, lock, 8);
    must("a renewal of the same pair at 2,500 (it conflicts with the settle)", &t1);
    let (l1, s1) = (live(&t1, 0, lock, &r1), live(&t1, 1, lock, &b1));
    let (t2, r2, _) = renew_tx(&l1, &s1, lock, 8);
    must("and a second at once", &t2);
    let l2 = live(&t2, 0, lock, &r2);
    assert_eq!(l2.s.lapse_from(), 2_800 + 1_200);
    refused("the lapse settle is no longer due", &lapse_tx(&l2, lock, 0), 0);
}

/// R9-N1, a note on DESIGN-ORACLE-IMPL 26.4 ("the pending part's refund, which a cure or a valid settlement pays back to the
/// underwriter"): a valid partial liquidation pays the underwriter `B + fee`, not its refund `B + carrier`; the carrier
/// the underwriter's vouch funded stays in the forfeited part (the winner's). 0.2 KAS per partial liquidation.
#[test]
fn r9_exploit_a_partial_liquidations_carrier_goes_to_the_winner_not_back_to_the_underwriter() {
    let (lp, _, _) = opened();
    let n = lp.s.size / 2;
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n, p: 920_000_000, t0: 450, lock: 450 }, 500);
    assert_eq!(lw.s.refund, lw.s.bond + lp.s.m.p.carrier);
    let (ft, s) = finalize_tx(&lw, 500, 200);
    must("the partial liquidation finalizes", &ft);
    let Settlement::Forfeit { uw, value, part } = s else { panic!("{s:?}") };
    assert_eq!(uw, lw.s.bond + lw.s.fee, "the underwriter gets B + fee");
    assert!(uw < lw.s.refund + lw.s.fee, "not its refund B + carrier");
    assert_eq!(value, lw.value as i64 - lw.s.bond - lw.s.fee);
    assert!(value >= part.floor(), "the carrier stays in the forfeited part (the winner's)");
}

// ---------------------------------------------------------------- R9-F3: shared headroom

/// `addMargin` of `side` by `kp` by `delta`: in 0 the pair (sequence quiet), 1 the signer's funding; out 0 the
/// continuation, 1 change.
fn add_margin_tx(lp: &LPair, side: i64, kp: &Keypair, delta: i64) -> (T, OPair) {
    let mut c = OPair { mk: Makers::default(), ..lp.s.clone() };
    if side == L {
        c.bl -= delta;
    } else {
        c.bs += delta;
    }
    let sc = lp.s.m.p.scale;
    let add = kpf_protocol::payoff::q(lp.s.size, delta, sc, sc - 1);
    let ins = vec![pair_in(lp, "addMargin", vec![Arg::Sig(*kp), iv(side), iv(delta)]), key_in(outpoint(0x92), 1_000_000_000_000, 0, kp)];
    let outs = vec![out(lp.value + add as u64, pair_spk(&c), bind(0, lp.id.unwrap())), out(500_000_000_000, p2pk_spk(kp), None)];
    (build_ext(&ins, outs, 0, &[seq_quiet(lp), 0]), c)
}

/// R9-F3. The fixture pair has 3 KAS of headroom (range 3, cover rate 6). The short adds 3 KAS of margin (its own
/// funds, refundable): the range is the cover rate. The long can then add no margin at all, and when the price falls
/// to 9.20 the underwriter's (true) liquidation of the long carries no cure (the cured range 15 - 8.56 exceeds the cover
/// rate): the long's 200 KAS of equity is forfeited to the short. Control: without the short's top-up the same claim
/// offers a cure and the long keeps its position. An entrant does the same through a transfer of the short side
/// (`nb = bl + coverRate`).
#[test]
fn r9_exploit_the_short_takes_the_shared_headroom_and_strips_the_longs_cure() {
    let (lp, _, _) = opened();
    let p = 920_000_000i64;
    assert!(lp.s.long_liquidatable(p));
    // Control: the claim at 9.20 on the untouched pair offers a cure, which the long takes.
    let lw0 = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p, t0: 450, lock: 450 }, 500);
    assert_ne!(lw0.s.cure_hash, [0; 32], "control: a cure is offered");
    must("control: the long cures", &cure_tx(&lw0, &long_kp()).0);
    // The short's top-up to the cover rate.
    let (t, c) = add_margin_tx(&lp, S, &short_kp(), 300_000_000);
    must("the short adds 3 KAS of margin: the range is the cover rate", &t);
    let lp2 = live(&t, 0, 400, &c);
    assert_eq!(lp2.s.bs - lp2.s.bl, lp2.s.b.cover_rate);
    refused("the long's addMargin of one sompi per unit", &add_margin_tx(&lp2, L, &long_kp(), 1).0, 0);
    let lw = proposed(&lp2, &ClaimSpec { kind: LIQ, side: L, n: lp2.s.size, p, t0: 450, lock: 450 }, 500);
    assert_eq!(lw.s.cure_hash, [0; 32], "no cure is offered");
    // The cure the long would have taken is refused.
    let (cure0, v0) = lw0.s.cure(lw0.value).unwrap();
    let cure_try = build_ext(
        &[pend_in(&lw, "cure", vec![Arg::Sig(long_kp())]), key_in(outpoint(0x93), 500_000_000_000, 0, &long_kp())],
        vec![out(lw.s.refund as u64, key_spk(&lw.s.b.uw_key), None), out(v0 as u64, pair_spk(&cure0), bind(0, lw.id.unwrap())), change(1_000_000_000)],
        0,
        &[0, 0],
    );
    refused("the long's cure", &cure_try, 0);
    let (ft, s) = finalize_tx(&lw, 500, 200);
    must("the liquidation finalizes", &ft);
    let Settlement::Forfeit { part, .. } = s else { panic!("{s:?}") };
    assert_eq!((part.long_key, part.short_key), (lp.s.short_key, lp.s.short_key), "the long's side is forfeited to the short");
    assert_eq!(lp2.s.long_at(p), 200 * KAS, "the long's equity at 9.20 that it could have kept by a cure");
    // The entrant's form: a taker of the short side posting nb = bl + coverRate.
    let (at, ls) = amend_tx(&lp, S, &short_kp(), 1_200_000_000, 20);
    must("the short lists", &at);
    let ll = live(&at, 0, 400, &ls);
    let (tt, part) = transfer_tx(&ll, S, 1_200_000_000, ll.s.bl + ll.s.b.cover_rate, &keypair(46), 20);
    must("a taker of the short posts the whole headroom", &tt);
    let lt = live(&tt, 1, 450, &part);
    refused("the stayer's addMargin after the entrant took the headroom", &add_margin_tx(&lt, L, &long_kp(), 1).0, 0);
}

// ---------------------------------------------------------------- lead 1: the identical-binding join and its cover

/// Close-versus-close of two whole pairs at `xp` with P3's binding `b3` and entry `e3` (as `oraclekit::close_vs_close_tx`,
/// the binding chosen by the builder): in 0 the host (closeHost), 1 the donor (closeDonor), 2 the executor; out 0 the
/// host's long, 1 the donor's short, 2 P3 (fresh id), 3 change.
fn cvc(host: &LPair, donor: &LPair, xp: i64, e3: i64, b3: OBinding) -> (T, OPair) {
    let (h, d) = (&host.s, &donor.s);
    let n = h.size;
    let mut p3 = h.rest_of(n);
    p3.b = b3;
    p3.bl = d.bl;
    p3.entry = e3;
    p3.long_key = d.long_key;
    (p3.long_ask, p3.long_min, p3.long_tip) = (d.long_ask, d.long_min, d.long_tip);
    p3.mk = Makers { host: maker_of(&host.op), donor: maker_of(&donor.op), ..Makers::default() };
    let sc = h.m.p.scale;
    let lpay = kpf_protocol::payoff::owed(n, xp - h.bl, h.long_tip, sc);
    let spay = kpf_protocol::payoff::owed(n, d.bs - xp, d.short_tip, sc) + donor.value as i64 - d.range_of(n);
    let host_args = vec![iv(n), iv(xp), iv(e3), iv(2), iv(d.bl), bv(&d.long_key), iv(d.long_ask), iv(d.long_min), iv(d.long_tip), bv(&maker_of(&donor.op))];
    let donor_args = vec![iv(n), iv(xp), iv(e3), iv(2), iv(h.bs), bv(&h.short_key), iv(h.short_bid), iv(h.short_min), iv(h.short_tip), bv(&maker_of(&host.op))];
    let ins = vec![pair_in(host, "closeHost", host_args), pair_in(donor, "closeDonor", donor_args), exec_in(229)];
    let keep_h = host.value as i64 - h.range_of(n);
    let mut outs = vec![
        out(lpay.max(2_000_000) as u64, key_spk(&h.long_key), None),
        out(spay.max(2_000_000) as u64, key_spk(&d.short_key), None),
        out((p3.range_of(n) + keep_h) as u64, pair_spk(&p3), None),
        change(90_000_000_000),
    ];
    seal(&mut outs, 2, 0, host.op);
    (build_ext(&ins, outs, 0, &[seq_quiet(host), seq_quiet(donor), 0]), p3)
}

/// Lead 1. Two pairs whose bindings differ in one field (slot id, slot key, cover rate, expiry or premium, each up and
/// down) cannot both accept one P3: whichever binding P3 carries, the pair with the other one refuses. Control: equal
/// bindings join.
#[test]
fn r9_holds_close_versus_close_joins_only_byte_identical_bindings() {
    let (lp, _, _) = opened();
    let base = lp.s.clone();
    let host = genesis_pair(OPair { long_ask: 1_000_000_000, mk: Makers::default(), ..base.clone() }, lp.value, 0x81, 400);
    let mk_donor = |b: OBinding| genesis_pair(OPair { short_bid: 1_000_000_000, b, mk: Makers::default(), ..base.clone() }, lp.value, 0x82, 400);
    let (t, _) = cvc(&host, &mk_donor(base.b), 1_000_000_000, base.entry, base.b);
    must("control: identical bindings", &t);
    let b = base.b;
    let variants: Vec<(&str, OBinding)> = vec![
        ("another slot id", OBinding { uw_id: [0x52; 32], ..b }),
        ("another slot key", OBinding { uw_key: x(&keypair(32)), ..b }),
        ("a higher cover rate", OBinding { cover_rate: b.cover_rate + 1, ..b }),
        ("a lower cover rate", OBinding { cover_rate: b.cover_rate - 1, ..b }),
        ("a later expiry", OBinding { expiry: b.expiry + 100, ..b }),
        ("an earlier expiry", OBinding { expiry: b.expiry - 100, ..b }),
        ("a higher premium", OBinding { premium: b.premium + 1, ..b }),
        ("a lower premium", OBinding { premium: b.premium - 1, ..b }),
    ];
    for (what, bd) in variants {
        let donor = mk_donor(bd);
        let (t, _) = cvc(&host, &donor, 1_000_000_000, base.entry, b);
        refused(&format!("donor with {what}, P3 with the host's binding"), &t, 1);
        let (t, _) = cvc(&host, &donor, 1_000_000_000, base.entry, bd);
        refused(&format!("donor with {what}, P3 with the donor's binding"), &t, 0);
    }
}

/// Lead 1, the cover accounting. Two pairs opened on one slot at one expiry (each 6,000 KAS counted at 1,200) become one
/// P3 by close-versus-close. Nothing a pair does lowers the ring: the slot cannot unbond while the bucket of 1,200 is
/// liable (advancing 11 steps at 1,300 leaves it, 12,000 KAS; the unbond is refused), and P3's renewal counts P3
/// again at 2,000, which keeps the slot bonded past 2,200 although both original counts are released at 1,400.
#[test]
fn r9_holds_two_covered_pairs_joined_stay_counted_until_their_liability_ends() {
    let (lp1, sl1, _) = opened();
    let (lo2, so2) = (order(L, &long_kp(), 0x73, 0), order(S, &short_kp(), 0x74, 0));
    let (t2, p2, b2) = open_tx(&lo2, &so2, &sl1, &open_spec(), &|_, _| {});
    must("a second pair on the same slot and expiry", &t2);
    let lp2 = live(&t2, 2, OPEN_DAA, &p2);
    let sl2 = live(&t2, 3, OPEN_DAA, &b2);
    assert_eq!(sl2.s.counted_at(1_200), 2 * lp1.s.cover_of(lp1.s.size));
    let (a1, h) = amend_tx(&lp1, L, &long_kp(), 1_000_000_000, 20);
    must("host lists its long", &a1);
    let (a2, d) = amend_tx(&lp2, S, &short_kp(), 1_000_000_000, 20);
    must("donor lists its short", &a2);
    let (host, donor) = (live(&a1, 0, 400, &h), live(&a2, 0, 400, &d));
    let (t, p3) = cvc(&host, &donor, 1_000_000_000, h.entry, h.b);
    must("close-versus-close", &t);
    assert_eq!(p3.b, lp1.s.b);
    refused("unbond while the bucket of 1,200 is liable", &unbond_tx(&sl2, 11, 1_300), 0);
    let lp3 = live(&t, 2, 450, &p3);
    let (rt, _, sl3) = renew_tx(&lp3, &sl2, 1_100, 8);
    must("P3 renews on the same slot", &rt);
    let sl3 = live(&rt, 1, 1_100, &sl3);
    assert_eq!(sl3.s.counted_at(2_000), p3.cover_of(p3.size));
    refused("unbond at 1,400: P3's renewal is counted at 2,000", &unbond_tx(&sl3, 12, 1_400), 0);
    must("the slot without the renewal unbonds at 1,400", &unbond_tx(&sl2, 12, 1_400));
}

/// Lead 1 / lead 4: a slot split races nothing it can win. Child A takes the bucket and enough value; child B, empty,
/// unbonds at once. The pair renews from either child (one covenant id): the renewal is counted by the child that binds
/// it, and that child's unbond is refused until the renewal's liability ends.
#[test]
fn r9_holds_a_split_slot_keeps_every_cover_counted() {
    let (lp, sl, _) = opened();
    let mut a = [0i64; 12];
    a[11] = sl.s.ring[11];
    let (st, ba, bb) = split_tx(&sl, &a, 1_200_000_000_000, 400);
    must("split: child A takes the bucket and 12,000 KAS", &st);
    let (ca, cb) = (live(&st, 0, 400, &ba), live(&st, 1, 400, &bb));
    must("child B (empty) unbonds", &unbond_tx(&cb, 0, 400));
    refused("child A cannot unbond while it counts the pair", &unbond_tx(&ca, 0, 400), 0);
    let (rt, _, b2) = renew_tx(&lp, &cb, 1_100, 8);
    must("the renewal by child B (same id)", &rt);
    let cb2 = live(&rt, 1, 1_100, &b2);
    refused("child B now counts the renewal at 2,000", &unbond_tx(&cb2, 12, 1_400), 0);
}

// ---------------------------------------------------------------- lead 3: take without a pair, within the order's terms

/// Lead 3. A take with no pair binds only the order and only within its terms: an entry below the price, a price above
/// the limit, a premium above `premMax`, a cover rate below the range, a counterparty rate below `minCounterRate`, the
/// slot's owner as the counterparty, or a slot of another id than `uwAccept` are refused. Without renewals the worst a
/// past expiry does is an immediate lapse settle at the entry, which pays the order its margin (it loses its carrier,
/// 0.2 KAS, and its tip). With them: R9-F1.
#[test]
fn r9_holds_a_take_without_a_pair_binds_only_within_the_orders_terms() {
    let now = 30_000u64;
    let lo = order(L, &long_kp(), 0x71, now - 100);
    let o = lo.s.clone();
    let sl = slot(K);
    let att = keypair(77);
    let ok = |xp: i64, e: i64, bound: i64, cr: i64, prem: i64, ck: &Keypair| take_alone(&lo, &sl, ck, xp, e, bound, 1_200, cr, prem, now).0;
    must("control", &ok(1_000_000_000, 1_000_000_000, 1_220_000_000, 320_000_000, o.prem_max, &att));
    refused("entry below the price", &ok(1_000_000_000, 999_999_999, 1_220_000_000, 320_000_000, o.prem_max, &att), 0);
    refused("price above the limit", &ok(o.limit + 1, o.limit + 1, 1_300_000_000, 400_000_000, o.prem_max, &att), 0);
    refused("premium above premMax", &ok(1_000_000_000, 1_000_000_000, 1_220_000_000, 320_000_000, o.prem_max + 1, &att), 0);
    refused("cover rate below the range", &ok(1_000_000_000, 1_000_000_000, 1_220_000_000, 319_999_999, o.prem_max, &att), 0);
    refused("counterparty rate below minCounterRate", &ok(1_000_000_000, 1_000_000_000, 1_000_000_000 + o.min_counter_rate - 1, 320_000_000, o.prem_max, &att), 0);
    refused("the slot's owner as the counterparty", &ok(1_000_000_000, 1_000_000_000, 1_220_000_000, 320_000_000, o.prem_max, &uw_kp()), 0);
    let lo2 = Live { s: OOpen { uw_accept: [0x52; 32], ..o.clone() }, ..lo.clone() };
    refused("a slot other than uwAccept", &take_alone(&lo2, &sl, &att, 1_000_000_000, 1_000_000_000, 1_220_000_000, 1_200, 320_000_000, o.prem_max, now).0, 0);
    let (t, part, _) = take_alone(&lo, &sl, &att, 1_000_000_000, 1_000_000_000, 1_220_000_000, 1_200, 320_000_000, o.prem_max, now);
    let lp = live(&t, 1, now, &part);
    let (l, _) = lp.s.lapse_out(lp.value);
    assert_eq!(l, o.take_spend(o.size_left) - o.m.p.carrier, "the immediate lapse pays the margin, the carrier is lost");
    must("the immediate lapse settle", &lapse_tx(&lp, now, 0));
}

// ---------------------------------------------------------------- lead 4: REVIEW-8 "Not checked"

/// A refuted claim's flow up to the restored pair: the restored pair under the claim id, the id, the dispute's anchor,
/// the resolved pool and the opener's receipt.
fn refuted_flow() -> (LPair, Hash, i64, LStake, LStake) {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p: 920_000_000, t0: 450, lock: 450 }, 500);
    let st = stake(70, 10 * UNIT as u64, 0, 0xc0);
    let (ct, w2, rc, dj) = contest_tx(&lw, &st, 70, 520);
    must("contest", &ct);
    let (cw, r, ld) = (live(&ct, 0, 520, &w2), live(&ct, 1, 520, &rc), live(&ct, 2, 520, &dj));
    let (dt, pool, dw) = decide_tx(&ld, &cw, 620);
    must("resolve and decide (refuted)", &dt);
    let (lpool, ldw) = (live(&dt, 0, 620, &pool), live(&dt, 1, 620, &dw));
    let (ft, s) = finalize_tx(&ldw, 520, 0);
    must("finalize the refuted part", &ft);
    let Settlement::Restore { part, .. } = s else { panic!("{s:?}") };
    let id = ldw.id.unwrap();
    (live(&ft, 1, 700, &part), id, 500, lpool, r)
}

/// Lead 4: a stake `open` inside a `deposit` of the same id. The dispute is the "claim" input of the open (it is the
/// only input of the id) and both entries count three outputs of the id; the opener's receipt sits at the opener's
/// own index, which the deposit requires to be a YES receipt of the dispute's round and anchor, the open a NO receipt
/// at round 0 anchored at the dispute's own DAA. Laid out for the deposit, the open refuses; laid out for the open, the
/// deposit refuses.
#[test]
fn r9_holds_a_stake_open_never_rides_inside_a_deposit_of_its_id() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p: 920_000_000, t0: 450, lock: 450 }, 500);
    let st = stake(70, 10 * UNIT as u64, 0, 0xc0);
    let (ct, _, _, dj) = contest_tx(&lw, &st, 70, 520);
    must("contest", &ct);
    let ld = live(&ct, 2, 520, &dj);
    let id = ld.id.unwrap();
    let (s2, s3) = (stake(72, 10 * UNIT as u64, 0, 0xc2), stake(71, 13 * UNIT as u64, 0, 0xc1));
    let jm = omarket().judge_market();
    let ins = || {
        vec![
            stake_in(&ld, "deposit", vec![b8(YES), b8(1), b8(2)]),
            stake_in(&s2, "open", vec![b8(0), b8(0), b8(2), Arg::Sig(keypair(72))]),
            stake_in(&s3, "commit", vec![b8(YES), b8(0), b8(500), Arg::Sig(keypair(71))]),
            exec_in(0xc3),
        ]
    };
    let sum = (s2.value + s3.value) as i64;
    let d2 = after_deposit(&ld.s, YES, sum, sum / UNIT, ld.daa);
    let r2y = Js::receipt(s2.s.owner, jm, YES, s2.value as i64, 0, 500);
    let r3y = Js::receipt(s3.s.owner, jm, YES, s3.value as i64, 0, 500);
    let outs = vec![
        out(ld.value, stake_spk(&d2), bind(0, id)),
        out(s2.value, stake_spk(&r2y), bind(0, id)),
        out(s3.value, stake_spk(&r3y), bind(0, id)),
        change(90_000_000_000),
    ];
    let t = build_ext(&ins(), outs, 600, &[0, 0, 0, 0]);
    refused("laid out for the deposit: the open refuses", &t, 1);
    assert_eq!(&verdicts(&t)[..1], &[true], "the deposit itself accepts that layout");
    let r2n = Js::receipt(s2.s.owner, jm, NO, s2.value as i64, 0, 520);
    let o0 = out(ld.value, stake_spk(&d2), bind(0, id));
    let nd = Js::opened(jm, spk_hash(&o0.script_public_key), 520, s2.value as i64);
    let outs = vec![o0, out(s2.value, stake_spk(&r2n), bind(0, id)), out(DISPUTE_BASE, stake_spk(&nd), bind(0, id)), change(90_000_000_000)];
    let t = build_ext(&ins(), outs, 600, &[0, 0, 0, 0]);
    refused("laid out for the open: the deposit refuses", &t, 0);
    assert!(verdicts(&t)[1], "the open itself accepts that layout");
}

/// Lead 4: a commit signed SINGLE|ANYONECANPAY (the stake input and its receipt at its own index, bound to the claim's
/// id) absorbed by another entry of the claim's id: the pair restored under the claim's id amends beside it, the
/// receipt authorized by the pair. The pair counts the id's outputs (one) and refuses; so does the pool's settle.
#[test]
fn r9_holds_a_single_anyonecanpay_commit_is_not_absorbed_by_another_entry_of_the_id() {
    let (rp, id, anchor, lpool, r) = refuted_flow();
    let s4 = stake(73, 10 * UNIT as u64, 0, 0xc4);
    let jm = omarket().judge_market();
    let rc = Js::receipt(s4.s.owner, jm, YES, s4.value as i64, 0, anchor);
    let mut c = OPair { mk: Makers::default(), ..rp.s.clone() };
    (c.long_ask, c.long_min, c.long_tip) = (1_000_000_000, 0, 0);
    let amend = || pair_in(&rp, "amend", vec![Arg::Sig(long_kp()), iv(L), iv(1_000_000_000), iv(0), iv(0)]);
    let commit = || stake_in(&s4, "commit", vec![b8(YES), b8(0), b8(anchor), Arg::SigType(keypair(73), 0x84)]);
    let cont = out(rp.value, pair_spk(&c), bind(0, id));
    let t = build_ext(&[amend(), commit(), exec_in(0xc5)], vec![cont.clone(), out(s4.value, stake_spk(&rc), bind(0, id)), change(90_000_000_000)], 0, &[20, 0, 0]);
    refused("the restored pair's amend beside an absorbed commit", &t, 0);
    assert!(verdicts(&t)[1], "the commit itself is satisfied: only the pair's count stops it");
    must("control: the amend alone", &build_ext(&[amend(), exec_in(0xc5)], vec![cont, change(90_000_000_000)], 0, &[20, 0]));
    // Beside the pool's settle of the opener's receipt: the pool counts one output of its id.
    let (stx, _, _) = settle_tx(&lpool, &[r.clone()], 700);
    let ins = vec![stake_in(&lpool, "settle", vec![b8(1), b8(1)]), stake_in(&r, "redeem", vec![b8(0)]), commit(), exec_in(0xc6)];
    let mut outs = stx.0.outputs[..2].to_vec();
    outs.push(out(s4.value, stake_spk(&rc), bind(0, id)));
    outs.push(change(90_000_000_000));
    refused("the pool's settle beside an absorbed commit", &build_ext(&ins, outs, 700, &[0, 0, 0, 0]), 0);
}

/// A resolved pool whose every receipt has been settled, holding its base and a burn `burn`, under id `id` (as
/// `audit8_tests`).
fn settled_pool(id: u8, burn: u64) -> LStake {
    let jm = omarket().judge_market();
    let s = Js {
        status: STATUS_RESOLVED,
        side: NO,
        no: 10 * UNIT,
        no_u: 10,
        yes: 10 * UNIT,
        yes_u: 10,
        w_left: 0,
        l_left: 0,
        base: DISPUTE_BASE as i64,
        start: 600,
        ..Js::opened(jm, [0x77; 32], 500, 0)
    };
    Live { op: outpoint(id), value: DISPUTE_BASE + burn, daa: 900, id: Some(Hash::from_bytes([id; 32])), s }
}

/// Lead 4: the burn of `KpfStake.close` at `M - 1 - self` beside another family input whose second payout is also
/// positional (a lapse settle's short at `M - 1 - self`): each sits at its own mirrored slot and both are accepted; with
/// the two slots swapped both refuse.
#[test]
fn r9_holds_the_pools_burn_and_a_pairs_mirrored_payout_never_share_an_output() {
    let (lp, _, _) = opened();
    let pool = settled_pool(0xb1, UNIT as u64);
    let (l, s) = lp.s.lapse_out(lp.value);
    let burn = || out(UNIT as u64, kpf_protocol::lock::burn_spk(), None);
    let ins = || vec![stake_in(&pool, "close", vec![b8(4)]), pair_in(&lp, "lapseSettle", vec![]), exec_in(0xb3)];
    let outs = vec![change(DISPUTE_BASE + 90_000_000_000), out(l as u64, key_spk(&lp.s.long_key), None), change(2_000_000), out(s as u64, key_spk(&lp.s.short_key), None), burn()];
    must("the burn at 4 (M - 1 - 0), the short at 3 (M - 1 - 1)", &build_ext(&ins(), outs, 2_400, &[0, 0, 0]));
    let outs = vec![change(DISPUTE_BASE + 90_000_000_000), out(l as u64, key_spk(&lp.s.long_key), None), change(2_000_000), burn(), out(s as u64, key_spk(&lp.s.short_key), None)];
    let t = build_ext(&ins(), outs, 2_400, &[0, 0, 0]);
    refused("the pool against the short's payout at its slot", &t, 0);
    refused("the pair against the burn at its slot", &t, 1);
}

/// Lead 4, the 64-bit edge: a pair at the largest size checkPair admits (`size / scale = (2^63 - 1 - carrier) /
/// priceMax - 1`, 92,232 units) with the short's bound at `priceMax`, the long's at half of it and a cover rate of the
/// range. A liquidation of the short at `priceMax` proposes (bond `q(n, coverRate * sigma)`, 4.6 * 10^17 sompi; the
/// pending part above 5 * 10^18), finalizes and forfeits; the pair lapse-settles at its entry. No product leaves 63 bits.
#[test]
fn r9_holds_the_arithmetic_at_the_largest_size_and_price() {
    let (lp, _, _) = opened();
    let m = lp.s.m;
    let pmax = m.p.price_max;
    let units = (i64::MAX - m.p.carrier) / pmax - 1;
    let size = units * m.p.scale;
    let p = OPair { size, bl: pmax / 2, bs: pmax, entry: pmax / 4 * 3, b: OBinding { cover_rate: pmax / 2, premium: 0, ..lp.s.b }, mk: Makers::default(), ..lp.s.clone() };
    let gp = genesis_pair(p.clone(), p.floor() as u64, 0x83, 400);
    assert!(gp.value > 4_600_000_000_000_000_000);
    let lw = proposed(&gp, &ClaimSpec { kind: LIQ, side: S, n: size, p: pmax, t0: 450, lock: 450 }, 500);
    assert!(lw.value > 5_000_000_000_000_000_000);
    let (ft, s) = finalize_tx(&lw, 500, 200);
    must("the liquidation at priceMax finalizes", &ft);
    assert!(matches!(s, Settlement::Forfeit { .. }), "{s:?}");
    must("the lapse settle at the largest size", &lapse_tx(&gp, gp.s.lapse_from(), 0));
    let (l, _) = gp.s.lapse_out(gp.value);
    assert_eq!(l, q(size, pmax / 4, m.p.scale));
}

// ---------------------------------------------------------------- lead 5: the seeded walk over every entry point

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
    /// A value around `c`: usually within `w` of it, sometimes far outside.
    fn around(&mut self, c: i64, w: i64) -> i64 {
        let w = w.max(1);
        if self.chance(10) {
            c + (self.below(20 * w as u64) as i64 - 10 * w)
        } else {
            c + (self.below(2 * w as u64 + 1) as i64 - w)
        }
    }
}

/// One live pair of the walk: its UTXO, the true price, the binding its lineage started with, the renewals of the
/// lineage and their DAA scores.
#[derive(Clone)]
struct WP {
    lp: LPair,
    truth: i64,
    b0: OBinding,
    renewals: Vec<u64>,
    /// The underwriter never renews this lineage (its cover lapses; the lapse settle is due at X + tail + idle).
    silent: bool,
}

/// The walk's books.
#[derive(Default, Debug)]
struct WBooks {
    steps: std::collections::BTreeMap<&'static str, usize>,
    refused_as_ruled: usize,
    hostile_refused: usize,
}

struct Walk {
    rng: Rng,
    seed: u64,
    step: usize,
    now: u64,
    slot: LSlot,
    pairs: Vec<WP>,
    keys: std::collections::HashMap<[u8; 32], Keypair>,
    next: u32,
    books: WBooks,
    /// Violations of the stated rules that the walk records instead of stopping (seed, step, what).
    violations: Vec<(u64, usize, String)>,
    hostile_takes: bool,
}

const DUST: i64 = 2_000_000;

impl Walk {
    fn new(seed: u64, hostile_takes: bool) -> Walk {
        let mut keys = std::collections::HashMap::new();
        for s in [41u8, 42, 45, 46, 47, 48, 49, 50, 31] {
            keys.insert(x(&keypair(s)), keypair(s));
        }
        Walk {
            rng: Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1),
            seed,
            step: 0,
            now: 300,
            slot: slot(20_000_000_000_000),
            pairs: vec![],
            keys,
            next: 0,
            books: WBooks::default(),
            violations: vec![],
            hostile_takes,
        }
    }
    fn fresh_op(&mut self) -> (kaspa_consensus_core::tx::TransactionOutpoint, Hash) {
        self.next += 1;
        let mut b = [0x5au8; 32];
        b[..4].copy_from_slice(&self.next.to_le_bytes());
        b[4..12].copy_from_slice(&self.seed.to_le_bytes());
        (kaspa_consensus_core::tx::TransactionOutpoint { transaction_id: kaspa_consensus_core::tx::TransactionId::from_bytes(b), index: self.next }, Hash::from_bytes(b))
    }
    fn count(&mut self, what: &'static str) {
        *self.books.steps.entry(what).or_default() += 1;
    }
    fn key(&self, k: &[u8; 32]) -> Keypair {
        *self.keys.get(k).expect("a known key")
    }
    fn entrant(&mut self) -> Keypair {
        let ks = [45u8, 46, 47, 48, 49, 50, 41, 42, 31];
        keypair(ks[self.rng.below(ks.len() as u64) as usize])
    }
    fn violation(&mut self, what: String) {
        self.violations.push((self.seed, self.step, what));
    }
    fn quiet_ok(&self, lp: &LPair) -> bool {
        self.now >= lp.daa + lp.s.m.p.quiet_daa as u64
    }
    fn hostile(&mut self, what: &str, t: &T) {
        assert!(!accepted(t), "seed {} step {}: hostile variant accepted: {what}", self.seed, self.step);
        self.books.hostile_refused += 1;
    }

    /// The lineage rule of review 8 (26.1) and of this review: a part's binding is its lineage's first binding with the
    /// expiry moved by exactly one term per renewal; checked for every live pair after every step.
    fn check_bindings(&self) {
        for w in &self.pairs {
            let mut b = w.b0;
            b.expiry += w.renewals.len() as i64 * w.lp.s.m.bond.term;
            assert_eq!(w.lp.s.b, b, "seed {} step {}: a binding changed without a renewal", self.seed, self.step);
            assert!(w.lp.value as i64 >= w.lp.s.floor(), "seed {} step {}: a pair below its floor", self.seed, self.step);
            assert!(w.lp.s.bs - w.lp.s.bl <= w.lp.s.b.cover_rate, "seed {} step {}: range above the cover rate", self.seed, self.step);
        }
    }

    fn open(&mut self) {
        let m = omarket();
        let xe = (((self.now + 800 + 99) / 100) * 100) as i64;
        let adv = (xe / 100 - self.slot.s.j0 - 12).max(0);
        let mut sp = open_spec();
        sp.lock = self.now;
        sp.xe = xe;
        sp.advance = adv;
        let (op1, id1) = self.fresh_op();
        let (op2, id2) = self.fresh_op();
        let mk = |op, id, side, kp: &Keypair, daa| Live { op, value: 2_000_000_000_000u64, daa, id: Some(id), s: sample_order(&m, side, x(kp)) };
        let (lo, so) = (mk(op1, id1, L, &long_kp(), self.now), mk(op2, id2, S, &short_kp(), self.now));
        let (t, p, b2) = open_tx(&lo, &so, &self.slot, &sp, &|_, _| {});
        if !accepted(&t) {
            self.books.refused_as_ruled += 1;
            return;
        }
        self.count("open");
        let lp = live(&t, 2, self.now, &p);
        self.slot = live(&t, 3, self.now, &b2);
        let truth = 1_000_000_000;
        let silent = self.rng.chance(25);
        self.pairs.push(WP { lp, truth, b0: p.b, renewals: vec![], silent });
    }

    fn tick(&mut self) {
        self.now += 10 + self.rng.below(140);
        for w in self.pairs.iter_mut() {
            let d = (self.rng.below(5) as i64 - 2) * 10_000_000;
            w.truth = (w.truth + d).clamp(800_000_000, 1_300_000_000);
        }
    }

    /// The owner of `side` lists it near the true price (its own signature) unless it is listed, and quietDaa passes.
    fn ensure_listed(&mut self, i: usize, side: i64) {
        let w = self.pairs[i].clone();
        let listed = if side == L { w.lp.s.long_ask > 0 } else { w.lp.s.short_bid > 0 };
        if listed || !self.quiet_ok(&w.lp) {
            return;
        }
        let ask = self.rng.around(w.truth, 20_000_000).max(1);
        let kp = self.key(if side == L { &w.lp.s.long_key } else { &w.lp.s.short_key });
        let (t, c) = amend_tx(&w.lp, side, &kp, ask, w.lp.s.m.p.quiet_daa as u64);
        must("list before a hand-over", &t);
        self.pairs[i].lp = live(&t, 0, self.now, &c);
        self.now += w.lp.s.m.p.quiet_daa as u64;
    }

    fn amend(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        if !self.quiet_ok(&w.lp) {
            return;
        }
        let side = if self.rng.chance(50) { L } else { S };
        let ask = self.rng.around(w.truth, 30_000_000).max(1);
        let kp = self.key(if side == L { &w.lp.s.long_key } else { &w.lp.s.short_key });
        let (t, c) = amend_tx(&w.lp, side, &kp, ask, w.lp.s.m.p.quiet_daa as u64);
        // Hostile: the other side's signature.
        let other = self.key(if side == L { &w.lp.s.short_key } else { &w.lp.s.long_key });
        if other.x_only_public_key() != kp.x_only_public_key() {
            let (h, _) = amend_tx(&w.lp, side, &other, ask, w.lp.s.m.p.quiet_daa as u64);
            self.hostile("amend signed by the other party", &h);
        }
        if ask > w.lp.s.m.p.price_max {
            assert!(!accepted(&t));
            return;
        }
        must("amend", &t);
        self.count("amend");
        self.pairs[i].lp = live(&t, 0, self.now, &c);
    }

    fn add_margin(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        if !self.quiet_ok(&w.lp) {
            return;
        }
        let p = &w.lp.s;
        let side = if self.rng.chance(50) { L } else { S };
        let head = p.b.cover_rate - (p.bs - p.bl);
        let delta = 1 + self.rng.below((2 * head).max(2) as u64) as i64;
        let kp = self.key(if side == L { &p.long_key } else { &p.short_key });
        let (t, c) = add_margin_tx(&w.lp, side, &kp, delta);
        let legal = c.bl >= 0 && c.bl < c.bs && c.bs <= p.m.p.price_max && c.bs - c.bl <= p.b.cover_rate;
        assert_eq!(accepted(&t), legal, "seed {} step {}: addMargin delta {delta} head {head}", self.seed, self.step);
        if legal {
            self.count("addMargin");
            self.pairs[i].lp = live(&t, 0, self.now, &c);
        } else {
            self.books.refused_as_ruled += 1;
        }
    }

    /// A whole transfer of `side` at `xp` to `ek` posting `nb`, with the part `part` and the leaver's payout `pay` as
    /// the builder writes them.
    fn transfer_custom(lp: &LPair, side: i64, xp: i64, nb: i64, ek: &Keypair, part: &OPair, pay: i64) -> T {
        let p = &lp.s;
        let leaver = if side == L { p.long_key } else { p.short_key };
        let args = vec![iv(side), iv(p.size), iv(xp), iv(nb), bv(&x(ek)), bv(&[0; 32]), iv(1)];
        let ins = vec![pair_in(lp, "transfer", args), key_in(outpoint(223), 1_000_000_000_000, 0, ek)];
        let keep = lp.value as i64 - p.range_of(p.size) + if pay < DUST { pay } else { 0 };
        let mut outs = vec![out(pay.max(DUST) as u64, key_spk(&leaver), None), out((part.range_of(p.size) + keep) as u64, pair_spk(part), None), out(100_000_000_000, p2pk_spk(ek), None)];
        seal(&mut outs, 1, 0, lp.op);
        build_ext(&ins, outs, 0, &[p.m.p.quiet_daa as u64, 0])
    }

    fn transfer(&mut self, i: usize) {
        let ls = if self.rng.chance(50) { L } else { S };
        self.ensure_listed(i, ls);
        let w = self.pairs[i].clone();
        if !self.quiet_ok(&w.lp) {
            return;
        }
        let p = w.lp.s.clone();
        let side = if p.long_ask > 0 && (p.short_bid == 0 || self.rng.chance(50)) { L } else if p.short_bid > 0 { S } else { return };
        let ek = self.entrant();
        let sc = p.m.p.scale;
        let mm = p.m.p.mmr_bps;
        let off = self.rng.below(3) as i64 * 50_000_000;
        let (xp, nb) = if side == L {
            let xp = self.rng.around(p.long_ask, 20_000_000).max(0);
            (xp, self.rng.around(p.bl - off, 30_000_000))
        } else {
            let xp = self.rng.around(p.short_bid, 20_000_000).max(0);
            (xp, self.rng.around(p.bs + off, 30_000_000))
        };
        let pay = if side == L { kpf_protocol::payoff::owed(p.size, xp - p.bl, p.long_tip, sc) } else { kpf_protocol::payoff::owed(p.size, p.bs - xp, p.short_tip, sc) };
        let mut part = p.rest_of(p.size);
        part.mk.parent = maker_of(&w.lp.op);
        if side == L {
            part.bl = nb;
            part.long_key = x(&ek);
            (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
        } else {
            part.bs = nb;
            part.short_key = x(&ek);
            (part.short_bid, part.short_min, part.short_tip) = (0, 0, 0);
        }
        let legal = xp >= 0
            && xp <= p.m.p.price_max
            && x(&ek) != p.b.uw_key
            && if side == L {
                xp >= p.long_ask && nb >= 0 && nb <= p.bl && (xp as i128) * (10_000 - mm) as i128 >= (nb as i128) * 10_000 && p.bs - nb <= p.b.cover_rate
            } else {
                xp <= p.short_bid && nb >= p.bs && nb <= p.m.p.price_max && (nb as i128) * 10_000 >= (xp as i128) * (10_000 + mm) as i128 && nb - p.bl <= p.b.cover_rate
            };
        let t = Self::transfer_custom(&w.lp, side, xp, nb, &ek, &part, pay);
        assert_eq!(accepted(&t), legal, "seed {} step {}: transfer side {side} x {xp} nb {nb}", self.seed, self.step);
        if !legal {
            self.books.refused_as_ruled += 1;
            return;
        }
        // Hostile variants: the stayer's bound, key or listing moved, a binding field changed, the leaver underpaid.
        let mut h = part.clone();
        match self.rng.below(5) {
            0 => {
                if side == L {
                    h.bs += 1
                } else {
                    h.bl -= 1
                }
            }
            1 => {
                if side == L {
                    h.short_key = x(&keypair(49))
                } else {
                    h.long_key = x(&keypair(49))
                }
            }
            2 => h.b.expiry -= 100,
            3 => h.entry += 1,
            _ => {
                if side == L {
                    h.short_bid += 1
                } else {
                    h.long_ask += 1
                }
            }
        }
        if h != part {
            self.hostile("transfer moving the stayer or the binding", &Self::transfer_custom(&w.lp, side, xp, nb, &ek, &h, pay));
        }
        if pay >= DUST + 1 {
            self.hostile("transfer underpaying the leaver", &Self::transfer_custom(&w.lp, side, xp, nb, &ek, &part, pay - 1));
        }
        self.count("transfer");
        self.keys.insert(x(&ek), ek);
        self.pairs[i].lp = live(&t, 1, self.now, &part);
    }

    /// `closeDonor` with no host, P3 as the builder writes it (`p3`), the short paid `spay`.
    fn donor_custom(donor: &LPair, xp: i64, e3: i64, bs3: i64, sk3: [u8; 32], p3: &OPair, spay: i64) -> T {
        let d = &donor.s;
        let args = vec![iv(d.size), iv(xp), iv(e3), iv(1), iv(bs3), bv(&sk3), iv(0), iv(0), iv(0), bv(&[0; 32])];
        let fund = key_in(outpoint(0xa1), 1_000_000_000_000, 0, &short_kp());
        let fop = fund.outpoint;
        let mut outs = vec![out(spay.max(DUST) as u64, key_spk(&d.short_key), None), out(p3.floor() as u64, pair_spk(p3), None), out(400_000_000_000, p2pk_spk(&short_kp()), None)];
        seal(&mut outs, 1, 1, fop);
        build_ext(&[pair_in(donor, "closeDonor", args), fund], outs, 0, &[d.m.p.quiet_daa as u64, 0])
    }

    fn donor_alone(&mut self, i: usize) {
        self.ensure_listed(i, S);
        let w = self.pairs[i].clone();
        let d = w.lp.s.clone();
        if !self.quiet_ok(&w.lp) || d.short_bid == 0 {
            return;
        }
        let sk3 = if self.rng.chance(5) { d.b.uw_key } else { x(&self.entrant()) };
        let xp = self.rng.around(d.short_bid, 20_000_000).max(0);
        let off = self.rng.below(3) as i64 * 50_000_000;
        let bs3 = self.rng.around(d.bs + off, 30_000_000);
        let e3 = self.rng.around(d.entry + 10_000_000, 15_000_000);
        let mut p3 = d.rest_of(d.size);
        p3.short_key = sk3;
        p3.bs = bs3;
        p3.entry = e3;
        (p3.short_bid, p3.short_min, p3.short_tip) = (0, 0, 0);
        p3.mk = Makers { donor: maker_of(&w.lp.op), ..Makers::default() };
        let sc = d.m.p.scale;
        let mut spay = kpf_protocol::payoff::owed(d.size, d.bs - xp, d.short_tip, sc) + w.lp.value as i64 - d.range_of(d.size);
        if spay < DUST {
            spay = 0;
        }
        let legal = xp >= 0 && xp <= d.m.p.price_max && xp <= d.short_bid && bs3 >= d.bs && bs3 <= d.m.p.price_max && e3 >= d.entry && sk3 != d.b.uw_key && bs3 - d.bl <= d.b.cover_rate;
        let t = Self::donor_custom(&w.lp, xp, e3, bs3, sk3, &p3, spay);
        assert_eq!(accepted(&t), legal, "seed {} step {}: closeDonor alone x {xp} bs3 {bs3} e3 {e3}", self.seed, self.step);
        if !legal {
            self.books.refused_as_ruled += 1;
            return;
        }
        let mut h = p3.clone();
        match self.rng.below(5) {
            0 => h.bl -= 1,
            1 => h.long_key = x(&keypair(49)),
            2 => h.b.cover_rate += 1,
            3 => h.b.premium = 0,
            _ => h.long_ask += 1,
        }
        if h != p3 {
            self.hostile("closeDonor moving the passive long or its binding", &Self::donor_custom(&w.lp, xp, e3, bs3, sk3, &h, spay));
        }
        if spay > DUST {
            self.hostile("closeDonor underpaying the short", &Self::donor_custom(&w.lp, xp, e3, bs3, sk3, &p3, spay - 1));
        }
        // The passive long: at every price its payoff in P3 is at least what it had, and its lapse payout too.
        for pr in [800_000_000i64, 1_000_000_000, 1_200_000_000, 1_500_000_000] {
            assert!(p3.long_at(pr) >= d.long_at(pr), "seed {} step {}: the passive long lost at {pr}", self.seed, self.step);
        }
        let lap = |p: &OPair| q(p.size, p.entry.clamp(p.bl, p.bs) - p.bl, p.m.p.scale);
        assert!(lap(&p3) >= lap(&d), "seed {} step {}: the passive long's lapse payout fell", self.seed, self.step);
        self.count("closeDonor alone");
        self.pairs[i].lp = live(&t, 1, self.now, &p3);
    }

    /// `closeHost` with no donor, P3 as the builder writes it, the long paid `lpay`.
    fn host_custom(host: &LPair, xp: i64, e3: i64, bl3: i64, lk3: [u8; 32], p3: &OPair, lpay: i64) -> T {
        let h = &host.s;
        let args = vec![iv(h.size), iv(xp), iv(e3), iv(1), iv(bl3), bv(&lk3), iv(0), iv(0), iv(0), bv(&[0; 32])];
        let fund = key_in(outpoint(0xa2), 1_000_000_000_000, 0, &long_kp());
        let fop = fund.outpoint;
        let keep = host.value as i64 - h.range_of(h.size) + if lpay < DUST { lpay } else { 0 };
        let mut outs = vec![out(lpay.max(DUST) as u64, key_spk(&h.long_key), None), out((p3.range_of(h.size) + keep) as u64, pair_spk(p3), None), out(400_000_000_000, p2pk_spk(&long_kp()), None)];
        seal(&mut outs, 1, 1, fop);
        build_ext(&[pair_in(host, "closeHost", args), fund], outs, 0, &[h.m.p.quiet_daa as u64, 0])
    }

    fn host_alone(&mut self, i: usize) {
        self.ensure_listed(i, L);
        let w = self.pairs[i].clone();
        let h = w.lp.s.clone();
        if !self.quiet_ok(&w.lp) || h.long_ask == 0 {
            return;
        }
        let lk3 = if self.rng.chance(5) { h.b.uw_key } else { x(&self.entrant()) };
        let xp = self.rng.around(h.long_ask, 20_000_000).max(0);
        let off = self.rng.below(3) as i64 * 50_000_000;
        let bl3 = self.rng.around(h.bl - off, 30_000_000);
        let e3 = self.rng.around(h.entry - 10_000_000, 15_000_000);
        let mut p3 = h.rest_of(h.size);
        p3.long_key = lk3;
        p3.bl = bl3;
        p3.entry = e3;
        (p3.long_ask, p3.long_min, p3.long_tip) = (0, 0, 0);
        p3.mk = Makers { host: maker_of(&w.lp.op), ..Makers::default() };
        let lpay = kpf_protocol::payoff::owed(h.size, xp - h.bl, h.long_tip, h.m.p.scale);
        let legal = xp >= 0 && xp <= h.m.p.price_max && xp >= h.long_ask && bl3 >= 0 && bl3 <= h.bl && e3 <= h.entry && lk3 != h.b.uw_key && h.bs - bl3 <= h.b.cover_rate;
        let t = Self::host_custom(&w.lp, xp, e3, bl3, lk3, &p3, lpay);
        assert_eq!(accepted(&t), legal, "seed {} step {}: closeHost alone x {xp} bl3 {bl3} e3 {e3}", self.seed, self.step);
        if !legal {
            self.books.refused_as_ruled += 1;
            return;
        }
        let mut hh = p3.clone();
        match self.rng.below(4) {
            0 => hh.bs += 1,
            1 => hh.short_key = x(&keypair(49)),
            2 => hh.b.expiry += 100,
            _ => hh.short_bid += 1,
        }
        if hh != p3 {
            self.hostile("closeHost moving the passive short or its binding", &Self::host_custom(&w.lp, xp, e3, bl3, lk3, &hh, lpay));
        }
        if lpay > DUST {
            self.hostile("closeHost underpaying the long", &Self::host_custom(&w.lp, xp, e3, bl3, lk3, &p3, lpay - 1));
        }
        for pr in [800_000_000i64, 1_000_000_000, 1_200_000_000, 1_500_000_000] {
            assert!(p3.short_at(pr) >= h.short_at(pr), "seed {} step {}: the passive short lost at {pr}", self.seed, self.step);
        }
        self.count("closeHost alone");
        self.pairs[i].lp = live(&t, 1, self.now, &p3);
    }

    /// Close-versus-close of two pairs of one binding: the first lists its long, the second its short at the first's
    /// entry, then the two close against each other.
    fn combined(&mut self) {
        let n = self.pairs.len();
        if n < 2 {
            return;
        }
        let i = self.rng.below(n as u64) as usize;
        let Some(j) = (0..n).find(|&j| j != i && self.pairs[j].lp.s.b == self.pairs[i].lp.s.b) else { return };
        let (a, b) = (self.pairs[i].clone(), self.pairs[j].clone());
        if a.lp.s.b != b.lp.s.b || a.lp.s.size != b.lp.s.size || !self.quiet_ok(&a.lp) || !self.quiet_ok(&b.lp) || b.lp.s.entry > a.lp.s.entry {
            return;
        }
        let xp = a.lp.s.entry;
        let (ta, ha) = amend_tx(&a.lp, L, &self.key(&a.lp.s.long_key), xp, 20);
        let (tb, db) = amend_tx(&b.lp, S, &self.key(&b.lp.s.short_key), xp, 20);
        if !accepted(&ta) || !accepted(&tb) {
            return;
        }
        let (host, donor) = (live(&ta, 0, self.now, &ha), live(&tb, 0, self.now, &db));
        // The host's rule bl3 <= bl and the donor's bs3 >= bs: P3 is no narrower on either side than the pair it keeps.
        if host.s.bs - donor.s.bl > host.s.b.cover_rate || xp < host.s.bl || xp > donor.s.bs || donor.s.bl > host.s.bl || host.s.bs < donor.s.bs {
            return;
        }
        let (t, p3) = cvc(&host, &donor, xp, xp, host.s.b);
        must(&format!("seed {} step {}: close-versus-close", self.seed, self.step), &t);
        self.count("close-versus-close");
        let (lo, hi) = (i.min(j), i.max(j));
        let w = WP { lp: live(&t, 2, self.now + 20, &p3), truth: a.truth, b0: a.b0, renewals: a.renewals.clone(), silent: a.silent };
        self.pairs.remove(hi);
        self.pairs.remove(lo);
        self.pairs.push(w);
    }

    /// The underwriter's renewal, from one step before the expiry; a silent underwriter (no renewal on some steps)
    /// renews late, and as often in a row as the contracts allow. A renewal that follows the lineage's previous one
    /// by less than half a term of calendar time is recorded as a violation of "the premium is paid at most once per
    /// term of calendar time" (R9-F2).
    fn renew(&mut self, i: usize) {
        loop {
            let w = self.pairs[i].clone();
            let p = &w.lp.s;
            let step = p.m.bond.step as u64;
            if (p.b.expiry as u64) > self.now + step || self.now >= p.lapse_from() {
                return;
            }
            let Some((r, pay)) = p.renewed() else { return };
            let x2 = p.b.expiry + p.m.bond.term;
            let d = (x2 / p.m.bond.step - 12 - self.slot.s.j0).max(0);
            let (t, r2, b2) = renew_tx(&w.lp, &self.slot, self.now, d);
            if !accepted(&t) {
                self.books.refused_as_ruled += 1;
                return;
            }
            assert_eq!(r2, r);
            // Hostile: one sompi more than the premium leaves the pair.
            let mut h = t.clone();
            h.0.outputs[0].value -= 1;
            h.0.finalize();
            assert!(!verdicts(&h)[0], "seed {} step {}: a renewal taking one sompi more", self.seed, self.step);
            self.books.hostile_refused += 1;
            if let Some(&prev) = w.renewals.last() {
                if self.now - prev < (p.m.bond.term / 2) as u64 {
                    self.violation(format!("R9-F2: renewal of X {} at DAA {} only {} DAA after the previous one (premium {pay})", p.b.expiry, self.now, self.now - prev));
                }
            }
            self.count("renew");
            self.slot = live(&t, 1, self.now, &b2);
            self.pairs[i].lp = live(&t, 0, self.now, &r2);
            self.pairs[i].renewals.push(self.now);
        }
    }

    /// A claim on a whole pair, then (W later, the other pairs' clocks moving too) its finalize: a close by either
    /// party at the true price, a true liquidation (cured half the time), or a false liquidation contested in time,
    /// refuted, the pool settled and closed. A claim landing too close to the expiry is void and restores the part.
    fn claim(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        let p = w.lp.s.clone();
        if p.long_key == p.short_key {
            return;
        }
        let liq_side = if p.long_liquidatable(w.truth) { Some(L) } else if p.short_liquidatable(w.truth) { Some(S) } else { None };
        let t0 = self.now as i64;
        let kind;
        let side;
        let price;
        let mut false_claim = false;
        if let Some(sd) = liq_side {
            (kind, side, price) = (LIQ, sd, w.truth);
        } else if self.now >= 600 && self.rng.chance(30) && p.range_ok() {
            // The underwriter lies: a liquidation of the long at a price that triggers it.
            let pf = p.trigger_of(L) * 9_000 / 10_000;
            if !p.long_liquidatable(pf) {
                return;
            }
            (kind, side, price, false_claim) = (LIQ, L, pf, true);
        } else {
            (kind, side, price) = (CLOSE, if self.rng.chance(50) { L } else { S }, w.truth);
        }
        let closer = if kind == CLOSE { Some(self.key(if side == L { &p.long_key } else { &p.short_key })) } else { None };
        let (pt, pw) = propose_tx(&w.lp, &ClaimSpec { kind, side, n: p.size, p: price, t0, lock: self.now }, &uw_kp(), closer.as_ref());
        if !accepted(&pt) {
            self.books.refused_as_ruled += 1;
            return;
        }
        self.count(if kind == CLOSE { "propose close" } else if false_claim { "propose false liquidation" } else { "propose liquidation" });
        let lw = live(&pt, 0, self.now, &pw);
        let pdaa = self.now as i64;
        self.pairs.remove(i);
        if false_claim {
            // An honest watcher contests in time with aged stake; the judge is honest (NO leads).
            let st = stake(70, 10 * UNIT as u64, 0, 0xc0);
            let (ct, w2, rc, dj) = contest_tx(&lw, &st, 70, self.now + 10);
            must("contest", &ct);
            let c_daa = self.now + 10;
            let (cw, r, ld) = (live(&ct, 0, c_daa, &w2), live(&ct, 1, c_daa, &rc), live(&ct, 2, c_daa, &dj));
            let (dt, pool, dw) = decide_tx(&ld, &cw, c_daa + 100);
            must("resolve, refuted", &dt);
            let (lpool, ldw) = (live(&dt, 0, c_daa + 100, &pool), live(&dt, 1, c_daa + 100, &dw));
            let (ft, s) = finalize_tx(&ldw, ldw.s.clock, 0);
            must("finalize refuted", &ft);
            let Settlement::Restore { part, defender, disputer, .. } = s else { panic!("seed {}: a refuted claim did not restore", self.seed) };
            assert_eq!(part, p.restore_state(p.size), "the refuted part is restored exactly");
            assert_eq!(defender + disputer, pw.bond, "the bond to the defender and the disputer");
            let (stx, p2, _) = settle_tx(&lpool, &[r], c_daa + 110);
            must("settle the opener's receipt", &stx);
            let lp2 = live(&stx, 0, c_daa + 110, &p2);
            let (ctx, _) = pool_close_tx(&lp2);
            must("close the pool", &ctx);
            self.count("refuted, pool closed");
            self.now = self.now.max(c_daa + 110);
            self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
            return;
        }
        if kind == LIQ && pw.cure_hash != [0; 32] && self.rng.chance(50) {
            let loser = self.key(if side == L { &p.long_key } else { &p.short_key });
            let (ct, c) = cure_tx(&lw, &loser);
            must("cure", &ct);
            self.count("cure");
            self.pairs.push(WP { lp: live(&ct, 1, self.now, &c), ..w });
            return;
        }
        let (ft, s) = finalize_tx(&lw, pdaa, 200);
        must(&format!("seed {} step {}: finalize {s:?}", self.seed, self.step), &ft);
        self.now += 200;
        match s {
            Settlement::Close { long, short } => {
                // The other party gets at least its payoff at the close price moved by g against the closer.
                let c = pw.close_price();
                let (lo_, so_) = (q(p.size, c - p.bl, p.m.p.scale), q(p.size, p.bs - c, p.m.p.scale));
                assert!(if side == L { short >= so_ || short == 0 } else { long >= lo_ || long == 0 }, "the non-closer underpaid");
                self.count("close settled");
            }
            Settlement::Forfeit { part, .. } => {
                self.count("liquidation settled");
                let wk = if side == L { p.short_key } else { p.long_key };
                assert_eq!((part.long_key, part.short_key), (wk, wk));
                self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
            }
            Settlement::Restore { part, defender, .. } => {
                assert_eq!(defender, pw.bond, "a void claim's bond goes to the defender");
                self.count("void claim restored");
                self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
            }
        }
    }

    fn lapse(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        if self.now < w.lp.s.lapse_from() {
            refused("lapse settle before it is due", &lapse_tx(&w.lp, w.lp.s.lapse_from() - 1, 0), 0);
            return;
        }
        must("lapse settle", &lapse_tx(&w.lp, self.now, 0));
        self.count("lapse settle");
        self.pairs.remove(i);
    }

    /// The shape of R9-F1: a take with no pair into an expiry in the past, then the renewals the contracts allow.
    fn hostile_take(&mut self) {
        if self.now < 4_000 {
            return;
        }
        let (op, id) = self.fresh_op();
        let lo = Live { op, value: 2_000_000_000_000u64, daa: self.now, id: Some(id), s: sample_order(&omarket(), L, x(&long_kp())) };
        let sl = Live { op: outpoint(0x99), value: K, daa: 0, id: Some(Hash::from_bytes([0x5b; 32])), s: Bond::new(x(&uw_kp()), omarket().bond_market(), 0) };
        let xe = 1_200;
        let (t, part, b2) = take_alone(&lo, &sl, &keypair(77), 1_000_000_000, 1_000_000_000, 1_220_000_000, xe, 320_000_000, lo.s.prem_max, self.now);
        must("the hostile take", &t);
        let mut lp = live(&t, 1, self.now, &part);
        let mut s2 = live(&t, 2, self.now, &b2);
        let mut last: Option<u64> = None;
        while lp.s.renewed().is_some() && lp.s.b.expiry as u64 <= self.now + 100 {
            let x2 = lp.s.b.expiry + lp.s.m.bond.term;
            let d = (x2 / lp.s.m.bond.step - 12 - s2.s.j0).max(0);
            let (rt, r, b) = renew_tx(&lp, &s2, self.now, d);
            must("a renewal of the taken part", &rt);
            if let Some(prev) = last {
                if self.now - prev < 400 {
                    self.violation(format!("R9-F1: take with expiry {xe} at DAA {}: renewal of X {} {} DAA after the previous one", self.now, lp.s.b.expiry, self.now - prev));
                }
            }
            last = Some(self.now);
            lp = live(&rt, 0, self.now, &r);
            s2 = live(&rt, 1, self.now, &b);
        }
        self.count("hostile take and renewals");
    }

    fn play(&mut self, steps: usize) {
        for s in 0..steps {
            self.step = s;
            self.tick();
            if self.pairs.len() < 4 && self.rng.chance(40) {
                self.open();
                // Two opens at one DAA share a binding, so close-versus-close can join them later.
                if self.rng.chance(50) {
                    self.open();
                }
            }
            if self.pairs.is_empty() {
                continue;
            }
            let i = self.rng.below(self.pairs.len() as u64) as usize;
            match self.rng.below(100) {
                0..=14 => self.amend(i),
                15..=24 => self.add_margin(i),
                25..=36 => self.transfer(i),
                37..=46 => self.donor_alone(i),
                47..=54 => self.host_alone(i),
                55..=62 => self.combined(),
                63..=72 => self.claim(i),
                73..=76 => self.lapse(i),
                77..=79 if self.hostile_takes => self.hostile_take(),
                _ => {}
            }
            // The underwriter renews each pair in its window most of the time; sometimes it is silent.
            for k in 0..self.pairs.len() {
                if k < self.pairs.len() && !self.pairs[k].silent && self.rng.chance(60) {
                    self.renew(k);
                }
            }
            self.check_bindings();
        }
    }
}

/// Lead 5. Seeded walks over every entry point of the five contracts: opens on one slot, amends, margin top-ups (legal
/// and beyond the headroom), whole transfers to signing takers with free arguments drawn wide (each compared with the
/// stated rule, and a hostile variant moving the stayer, its listing, the entry or the binding, or underpaying the
/// leaver), `closeDonor` with no host and `closeHost` with no donor (the same), close-versus-close of two pairs of one
/// binding, renewals in the window or late (a silent underwriter), claims of every outcome (a close, a true
/// liquidation cured or forfeited, a void claim, a false liquidation contested in time, refuted, its pool settled and
/// closed), and lapse settles. After every step every live pair keeps its lineage's binding (only the renewals move
/// the expiry, by one term each), its floor and its range within the cover rate. A rule the contracts accept beyond
/// the stated rule panics with the seed and step; the one statement the walk records instead, "at most one premium per
/// term of calendar time", is violated (R9-F2, the late double renewal) on the seeds and steps printed.
#[test]
fn r9_holds_the_seeded_walk_over_every_entry_point_but_records_renewal_bursts() {
    let mut total = WBooks::default();
    let mut violations = vec![];
    for seed in 1..=6u64 {
        let mut w = Walk::new(seed, false);
        w.play(200);
        for (k, v) in w.books.steps.iter() {
            *total.steps.entry(k).or_default() += v;
        }
        total.refused_as_ruled += w.books.refused_as_ruled;
        total.hostile_refused += w.books.hostile_refused;
        violations.extend(w.violations);
    }
    println!("walk steps: {:?}", total.steps);
    println!("refused as the rules say: {}, hostile variants refused: {}", total.refused_as_ruled, total.hostile_refused);
    for (s, st, v) in &violations {
        println!("seed {s} step {st}: {v}");
    }
    for k in ["open", "amend", "addMargin", "transfer", "closeDonor alone", "closeHost alone", "renew", "propose close", "lapse settle", "close-versus-close", "refuted, pool closed", "void claim restored", "liquidation settled"] {
        assert!(total.steps.get(k).copied().unwrap_or(0) > 0, "the walk never ran {k}");
    }
    assert!(total.hostile_refused > 50);
    assert!(violations.iter().all(|(_, _, v)| v.starts_with("R9-F2")), "only renewal bursts are recorded");
}

/// R9-F1 in the walk: with the hostile take enabled, every seed that reaches it records a burst of renewals at one DAA.
#[test]
fn r9_exploit_the_seeded_walk_finds_the_renewal_burst_of_a_take_into_the_past() {
    let mut found = vec![];
    for seed in 1..=4u64 {
        let mut w = Walk::new(seed, true);
        w.play(120);
        found.extend(w.violations.into_iter().filter(|(_, _, v)| v.contains("take with expiry")));
    }
    for (s, st, v) in found.iter().take(4) {
        println!("seed {s} step {st}: {v}");
    }
    assert!(!found.is_empty(), "the walk did not reach the hostile take");
}
