//! Review 9 (docs/audit/REVIEW-9.md): the oracle-market family at commit `7d9024e` (after the fix of review 8). Every
//! transaction here runs every input in the real script engine with the covenant context (`common::run`).
//! Since the fix of review 9 (docs/DESIGN-ORACLE-IMPL.md 27) every former `r9_exploit_` test is an `r9_holds_` test that
//! builds the same attack and asserts that it is refused, or, for a residual kept by design, an `r9_bounded_` test that
//! measures it and asserts its bound. `r9_holds_` tests assert that an attack is refused (or that a shape keeps the rule
//! it is meant to keep). Worked numbers use the fixture market of `oraclekit` (term 800, step 100, tail 200, idle 1,000,
//! W 200, quiet 20, mmr 5 %, g 2 %, a claim's bond 10 % of the cover, carrier 0.2 KAS).
//!
//! The fix: `KpfPairO.renew` requires the pair UTXO's own DAA score (exact) to be no later than its expiry; every
//! price-free entry that grows a side's bound (`addMargin`, an entrant's bound in `transfer` and `take`, P3's entering
//! side in `closeHost` and `closeDonor`) and every cure leaves the other side room up to its share of the cover rate
//! (the short `floor(coverRate / 2)` above the entry, the long the rest below it); close-versus-close keeps the entry.
//!
//! What the review found at commit `7d9024e`:
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
/// calendar time passes between them), advancing its ring just enough each time. Returns the last pair, the slot, what
/// each accepted renewal took from the pair, and the refused attempt that ended the burst.
fn renew_burst(mut lp: LPair, mut sl: LSlot, lock: u64) -> (LPair, LSlot, Vec<i64>, T) {
    let mut pays = vec![];
    loop {
        let Some((_, pay)) = lp.s.renewed() else { panic!("the range is too narrow to renew") };
        let x2 = lp.s.b.expiry + lp.s.m.bond.term;
        let d = (x2 / lp.s.m.bond.step - 12 - sl.s.j0).max(0);
        let (t, r, b2) = renew_tx(&lp, &sl, lock, d);
        if !accepted(&t) {
            return (lp, sl, pays, t);
        }
        pays.push(pay);
        lp = live(&t, 0, lock, &r);
        sl = live(&t, 1, lock, &b2);
    }
}

// ---------------------------------------------------------------- R9-F1: renewals of a pair whose expiry lies in the past

/// R9-F1, the drain of an order through a take with no pair (the same transaction as at `7d9024e`). An order rests
/// (long, 2,000 units, limit 10.50, margin rate 1 KAS, premium up to 0.10 KAS per unit and term, any slot). At DAA
/// 30,000 the attacker, holding a slot whose ring was never advanced and a second key as the counterparty, takes the
/// order at 10.00 into a part with expiry 1,200, short bound 12.20, cover rate 3.20 and the order's maximal premium; the
/// take is still accepted (it binds only the order, within its terms). Then its slot renews: the part was accepted at
/// 30,000, after its expiry, so the very first renewal is refused by the pair. Before the fix: 21 renewals, 2,000.2 KAS
/// taken, the order's whole margin.
#[test]
fn r9_holds_a_take_without_a_pair_into_the_past_is_never_renewed() {
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
    let lp = live(&t, 1, now, &part);
    let sl = live(&t, 2, now, &b2);
    let (lp2, _, pays, last) = renew_burst(lp.clone(), sl, now);
    assert!(pays.is_empty(), "no renewal of a part accepted after its expiry");
    refused("the first renewal of the part (accepted at 30,000, expiry 1,200)", &last, 0);
    assert!(verdicts(&last)[1], "the slot's bind itself is satisfied: only the pair's new rule refuses");
    assert_eq!(lp2.s.b.expiry, 1_200);
    // The order's margin comes back at the immediate lapse settle.
    assert!(lp.s.lapse_from() <= now);
    assert_eq!(lp.s.lapse_out(lp.value).0, q(n, o.my_rate, sc));
    must("the lapse settle of the part", &lapse_tx(&lp, now, 0));
}

/// R9-F1's residual for a take with no pair: what a past expiry still costs the order. The part is not renewable, so it
/// settles at its entry at once; the order gets its margin back (at the entry `e >= x` its long is owed at least its
/// rate) and loses what it put in beyond it: its tip and the part's carrier, both bounded by its own terms. A take pays
/// no premium. Measured with a tip of 0.01 KAS per unit: 2,000 units lose 20 KAS + 0.2 KAS.
#[test]
fn r9_bounded_a_take_into_the_past_costs_the_order_only_its_tip_and_the_carrier() {
    let now = 30_000u64;
    let lo0 = order(L, &long_kp(), 0x71, now - 100);
    let lo = Live { s: OOpen { tip: 1_000_000, ..lo0.s.clone() }, ..lo0.clone() };
    let o = lo.s.clone();
    let n = o.size_left;
    let sc = o.m.p.scale;
    let (t, part, _) = take_alone(&lo, &slot(K), &keypair(77), 1_000_000_000, 1_000_000_000, 1_220_000_000, 1_200, 320_000_000, o.prem_max, now);
    must("the take with no pair into the past", &t);
    let lp = live(&t, 1, now, &part);
    let paid = o.take_spend(n);
    let (back, _) = lp.s.lapse_out(lp.value);
    must("the immediate lapse settle", &lapse_tx(&lp, now, 0));
    let loss = paid - back;
    assert_eq!(loss, q(n, o.tip, sc) + o.m.p.carrier, "the order loses its tip and the carrier");
    assert_eq!(loss, 2_020_000_000);
}

