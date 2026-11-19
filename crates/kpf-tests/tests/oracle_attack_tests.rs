//! Attacks on the oracle-market integration (docs/DESIGN-ORACLE-IMPL.md, third slice, "Attacks"), in the manner of the
//! reviews: every way an underwriter can take from the parties it underwrites, every way the parties can take from the
//! underwriter, a stuck pair, bond, stake or dispute, twins and look-alikes across the three templates, a hostile
//! market block; and the shared texts that keep the families from drifting. Each test is `holds_` (the attack fails, or
//! costs the attacker what is stated) or `exploit_` (it works, with its bound). Worked numbers use the fixture pair:
//! 1,000 units at 10 KAS, long margin 1,000 KAS (bl 9), short margin 2,000 KAS (bs 12), cover rate 6 KAS (cover 6,000
//! KAS), bond 10 % of the cover (600 KAS), the disputer 20 % of a refuted bond; `--nocapture` prints them.

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use common::*;
use kaspa_consensus_core::tx::TransactionOutput;
use kaspa_consensus_core::Hash;
use kpf_protocol::bond::{BondParams, RING};
use kpf_protocol::judge::{UNIT, YES};
#[allow(unused_imports)]
use kpf_protocol::state::{KIND_CLOSE, LONG};
use kpf_protocol::oracle::*;
use oraclekit::*;

const KAS: i64 = 100_000_000;
const FALSE_P: i64 = 920_000_000;

fn liq(side: i64, p: i64, t0: i64) -> ClaimSpec {
    ClaimSpec { kind: LIQ, side, n: 1_000_000_000, p, t0, lock: t0 as u64 }
}

fn contest_at(lw: &LPend, seed: u8, v: u64, daa: u64) -> (LPend, LStake, LStake) {
    let st = stake(seed, v, 0, 0x90 + seed);
    let (t, w2, r, d) = contest_tx(lw, &st, seed, daa);
    must("contest", &t);
    (live(&t, 0, daa, &w2), live(&t, 1, daa, &r), live(&t, 2, daa, &d))
}

fn src(rel: &str) -> String {
    contract_source(rel)
}

fn between(s: &str, a: &str, b: &str) -> String {
    let i = s.find(a).unwrap_or_else(|| panic!("{a}"));
    let j = s[i..].find(b).unwrap_or_else(|| panic!("{b}")) + i;
    s[i..j].to_string()
}

fn constant(s: &str, name: &str) -> Option<String> {
    let key = format!("int constant {name} = ");
    s.lines().find_map(|l| l.trim().strip_prefix(&key).map(|v| v.trim_end_matches(';').to_string()))
}

/// Opens `count` pairs on one slot of value `k` in the market `m`, one after the other (each bind spends the previous
/// slot continuation); returns the pairs, the last slot and how many opens were accepted.
fn open_many(m: &OMarket, k: u64, count: usize) -> (Vec<LPair>, LSlot, usize) {
    let mut sl = slot(k);
    sl.s.m = m.bond_market();
    let mut pairs = vec![];
    let mut ok = 0;
    for i in 0..count {
        let mut lo = order(L, &long_kp(), 0x71 + 2 * i as u8, 0);
        let mut so = order(S, &short_kp(), 0x72 + 2 * i as u8, 0);
        lo.s.m = *m;
        so.s.m = *m;
        let (t, p, b2) = open_tx(&lo, &so, &sl, &open_spec(), &|_, _| {});
        if accepted(&t) {
            ok += 1;
            pairs.push(live(&t, 2, OPEN_DAA, &p));
            sl = live(&t, 3, OPEN_DAA, &b2);
        }
    }
    (pairs, sl, ok)
}

// ---------------------------------------------------------------- one text

