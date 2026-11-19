//! Review 11 (docs/audit/REVIEW-11.md): the oracle-market family at commit `b3675fb` (the fix of review 10), and its
//! fixes (docs/DESIGN-ORACLE-IMPL.md 29: every former `r11_exploit_` and most `r11_bounded_` are now `r11_holds_`). Every
//! transaction here runs every input in the real script engine with the covenant context (`common::run`), except the
//! honest life and the live-run replay (`r11_holds_the_honest_life_...`), which build every transaction with the tool's
//! own builders on the pinned artifacts and check it with `verify::verify`.
//!
//! `r11_exploit_` tests an attack that works, `r11_holds_` one that is refused (or a shape that keeps its rule),
//! `r11_bounded_` a residual that is measured. Worked numbers use a gold-like pair on the testnet defaults
//! (`OMarket::testnet`: scale 10^8, minimum size 0.01 unit, term 28 days, step 3.5 days, tail 3.25 days, idle 7 days,
//! W 2 minutes (review 11; 6 hours before), lateDaa as given (100 DAA = 10 s, the default since review 11), quiet 60 s,
//! mmr 5 %, g 0.5 %, bond 10 % of the cover, rate limit 60 s, at 10 DAA/s):
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

// ---------------------------------------------------------------- lead 1: the look-back option (R11-F2)

/// The look-back a closer and its underwriter held: the expected and the 95 % largest move of the best price over a
/// window of `secs` seconds for a yearly volatility `sigma` (a driftless Brownian motion: E[max] = sqrt(2/pi) s, P(max >
/// a) = 2 (1 - Phi(a / s)), so the 95 % point is 1.96 s, with s = sigma sqrt(secs / year)).
fn lookback(sigma: f64, secs: f64) -> (f64, f64) {
    let s = sigma * (secs / 31_557_600.0).sqrt();
    ((2.0 / std::f64::consts::PI).sqrt() * s, 1.96 * s)
}

/// The feed definition's rule for one moment (29.3): a claimed price `p` is true if the feed `f` lies in
/// `[p (1 - g), p (1 + g)]`. A claim is true only if this holds at its t0 and at P.
fn true_at(p: i64, f: i64, g: i64) -> bool {
    let (p, f, g) = (p as i128, f as i128, g as i128);
    f * 10_000 >= p * (10_000 - g) && f * 10_000 <= p * (10_000 + g)
}

/// The highest price true both at a moment with feed `a` and at one with feed `b`: `min(a, b) / (1 - g)`, rounded down.
fn highest_true(a: i64, b: i64, g: i64) -> i64 {
    (a.min(b) as i128 * 10_000 / (10_000 - g as i128)) as i64
}

