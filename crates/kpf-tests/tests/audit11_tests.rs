//! Review 11 (docs/audit/REVIEW-11.md): the oracle-market family at commit `b3675fb` (the fix of review 10). Every
//! transaction here runs every input in the real script engine with the covenant context (`common::run`), except the
//! honest life and the live-run replay (`r11_holds_the_honest_life_...`), which build every transaction with the tool's
//! own builders on the pinned artifacts and check it with `verify::verify`.
//!
//! `r11_exploit_` tests an attack that works, `r11_holds_` one that is refused (or a shape that keeps its rule),
//! `r11_bounded_` a residual that is measured. Worked numbers use a gold-like pair on the testnet defaults
//! (`OMarket::testnet`: scale 10^8, minimum size 0.01 unit, term 28 days, step 3.5 days, tail 3.25 days, idle 7 days,
//! W 6 hours, late 10 minutes, quiet 60 s, mmr 5 %, g 0.5 %, bond 10 % of the cover, rate limit 60 s, at 10 DAA/s):
//! 0.1 oz at 80,000 KAS an ounce (8,000 KAS notional), 10x on each side (bounds 72,000 and 88,000, 800 KAS of margin
//! each), cover rate 20,000 KAS an ounce (2,000 KAS of cover, a 200 KAS bond), premium 10 % of the cover a year.

mod common;
mod oraclekit;

use common::*;
use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::bond::{BindArgs, Bond, Claim, Ctx};
use kpf_protocol::oracle::*;
use oraclekit::*;
use secp256k1::Keypair;

const KAS: i64 = 100_000_000;
const DUST: i64 = 2_000_000;

fn uop(tag: u8, n: u32) -> TransactionOutpoint {
    let mut b = [tag; 32];
    b[..4].copy_from_slice(&n.to_le_bytes());
    TransactionOutpoint { transaction_id: TransactionId::from_bytes(b), index: n }
}

/// The testnet defaults with `lateDaa` set (6,000 is the default: 10 minutes; 100 is the setting asked about: 10 s).
fn gold_market(late: i64) -> OMarket {
    let mut m = OMarket::testnet(feed_hash_of(b"review 11: a gold-like underlying priced in KAS"));
    m.p.late_daa = late;
    assert!(m.check().is_ok());
    m
}

/// 80,000 KAS an ounce.
const E: i64 = 8_000_000_000_000;
const SLOT_TAG: u8 = 0x5c;

/// The premium per whole unit per term at 10 % of the cover a year (365 days at 10 DAA/s).
fn premium_10pct(m: &OMarket, cover_rate: i64) -> i64 {
    (cover_rate as i128 / 10 * m.bond.term as i128 / 315_360_000) as i64
}

/// The gold-like pair: 0.1 oz at 80,000, bounds 72,000 and 88,000, cover rate 20,000 an ounce, expiry `x`.
fn gold_pair(m: &OMarket, x_: i64, premium: i64) -> OPair {
    OPair {
        m: *m,
        size: 10_000_000,
        bl: 7_200_000_000_000,
        bs: 8_800_000_000_000,
        entry: E,
        long_key: x(&long_kp()),
        short_key: x(&short_kp()),
        b: OBinding { uw_id: [SLOT_TAG; 32], uw_key: x(&uw_kp()), cover_rate: 2_000_000_000_000, expiry: x_, premium },
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers::default(),
    }
}

fn gold_slot(m: &OMarket, value: u64) -> LSlot {
    Live { op: uop(SLOT_TAG, 0), value, daa: 0, id: Some(Hash::from_bytes([SLOT_TAG; 32])), s: Bond::new(x(&uw_kp()), m.bond_market(), 0) }
}

fn slot_ctx(sl: &LSlot, lock: u64) -> Ctx {
    Ctx { lock: lock as i64, seq: sl.s.m.p.rate, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true }
}

fn advance_to(sl: &Bond, x2: i64) -> i64 {
    (x2 / sl.m.p.step - 12 - sl.j0).max(0)
}

/// A renewal in any market (the slot's own rate limit as its sequence): in 0 the pair, 1 the slot, 2 the executor; out 0
/// the renewed pair, 1 the slot, 2 the premium to the underwriter, 3 change.
fn renew_at(lp: &LPair, sl: &LSlot, lock: u64, advance: i64, seq: u64) -> (T, OPair, Bond) {
    let (r, pay) = lp.s.renewed().unwrap_or((lp.s.clone(), 0));
    let cover = lp.s.cover_of(lp.s.size);
    let a = BindArgs::new(lock as i64, lp.s.b.expiry + lp.s.m.bond.term, advance, &[Claim::input(0, cover)]);
    let b2 = sl.s.bind(&slot_ctx(sl, lock), &a, sl.value as i64).unwrap_or_else(|_| sl.s.clone());
    let ins = vec![pair_in(lp, "renew", vec![iv(1), iv(0)]), slot_in(sl, "bind", bind_args(&a, &uw_kp())), exec_in(219)];
    let outs = vec![
        out(lp.value - pay as u64, pair_spk(&r), bind(0, lp.id.unwrap())),
        out(sl.value, bond_spk(&b2), bind(1, sl.id.unwrap())),
        out((pay as u64).max(DUST as u64), key_spk(&sl.s.owner), None),
        change(90_000_000_000),
    ];
    (build_ext(&ins, outs, lock, &[0, seq, 0]), r, b2)
}

fn kas(v: i64) -> String {
    format!("{:.4}", v as f64 / KAS as f64)
}

// ---------------------------------------------------------------- lead 1: the look-back option

/// The look-back a closer and its underwriter hold: the expected and the 95 % largest move of the best price over a
/// window of `secs` seconds for a yearly volatility `sigma` (a driftless Brownian motion: E[max] = sqrt(2/pi) s, P(max >
/// a) = 2 (1 - Phi(a / s)), so the 95 % point is 1.96 s, with s = sigma sqrt(secs / year)).
fn lookback(sigma: f64, secs: f64) -> (f64, f64) {
    let s = sigma * (secs / 31_557_600.0).sqrt();
    ((2.0 / std::f64::consts::PI).sqrt() * s, 1.96 * s)
}

