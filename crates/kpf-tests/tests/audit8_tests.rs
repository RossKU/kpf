//! Review 8 (docs/audit/REVIEW-8.md): the oracle-market family at commit `a6ed5bb` (after the makers of review 7).
//! Every transaction here runs every input in the real script engine with the covenant context (`common::run`).
//! `r8_exploit_` tests build an attack and assert that the engine ACCEPTS it; `r8_holds_` tests assert that an attack is
//! refused. Worked numbers use the fixture market of `oraclekit` (1,000 units at 10 KAS, long margin 1,000 KAS, short
//! margin 2,000 KAS, cover 6,000 KAS, expiry 1,200, tail 200, idle 1,000, W 200, quiet 20, a claim's bond 10 % of the
//! cover).
//!
//! R8-F1: `KpfPairO.closeDonor` lets whoever builds it choose P3's binding fields `cr3`, `x3`, `pr3` within one-sided
//! bounds only (`x3 <= expiry`, `pr3 <= premium`, `cr3 >= range`). Nothing makes a host exist, so the donor's short
//! (or anyone who buys its listing) moves the passive long into a P3 whose cover has already "lapsed" (`x3` in the
//! past): every claim on it is void and anyone settles it at once at the entry price.

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use common::*;
use kaspa_consensus_core::Hash;
use kpf_protocol::judge::{Js, NO, STATUS_RESOLVED, UNIT};
use kpf_protocol::oracle::*;
use oraclekit::*;

const KAS: i64 = 100_000_000;

/// The fixture pair with its short listed at `bid` (the short's own amend), live at DAA 400.
fn listed_short(bid: i64) -> (LPair, LSlot) {
    let (lp, sl, _) = opened();
    let (at, listed) = amend_tx(&lp, S, &short_kp(), bid, 20);
    must("the short lists its side", &at);
    (live(&at, 0, 400, &listed), sl)
}

/// `closeDonor` with no host at all: in 0 the donor pair (closeDonor, the short's listing filled at `xp`), 1 the
/// short's own funding (P2PK of `short_kp`, it authors P3's genesis); out 0 the short's payout, 1 P3 (the donor's long
/// against `sk3` with bound `bs3`, entry `e3`, the binding (uwId, uwKey, cr3, x3, pr3)), 2 change. Returns the
/// transaction, P3's state and the short's payout.
#[allow(clippy::too_many_arguments)]
fn donor_alone(donor: &LPair, xp: i64, e3: i64, bs3: i64, sk3: [u8; 32], cr3: i64, x3: i64, pr3: i64) -> (T, OPair, i64) {
    let d = &donor.s;
    let n = d.size;
    let mut p3 = d.rest_of(n);
    p3.short_key = sk3;
    p3.bs = bs3;
    p3.entry = e3;
    (p3.short_bid, p3.short_min, p3.short_tip) = (0, 0, 0);
    (p3.b.cover_rate, p3.b.expiry, p3.b.premium) = (cr3, x3, pr3);
    p3.mk = Makers { donor: maker_of(&donor.op), ..Makers::default() };
    let args = vec![iv(n), iv(xp), iv(e3), iv(1), iv(bs3), bv(&sk3), iv(0), iv(0), iv(0), iv(cr3), iv(x3), iv(pr3), bv(&[0; 32])];
    let spay = kpf_protocol::payoff::owed(n, d.bs - xp, d.short_tip, d.m.p.scale) + donor.value as i64 - d.range_of(n);
    let spay = if spay < 2_000_000 { 0 } else { spay };
    let fund = key_in(outpoint(0xa1), 500_000_000_000, 0, &short_kp());
    let fund_op = fund.outpoint;
    let ins = vec![pair_in(donor, "closeDonor", args), fund];
    let mut outs = vec![
        out(spay.max(2_000_000) as u64, key_spk(&d.short_key), None),
        out(p3.floor() as u64, pair_spk(&p3), None),
        out(400_000_000_000, p2pk_spk(&short_kp()), None),
    ];
    seal(&mut outs, 1, 1, fund_op);
    (build_ext(&ins, outs, 400, &[seq_quiet(donor), 0]), p3, spay)
}

// ---------------------------------------------------------------- R8-F1: closeDonor writes P3's binding