/// R11-F2, fixed (29.3). The review's look-back: a closer (with its underwriter, before) vouched the best price of the
/// last lateDaa moved by g, true at its t0, and was paid that best moment. Now a close is the party's own (no vouch) and
/// the judge holds every claim to the feed at t0 AND at P. On the gold pair (the feed at P 80,000; the window's 95 %
/// best price 80,560 over 10 minutes at 80 % a year, 80,070 over 10 s): the review's price 80,962.80 is true at t0 and
/// false at P, so a contest refutes it and the closer's bond goes to the short and the disputer. The highest price true
/// at both moments is the lower feed moved up by g; it pays the long closer at most the feed at P (800 KAS at 80,000, its
/// value at the feed): the short, which signed nothing, gets at least its value at the feed. An honest closer quoting
/// the feed pays g (40 KAS here). The contract accepts every one of these claims (lateness voids nothing): only the
/// judge's rule decides, which is what the feed definition now states.
#[test]
fn r11_holds_a_close_is_paid_at_most_the_feed_when_it_lands() {
    for (late, best) in [(6_000i64, 8_056_000_000_000i64), (100, 8_007_000_000_000)] {
        let m = gold_market(late);
        let (step, term, w_) = (m.bond.step, m.bond.term, m.p.challenge_daa);
        let pland: i64 = 30_000_000;
        let x_ = ((pland + w_ + step) / step + 1) * step;
        assert!(x_ <= pland + term + step);
        let p = gold_pair(&m, x_, 0);
        let lp = genesis_pair(p.clone(), p.floor() as u64, 0x60, (pland - 1_000_000) as u64);
        let g = m.p.g_bps;
        let fair = q(p.size, E - p.bl, m.p.scale);
        // The review's price: true at t0 (the window's best), false at P (the feed 80,000).
        let vouched = (best as i128 * (10_000 + g as i128) / 10_000) as i64;
        assert!(true_at(vouched, best, g) && !true_at(vouched, E, g), "the review's look-back price is false at P");
        let (t, w) = self_close_tx(&lp, L, p.size, vouched, pland - late, pland as u64, &long_kp());
        must("the closer's own claim at the look-back price (the contract accepts it; the judge decides)", &t);
        let lw = live(&t, 0, pland as u64, &w);
        // Contested within W and refuted: the part is restored, the bond to the short and the disputer.
        let refuted = lw.s.contested(pland, x(&keypair(70))).decided(false, pland + 1);
        let Settlement::Restore { defender, disputer, proposer, .. } = refuted.settlement(lw.value, pland) else { panic!() };
        assert_eq!((defender + disputer, proposer), (w.bond, 0));
        // The highest price true at both moments pays the long at most its value at the feed at P.
        let pstar = highest_true(best, E, g);
        assert!(true_at(pstar, best, g) && true_at(pstar, E, g) && !true_at(pstar + 1, E, g));
        let (t, w2) = self_close_tx(&lp, L, p.size, pstar, pland - late, pland as u64, &long_kp());
        must("the closer's own claim at the highest price true at both moments", &t);
        let lw2 = live(&t, 0, pland as u64, &w2);
        let (ft, s) = finalize_tx(&lw2, pland, w_ as u64);
        must("finalize, W later", &ft);
        let Settlement::Close { long, short } = s else { panic!("{s:?}") };
        let long_net = long - lw2.s.refund;
        assert!(long_net <= fair, "the closer is paid at most its value at the feed at P");
        assert!(short >= lp.value as i64 - fair, "the short gets at least its value at the feed at P");
        // An honest closer quoting the feed pays g.
        let honest = q(p.size, (E as i128 * (10_000 - g as i128) / 10_000) as i64 - p.bl, m.p.scale);
        println!(
            "lateDaa {late}: the review's price {} is false at P (refuted: {} KAS to the short, {} to the disputer); the highest price true at both moments {} pays the long {} KAS, its value at the feed {} KAS; an honest close at the feed pays it {} KAS (g: {} KAS)",
            kas(vouched),
            kas(defender),
            kas(disputer),
            kas(pstar),
            kas(long_net),
            kas(fair),
            kas(honest),
            kas(fair - honest)
        );
    }
    for (sigma, name) in [(0.15, "15 % a year (gold in a stable unit)"), (0.80, "80 % a year (gold priced in KAS)")] {
        for secs in [600.0, 10.0] {
            let (e, p95) = lookback(sigma, secs);
            println!("look-back the old rule allowed at {name} over {secs} s: expected {:.4} %, 95 % {:.4} % of the price; at 10x {:.3} % / {:.3} % of a side's margin; under the two-moment rule: 0 beyond the feed at P", e * 100.0, p95 * 100.0, e * 1000.0, p95 * 1000.0);
        }
    }
}