/// Lead 1 (I-party's "a close by the other party at the vouched price"). The contract proves only `t0 <= lock time` at
/// the proposal and `P - t0 <= lateDaa` at the finalize (P exact); nothing else ties `t0` to the present. A judge refutes
/// only a price further than g from the feed at t0. So a closer and the pair's underwriter choose the moment in the
/// last `lateDaa` with the best price and vouch it moved by g in the closer's favour; the settlement moves it back by g:
/// the closer is paid at max(feed over [P - lateDaa, P]) (1 - g^2). On the gold-like pair (the feed at P 80,000) with
/// the window's best price 80,560 (the 95 % look-back over 10 minutes at 80 % a year, the volatility of gold priced in
/// KAS): a vouched 80,962.80 is within g of 80,560 (not refutable), and the long closer is paid 855.80 KAS where a close
/// at the present feed pays 800 and the documented close (at the feed moved by g against the closer) 760. The short,
/// which signed nothing, loses 55.80 KAS against the fair price (7.0 % of its margin) and 95.80 KAS against the close the
/// documentation describes. With `t0` one DAA earlier the claim is void (its bond goes to the short). With `lateDaa` 100
/// (10 s) the window's 95 % best price is 80,070 and the take falls to 6.6 KAS against the fair price; the g of the
/// documented close (40 KAS) is still taken in full.
#[test]
fn r11_bounded_a_close_is_priced_at_the_best_moment_of_the_last_late_daa() {
    for (late, best) in [(6_000i64, 8_056_000_000_000i64), (100, 8_007_000_000_000)] {
        let m = gold_market(late);
        let (step, term, w_) = (m.bond.step, m.bond.term, m.p.challenge_daa);
        let pland: i64 = 30_000_000;
        let x_ = ((pland + w_ + step) / step + 1) * step;
        assert!(x_ <= pland + term + step);
        let p = gold_pair(&m, x_, 0);
        let lp = genesis_pair(p.clone(), p.floor() as u64, 0x60, (pland - 1_000_000) as u64);
        let g = m.p.g_bps;
        let vouched = (best as i128 * (10_000 + g as i128) / 10_000) as i64;
        assert!((vouched as i128 * 10_000) <= best as i128 * (10_000 + g as i128), "within g of the best price: not refutable");
        // At t0 = P - lateDaa the close is valid and pays the long at the vouched price moved back by g.
        let lw = proposed(&lp, &ClaimSpec { kind: CLOSE, side: L, n: p.size, p: vouched, t0: pland - late, lock: pland as u64 }, pland as u64);
        assert!(lw.s.valid_at(pland));
        let (ft, s) = finalize_tx(&lw, pland, w_ as u64);
        must("the close priced lateDaa back", &ft);
        let Settlement::Close { long, short } = s else { panic!("{s:?}") };
        let long_net = long - lw.s.refund;
        let fair = q(p.size, E - p.bl, m.p.scale);
        let documented = q(p.size, E * (10_000 - g) / 10_000 - p.bl, m.p.scale);
        println!(
            "lateDaa {late}: the long closer is paid {} KAS (fair at the feed {} KAS, documented close {} KAS); the short gets {} KAS: {} KAS below fair ({:.1} % of its 800 KAS margin), {} KAS below the documented close",
            kas(long_net),
            kas(fair),
            kas(documented),
            kas(short),
            kas(long_net - fair),
            (long_net - fair) as f64 / (800 * KAS) as f64 * 100.0,
            kas(long_net - documented)
        );
        assert!(long_net > fair && long_net > documented);
        // One DAA further back: void, the closer's bond to the short.
        let lw2 = proposed(&lp, &ClaimSpec { kind: CLOSE, side: L, n: p.size, p: vouched, t0: pland - late - 1, lock: pland as u64 }, pland as u64);
        assert!(!lw2.s.valid_at(pland));
        let (ft, s) = finalize_tx(&lw2, pland, w_ as u64);
        must("the same close one DAA older", &ft);
        let Settlement::Restore { defender, .. } = s else { panic!("{s:?}") };
        assert_eq!(defender, lw2.s.bond);
    }
    for (sigma, name) in [(0.15, "15 % a year (gold in a stable unit)"), (0.80, "80 % a year (gold priced in KAS)")] {
        for secs in [600.0, 10.0] {
            let (e, p95) = lookback(sigma, secs);
            println!("look-back at {name} over {secs} s: expected {:.4} %, 95 % {:.4} % of the price; at 10x {:.3} % / {:.3} % of a side's margin", e * 100.0, p95 * 100.0, e * 1000.0, p95 * 1000.0);
        }
    }
}

/// Lead 1, the liquidation side: the underwriter vouches the lowest moment of the last `lateDaa`. The long of the gold
/// pair stands at 76,000 (healthy: its trigger with g is about 75,412) and dipped to 75,000 inside the window; a claim at
/// 75,000 with `t0` in the dip is valid and, uncured, forfeits the long's whole 400 KAS (its value at the present feed)
/// to the short. The cure is offered here (cure bound 70,893.75, inside the long's share) and costs the long a top-up of
/// 110.6 KAS within W (6 hours) and no value; where the cure is not offered (a bound beyond the loser's share, 27.2) the
/// dip costs the whole residual. A claim on the present price (76,000) is refused (no trigger within g).
#[test]
fn r11_bounded_a_liquidation_may_price_the_lowest_moment_of_the_last_late_daa() {
    let m = gold_market(6_000);
    let (step, w_) = (m.bond.step, m.p.challenge_daa);
    let pland: i64 = 30_000_000;
    let x_ = ((pland + w_ + step) / step + 1) * step;
    let p = gold_pair(&m, x_, 0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x61, (pland - 1_000_000) as u64);
    let now_price = 7_600_000_000_000i64;
    let dip = 7_500_000_000_000i64;
    assert!(!p.long_liquidatable(now_price), "healthy at the present feed");
    let k = kit();
    assert!(p.pending_of(k, p.size, LIQ, L, now_price, pland).is_none(), "no claim at the present price");
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: p.size, p: dip, t0: pland - 6_000, lock: pland as u64 }, pland as u64);
    assert!(lw.s.valid_at(pland));
    println!("the dip claim's cure bound {} KAS an ounce, the top-up {} KAS", kas(lw.s.cure_bound), kas(q(p.size, p.bl - lw.s.cure_bound, m.p.scale)));
    let (ft, s) = finalize_tx(&lw, pland, w_ as u64);
    must("the uncured dip claim", &ft);
    assert!(matches!(s, Settlement::Forfeit { .. }));
    println!("uncured: the long loses {} KAS, its value at the present feed", kas(q(p.size, now_price - p.bl, m.p.scale)));
}