/// R9-F1 without any colluding party (the same transaction as at `7d9024e`): two honest resting orders (accepted at DAA
/// 0, any slot) are filled at DAA 30,000 by an outsider's slot with expiry 800 (`xe >= the order's DAA + term` holds).
/// Before the fix the slot renewed the pair 19 times at DAA 30,000 and took 1,900 KAS. Since the fix the pair, accepted
/// at 30,000 after its expiry 800, is never renewed.
#[test]
fn r9_holds_an_underwriter_alone_cannot_renew_a_fill_into_a_past_expiry() {
    let now = 30_000u64;
    let (lo, so, sl) = (order(L, &long_kp(), 0x71, 0), order(S, &short_kp(), 0x72, 0), slot(K));
    let sp = OpenSpec { n: 1_000_000_000, x: 1_000_000_000, cover_rate: 600_000_000, premium: lo.s.prem_max, xe: 800, lock: now, advance: 0 };
    let (t, p, b2) = open_tx(&lo, &so, &sl, &sp, &|_, _| {});
    must("the fill of two aged orders with expiry 800 at DAA 30,000", &t);
    let lp = live(&t, 2, now, &p);
    let sl = live(&t, 3, now, &b2);
    let (_, _, pays, last) = renew_burst(lp.clone(), sl, now);
    assert!(pays.is_empty());
    refused("the first renewal of a pair accepted after its expiry", &last, 0);
    let (l, s) = lp.s.lapse_out(lp.value);
    assert_eq!((l, s - p.m.p.carrier), (1_000 * KAS, 2_000 * KAS), "the lapse settle pays both margins back at the entry");
    must("the immediate lapse settle", &lapse_tx(&lp, now, 0));
}

/// The residual of a fill into the past: each aged order loses what it paid beyond its margin, its tip and its half of
/// the first premium (and the carrier, only with a fill that empties the order: review 10; here each order keeps 1,000
/// units, so the executor funds the pair's carrier and the short gets it at the lapse settle). Bounded by the order's own
/// terms for the whole order: `q(size, tip + premMax / 2) + carrier`. On the fixture (1,000 units, premium 0.10 KAS, no
/// tip): the long 50 KAS, the short 50 KAS less the carrier, 100 KAS to the underwriter for a cover that had already
/// lapsed. An order that rests less than one term is never exposed: `fill` requires `xe >= its DAA + term`.
#[test]
fn r9_bounded_a_fill_of_aged_orders_into_the_past_costs_each_its_tip_and_half_the_first_premium() {
    let now = 30_000u64;
    let (lo, so, sl) = (order(L, &long_kp(), 0x71, 0), order(S, &short_kp(), 0x72, 0), slot(K));
    let prem = lo.s.prem_max;
    let n = 1_000_000_000;
    let sp = OpenSpec { n, x: 1_000_000_000, cover_rate: 600_000_000, premium: prem, xe: 800, lock: now, advance: 0 };
    let (t, p, _) = open_tx(&lo, &so, &sl, &sp, &|_, _| {});
    must("the fill into the past", &t);
    let lp = live(&t, 2, now, &p);
    let (l, s) = lp.s.lapse_out(lp.value);
    let sc = p.m.p.scale;
    let long_loss = lo.s.fill_spend(n, prem) - l;
    let short_loss = so.s.fill_spend(n, prem) - s;
    assert_eq!(long_loss, q(n, lo.s.tip + prem - prem / 2, sc));
    assert_eq!(short_loss, q(n, so.s.tip + prem / 2, sc) - p.m.p.carrier);
    assert_eq!((long_loss, short_loss), (5_000_000_000, 4_980_000_000));
}

/// R9-F2 (the same transactions as at `7d9024e`): the fixture pair (opened at 300, X = 1,200, lapse settle due at 2,400)
/// renewed by a silent underwriter at DAA 2,300: the first renewal (to 2,000) is still accepted, the second (to 2,800) at
/// the same DAA is refused, because the first one's continuation was accepted at 2,300, after its new expiry 2,000. What
/// remains: a renewal that lands in the last step of the following term, [X + term - step, X + term] = [1,900, 2,000],
/// may be followed by a second at once (the continuation, accepted by 2,000, is not past its new expiry): two premiums
/// at one DAA, the first for a term already over. The pair then stands exactly where an on-time underwriter (renewing
/// at 1,150 and 1,950) would have left it: a late underwriter catches up with the calendar, never runs ahead of it.
#[test]
fn r9_bounded_a_late_renewal_charges_no_more_than_the_on_time_schedule() {
    let (lp, sl, _) = opened();
    let (t1, r1, b1) = renew_tx(&lp, &sl, 2_300, 8);
    must("the late renewal 1,200 -> 2,000 at 2,300", &t1);
    let (l1, s1) = (live(&t1, 0, 2_300, &r1), live(&t1, 1, 2_300, &b1));
    refused("the second renewal 2,000 -> 2,800 at 2,300", &renew_tx(&l1, &s1, 2_300, 8).0, 0);
    // The residual: the double renewal at 1,950.
    let (t1, r1, b1) = renew_tx(&lp, &sl, 1_950, 8);
    must("the late renewal 1,200 -> 2,000 at 1,950", &t1);
    let (l1, s1) = (live(&t1, 0, 1_950, &r1), live(&t1, 1, 1_950, &b1));
    let (t2, r2, b2) = renew_tx(&l1, &s1, 1_950, 8);
    must("and 2,000 -> 2,800 at once (the residual)", &t2);
    let (l2, s2) = (live(&t2, 0, 1_950, &r2), live(&t2, 1, 1_950, &b2));
    refused("a third at 1,950: the slot's window (3,600 > 1,950 + 900)", &renew_tx(&l2, &s2, 1_950, 8).0, 1);
    assert_eq!(r2.bl - lp.s.bl, 2 * (lp.s.b.premium - lp.s.b.premium / 2), "two premium halves from the long");
    assert_eq!(lp.value - t2.0.outputs[0].value, 2 * 2_000_000_000, "two premiums of 20 KAS");
    // The on-time schedule reaches the same state by the same DAA.
    let (o1, q1, c1) = renew_tx(&lp, &sl, 1_150, 8);
    must("on time: 1,200 -> 2,000 at 1,150", &o1);
    let (ol, os) = (live(&o1, 0, 1_150, &q1), live(&o1, 1, 1_150, &c1));
    let (o2, q2, _) = renew_tx(&ol, &os, 1_950, 8);
    must("on time: 2,000 -> 2,800 at 1,950", &o2);
    assert_eq!(q2, r2, "the late underwriter's pair equals the on-time one");
}