/// R11-F2, the liquidation side, fixed. The long of the gold pair stands at 76,000 (healthy: below maintenance only under
/// 72,000 / 0.95 = 75,789) and dipped to 75,000 inside the window. The dip claim at 75,000 is accepted by the contract and
/// is true at t0, but false at P (76,000 lies 1.33 % above it): refuted when contested. No price true at P liquidates the
/// long: the lowest, 76,000 / 1.005, fails the trigger (g is inside it), so a true liquidation needs the loser below its
/// maintenance at the feed at both moments, and it then takes at most the loser's maintenance margin at the feed (at a
/// feed of 75,000 the long's 300 KAS, within q(0.1, 5 % of 75,000) = 375 KAS).
#[test]
fn r11_holds_a_liquidation_needs_the_loser_below_maintenance_when_it_lands() {
    let m = gold_market(100);
    let (step, w_) = (m.bond.step, m.p.challenge_daa);
    let g = m.p.g_bps;
    let pland: i64 = 30_000_000;
    let x_ = ((pland + w_ + step) / step + 1) * step;
    let p = gold_pair(&m, x_, 0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x61, (pland - 1_000_000) as u64);
    let now_price = 7_600_000_000_000i64;
    let dip = 7_500_000_000_000i64;
    assert!(true_at(dip, dip, g) && !true_at(dip, now_price, g), "the dip claim is false at P");
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: p.size, p: dip, t0: pland - 50, lock: pland as u64 }, pland as u64);
    let refuted = lw.s.contested(pland, x(&keypair(70))).decided(false, pland + 1);
    assert!(matches!(refuted.settlement(lw.value, pland), Settlement::Restore { proposer: 0, .. }), "refuted: restored, the bond to the long and the disputer");
    let lowest = ((now_price as i128 * 10_000 + 10_000 + g as i128 - 1) / (10_000 + g as i128)) as i64;
    assert!(true_at(lowest, now_price, g) && !p.long_liquidatable(lowest), "no price true at P liquidates a long above its maintenance");
    let k = kit();
    assert!(p.pending_of(k, p.size, LIQ, L, lowest, pland).is_none());
    // A due liquidation: the feed at 75,000 at both moments; the long loses its value at the feed, its maintenance margin.
    let lost = q(p.size, dip - p.bl, m.p.scale);
    let bound = q(p.size, dip * m.p.mmr_bps / 10_000, m.p.scale);
    assert!(lost <= bound);
    println!("a true liquidation at a feed of 75,000 takes the long's {} KAS, within its maintenance margin at the feed {} KAS; the dip claim is refuted", kas(lost), kas(bound));
}

/// R11-F3, fixed. With lateDaa 100 (10 s) an honest, true liquidation (the feed 75,000 at t0 and at P) landing 100, 101
/// or 6,000 DAA after its t0 settles: lateness voids nothing. One signed 50 DAA before the last valid landing of the term
/// and delayed 100 DAA (across it) is void, and its bond comes back to the underwriter (before: 200 KAS to the loser it
/// was right about). A liquidation from a pair UTXO accepted after the last valid landing is refused at the proposal, so
/// a void claim cannot be repeated for free: a frozen part is frozen once, for W.
#[test]
fn r11_holds_a_late_honest_claim_settles_and_one_delayed_past_its_last_landing_keeps_its_bond() {
    let m = gold_market(100);
    let (step, w_) = (m.bond.step, m.p.challenge_daa);
    let t0: i64 = 30_000_000;
    let x_ = ((t0 + 10_000 + w_ + step) / step + 1) * step;
    let p = gold_pair(&m, x_, 0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x62, (t0 - 1_000_000) as u64);
    for land in [t0 + 100, t0 + 101, t0 + 6_000] {
        let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: p.size, p: 7_500_000_000_000, t0, lock: t0 as u64 }, land as u64);
        let (ft, s) = finalize_tx(&lw, land, w_ as u64);
        must("finalize", &ft);
        assert!(matches!(s, Settlement::Forfeit { .. }), "landing {} DAA after t0: {s:?}", land - t0);
    }
    let last = OPending::last_valid_landing(&m, x_);
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: p.size, p: 7_500_000_000_000, t0: last - 50, lock: (last - 50) as u64 }, (last + 50) as u64);
    let (ft, s) = finalize_tx(&lw, last + 50, w_ as u64);
    must("finalize the claim delayed across its last valid landing", &ft);
    let Settlement::Restore { defender, proposer, .. } = s else { panic!("{s:?}") };
    assert_eq!((defender, proposer), (0, lw.s.bond));
    println!("delayed across the last valid landing: void, the bond {} KAS back to the underwriter, the defender 0", kas(proposer));
    let young = Live { daa: (last + 1) as u64, ..lp.clone() };
    refused("a liquidation from a pair UTXO accepted after the last valid landing", &propose_tx(&young, &ClaimSpec { kind: LIQ, side: L, n: p.size, p: 7_500_000_000_000, t0: last + 1, lock: (last + 1) as u64 }, &uw_kp(), None).0, 0);
}