/// Lead 1 (b), the setting asked about: `lateDaa` = 100 (10 s). An honest, true liquidation (the feed 75,000 at t0) that
/// lands 101 DAA after its t0 (propagation, a busy mempool, or a party paying to fill blocks for 10 s) is void: the part
/// is restored and the claim's whole bond, 200 KAS, goes to the loser it was right about, who keeps its position. The
/// same claim at 100 DAA settles. With the default 6,000 it settles at 101 too. The contract cannot tell an honest
/// delay from a chosen one: lateness is P - t0 alone.
#[test]
fn r11_bounded_a_late_honest_claim_at_late_daa_100_pays_its_bond_to_the_loser() {
    for (late, settles) in [(100i64, false), (6_000, true)] {
        let m = gold_market(late);
        let (step, w_) = (m.bond.step, m.p.challenge_daa);
        let t0: i64 = 30_000_000;
        let x_ = ((t0 + 200 + w_ + step) / step + 1) * step;
        let p = gold_pair(&m, x_, 0);
        let lp = genesis_pair(p.clone(), p.floor() as u64, 0x62, (t0 - 1_000_000) as u64);
        for (land, ok) in [(t0 + 100, true), (t0 + 101, settles)] {
            let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: p.size, p: 7_500_000_000_000, t0, lock: t0 as u64 }, land as u64);
            let (ft, s) = finalize_tx(&lw, land, w_ as u64);
            must("finalize", &ft);
            match (&s, ok) {
                (Settlement::Forfeit { .. }, true) => {}
                (Settlement::Restore { defender, .. }, false) => {
                    assert_eq!(*defender, lw.s.bond);
                    println!("lateDaa {late}: the true claim landing {} DAA after t0 is void; the loser keeps its side and gets the bond, {} KAS", land - t0, kas(*defender));
                }
                _ => panic!("lateDaa {late}, landing {}: {s:?}", land - t0),
            }
        }
    }
}

// ---------------------------------------------------------------- lead 2: I-exit against an underwriter that renews

/// Lead 2 (28.6 item 1), measured on the gold pair with the testnet defaults and a premium of 10 % of the cover a year
/// (153.42 KAS an ounce a term, 15.34 KAS a term for the pair): an underwriter that renews on time and vouches nothing
/// renews the pair 52 times (52 terms of 28 days, almost four years) before the premiums have narrowed the range to the
/// maintenance bands; meanwhile each side pays 398.9 KAS of its 800 KAS margin in premiums, and the lapse settle that
/// finally comes pays each side at the entry. No party entry can stop a renewal (r10_holds_a_party_respending_...), no
/// party can write a "do not renew" state (an amend with a negative minimum, the only free field, is refused), so a party
/// whose counterparty and taker are absent and whose underwriter refuses to vouch its close is held for the whole time.
/// At 2 % a year the count is 260 terms (19.9 years); at a premium of 0, for ever.
#[test]
fn r11_bounded_an_underwriter_renewing_on_time_holds_a_party_for_52_terms() {
    let m = gold_market(6_000);
    let (step, term) = (m.bond.step, m.bond.term);
    let prem = premium_10pct(&m, 2_000_000_000_000);
    let x0 = 10 * step;
    let p = gold_pair(&m, x0, prem);
    let mut lp = genesis_pair(p.clone(), p.floor() as u64, 0x63, (x0 - term) as u64);
    let mut sl = gold_slot(&m, 10_000 * KAS as u64);
    let (mut n, mut paid_l, mut paid_s) = (0i64, 0i64, 0i64);
    while lp.s.renewed().is_some() {
        let lock = (lp.s.b.expiry - step) as u64;
        let (t, r2, b2) = renew_at(&lp, &sl, lock, advance_to(&sl.s, lp.s.b.expiry + term), m.bond.rate as u64);
        must(&format!("renewal {}", n + 1), &t);
        let (l0, s0) = lp.s.lapse_out(lp.value);
        let (l1, s1) = r2.lapse_out(t.0.outputs[0].value);
        paid_l += l0 - l1;
        paid_s += s0 - s1;
        lp = live(&t, 0, lock, &r2);
        sl = live(&t, 1, lock, &b2);
        n += 1;
    }
    let lock = (lp.s.b.expiry - step) as u64;
    refused("the renewal after the bands are reached", &renew_at(&lp, &sl, lock, advance_to(&sl.s, lp.s.b.expiry + term), m.bond.rate as u64).0, 0);
    println!(
        "premium {} KAS an ounce a term: {n} renewals ({:.2} years); the long paid {} KAS, the short {} KAS of their 800 KAS margins; the lapse settle comes at DAA {} ({:.2} years after the first expiry)",
        kas(prem),
        n as f64 * 28.0 / 365.0,
        kas(paid_l),
        kas(paid_s),
        lp.s.lapse_from(),
        (lp.s.lapse_from() as f64 - x0 as f64) / 315_360_000.0
    );
    assert_eq!(n, 52);
    // No opt-out: the listing window is the only field a party writes alone, and a negative minimum is refused.
    let mut c = OPair { mk: Makers::default(), ..lp.s.clone() };
    c.long_min = -1;
    let ins = vec![pair_in(&lp, "amend", vec![Arg::Sig(long_kp()), iv(L), iv(0), iv(-1), iv(0)]), exec_in(222)];
    let outs = vec![out(lp.value, pair_spk(&c), bind(0, lp.id.unwrap())), change(90_000_000_000)];
    refused("a 'do not renew' flag written by the long", &build_ext(&ins, outs, 0, &[m.p.quiet_daa as u64, 0]), 0);
    // The model's count at 2 % a year.
    let mut q2 = OPair { b: OBinding { premium: prem / 5, ..p.b }, ..p.clone() };
    let mut n2 = 0;
    while let Some((r, _)) = q2.renewed() {
        q2 = r;
        n2 += 1;
    }
    println!("at 2 % a year: {n2} renewals ({:.1} years)", n2 as f64 * 28.0 / 365.0);
}