/// R9-F2, a renewal after the lapse settle became due (the same transactions as at `7d9024e`). At 2,500 the settle due at
/// 2,400 and a renewal of the same pair still conflict (the pair UTXO was accepted at 300, before its expiry 1,200): one
/// renewal remains possible and postpones the settle by one term, to 3,200, for one premium buying a cover that is
/// already over (X = 2,000 < 2,500). The renewal's continuation, accepted at 2,500, after 2,000, is never renewed again:
/// the settle is due at 3,200 whatever the underwriter does. Before the fix the underwriter renewed again at once (to
/// 2,800, settle at 4,000), and again every term. This shape needs `tailDaa + idleDaa > termDaa` (the fixture: 1,200 >
/// 800); otherwise a renewal landing between the due settle and X + term resumes the cover (section 27).
#[test]
fn r9_bounded_a_renewal_after_the_lapse_settle_is_due_postpones_it_once_by_one_term() {
    let (lp, sl, _) = opened();
    let lock = 2_500u64;
    assert!(lp.s.lapse_from() <= lock);
    must("the lapse settle is due at 2,500", &lapse_tx(&lp, lock, 0));
    let (t1, r1, b1) = renew_tx(&lp, &sl, lock, 8);
    must("one renewal of the same pair at 2,500 (it conflicts with the settle)", &t1);
    let (l1, s1) = (live(&t1, 0, lock, &r1), live(&t1, 1, lock, &b1));
    refused("a second renewal: its UTXO was accepted after its expiry", &renew_tx(&l1, &s1, lock, 8).0, 0);
    refused("later too", &renew_tx(&l1, &s1, 2_800, 8).0, 0);
    assert_eq!(l1.s.lapse_from(), 3_200, "postponed by one term");
    refused("the settle one DAA early", &lapse_tx(&l1, 3_199, 0), 0);
    must("the settle at 3,200", &lapse_tx(&l1, 3_200, 0));
}

/// R9-F2, the parties' defence: any party entry accepted after the expiry (an amend costs nothing but its fee) makes the
/// pair unrenewable, so its lapse date is fixed. Control: an amend before the expiry does not stop an on-time renewal.
#[test]
fn r9_holds_a_party_entry_after_the_expiry_fixes_the_lapse_date() {
    let (lp, sl, _) = opened();
    let (at, a) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("the long lists after the expiry", &at);
    let la = live(&at, 0, 1_250, &a);
    refused("a renewal at 1,250 of the amended pair", &renew_tx(&la, &sl, 1_250, 8).0, 0);
    refused("a renewal at 2,500 of the amended pair", &renew_tx(&la, &sl, 2_500, 8).0, 0);
    assert_eq!(la.s.lapse_from(), 2_400);
    must("the settle at 2,400", &lapse_tx(&la, 2_400, 0));
    // Control: amended at 1,150 (before X), renewed at 1,180.
    let lb = live(&at, 0, 1_150, &a);
    must("an on-time renewal after an amend before the expiry", &renew_tx(&lb, &sl, 1_180, 8).0);
}

/// R9-N1 (a note; the contract is unchanged, the documentation corrected): the carrier of the new part of a partial
/// liquidation, which the underwriter's vouch funds (the pending part's `refund` is `B + carrier`), comes back to the
/// underwriter only on a cure. A valid settlement pays it `B + fee` and the carrier stays in the forfeited part (the
/// winner's); a void claim and a refuted one restore the part with the carrier in it (the parties'), the bond going to
/// the defender (and the disputer). 0.2 KAS per partial liquidation.
#[test]
fn r9_bounded_a_partial_liquidations_carrier_returns_to_the_underwriter_only_on_a_cure() {
    let (lp, _, _) = opened();
    let n = lp.s.size / 2;
    let c = lp.s.m.p.carrier;
    let spec = ClaimSpec { kind: LIQ, side: L, n, p: 920_000_000, t0: 450, lock: 450 };
    let lw = proposed(&lp, &spec, 500);
    assert_eq!(lw.s.refund, lw.s.bond + c);
    // A valid settlement.
    let (ft, s) = finalize_tx(&lw, 500, 200);
    must("the partial liquidation finalizes", &ft);
    let Settlement::Forfeit { uw, value, part } = s else { panic!("{s:?}") };
    assert_eq!(uw, lw.s.bond + lw.s.fee, "valid: the underwriter gets B + fee");
    assert_eq!(value, lw.value as i64 - lw.s.bond - lw.s.fee);
    assert!(value >= part.floor(), "valid: the carrier stays in the forfeited part (the winner's)");
    // A cure.
    let (ct, _) = cure_tx(&lw, &long_kp());
    must("the cure", &ct);
    assert_eq!(ct.0.outputs[0].value as i64, lw.s.bond + c, "cure: the underwriter gets B + carrier");
    // A void claim (landed at 1,100: P + W > X).
    let lv = proposed(&lp, &ClaimSpec { t0: 1_100, lock: 1_100, ..spec }, 1_100);
    let (vt, s) = finalize_tx(&lv, 1_100, 200);
    must("the void claim restores", &vt);
    let Settlement::Restore { part, value, defender, proposer, .. } = s else { panic!("{s:?}") };
    assert_eq!((defender, proposer), (0, lv.s.bond), "void: the bond back to the underwriter (review 11)");
    assert_eq!(value - part.range_of(n), c, "void: the carrier stays in the restored part");
    // A refuted claim.
    let st = stake(70, 10 * UNIT as u64, 0, 0xc0);
    let (ctx, w2, _, dj) = contest_tx(&lw, &st, 70, 520);
    must("contest", &ctx);
    let (cw, ld) = (live(&ctx, 0, 520, &w2), live(&ctx, 2, 520, &dj));
    let (dt, _, dw) = decide_tx(&ld, &cw, 620);
    must("resolve and decide (refuted)", &dt);
    let ldw = live(&dt, 1, 620, &dw);
    let (rt, s) = finalize_tx(&ldw, 520, 0);
    must("finalize the refuted part", &rt);
    let Settlement::Restore { part, value, defender, disputer, .. } = s else { panic!("{s:?}") };
    assert_eq!(defender + disputer, lw.s.bond, "refuted: the bond to the defender and the disputer");
    assert_eq!(value - part.range_of(n), c, "refuted: the carrier stays in the restored part");
}

// ---------------------------------------------------------------- R9-F3: each side's share of the headroom

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

/// `closeDonor` with no host on the listed short of `ll` at its bid: the entering short `sk3` posts `bs3`, P3's entry
/// `e3`.
fn donor_try(ll: &LPair, e3: i64, bs3: i64) -> T {
    let d = &ll.s;
    let sk3 = x(&keypair(46));
    let mut p3 = d.rest_of(d.size);
    (p3.short_key, p3.bs, p3.entry) = (sk3, bs3, e3);
    (p3.short_bid, p3.short_min, p3.short_tip) = (0, 0, 0);
    p3.mk = Makers { donor: maker_of(&ll.op), ..Makers::default() };
    let xp = d.short_bid;
    let mut spay = kpf_protocol::payoff::owed(d.size, d.bs - xp, d.short_tip, d.m.p.scale) + ll.value as i64 - d.range_of(d.size);
    if spay < DUST {
        spay = 0;
    }
    Walk::donor_custom(ll, xp, e3, bs3, sk3, &p3, spay)
}