/// The oracle market check, the bond bounds and the judge bounds are one text in the three templates; the bond bounds
/// are KpfBond's (the slot's unbonding delay renamed), the judge bounds KpfStake's, the bind proof one text in the order
/// and the pair; every line the oracle check shares with the book market's `KpfOpen.checkMarket` is the same line there,
/// with the same constants. So no family can relax or tighten a shared bound alone.
#[test]
fn holds_the_market_bond_judge_bounds_and_the_bind_proof_are_one_text() {
    let (o, p, w) = (src("KpfOpenO.sil"), src("KpfPairO.sil"), src("KpfPendingO.sil"));
    let (bond, stake, kopen) = (src("KpfBond.sil"), src("KpfStake.sil"), src("KpfOpen.sil"));
    let m = |s: &str| between(s, "// BEGIN oracle market bounds", "// END oracle market bounds");
    assert_eq!(m(&o), m(&p));
    assert_eq!(m(&o), m(&w));
    let b = |s: &str| between(s, "// BEGIN oracle bond bounds", "// END oracle bond bounds");
    assert_eq!(b(&o), b(&p));
    assert_eq!(b(&o), b(&w));
    let kb = between(&bond, "// BEGIN bond bounds", "// END bond bounds").replace("unbondDaa", "slotUnbondDaa").replace("// BEGIN bond bounds", "// BEGIN oracle bond bounds");
    assert_eq!(b(&o), kb, "the slot's bounds, byte for byte but the renamed field");
    let j = |s: &str| between(s, "// BEGIN judge bounds", "// END judge bounds");
    assert_eq!(j(&o), j(&stake));
    assert_eq!(j(&p), j(&stake));
    assert_eq!(j(&w), j(&stake));
    let bp = |s: &str| between(s, "// BEGIN oracle bind proof", "// END oracle bind proof");
    assert_eq!(bp(&o), bp(&p));
    // Review 6: the fresh-id check is one text in the order and the pair, with the same output bound, and it runs at
    // every entry that starts a lineage (the order's fill and take; the pair's transfer and closeHost through
    // partAndRest, closeDonor and propose).
    let fr = |s: &str| between(s, "// BEGIN fresh id", "// END fresh id");
    assert_eq!(fr(&o), fr(&p));
    assert_eq!(constant(&o, "MAX_OUTPUTS"), constant(&p, "MAX_OUTPUTS"));
    assert_eq!(o.matches("fresh(").count(), 3, "the definition, fill and take");
    assert_eq!(p.matches("fresh(").count(), 4, "the definition, partAndRest, closeDonor, propose");
    // Every check of the book market on a parameter both families carry is the same line in both.
    let book = between(&kopen, "// BEGIN market bounds", "// END market bounds");
    let shared = [
        "require(scale > 0);",
        "require(scale <= SCALE_MAX);",
        "require(priceMax > 0);",
        "require(priceMax <= PRICE_CAP);",
        "require(carrier >= 0);",
        "require(minSize > 0);",
        "require(idleDaa >= 0);",
        "require(idleDaa < CSV_LIMIT);",
        "require(mmrBps >= 0);",
        "require(mmrBps < MMR_CAP);",
        "require(liqFeeBps >= 0);",
        "require(liqFeeBps <= BPS);",
        "require(keeperFeeMin >= 0);",
        "require(bondMin >= 0);",
        "require(bondMin <= BOND_MIN_MAX);",
        "require(gBps > 0);",
        "require(gBps < BPS);",
    ];
    for l in shared {
        assert!(book.contains(l), "book: {l}");
        assert!(m(&o).contains(l), "oracle: {l}");
    }
    for n in ["SCALE_MAX", "PRICE_CAP", "MMR_CAP", "BOND_MIN_MAX", "BPS", "CSV_LIMIT"] {
        let v = constant(&kopen, n).unwrap();
        for s in [&o, &p, &w] {
            assert_eq!(constant(s, n).as_deref(), Some(v.as_str()), "{n}");
        }
    }
    for n in ["RING", "RHO_MAX_BPS", "BPS", "CSV_LIMIT"] {
        let v = constant(&bond, n).unwrap();
        for s in [&o, &p, &w] {
            assert_eq!(constant(s, n).as_deref(), Some(v.as_str()), "{n}");
        }
    }
    for n in ["MU_MIN_BPS", "MU_MAX_BPS", "BURN_MAX_BPS", "MIN_STAKE_MAX", "ROUND_MAX", "AGE_MAX", "UNBOND_MAX", "UNIT"] {
        let v = constant(&stake, n).unwrap();
        for s in [&o, &p, &w] {
            assert_eq!(constant(s, n).as_deref(), Some(v.as_str()), "{n}");
        }
    }
    // Every entry that relies on the block runs the check: both of the order's opening entries, the pair's propose and
    // renew, the pending part's contest.
    assert_eq!(o.matches("checkMarket();").count(), 1, "inside checkOrder");
    assert_eq!(o.matches("        checkOrder();").count(), 2, "fill and take");
    assert_eq!(p.matches("        checkMarket();").count(), 2, "renew and propose");
    assert_eq!(w.matches("        checkMarket();").count(), 1, "contest");
    // The library's bounds are the same numbers (the edges of OMarket::check).
    let mk = omarket();
    assert!(mk.check().is_ok());
    for bad in [
        OMarket { p: OParams { challenge_daa: 0, ..mk.p }, ..mk },
        OMarket { p: OParams { challenge_daa: mk.bond.term, ..mk.p }, ..mk },
        OMarket { p: OParams { g_bps: 0, ..mk.p }, ..mk },
        OMarket { p: OParams { beta_bps: 10_001, ..mk.p }, ..mk },
        OMarket { bond: BondParams { rho_bps: 50_001, ..mk.bond }, ..mk },
        OMarket { judge: kpf_protocol::judge::JudgeParams { mu_bps: 10_999, ..mk.judge }, ..mk },
    ] {
        assert!(bad.check().is_err());
    }
}

/// The templates these engine tests compile from source are the pinned ones the library and the command line build
/// with (`contracts/artifacts/oracle/`), constants included.
#[test]
fn holds_the_engine_templates_are_the_pinned_artifacts() {
    let (a, b) = (kit(), kpf_protocol::oracle_artifacts::kit());
    for (x, y) in [(&a.open, &b.open), (&a.pair, &b.pair), (&a.pending, &b.pending), (&a.bond, &b.bond), (&a.stake, &b.stake)] {
        assert_eq!((x.hash, &x.prefix, &x.suffix, x.state_len), (y.hash, &y.prefix, &y.suffix, y.state_len), "{}", x.name);
    }
    assert_eq!(a.consts, b.consts);
}