// ---------------------------------------------------------------- lead 5: the slot's rate limit against `advance`

/// Lead 5, `KpfBond.advance` (anyone, no signature, no age) against the owner's rate limit (`bind` needs the slot UTXO
/// `rateDaa` old, CSV). An outsider advances the slot once per step, at each bucket end, which restarts the slot UTXO's
/// age; consensus accepts the owner's bind only from `slot DAA + rateDaa`. The market bounds accept any `rateDaa` below
/// 2^32 and the wallet does not warn: on a market with `rateDaa >= stepDaa` (the fixture with rate 150, step 100) the
/// outsider's next advance always lands no later than the owner's first usable DAA, so the owner never binds again: no
/// renewal of any pair of the slot, no open, no take. Every pair of the slot then lapses at its expiry and settles at its
/// entry: a party that loses against the entry escapes its loss at the first expiry, the winner loses its gain. The
/// script half is run here (each advance accepted with no signature; the bind with a sequence below the rate refused; the
/// bind with the rate as its sequence accepted by the script, which consensus admits only from slot DAA + rate). On the
/// testnet defaults (rate 600, step 3,024,000) the owner keeps all but 60 s of every step.
#[test]
fn r11_exploit_on_a_market_with_rate_at_least_step_anyone_stops_every_renewal_by_advancing_the_slot() {
    let m = omarket_with(kpf_protocol::bond::BondParams { rho_bps: 10_000, step: 100, term: 800, tail: 200, rate: 150, unbond: 50 });
    assert!(m.check().is_ok(), "the market bounds accept rate 150 with step 100");
    assert!(market_warnings(&m, 10).iter().all(|w| !w.contains("rate")), "and the wallet does not warn");
    let (step, tail, rate, term) = (m.bond.step, m.bond.tail, m.bond.rate, m.bond.term);
    let (lp0, _, _) = opened();
    let p = OPair { m, mk: Makers::default(), b: OBinding { uw_id: [SLOT_TAG; 32], ..lp0.s.b }, ..lp0.s.clone() };
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x64, 300);
    let mut sl = Live { op: uop(SLOT_TAG, 1), value: K, daa: 0, id: Some(Hash::from_bytes([SLOT_TAG; 32])), s: Bond::new(x(&uw_kp()), m.bond_market(), 0) };
    let x_ = p.b.expiry;
    let mut blocked = 0;
    let mut at = 0i64;
    while at <= x_ {
        // The outsider's advance at the next bucket end (by the calendar alone, no key).
        let d = 1i64;
        at = (sl.s.j0 + d) * step + tail;
        let ctx = Ctx { lock: at, seq: 0, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: false, has_id: true };
        let b2 = sl.s.advance(&ctx, d, sl.value as i64).expect("the model's advance");
        let ins = vec![slot_in(&sl, "advance", vec![b8(d)]), key_in(uop(0x65, at as u32), 10 * KAS as u64, 0, &keypair(88))];
        let t = build_ext(&ins, vec![out(sl.value, bond_spk(&b2), bind(0, sl.id.unwrap())), out(9 * KAS as u64, p2pk_spk(&keypair(88)), None)], at as u64, &[0, 0]);
        must(&format!("an outsider's advance at {at}"), &t);
        sl = live(&t, 0, at as u64, &b2);
        // The owner's renewal in the window [X - step, X]: usable only from the slot's DAA + rate, after the next advance.
        let next = (sl.s.j0 + 1) * step + tail;
        if at >= x_ - step - step {
            let lock = (at.max(x_ - step)) as u64;
            refused("a bind whose sequence is below the rate", &renew_at(&lp, &sl, lock, advance_to(&sl.s, x_ + term), (rate - 1) as u64).0, 1);
            let ok = renew_at(&lp, &sl, lock, advance_to(&sl.s, x_ + term), rate as u64).0;
            assert!(verdicts(&ok)[1], "the script accepts the bind with the rate as its sequence");
            assert!(sl.daa as i64 + rate >= next, "consensus admits it from {} but the next advance lands at {next}", sl.daa as i64 + rate);
            blocked += 1;
        }
    }
    assert!(blocked >= 2);
    println!("rate {rate} >= step {step}: every renewal of the pair (X {x_}) is beaten by the next advance; the pair lapses and settles at its entry from {}", lp.s.lapse_from());
    must("the lapse settle at the entry", &lapse_tx(&lp, lp.s.lapse_from(), 0));
}

// ---------------------------------------------------------------- lead 3 / lead 4: the payout floor per lineage

/// A partial transfer of `n` units of the listed short side of `lp` to the signing taker `ek` at price `xp`, keeping the
/// short's bound: in 0 the pair, 1 the taker's funding; out 0 the taker's change, 1 the part (fresh id), 2 the remainder
/// (the pair's id). The leaver's payout is below the payout floor here and stays in the remainder.
fn shred_once(lp: &LPair, n: i64, xp: i64, ek: &Keypair, fop: TransactionOutpoint) -> (T, OPair, OPair) {
    let p = &lp.s;
    let mut part = p.rest_of(n);
    part.short_key = x(ek);
    (part.short_bid, part.short_min, part.short_tip) = (0, 0, 0);
    part.mk.parent = maker_of(&lp.op);
    let rest = p.rest_of(p.size - n);
    let pay = kpf_protocol::payoff::owed(n, p.bs - xp, p.short_tip, p.m.p.scale);
    assert!(pay < DUST);
    let keep = lp.value as i64 - p.range_of(n) + pay;
    let fv = 10 * KAS;
    let pv = part.range_of(n) + p.m.p.carrier;
    let args = vec![iv(S), iv(n), iv(xp), iv(p.bs), bv(&x(ek)), bv(&[0; 32]), iv(1)];
    let ins = vec![pair_in(lp, "transfer", args), key_in(fop, fv as u64, 0, ek)];
    let mut outs = vec![out((fv - pv - 1_000_000) as u64, p2pk_spk(ek), None), out(pv as u64, pair_spk(&part), None), out(keep as u64, pair_spk(&rest), bind(0, lp.id.unwrap()))];
    seal(&mut outs, 1, 0, lp.op);
    (build_ext(&ins, outs, 0, &[p.m.p.quiet_daa as u64, 0]), part, rest)
}