// ---------------------------------------------------------------- lead 2: a party's own exit (R11-F1)

/// R11-F1, fixed (29.3). The review's hold: an underwriter that renews on time and vouches nothing renewed the gold pair
/// 52 times (3.99 years), each side paying 398.90 KAS of its 800 KAS margin, before the bands stopped it. The renewals
/// still work, but they hold nobody: after the first renewal and after the 26th, the long closes its own side at the
/// feed with its own bond and its own signature only (no underwriter input, no counterparty), and the close settles W
/// later at the feed moved by g against it. A party pays premiums only for the terms it chooses to stay. (After the 52nd
/// the long stands within g of its maintenance band at the feed: its close needs a top-up first, within its share.)
#[test]
fn r11_holds_a_party_leaves_by_its_own_close_while_its_underwriter_renews() {
    let m = gold_market(100);
    let (step, term, w_) = (m.bond.step, m.bond.term, m.p.challenge_daa);
    let prem = premium_10pct(&m, 2_000_000_000_000);
    let x0 = 10 * step;
    let p = gold_pair(&m, x0, prem);
    let mut lp = genesis_pair(p.clone(), p.floor() as u64, 0x63, (x0 - term) as u64);
    let mut sl = gold_slot(&m, 10_000 * KAS as u64);
    let mut n = 0i64;
    let own_close = |lp: &LPair, at: u64| {
        let (t, w) = self_close_tx(lp, L, lp.s.size, E, at as i64, at, &long_kp());
        must("the long's own close (no underwriter input)", &t);
        let lw = live(&t, 0, at, &w);
        let (ft, s) = finalize_tx(&lw, at as i64, w_ as u64);
        must("its finalize W later", &ft);
        let Settlement::Close { long, .. } = s else { panic!("{s:?}") };
        long - lw.s.refund
    };
    while lp.s.renewed().is_some() {
        let lock = (lp.s.b.expiry - step) as u64;
        let (t, r2, b2) = renew_at(&lp, &sl, lock, advance_to(&sl.s, lp.s.b.expiry + term), m.bond.rate as u64);
        must(&format!("renewal {}", n + 1), &t);
        lp = live(&t, 0, lock, &r2);
        sl = live(&t, 1, lock, &b2);
        n += 1;
        if n == 1 || n == 26 {
            let paid = own_close(&lp, lock + m.p.quiet_daa as u64);
            println!("after renewal {n}: the long closes its own side at the feed and is paid {} KAS (its value at the feed {} KAS)", kas(paid), kas(lp.s.long_at(E)));
        }
    }
    assert_eq!(n, 52, "the underwriter may still renew 52 times; it holds nobody");
}

// ---------------------------------------------------------------- lead 5: the slot's rate limit against `advance` (R11-F4)

/// R11-F4, fixed (29.3): the market bounds require `2 rateDaa <= stepDaa` (all five templates' checkBond and the
/// library's). The review's market (rate 150, step 100) is refused by the library and by the slot itself: the outsider's
/// advance fails `checkBond`, so no slot, pair or order of that market can exist. At rate 50 (half a step) the advance
/// runs, and the owner's bind is usable from the slot's DAA + 50, before the next advance (one step later).
#[test]
fn r11_holds_a_market_whose_rate_limit_exceeds_half_a_step_is_refused() {
    for (rate, ok) in [(150i64, false), (100, false), (50, true)] {
        let m = omarket_with(kpf_protocol::bond::BondParams { rho_bps: 10_000, step: 100, term: 800, tail: 200, rate, unbond: 50 });
        assert_eq!(m.check().is_ok(), ok, "rate {rate}");
        let sl = Live { op: uop(SLOT_TAG, 1), value: K, daa: 0, id: Some(Hash::from_bytes([SLOT_TAG; 32])), s: Bond::new(x(&uw_kp()), m.bond_market(), 0) };
        let at = (sl.s.j0 + 1) * m.bond.step + m.bond.tail;
        let ctx = Ctx { lock: at, seq: 0, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: false, has_id: true };
        let b2 = sl.s.advance(&ctx, 1, sl.value as i64).unwrap_or_else(|_| Bond { j0: sl.s.j0 + 1, ..sl.s.clone() });
        let ins = vec![slot_in(&sl, "advance", vec![b8(1)]), key_in(uop(0x65, at as u32), 10 * KAS as u64, 0, &keypair(88))];
        let t = build_ext(&ins, vec![out(sl.value, bond_spk(&b2), bind(0, sl.id.unwrap())), out(9 * KAS as u64, p2pk_spk(&keypair(88)), None)], at as u64, &[0, 0]);
        if ok {
            must("an outsider's advance on a market with rate 50, step 100", &t);
            assert!(rate < m.bond.step, "the owner's bind is usable from the slot's DAA + {rate}, before the next advance");
        } else {
            refused(&format!("any entry of a slot of a market with rate {rate}, step 100"), &t, 0);
        }
    }
}