/// `closeHost` with no donor on the listed long of `ll` at its ask: the entering long posts `bl3`, P3's entry `e3`.
fn host_try(ll: &LPair, e3: i64, bl3: i64) -> T {
    let h = &ll.s;
    let lk3 = x(&keypair(45));
    let mut p3 = h.rest_of(h.size);
    (p3.long_key, p3.bl, p3.entry) = (lk3, bl3, e3);
    (p3.long_ask, p3.long_min, p3.long_tip) = (0, 0, 0);
    p3.mk = Makers { host: maker_of(&ll.op), ..Makers::default() };
    let lpay = kpf_protocol::payoff::owed(h.size, h.long_ask - h.bl, h.long_tip, h.m.p.scale);
    Walk::host_custom(ll, h.long_ask, e3, bl3, lk3, &p3, lpay)
}

/// R9-F3 (the same transactions as at `7d9024e`). The fixture pair: entry 10, long bound 9, short bound 12, cover rate 6,
/// so the short's share reaches 13 and the long's 7. The short's top-up of 3 KAS (to the cover rate) is refused; one of
/// 1 KAS (to its share) is accepted, after which the long can still add 2 KAS (to 7, one sompi more refused), and a
/// true liquidation of the long at 9.20 still offers the cure (to 8.56), which the long takes. An entrant on the short
/// side may post at most 13 (`transfer`, `closeDonor` without a host), and an entrant on the long side at least 7
/// (`closeHost` without a donor); close-versus-close keeps the entry (an entrant's `e3 = E + 1` is refused). Before the
/// fix the short's top-up to 15 left the long no top-up and no cure, and its 200 KAS of equity at 9.20 were forfeited.
#[test]
fn r9_holds_neither_side_nor_an_entrant_takes_the_other_sides_share_of_the_headroom() {
    let (lp, _, _) = opened();
    let (e, cr) = (lp.s.entry, lp.s.b.cover_rate);
    assert_eq!((lp.s.short_reach(), lp.s.long_reach()), (1_300_000_000, 700_000_000));
    let p = 920_000_000i64;
    refused("the short's top-up of 3 KAS (the whole headroom)", &add_margin_tx(&lp, S, &short_kp(), 300_000_000).0, 0);
    refused("the short's top-up one sompi past its share", &add_margin_tx(&lp, S, &short_kp(), 100_000_001).0, 0);
    let (t, c) = add_margin_tx(&lp, S, &short_kp(), 100_000_000);
    must("the short's top-up of 1 KAS, to its share", &t);
    let lp2 = live(&t, 0, 400, &c);
    must("the long's top-up of 2 KAS after it, to its share", &add_margin_tx(&lp2, L, &long_kp(), 200_000_000).0);
    refused("the long's top-up one sompi past its share", &add_margin_tx(&lp2, L, &long_kp(), 200_000_001).0, 0);
    let lw = proposed(&lp2, &ClaimSpec { kind: LIQ, side: L, n: lp2.s.size, p, t0: 450, lock: 450 }, 500);
    assert_ne!(lw.s.cure_hash, [0; 32], "a cure is offered after the short used its share");
    let (ct, cured) = cure_tx(&lw, &long_kp());
    must("the long cures", &ct);
    assert_eq!(cured.bl, 856_520_000);
    assert!(cured.bl >= e - (cr - cr / 2), "the cure stays in the long's share");
    // Entrants on the short side.
    let (at, ls) = amend_tx(&lp, S, &short_kp(), 1_200_000_000, 20);
    must("the short lists", &at);
    let ll = live(&at, 0, 400, &ls);
    refused("a taker of the short posting bl + coverRate (15)", &transfer_tx(&ll, S, 1_200_000_000, ll.s.bl + cr, &keypair(46), 20).0, 0);
    refused("a taker of the short one sompi past the share", &transfer_tx(&ll, S, 1_200_000_000, 1_300_000_001, &keypair(46), 20).0, 0);
    let (tt, part) = transfer_tx(&ll, S, 1_200_000_000, 1_300_000_000, &keypair(46), 20);
    must("a taker of the short posting its share's end (13)", &tt);
    let lt = live(&tt, 1, 450, &part);
    must("the stayer's top-up of 2 KAS after the entrant", &add_margin_tx(&lt, L, &long_kp(), 200_000_000).0);
    refused("closeDonor alone: the entering short at 15", &donor_try(&ll, e, ll.s.bl + cr), 0);
    refused("closeDonor alone: one sompi past the share", &donor_try(&ll, e, 1_300_000_001), 0);
    refused("closeDonor alone: P3's entry moved up by one sompi", &donor_try(&ll, e + 1, 1_300_000_000), 0);
    must("closeDonor alone: the entry kept, the short at 13", &donor_try(&ll, e, 1_300_000_000));
    // Entrants on the long side.
    let (at, ls) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("the long lists", &at);
    let ll = live(&at, 0, 400, &ls);
    refused("closeHost alone: the entering long one sompi past the share", &host_try(&ll, e, 699_999_999), 0);
    refused("closeHost alone: P3's entry moved down by one sompi", &host_try(&ll, e - 1, 700_000_000), 0);
    must("closeHost alone: the entry kept, the long at 7", &host_try(&ll, e, 700_000_000));
    refused("a taker of the long one sompi past the share", &transfer_tx(&ll, L, 1_000_000_000, 699_999_999, &keypair(45), 20).0, 0);
    must("a taker of the long at 7", &transfer_tx(&ll, L, 1_000_000_000, 700_000_000, &keypair(45), 20).0);
    // An order taking the long with a pair (T2): its rate sets the entering bound.
    let sl = opened().1;
    let lo = order(L, &keypair(45), 0x75, 0);
    let with_rate = |r: i64| Live { s: OOpen { my_rate: r, ..lo.s.clone() }, ..lo.clone() };
    refused("an order taking the long with a rate one sompi past the share", &take_tx(&ll, &with_rate(300_000_001), &sl, 1_000_000_000, 450, true).0, 0);
    must("an order taking the long at the share's end", &take_tx(&ll, &with_rate(300_000_000), &sl, 1_000_000_000, 450, true).0);
}