// ---------------------------------------------------------------- the underwriter against its parties

/// A false claim nobody contests within W settles: the loser's side goes to the winner (forfeit and keep). The bound
/// is the slot: an underwriter of bond K can have at most rho x K of cover counted, so it can falsely settle at most
/// rho x K of its own pairs' value, summed over its slots. At rho = 1 a slot of 6,000 KAS underwrites one pair (cover
/// 6,000); at rho = 5 five, and a sixth open is refused.
#[test]
fn exploit_an_undisputed_false_claim_takes_the_losers_side_bounded_by_rho_times_the_bond() {
    let cover = 600_000_000_000u64;
    for (rho, n_pairs) in [(10_000i64, 1usize), (50_000, 5)] {
        let m = omarket_with(BondParams { rho_bps: rho, ..omarket().bond });
        let (pairs, _, ok) = open_many(&m, cover, n_pairs + 1);
        assert_eq!(ok, n_pairs, "rho {}: {} pairs fit on a 6,000 KAS slot, the next open is refused", rho / 10_000, n_pairs);
        let mut taken = 0i64;
        let mut fees = 0i64;
        for lp in &pairs {
            let lw = proposed(lp, &liq(L, FALSE_P, 450), 500);
            let (t, s) = finalize_tx(&lw, 500, 200);
            must("an undisputed false liquidation", &t);
            let Settlement::Forfeit { value, uw, .. } = s else { panic!() };
            // At the true price of 10 KAS the long's side was worth q(1,000, 10 - 9) = 1,000 KAS; it gets nothing.
            taken += lp.s.long_at(1_000_000_000);
            fees += uw - lw.s.bond;
            assert_eq!(value, lw.value as i64 - lw.s.bond - lw.s.fee);
        }
        println!(
            "rho {}: bond 6,000 KAS, {} pairs, undisputed false liquidations take {} KAS from the longs (the shorts gain it, the underwriter's fees {} KAS); disputed in time each would cost the underwriter its 600 KAS bond ({} KAS in all) and restore every part",
            rho / 10_000,
            pairs.len(),
            taken / KAS,
            fees as f64 / KAS as f64,
            600 * pairs.len()
        );
        assert_eq!(taken, 1_000 * KAS * n_pairs as i64);
    }
}

/// The same false claim contested within W costs the underwriter its bond and costs the parties nothing: the part is
/// restored, the long (the defender) is paid 480 KAS, the disputer 120 KAS. The loss to the parties does not depend on
/// rho; what rho changes is how many such claims one bond can carry (one at 1, five at 5).
#[test]
fn holds_a_false_claim_disputed_in_time_costs_its_underwriter_the_bond() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq(L, FALSE_P, 450), 500);
    let (cw, _, d) = contest_at(&lw, 70, 10 * UNIT as u64, 699);
    let (t, _, dw) = decide_tx(&d, &cw, 799);
    must("decide", &t);
    let dw = live(&t, 1, 799, &dw);
    let (t, s) = finalize_tx(&dw, 500, 0);
    must("restore", &t);
    let Settlement::Restore { part, value, defender, disputer, .. } = s else { panic!() };
    assert_eq!((part.bl, part.bs, part.long_key, part.short_key), (lp.s.bl, lp.s.bs, lp.s.long_key, lp.s.short_key));
    assert_eq!(value, lp.value as i64);
    assert_eq!((defender, disputer), (480 * KAS, 120 * KAS));
}

/// A claim wrong by less than the tolerance g neither liquidates a healthy pair nor pays a closer more than fair, when g
/// exceeds the feed's own disagreement tolerance (the judge upholds any claim within it). At g = 2 % and a feed tolerance
/// of 1 %, a long healthy at 9.48 (0.06 % above its trigger 9.4737) survives a claim 1 % low (9.3852); a closer paid at a
/// price 1 % high (10.10 for 10) is paid at 9.898 < 10.
#[test]
fn holds_a_slightly_wrong_claim_within_the_feeds_tolerance_takes_nothing() {
    let (lp, _, _) = opened();
    let p = &lp.s;
    let truth = 948_000_000;
    assert!(truth as i128 * 9_500 >= p.bl as i128 * 10_000, "healthy at the truth");
    let claim = truth * 99 / 100;
    assert!(!p.long_liquidatable(claim), "1 % low does not trigger");
    refused("1 % low", &propose_tx(&lp, &liq(L, claim, 450), &uw_kp(), None).0, 0);
    let close = OPending { kind: CLOSE, side: L, quote: 1_010_000_000, ..sample_pending(p) };
    assert!(close.close_price() < 1_000_000_000);
    assert_eq!(close.close_price(), 989_800_000);
}