/// Lead 3 and lead 4 (I-party's "the payout floor D at the lapse settle, once per lineage end"). The part floor of review
/// 10 lives in the order only; a pair's hand-overs (`transfer`, `closeHost`, `closeDonor`) split it into parts of the
/// market's minimum size and need only the leaver's listing. A short that lists its side and takes it in small parts with
/// a second key splits the passive long into lineages whose long margin lies below the payout floor, and the lapse
/// settle pays each such long nothing. On a 1-unit pair at 1 KAS a unit, 10x (the long's margin 0.1 KAS), testnet
/// defaults: five takes of 0.19 unit and the remainder of 0.05 unit leave the long 0.019 KAS and 0.005 KAS of lapse
/// value in each, all below 0.02 KAS: it loses its whole margin, which the attacker's short keys collect. I-party
/// allows it (six lineage ends, six floors). The attacker pays six transfer fees (about 0.03 KAS each), more than it takes:
/// griefing at a loss, possible only where a minimum-size part holds less than 0.02 KAS of a side's margin.
#[test]
fn r11_bounded_hand_overs_split_a_passive_long_into_parts_below_the_payout_floor() {
    let m = gold_market(6_000);
    let sc = m.p.scale;
    let p0 = OPair {
        m,
        size: sc,
        bl: 90_000_000,
        bs: 110_000_000,
        entry: 100_000_000,
        long_key: x(&long_kp()),
        short_key: x(&short_kp()),
        b: OBinding { uw_id: [SLOT_TAG; 32], uw_key: x(&uw_kp()), cover_rate: 20_000_000, expiry: 10 * m.bond.step, premium: 0 },
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 100_000_000,
        short_min: m.p.min_size,
        short_tip: 0,
        mk: Makers::default(),
    };
    let mut lp = genesis_pair(p0.clone(), p0.floor() as u64, 0x66, 1_000);
    let (l_before, _) = p0.lapse_out(lp.value);
    assert_eq!(l_before, 10_000_000, "the long's 0.1 KAS is paid at the lapse settle of the whole pair");
    let ek = keypair(77);
    let mut parts = vec![];
    let mut daa = 1_000u64;
    for i in 0..5 {
        let (t, part, rest) = shred_once(&lp, 19_000_000, 100_000_000, &ek, uop(0x67, i));
        must(&format!("take {i} of 0.19 unit of the short side"), &t);
        daa += m.p.quiet_daa as u64;
        parts.push(live(&t, 1, daa, &part));
        lp = live(&t, 2, daa, &rest);
    }
    parts.push(lp);
    let mut long_back = 0;
    for pp in &parts {
        let (l, _) = pp.s.lapse_out(pp.value);
        assert_eq!(l, 0, "each piece pays the long nothing (below 0.02 KAS)");
        must("its lapse settle", &lapse_tx(pp, pp.s.lapse_from(), 0));
        long_back += l;
    }
    println!("the passive long's lapse payout: {} KAS before, {} KAS after six lineages (each below the payout floor)", kas(l_before), kas(long_back));
}

/// Lead 3: the carrier an order pays with the part that empties it, when that part is a take of a whole pair. The pair's
/// own carrier moves into the part with `keep`, so the part needs no carrier from the order, and the order's carrier
/// (with its tip) is left to whoever builds the transaction: the executor keeps it, where 28.3 says the carrier comes back
/// to the short at the pair's end. Bounded by I-order (one carrier per order); the tool has no take builder.
#[test]
fn r11_bounded_the_carrier_of_an_emptying_take_of_a_whole_pair_is_the_executors() {
    let (lp, sl, _) = opened();
    let (at, a) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("the long lists", &at);
    let ll = live(&at, 0, 400, &a);
    let o = OOpen { size_left: ll.s.size, min_fill: ll.s.size, ..sample_order(&omarket(), L, x(&keypair(45))) };
    let lo = Live { op: uop(0x68, 0), value: 2_000_000_000_000u64, daa: 400, id: Some(Hash::from_bytes([0x68; 32])), s: o.clone() };
    let (t, part) = take_tx(&ll, &lo, &sl, 1_000_000_000, 420, true);
    must("the take that empties the order", &t);
    let keep = ll.value as i64 - ll.s.range_of(ll.s.size);
    assert_eq!(t.0.outputs[2].value as i64, part.range_of(part.size) + keep, "the part holds the pair's carrier (in keep), none of the order's");
    let paid = lo.value as i64 - t.0.outputs[1].value as i64;
    assert_eq!(paid, q(o.size_left, o.my_rate + o.tip, 1_000_000) + o.m.p.carrier, "the order paid a carrier");
    let need = part.range_of(part.size) + keep + t.0.outputs[0].value as i64 + t.0.outputs[3].value as i64 - ll.value as i64 - sl.value as i64;
    println!("the order paid {} KAS; the contract outputs needed {} KAS of it; the builder keeps {} KAS (its carrier)", kas(paid), kas(need), kas(paid - need));
    assert_eq!(paid - need, o.m.p.carrier);
}