/// R9-F3, the short side's cure: on the fixture pair (the long's share ends at 7, so the short may be cured up to 13) a
/// true liquidation of the short at 11.70 has the cure bound 12.5307, within its share: the cure is offered and taken.
/// At 12.40 the bound 13.2804 is beyond it: no cure, although the range rule alone (13.2804 - 9 <= 6) would offer it.
#[test]
fn r9_holds_a_short_cure_is_offered_only_within_the_shorts_share() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: S, n: lp.s.size, p: 1_170_000_000, t0: 450, lock: 450 }, 500);
    assert_eq!(lw.s.cure_bound, 1_253_070_000);
    let (ct, c) = cure_tx(&lw, &short_kp());
    must("the short cures within its share", &ct);
    assert!(c.bs <= lp.s.growth_limit(S));
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: S, n: lp.s.size, p: 1_240_000_000, t0: 450, lock: 450 }, 500);
    assert_eq!(lw.s.cure_hash, [0; 32], "no cure beyond the short's share");
    assert!(1_328_040_000 - lp.s.bl <= lp.s.b.cover_rate, "the range rule alone would offer it");
}

/// R9-F3, a side opened beyond its share: the long's bound 6.50 lies 3.50 below the entry (its share is 3), the short's
/// 11 lies 1 above it. The long keeps its bound but can grow no further (a top-up, or a taker posting below 6.50, is
/// refused; a taker keeping 6.50 is accepted); the short can grow to 12.50, the range rule's limit and below its share's
/// end (13), and no further.
#[test]
fn r9_holds_a_side_beyond_its_share_keeps_its_bound_but_cannot_grow() {
    let (lp, _, _) = opened();
    let gp = genesis_pair(OPair { bl: 650_000_000, bs: 1_100_000_000, mk: Makers::default(), ..lp.s.clone() }, 0, 0x84, 400);
    let gp = Live { value: gp.s.floor() as u64, ..gp };
    refused("the long's top-up of one sompi per unit", &add_margin_tx(&gp, L, &long_kp(), 1).0, 0);
    must("the short's top-up to 12.50", &add_margin_tx(&gp, S, &short_kp(), 150_000_000).0);
    refused("the short's top-up one sompi further", &add_margin_tx(&gp, S, &short_kp(), 150_000_001).0, 0);
    let (at, ls) = amend_tx(&gp, L, &long_kp(), 1_000_000_000, 20);
    must("the long lists", &at);
    let ll = live(&at, 0, 450, &ls);
    must("a taker of the long keeping 6.50", &transfer_tx(&ll, L, 1_000_000_000, 650_000_000, &keypair(45), 20).0);
    refused("a taker of the long one sompi lower", &transfer_tx(&ll, L, 1_000_000_000, 649_999_999, &keypair(45), 20).0, 0);
}