/// With g no larger than the feed's tolerance the margin is second order: a claim off by exactly g (which the judge
/// upholds if the feed tolerates g) liquidates a long healthy within g^2 of its trigger: the trigger is reached at
/// p* < T / (1 - g^2), 0.04 % above T at g = 2 %. The loser's loss is its residual, at most its maintenance slice
/// (q(1,000, T - bl) = 473.68 KAS on the fixture pair). Only a wallet can compare g with the feed definition.
#[test]
fn exploit_with_g_at_the_feeds_tolerance_a_pair_within_g_squared_of_its_trigger_can_be_liquidated() {
    let (lp, _, _) = opened();
    let p = &lp.s;
    let t = p.trigger_of(L);
    assert_eq!(t, 947_368_421);
    let truth = 947_600_000; // healthy: 9.476 x 0.95 = 9.0022 >= 9
    assert!(truth as i128 * 9_500 >= p.bl as i128 * 10_000);
    let claim = truth * 98 / 100; // 2 % low, within a feed tolerance of 2 %
    assert!(p.long_liquidatable(claim));
    proposed(&lp, &liq(L, claim, 450), 500);
    let slice = q(p.size, t - p.bl, p.m.p.scale);
    println!("g = 2 % = the feed's tolerance: a long healthy at 9.476 (trigger 9.4737) is liquidated by a claim of {} (2 % low); the loser loses its residual, at most {} KAS", claim as f64 / 1e8, slice as f64 / 1e8);
    assert_eq!(slice, 47_368_421_000);
}

/// A silent underwriter (colluding with a losing long) never vouches the due liquidation and lets the cover lapse; the
/// pair then settles at its entry price. At a true price of 8.50 (below bl = 9: the long is bankrupt) the short is owed
/// the whole part, 3,000.2 KAS; lapse settle pays it 2,000.2 KAS and the long 1,000 KAS. The loss to the winner is the
/// loser's margin at entry, 1,000 KAS per pair, at rho = 1 and at rho = 5 alike (per pair); summed it is bounded by the
/// pairs the colluding underwriter holds (rho x its bond in cover).
#[test]
fn exploit_a_silent_underwriter_turns_a_due_liquidation_into_a_settlement_at_the_entry() {
    let (lp, _, _) = opened();
    let t = lapse_tx(&lp, lp.s.lapse_from(), 0);
    must("lapse settle", &t);
    let (l, s) = lp.s.lapse_out(lp.value);
    let fair_short = lp.value as i64 - lp.s.long_at(850_000_000);
    println!("silent underwriter: at 8.50 the short is owed {} KAS, lapse settle pays it {} KAS and the long {} KAS: the winner loses {} KAS", fair_short / KAS, s as f64 / 1e8, l / KAS, (fair_short - s) / KAS);
    assert_eq!(fair_short - s, 1_000 * KAS);
}

/// An underwriter renews a lapsed pair after the fact: the slot's window only bounds X from above (x <= t + term +
/// step), so a renewal at DAA 2,500 of a pair whose cover lapsed at 1,200 moves X to 2,000 (still in the past) and
/// charges a premium for a term in which no claim was valid. Before review 9 a second renewal to 2,800 followed at
/// once; since review 9 (R9-F2) the renewal's continuation, accepted at 2,500, after its new expiry 2,000, is never
/// renewed again: one premium (20 KAS on the fixture pair, 0.67 % of the range) for the lapsed term, and the lapse
/// settle is due at 3,200.
#[test]
fn exploit_a_back_dated_renewal_charges_one_premium_for_a_lapsed_term() {
    let (lp, sl, _) = opened();
    let (t, r, b2) = renew_tx(&lp, &sl, 2_500, 12);
    must("renewal to X = 2,000 at DAA 2,500", &t);
    assert_eq!(r.b.expiry, 2_000);
    assert_eq!(lp.value - t.0.outputs[0].value, 2_000_000_000, "one premium of 20 KAS");
    let lp2 = live(&t, 0, 2_500, &r);
    let sl2 = live(&t, 1, 2_500, &b2);
    refused("review 9: a renewal of a UTXO accepted after its expiry", &renew_tx(&lp2, &sl2, 2_500, 4).0, 0);
    assert_eq!(lp2.s.lapse_from(), 3_200);
}

/// Extortion for renewal: the premium is fixed at open; a renewal that takes one sompi more is refused, and nobody but
/// the pair's own slot can renew. What the underwriter can do is not renew (then the parties exit or the pair lapses and
/// settles at its entry) or renew without ever vouching: the renewals continue only while the premium leaves the range
/// wider than both maintenance bands, so a pair whose underwriter renews but never vouches is held at most that many
/// terms (for the fixture pair, range 3 KAS, premium 0.02 KAS a term, mmr 5 %: 97 terms, `(9 + 0.01k) x 1.05 <
/// (12 - 0.01k) x 0.95` while k <= 97) unless a party exits by transfer.
#[test]
fn exploit_an_underwriter_that_renews_but_never_vouches_holds_the_pair_for_a_bounded_number_of_terms() {
    let (lp, _, _) = opened();
    let mut p = lp.s.clone();
    let mut terms = 0;
    while let Some((r, _)) = p.renewed() {
        p = r;
        terms += 1;
    }
    println!("renewals until the range is no longer wider than both bands: {terms} terms; then the cover lapses and the pair settles at its entry");
    assert_eq!(terms, 97);
    assert!(!p.range_ok() || p.renewed().is_none());
}