/// R8-F1, the theft. The price has risen to 11.50 (true; the long is owed 2,500 KAS of the 3,000 KAS range at that
/// price, the short 500 KAS). The short lists its side at 11.50 and, `quietDaa` later, runs `closeDonor` alone: its
/// listing is "filled" at 11.50 by itself (paid 500 KAS from the pair), it funds P3 with the difference (500.2 KAS) and
/// names its own key as P3's short and an expiry in the past (`x3 = -750`, so `x3 + tail + idle = 450`). P3 lands at DAA
/// 450, while the donor pair's own cover runs to 1,200. At once anyone lapse-settles P3 at the entry price 10: the long
/// gets 1,000 KAS, the short 2,000.2 KAS. The short leaves with its entry margin (2,000 KAS) instead of 500; the long,
/// which signed nothing and had a covered pair, loses 1,500 KAS. No underwriter, no claim, no contest.
#[test]
fn r8_exploit_short_alone_forces_its_long_into_a_lapsed_p3_and_settles_at_entry() {
    let p_true = 1_150_000_000i64;
    let (donor, _) = listed_short(p_true);
    let d = donor.s.clone();
    assert_eq!(d.b.expiry, 1_200, "the donor pair's cover runs to 1,200 (claims valid while P + W <= 1,200)");
    let (t, p3, spay) = donor_alone(&donor, p_true, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, -750, d.b.premium);
    must("closeDonor with no host, P3's expiry in the past", &t);
    let funded = p3.floor() - (donor.value as i64 - spay);
    let lp3 = live(&t, 1, 450, &p3);
    assert_eq!(lp3.s.lapse_from(), 450, "P3 is due for the lapse settle the moment it lands");
    assert_eq!(lp3.s.long_key, d.long_key);
    let lt = lapse_tx(&lp3, 450, 0);
    must("anyone lapse-settles P3 at once", &lt);
    let (long_gets, short_gets) = lp3.s.lapse_out(lp3.value);
    assert_eq!(long_gets, 1_000 * KAS, "the long is paid at the entry price 10");
    assert_eq!(d.long_at(p_true), 2_500 * KAS, "what the long's side is worth at the true price");
    let short_net = spay + short_gets - funded;
    let c = d.m.p.carrier;
    assert_eq!(spay, 500 * KAS + c, "the short's payout at its own listing (bs - x, and the carrier)");
    assert_eq!(funded, 500 * KAS + c, "what the short adds to P3");
    assert_eq!(short_net, 2_000 * KAS + c, "the short leaves with its whole entry margin");
    assert_eq!(d.short_at(p_true), 500 * KAS, "what the short's side is worth at the true price");
    // The donor pair's own lapse settle is not due before 2,400: the cover was live.
    refused("the donor pair itself cannot be lapse-settled before 2,400", &lapse_tx(&donor, 450, 0), 0);
}

/// R8-F1, the escape from a due liquidation. The price is 13.50, beyond the short's bound 12: a vouched liquidation of
/// the short at 13.50 triggers (the long would get the whole 3,000 KAS range). The short lists at its bound and runs
/// `closeDonor` alone with `x3` in the past; the liquidation the underwriter then vouches on P3 is void (P + W > X3), its
/// bond is paid to the short (the defender), and the lapse settle at entry pays the long 1,000 KAS: the short keeps
/// 2,000 KAS of margin it had lost.
#[test]
fn r8_exploit_a_short_past_its_trigger_escapes_its_liquidation() {
    let p_true = 1_350_000_000i64;
    let (donor, _) = listed_short(1_200_000_000);
    let d = donor.s.clone();
    assert!(d.short_liquidatable(p_true), "a liquidation of the short at 13.50 triggers on the donor pair");
    let lw0 = proposed(&donor, &ClaimSpec { kind: LIQ, side: S, n: d.size, p: p_true, t0: 395, lock: 400 }, 400);
    let (ft0, s0) = finalize_tx(&lw0, 400, 200);
    must("control: on the donor pair the liquidation is valid and forfeits the short's side", &ft0);
    assert!(matches!(s0, Settlement::Forfeit { .. }), "{s0:?}");
    let (t, p3, spay) = donor_alone(&donor, 1_200_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, -750, d.b.premium);
    must("closeDonor with no host (the short past its trigger)", &t);
    let lp3 = live(&t, 1, 450, &p3);
    let lw = proposed(&lp3, &ClaimSpec { kind: LIQ, side: S, n: p3.size, p: p_true, t0: 455, lock: 460 }, 460);
    let (ft, s) = finalize_tx(&lw, 460, 200);
    must("the true liquidation on P3 finalizes", &ft);
    let Settlement::Restore { defender, .. } = s else { panic!("the liquidation on P3 is void: {s:?}") };
    assert_eq!(lw.s.defender(), x(&short_kp()));
    assert_eq!(defender, 600 * KAS, "the underwriter's 600 KAS bond goes to the short");
    let (long_gets, short_gets) = lp3.s.lapse_out(lp3.value);
    must("lapse settle of P3", &lapse_tx(&lp3, 460, 0));
    assert_eq!(long_gets, 1_000 * KAS);
    let funded = p3.floor() - (donor.value as i64 - spay);
    assert_eq!(spay + short_gets - funded, 2_000 * KAS + d.m.p.carrier, "the short keeps its entry margin");
}