/// Lead 1 / lead 5 (the vouch, 16 "The vouched claim"). The evidence of a claim is any P2PK input of `uwKey` signed
/// SIGHASH_ALL in the same transaction; its value may go anywhere. Every premium, every bond repaid and every keeper fee
/// lands on `uwKey` as a plain P2PK output, so every later spend of one of them is a vouch for every `propose` input the
/// transaction carries. A transaction in which the underwriter's input pays a merchant 1 KAS, built by that merchant
/// (a signing request of a web wallet), and that also spends the fixture pair by `propose` with a false price, is
/// accepted: the underwriter signed "the whole transaction", not "this price". The bond is funded by the builder and
/// comes back to the underwriter if nobody contests within W; the long loses its whole side (1,000 KAS). Contested in
/// time, the builder loses the bond. Nothing in a vouch says it is one.
#[test]
fn r11_bounded_any_sighash_all_spend_of_an_underwriter_utxo_vouches_the_claims_beside_it() {
    let (lp, _, _) = opened();
    let merchant = keypair(77);
    let c = ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p: 500_000_000, t0: 400, lock: 400 };
    let (mut t, w) = propose_tx(&lp, &c, &uw_kp(), None);
    // The underwriter's input (a premium it received) pays the merchant; the merchant funds the bond and the fee.
    t.0.outputs.insert(1, out(KAS as u64, p2pk_spk(&merchant), None));
    t.0.finalize();
    let (t, _) = {
        // Re-sign: the signatures cover the outputs.
        let ins = vec![
            pair_in(&lp, "propose", vec![iv(c.n), iv(c.kind), iv(c.side), iv(c.p), iv(c.t0), iv(1), bv(&[0u8; 65]), iv(lp.s.pending_of(kit(), c.n, c.kind, c.side, c.p, c.t0).unwrap().1), iv(0), bv(&kit().pending.prefix), bv(&kit().pending.suffix)]),
            key_in(uop(0x69, 0), 2 * KAS as u64, 0, &uw_kp()),
            key_in(uop(0x69, 1), 10_000 * KAS as u64, 0, &merchant),
        ];
        let mut outs = t.0.outputs.clone();
        place(&mut outs, 0, 0, lp.op, lp.id.unwrap(), Lineage::Fresh);
        (build_ext(&ins, outs, c.lock, &[0, 0, 0]), ())
    };
    must("a payment by the underwriter that carries a false liquidation", &t);
    let lw = live(&t, 0, 400, &w.made_by(&lp.op));
    let (ft, s) = finalize_tx(&lw, 400, 200);
    must("uncontested, it settles", &ft);
    assert!(matches!(s, Settlement::Forfeit { .. }));
}

// ---------------------------------------------------------------- lead 6: the honest life, with the tool's builders

mod life {
    use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
    use kpf_protocol::bond::{Bond, RING};
    use kpf_protocol::judge::{Js, NO};
    use kpf_protocol::oracle::*;
    use kpf_protocol::oracle_build::*;
    use kpf_protocol::state::{KIND_LIQ, LONG, SHORT};
    use kpf_protocol::tx::{pubkey_of, KeySet, KeyUtxo, MIN_FEE_RATE};

    const KAS: u64 = 100_000_000;

    pub struct Chain {
        n: u32,
        keys: KeySet,
        pub fees: Vec<(String, u64)>,
    }

    impl Chain {
        pub fn new(seeds: &[u8]) -> Chain {
            let mut keys = KeySet::new();
            for s in seeds {
                keys.add_secret(&[*s; 32]).unwrap();
            }
            Chain { n: 0, keys, fees: vec![] }
        }
        pub fn mint(&mut self, key: [u8; 32], sompi: u64) -> KeyUtxo {
            self.n += 1;
            let mut b = [0x7cu8; 32];
            b[..4].copy_from_slice(&self.n.to_le_bytes());
            KeyUtxo { outpoint: TransactionOutpoint { transaction_id: TransactionId::from_bytes(b), index: 0 }, amount: sompi, daa: 0, key }
        }
        pub fn sign(&mut self, name: &str, b: kpf_protocol::error::Result<OBuilt>) -> OSigned {
            let b = b.unwrap_or_else(|e| panic!("{name}: the builder refuses: {e}"));
            let sg = b.sign(&self.keys).unwrap_or_else(|e| panic!("{name}: {e}"));
            let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
            assert!(v.is_ok(), "{name}: {:?}", v.errors());
            self.fees.push((name.into(), sg.fee));
            sg
        }
    }

    pub fn k(s: u8) -> [u8; 32] {
        pubkey_of(&[s; 32]).unwrap()
    }

    fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
        let o = &sg.tx.outputs[i];
        OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
    }

    fn made(sg: &OSigned, i: usize) -> OObj {
        sg.created.iter().find(|(j, _, _)| *j == i).unwrap_or_else(|| panic!("no object at output {i}")).1.clone()
    }
    fn slot_at(sg: &OSigned, i: usize, daa: u64) -> OLive<Bond> {
        let OObj::Slot(b) = made(sg, i) else { panic!("not a slot") };
        nth(sg, i, b, daa)
    }
    fn pair_at(sg: &OSigned, i: usize, daa: u64) -> OLive<OPair> {
        let OObj::Pair(p) = made(sg, i) else { panic!("not a pair") };
        nth(sg, i, p, daa)
    }
    fn order_at(sg: &OSigned, i: usize, daa: u64) -> OLive<OOpen> {
        let OObj::Order(o) = made(sg, i) else { panic!("not an order") };
        nth(sg, i, o, daa)
    }

    /// The life; returns the fees by transaction and what the executor of the fills paid beyond their fees.
    pub fn run() -> (Vec<(String, u64)>, u64) {
        let (lk, sk, uk, stk, t1, t2, ek) = (k(41), k(42), k(31), k(70), k(46), k(47), k(60));
        let mut c = Chain::new(&[41, 42, 31, 70, 46, 47, 60]);
        let r = MIN_FEE_RATE;
        let m = super::gold_market(6_000);
        let jm = m.judge_market();
        let (step, term, tail, idle, w_) = (m.bond.step as u64, m.bond.term as u64, m.bond.tail as u64, m.p.idle_daa as u64, m.p.challenge_daa as u64);
        let prem = super::premium_10pct(&m, 2_000_000_000_000);
        let sc = m.p.scale;
        // Stake, slot, orders.
        let f = c.mint(stk, 201 * KAS);
        let sg = c.sign("the staker bonds 200 KAS", create_stake(&m, stk, 200 * KAS, &[f], &stk, r));
        let stake = nth(&sg, 1, Js::free(stk, jm), 0);
        let f = c.mint(uk, 10_001 * KAS);
        let sg = c.sign("the underwriter bonds a 10,000 KAS slot", create_slot(&m, uk, 10_000 * KAS, 0, &[f], &uk, r));
        let mut slot = nth(&sg, 1, Bond::new(uk, m.bond_market(), 0), 0);
        let base = |side: i64, owner: [u8; 32]| OOpen {
            m,
            side,
            owner,
            size_left: 3 * sc / 10,
            limit: super::E,
            my_rate: 800_000_000_000,
            min_counter_rate: 0,
            min_fill: sc / 10,
            tip: 0,
            active_from: 0,
            expiry_daa: 1_000_000_000,
            uw_accept: slot.idb(),
            prem_max: prem,
        };
        let (lo, so) = (base(LONG, lk), base(SHORT, sk));
        let esc = |o: &OOpen| o.fill_spend(o.size_left, o.prem_max) as u64 + KAS;
        let f = c.mint(lk, esc(&lo) + KAS);
        let sg = c.sign("long order, 0.3 oz at 80,000, 10x", create_order(&lo, esc(&lo), &[f], &lk, r));
        let mut lorder = nth(&sg, 1, lo.clone(), 1_000);
        let f = c.mint(sk, esc(&so) + KAS);
        let sg = c.sign("short order, 0.3 oz at 80,000, 10x", create_order(&so, esc(&so), &[f], &sk, r));
        let mut sorder = nth(&sg, 1, so.clone(), 1_000);
        // Three partial fills of 0.1 oz; the last empties both orders.
        let x_ = 9 * step as i64;
        let mut pairs = vec![];
        let mut carriers = 0u64;
        for i in 0..3u64 {
            let lock = 2_000_000 + 700 * i;
            let terms = OpenTerms { n: sc / 10, x: super::E, cover_rate: 2_000_000_000_000, premium: prem, expiry: x_, lock };
            let f = c.mint(ek, 10 * KAS);
            let fin = f.amount;
            let (b, _) = open(&lorder, &sorder, &slot, &terms, &[f], &ek, r).unwrap_or_else(|e| panic!("fill {i}: {e}"));
            let sg = c.sign(&format!("open, fill {} of 3 (0.1 oz)", i + 1), Ok(b));
            let back: u64 = sg.tx.outputs.iter().filter(|o| o.script_public_key == kpf_protocol::tx::p2pk_spk(&ek)).map(|o| o.value).sum();
            carriers += fin - back - sg.fee;
            pairs.push(pair_at(&sg, 2, lock));
            slot = slot_at(&sg, 3, lock);
            if i < 2 {
                lorder = order_at(&sg, 0, lock);
                sorder = order_at(&sg, 1, lock);
            }
        }
        let (a, b_, cpair) = (pairs[0].clone(), pairs[1].clone(), pairs[2].clone());
        assert_eq!(a.s.bs - a.s.bl, 1_600_000_000_000);
        // Pair A: each side handed over to a signing taker at 80,000.
        let f = c.mint(ek, 10 * KAS);
        let (bb, p) = amend(&a, LONG, super::E, 0, 0, &[f], &ek, r).unwrap();
        let sg = c.sign("A: the long lists at 80,000", Ok(bb));
        let a = nth(&sg, 0, p, 3_000_000);
        let f = c.mint(t1, 1_000 * KAS);
        let (bb, p) = transfer(&a, LONG, super::E, a.s.bl, t1, &[f], &t1, r).unwrap();
        let sg = c.sign("A: T1 takes the long", Ok(bb));
        let a = nth(&sg, 1, p, 3_001_000);
        let f = c.mint(ek, 10 * KAS);
        let (bb, p) = amend(&a, SHORT, super::E, 0, 0, &[f], &ek, r).unwrap();
        let sg = c.sign("A: the short bids 80,000", Ok(bb));
        let a = nth(&sg, 0, p, 3_002_000);
        let f = c.mint(t2, 1_000 * KAS);
        let (bb, p) = transfer(&a, SHORT, super::E, a.s.bs, t2, &[f], &t2, r).unwrap();
        let sg = c.sign("A: T2 takes the short", Ok(bb));
        let a = nth(&sg, 1, p, 3_003_000);
        // Pair B: a true liquidation of the long at 75,000, cured.
        let liq = |t0: i64| ClaimTerms { kind: KIND_LIQ, side: LONG, n: sc / 10, price: 7_500_000_000_000, t0 };
        let vouch = c.mint(uk, 300 * KAS);
        let f = c.mint(ek, 10 * KAS);
        let (bb, w) = propose(&b_, &liq(10_000_000), &vouch, &[f], &ek, r).unwrap();
        let sg = c.sign("B: propose the liquidation of the long at 75,000", Ok(bb));
        let pend = nth(&sg, 0, w, 10_000_000);
        let f = c.mint(lk, 200 * KAS);
        let (bb, cured) = cure(&pend, &[f], &lk, r).unwrap();
        let sg = c.sign("B: the long's cure", Ok(bb));
        let b_ = nth(&sg, 1, cured, 10_000_100);
        // Pair C: a true liquidation of the long, forfeited after W.
        let vouch = c.mint(uk, 300 * KAS);
        let f = c.mint(ek, 10 * KAS);
        let (bb, w) = propose(&cpair, &liq(10_001_000), &vouch, &[f], &ek, r).unwrap();
        let sg = c.sign("C: propose the liquidation of the long at 75,000", Ok(bb));
        let pend = nth(&sg, 0, w, 10_001_000);
        let f = c.mint(ek, 10 * KAS);
        let (bb, s) = finalize(&pend, &[f], &ek, r).unwrap();
        let sg = c.sign("C: finalize, forfeit", Ok(bb));
        let Settlement::Forfeit { part, .. } = s else { panic!("{s:?}") };
        let cpair = nth(&sg, 1, part, 10_001_000 + w_);
        // Pair A renewed on time.
        let lock = x_ as u64 - step;
        let f = c.mint(ek, 10 * KAS);
        let (bb, rs) = renew(&[a.clone()], &slot, lock, &[f], &ek, r).unwrap();
        let sg = c.sign("A: the on-time renewal (executor-funded)", Ok(bb));
        let a = nth(&sg, 0, rs[0].clone(), lock);
        slot = slot_at(&sg, 1, lock);
        // Pair A: a false liquidation (the feed 80,000), contested, nobody defends, refuted.
        let vouch = c.mint(uk, 300 * KAS);
        let f = c.mint(ek, 10 * KAS);
        let (bb, w) = propose(&a, &liq(30_000_000), &vouch, &[f], &ek, r).unwrap();
        let sg = c.sign("A: propose a false liquidation at 75,000", Ok(bb));
        let pend = nth(&sg, 0, w, 30_000_000);
        let f = c.mint(ek, 10 * KAS);
        let (bb, cw, d) = contest(&pend, &stake, 50_000_000, &[f], &ek, r, &Trust::Untraced).unwrap();
        let sg = c.sign("A: the staker's contest", Ok(bb));
        let (cpart, dispute) = (nth(&sg, 0, cw, 30_000_100), nth(&sg, 2, d, 30_000_100));
        let receipt = nth(&sg, 1, Js::receipt(stk, jm, NO, stake.value as i64, 0, 30_000_000), 30_000_100);
        let f = c.mint(ek, 10 * KAS);
        let (bb, dw, pool) = decide(&dispute, &cpart, &[f], &ek, r).unwrap();
        assert_eq!(dw.status, REFUTED);
        let sg = c.sign("A: resolve, refuted", Ok(bb));
        let (decided, pool) = (nth(&sg, 1, dw, 30_432_200), nth(&sg, 0, pool, 30_432_200));
        let f = c.mint(ek, 10 * KAS);
        let (bb, s) = finalize(&decided, &[f], &ek, r).unwrap();
        let sg = c.sign("A: finalize, restored (the bond to T1 and the staker)", Ok(bb));
        let Settlement::Restore { part, defender, disputer, .. } = s else { panic!("{s:?}") };
        assert_eq!((defender, disputer), (160 * KAS as i64, 40 * KAS as i64));
        let a = nth(&sg, 1, part, 30_432_300);
        let f = c.mint(ek, 10 * KAS);
        let (bb, p2) = pool_settle(&pool, &[receipt], &[f], &ek, r).unwrap();
        let sg = c.sign("A: settle the pool", Ok(bb));
        let won = nth(&sg, 1, Js::free(stk, jm), 30_432_400);
        let _ = c.sign("A: close the pool", pool_close(&nth(&sg, 0, p2, 30_432_400), &ek, r));
        // The lapse settles: B (cured, not renewed) and C (forfeited) after X, A after X + term.
        for (name, p) in [("B: lapse settle at the entry", &b_), ("C: lapse settle of the forfeited part", &cpair), ("A: lapse settle after its renewed expiry", &a)] {
            assert!(p.s.lapse_from() <= p.s.b.expiry as u64 + tail + idle);
            let f = c.mint(ek, 10 * KAS);
            c.sign(name, lapse_settle(p, &[f], &ek, r));
        }
        // The slot's unbond and withdrawal, the stake's.
        let j0 = slot.s.j0;
        let last = (j0 as usize..j0 as usize + RING).rev().find(|&j| slot.s.ring[j - j0 as usize] != 0).map(|j| j as i64 + 1).unwrap_or(j0);
        let free_at = (last * step as i64 + tail as i64) as u64;
        assert_eq!(free_at, x_ as u64 + term + tail, "the slot is free once the renewed cover's tail has passed");
        let f = c.mint(ek, 10 * KAS);
        let (bb, ub) = slot_unbond(&slot, last - j0, free_at, &[f], &ek, r).unwrap();
        let sg = c.sign("unbond the slot", Ok(bb));
        let sg = c.sign("withdraw the slot", slot_withdraw(&nth(&sg, 0, ub, free_at), &uk, r));
        assert!(sg.tx.outputs.iter().any(|o| o.script_public_key == kpf_protocol::tx::p2pk_spk(&uk) && o.value > 9_999 * KAS));
        let f = c.mint(ek, 10 * KAS);
        let (bb, su) = stake_unbond(&won, &[f], &ek, r).unwrap();
        let sg = c.sign("unbond the stake", Ok(bb));
        c.sign("withdraw the stake", stake_withdraw(&nth(&sg, 0, su, 31_000_000), &stk, r));
        let _ = (sk, SHORT);
        (c.fees, carriers)
    }
}