/// Void claims and freezes (review 11): a void liquidation now returns its bond to its proposer (an honest claim delayed
/// past its last valid landing must not pay the loser), so a freeze is bounded by the proposal instead: `propose` refuses
/// a liquidation from a pair UTXO accepted too late for any claim on it to be valid, and the restored part of a void claim
/// is such a UTXO. One void claim per part per term: the pair (accepted at 300, X 1,200, last valid landing 900) is frozen
/// once by a claim landing at 950 (W, then restored at 1,150 with the bond back to the underwriter); the restored part,
/// accepted 50 DAA before the expiry, cannot carry a valid claim, so no second freeze of it is accepted.
#[test]
fn holds_a_freeze_by_void_claims_is_once_per_part_per_term() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq(L, FALSE_P, 900), 950);
    let (t, s) = finalize_tx(&lw, 950, 200);
    must("void", &t);
    let Settlement::Restore { part, defender, proposer, .. } = s else { panic!("void") };
    assert_eq!((defender, proposer), (0, 600 * KAS), "the bond back to the underwriter");
    let back = live(&t, 1, 1_150, &part);
    refused("a second freeze of the restored part", &propose_tx(&back, &liq(L, FALSE_P, 1_150), &uw_kp(), None).0, 0);
}

/// The underwriter cannot vouch a claim after the cover's liability ended: it is void, and it cannot reach the slot;
/// and the slot's capacity it freed can bind new cover only once the old cover's liability has ended.
#[test]
fn holds_cover_is_released_only_by_the_calendar() {
    let (lp, sl, _) = opened();
    let (lo, so) = (order(L, &long_kp(), 0x73, 0), order(S, &short_kp(), 0x74, 0));
    let cover = lp.s.cover_of(lp.s.size) as u64;
    let mut full = sl.clone();
    full.value = cover;
    // Advancing past X + tail drops the bucket; before, a second pair of the same cover is refused.
    let mut sp = open_spec();
    sp.lock = 1_399;
    sp.xe = 2_000;
    sp.advance = 12;
    let (t, _, _) = open_tx(&lo, &so, &full, &sp, &|_, _| {});
    refused("advance before X + tail", &t, 2);
    sp.lock = 1_400;
    let (lo2, so2) = (order(L, &long_kp(), 0x73, 1_200), order(S, &short_kp(), 0x74, 1_200));
    let (t, _, _) = open_tx(&lo2, &so2, &full, &sp, &|_, _| {});
    must("the capacity back after X + tail", &t);
}

// ---------------------------------------------------------------- the parties against the underwriter

/// A frivolous contest of a true claim costs the opener its stake (less the burn) and delays the settlement by the
/// dispute's rounds; the underwriter defends with aged stake and is paid. With a 10 KAS opener and a 20 KAS defence the
/// underwriter gains 9 KAS; the claim settles at the first DAA the second round allows (two rounds after the claim).
#[test]
fn holds_a_frivolous_contest_costs_the_opener_its_stake() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq(L, 900_000_000, 450), 500);
    let (cw, r_no, d) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    let (t, d2, rs) = deposit_tx(&d, YES, &[(stake(31, 20 * UNIT as u64, 0, 0xa1), 31)], 530);
    must("defence", &t);
    let (dt, pool, dw) = decide_tx(&live(&t, 0, 530, &d2), &cw, 630);
    must("decide", &dt);
    assert_eq!(dw.status, UPHELD);
    let (st, _, pays) = settle_tx(&live(&dt, 0, 630, &pool), &[live(&t, 1, 530, &rs[0]), r_no], 631);
    must("settle", &st);
    assert_eq!(pays[0] - 20 * UNIT as u64, 9 * UNIT as u64);
}

/// A party cannot dodge its underwriter's claim by respending the pair faster than the claim can land: after any party
/// entry the pair is quietDaa young and only the underwriter's entries (and a cooperative close by both) can spend it;
/// the claim signed for the new outpoint lands in that time.
#[test]
fn holds_a_party_cannot_dodge_a_claim_by_respending_its_pair() {
    let (lp, _, _) = opened();
    let (at, a1) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("the long respends its pair", &at);
    let young = live(&at, 0, 400, &a1);
    refused("and again before quietDaa", &amend_tx(&young, L, &long_kp(), 1_010_000_000, 19).0, 0);
    // The underwriter's claim on the new outpoint is valid at once (no quiet for propose).
    let (t, _) = propose_tx(&young, &liq(L, FALSE_P, 401), &uw_kp(), None);
    must("the claim lands inside the quiet time", &t);
}