/// R8-F1: the long cannot rescue itself through the claim path either: a close it proposes at the true price on P3
/// (vouched by the honest underwriter) is void, and its own bond (600 KAS, posted by the closer) goes to the short.
#[test]
fn r8_exploit_the_longs_own_close_on_the_lapsed_p3_pays_its_bond_to_the_short() {
    let p_true = 1_150_000_000i64;
    let (donor, _) = listed_short(p_true);
    let d = donor.s.clone();
    let (t, p3, _) = donor_alone(&donor, p_true, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, -750, d.b.premium);
    must("closeDonor with no host", &t);
    let lp3 = live(&t, 1, 450, &p3);
    let lw = proposed(&lp3, &ClaimSpec { kind: CLOSE, side: L, n: p3.size, p: p_true, t0: 455, lock: 460 }, 460);
    let (ft, s) = finalize_tx(&lw, 460, 200);
    must("the long's close finalizes", &ft);
    let Settlement::Restore { defender, .. } = s else { panic!("void: {s:?}") };
    assert_eq!(lw.s.defender(), x(&short_kp()));
    assert_eq!(defender, 600 * KAS);
}

/// R8-F1, the other free field: with `x3` equal to the donor's expiry the short can still set P3's cover rate to
/// `priceMax`. The range stays within the old cover rate (so `closeDonor` accepts), but no slot can ever count the
/// cover of a renewal (q(n, priceMax) = 10^17 sompi) and every claim's bond becomes q(n, priceMax * sigma) =
/// 100,000,000 KAS: P3 can be neither renewed nor liquidated, and it lapse-settles at entry after its expiry.
#[test]
fn r8_exploit_close_donor_inflates_the_cover_rate_so_no_claim_or_renewal_is_affordable() {
    let (donor, sl) = listed_short(1_150_000_000);
    let d = donor.s.clone();
    let pmax = d.m.p.price_max;
    let (t, p3, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&short_kp()), pmax, d.b.expiry, d.b.premium);
    must("closeDonor with cr3 = priceMax", &t);
    assert_eq!(d.m.bond_of(p3.size, pmax), 10_000_000_000_000_000, "a claim's bond on P3: 100,000,000 KAS");
    assert_eq!(d.m.bond_of(d.size, d.b.cover_rate), 600 * KAS, "on the donor pair: 600 KAS");
    let lp3 = live(&t, 1, 450, &p3);
    let lsl = Live { daa: 450, ..sl };
    let (rt, _, _) = renew_tx(&lp3, &lsl, 1_100, 8);
    not_accepted("a renewal of P3 (the slot cannot count q(n, priceMax))", &rt);
    let (rt0, _, _) = renew_tx(&Live { s: OPair { b: d.b, mk: Makers::default(), ..p3.clone() }, ..lp3.clone() }, &lsl, 1_100, 8);
    must("control: the same P3 with the donor's cover rate renews", &rt0);
}