// ---------------------------------------------------------------- lead 3 / lead 4: the part floor of a pair (R11-F7)

/// A partial transfer of `n` units of the listed short side of `lp` to the signing taker `ek` at price `xp`, keeping the
/// short's bound: in 0 the pair, 1 the taker's funding; out 0 the taker's change, 1 the part (fresh id), 2 the remainder
/// (the pair's id). The leaver's payout below the payout floor stays in the remainder.
fn shred_once(lp: &LPair, n: i64, xp: i64, ek: &Keypair, fop: TransactionOutpoint) -> (T, OPair, OPair) {
    let p = &lp.s;
    let mut part = p.rest_of(n);
    part.short_key = x(ek);
    (part.short_bid, part.short_min, part.short_tip) = (0, 0, 0);
    part.mk.parent = maker_of(&lp.op);
    let rest = p.rest_of(p.size - n);
    let pay = kpf_protocol::payoff::owed(n, p.bs - xp, p.short_tip, p.m.p.scale);
    let keep = lp.value as i64 - p.range_of(n) + if pay < DUST { pay } else { 0 };
    let fv = 1_000 * KAS;
    let pv = part.range_of(n) + p.m.p.carrier;
    let args = vec![iv(S), iv(n), iv(xp), iv(p.bs), bv(&x(ek)), bv(&[0; 32]), iv(1)];
    let ins = vec![pair_in(lp, "transfer", args), key_in(fop, fv as u64, 0, ek)];
    let paid = if pay >= DUST { pay } else { 0 };
    let first = if paid > 0 { out(paid as u64, key_spk(&p.short_key), None) } else { out((fv - pv - 1_000_000) as u64, p2pk_spk(ek), None) };
    let mut outs = vec![first, out(pv as u64, pair_spk(&part), None), out(keep as u64, pair_spk(&rest), bind(0, lp.id.unwrap()))];
    if paid > 0 {
        outs.push(out((fv - pv - paid - 1_000_000) as u64, p2pk_spk(ek), None));
    }
    seal(&mut outs, 1, 0, lp.op);
    (build_ext(&ins, outs, 0, &[p.m.p.quiet_daa as u64, 0]), part, rest)
}