/// The parties cannot void a valid claim: validity depends on P, t0 and X only; a late contest is ignored; neither
/// party can spend the pending part but by cure (the loser, paying the underwriter its refund).
#[test]
fn holds_the_parties_cannot_void_a_valid_claim_or_take_the_bond() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq(L, 900_000_000, 450), 500);
    // A late contest by the loser's own stake: ignored, the claim settles.
    let (cw, _, d) = contest_at(&lw, 41, 10 * UNIT as u64, 700);
    let (dt, _, dw) = decide_tx(&d, &cw, 800);
    must("decide", &dt);
    let (t, s) = finalize_tx(&live(&dt, 1, 800, &dw), 500, 0);
    must("finalize", &t);
    assert!(matches!(s, Settlement::Forfeit { .. }));
}

/// An underwriter that contests its own false claim first (to own the only dispute) and defends it does not shut out
/// honest stakers: anyone on the trailing side deposits into that dispute, and NO, answered with 1.25 x the defence,
/// refutes the claim; the underwriter loses the bond and its defence.
#[test]
fn holds_a_self_contest_does_not_shut_out_honest_stakers() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq(L, FALSE_P, 450), 500);
    let (cw, _, d) = contest_at(&lw, 31, 10 * UNIT as u64, 520);
    let (t, d2, _) = deposit_tx(&d, YES, &[(stake(32, 20 * UNIT as u64, 0, 0xa2), 32)], 530);
    must("the underwriter's sybil defends", &t);
    let ld2 = live(&t, 0, 530, &d2);
    let (t, d3, _) = deposit_tx(&ld2, kpf_protocol::judge::NO, &[(stake(72, 25 * UNIT as u64, 0, 0xa3), 72)], 540);
    must("an honest answer of 25 KAS (10 + 25 >= 1.25 x 20)", &t);
    assert_eq!(d3.side, kpf_protocol::judge::NO);
    let (dt, _, dw) = decide_tx(&live(&t, 0, 540, &d3), &cw, 640);
    must("decide", &dt);
    assert_eq!(dw.status, REFUTED);
}

/// A Sybil of the underwriter can be a party: the contracts compare keys, not owners.
#[test]
fn exploit_a_sybil_key_of_the_underwriter_can_be_a_party() {
    let (lo, so) = (order(L, &long_kp(), 0x71, 0), order(S, &keypair(32), 0x72, 0));
    let (t, _, _) = open_tx(&lo, &so, &slot(K), &open_spec(), &|_, _| {});
    must("an open whose short is another key of the underwriter's owner", &t);
}

// ---------------------------------------------------------------- stuck states

/// From every state the three templates, the slot and the judge write there is an exit that needs nobody's cooperation
/// (or only the owner's): a pending part finalizes after W; a contested one decides when its dispute resolves (anyone,
/// after the round); a decided one finalizes at once; a pair lapse-settles after its cover ended and idleDaa; a slot
/// advances and unbonds; a free stake unbonds; receipts settle with their pool and the pool closes.
#[test]
fn holds_no_pair_bond_stake_or_dispute_is_left_unspendable() {
    let (lp, sl, _) = opened();
    must("pair: lapse settle by anyone", &lapse_tx(&lp, lp.s.lapse_from(), 0));
    must("slot: unbond by its owner once the calendar released the cover", &unbond_tx(&sl, 12, 1_400));
    let lw = proposed(&lp, &liq(L, FALSE_P, 450), 500);
    must("pending: finalize by anyone after W", &finalize_tx(&lw, 500, 200).0);
    let (cw, r, d) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    let (dt, pool, dw) = decide_tx(&d, &cw, 620);
    must("contested: decide by anyone after the round", &dt);
    must("decided: finalize by anyone at once", &finalize_tx(&live(&dt, 1, 620, &dw), 500, 0).0);
    let lpool = live(&dt, 0, 620, &pool);
    let (st, p2, _) = settle_tx(&lpool, &[r], 621);
    must("receipt: settled by anyone", &st);
    must("pool: closed by anyone", &pool_close_tx(&live(&st, 0, 622, &p2)).0);
}

// ---------------------------------------------------------------- twins and look-alikes

/// A pair written by a genesis that names a real slot (uncounted cover) can be vouched by the slot's owner only while
/// X <= P + term + step: at most one period after its genesis; every renewal is counted. An order cannot enter it without
/// the slot counting the entered part in the same transaction (take requires the bind); a signing taker can.
#[test]
fn exploit_a_genesis_pair_naming_a_real_slot_is_bounded_and_no_order_enters_it_uncounted() {
    let (lp, sl, _) = opened();
    let mut gp = lp.s.clone();
    (gp.long_ask, gp.mk) = (1_000_000_000, Makers::default());
    let fake = genesis_pair(gp.clone(), lp.value, 0x5a, 400);
    // The underwriter can vouch it (it is the attacker).
    let lw = proposed(&fake, &liq(L, FALSE_P, 450), 500);
    must("an uncounted pair's claim settles", &finalize_tx(&lw, 500, 200).0);
    // The same state with X beyond the pair UTXO's DAA + term + step: no claim on it can be valid, so the proposal is
    // refused (review 11; before, it was proposed and void at finalize).
    let mut far = gp.clone();
    far.b.expiry = 1_200 + 900 + 100;
    let gfar = genesis_pair(far, lp.value, 0x5b, 400);
    refused("a liquidation of a pair whose expiry lies beyond its DAA + term + step", &propose_tx(&gfar, &liq(L, FALSE_P, 450), &uw_kp(), None).0, 0);
    // An order entering it: refused without the bind, accepted with it (and then the slot counts the part).
    let lo = order(L, &keypair(46), 0x7a, 0);
    refused("take without a bind", &take_tx(&fake, &lo, &sl, 1_000_000_000, 400, false).0, 1);
    let (t, _) = take_tx(&fake, &lo, &sl, 1_000_000_000, 400, true);
    must("an order enters the look-alike only with the slot counting the part", &t);
    let counted = bondkit_counted(&t, 3);
    assert_eq!(counted, 2 * lp.s.cover_of(lp.s.size), "the honest pair's cover and the entered part's, both counted at X");
    // A slot with no capacity left refuses to count it: the order cannot enter.
    let mut full = sl.clone();
    full.value = lp.s.cover_of(lp.s.size) as u64;
    let (t, _) = take_tx(&fake, &lo, &full, 1_000_000_000, 400, true);
    refused("no capacity for the entered part", &t, 2);
}