/// Lead 6: the honest life of the gold-like 10x pair on the testnet defaults, every transaction built by the tool's
/// builders on the pinned artifacts, signed and passed through `verify::verify`: a stake, a slot, two orders of 0.3 oz,
/// three partial fills of 0.1 oz (the executor funds the carrier of the two fills that empty neither order), the long and
/// the short of one pair each handed over to a signing taker, a true liquidation cured, another forfeited, an on-time
/// renewal, a false liquidation contested and refuted (the bond to the taker who holds the long and to the staker), the
/// pool settled and closed, three lapse settles, the slot's unbond and withdrawal, the stake's. Every step works; each
/// fee is printed and compared with the documented numbers (28.5 and earlier tables); none is more than twice the
/// documented fee of its shape.
#[test]
fn r11_holds_the_honest_life_of_a_10x_pair_with_the_testnet_defaults() {
    let (fees, carriers) = life::run();
    let doc = [("open", 9_480_000u64), ("propose", 3_850_000), ("cure", 970_000), ("finalize", 950_000), ("renewal", 4_070_000), ("contest", 2_950_000), ("resolve", 2_910_000), ("settle the pool", 5_940_000), ("close the pool", 2_020_000), ("lapse", 2_750_000), ("lists", 2_770_000), ("bids", 2_770_000), ("takes", 3_120_000)];
    for (name, fee) in &fees {
        let d = doc.iter().find(|(k, _)| name.contains(k)).map(|(_, v)| *v);
        println!("{name}: {} KAS{}", kas(*fee as i64), d.map(|v| format!(" (documented {})", kas(v as i64))).unwrap_or_default());
        if let Some(v) = d {
            assert!(*fee <= 2 * v, "{name}: {fee} sompi, more than twice the documented {v}");
        }
    }
    println!("the executor of the fills paid {} KAS beyond the fees (carriers of fills that empty neither order)", kas(carriers as i64));
    // Two carriers, and one sompi a fill: the premium output is q(n, premium), the orders' halves are each rounded down.
    assert!(carriers >= 2 * 20_000_000 && carriers <= 2 * 20_000_000 + 3, "{carriers}");
    println!("{} transactions", fees.len());
}