/// R11-F7, fixed (29.3): `checkPart` gives each side with value at the entry at least the part floor (0.2 KAS) of it in
/// each piece of every split (transfer, closeHost, closeDonor, mutualClose, propose). The review's attack on the 1-unit
/// pair at 1 KAS a unit, 10x (the passive long's whole margin 0.1 KAS) is refused at its first take: no split of that
/// pair leaves the long 0.2 KAS in each piece. The long keeps its 0.1 KAS at the lapse settle. On the gold pair a split
/// of 0.01 oz (8 KAS of each side's value) is accepted.
#[test]
fn r11_holds_hand_overs_cannot_split_a_passive_side_below_the_part_floor() {
    let m = gold_market(100);
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
    let lp = genesis_pair(p0.clone(), p0.floor() as u64, 0x66, 1_000);
    let (l_before, _) = p0.lapse_out(lp.value);
    assert_eq!(l_before, 10_000_000, "the long's 0.1 KAS is paid at the lapse settle of the whole pair");
    let ek = keypair(77);
    for n in [19_000_000i64, 50_000_000] {
        assert!(!p0.split_ok(n));
        refused(&format!("a take of {n} base units of the short side, leaving the long less than 0.2 KAS in a piece"), &shred_once(&lp, n, 100_000_000, &ek, uop(0x67, n as u32)).0, 0);
    }
    must("the long's lapse settle of the whole pair", &lapse_tx(&lp, lp.s.lapse_from(), 0));
    // The gold pair (800 KAS a side at 0.1 oz): a split of 0.01 oz leaves each side 80 KAS in the smaller piece.
    let mut g = gold_pair(&m, 10 * m.bond.step, 0);
    g.short_bid = E;
    let lg = genesis_pair(g.clone(), g.floor() as u64, 0x6a, 1_000);
    assert!(g.split_ok(1_000_000));
    must("a take of 0.01 oz of the gold pair's short side", &shred_once(&lg, 1_000_000, E, &ek, uop(0x6b, 0)).0);
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

/// Lead 1 / lead 5 (the vouch, 16 "The vouched claim"), R11-F6: bounded, the bound at the feed asserted. The evidence of
/// a liquidation is any P2PK input of `uwKey` signed SIGHASH_ALL in the same transaction; its value may go anywhere. A
/// transaction in which the underwriter's input pays a merchant 1 KAS, built by that merchant (a signing request of a web
/// wallet), and that also spends the fixture pair by `propose` with a false price, is accepted by the scripts. What it
/// can take: the long's value at the feed (the feed 10 KAS: 1,000 KAS) if nobody contests within W; the price (5 KAS)
/// is false at t0 and at P, so a watcher's contest refutes it and the merchant's bond goes to the long and the disputer.
/// Since review 11 a close carries no vouch at all, so only a liquidation can be smuggled this way, and the tool refuses
/// to sign with an underwriter key any transaction holding a proposal beside a close or that it did not build as the
/// vouched liquidation (`OBuilt::vouch_guard`, exercised in the honest life); the documentation tells underwriters to keep
/// vouching funds on a key of their own. A contract-level binding (the vouch naming the claim) is not worth its size:
/// it needs a tagged script or a data signature and one more output per claim, and protects only an underwriter that
/// signs others' transactions with its slot key, which the tool never asks it to.
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
    // The bound at the feed (10 KAS): the long's whole value there, unless contested within W; contested, refuted.
    let lost = lp.s.long_at(1_000_000_000);
    assert_eq!(lost, 100_000_000_000, "1,000 KAS: the long's value at the feed");
    let refuted = lw.s.contested(400, x(&keypair(70))).decided(false, 401);
    let Settlement::Restore { defender, disputer, .. } = refuted.settlement(lw.value, 400) else { panic!() };
    assert_eq!(defender + disputer, lw.s.bond);
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
        pub keys: KeySet,
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
        // Review 11 (R11-F1): B's long closes its own side at the feed (80,000), its own bond, no underwriter input.
        // R11-F6: the same close funded from the underwriter's key is refused at signing (it would vouch it).
        let f = c.mint(uk, 300 * KAS);
        let bad = propose_close(&b_, LONG, b_.s.size, super::E, 10_001_000, &[f], &uk, r).unwrap().0;
        let e = bad.sign(&c.keys).unwrap_err().to_string();
        assert!(e.contains("R11-F6"), "{e}");
        let f = c.mint(lk, 300 * KAS);
        let (bb, w) = propose_close(&b_, LONG, b_.s.size, super::E, 10_001_000, &[f], &lk, r).unwrap();
        let sg = c.sign("B: the long closes its own side at the feed", Ok(bb));
        let pend = nth(&sg, 0, w, 10_001_000);
        let f = c.mint(ek, 10 * KAS);
        let (bb, s) = finalize(&pend, &[f], &ek, r).unwrap();
        let Settlement::Close { long, .. } = s else { panic!("{s:?}") };
        assert_eq!(long - pend.s.refund, kpf_protocol::payoff::q(b_.s.size, pend.s.close_price() - b_.s.bl, sc, 0));
        c.sign("B: finalize its own close, W later", Ok(bb));
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
        for (name, p) in [("C: lapse settle of the forfeited part", &cpair), ("A: lapse settle after its renewed expiry", &a)] {
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