/// R9-F3, what the share rule costs: a cure is offered only within the loser's own share. On the untouched fixture pair a
/// liquidation of the long at 6.98 (a gap far below its trigger) has the cure bound 6.498: the range rule alone would
/// offer it (12 - 6.498 <= 6), but it lies below the long's share (7), so no cure is offered (the short's share up to 13
/// is kept for it). The long's reach for a cure falls from `bs - coverRate` (6) to `entry - ceil(coverRate / 2)` (7).
#[test]
fn r9_bounded_the_share_rule_offers_a_cure_only_within_the_losers_share() {
    let (lp, _, _) = opened();
    let p = 698_000_000i64;
    assert!(lp.s.long_liquidatable(p));
    let (g, m) = (lp.s.m.p.g_bps as i128, lp.s.m.p.mmr_bps as i128);
    let pg = (p as i128 * (10_000 - g) + 9_999) / 10_000;
    let cb = (pg * (10_000 - m) / 10_000) as i64;
    assert_eq!(cb, 649_838_000);
    assert!(lp.s.bs - cb <= lp.s.b.cover_rate, "the range rule alone would offer the cure");
    assert!(lp.s.short_reach() - cb > lp.s.b.cover_rate, "the cure would take the short's share");
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p, t0: 450, lock: 450 }, 500);
    assert_eq!(lw.s.cure_hash, [0; 32], "no cure is offered");
    must("the liquidation finalizes", &finalize_tx(&lw, 500, 200).0);
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
    /// The DAA the lineage was opened at (the calendar bound on its premiums).
    born: u64,
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
        self.pairs.push(WP { lp, truth, b0: p.b, renewals: vec![], born: self.now, silent });
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
        // Review 9 (R9-F3): the growing side leaves the other its reach (its bound or its share's end).
        let share_ok = if side == L { c.bl >= p.growth_limit(L) } else { c.bs <= p.growth_limit(S) };
        let legal = c.bl >= 0 && c.bl < c.bs && c.bs <= p.m.p.price_max && c.bs - c.bl <= p.b.cover_rate && share_ok;
        assert_eq!(accepted(&t), legal, "seed {} step {}: addMargin delta {delta} head {head}", self.seed, self.step);
        if legal {
            self.count("addMargin");
            self.pairs[i].lp = live(&t, 0, self.now, &c);
        } else {
            self.books.refused_as_ruled += 1;
        }
    }

    /// The stated rule of `KpfPairO.transfer` (`handOver`) for a whole hand-over of `side` at `xp` to `ek` posting `nb`:
    /// the listing, the no-downgrade rule, health at `xp`, the range within the cover rate and (review 9, R9-F3) the
    /// stayer's reach kept unless the entrant keeps the leaver's bound.
    fn hand_over_rule(p: &OPair, side: i64, xp: i64, nb: i64, ek: &[u8; 32]) -> bool {
        let mm = p.m.p.mmr_bps;
        xp >= 0
            && xp <= p.m.p.price_max
            && *ek != p.b.uw_key
            && if side == L {
                p.long_ask > 0
                    && xp >= p.long_ask
                    && nb >= 0
                    && nb <= p.bl
                    && (xp as i128) * (10_000 - mm) as i128 >= (nb as i128) * 10_000
                    && p.bs - nb <= p.b.cover_rate
                    && (nb == p.bl || nb >= p.growth_limit(L))
            } else {
                p.short_bid > 0
                    && xp <= p.short_bid
                    && nb >= p.bs
                    && nb <= p.m.p.price_max
                    && (nb as i128) * 10_000 >= (xp as i128) * (10_000 + mm) as i128
                    && nb - p.bl <= p.b.cover_rate
                    && (nb == p.bs || nb <= p.growth_limit(S))
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
        let _ = mm;
        let legal = Self::hand_over_rule(&p, side, xp, nb, &x(&ek));
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
        let e3 = if self.rng.chance(70) { d.entry } else { self.rng.around(d.entry + 10_000_000, 15_000_000) };
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
        let legal = xp >= 0 && xp <= d.m.p.price_max && xp <= d.short_bid && bs3 >= d.bs && bs3 <= d.m.p.price_max && e3 == d.entry && sk3 != d.b.uw_key && bs3 - d.bl <= d.b.cover_rate && (bs3 == d.bs || bs3 <= d.growth_limit(S));
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
        let e3 = if self.rng.chance(70) { h.entry } else { self.rng.around(h.entry - 10_000_000, 15_000_000) };
        let mut p3 = h.rest_of(h.size);
        p3.long_key = lk3;
        p3.bl = bl3;
        p3.entry = e3;
        (p3.long_ask, p3.long_min, p3.long_tip) = (0, 0, 0);
        p3.mk = Makers { host: maker_of(&w.lp.op), ..Makers::default() };
        let lpay = kpf_protocol::payoff::owed(h.size, xp - h.bl, h.long_tip, h.m.p.scale);
        let legal = xp >= 0 && xp <= h.m.p.price_max && xp >= h.long_ask && bl3 >= 0 && bl3 <= h.bl && e3 == h.entry && lk3 != h.b.uw_key && h.bs - bl3 <= h.b.cover_rate && (bl3 == h.bl || bl3 >= h.growth_limit(L));
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
        if a.lp.s.b != b.lp.s.b || a.lp.s.size != b.lp.s.size || !self.quiet_ok(&a.lp) || !self.quiet_ok(&b.lp) || b.lp.s.entry != a.lp.s.entry {
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
        // Review 9 (R9-F3): each passive side keeps its reach (P3's entering bound kept or within the share).
        let host_ok = donor.s.bl == host.s.bl || donor.s.bl >= host.s.growth_limit(L);
        let donor_ok = host.s.bs == donor.s.bs || host.s.bs <= donor.s.growth_limit(S);
        if !host_ok || !donor_ok {
            let (t, _) = cvc(&host, &donor, xp, xp, host.s.b);
            assert!(!accepted(&t), "seed {} step {}: close-versus-close taking a passive side's share", self.seed, self.step);
            self.books.refused_as_ruled += 1;
            return;
        }
        let (t, p3) = cvc(&host, &donor, xp, xp, host.s.b);
        must(&format!("seed {} step {}: close-versus-close", self.seed, self.step), &t);
        self.count("close-versus-close");
        let (lo, hi) = (i.min(j), i.max(j));
        let w = WP { lp: live(&t, 2, self.now + 20, &p3), truth: a.truth, b0: a.b0, renewals: a.renewals.clone(), born: a.born, silent: a.silent };
        self.pairs.remove(hi);
        self.pairs.remove(lo);
        self.pairs.push(w);
    }

    /// The underwriter's renewal, from one step before the expiry; a late underwriter (no renewal on some steps) renews
    /// late, and as often in a row as the contracts allow. The pair's verdict is compared with the rule of review 9 (the
    /// pair UTXO accepted no later than its expiry); every accepted renewal is checked against the calendar bound of
    /// section 27: a lineage opened at `born` has paid at most `1 + (now + step - born) / term` premiums (the open's
    /// included), the most an on-time underwriter charges by then. A catch-up renewal (less than half a term after the
    /// previous one: the residual of R9-F2) is counted, not a violation.
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
            let rule = w.lp.daa as i64 <= p.b.expiry;
            assert_eq!(verdicts(&t)[0], rule, "seed {} step {}: renewal of a UTXO accepted at {} with expiry {}", self.seed, self.step, w.lp.daa, p.b.expiry);
            if !accepted(&t) {
                self.books.refused_as_ruled += 1;
                if !rule {
                    self.count("renewal refused: the UTXO is younger than its expiry");
                }
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
                    self.count("renew: a catch-up of a late renewal");
                }
            }
            self.count("renew");
            self.slot = live(&t, 1, self.now, &b2);
            self.pairs[i].lp = live(&t, 0, self.now, &r2);
            self.pairs[i].renewals.push(self.now);
            let paid = 1 + self.pairs[i].renewals.len() as i64;
            let most = 1 + (self.now as i64 + step as i64 - w.born as i64) / p.m.bond.term;
            if paid > most {
                self.violation(format!("premiums ahead of the calendar: {paid} by DAA {} for a lineage opened at {} (premium {pay})", self.now, w.born));
            }
        }
    }

    /// A claim on the whole pair or on half of it, then (W later, the other pairs' clocks moving too) its finalize: a
    /// close by either party at the true price, a true liquidation (cured half the time), or a false liquidation that
    /// is contested: in time and refuted by an honest judge, in time and upheld because the colluders' YES stake leads
    /// (the pool then has a losing receipt), or after the window (the verdict ignored). Every pool is settled and
    /// closed. A claim landing too close to the expiry is void and restores the part; the remainder of a partial claim
    /// continues the lineage.
    fn claim(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        let p = w.lp.s.clone();
        if p.long_key == p.short_key {
            return;
        }
        let ms = p.m.p.min_size;
        let n = if self.rng.chance(30) && p.size / 2 >= ms && p.size - p.size / 2 >= ms { p.size / 2 } else { p.size };
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
        let (pt, pw) = propose_tx(&w.lp, &ClaimSpec { kind, side, n, p: price, t0, lock: self.now }, &uw_kp(), closer.as_ref());
        if !accepted(&pt) {
            self.books.refused_as_ruled += 1;
            return;
        }
        self.count(if kind == CLOSE { "propose close" } else if false_claim { "propose false liquidation" } else { "propose liquidation" });
        let lw = live(&pt, 0, self.now, &pw);
        let pdaa = self.now as i64;
        self.pairs.remove(i);
        if n < p.size {
            self.count("partial claim");
            self.pairs.push(WP { lp: live(&pt, 1, self.now, &p.rest_of(p.size - n)), ..w.clone() });
        }
        if false_claim {
            let mode = self.rng.below(3);
            let st = stake(70, 10 * UNIT as u64, 0, 0xc0);
            // Mode 2: the contest lands at P + W, after the window.
            let c_daa = if mode == 2 { self.now + p.m.p.challenge_daa as u64 } else { self.now + 10 };
            let (ct, w2, rc, dj) = contest_tx(&lw, &st, 70, c_daa);
            must("contest", &ct);
            let (cw, r, ld) = (live(&ct, 0, c_daa, &w2), live(&ct, 1, c_daa, &rc), live(&ct, 2, c_daa, &dj));
            let mut receipts = vec![r];
            let (ld, d_daa) = if mode == 1 {
                // Colluders outvote the opener: 13 units on YES against its 10 on NO (mu 1.25).
                let s3 = stake(71, 13 * UNIT as u64, 0, 0xc1);
                let (dt, d2, rs) = deposit_tx(&ld, YES, &[(s3, 71)], c_daa + 10);
                must("deposit on YES", &dt);
                receipts.push(live(&dt, 1, c_daa + 10, &rs[0]));
                (live(&dt, 0, c_daa + 10, &d2), c_daa + 10)
            } else {
                (ld, c_daa)
            };
            let r_daa = d_daa + p.m.judge.round as u64;
            let (dt, pool, dw) = decide_tx(&ld, &cw, r_daa);
            must("resolve and decide", &dt);
            let (lpool, ldw) = (live(&dt, 0, r_daa, &pool), live(&dt, 1, r_daa, &dw));
            let (ft, s) = finalize_tx(&ldw, ldw.s.clock, 0);
            must("finalize the decided part", &ft);
            match (mode, &s) {
                (0, Settlement::Restore { part, defender, disputer, .. }) => {
                    assert_eq!(*part, p.restore_state(n), "the refuted part is restored exactly");
                    assert_eq!(defender + disputer, pw.bond, "the bond to the defender and the disputer");
                    self.count("refuted, pool closed");
                }
                (0, _) => panic!("seed {} step {}: a refuted claim did not restore: {s:?}", self.seed, self.step),
                (_, Settlement::Forfeit { part, .. }) => {
                    assert_eq!((part.long_key, part.short_key), (p.short_key, p.short_key), "the upheld false claim forfeits the long");
                    self.count(if mode == 1 { "upheld by YES, pool with a loser closed" } else { "late contest ignored, pool closed" });
                }
                (_, Settlement::Restore { defender, disputer, .. }) => {
                    assert_eq!((*defender, *disputer), (0, 0), "an upheld but void claim's bond goes back to its proposer (review 11)");
                    self.count("upheld but void, restored");
                }
                (_, Settlement::Close { .. }) => panic!("a liquidation settled as a close"),
            }
            assert_eq!(pool.side, if mode == 1 { YES } else { NO });
            let (stx, p2, pays) = settle_tx(&lpool, &receipts, r_daa + 10);
            must("settle the receipts", &stx);
            if mode == 1 {
                assert_eq!(pays[0], 0, "the opener's NO receipt lost");
                assert!(pays[1] > receipts[1].value, "the YES receipt won a share of the loser's stake");
                self.count("contest won by YES, a pool with a losing receipt settled");
            }
            let lp2 = live(&stx, 0, r_daa + 10, &p2);
            let (ctx, _) = pool_close_tx(&lp2);
            must("close the pool", &ctx);
            self.now = self.now.max(r_daa + 10);
            let (Settlement::Restore { part, .. } | Settlement::Forfeit { part, .. }) = s else { unreachable!() };
            self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
            return;
        }
        if kind == LIQ && pw.cure_hash != [0; 32] && self.rng.chance(50) {
            let loser = self.key(if side == L { &p.long_key } else { &p.short_key });
            let (ct, c) = cure_tx(&lw, &loser);
            must("cure", &ct);
            self.count("cure");
            // Review 9 (R9-F3): the cure stays within the loser's share.
            if side == L {
                assert!(c.bl >= p.growth_limit(L), "seed {} step {}: a cure took the short's share", self.seed, self.step);
            } else {
                assert!(c.bs <= p.growth_limit(S), "seed {} step {}: a cure took the long's share", self.seed, self.step);
            }
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
                let (lo_, so_) = (q(n, c - p.bl, p.m.p.scale), q(n, p.bs - c, p.m.p.scale));
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
                assert_eq!(defender, 0, "a void claim's bond goes back to its proposer (review 11)");
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

    /// T2 with a pair: an order of the listed side takes it whole (`take_tx`: the pair's transfer to the order, the
    /// order's take, the slot's bind of the part at the pair's expiry). The pair's verdict is compared with the
    /// hand-over rule (review 9's share rule included); an accepted take also kept the order's own terms.
    fn take_pair(&mut self, i: usize) {
        let ls = if self.rng.chance(50) { L } else { S };
        self.ensure_listed(i, ls);
        let w = self.pairs[i].clone();
        let p = w.lp.s.clone();
        if !self.quiet_ok(&w.lp) || (ls == L && p.long_ask == 0) || (ls == S && p.short_bid == 0) {
            return;
        }
        let ek = self.entrant();
        let rate = [50_000_000i64, 100_000_000, 200_000_000, 300_000_000][self.rng.below(4) as usize];
        let xp = if ls == L { self.rng.around(p.long_ask, 20_000_000) } else { self.rng.around(p.short_bid, 20_000_000) }.max(0);
        let (op, id) = self.fresh_op();
        let o = OOpen { size_left: p.size, min_fill: p.size, my_rate: rate, limit: xp, min_counter_rate: 0, ..sample_order(&p.m, ls, x(&ek)) };
        let lo = Live { op, value: 2_000_000_000_000u64, daa: self.now, id: Some(id), s: o };
        let (t, part) = take_tx(&w.lp, &lo, &self.slot, xp, self.now, true);
        let nb = if ls == L { xp - rate } else { xp + rate };
        let rule = Self::hand_over_rule(&p, ls, xp, nb, &x(&ek));
        assert_eq!(verdicts(&t)[0], rule, "seed {} step {}: take with a pair, side {ls} x {xp} nb {nb}", self.seed, self.step);
        if !accepted(&t) {
            self.books.refused_as_ruled += 1;
            return;
        }
        assert!(if ls == L { p.entry >= xp } else { p.entry <= xp }, "the order took an entry against it");
        assert!(part.range_ok() && part.b.cover_rate >= part.bs - part.bl && part.b == p.b);
        let a = BindArgs::new(self.now as i64, p.b.expiry, 0, &[Claim::output(2, p.cover_of(p.size))]);
        let b2 = self.slot.s.bind(&slot_ctx(&self.slot, self.now), &a, self.slot.value as i64).expect("the slot's bind");
        self.slot = live(&t, 3, self.now, &b2);
        self.count("take with a pair");
        self.keys.insert(x(&ek), ek);
        self.pairs[i].lp = live(&t, 2, self.now, &part);
    }

    /// The shape of R9-F1: a take with no pair into an expiry in the past, then a renewal, which the pair refuses.
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
        let lp = live(&t, 1, self.now, &part);
        let s2 = live(&t, 2, self.now, &b2);
        let x2 = lp.s.b.expiry + lp.s.m.bond.term;
        let d = (x2 / lp.s.m.bond.step - 12 - s2.s.j0).max(0);
        let (rt, _, _) = renew_tx(&lp, &s2, self.now, d);
        if verdicts(&rt)[0] {
            self.violation(format!("R9-F1: a part taken at DAA {} with expiry {xe} was renewed", self.now));
        }
        self.books.hostile_refused += 1;
        self.count("hostile take, renewal refused");
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
                80..=87 => self.take_pair(i),
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
/// and beyond the headroom or a side's share), whole transfers to signing takers with free arguments drawn wide (each
/// compared with the stated rule, and a hostile variant moving the stayer, its listing, the entry or the binding, or
/// underpaying the leaver), takes by an order with a pair (the pair's verdict compared with the same rule),
/// `closeDonor` with no host and `closeHost` with no donor (the same), close-versus-close of two pairs of one binding
/// and entry, renewals in the window or late (a late underwriter; the pair's verdict compared with review 9's rule),
/// claims on the whole pair or half of it of every outcome (a close, a true liquidation cured or forfeited, a void
/// claim, a false liquidation contested in time and refuted, upheld by a colluding YES stake with a losing receipt in
/// its pool, or contested late and ignored; every pool settled and closed), and lapse settles. After every step every
/// live pair keeps its lineage's binding (only the renewals move the expiry, by one term each), its floor and its range
/// within the cover rate; every lineage has paid no more premiums than an on-time underwriter would have charged by
/// then. Since the fix of review 9 the walk records no violation.
#[test]
fn r9_holds_the_seeded_walk_over_every_entry_point() {
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
    for k in [
        "open",
        "amend",
        "addMargin",
        "transfer",
        "take with a pair",
        "closeDonor alone",
        "closeHost alone",
        "renew",
        "propose close",
        "partial claim",
        "lapse settle",
        "close-versus-close",
        "refuted, pool closed",
        "void claim restored",
        "liquidation settled",
    ] {
        assert!(total.steps.get(k).copied().unwrap_or(0) > 0, "the walk never ran {k}");
    }
    let upheld_yes = total.steps.get("contest won by YES, a pool with a losing receipt settled").copied().unwrap_or(0);
    let late = total.steps.get("late contest ignored, pool closed").copied().unwrap_or(0);
    let void_upheld = total.steps.get("upheld but void, restored").copied().unwrap_or(0);
    // Review 11: lateness voids nothing and a liquidation that cannot be valid is refused at the proposal, so an upheld void
    // claim (one delayed past its last valid landing) is rare here; it is counted, not required.
    println!("upheld but void claims: {void_upheld}");
    assert!(upheld_yes > 0 && late > 0, "the walk never ran a contest won by YES or a late contest");
    assert!(total.hostile_refused > 50);
    assert!(violations.is_empty(), "violations recorded: {violations:?}");
}

/// R9-F1 in the walk (the same shape as at `7d9024e`, where every seed found a burst): with the hostile take enabled,
/// a part taken into a past expiry is never renewed.
#[test]
fn r9_holds_the_seeded_walk_with_takes_into_the_past_finds_no_renewal() {
    let mut takes = 0;
    let mut violations = vec![];
    for seed in 1..=4u64 {
        let mut w = Walk::new(seed, true);
        w.play(120);
        takes += w.books.steps.get("hostile take, renewal refused").copied().unwrap_or(0);
        violations.extend(w.violations);
    }
    assert!(takes > 0, "the walk did not reach the hostile take");
    assert!(violations.is_empty(), "violations recorded: {violations:?}");
}

/// R9-F3's residual through a take without a pair: the counterparty, which writes the new lineage, chooses its entry
/// within the order's terms (a long order: any `e >= x`), and the shares are measured from it. At `e` = 12.20 on a cover
/// rate of 6 the long order (bound 9) lies beyond its share (floor 9.20): it cannot add margin, while the short may grow
/// to 15; in exchange the order's lapse payout is the whole range at that entry (2,000 units x 3.20 KAS). An entry near
/// the 64-bit edge is clamped to `priceMax` for the shares, so no share check overflows: a liquidation of the long still
/// proposes (no cure offered).
#[test]
fn r9_bounded_a_take_without_a_pair_sets_the_shares_by_the_entry_it_writes() {
    let now = 30_000u64;
    let lo = order(L, &long_kp(), 0x71, now - 100);
    let o = lo.s.clone();
    let sl = slot(K);
    let (t, part, _) = take_alone(&lo, &sl, &keypair(77), 1_000_000_000, 1_220_000_000, 1_220_000_000, 1_200, 600_000_000, o.prem_max, now);
    must("the take with the entry at the short's bound", &t);
    let lp = live(&t, 1, now, &part);
    assert_eq!(lp.s.growth_limit(L), 920_000_000);
    refused("the order's top-up", &add_margin_tx(&lp, L, &long_kp(), 1).0, 0);
    assert_eq!(lp.s.lapse_out(lp.value).0, q(o.size_left, 320_000_000, o.m.p.scale), "the lapse pays the long the whole range");
    let (t, part, _) = take_alone(&lo, &sl, &keypair(77), 1_000_000_000, i64::MAX / 2, 1_220_000_000, 1_200, 600_000_000, o.prem_max, now);
    must("the take with an extreme entry", &t);
    let lp = live(&t, 1, now, &part);
    // Review 11: a liquidation needs a pair UTXO that can carry a valid claim; this part (expiry 1,200) is shown as accepted
    // at 300, which leaves the share arithmetic of the propose exercised with the extreme entry.
    let lp = Live { daa: 300, ..lp };
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p: 900_000_000, t0: now as i64, lock: now }, now);
    assert_eq!(lw.s.cure_hash, [0; 32]);
}
