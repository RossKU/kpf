//! Review 8 (docs/audit/REVIEW-8.md): the oracle-market family at commit `a6ed5bb` (after the makers of review 7).
//! Every transaction here runs every input in the real script engine with the covenant context (`common::run`).
//! `r8_holds_` tests assert that an attack is refused (since the fix every former `r8_exploit_` test is one). Worked
//! numbers use the fixture market of `oraclekit` (1,000 units at 10 KAS, long margin 1,000 KAS, short margin 2,000 KAS,
//! cover 6,000 KAS, expiry 1,200, tail 200, idle 1,000, W 200, quiet 20, a claim's bond 10 % of the cover).
//!
//! R8-F1: `KpfPairO.closeDonor` let whoever built it choose P3's binding fields `cr3`, `x3`, `pr3` within one-sided
//! bounds only (`x3 <= expiry`, `pr3 <= premium`, `cr3 >= range`), and nothing makes a host exist, so the donor's short
//! (or anyone who bought its listing) moved the passive long into a P3 whose cover had already "lapsed". Fixed
//! (docs/DESIGN-ORACLE-IMPL.md 26): P3 carries the donor's own binding byte for byte and the three arguments are gone.
//! Each test below builds the transaction of a former exploit, asserts that the donor refuses it, and checks that the
//! one shape left (P3 with the donor's binding) keeps the long covered.
//!
//! R8-F2: `KpfStake.close(j)` named its burn output by argument. Fixed: `j == M - 1 - self` and `j >= inputs`.

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
/// against `sk3` with bound `bs3`, entry `e3`, the binding (uwId, uwKey, cr3, x3, pr3)), 2 change. Since review 8 the
/// entry takes no binding argument: `cr3`, `x3`, `pr3` only shape the P3 output the builder writes. Returns the
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
    let args = vec![iv(n), iv(xp), iv(e3), iv(1), iv(bs3), bv(&sk3), iv(0), iv(0), iv(0), bv(&[0; 32])];
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

/// The control every former exploit ends with: the same sale with P3 carrying the donor's binding is accepted, P3's
/// binding is the donor's, the long keeps the donor pair's lapse date (nothing settles P3 early), and its close at
/// `p_true` vouched on P3 is valid.
fn the_long_keeps_its_cover(donor: &LPair, xp: i64, p_true: i64) {
    let d = donor.s.clone();
    let (t, p3, _) = donor_alone(donor, xp, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, d.b.expiry, d.b.premium);
    must("control: closeDonor writing the donor's own binding", &t);
    assert_eq!(p3.b, d.b, "P3's binding is the donor's");
    let lp3 = live(&t, 1, 450, &p3);
    assert_eq!(lp3.s.lapse_from(), d.lapse_from(), "the long keeps its lapse date");
    refused("no early lapse settle of P3", &lapse_tx(&lp3, 450, 0), 0);
    let lw = proposed(&lp3, &ClaimSpec { kind: CLOSE, side: L, n: p3.size, p: p_true, t0: 455, lock: 460 }, 460);
    let (ft, s) = finalize_tx(&lw, 460, 200);
    must("the long's close on P3 is valid", &ft);
    assert!(matches!(s, Settlement::Close { .. }), "{s:?}");
}

// ---------------------------------------------------------------- R8-F1: closeDonor writes P3's binding