/// R8-F1, the freeze. With `x3` far in the past (`-4 * 10^18`) the lapse settle's end `x3 + tail + idle` is negative,
/// which `lapseSettle` refuses at every DAA, a renewal's `x3 + term` is no ring boundary any slot accepts, and every
/// claim is void. P3 has no exit that needs nobody: the long leaves only by the short's consent (mutual or cooperative
/// close) or by finding a taker for a pair nobody can settle. The short pays the donor's own listing to itself.
#[test]
fn r8_exploit_close_donor_freezes_the_long_with_an_expiry_far_in_the_past() {
    let (donor, sl) = listed_short(1_150_000_000);
    let d = donor.s.clone();
    let x3 = -4_000_000_000_000_000_000i64;
    let (t, p3, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, x3, d.b.premium);
    must("closeDonor with x3 = -4e18", &t);
    let lp3 = live(&t, 1, 450, &p3);
    for lock in [450u64, 2_400, 100_000_000, 499_999_999_999] {
        refused("lapse settle of the frozen P3", &lapse_tx(&lp3, lock, 0), 0);
    }
    let (rt, _, _) = renew_tx(&lp3, &Live { daa: 450, ..sl }, 1_100, 8);
    not_accepted("a renewal of the frozen P3", &rt);
    let lw = proposed(&lp3, &ClaimSpec { kind: CLOSE, side: L, n: p3.size, p: 1_150_000_000, t0: 455, lock: 460 }, 460);
    let (ft, s) = finalize_tx(&lw, 460, 200);
    must("the long's close finalizes", &ft);
    assert!(matches!(s, Settlement::Restore { .. }), "void: {s:?}");
}

/// The one direction the contract does bound: P3's expiry later than the donor's is refused.
#[test]
fn r8_holds_close_donor_refuses_a_later_expiry_or_premium() {
    let (donor, _) = listed_short(1_150_000_000);
    let d = donor.s.clone();
    let (t, _, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, d.b.expiry + 100, d.b.premium);
    refused("x3 later than the donor's expiry", &t, 0);
    let (t, _, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, d.b.expiry, d.b.premium + 1);
    refused("pr3 above the donor's premium", &t, 0);
    let (t, _, _) = donor_alone(&donor, 1_150_000_000, d.entry - 1, d.bs, x(&short_kp()), d.b.cover_rate, d.b.expiry, d.b.premium);
    refused("e3 below the donor's entry", &t, 0);
    let (t, _, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&uw_kp()), d.b.cover_rate, d.b.expiry, d.b.premium);
    refused("the underwriter as P3's short", &t, 0);
}

/// The same shape with P3's binding equal to the donor's (what the fix would require) is an ordinary sale of the short's
/// listing: the long keeps its expiry, its claims and its lapse date.
#[test]
fn r8_holds_close_donor_with_the_donors_binding_keeps_the_long_covered() {
    let (donor, _) = listed_short(1_150_000_000);
    let d = donor.s.clone();
    let (t, p3, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, d.b.expiry, d.b.premium);
    must("closeDonor with the donor's binding", &t);
    let lp3 = live(&t, 1, 450, &p3);
    assert_eq!(lp3.s.lapse_from(), d.lapse_from());
    refused("no early lapse settle", &lapse_tx(&lp3, 450, 0), 0);
    let lw = proposed(&lp3, &ClaimSpec { kind: CLOSE, side: L, n: p3.size, p: 1_150_000_000, t0: 455, lock: 460 }, 460);
    let (ft, s) = finalize_tx(&lw, 460, 200);
    must("the long's close is valid", &ft);
    assert!(matches!(s, Settlement::Close { .. }), "{s:?}");
}

/// The host side cannot do the same to the passive short: `closeHost` copies the host's own binding into P3 (only the
/// entry, the long's bound and the long's listing are arguments), so a P3 with an earlier expiry is refused.
#[test]
fn r8_holds_close_host_cannot_shorten_the_shorts_cover() {
    let (lp, _, _) = opened();
    let (at, h) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("the host lists its long", &at);
    let host = live(&at, 0, 400, &h);
    let hs = &host.s;
    let n = hs.size;
    let build = |x3: i64| {
        let mut p3 = hs.rest_of(n);
        p3.long_key = x(&long_kp());
        (p3.long_ask, p3.long_min, p3.long_tip) = (0, 0, 0);
        p3.b.expiry = x3;
        p3.mk = Makers { host: maker_of(&host.op), ..Makers::default() };
        let args = vec![iv(n), iv(1_000_000_000), iv(hs.entry), iv(1), iv(hs.bl), bv(&x(&long_kp())), iv(0), iv(0), iv(0), bv(&[0; 32])];
        let lpay = kpf_protocol::payoff::owed(n, 1_000_000_000 - hs.bl, 0, hs.m.p.scale);
        let fund = key_in(outpoint(0xa2), 500_000_000_000, 0, &long_kp());
        let fop = fund.outpoint;
        let mut outs = vec![out(lpay as u64, key_spk(&hs.long_key), None), out(p3.floor() as u64, pair_spk(&p3), None), change(90_000_000_000)];
        seal(&mut outs, 1, 1, fop);
        build_ext(&[pair_in(&host, "closeHost", args), fund], outs, 400, &[seq_quiet(&host), 0])
    };
    must("control: closeHost alone with the host's binding", &build(hs.b.expiry));
    refused("closeHost writing an earlier expiry", &build(-750), 0);
}