/// The cover the slot output `j` of `t` counts at X = 1,200 (its state decoded from the script is not needed: the
/// model's continuation is rebuilt and compared).
fn bondkit_counted(t: &T, j: usize) -> i64 {
    let (_, sl, _) = opened();
    let lp_cover = 600_000_000_000i64;
    let mut b = sl.s.clone();
    let idx = b.bucket_of(1_200).unwrap();
    b.ring[idx] += lp_cover;
    assert_eq!(t.0.outputs[j].script_public_key, bond_spk(&b), "the slot continuation counts the part");
    b.counted_at(1_200)
}

/// An order entering an honest listed pair takes it with the slot's bind of the part (the underwriter signs the entry).
#[test]
fn holds_an_order_enters_a_pair_only_with_its_cover_counted() {
    let (lp, sl, _) = opened();
    let (at, listed) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("list", &at);
    let lp1 = live(&at, 0, 400, &listed);
    let mut big = sl.clone();
    big.value = 2 * lp.s.cover_of(lp.s.size) as u64;
    let lo = order(L, &keypair(46), 0x7a, 0);
    let (t, part) = take_tx(&lp1, &lo, &big, 1_000_000_000, 400, true);
    must("take with the bind", &t);
    assert_eq!((part.long_key, part.mk.long, part.mk.parent), (x(&keypair(46)), maker_of(&lo.op), maker_of(&lp1.op)));
}

/// A pending part written by a genesis that names a real slot reaches nothing of the slot: its bond is its own, its
/// contest, decide and finalize spend only itself and its dispute; and no output can carry an existing part's id
/// without that part as input (a genesis makes a new id), so no look-alike dispute can decide a real part.
#[test]
fn holds_look_alike_pending_parts_and_disputes_cannot_reach_real_ones() {
    let (lp, _, _) = opened();
    let w = lp.s.pending_of(kit(), lp.s.size, LIQ, L, FALSE_P, 450).unwrap().0;
    let fake = Live { op: outpoint(0x5c), value: lp.value + w.bond as u64, daa: 500, id: Some(Hash::from_bytes([0x5c; 32])), s: w };
    let (t, _) = finalize_tx(&fake, 500, 200);
    must("a look-alike finalizes with its own value", &t);
    assert!(t.0.inputs.len() == 2, "the slot is not an input");
    // An output bound to a real part's id in a transaction without that part: the covenant context refuses it.
    let real = proposed(&lp, &liq(L, FALSE_P, 450), 500);
    let ins = vec![exec_in(240)];
    let outs = vec![TransactionOutput { value: 1_000_000, script_public_key: p2pk_spk(&exec_kp()), covenant: bind(0, real.id.unwrap()) }];
    let t = build_ext(&ins, outs, 0, &[0]);
    assert!(run(&t.0, &t.1).is_err());
}

/// Twins across templates: a second output of a new pair's id at its genesis is refused by both orders (the scan),
/// whatever template the twin carries (a pair, a pending part, a plain key).
#[test]
fn holds_a_twin_of_any_template_at_a_pairs_genesis_is_refused() {
    let (lo, so) = (order(L, &long_kp(), 0x71, 0), order(S, &short_kp(), 0x72, 0));
    for twin in 0..3 {
        let (t, _, _) = open_tx(&lo, &so, &slot(K), &open_spec(), &|o, p| {
            let spk = match twin {
                0 => pair_spk(p),
                1 => pend_spk(&sample_pending(p)),
                _ => p2pk_spk(&exec_kp()),
            };
            o.push(TransactionOutput { value: 1_000_000, script_public_key: spk, covenant: twin_binding() });
        });
        let v = verdicts(&t);
        assert!(!v[0] && !v[1], "twin {twin}: {v:?}");
    }
}