/// R8-F1, the theft (was `r8_exploit_short_alone_forces_its_long_into_a_lapsed_p3_and_settles_at_entry`). The price has
/// risen to 11.50; the short lists its side at 11.50 and runs `closeDonor` alone, naming its own key as P3's short and
/// an expiry in the past (`x3 = -750`: the lapse settle due at 450). Before the fix the long, which signed nothing, was
/// settled at the entry price and lost 1,500 KAS. Now the donor refuses every P3 expiry but its own, earlier or later.
#[test]
fn r8_holds_a_short_alone_cannot_move_its_long_into_a_lapsed_p3() {
    let p_true = 1_150_000_000i64;
    let (donor, _) = listed_short(p_true);
    let d = donor.s.clone();
    assert_eq!(d.b.expiry, 1_200, "the donor pair's cover runs to 1,200 (claims valid while P + W <= 1,200)");
    let (t, _, _) = donor_alone(&donor, p_true, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, -750, d.b.premium);
    refused("closeDonor with no host, P3's expiry in the past", &t, 0);
    for x3 in [d.b.expiry - 1, d.b.expiry - 600, 0, d.b.expiry + 1] {
        let (t, _, _) = donor_alone(&donor, p_true, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, x3, d.b.premium);
        refused(&format!("P3 expiry {x3}, not the donor's {}", d.b.expiry), &t, 0);
    }
    the_long_keeps_its_cover(&donor, p_true, p_true);
}

/// R8-F1, the escape from a due liquidation (was `r8_exploit_a_short_past_its_trigger_escapes_its_liquidation`). At
/// 13.50 a liquidation of the short is valid on the donor pair. The short's `closeDonor` into a P3 with a past expiry is
/// refused; the only P3 it can write carries the donor's expiry, and the same liquidation on it is valid and forfeits.
#[test]
fn r8_holds_a_short_past_its_trigger_cannot_escape_its_liquidation() {
    let p_true = 1_350_000_000i64;
    let (donor, _) = listed_short(1_200_000_000);
    let d = donor.s.clone();
    assert!(d.short_liquidatable(p_true), "a liquidation of the short at 13.50 triggers on the donor pair");
    let lw0 = proposed(&donor, &ClaimSpec { kind: LIQ, side: S, n: d.size, p: p_true, t0: 395, lock: 400 }, 400);
    let (ft0, s0) = finalize_tx(&lw0, 400, 200);
    must("control: on the donor pair the liquidation is valid and forfeits the short's side", &ft0);
    assert!(matches!(s0, Settlement::Forfeit { .. }), "{s0:?}");
    let (t, _, _) = donor_alone(&donor, 1_200_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, -750, d.b.premium);
    refused("closeDonor with no host and a past expiry (the short past its trigger)", &t, 0);
    let (t, p3, _) = donor_alone(&donor, 1_200_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, d.b.expiry, d.b.premium);
    must("closeDonor with the donor's binding", &t);
    let lp3 = live(&t, 1, 450, &p3);
    let lw = proposed(&lp3, &ClaimSpec { kind: LIQ, side: S, n: p3.size, p: p_true, t0: 455, lock: 460 }, 460);
    let (ft, s) = finalize_tx(&lw, 460, 200);
    must("the true liquidation on P3 finalizes", &ft);
    assert!(matches!(s, Settlement::Forfeit { .. }), "the liquidation on P3 is valid and forfeits: {s:?}");
}

/// R8-F1 (was `r8_exploit_the_longs_own_close_on_the_lapsed_p3_pays_its_bond_to_the_short`): the lapsed P3 on which the
/// long's own close was void cannot be written; on the P3 that can, the long's close is valid.
#[test]
fn r8_holds_the_longs_own_close_stays_valid_after_a_donor_sale() {
    let p_true = 1_150_000_000i64;
    let (donor, _) = listed_short(p_true);
    let d = donor.s.clone();
    let (t, _, _) = donor_alone(&donor, p_true, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, -750, d.b.premium);
    refused("closeDonor into a lapsed P3", &t, 0);
    the_long_keeps_its_cover(&donor, p_true, p_true);
}