// ---------------------------------------------------------------- multi-entry shapes that hold

/// A decided part and its pool (and a receipt) spent in one transaction: the part's finalize counts one input of the
/// claim's id, the pool's settle 1 + k: neither accepts the other beside it, so a builder cannot pair the restore of a
/// refuted part with a settlement of the same pool to reuse one output for both.
#[test]
fn r8_holds_a_decided_part_and_its_pool_are_never_spent_together() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p: 920_000_000, t0: 450, lock: 450 }, 500);
    let st = stake(70, 10 * UNIT as u64, 0, 0xc0);
    let (ct, w2, rc, dj) = contest_tx(&lw, &st, 70, 520);
    must("contest", &ct);
    let (cw, r, ld) = (live(&ct, 0, 520, &w2), live(&ct, 1, 520, &rc), live(&ct, 2, 520, &dj));
    let (dt, pool, dw) = decide_tx(&ld, &cw, 620);
    must("resolve and decide (refuted)", &dt);
    let (lpool, ldw) = (live(&dt, 0, 620, &pool), live(&dt, 1, 620, &dw));
    let id = ldw.id.unwrap();
    let (ft, s) = finalize_tx(&ldw, 520, 0);
    must("control: the refuted part finalizes alone", &ft);
    let Settlement::Restore { part, value, .. } = s else { panic!("{s:?}") };
    let ins = vec![pend_in(&ldw, "finalize", vec![]), stake_in(&lpool, "settle", vec![b8(2), b8(1)]), stake_in(&r, "redeem", vec![b8(1)]), exec_in(0xc1)];
    let free = Js::free(r.s.owner, lpool.s.m);
    let outs = vec![
        out(600 * KAS as u64, key_spk(&ldw.s.defender()), None),
        out(value as u64, pair_spk(&part), bind(0, id)),
        out(r.value, stake_spk(&free), None),
        out(lpool.value, stake_spk(&lpool.s), bind(1, id)),
        change(90_000_000_000),
    ];
    let t = build_ext(&ins, outs, 700, &[0, 0, 0, 0]);
    refused("finalize beside the pool", &t, 0);
    refused("settle beside the decided part", &t, 1);
}

// ---------------------------------------------------------------- R8-F2: two pools share one burn output

/// A resolved pool whose every receipt has been settled, holding its base and a burn `burn`, under id `id`.
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

/// R8-F2: `KpfStake.close(j)` names its burn output by an argument and checks only that output j is the burn script with
/// at least this pool's burn. Two pools closed in one transaction with the same j share one burn output: the losers'
/// 10 % burned stake of the second pool (1 KAS each here) goes to whoever closes. Anyone may close.
#[test]
fn r8_exploit_two_pools_closed_together_share_one_burn_output() {
    let (a, b) = (settled_pool(0xb1, UNIT as u64), settled_pool(0xb2, UNIT as u64));
    let ins = vec![stake_in(&a, "close", vec![b8(1)]), stake_in(&b, "close", vec![b8(1)]), exec_in(0xb3)];
    let outs = vec![change(2 * DISPUTE_BASE + UNIT as u64 + 90_000_000_000), out(UNIT as u64, kpf_protocol::lock::burn_spk(), None)];
    let t = build_ext(&ins, outs, 0, &[0, 0, 0]);
    must("two pools close against one burn output of one pool's burn", &t);
    let burned = t.0.outputs[1].value;
    assert_eq!(burned, UNIT as u64, "1 KAS burned of the 2 KAS due; the closer keeps the other");
}
