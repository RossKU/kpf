//! Review 12 (docs/audit/REVIEW-12.md): the oracle-market family at commit `1313454` (the fix of review 11: a party's
//! own close without the underwriter, the two-moment rule, no void for lateness, a void liquidation returns its bond, the
//! part floor in every split, W = 2 minutes and lateDaa = 10 s on the testnet defaults). Every transaction here runs
//! every input in the real script engine with the covenant context (`common::run`), except the honest life
//! (`r12_holds_the_honest_life_with_own_closes_...`), which builds every transaction with the tool's own builders on the
//! pinned artifacts and checks it with `verify::verify`.
//!
//! `r12_exploit_` tests an attack that works, `r12_holds_` one that is refused (or a shape that keeps its rule),
//! `r12_bounded_` a residual that is measured. Worked numbers use the gold-like pair of review 11 on the testnet
//! defaults (`OMarket::testnet`): 0.1 oz at 80,000 KAS an ounce (8,000 KAS notional), 10x on each side (bounds 72,000 and
//! 88,000, 800 KAS of margin each), cover rate 20,000 KAS an ounce (2,000 KAS of cover, a 200 KAS bond), W 1,200 DAA
//! (2 minutes), quiet 600 DAA (60 s), judge rounds 432,000 DAA (12 hours), stake age 6,048,000 DAA (7 days), 100 KAS
//! stakes, mu 1.25, at 10 DAA/s.

mod common;
mod oraclekit;

use common::*;
use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::bond::{BindArgs, Bond, Claim, Ctx};
use kpf_protocol::judge::{Js, YES};
use kpf_protocol::oracle::*;
use oraclekit::*;
use secp256k1::Keypair;

const KAS: i64 = 100_000_000;
const DUST: i64 = 2_000_000;
/// 80,000 KAS an ounce.
const E: i64 = 8_000_000_000_000;
const SLOT_TAG: u8 = 0x5d;

fn uop(tag: u8, n: u32) -> TransactionOutpoint {
    let mut b = [tag; 32];
    b[..4].copy_from_slice(&n.to_le_bytes());
    TransactionOutpoint { transaction_id: TransactionId::from_bytes(b), index: n }
}

fn kas(v: i64) -> String {
    format!("{:.4}", v as f64 / KAS as f64)
}

/// The testnet defaults as shipped after review 11 (W 1,200, lateDaa 100, quiet 600).
fn gold_market() -> OMarket {
    let m = OMarket::testnet(feed_hash_of(b"review 12: a gold-like underlying priced in KAS"));
    assert!(m.check().is_ok());
    assert_eq!((m.p.challenge_daa, m.p.late_daa, m.p.quiet_daa), (1_200, 100, 600));
    m
}

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

/// An expiry that leaves every claim landing at or after `from` valid for at least `room` DAA.
fn expiry_after(m: &OMarket, from: i64, room: i64) -> i64 {
    let (step, w_) = (m.bond.step, m.p.challenge_daa);
    ((from + room + w_ + step) / step + 1) * step
}

/// A free judge stake of this market owned by `keypair(seed)` (the signer `contest_tx` and `deposit_tx` use).
fn gstake(m: &OMarket, seed: u8, v: u64, daa: u64, tag: u8) -> LStake {
    Live { op: uop(tag, seed as u32), value: v, daa, id: None, s: Js::free(x(&keypair(seed)), m.judge_market()) }
}

fn true_at(p: i64, f: i64, g: i64) -> bool {
    let (p, f, g) = (p as i128, f as i128, g as i128);
    f * 10_000 >= p * (10_000 - g) && f * 10_000 <= p * (10_000 + g)
}