/// R8-F1, the other free field (was `r8_exploit_close_donor_inflates_the_cover_rate_so_no_claim_or_renewal_is_affordable`):
/// a P3 cover rate of `priceMax`, or any rate but the donor's (up or down, down to the range itself), is refused; the P3
/// the donor can write renews like the donor pair.
#[test]
fn r8_holds_close_donor_cannot_change_the_cover_rate() {
    let (donor, sl) = listed_short(1_150_000_000);
    let d = donor.s.clone();
    let pmax = d.m.p.price_max;
    for cr3 in [pmax, d.b.cover_rate + 1, d.b.cover_rate - 1, d.bs - d.bl] {
        let (t, _, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&short_kp()), cr3, d.b.expiry, d.b.premium);
        refused(&format!("closeDonor with cr3 = {cr3}"), &t, 0);
    }
    let (t, p3, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, d.b.expiry, d.b.premium);
    must("closeDonor with the donor's cover rate", &t);
    let lp3 = live(&t, 1, 450, &p3);
    let (rt, _, _) = renew_tx(&lp3, &Live { daa: 450, ..sl }, 1_100, 8);
    must("P3 renews like the donor pair", &rt);
}

/// R8-F1, the freeze (was `r8_exploit_close_donor_freezes_the_long_with_an_expiry_far_in_the_past`): an expiry of
/// `-4 * 10^18` (no lapse settle at any DAA) or of `2^63 - 1` is refused.
#[test]
fn r8_holds_close_donor_cannot_freeze_the_long() {
    let (donor, _) = listed_short(1_150_000_000);
    let d = donor.s.clone();
    for x3 in [-4_000_000_000_000_000_000i64, i64::MAX] {
        let (t, _, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, x3, d.b.premium);
        refused(&format!("closeDonor with x3 = {x3}"), &t, 0);
    }
}

/// The bounds that stay, and the premium now fixed both ways: a later expiry, a higher or lower premium, an entry below
/// the donor's, the underwriter as P3's short.
#[test]
fn r8_holds_close_donor_refuses_a_later_expiry_or_premium() {
    let (donor, _) = listed_short(1_150_000_000);
    let d = donor.s.clone();
    let (t, _, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, d.b.expiry + 100, d.b.premium);
    refused("x3 later than the donor's expiry", &t, 0);
    let (t, _, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, d.b.expiry, d.b.premium + 1);
    refused("pr3 above the donor's premium", &t, 0);
    let (t, _, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&short_kp()), d.b.cover_rate, d.b.expiry, d.b.premium - 1);
    refused("pr3 below the donor's premium", &t, 0);
    let (t, _, _) = donor_alone(&donor, 1_150_000_000, d.entry - 1, d.bs, x(&short_kp()), d.b.cover_rate, d.b.expiry, d.b.premium);
    refused("e3 below the donor's entry", &t, 0);
    let (t, _, _) = donor_alone(&donor, 1_150_000_000, d.entry, d.bs, x(&uw_kp()), d.b.cover_rate, d.b.expiry, d.b.premium);
    refused("the underwriter as P3's short", &t, 0);
}

/// The same shape with P3's binding equal to the donor's (the only one the fix accepts) is an ordinary sale of the
/// short's listing: the long keeps its expiry, its claims and its lapse date.
#[test]
fn r8_holds_close_donor_with_the_donors_binding_keeps_the_long_covered() {
    let (donor, _) = listed_short(1_150_000_000);
    the_long_keeps_its_cover(&donor, 1_150_000_000, 1_150_000_000);
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
    let build = |x3: i64, cr3: i64, pr3: i64| {
        let mut p3 = hs.rest_of(n);
        p3.long_key = x(&long_kp());
        (p3.long_ask, p3.long_min, p3.long_tip) = (0, 0, 0);
        (p3.b.expiry, p3.b.cover_rate, p3.b.premium) = (x3, cr3, pr3);
        p3.mk = Makers { host: maker_of(&host.op), ..Makers::default() };
        let args = vec![iv(n), iv(1_000_000_000), iv(hs.entry), iv(1), iv(hs.bl), bv(&x(&long_kp())), iv(0), iv(0), iv(0), bv(&[0; 32])];
        let lpay = kpf_protocol::payoff::owed(n, 1_000_000_000 - hs.bl, 0, hs.m.p.scale);
        let fund = key_in(outpoint(0xa2), 500_000_000_000, 0, &long_kp());
        let fop = fund.outpoint;
        let mut outs = vec![out(lpay as u64, key_spk(&hs.long_key), None), out(p3.floor() as u64, pair_spk(&p3), None), change(90_000_000_000)];
        seal(&mut outs, 1, 1, fop);
        build_ext(&[pair_in(&host, "closeHost", args), fund], outs, 400, &[seq_quiet(&host), 0])
    };
    let b = hs.b;
    must("control: closeHost alone with the host's binding", &build(b.expiry, b.cover_rate, b.premium));
    refused("closeHost writing an earlier expiry", &build(-750, b.cover_rate, b.premium), 0);
    refused("closeHost writing a larger cover rate", &build(b.expiry, hs.m.p.price_max, b.premium), 0);
    refused("closeHost writing a lower premium", &build(b.expiry, b.cover_rate, b.premium - 1), 0);
}