/// Split bonds: each child counts within its own value; splitting never creates capacity.
#[test]
fn holds_split_bonds_each_count_within_their_own_value() {
    let cover = 600_000_000_000u64;
    let sl = slot(2 * cover);
    let (t, ba, bb) = split_tx(&sl, &[0; RING], cover, 300);
    must("split", &t);
    let (a, b) = (live(&t, 0, 300, &ba), live(&t, 1, 300, &bb));
    for (child, n) in [(a, 0x71u8), (b, 0x75)] {
        let (lo, so) = (order(L, &long_kp(), n, 0), order(S, &short_kp(), n + 1, 0));
        let (t, _, b2) = open_tx(&lo, &so, &child, &open_spec(), &|_, _| {});
        must("one pair per child", &t);
        let full = live(&t, 3, 300, &b2);
        let (lo, so) = (order(L, &long_kp(), n + 2, 0), order(S, &short_kp(), n + 3, 0));
        refused("a second pair on the child", &open_tx(&lo, &so, &full, &open_spec(), &|_, _| {}).0, 2);
    }
    // A split that gives a child more cover than its value carries is refused.
    let (lp_t, _, b1) = {
        let (lo, so) = (order(L, &long_kp(), 0x79, 0), order(S, &short_kp(), 0x7a, 0));
        open_tx(&lo, &so, &slot(2 * cover), &open_spec(), &|_, _| {})
    };
    let bound = live(&lp_t, 3, 300, &b1);
    let mut ring = [0i64; RING];
    ring[11] = cover as i64;
    refused("child A with all the cover and half the value minus one", &split_tx(&bound, &ring, cover - 1, 300).0, 0);
    must("child A with all the cover and exactly its value", &split_tx(&bound, &ring, cover, 300).0);
}

// ---------------------------------------------------------------- hostile markets

/// A market block can pass every bound and still be hostile: a contest window of 1 DAA, no quiet time, a bond of 0.02
/// KAS, rho 5, the disputer taking the whole bond, settlement at the entry right after a lapse. The contracts accept it
/// (they enforce only bounds); the wallet's check flags each. A feed definition whose hash matches can still render
/// misleadingly (a bidirectional override, a homoglyph, an invisible character): only a wallet can catch that.
#[test]
fn exploit_a_hostile_market_passes_the_bounds_and_only_a_wallet_can_flag_it() {
    let mut m = omarket_with(BondParams { rho_bps: 50_000, ..omarket().bond });
    m.p = OParams { challenge_daa: 1, quiet_daa: 0, sigma_bps: 0, bond_min: 0, beta_bps: 10_000, idle_daa: 0, late_daa: 1_000, ..m.p };
    assert!(m.check().is_ok());
    let (pairs, _, ok) = open_many(&m, K, 1);
    assert_eq!(ok, 1, "the contracts open a pair on it");
    let lw = proposed(&pairs[0], &liq(L, FALSE_P, 450), 500);
    assert_eq!(lw.s.bond, 2_000_000, "a claim's bond is the dust floor");
    must("a false claim finalizes one DAA after it lands", &finalize_tx(&lw, 500, 1).0);
    let w = market_warnings(&m, 10);
    for needle in ["contest window", "automated watcher", "rho is 5", "bond is 0", "disputer takes 100", "quiet time", "lapsed settles", "older than the whole contest window"] {
        assert!(w.iter().any(|l| l.contains(needle)), "{needle}: {w:?}");
    }
    // Review 11: the testnet defaults' 2-minute window raises one statement: only an automated watcher can contest.
    let tw = market_warnings(&OMarket::testnet([0; 32]), 10);
    assert!(tw.len() == 1 && tw[0].contains("automated watcher"), "{tw:?}");
    // The feed definition.
    let honest = FEED.as_bytes();
    let c = check_feed(honest);
    assert!(c.suspicious.is_empty() && c.missing.is_empty(), "{c:?}");
    for bad in ["quantity: \u{0425}AU in KAS\n", "quantity: gold\u{202e}DSU\n", "quantity: go\u{200b}ld\n", "quantity: A\rquantity: B\n"] {
        let c = check_feed(bad.as_bytes());
        assert!(!c.suspicious.is_empty(), "{bad:?}");
        assert_eq!(c.hash, feed_hash_of(bad.as_bytes()), "the hash matches the text exactly; the rendering is the wallet's problem");
    }
}

/// The note's worked capacity at rho = 1 and 5 (DESIGN-ORACLE-2.md 2): a slot of K underwrites rho x K of cover, and
/// one pair's cover is never above the slot's value (a bind refuses a claim above it), so the restitution of a later
/// stage (H <= c) would always be payable from one slot.
#[test]
fn holds_one_pairs_cover_never_exceeds_its_slot() {
    let m = omarket_with(BondParams { rho_bps: 50_000, ..omarket().bond });
    let (lo, mut so) = (order(L, &long_kp(), 0x71, 0), order(S, &short_kp(), 0x72, 0));
    let mut lo = lo;
    lo.s.m = m;
    so.s.m = m;
    let mut sl = slot(600_000_000_000 - 1);
    sl.s.m = m.bond_market();
    refused("a cover above the slot's value at rho 5", &open_tx(&lo, &so, &sl, &open_spec(), &|_, _| {}).0, 2);
}