/// The lowest price at which `side` may close its own side of `p` (the contract's maintenance rule at the close price).
fn lowest_close(p: &OPair, side: i64) -> i64 {
    assert_eq!(side, L);
    let (mut lo, mut hi) = (p.bl, p.bs * 2);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if p.close_ok(L, mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

/// The highest price at which the short may close its own side of `p`.
fn highest_short_close(p: &OPair) -> i64 {
    let (mut lo, mut hi) = (0i64, p.bs);
    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        if p.close_ok(S, mid) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// `self_close_tx` with the pair input's sequence given (the kit's commits quietDaa).
#[allow(clippy::too_many_arguments)]
fn close_seq(lp: &LPair, side: i64, n: i64, p: i64, t0: i64, lock: u64, closer: &Keypair, seq: u64) -> (T, OPending) {
    let k = kit();
    let (w, fb) = lp.s.pending_of(k, n, CLOSE, side, p, t0).expect("a close the model accepts");
    let w = w.made_by(&lp.op);
    let args = vec![iv(n), iv(CLOSE), iv(side), iv(p), iv(t0), iv(1), Arg::Sig(*closer), iv(fb), iv(0), bv(&k.pending.prefix), bv(&k.pending.suffix)];
    let fv: i64 = 1_000_000_000_000;
    let fop = TransactionOutpoint { transaction_id: lp.op.transaction_id, index: 9_100 + lp.op.index };
    let ins = vec![pair_in(lp, "propose", args), key_in(fop, fv as u64, 0, closer)];
    let id = lp.id.unwrap();
    let pv = lp.s.pending_value(lp.value, &w);
    let mut outs = vec![out(pv as u64, pend_spk(&w), None)];
    let mut adds = pv - lp.value as i64;
    if n < lp.s.size {
        let rest = lp.s.rest_of(lp.s.size - n);
        let rv = lp.s.rest_value(lp.value, n);
        adds += rv;
        outs.push(out(rv as u64, pair_spk(&rest), bind(0, id)));
    }
    outs.push(out((fv - adds - 10_000) as u64, p2pk_spk(closer), None));
    place(&mut outs, 0, 0, lp.op, id, Lineage::Fresh);
    (build_ext(&ins, outs, lock, &[seq, 0]), w)
}

/// A contest by `st` (owner `keypair(seed)`) at `at`, resolved one round later with no deposit, and the finalize.
/// Returns the settlement, the finalize, the DAA the decided part was finalized at, the pool and the opener's receipt.
fn contest_and_resolve(lw: &LPend, st: &LStake, seed: u8, at: u64) -> (Settlement, T, u64, LStake, LStake) {
    let (ct, w2, rc, dj) = contest_tx(lw, st, seed, at);
    must("contest", &ct);
    let (cw, r0, ld) = (live(&ct, 0, at, &w2), live(&ct, 1, at, &rc), live(&ct, 2, at, &dj));
    let r_daa = at + lw.s.m.judge.round as u64;
    let (dt, pool, dw) = decide_tx(&ld, &cw, r_daa);
    must("resolve and decide", &dt);
    let (lpool, ldw) = (live(&dt, 0, r_daa, &pool), live(&dt, 1, r_daa, &dw));
    let (ft, s) = finalize_tx(&ldw, ldw.s.clock, 0);
    must("finalize the decided part", &ft);
    (s, ft, r_daa, lpool, r0)
}

fn gold_slot(m: &OMarket, value: u64) -> LSlot {
    Live { op: uop(SLOT_TAG, 0), value, daa: 0, id: Some(Hash::from_bytes([SLOT_TAG; 32])), s: Bond::new(x(&uw_kp()), m.bond_market(), 0) }
}

fn advance_to(sl: &Bond, x2: i64) -> i64 {
    (x2 / sl.m.p.step - 12 - sl.j0).max(0)
}

/// A renewal in the gold market (the slot's rate limit as its sequence): in 0 the pair, 1 the slot, 2 the executor.
fn renew_at(lp: &LPair, sl: &LSlot, lock: u64) -> (T, OPair, Bond) {
    let (r, pay) = lp.s.renewed().unwrap_or((lp.s.clone(), 0));
    let cover = lp.s.cover_of(lp.s.size);
    let advance = advance_to(&sl.s, lp.s.b.expiry + lp.s.m.bond.term);
    let a = BindArgs::new(lock as i64, lp.s.b.expiry + lp.s.m.bond.term, advance, &[Claim::input(0, cover)]);
    let ctx = Ctx { lock: lock as i64, seq: sl.s.m.p.rate, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true };
    let b2 = sl.s.bind(&ctx, &a, sl.value as i64).unwrap_or_else(|_| sl.s.clone());
    let ins = vec![pair_in(lp, "renew", vec![iv(1), iv(0)]), slot_in(sl, "bind", bind_args(&a, &uw_kp())), exec_in(219)];
    let outs = vec![
        out(lp.value - pay as u64, pair_spk(&r), bind(0, lp.id.unwrap())),
        out(sl.value, bond_spk(&b2), bind(1, sl.id.unwrap())),
        out((pay as u64).max(DUST as u64), key_spk(&sl.s.owner), None),
        change(90_000_000_000),
    ];
    (build_ext(&ins, outs, lock, &[0, sl.s.m.p.rate as u64, 0]), r, b2)
}

/// E[n (f_T - bl) 1{f_T >= k}] for f_T lognormal around f0 with volatility `sigma` a year over `secs` seconds (no drift):
/// what a loser below its trigger keeps when the judge's round delays its liquidation and the feed recovers to `k`,
/// where it can close its own side at the feed. KAS for 0.1 oz.
fn kept_after_delay(f0: f64, bl: f64, k: f64, sigma: f64, secs: f64, units: f64) -> f64 {
    let s = sigma * (secs / 31_557_600.0).sqrt();
    let n = 40_000;
    let (a, b) = (-10.0f64, 10.0f64);
    let h = (b - a) / n as f64;
    let mut sum = 0.0;
    for i in 0..=n {
        let z = a + i as f64 * h;
        let f = f0 * (s * z - s * s / 2.0).exp();
        let v = if f >= k { units * (f - bl) } else { 0.0 };
        let wgt = if i == 0 || i == n { 1.0 } else if i % 2 == 1 { 4.0 } else { 2.0 };
        sum += wgt * v * (-z * z / 2.0).exp();
    }
    sum * h / 3.0 / (2.0 * std::f64::consts::PI).sqrt()
}

// ---------------------------------------------------------------- lead 1: the party's own close

/// Lead 1 (R12-F1). A party alone takes its counterparty's whole side with a false close: the long of the gold pair
/// closes at 88,442.21 (the lowest price whose close price, moved by g against it, is the short's bound 88,000; the feed
/// is 80,000 at t0 and at P, so the claim is false at both moments). It needs only the long's signature and its own bond
/// (200 KAS); uncontested within W (2 minutes) it settles: the long gets 1,600 KAS (the whole range) and its bond back,
/// the short 0.2 KAS (the carrier) for its 800 KAS of value at the feed. Contested and refuted it costs the long the
/// bond (160 KAS to the short, 40 to the disputer). The break-even contest probability is 800 / (800 + 200) = 80 %: a
/// false close is a winning bet against any counterparty that is contested less often than that. The short's mirror (a
/// close at 1 sompi) takes the long's side. And the tools cannot see it: the pending part's address commits to the
/// claimed price and t0, so the victim cannot compute it, and `kpf oracle-watch` reads claims only from object files
/// (the node API looks UTXOs up by address); the victim's pair file only turns into "NOT AT THE NODE".
#[test]
fn r12_exploit_a_party_takes_its_counterpartys_side_by_a_false_close_the_tools_cannot_see() {
    let m = gold_market();
    let w_ = m.p.challenge_daa;
    let g = m.p.g_bps;
    let pland: i64 = 30_000_000;
    let p = gold_pair(&m, expiry_after(&m, pland, 0), 0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x70, (pland - 1_000_000) as u64);
    let pf = ((p.bs as i128 * 10_000 + (10_000 - g as i128) - 1) / (10_000 - g as i128)) as i64;
    assert!(p.close_ok(L, pf) && !true_at(pf, E, g), "the price passes the closer's maintenance rule and is false at the feed");
    let (t, w) = self_close_tx(&lp, L, p.size, pf, pland, pland as u64, &long_kp());
    must("the long's false close, its own signature and bond only", &t);
    let lw = live(&t, 0, pland as u64, &w);
    let (ft, s) = finalize_tx(&lw, pland, w_ as u64);
    must("finalize W later, uncontested", &ft);
    let Settlement::Close { long, short } = s else { panic!("{s:?}") };
    let short_fair = lp.value as i64 - p.long_at(E);
    let taken = short_fair - short;
    assert_eq!(long - w.refund, p.range_of(p.size), "the long is paid the whole range");
    assert_eq!(short, m.p.carrier, "the short keeps only the carrier");
    assert_eq!(taken, 800 * KAS);
    assert_eq!(w.bond, 200 * KAS);
    let refuted = lw.s.contested(pland, x(&keypair(70))).decided(false, pland + 1);
    let Settlement::Restore { defender, disputer, .. } = refuted.settlement(lw.value, pland) else { panic!() };
    assert_eq!((defender, disputer), (160 * KAS, 40 * KAS));
    // The short's mirror: a close at 1 sompi takes the long's side.
    assert!(p.close_ok(S, 1));
    let (t2, w2) = self_close_tx(&lp, S, p.size, 1, pland, pland as u64, &short_kp());
    must("the short's false close at 1 sompi", &t2);
    let (ft2, s2) = finalize_tx(&live(&t2, 0, pland as u64, &w2), pland, w_ as u64);
    must("finalize", &ft2);
    let Settlement::Close { long: l2, .. } = s2 else { panic!() };
    assert_eq!(l2, m.p.carrier, "the long keeps only the carrier");
    // The victim cannot compute where the claim is: one sompi of price or one DAA of t0 is another address.
    let (_, w3) = self_close_tx(&lp, L, p.size, pf + 1, pland, pland as u64, &long_kp());
    let (_, w4) = self_close_tx(&lp, L, p.size, pf, pland - 1, pland as u64, &long_kp());
    assert_ne!(pend_spk(&w), pend_spk(&w3));
    assert_ne!(pend_spk(&w), pend_spk(&w4));
    // The economics.
    let be = taken as f64 / (taken + w.bond) as f64;
    println!("a false close takes {} KAS for a bond of {} KAS: break-even contest probability {:.1} %", kas(taken), kas(w.bond), be * 100.0);
    for pi in [0.0, 0.5, 0.8, 0.9, 0.99] {
        println!("  contested with probability {:.0} %: the closer's expected gain {} KAS", pi * 100.0, kas(((1.0 - pi) * taken as f64 - pi * w.bond as f64) as i64));
    }
    // Leverage does not help: with the cover at the range the bond is sigma of the range and the take half of it.
    for lev in [2i64, 5, 10, 20] {
        let margin = q(p.size, E / lev, m.p.scale);
        for (name, cr) in [("cover = range", 2 * E / lev), ("cover = 1.25 range", 5 * E / (2 * lev))] {
            let bond = m.bond_of(p.size, cr);
            println!("  {lev}x, {name}: take {} KAS, bond {} KAS, break-even {:.1} %", kas(margin), kas(bond), 100.0 * margin as f64 / (margin + bond) as f64);
        }
    }
    println!("  the worst case (the victim's value at the feed is the whole cover): break-even 1 / (1 + sigma) = {:.1} %", 100.0 / (1.0 + m.p.sigma_bps as f64 / 10_000.0));
}

/// Lead 1 / lead 3 (R12-F2). A true close (the long at the feed, 80,000) contested by the counterparty itself with one
/// aged stake of its own, and nobody deposits YES: NO leads, the claim is refuted, the short receives the whole bond (160
/// KAS as the defender and 40 as the disputer) and its stake back with the pool, and the long is back in the pair, 200
/// KAS poorer. The contract offers the defence (the closer's YES deposit of 1.25 times the opener's stake, aged 7 days
/// before the claim: then upheld, settled at the close price, and the short's stake forfeited), but the closer needs an
/// aged stake and the dispute's UTXO, which is a new output of the contester's transaction that the closer's tools cannot
/// find (`oracle-watch` shows the closer's own claim only as gone from the node, and prompts no defence).
#[test]
fn r12_bounded_a_false_contest_of_a_true_close_that_nobody_defends_takes_the_closers_bond() {
    let m = gold_market();
    let w_ = m.p.challenge_daa as u64;
    let pland: i64 = 30_000_000;
    let p = gold_pair(&m, expiry_after(&m, pland, 2_000_000), 0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x71, (pland - 1_000_000) as u64);
    let (t, w) = self_close_tx(&lp, L, p.size, E, pland, pland as u64, &long_kp());
    must("the long's true close at the feed", &t);
    let lw = live(&t, 0, pland as u64, &w);
    let short_stake = gstake(&m, 42, 100 * KAS as u64, 0, 0xa1);
    let (s, _, r_daa, pool, receipt) = contest_and_resolve(&lw, &short_stake, 42, pland as u64 + 1);
    let Settlement::Restore { defender, disputer, part, .. } = s else { panic!("{s:?}") };
    assert_eq!((defender, disputer), (160 * KAS, 40 * KAS));
    assert_eq!((w.defender(), x(&short_kp())), (x(&short_kp()), x(&short_kp())), "defender and disputer are the short");
    assert_eq!(part, p.restore_state(p.size), "the long is back in the pair");
    let (stx, _, pays) = settle_tx(&pool, &[receipt.clone()], r_daa + 10);
    must("the short's receipt wins the pool (NO led)", &stx);
    assert!(pays[0] >= short_stake.value, "the short's stake comes back");
    println!("an undefended false contest of a true close: the short receives {} KAS of the long's bond and its {} KAS stake back; the long is restored after {} DAA (12 h)", kas(defender + disputer), kas(short_stake.value as i64), m.judge.round);
    // Defended: the long deposits YES with an aged stake of 125 KAS; upheld, the close settles, the short's stake lost.
    let (ct, w2, rc, dj) = contest_tx(&lw, &short_stake, 42, pland as u64 + 1);
    must("contest", &ct);
    let (cw, r_short, ld) = (live(&ct, 0, pland as u64 + 1, &w2), live(&ct, 1, pland as u64 + 1, &rc), live(&ct, 2, pland as u64 + 1, &dj));
    let long_stake = gstake(&m, 41, 125 * KAS as u64, 0, 0xa2);
    let (dt, d2, rs) = deposit_tx(&ld, YES, &[(long_stake, 41)], pland as u64 + 10);
    must("the long's YES deposit", &dt);
    assert_eq!(d2.side, YES);
    let ld2 = live(&dt, 0, pland as u64 + 10, &d2);
    let r_long = live(&dt, 1, pland as u64 + 10, &rs[0]);
    let r2 = pland as u64 + 10 + m.judge.round as u64;
    let (xt, pool2, dw) = decide_tx(&ld2, &cw, r2);
    must("resolve and decide: upheld", &xt);
    let (ft, s) = finalize_tx(&live(&xt, 1, r2, &dw), pland, 0);
    must("finalize the upheld close", &ft);
    assert!(matches!(s, Settlement::Close { .. }));
    let (stx, _, pays) = settle_tx(&live(&xt, 0, r2, &pool2), &[r_short, r_long], r2 + 10);
    must("settle the pool", &stx);
    assert_eq!(pays[0], 0, "the short's stake is forfeited");
    let _ = w_;
}

/// Lead 1 (R12-F3). The quiet time of a party's own close is the pair UTXO's age, and the counterparty restarts it with
/// any entry of its own (an amend, 0.033 KAS): at each minute's boundary the close and the counterparty's amend become
/// valid at the same DAA and spend the same outpoint, so whoever lands first (a miner's choice, or the higher fee) wins
/// it; a close one DAA early is refused by the script. Five rounds measured; at the documented amend fee the hold costs
/// the counterparty 47.52 KAS a day. The underwriter's liquidation needs no quiet time, so it is not held.
#[test]
fn r12_bounded_the_counterparty_restarts_the_quiet_time_of_a_close_every_minute() {
    let m = gold_market();
    let quiet = m.p.quiet_daa as u64;
    let pland: i64 = 30_000_000;
    let p = gold_pair(&m, expiry_after(&m, pland, 1_000_000), 0);
    let mut cur = genesis_pair(p.clone(), p.floor() as u64, 0x72, (pland - 1_000_000) as u64);
    let mut now = cur.daa + quiet;
    for i in 0..5 {
        let (ct, _) = close_seq(&cur, L, p.size, E, now as i64, now, &long_kp(), quiet);
        must("the long's close, valid from the pair's age 600", &ct);
        let (at, a) = amend_tx(&cur, S, &short_kp(), 1_000 + i, quiet);
        must("the short's amend at the same age (spends the same outpoint)", &at);
        cur = live(&at, 0, now, &a);
        let (early, _) = close_seq(&cur, L, p.size, E, now as i64, now, &long_kp(), quiet - 1);
        refused("the long's close one DAA before the amended pair's quiet time", &early, 0);
        now += quiet;
    }
    let (lt, _) = propose_tx(&cur, &ClaimSpec { kind: LIQ, side: L, n: p.size, p: 7_500_000_000_000, t0: now as i64, lock: now }, &uw_kp(), None);
    must("the underwriter's liquidation needs no quiet time (the model's claim at 75,000)", &lt);
    println!("each minute the close races an amend of the counterparty (0.033 KAS each): 1,440 a day = {} KAS a day", kas(1_440 * 3_300_000));
}

/// Lead 1 (R12-F4): a false close racing a due liquidation. The long of the gold pair is below maintenance at the feed
/// (75,000; trigger 75,789.47): the underwriter's liquidation at 75,000 is valid, and so is the long's false close at
/// 76,170.33 (the lowest price its own maintenance rule accepts), both spending the same pair UTXO. If the close lands,
/// a watcher contests it, it is refuted after a round (12 h) and the part restored; the liquidation from the restored UTXO
/// lands at once (no quiet time), the long's next close is refused for 60 s. The loser bought 12 h 2 min of delay for
/// the bond (200 KAS, 160 to the short). The delay is worth E[n (f - bl) 1{f >= 75,789}] after 12 h to a loser that has
/// no cure: printed for both volatilities; with a cure on offer (the gold pair at 75,000: a top-up of 110.63 KAS keeps
/// the position) the race buys nothing a cure does not.
#[test]
fn r12_bounded_a_false_close_racing_a_due_liquidation_buys_a_judge_round_for_its_bond() {
    let m = gold_market();
    let quiet = m.p.quiet_daa as u64;
    let f0 = 7_500_000_000_000i64;
    let pland: i64 = 30_000_000;
    let p = gold_pair(&m, expiry_after(&m, pland, 1_000_000), 0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x73, (pland - 1_000_000) as u64);
    assert!(p.long_liquidatable(f0) && !p.close_ok(L, f0));
    let liq = ClaimSpec { kind: LIQ, side: L, n: p.size, p: f0, t0: pland, lock: pland as u64 };
    must("the underwriter's true liquidation", &propose_tx(&lp, &liq, &uw_kp(), None).0);
    let pmin = lowest_close(&p, L);
    assert!(!p.close_ok(L, pmin - 1));
    let (t, w) = self_close_tx(&lp, L, p.size, pmin, pland, pland as u64, &long_kp());
    must("the loser's false close from the same UTXO", &t);
    assert!(!true_at(pmin, f0, m.p.g_bps));
    let lw = live(&t, 0, pland as u64, &w);
    let watcher = gstake(&m, 70, 100 * KAS as u64, 0, 0xa3);
    let (s, ft, r_daa, _, _) = contest_and_resolve(&lw, &watcher, 70, pland as u64 + 1);
    let Settlement::Restore { part, defender, disputer, .. } = s else { panic!("{s:?}") };
    let restored = live(&ft, 1, r_daa, &part);
    let liq2 = ClaimSpec { t0: r_daa as i64, lock: r_daa, ..liq };
    must("the liquidation from the restored part, at once", &propose_tx(&restored, &liq2, &uw_kp(), None).0);
    refused("the loser's second close before the quiet time", &close_seq(&restored, L, p.size, pmin, r_daa as i64, r_daa, &long_kp(), quiet - 1).0, 0);
    let delay = r_daa as i64 - pland;
    println!("the loser's false close at {}: refuted, {} KAS to the short and {} to the disputer; the liquidation waits {} DAA ({:.2} h)", kas(pmin), kas(defender), kas(disputer), delay, delay as f64 / 36_000.0);
    let cure = p.cure_bound_of(L, f0).map(|cb| q(p.size, p.bl - cb, m.p.scale));
    println!("a cure at 75,000 costs a top-up of {} KAS (the position is kept)", cure.map(kas).unwrap_or("none".into()));
    let trig = p.trigger_of(L) as f64 / 1e8;
    for sigma in [0.15, 0.80] {
        for (name, start) in [("at 75,000", 75_000.0), ("at the trigger", trig)] {
            let kept = kept_after_delay(start, 72_000.0, trig, sigma, delay as f64 / 10.0, 0.1);
            println!("  {:.0} % a year, the feed {name}: the loser keeps {kept:.2} KAS in expectation (a liquidation leaves it 0); net of the bond {:.2} KAS, if the close is contested for sure", sigma * 100.0, kept - 200.0);
        }
    }
}

/// Lead 1 / lead 2 (R12-F5): a false close pending across the expiry. The pair has not been renewed yet (the underwriter
/// may renew until X); the long falls below maintenance in the last step (where no liquidation is valid before a
/// renewal). At X - 12 h + 100 the long posts a false close; contested at once and refuted a round later, the part is
/// restored after X. A part written after its expiry is never renewed (review 9), and no liquidation can be proposed from
/// it (review 11's claimability): the loser is never liquidated. The short exits by its own close at the feed (or the
/// lapse settle at the entry); the long keeps its 300 KAS of value at the feed that the liquidation would have taken,
/// for its bond (200 KAS, 160 of it to the short). Had the underwriter renewed before the last round, the liquidation
/// from the renewed pair is valid.
#[test]
fn r12_bounded_a_false_close_pending_across_the_expiry_leaves_the_part_unliquidatable() {
    let m = gold_market();
    let (step, term, quiet, w_) = (m.bond.step, m.bond.term, m.p.quiet_daa as u64, m.p.challenge_daa as u64);
    let round = m.judge.round;
    let f0 = 7_500_000_000_000i64;
    let x0 = 10 * step;
    let p = gold_pair(&m, x0, 0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x74, (x0 - term) as u64);
    let sl = gold_slot(&m, 10_000 * KAS as u64);
    let at = (x0 - round + 100) as u64;
    let liq = |l: &LPair, d: u64| propose_tx(l, &ClaimSpec { kind: LIQ, side: L, n: p.size, p: f0, t0: d as i64, lock: d }, &uw_kp(), None).0;
    // A liquidation from the old UTXO is still accepted (its DAA score allows a valid one), but landing in the last step
    // it is void: frozen W, the bond back to the underwriter.
    let lwv = live(&liq(&lp, at), 0, at, &lp.s.pending_of(kit(), p.size, LIQ, L, f0, at as i64).unwrap().0.made_by(&lp.op));
    let (vt, vs) = finalize_tx(&lwv, at as i64, w_);
    must("the void liquidation's finalize", &vt);
    assert!(matches!(vs, Settlement::Restore { defender: 0, .. }), "a liquidation landing in the last step before a renewal is void: {vs:?}");
    let (t, w) = self_close_tx(&lp, L, p.size, lowest_close(&p, L), at as i64, at, &long_kp());
    must("the loser's false close in the last round before X", &t);
    let lw = live(&t, 0, at, &w);
    let watcher = gstake(&m, 70, 100 * KAS as u64, 0, 0xa4);
    let (s, ft, r_daa, _, _) = contest_and_resolve(&lw, &watcher, 70, at + 1);
    assert!(r_daa as i64 > x0);
    let Settlement::Restore { part, .. } = s else { panic!("{s:?}") };
    let restored = live(&ft, 1, r_daa, &part);
    refused("the renewal of the part restored after X", &renew_at(&restored, &sl, r_daa).0, 0);
    refused("a liquidation of the part restored after X", &liq(&restored, r_daa), 0);
    let (st, sw) = self_close_tx(&Live { daa: r_daa, ..restored.clone() }, S, p.size, f0, (r_daa + quiet) as i64, r_daa + quiet, &short_kp());
    must("the short's own close at the feed", &st);
    let (sf, ss) = finalize_tx(&live(&st, 0, r_daa + quiet, &sw), (r_daa + quiet) as i64, w_);
    must("its finalize", &sf);
    let Settlement::Close { long, short } = ss else { panic!() };
    println!("restored at {r_daa} (X {x0}): unrenewable, unliquidatable; the short closes at the feed: the long keeps {} KAS (its value at the feed {}), the short {} KAS; the liquidation would have forfeited the long's {} KAS", kas(long), kas(p.long_at(f0)), kas(short - sw.refund), kas(p.long_at(f0)));
    must("the lapse settle at the entry, the other way out", &lapse_tx(&restored, restored.s.lapse_from(), 0));
    // Renewed before the last round: the liquidation is valid again.
    let (rt, r2, _) = renew_at(&lp, &sl, (x0 - step) as u64);
    must("an on-time renewal at X - step", &rt);
    let renewed = live(&rt, 0, (x0 - step) as u64, &r2);
    must("the liquidation from the renewed pair", &liq(&renewed, at));
}

// ---------------------------------------------------------------- lead 2 / lead 3: the window, the judge's resources

/// Lead 3 (R12-F6): a contest uses up the opener's aged stake for the stake age. After a refuted false close, the
/// watcher's stake comes back from the pool as a new free stake (its age restarts); the attacker's next false close (one
/// round and the quiet time later) cannot be contested with it, only with another stake bonded 7 days before. A watcher
/// with k aged stakes stops k false closes every 7.5 days; the (k + 1)-th stands. On the gold pair the attacker's net is
/// 800 - 200 k KAS: positive against a counterparty (or a market) with at most 3 aged stakes per such period.
#[test]
fn r12_bounded_a_contest_uses_up_an_aged_stake_for_the_stake_age() {
    let m = gold_market();
    let quiet = m.p.quiet_daa as u64;
    let pland: i64 = 30_000_000;
    let p = gold_pair(&m, expiry_after(&m, pland, 2_000_000), 0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x75, (pland - 1_000_000) as u64);
    let pf = lowest_close(&p, L) + 500_000_000_000;
    let (t, w) = self_close_tx(&lp, L, p.size, pf, pland, pland as u64, &long_kp());
    must("false close 1", &t);
    let s1 = gstake(&m, 70, 100 * KAS as u64, 0, 0xa5);
    let (s, ft, r_daa, pool, receipt) = contest_and_resolve(&live(&t, 0, pland as u64, &w), &s1, 70, pland as u64 + 1);
    let Settlement::Restore { part, .. } = s else { panic!() };
    let (stx, _, _) = settle_tx(&pool, &[receipt], r_daa + 10);
    must("the pool pays the watcher", &stx);
    let back = live(&stx, 1, r_daa + 10, &Js::free(x(&keypair(70)), m.judge_market()));
    let restored = live(&ft, 1, r_daa, &part);
    let at = r_daa + 10 + quiet;
    let (t2, w2) = self_close_tx(&Live { daa: r_daa + 10, ..restored }, L, p.size, pf, at as i64, at, &long_kp());
    must("false close 2", &t2);
    let lw2 = live(&t2, 0, at, &w2);
    refused("a contest with the stake that came back from the pool", &contest_tx(&lw2, &back, 70, at + 1).0, 1);
    let s2 = gstake(&m, 70, 100 * KAS as u64, 0, 0xa6);
    must("a contest with a second stake bonded 7 days before", &contest_tx(&lw2, &s2, 70, at + 1).0);
    let period = m.judge.age + m.judge.round;
    println!("a stake is out for {} DAA ({:.1} days) per contest; the attacker can repeat every {} DAA ({:.2} h): {} attempts in a period", period, period as f64 / 864_000.0, at - pland as u64, (at - pland as u64) as f64 / 36_000.0, period / (at as i64 - pland));
}

/// Lead 3 (R12-F7, Low): a late contest delays a true close by a judge round for its fees. At P + W the finalize and a
/// contest are both valid on the pending part; if the contest lands, the verdict is ignored (upheld), but the close
/// settles only after the dispute resolves (12 h), and the late contester's NO receipt wins its own pool: it loses
/// nothing. The closer's payout is fixed (at its close price), so the delay costs it only time.
#[test]
fn r12_bounded_a_late_contest_delays_a_true_close_for_its_fees() {
    let m = gold_market();
    let w_ = m.p.challenge_daa as u64;
    let pland: i64 = 30_000_000;
    let p = gold_pair(&m, expiry_after(&m, pland, 2_000_000), 0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x76, (pland - 1_000_000) as u64);
    let (t, w) = self_close_tx(&lp, L, p.size, E, pland, pland as u64, &long_kp());
    must("the long's true close", &t);
    let lw = live(&t, 0, pland as u64, &w);
    must("its finalize at P + W", &finalize_tx(&lw, pland, w_).0);
    let st = gstake(&m, 42, 100 * KAS as u64, 0, 0xa7);
    let (s, _, r_daa, pool, receipt) = contest_and_resolve(&lw, &st, 42, pland as u64 + w_);
    assert!(matches!(s, Settlement::Close { .. }), "late: upheld whatever the verdict");
    let (stx, _, pays) = settle_tx(&pool, &[receipt], r_daa + 10);
    must("the late contester's pool", &stx);
    assert!(pays[0] >= st.value);
    println!("a late contest delays the close by {} DAA (12 h) and returns the contester's stake ({} KAS)", r_daa - pland as u64 - w_, kas(pays[0] as i64));
}

/// Lead 2 (R12-F8, Low): a void liquidation is free. The underwriter proposes from a UTXO that can carry a valid claim
/// and lands it one DAA after the last valid landing (it holds it; or a miner delays it): void, the part frozen W and
/// restored, the bond back to the underwriter, and both parties' listings cleared (every restore is unlisted). The
/// restored UTXO cannot carry another liquidation until the renewal, so it happens once per term.
#[test]
fn r12_bounded_a_void_liquidation_freezes_and_unlists_the_pair_for_free_once_a_term() {
    let m = gold_market();
    let w_ = m.p.challenge_daa as u64;
    let x0 = 10 * m.bond.step;
    let mut p = gold_pair(&m, x0, 0);
    p.long_ask = 8_100_000_000_000;
    p.short_bid = 7_900_000_000_000;
    let last = OPending::last_valid_landing(&m, x0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x77, (last - 10) as u64);
    let spec = ClaimSpec { kind: LIQ, side: L, n: p.size, p: 7_500_000_000_000, t0: last - 10, lock: (last - 10) as u64 };
    let lw = proposed(&lp, &spec, (last + 1) as u64);
    let (ft, s) = finalize_tx(&lw, last + 1, w_);
    must("finalize the void liquidation", &ft);
    let Settlement::Restore { part, proposer, defender, .. } = s else { panic!("{s:?}") };
    assert_eq!((proposer, defender), (lw.s.bond, 0));
    assert_eq!((part.long_ask, part.short_bid), (0, 0), "both listings cleared");
    let restored = live(&ft, 1, (last + 1) as u64 + w_, &part);
    refused("a second liquidation from the restored part", &propose_tx(&restored, &ClaimSpec { t0: last + 2_000, lock: (last + 2_000) as u64, ..spec }, &uw_kp(), None).0, 0);
}

// ---------------------------------------------------------------- lead 1 / lead 5: rules that hold

/// Lead 1: the maintenance rule of a party's own close at its edges, on a whole pair, a split part and a cured part: the
/// lowest accepted long price and the highest accepted short price are accepted, one sompi beyond is refused by the
/// script; at the lowest accepted price the long is paid at a close price at which it is not below maintenance, so no
/// true liquidation of that side coexists with a true close (the lowest price true at the feed f = c triggers nothing).
/// A close signed by the other party is refused.
#[test]
fn r12_holds_the_close_maintenance_rule_at_its_edges() {
    let m = gold_market();
    let pland: i64 = 30_000_000;
    let p = gold_pair(&m, expiry_after(&m, pland, 1_000_000), 0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x78, (pland - 1_000_000) as u64);
    let g = m.p.g_bps;
    for n in [p.size, p.size / 2] {
        let lo = lowest_close(&p, L);
        must(&format!("the long's close of {n} at the lowest accepted price"), &close_seq(&lp, L, n, lo, pland, pland as u64, &long_kp(), 600).0);
        let mut bad = close_seq(&lp, L, n, lo, pland, pland as u64, &long_kp(), 600);
        // The same close one sompi lower: build it against the model's refusal.
        bad = (self_close_tx(&lp, L, n, lo - 1, pland, pland as u64, &long_kp()).0, bad.1);
        refused("one sompi lower", &bad.0, 0);
        let hi = highest_short_close(&p);
        must("the short's close at the highest accepted price", &close_seq(&lp, S, n, hi, pland, pland as u64, &short_kp(), 600).0);
        refused("one sompi higher", &self_close_tx(&lp, S, n, hi + 1, pland, pland as u64, &short_kp()).0, 0);
        // A true close at lo needs the feed f >= lo (1 - g) >= c; the lowest price true there does not liquidate.
        let c = (lo as i128 * (10_000 - g as i128) / 10_000) as i64;
        let lowest_true = ((c as i128 * 10_000 + 10_000 + g as i128 - 1) / (10_000 + g as i128)) as i64;
        assert!(!p.long_liquidatable(lowest_true) && !p.long_liquidatable(c));
        refused("a close of the long signed by the short", &self_close_tx(&lp, L, n, E, pland, pland as u64, &short_kp()).0, 0);
    }
    // After a cure: the long's bound is lower, so is its lowest close.
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: p.size, p: 7_500_000_000_000, t0: pland, lock: pland as u64 }, pland as u64);
    let (ct, cured) = cure_tx(&lw, &long_kp());
    must("cure", &ct);
    let lc = live(&ct, 1, pland as u64 + 1, &cured);
    let lo = lowest_close(&cured, L);
    assert!(lo < lowest_close(&p, L));
    must("the cured long's close at its lowest price", &close_seq(&Live { daa: pland as u64 - 600, ..lc.clone() }, L, cured.size, lo, pland, pland as u64, &long_kp(), 600).0);
    refused("one sompi lower", &self_close_tx(&Live { daa: pland as u64 - 600, ..lc }, L, cured.size, lo - 1, pland, pland as u64, &long_kp()).0, 0);
}

/// Lead 5 (R7 in spirit, with the new close): two pairs of one state, both closed by their (common) long into ONE pending
/// output. The pending part names the outpoint of the pair input that proposed it (its maker): the second pair's input
/// computes another script and is refused, so no builder keeps a second pair's value.
#[test]
fn r12_holds_two_pairs_never_close_into_one_pending_part() {
    let m = gold_market();
    let pland: i64 = 30_000_000;
    let p = gold_pair(&m, expiry_after(&m, pland, 1_000_000), 0);
    let (a, b) = (genesis_pair(p.clone(), p.floor() as u64, 0x79, 1_000), genesis_pair(p.clone(), p.floor() as u64, 0x7a, 1_000));
    let k = kit();
    let (w, fb) = p.pending_of(k, p.size, CLOSE, L, E, pland).unwrap();
    let wa = w.made_by(&a.op);
    let args = || vec![iv(p.size), iv(CLOSE), iv(L), iv(E), iv(pland), iv(1), Arg::Sig(long_kp()), iv(fb), iv(0), bv(&k.pending.prefix), bv(&k.pending.suffix)];
    let ins = vec![pair_in(&a, "propose", args()), pair_in(&b, "propose", args()), key_in(uop(0x7b, 0), 10_000 * KAS as u64, 0, &long_kp())];
    let mut outs = vec![out(p.pending_value(a.value, &wa) as u64, pend_spk(&wa), None), out(9_000 * KAS as u64, p2pk_spk(&long_kp()), None)];
    place(&mut outs, 0, 0, a.op, a.id.unwrap(), Lineage::Fresh);
    let t = build_ext(&ins, outs, pland as u64, &[600, 600, 0]);
    let v = verdicts(&t);
    assert!(v[0] && !v[1], "the first pair accepts its own pending part, the second refuses it: {v:?}");
}

/// Lead 5 (R10, R11-F7 in spirit): a partial close is a split, and keeps the part floor: on the 1-unit pair at 1 KAS a
/// unit, 10x (each side's 0.1 KAS), a close of 0.19 unit or of half is refused; on the gold pair a close of 0.01 oz is
/// accepted (80 KAS of each side in the smaller piece).
#[test]
fn r12_holds_a_partial_close_keeps_the_part_floor() {
    let m = gold_market();
    let sc = m.p.scale;
    let pland: i64 = 30_000_000;
    let small = OPair { size: sc, bl: 90_000_000, bs: 110_000_000, entry: 100_000_000, b: OBinding { cover_rate: 20_000_000, ..gold_pair(&m, 0, 0).b }, ..gold_pair(&m, expiry_after(&m, pland, 1_000_000), 0) };
    let lp = genesis_pair(small.clone(), small.floor() as u64, 0x7c, 1_000);
    for n in [19_000_000i64, 50_000_000] {
        assert!(!small.split_ok(n));
        refused(&format!("a close of {n} base units"), &self_close_tx(&lp, L, n, 100_000_000, pland, pland as u64, &long_kp()).0, 0);
    }
    let g = gold_pair(&m, expiry_after(&m, pland, 1_000_000), 0);
    let lg = genesis_pair(g.clone(), g.floor() as u64, 0x7d, 1_000);
    must("a close of 0.01 oz of the gold pair", &self_close_tx(&lg, L, 1_000_000, E, pland, pland as u64, &long_kp()).0);
}

/// Lead 5 (R6 in spirit): the verdict of a dispute of another covenant id cannot decide a contested close.
#[test]
fn r12_holds_a_verdict_of_another_dispute_cannot_decide_a_close() {
    let m = gold_market();
    let pland: i64 = 30_000_000;
    let p = gold_pair(&m, expiry_after(&m, pland, 2_000_000), 0);
    let lp = genesis_pair(p.clone(), p.floor() as u64, 0x7e, 1_000);
    let (t, w) = self_close_tx(&lp, L, p.size, E + 1_000_000_000_000, pland, pland as u64, &long_kp());
    must("a false close", &t);
    let lw = live(&t, 0, pland as u64, &w);
    let st = gstake(&m, 70, 100 * KAS as u64, 0, 0xa8);
    let (ct, w2, _, dj) = contest_tx(&lw, &st, 70, pland as u64 + 1);
    must("contest", &ct);
    let (cw, ld) = (live(&ct, 0, pland as u64 + 1, &w2), live(&ct, 2, pland as u64 + 1, &dj));
    let mut fake = ld.clone();
    fake.id = Some(Hash::from_bytes([0xdd; 32]));
    fake.op = outpoint(0xdd);
    let r = pland as u64 + 1 + m.judge.round as u64;
    not_accepted("a verdict from a dispute of another id", &decide_tx(&fake, &cw, r).0);
    must("its own dispute's verdict", &decide_tx(&ld, &cw, r).0);
}

// ---------------------------------------------------------------- lead 6: the honest life, with the tool's builders

mod life {
    use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
    use kpf_protocol::bond::Bond;
    use kpf_protocol::judge::{Js, NO, YES};
    use kpf_protocol::oracle::*;
    use kpf_protocol::oracle_build::*;
    use kpf_protocol::state::{LONG, SHORT};
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
            let mut b = [0x7du8; 32];
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

    /// Two pairs of 0.1 oz of the gold pair: A, whose underwriter disappears right after the open (it never renews,
    /// vouches or answers): its long closes half of its side at the feed before the expiry, its short the rest after
    /// the expiry; B, whose long closes at the feed and whose short contests that true close with its own aged stake;
    /// the long defends it with a YES deposit of its own aged stake; upheld, settled; the pool pays the long the short's
    /// stake less the burn. Returns the fees and the payouts checked.
    pub fn run() -> Vec<(String, u64)> {
        let (lk, sk, uk, ek) = (k(41), k(42), k(31), k(60));
        let mut c = Chain::new(&[41, 42, 31, 60]);
        let r = MIN_FEE_RATE;
        let m = super::gold_market();
        let jm = m.judge_market();
        let (step, w_, q_, round) = (m.bond.step as u64, m.p.challenge_daa as u64, m.p.quiet_daa as u64, m.judge.round as u64);
        let sc = m.p.scale;
        let e = super::E;
        let f = c.mint(lk, 201 * KAS);
        let sg = c.sign("the long bonds a 200 KAS judge stake", create_stake(&m, lk, 200 * KAS, &[f], &lk, r));
        let lstake = nth(&sg, 1, Js::free(lk, jm), 0);
        let f = c.mint(sk, 101 * KAS);
        let sg = c.sign("the short bonds a 100 KAS judge stake", create_stake(&m, sk, 100 * KAS, &[f], &sk, r));
        let sstake = nth(&sg, 1, Js::free(sk, jm), 0);
        let f = c.mint(uk, 10_001 * KAS);
        let sg = c.sign("the underwriter bonds a 10,000 KAS slot", create_slot(&m, uk, 10_000 * KAS, 0, &[f], &uk, r));
        let mut slot = nth(&sg, 1, Bond::new(uk, m.bond_market(), 0), 0);
        let base = |side: i64, owner: [u8; 32]| OOpen {
            m,
            side,
            owner,
            size_left: 2 * sc / 10,
            limit: e,
            my_rate: 800_000_000_000,
            min_counter_rate: 0,
            min_fill: sc / 10,
            tip: 0,
            active_from: 0,
            expiry_daa: 1_000_000_000,
            uw_accept: slot.idb(),
            prem_max: 0,
        };
        let (lo, so) = (base(LONG, lk), base(SHORT, sk));
        let esc = |o: &OOpen| o.fill_spend(o.size_left, o.prem_max) as u64 + KAS;
        let f = c.mint(lk, esc(&lo) + KAS);
        let sg = c.sign("long order, 0.2 oz at 80,000, 10x", create_order(&lo, esc(&lo), &[f], &lk, r));
        let mut lorder = nth(&sg, 1, lo.clone(), 1_000);
        let f = c.mint(sk, esc(&so) + KAS);
        let sg = c.sign("short order, 0.2 oz at 80,000, 10x", create_order(&so, esc(&so), &[f], &sk, r));
        let mut sorder = nth(&sg, 1, so.clone(), 1_000);
        let x_ = 9 * step as i64;
        let mut pairs = vec![];
        for i in 0..2u64 {
            let lock = 2_000_000 + 700 * i;
            let terms = OpenTerms { n: sc / 10, x: e, cover_rate: 2_000_000_000_000, premium: 0, expiry: x_, lock };
            let f = c.mint(ek, 10 * KAS);
            let (b, _) = open(&lorder, &sorder, &slot, &terms, &[f], &ek, r).unwrap_or_else(|er| panic!("fill {i}: {er}"));
            let sg = c.sign(&format!("open, fill {} of 2 (0.1 oz)", i + 1), Ok(b));
            let OObj::Pair(p) = made(&sg, 2) else { panic!() };
            pairs.push(nth(&sg, 2, p, lock));
            let OObj::Slot(b) = made(&sg, 3) else { panic!() };
            slot = nth(&sg, 3, b, lock);
            if i == 0 {
                let OObj::Order(o) = made(&sg, 0) else { panic!() };
                lorder = nth(&sg, 0, o, lock);
                let OObj::Order(o) = made(&sg, 1) else { panic!() };
                sorder = nth(&sg, 1, o, lock);
            }
        }
        let (a, b) = (pairs[0].clone(), pairs[1].clone());
        // A: the underwriter is gone. The long closes half of its side at the feed before the expiry.
        let t1 = 10_000_000u64;
        let half = a.s.size / 2;
        let f = c.mint(lk, 300 * KAS);
        let (bb, w) = propose_close(&a, LONG, half, e, t1 as i64, &[f], &lk, r).unwrap();
        let sg = c.sign("A: the long closes half its side at the feed (underwriter gone)", Ok(bb));
        let pend = nth(&sg, 0, w, t1);
        let rest = nth(&sg, 1, a.s.rest_of(a.s.size - half), t1);
        let f = c.mint(ek, 10 * KAS);
        let (bb, s) = finalize(&OLive { daa: t1, ..pend.clone() }, &[f], &ek, r).unwrap();
        let Settlement::Close { long, short } = s else { panic!() };
        assert_eq!(long - pend.s.refund, kpf_protocol::payoff::q(half, pend.s.close_price() - a.s.bl, sc, 0));
        assert_eq!(short, kpf_protocol::payoff::q(half, a.s.bs - pend.s.close_price(), sc, 0));
        c.sign("A: finalize the long's close, W later", Ok(bb));
        // The short closes the rest after the expiry (the cover has lapsed; the close is valid at any time).
        let t2 = x_ as u64 + 1_000;
        let f = c.mint(sk, 300 * KAS);
        let (bb, w) = propose_close(&OLive { daa: t1, ..rest }, SHORT, a.s.size - half, e, t2 as i64, &[f], &sk, r).unwrap();
        let sg = c.sign("A: the short closes the rest at the feed after the expiry", Ok(bb));
        let pend = nth(&sg, 0, w, t2);
        let f = c.mint(ek, 10 * KAS);
        let (bb, s) = finalize(&pend, &[f], &ek, r).unwrap();
        assert!(matches!(s, Settlement::Close { .. }));
        c.sign("A: finalize the short's close", Ok(bb));
        // B: the long closes at the feed; the short contests the true close; the long defends it.
        let t3 = 12_000_000u64;
        let f = c.mint(lk, 300 * KAS);
        let (bb, w) = propose_close(&b, LONG, b.s.size, e, t3 as i64, &[f], &lk, r).unwrap();
        let sg = c.sign("B: the long closes at the feed", Ok(bb));
        let pend = nth(&sg, 0, w, t3);
        let f = c.mint(ek, 10 * KAS);
        let (bb, cw, d) = contest(&pend, &sstake, 50_000_000, &[f], &ek, r, &Trust::Untraced).unwrap();
        let sg = c.sign("B: the short contests the true close (false)", Ok(bb));
        let (cpart, dispute) = (nth(&sg, 0, cw, t3 + 1), nth(&sg, 2, d, t3 + 1));
        let r_short = nth(&sg, 1, Js::receipt(sk, jm, NO, sstake.value as i64, 0, t3 as i64), t3 + 1);
        let f = c.mint(ek, 10 * KAS);
        let (bb, d2) = deposit(&dispute, YES, &[lstake.clone()], &[f], &ek, r, &Trust::Untraced).unwrap();
        let sg = c.sign("B: the long defends its close (YES, 200 KAS)", Ok(bb));
        assert_eq!(d2.side, YES);
        let dispute = nth(&sg, 0, d2, t3 + 2);
        let r_long = nth(&sg, 1, Js::receipt(lk, jm, YES, lstake.value as i64, 0, t3 as i64), t3 + 2);
        let f = c.mint(ek, 10 * KAS);
        let (bb, dw, pool) = decide(&dispute, &cpart, &[f], &ek, r).unwrap();
        assert_eq!(dw.status, UPHELD);
        let at = t3 + 2 + round;
        let sg = c.sign("B: resolve, upheld", Ok(bb));
        let (decided, pool) = (nth(&sg, 1, dw, at), nth(&sg, 0, pool, at));
        let f = c.mint(ek, 10 * KAS);
        let (bb, s) = finalize(&decided, &[f], &ek, r).unwrap();
        let Settlement::Close { long, .. } = s else { panic!("{s:?}") };
        assert_eq!(long - decided.s.refund, kpf_protocol::payoff::q(b.s.size, decided.s.close_price() - b.s.bl, sc, 0));
        c.sign("B: finalize the upheld close", Ok(bb));
        let f = c.mint(ek, 10 * KAS);
        let (bb, p2) = pool_settle(&pool, &[r_short, r_long], &[f], &ek, r).unwrap();
        let sg = c.sign("B: settle the pool (the short's stake forfeited)", Ok(bb));
        let won = sg.tx.outputs[2].value;
        assert!(won > lstake.value + 80 * KAS, "the long wins the short's stake less the burn: {won}");
        let _ = c.sign("B: close the pool", pool_close(&nth(&sg, 0, p2, at + 10), &ek, r));
        let _ = (w_, q_);
        c.fees
    }
}

/// Lead 6: the honest life of review 11 with the new defaults and the new exits, every transaction built by the tool's
/// builders on the pinned artifacts, signed and passed through `verify::verify`: both parties of one pair exit by their
/// own closes while its underwriter has disappeared after the open (half before the expiry, the rest after it); on the
/// other pair the long's true close is contested falsely by the short, the long defends it with its own aged stake, the
/// claim is upheld and settled, and the pool pays the long. Every fee is printed against the documented one (29.5).
#[test]
fn r12_holds_the_honest_life_with_own_closes_a_defended_close_and_a_vanished_underwriter() {
    let fees = life::run();
    let doc = [("closes", 4_100_000u64), ("open", 9_940_000), ("finalize", 970_000), ("contest", 2_970_000), ("defends", 4_000_000), ("resolve", 2_930_000), ("settle the pool", 5_940_000), ("close the pool", 2_020_000)];
    for (name, fee) in &fees {
        let d = doc.iter().find(|(k, _)| name.contains(k)).map(|(_, v)| *v);
        println!("{name}: {} KAS{}", kas(*fee as i64), d.map(|v| format!(" (documented {})", kas(v as i64))).unwrap_or_default());
        if let Some(v) = d {
            assert!(*fee <= 2 * v, "{name}: {fee} sompi, more than twice the documented {v}");
        }
    }
}