/// Review 8, the field table (docs/DESIGN-ORACLE-IMPL.md 26): every price-free entry of `KpfPairO` that writes a pair
/// state takes the binding from the spent state. `transfer` to a signing taker (each binding field changed in the part)
/// and the remainder of a partial `closeDonor` (the premium changed) are run here: each refuses. (closeDonor's P3 is
/// above, closeHost's P3 in `r8_holds_close_host_cannot_shorten_the_shorts_cover`.)
#[test]
fn r8_holds_no_party_entry_rewrites_a_binding() {
    let (lp, _, _) = opened();
    let (at, a) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("list the long", &at);
    let la = live(&at, 0, 400, &a);
    // transfer: the part the entrant gets, with a binding field changed (as transfer_tx, the part written by hand).
    let (t, part) = transfer_tx(&la, L, 1_000_000_000, 900_000_000, &keypair(45), 20);
    must("control: transfer to a signing taker", &t);
    let ek = keypair(45);
    let tr = |part: &OPair| {
        let p = &la.s;
        let pay = kpf_protocol::payoff::owed(p.size, 1_000_000_000 - p.bl, p.long_tip, p.m.p.scale);
        let args = vec![iv(L), iv(p.size), iv(1_000_000_000), iv(900_000_000), bv(&x(&ek)), bv(&[0; 32]), iv(1)];
        let ins = vec![pair_in(&la, "transfer", args), key_in(outpoint(223), 500_000_000_000, 0, &ek)];
        let floor = part.range_of(p.size) + la.value as i64 - p.range_of(p.size);
        let mut outs = vec![out(pay.max(2_000_000) as u64, key_spk(&p.long_key), None), out(floor as u64, pair_spk(part), None), out(100_000_000_000, p2pk_spk(&ek), None)];
        seal(&mut outs, 1, 0, la.op);
        build_ext(&ins, outs, 0, &[20, 0])
    };
    must("control: the hand-written transfer", &tr(&part));
    for (what, f) in [("expiry", 0usize), ("cover rate", 1), ("premium", 2), ("slot", 3)] {
        let mut p2 = part.clone();
        match f {
            0 => p2.b.expiry -= 1,
            1 => p2.b.cover_rate += 1,
            2 => p2.b.premium -= 1,
            _ => p2.b.uw_id = [0x52; 32],
        }
        refused(&format!("transfer writing another {what} into the part"), &tr(&p2), 0);
    }
    // A partial closeDonor: P3 and the remainder each keep the donor's binding; a remainder with another one is refused.
    let (donor, _) = listed_short(1_150_000_000);
    let d = donor.s.clone();
    let n = d.size / 2;
    let mut p3 = d.rest_of(n);
    p3.short_key = x(&short_kp());
    (p3.short_bid, p3.short_min, p3.short_tip) = (0, 0, 0);
    p3.mk = Makers { donor: maker_of(&donor.op), ..Makers::default() };
    let rest = d.rest_of(d.size - n);
    let partial = |rest: &OPair| {
        let args = vec![iv(n), iv(1_150_000_000), iv(d.entry), iv(1), iv(d.bs), bv(&x(&short_kp())), iv(0), iv(0), iv(0), bv(&[0; 32])];
        let fund = key_in(outpoint(0xa3), 500_000_000_000, 0, &short_kp());
        let fop = fund.outpoint;
        let spay = kpf_protocol::payoff::owed(n, d.bs - 1_150_000_000, d.short_tip, d.m.p.scale);
        let mut outs = vec![
            out(spay.max(2_000_000) as u64, key_spk(&d.short_key), None),
            out(p3.floor() as u64, pair_spk(&p3), None),
            out(rest.floor().max(donor.value as i64 - d.range_of(n)) as u64, pair_spk(rest), bind(0, donor.id.unwrap())),
            out(400_000_000_000, p2pk_spk(&short_kp()), None),
        ];
        seal(&mut outs, 1, 1, fop);
        build_ext(&[pair_in(&donor, "closeDonor", args), fund], outs, 400, &[seq_quiet(&donor), 0])
    };
    must("control: a partial closeDonor, the remainder with the donor's binding", &partial(&rest));
    let mut r2 = rest.clone();
    r2.b.premium += 1;
    refused("a partial closeDonor whose remainder changes the premium", &partial(&r2), 0);
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

/// R8-F2 (was `r8_exploit_two_pools_closed_together_share_one_burn_output`): two pools closed in one transaction naming
/// one burn output are refused (output 1 is no pool's mirrored slot: pool 0's is M - 1 = 2, pool 1's is M - 2 = 1 but
/// below the input count 3). Each pool needs its own burn at M - 1 - i: with five outputs (the closer's change at 0,
/// 1, 2, the burns at 3 for pool 1 and 4 for pool 0) both are accepted and 2 KAS are burned.
#[test]
fn r8_holds_two_pools_closed_together_burn_twice() {
    let (a, b) = (settled_pool(0xb1, UNIT as u64), settled_pool(0xb2, UNIT as u64));
    let burn = || out(UNIT as u64, kpf_protocol::lock::burn_spk(), None);
    let ins = vec![stake_in(&a, "close", vec![b8(1)]), stake_in(&b, "close", vec![b8(1)]), exec_in(0xb3)];
    let outs = vec![change(2 * DISPUTE_BASE + UNIT as u64 + 90_000_000_000), burn()];
    let t = build_ext(&ins, outs, 0, &[0, 0, 0]);
    refused("pool 0 against the shared burn output", &t, 0);
    refused("pool 1 against the shared burn output", &t, 1);
    // Both naming their mirrored slots but one burn output short: the pool whose slot holds change refuses.
    let ins = vec![stake_in(&a, "close", vec![b8(3)]), stake_in(&b, "close", vec![b8(2)]), exec_in(0xb3)];
    let outs = vec![change(2 * DISPUTE_BASE + 90_000_000_000), change(2_000_000), change(2_000_000), burn()];
    let t = build_ext(&ins, outs, 0, &[0, 0, 0]);
    refused("pool 1's mirrored slot 2 is below the input count", &t, 1);
    // Each pool's burn at its own mirrored slot.
    let ins = vec![stake_in(&a, "close", vec![b8(4)]), stake_in(&b, "close", vec![b8(3)]), exec_in(0xb3)];
    let outs = vec![change(2 * DISPUTE_BASE + 90_000_000_000), change(2_000_000), change(2_000_000), burn(), burn()];
    let t = build_ext(&ins, outs, 0, &[0, 0, 0]);
    must("two pools, each burn at its own M - 1 - i", &t);
    let burned: u64 = t.0.outputs.iter().filter(|o| o.script_public_key == kpf_protocol::lock::burn_spk()).map(|o| o.value).sum();
    assert_eq!(burned, 2 * UNIT as u64, "both pools' burns are destroyed");
    // A pool naming an argument other than its mirrored slot (even a burn output of the right value) is refused.
    let ins = vec![stake_in(&a, "close", vec![b8(3)]), stake_in(&b, "close", vec![b8(3)]), exec_in(0xb3)];
    let t = build_ext(&ins, t.0.outputs.clone(), 0, &[0, 0, 0]);
    refused("pool 0 naming pool 1's slot", &t, 0);
}
