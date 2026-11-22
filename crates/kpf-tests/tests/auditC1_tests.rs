//! Review C1 of the committee family (docs/audit/REVIEW-C1.md), brought to the fixed family for review C2
//! (docs/audit/REVIEW-C2.md): `KpfCommittee`, `KpfEpochTable`, `KpfExit`, `KpfOpenC`, `KpfPairC`, `KpfPendingC` against
//! `docs/DESIGN-COMMITTEE.md`.
//!
//! `c1_fixed_*`: an attack of review C1 (then `c1_exploit_*`, or a `c1_bounded_*` whose cause was removed) that the
//! fixed contracts refuse; the old numbers are kept in a comment and the refusal is asserted at the step that used to
//! succeed.
//! `c1_bounded_*`: an accepted behaviour whose cost or damage is measured here, with the bound as it is after the fixes.
//! `c1_holds_*`: an attack that the contracts refuse, then and now.
//!
//! Fixture (`committeekit`): the committee's epoch [6,000, 7,000), 100 KAS of weight in 15 members (30, 22, 15, 12, 8, 2, 2,
//! seven of 1, and 2), a quorum of 67 % within a spread of 0.5 %; the market: a lease of 8,000 DAA, a lapse 1,000 after
//! it, 50 DAA to cure, a claim valid within 100 DAA of its time, 20 DAA of quiet, mmr 5 %, g 2 %, a fee of 0.5 %, a
//! carrier of 0.2 KAS. The worked pair: 1,000 units at 10 KAS, the long's bound at 9, the short's at 12 (1,000 and 2,000
//! KAS of margin), opened at 6,100 with the lease 14,000.
#![allow(non_snake_case)]

mod committeekit;
mod common;

use committeekit::*;
use common::*;
use kaspa_consensus_core::tx::{TransactionInput, TransactionOutput};
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::*;
use kpf_protocol::committee::*;
use kpf_protocol::oracle::{maker_of, Makers};
use kpf_protocol::state::{KIND_CLOSE, KIND_LIQ, LONG, SHORT};
use kpf_protocol::verify::{run_raw, Meter};

const KAS1: i64 = 100_000_000;
const SIZE: i64 = 1_000_000_000;
/// The feed in the scenarios: 10 KAS.
const FEED: i64 = 1_000_000_000;
/// The highest and the lowest price a quorum that signed 10 KAS attests at a spread of 0.5 %:
/// `|10 - p| x 10^4 <= 50 p`.
const FEED_HI: i64 = 1_005_025_125;
const FEED_LO: i64 = 995_024_876;

// ---------------------------------------------------------------- helpers

/// An attestation of `p` for the time `t0` of `market` in which the members `(i, price)` of the fixture keys signed.
fn att_of(market: &[u8; 32], t0: i64, votes: &[(usize, i64)], p: i64) -> Attestation {
    let mut a = Attestation { market: *market, t0, p, bits: 0, prices: [0; MEMBERS], sigs: [[0; 64]; MEMBERS] };
    for (i, price) in votes {
        let s = sign_price(&member_kp(*i), &rid(), market, t0, *price);
        a.bits |= 1 << i;
        a.prices[*i] = *price;
        a.sigs[*i] = s.sig;
    }
    a
}

fn fx(t0: i64, votes: &[(usize, i64)], p: i64) -> Attestation {
    att_of(&cmarket().id(), t0, votes, p)
}

fn liq_with(a: Attestation, side: i64, n: i64, proposer: [u8; 32], lock: u64) -> Propose {
    Propose { c: Claim { kind: KIND_LIQ, side, n, p: a.p, t0: a.t0 }, a, signer: None, proposer, lock, seq: 0 }
}

fn close_with(a: Attestation, side: i64, n: i64, kp: &secp256k1::Keypair, lock: u64) -> Propose {
    Propose { c: Claim { kind: KIND_CLOSE, side, n, p: a.p, t0: a.t0 }, a, signer: Some(*kp), proposer: x(kp), lock, seq: cmarket().p.quiet_daa as u64 }
}

/// What the outputs of `t` pay to `key` (plain outputs only).
fn paid_to(t: &T, key: &[u8; 32]) -> i64 {
    let spk = key_spk(key);
    t.0.outputs.iter().filter(|o| o.script_public_key == spk && o.covenant.is_none()).map(|o| o.value as i64).sum()
}

/// Script units used by every input and the relay fee floor of the transaction at the node's minimum rate, with each
/// input's compute budget committed as the tool's builder commits it (an estimate: the kit's transactions carry a
/// one-sompi change and no fee of their own).
fn cost_of(t: &T) -> (u64, u64) {
    let units = run_raw(&t.0, &t.1, Meter::Unlimited).expect("bindings");
    let mut tx = t.0.clone();
    let mut total = 0;
    for (i, u) in units.iter().enumerate() {
        let u = *u.as_ref().expect("an accepted input");
        total += u;
        let inp = &t.0.inputs[i];
        tx.inputs[i] = TransactionInput::new_with_compute_budget(inp.previous_outpoint, inp.signature_script.clone(), inp.sequence, kpf_protocol::tx::budget_for_units(u));
    }
    let m = kpf_protocol::tx::masses(&tx, &t.1);
    (total, kpf_protocol::tx::min_fee(&m, kpf_protocol::tx::MIN_FEE_RATE))
}

fn kas(v: i64) -> String {
    format!("{}.{:08}", v / KAS1, v % KAS1)
}

/// The epoch table's own next UTXO after a transaction that spent it by attest and continued it at output `j`.
fn epoch_table_after(t: &T, j: usize, lt: &LEpochTable, daa: u64) -> LEpochTable {
    live(t, j, daa, &lt.s)
}

fn equal_epoch_table() -> LEpochTable {
    epoch_table(&EpochTable { members: table([10; MEMBERS]), total: 150 * KAS1, from: 6_000, until: 7_000, quorum_bps: 6_700, spread_bps: 50 })
}

/// The epoch table of the epoch of 1,000 DAA that holds `t0`, as a roll would have written it.
fn epoch_table_for(t0: i64) -> LEpochTable {
    let from = t0 / 1_000 * 1_000;
    epoch_table(&EpochTable { members: table(TOP3), total: 100 * KAS1, from, until: from + 1_000, quorum_bps: 6_700, spread_bps: 50 })
}

/// `KpfEpochTable.attest`'s rule on plain numbers: every signed (price, weight) lies within `spread` of `p` and the weight
/// reaches the quorum of `total`.
fn band_ok(signed: &[(i64, i64)], total: i64, quorum_bps: i64, spread: i64, p: i64) -> bool {
    let w: i64 = signed.iter().map(|s| s.1).sum();
    w >= quorum_of(total, quorum_bps) && signed.iter().all(|s| (s.0 as i128 - p as i128).abs() * 10_000 <= p as i128 * spread as i128)
}

// ---------------------------------------------------------------- C1-F1: the price rule

#[test]
fn c1_fixed_half_a_quorum_no_longer_dictates_the_price() {
    // Review C1 (c1_exploit_half_a_quorum_dictates_the_price_with_the_honest_members_own_signatures): members 1 and 3 (34 KAS
    // of 100) and the honest signatures of members 0, 5 and 7 (33 KAS, signed 10.00) attested 12.30 and 9.28 as a
    // weighted median: a false close paid the long 3,000 KAS (the short's 2,000 gone), a false liquidation forfeited
    // the long's 1,000 KAS. Now every included price must lie within 0.5 % of the price claimed.
    let lt = epoch_epoch_table();
    assert_eq!(lt.s.quorum(), 67 * KAS1);
    let colluders = [1usize, 3];
    let honest_used = [0usize, 5, 7];
    let weight = |members: &[usize]| members.iter().map(|i| lt.s.members[*i].weight).sum::<i64>();
    assert_eq!((weight(&colluders), weight(&honest_used)), (34 * KAS1, 33 * KAS1));
    let lp = opened();
    for (false_price, side) in [(1_230_000_000i64, LONG), (928_000_000, SHORT)] {
        let votes: Vec<(usize, i64)> = colluders.iter().map(|i| (*i, false_price)).chain(honest_used.iter().map(|i| (*i, FEED))).collect();
        // The step that used to succeed: the epoch table's attest with the false price claimed.
        let a = fx(6_500, &votes, false_price);
        assert!(a.check(&lt.s, &rid(), 6_500).is_err());
        refused("the epoch table attests the false price with 34 % lying", &attest_tx(&lt, &a, 6_500, |_| {}), 0);
        // Nor does that set attest anything else: the two groups are not within the spread of one price.
        for p in [FEED, (false_price + FEED) / 2, FEED_HI, FEED_LO] {
            refused("the mixed set attests another price", &attest_tx(&lt, &fx(6_500, &votes, p), 6_500, |_| {}), 0);
        }
        // The claim itself: the pair's script is satisfied, the epoch table's is not.
        let pr = if side == LONG { close_with(a, LONG, SIZE, &long_kp(), 6_500) } else { liq_with(a, LONG, SIZE, x(&short_kp()), 6_500) };
        refused("the claim at the false price", &propose_tx(&lp, &lt, &pr, |_| {}), 1);
    }
    // The colluders' own signatures and no honest one: 34 KAS, no quorum.
    refused("the colluders alone", &attest_tx(&lt, &fx(6_500, &[(1, 928_000_000), (3, 928_000_000)], 928_000_000), 6_500, |_| {}), 0);
    // The rule on numbers: a false price far from the feed needs a quorum of the weight signed near it.
    let far = 928_000_000;
    assert!(!band_ok(&[(far, 34), (FEED, 33)], 100, 6_700, 50, far));
    assert!(!band_ok(&[(far, 66), (FEED, 1)], 100, 6_700, 50, far));
    assert!(band_ok(&[(far, 67)], 100, 6_700, 50, far));
}

#[test]
fn c1_fixed_six_lying_members_of_fifteen_attest_nothing() {
    // Review C1 (c1_exploit_six_of_sixteen_equal_members_attest_any_price): 6 lying members and 5 honest signatures attested
    // 1 sompi, 5 KAS, 50 KAS and 999,999.99999999 KAS on 16 equal members. Fifteen members of 10 KAS now: the quorum is 100.5
    // KAS, 11 members.
    let lt = equal_epoch_table();
    assert_eq!(lt.s.quorum(), 100 * KAS1 + 50_000_000);
    let votes = |liars: usize, honest: usize, p: i64| -> Vec<(usize, i64)> { (0..liars).map(|i| (i, p)).chain((liars..liars + honest).map(|i| (i, FEED))).collect() };
    for p in [1i64, 500_000_000, 5_000_000_000, 99_999_999_999_999] {
        refused("6 lying members and 5 honest signatures", &attest_tx(&lt, &fx(6_500, &votes(6, 5, p), p), 6_500, |_| {}), 0);
        refused("10 lying members and 1 honest signature", &attest_tx(&lt, &fx(6_500, &votes(10, 1, p), p), 6_500, |_| {}), 0);
        refused("10 lying members alone", &attest_tx(&lt, &fx(6_500, &votes(10, 0, p), p), 6_500, |_| {}), 0);
        // What the note admits: a quorum that lies.
        must("11 lying members", &attest_tx(&lt, &fx(6_500, &votes(11, 0, p), p), 6_500, |_| {}));
    }
    must("11 honest signatures attest the feed", &attest_tx(&lt, &fx(6_500, &votes(0, 11, FEED), FEED), 6_500, |_| {}));
    refused("10 honest signatures do not", &attest_tx(&lt, &fx(6_500, &votes(0, 10, FEED), FEED), 6_500, |_| {}), 0);
}

#[test]
fn c1_holds_less_than_a_quorum_cannot_attest_beyond_the_spread_of_an_honest_price() {
    // Members 0, 1, 3 and 14 collude: 30 + 22 + 12 + 2 = 66 KAS, one short of the quorum. Whatever they sign and whatever
    // honest signatures the claimant adds, an accepted set holds an honest member, so the price claimed lies within the
    // spread of the feed: between 9.95024876 and 10.05025125, which a claimant reaches with honest signatures alone.
    let lt = epoch_epoch_table();
    let colluders = [0usize, 1, 3, 14];
    let honest: Vec<usize> = (0..MEMBERS).filter(|i| !colluders.contains(i)).collect();
    let a: i64 = colluders.iter().map(|i| lt.s.members[*i].weight / KAS1).sum();
    assert_eq!(a, 66);
    let mut accepted_prices = 0;
    for mask in 1u32..1 << honest.len() {
        let h: i64 = honest.iter().enumerate().filter(|(k, _)| mask >> k & 1 == 1).map(|(_, i)| lt.s.members[*i].weight / KAS1).sum();
        for xp in [1i64, 900_000_000, 990_000_000, FEED_LO - 1, FEED_LO, FEED, FEED_HI, FEED_HI + 1, 1_010_000_000, 1_100_000_000] {
            for p in [1i64, xp, (xp + FEED) / 2, FEED_LO - 1, FEED_LO, FEED, FEED_HI, FEED_HI + 1] {
                if band_ok(&[(xp, a), (FEED, h)], 100, 6_700, 50, p) {
                    assert!((FEED_LO..=FEED_HI).contains(&p), "colluders at {xp}, honest {h}: {p} attested");
                    accepted_prices += 1;
                }
            }
        }
    }
    assert!(accepted_prices > 0);
    // In the engine: the 66 KAS sign 10.09 (within the spread of 10.05025125), member 2 signed the feed.
    let votes: Vec<(usize, i64)> = colluders.iter().map(|i| (*i, 1_009_000_000)).chain([(2usize, FEED)]).collect();
    must("the edge of the feed's band", &attest_tx(&lt, &fx(6_500, &votes, FEED_HI), 6_500, |_| {}));
    refused("one sompi beyond it", &attest_tx(&lt, &fx(6_500, &votes, FEED_HI + 1), 6_500, |_| {}), 0);
    refused("the colluders' own price", &attest_tx(&lt, &fx(6_500, &votes, 1_009_000_000), 6_500, |_| {}), 0);
    // The same edge from honest signatures alone.
    let honest3 = [(0usize, FEED), (1, FEED), (2, FEED)];
    must("three honest members, the upper edge", &attest_tx(&lt, &fx(6_500, &honest3, FEED_HI), 6_500, |_| {}));
    must("three honest members, the lower edge", &attest_tx(&lt, &fx(6_500, &honest3, FEED_LO), 6_500, |_| {}));
    refused("above the upper edge", &attest_tx(&lt, &fx(6_500, &honest3, FEED_HI + 1), 6_500, |_| {}), 0);
    refused("below the lower edge", &attest_tx(&lt, &fx(6_500, &honest3, FEED_LO - 1), 6_500, |_| {}), 0);
}

#[test]
fn c1_bounded_a_claimant_moves_the_price_by_the_spread_around_what_a_quorum_signed() {
    // Review C1: under the median rule a claimant chose between the 6th and the 11th of 16 honest prices (9.994 to
    // 10.004 for prices 0.02 % apart). Under the band rule it claims any price within the spread of 11 signed prices:
    // fifteen honest members sign prices 0.02 % apart (9.986 to 10.014); the lowest claim is the 11th lowest price less
    // the spread, the highest the 5th lowest plus it.
    let price = |i: usize| FEED + (i as i64 - 7) * 200_000;
    let lt = equal_epoch_table();
    let lowest = (price(10) as i128 * 10_000 + 10_049) / 10_050;
    let highest = price(4) as i128 * 10_000 / 9_950;
    let (lowest, highest) = (lowest as i64, highest as i64);
    let low_set: Vec<(usize, i64)> = (0..11).map(|i| (i, price(i))).collect();
    let high_set: Vec<(usize, i64)> = (4..15).map(|i| (i, price(i))).collect();
    must("the 11 lowest prices, the lowest claim", &attest_tx(&lt, &fx(6_500, &low_set, lowest), 6_500, |_| {}));
    refused("one sompi lower", &attest_tx(&lt, &fx(6_500, &low_set, lowest - 1), 6_500, |_| {}), 0);
    must("the 11 highest prices, the highest claim", &attest_tx(&lt, &fx(6_500, &high_set, highest), 6_500, |_| {}));
    refused("one sompi higher", &attest_tx(&lt, &fx(6_500, &high_set, highest + 1), 6_500, |_| {}), 0);
    // No other set does better: every set of at least 11 members, on numbers.
    let (mut lo, mut hi) = (i64::MAX, i64::MIN);
    for mask in 1u32..1 << MEMBERS {
        if mask.count_ones() < 11 {
            continue;
        }
        let (mn, mx) = (0..MEMBERS).filter(|i| mask >> i & 1 == 1).fold((i64::MAX, i64::MIN), |(a, b), i| (a.min(price(i)), b.max(price(i))));
        lo = lo.min(((mx as i128 * 10_000 + 10_049) / 10_050) as i64);
        hi = hi.max((mn as i128 * 10_000 / 9_950) as i64);
    }
    assert_eq!((lo, hi), (lowest, highest));
    println!(
        "C1 bounded (now): honest prices {} .. {} (15 members, spread 0.5 %); a claimant attests {} .. {}; with every member at 10.00000000: {} .. {}",
        kas(price(0)),
        kas(price(14)),
        kas(lowest),
        kas(highest),
        kas(FEED_LO),
        kas(FEED_HI)
    );
}

// ---------------------------------------------------------------- C1-F2: an order taken without a pair

fn order_with(side: i64, kp: &secp256k1::Keypair, n: u8, daa: u64, tip: i64, min_counter: i64) -> LOrder {
    let o = COpen { tip, min_counter_rate: min_counter, ..sample_order(&cmarket(), side, x(kp)) };
    Live { op: outpoint(n), value: (o.spend(o.size_left) + KAS1) as u64, daa, id: Some(Hash::from_bytes([n; 32])), s: o }
}

/// The part a take of the whole order writes when no pair hands anything over: the order's side at `px`, the counter
/// side `counter` with its bound at `bound`, the entry `e`, the lease `xe`, no parent.
fn lone_part(lo: &LOrder, px: i64, e: i64, bound: i64, xe: i64, counter: [u8; 32]) -> CPair {
    let o = &lo.s;
    let (bl, bs, lk, sk, mk) = if o.side == LONG {
        (px - o.my_rate, bound, o.owner, counter, Makers { long: maker_of(&lo.op), ..Makers::default() })
    } else {
        (bound, px + o.my_rate, counter, o.owner, Makers { short: maker_of(&lo.op), ..Makers::default() })
    };
    CPair { m: o.m, size: o.size_left, bl, bs, entry: e, long_key: lk, short_key: sk, expiry: xe, long_ask: 0, long_min: 0, long_tip: 0, short_bid: 0, short_min: 0, short_tip: 0, mk }
}

/// Inputs: the order (take), the taker's key. Outputs: the order's refund at 0, the part (a fresh id authored by the
/// taker's input) at 1 holding exactly its floor, the change.
fn lone_take_tx(lo: &LOrder, px: i64, e: i64, bound: i64, xe: i64, counter: [u8; 32], lock: u64) -> T {
    let part = lone_part(lo, px, e, bound, xe, counter);
    let n = lo.s.size_left;
    let mut outs = vec![out(lo.value - lo.s.spend(n) as u64, key_spk(&lo.s.owner), None), out(part.floor().max(1) as u64, pair_spk(&part), None), change(1)];
    seal(&mut outs, 1, 1, outpoint(60));
    let k = kit();
    let args = vec![iv(1), iv(n), iv(px), iv(e), iv(bound), bv(&counter), bv(&[0u8; 32]), iv(0), iv(0), iv(0), iv(xe), bv(&k.pair.prefix), bv(&k.pair.suffix)];
    build_ext(&[order_in(lo, "take", args), exec_in(60)], outs, lock, &[])
}

#[test]
fn c1_fixed_a_take_needs_the_counter_side_healthy_at_the_price() {
    // Review C1 (c1_exploit_a_take_without_a_pair_against_no_margin_pays_the_taker_a_liquidation_fee_from_the_orders_
    // margin): a long order of 1,000 units at 10 KAS (1,010.2 KAS escrowed, min counter rate 0) was taken with the
    // short's bound at the price; the taker posted nothing, kept the 10 KAS tip, liquidated its own marginless side
    // with the true price and was paid a 50 KAS fee out of the order's margin.
    let owner = keypair(43);
    let lo = order_with(LONG, &owner, 13, 6_150, 1_000_000, 0);
    let me = x(&exec_kp());
    // The step that used to succeed: the take with the counter bound at the price (the lease in the new range).
    refused("a take with the short's bound at the price", &lone_take_tx(&lo, FEED, FEED, FEED, 14_200, me, 6_200), 0);
    refused("a sompi below the short's maintenance", &lone_take_tx(&lo, FEED, FEED, 1_049_999_999, 14_200, me, 6_200), 0);
    // At the maintenance band it is accepted: the taker posts 500 KAS of margin (less the tip), and the true price
    // does not liquidate it, nor does the highest price a quorum that signed the feed attests.
    let take = lone_take_tx(&lo, FEED, FEED, 1_050_000_000, 14_200, me, 6_200);
    must("a take with the short at its maintenance", &take);
    let part = lone_part(&lo, FEED, FEED, 1_050_000_000, 14_200, me);
    assert_eq!(part.floor() - lo.s.spend(SIZE), 490 * KAS1);
    assert!(!part.short_liquidatable(FEED) && !part.short_liquidatable(FEED_HI));
    let lt = epoch_epoch_table();
    let lpart = live(&take, 1, 6_200, &part);
    refused("no liquidation of the short at the feed", &propose_tx(&lpart, &lt, &Propose::liq(&lt, SHORT, SIZE, FEED, 6_500), |_| {}), 0);
    let hi = fx(6_500, &[(0, FEED), (1, FEED), (2, FEED)], FEED_HI);
    refused("nor at the band's upper edge", &propose_tx(&lpart, &lt, &liq_with(hi, SHORT, SIZE, me, 6_500), |_| {}), 0);
    // The mirror: a short order (2 KAS of margin a unit), the taker a long.
    let so = order_with(SHORT, &owner, 14, 6_150, 0, 0);
    refused("a take of a short order with the long's bound at the price", &lone_take_tx(&so, FEED, FEED, FEED, 14_200, me, 6_200), 0);
    refused("a sompi above the long's maintenance", &lone_take_tx(&so, FEED, FEED, 950_000_001, 14_200, me, 6_200), 0);
    must("the long at its maintenance", &lone_take_tx(&so, FEED, FEED, 950_000_000, 14_200, me, 6_200));
}

#[test]
fn c1_fixed_a_take_writes_no_lease_that_is_over_or_never_ends() {
    // Review C1 (c1_exploit_a_take_without_a_pair_writes_a_lease_that_is_over_or_never_ends): a take with xe = 0 gave
    // the taker an option to settle at the entry (and the order's tip and carrier, 10.2 KAS); xe = 499,999,999,999 gave
    // a pair no lapse settle could ever end. The order here was accepted at 6,150; the term is 8,000.
    let owner = keypair(43);
    let lo = order_with(LONG, &owner, 13, 6_150, 1_000_000, 0);
    let me = x(&exec_kp());
    let take = |xe: i64, lock: u64| lone_take_tx(&lo, FEED, FEED, 1_200_000_000, xe, me, lock);
    refused("a lease of 0", &take(0, 6_200), 0);
    refused("a lease already due for the lapse settle", &take(5_199, 6_200), 0);
    refused("a lease one DAA short of half a term after the order", &take(10_149, 6_200), 0);
    must("a lease half a term after the order", &take(10_150, 6_200));
    must("a lease a term from the lock time", &take(14_200, 6_200));
    refused("one DAA beyond a term from the lock time", &take(14_201, 6_200), 0);
    refused("a lease at the end of the DAA range", &take(499_999_999_999, 6_200), 0);
    refused("a lease 1,585 years out", &take(499_999_499_999, 6_200), 0);
    // A far lease needs a lock time as far: such a transaction is mined when the lease is a term away.
    must("a far lease with its own lock time", &take(499_999_499_999, 499_999_491_999));
    // The order's terms still bind.
    assert!(!accepted(&lone_take_tx(&lo, FEED + 1, FEED + 1, 1_200_000_000, 14_200, me, 6_200)), "above the limit");
    assert!(!accepted(&lone_take_tx(&lo, FEED, FEED - 1, 1_200_000_000, 14_200, me, 6_200)), "an entry against the order");
}

#[test]
fn c1_bounded_an_old_order_is_filled_or_taken_into_a_lapsed_pair() {
    // The residual of fill, unchanged (review C1, C1-N3): the least lease is a term after the order was accepted. An
    // order accepted at 0 and filled at 9,100 gets the lease 8,000, due for the lapse settle from 9,000.
    let owner = keypair(43);
    let o = COpen { tip: 1_000_000, ..sample_order(&cmarket(), LONG, x(&owner)) };
    let lo: LOrder = Live { op: outpoint(13), value: (o.spend(o.size_left) + KAS1) as u64, daa: 0, id: Some(Hash::from_bytes([13; 32])), s: o };
    let me = x(&exec_kp());
    let p = CPair { mk: Makers { long: maker_of(&lo.op), ..Makers::default() }, expiry: 8_000, short_key: me, long_key: x(&owner), ..sample_pair(&cmarket()) };
    let k = kit();
    let fill = |xe: i64, lock: u64| {
        let p = CPair { expiry: xe, ..p.clone() };
        let mut outs = vec![out(lo.value - lo.s.spend(SIZE) as u64, key_spk(&lo.s.owner), None), out(p.floor() as u64, pair_spk(&p), None), change(1)];
        seal(&mut outs, 1, 1, outpoint(60));
        let args = vec![iv(1), iv(SIZE), iv(FEED), iv(p.bl), iv(p.bs), bv(&me), bv(&[0u8; 32]), iv(xe), bv(&k.pair.prefix), bv(&k.pair.suffix)];
        build_ext(&[order_in(&lo, "fill", args), exec_in(60)], outs, lock, &[])
    };
    let t = fill(8_000, 9_100);
    must("the fill of the old order", &t);
    refused("a lease one DAA earlier", &fill(7_999, 9_100), 0);
    let lp = live(&t, 1, 9_100, &p);
    must("the lapse settle in the next block", &lapse_tx(&lp, 9_100, |_| {}));
    let (_, short) = p.lapse_out(lp.value);
    assert_eq!(short - (p.floor() - lo.s.spend(SIZE)), 10 * KAS1 + 20_000_000);
    // The residual of take after the fix: half a term. The same order accepted at 10,000 and taken with no pair at
    // 15,100 gets the lease 14,000, due from 15,000: the taker settles at the entry in the next block (or keeps the
    // position if the price fell) and nets the tip and the carrier, 10.2 KAS. An order is safe from it for half a term
    // and the idle time.
    let lo = Live { daa: 10_000, ..lo.clone() };
    let take = lone_take_tx(&lo, FEED, FEED, 1_200_000_000, 14_000, me, 15_100);
    must("the take of an order older than half a term and the idle time", &take);
    refused("a lease one DAA earlier", &lone_take_tx(&lo, FEED, FEED, 1_200_000_000, 13_999, me, 15_100), 0);
    let part = lone_part(&lo, FEED, FEED, 1_200_000_000, 14_000, me);
    let lpart = live(&take, 1, 15_100, &part);
    must("its lapse settle in the next block", &lapse_tx(&lpart, 15_100, |_| {}));
    let (long, short) = part.lapse_out(lpart.value);
    assert_eq!((long, short - (part.floor() - lo.s.spend(SIZE))), (1_000 * KAS1, 10 * KAS1 + 20_000_000));
}

// ---------------------------------------------------------------- C1-F3: void liquidations

#[test]
fn c1_bounded_a_loser_holds_its_pair_in_void_claims_one_block_in_two_while_it_wins_every_race() {
    // Review C1 (c1_bounded_a_loser_chains_void_liquidations_of_itself_up_to_the_lapse): 171 void claims, each resting
    // graceDaa (50 DAA) and proposed again in the block of its restore, held a pair due for liquidation from 6,450 to
    // the lapse date for no value: the long kept 1,000 KAS instead of 0, the short got 2,000.2 instead of 2,954.2 KAS.
    // Now a void claim is restored at once, and a claim needs a signed price at most lateDaa older than the pair UTXO it
    // spends, which must be accepted: the chain is one claim per restore, each on a signed price exactly lateDaa older than
    // the restore, each in a race with a keeper's valid claim on the same accepted UTXO.
    let m = cmarket();
    let low = 920_000_000;
    let mut lp = opened();
    assert!(lp.s.long_liquidatable(low));
    let mut now = 6_450u64;
    let mut t0 = 6_349i64;
    let (mut fee_claim, mut fee_restore) = (0, 0);
    for round in 0..60 {
        let lt = epoch_table_for(t0);
        let pr = Propose { proposer: x(&long_kp()), lock: now, ..Propose::liq(&lt, LONG, SIZE, low, t0) };
        // The race: a keeper's valid claim (a signed price one DAA old) is accepted from the same UTXO, in every round.
        let fresh = Propose { lock: now, ..Propose::liq(&lt, LONG, SIZE, low, now as i64 - 1) };
        let (v, _) = proposed(&lp, &lt, &fresh, now);
        assert!(v.s.valid_at(now as i64));
        if round > 0 {
            // The step that used to succeed: a signed price older than lateDaa before this UTXO (101 DAA old when it lands).
            assert!(!lp.s.price_fresh(t0 - 1, lp.daa));
            let stale = Propose { proposer: x(&long_kp()), lock: now, ..Propose::liq(&epoch_table_for(t0 - 1), LONG, SIZE, low, t0 - 1) };
            refused("a signed price more than lateDaa older than the restored UTXO", &propose_tx(&lp, &epoch_table_for(t0 - 1), &stale, |_| {}), 0);
        }
        let t = propose_tx(&lp, &lt, &pr, |_| {});
        must("a void liquidation of the long by the long", &t);
        let (w, _) = lp.s.pending_of(kit(), SIZE, KIND_LIQ, LONG, low, t0, x(&long_kp())).expect("the model accepts");
        let lw = live(&t, 0, now, &w.made_by(&lp.op));
        assert!(!lw.s.valid_at(now as i64));
        assert_eq!((lw.value, lw.s.refund, lw.s.finalize_age(now as i64)), (lp.value, 0, 0));
        // The restore needs no grace: anyone finalizes in the next block.
        let fin = finalize_tx(&lw, 0, |_| {});
        must("the void claim is restored at once", &fin);
        if round == 0 {
            (fee_claim, fee_restore) = (cost_of(&t).1, cost_of(&fin).1);
        }
        now += 1;
        lp = live(&fin, 0, now, &lw.s.pair_part().restore_state(SIZE));
        assert_eq!(lp.value, lw.value);
        // The next claim lands one block after the restore, on the one time that is void then and fresh for the UTXO.
        t0 = now as i64 - m.p.late_daa;
        now += 1;
    }
    assert_eq!(now, 6_450 + 120);
    let per_day = (fee_claim + fee_restore) as i64 * 432_000;
    println!(
        "C1-F3 (now): a void claim holds the pair for one block and costs {} + {} KAS of relay fee; held in every other block that is {} KAS a day, and 4,275 races won in a row to reach the lapse date from 6,450",
        kas(fee_claim as i64),
        kas(fee_restore as i64),
        kas(per_day)
    );
}

#[test]
fn c1_fixed_one_stale_price_holds_a_pair_once_for_a_block_and_keeps_its_listings() {
    // Review C1 (c1_bounded_one_stale_tick_freezes_and_unlists_a_healthy_pair_for_as_long_as_its_epoch_table_lives): a dip to
    // 9.28 at the time 6,300, claimed from 6,450 again and again: 50 DAA of freeze a round, both listings cleared, the
    // parties' entries refused for 20 DAA after each restore, until the epoch table was retired at 8,000 or later.
    let low = 928_000_000;
    let listed = CPair { long_ask: 1_100_000_000, long_min: 1_000_000, short_bid: 950_000_000, short_min: 1_000_000, ..sample_pair(&cmarket()) };
    let lp = pair(&listed, 6_100);
    let lt = epoch_epoch_table();
    let a = att(&lt.s, 6_300, low);
    let pr = liq_with(a.clone(), LONG, SIZE, x(&keypair(77)), 6_450);
    // The pair UTXO is older than the signed price, so the first void claim is still accepted.
    let t = propose_tx(&lp, &lt, &pr, |_| {});
    must("a void liquidation with the stale signed price", &t);
    let (w, _) = lp.s.pending_of(kit(), SIZE, KIND_LIQ, LONG, low, 6_300, x(&keypair(77))).expect("the model accepts");
    let lw = live(&t, 0, 6_450, &w.made_by(&lp.op));
    let lt = epoch_table_after(&t, 1, &lt, 6_450);
    let fin = finalize_tx(&lw, 0, |_| {});
    must("restored in the next block", &fin);
    let restored = live(&fin, 0, 6_451, &lw.s.pair_part().restore_state(SIZE));
    // The listings survive.
    assert_eq!((restored.s.long_ask, restored.s.long_min, restored.s.short_bid, restored.s.short_min), (1_100_000_000, 1_000_000, 950_000_000, 1_000_000));
    refused("a restore that clears the listings", &finalize_tx(&lw, 0, |o| o[0].script_public_key = pair_spk(&CPair { long_ask: 0, long_min: 0, short_bid: 0, short_min: 0, ..restored.s.clone() })), 0);
    // The step that used to succeed: the same time on the restored UTXO.
    refused("the stale signed price a second time", &propose_tx(&restored, &lt, &liq_with(a, LONG, SIZE, x(&keypair(77)), 6_452), |_| {}), 0);
    // A signed price older than the pair UTXO by more than lateDaa is refused the first time too, for a close as well.
    let old = epoch_table_for(5_999);
    refused("a signed price 101 DAA before the pair UTXO", &propose_tx(&lp, &old, &Propose { lock: 6_450, ..Propose::liq(&old, LONG, SIZE, low, 5_999) }, |_| {}), 0);
    must("a signed price 100 DAA before it", &propose_tx(&lp, &lt, &Propose { lock: 6_450, ..Propose::liq(&lt, LONG, SIZE, low, 6_000) }, |_| {}));
    refused("a close on a signed price 101 DAA before it", &propose_tx(&lp, &old, &Propose { lock: 6_450, ..Propose::close(&old, LONG, SIZE, 1_050_000_000, 5_999, &long_kp()) }, |_| {}), 0);
    // What is left of the grief: the quiet time restarts once (the parties' own entries wait 20 DAA after the restore).
    refused("the long relists one DAA early", &amend_tx(&restored, LONG, 1_100_000_000, 1_000_000, 0, &long_kp(), 19, |_| {}), 0);
    must("and at 20", &amend_tx(&restored, LONG, 1_100_000_000, 1_000_000, 0, &long_kp(), 20, |_| {}));
}

// ---------------------------------------------------------------- C1-F4: a price above priceMax

#[test]
fn c1_fixed_a_feed_above_priceMax_is_claimed_at_priceMax() {
    // Review C1 (c1_bounded_a_feed_above_priceMax_cannot_be_claimed): priceMax 20 KAS, the worked pair, the feed at 25:
    // the liquidation of the short and the long's close were refused, and the pair lapsed at its entry (the long 1,000
    // KAS for a side worth 3,000).
    let m2 = CMarket { p: CParams { price_max: 2_000_000_000, ..cmarket().p }, ..cmarket() };
    assert!(m2.check().is_ok());
    let lp = pair(&sample_pair(&m2), 6_100);
    let lt = epoch_epoch_table();
    let feed = 2_500_000_000;
    let votes = |p: i64| [(0usize, p), (1, p), (2, p)];
    let a = att_of(&m2.id(), 6_500, &votes(feed), feed);
    // The steps that used to be refused.
    let (lw, _) = proposed(&lp, &lt, &liq_with(a.clone(), SHORT, SIZE, x(&exec_kp()), 6_500), 6_510);
    assert_eq!(lw.s.quote, 2_000_000_000);
    must("the short is forfeited", &finalize_tx(&lw, 50, |_| {}));
    let (cw, _) = proposed(&lp, &lt, &close_with(a.clone(), LONG, SIZE, &long_kp(), 6_500), 6_510);
    assert_eq!(cw.s.quote, 2_000_000_000);
    let fin = finalize_tx(&cw, 0, |_| {});
    must("the long's close settles", &fin);
    assert_eq!(paid_to(&fin, &x(&long_kp())), 3_000 * KAS1);
    // The pending part must store the clamped price, not the attested one.
    let (forced, _) = pending_forced(&lp.s, &Claim { kind: KIND_LIQ, side: SHORT, n: SIZE, p: feed, t0: 6_500 }, x(&exec_kp()));
    assert_eq!(forced.quote, 2_000_000_000);
    let raw = CPending { quote: feed, ..forced.made_by(&lp.op) };
    refused("a pending part that stores 25", &propose_tx(&lp, &lt, &liq_with(a.clone(), SHORT, SIZE, x(&exec_kp()), 6_500), |o| o[0].script_public_key = pend_spk(&raw)), 0);
    // The epoch table still attests only what was signed, and nothing above nine times the cap of every market.
    refused("the signatures for 25 do not attest 20", &attest_tx(&lt, &a.clone().claiming(2_000_000_000), 6_500, |_| {}), 0);
    must("a price of 9 x 10^14", &attest_tx(&lt, &att_of(&m2.id(), 6_500, &votes(PRICE_LIMIT), PRICE_LIMIT), 6_500, |_| {}));
    refused("one sompi more", &attest_tx(&lt, &att_of(&m2.id(), 6_500, &votes(PRICE_LIMIT + 1), PRICE_LIMIT + 1), 6_500, |_| {}), 0);
}

// ---------------------------------------------------------------- C1-F5: the shared UTXOs

#[test]
fn c1_fixed_the_committee_rests_after_every_change_and_a_roll_does_not() {
    // Review C1 (c1_bounded_anyone_respends_the_committee_and_every_epoch_table_for_a_fee, the committee's half): twelve top-ups
    // of one sompi in a row, 0.0085 KAS of fee each, kept a roll from landing: 7,323 KAS a day in every block.
    let first = committee(&Committee { p: rparams(), from: 5_000, members: table(TOP3), holds: kpf_protocol::committee::NO_HOLDS });
    // The steps that used to succeed.
    refused("a top-up of one sompi", &top_up_tx(&first, 7, 1, |_| {}), 0);
    refused("a top-up one sompi below 1 KAS", &top_up_tx(&first, 7, KAS1 - 1, |_| {}), 0);
    let t = top_up_tx(&first, 7, KAS1, |_| {});
    must("a top-up of 1 KAS on a rested committee", &t);
    let l = live(&t, 0, 5_990, &first.s.top_up(7, KAS1).unwrap());
    with_rest(19, || {
        refused("a second top-up 19 DAA later", &top_up_tx(&l, 7, KAS1, |_| {}), 0);
        refused("a join 19 DAA later", &join_tx(&l, 8, x(&keypair(77)), 10 * KAS1, |_| {}), 0);
        refused("a leave 19 DAA later", &leave_tx(&l, 1, &member_kp(1), |_| {}), 0);
    });
    must("a second top-up 20 DAA later", &top_up_tx(&l, 7, KAS1, |_| {}));
    must("a join 20 DAA later", &join_tx(&l, 8, x(&keypair(77)), 10 * KAS1, |_| {}));
    must("a leave 20 DAA later", &leave_tx(&l, 1, &member_kp(1), |_| {}));
    // The roll that is due lands on the fresh UTXO at once (its input carries no sequence).
    let roll = roll_tx(&l, 1, 6_000, |_| {});
    must("the roll in the block after the top-up", &roll);
    assert_eq!(roll.0.inputs[0].sequence, 0);
    // What is left: at each end of the rest a leave or a join races the next top-up of 1 KAS (stake its payer gives to
    // a member, its own if it likes); a leave signed for one UTXO is void on the next.
    let (_, fee_top) = cost_of(&t);
    println!(
        "C1-F5 (now): a roll is held back by one block at most; to hold a leave or a join back, one top-up of 1 KAS every 20 DAA: {} KAS of fee and 43,200 KAS of stake a day",
        kas(fee_top as i64 * 43_200)
    );
}

#[test]
fn c1_bounded_anyone_respends_a_epoch_table_for_a_fee() {
    // Unchanged by decision (review C1, C1-F5, the epoch tables' half): anyone attests any signed price the members signed, again and
    // again, with the same public signatures, and every claim built on that epoch table UTXO is void.
    let mut lt = epoch_epoch_table();
    let a = att(&lt.s, 6_001, FEED);
    let (units, fee_att) = cost_of(&attest_tx(&lt, &a, 6_001, |_| {}));
    for k in 0..12u64 {
        let t = attest_tx(&lt, &a, 6_001, |_| {});
        must("the same attestation again", &t);
        lt = epoch_table_after(&t, 0, &lt, 6_002 + k);
    }
    // Eleven signatures of fifteen equal members (the testnet shape), and a roll that writes three epoch tables of 1 KAS.
    let eq = equal_epoch_table();
    let votes: Vec<(usize, i64)> = (0..11).map(|i| (i, FEED)).collect();
    let (units11, fee11) = cost_of(&attest_tx(&eq, &fx(6_001, &votes, FEED), 6_001, |_| {}));
    let r = committee(&Committee { p: rparams(), from: 5_000, members: table(TOP3), holds: kpf_protocol::committee::NO_HOLDS });
    let (roll_units, fee_roll) = cost_of(&roll_tx(&r, 1, 6_000, |_| {}));
    println!(
        "C1-F5 (epoch tables): relay floor at {} sompi a gram: attest with 3 signatures ({} units) {} KAS, with 11 ({} units) {} KAS; roll ({} units) {} KAS; four epoch tables respent in each of the 3,000 blocks of a testnet epoch: {} KAS",
        kpf_protocol::tx::MIN_FEE_RATE,
        units,
        kas(fee_att as i64),
        units11,
        kas(fee11 as i64),
        roll_units,
        kas(fee_roll as i64),
        kas(fee11 as i64 * 12_000)
    );
    assert!(fee_att > 0 && fee11 >= fee_att);
}

// ---------------------------------------------------------------- C1-F6, C1-F7: clocks

#[test]
fn c1_bounded_the_counterparty_restarts_the_quiet_time_of_an_assign_and_of_a_close() {
    // Unchanged by decision (review C1, C1-F6; review 12's F3): every party entry needs the pair UTXO to be quietDaa
    // old, and any party entry restarts that age. At the boundary the long's exit and the short's amend are both valid.
    let lt = epoch_epoch_table();
    let mut lp = opened();
    for _ in 0..3 {
        must("the long's assign at age 20", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 20, |_| {}));
        must("the long's close at age 20", &propose_tx(&lp, &lt, &Propose::close(&lt, LONG, SIZE, 1_050_000_000, 6_500, &long_kp()), |_| {}));
        let amend = amend_tx(&lp, SHORT, 0, 0, 0, &short_kp(), 20, |_| {});
        must("the short's amend that changes nothing, at age 20", &amend);
        lp = live(&amend, 0, lp.daa + 20, &lp.s.rest_of(SIZE));
        refused("the long's assign one DAA early on the amended pair", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 19, |_| {}), 0);
        refused("the long's close one DAA early", &propose_tx(&lp, &lt, &Propose { seq: 19, ..Propose::close(&lt, LONG, SIZE, 1_050_000_000, 6_500, &long_kp()) }, |_| {}), 0);
    }
    let (_, fee) = cost_of(&amend_tx(&lp, SHORT, 0, 0, 0, &short_kp(), 20, |_| {}));
    println!("C1-F6: an amend's relay floor {} KAS; one every 20 DAA is {} a day at 10 DAA/s", kas(fee as i64), kas(fee as i64 * 43_200));
}

#[test]
fn c1_bounded_a_cure_landing_after_the_lapse_date_is_settled_at_the_entry() {
    // Unchanged by decision (review C1, C1-F7; review 10's F2). The lease ends at 14,000 and nobody renewed it; the long
    // is liquidated with a signed price at 14,950 (valid: landed at 14,960). It cures at 15,005, after the lapse date and before
    // the finalize at 15,010.
    let lp = opened();
    let lt = epoch_table_for(14_950);
    let (lw, _) = proposed(&lp, &lt, &Propose::liq(&lt, LONG, SIZE, 928_000_000, 14_950), 14_960);
    assert!(lw.s.valid_at(14_960));
    let cure = cure_tx(&lw, &long_kp(), |_| {});
    must("the cure", &cure);
    let (cured, value) = lw.s.cure(lw.value).expect("a cure");
    let add = value - lw.value as i64;
    let lc = live(&cure, 1, 15_005, &cured);
    let lapse = lapse_tx(&lc, 15_005, |_| {});
    must("the lapse settle of the cured pair in the next block", &lapse);
    assert_eq!(paid_to(&lapse, &x(&long_kp())) - add, 1_000 * KAS1);
    assert_eq!(paid_to(&lapse, &x(&short_kp())), 2_000 * KAS1 + 20_000_000);
    // The winner's defences: the finalize at 15,010 if the loser does not cure, and a renewal of the cured pair if it
    // lands before the lapse settle (two spends of one UTXO).
    refused("the finalize before the grace", &finalize_tx(&lw, 49, |_| {}), 0);
    must("the finalize", &finalize_tx(&lw, 50, |_| {}));
    let late = epoch_table_for(15_004);
    must("a renewal of the cured pair", &keep_alive_tx(&lc, &late, &att(&late.s, 15_004, FEED), 15_005, |_| {}));
}

#[test]
fn c1_bounded_the_bounds_still_admit_markets_and_committees_a_wallet_must_refuse() {
    let base = cmarket();
    let with = |f: &dyn Fn(&mut CParams)| {
        let mut m = base;
        f(&mut m.p);
        m
    };
    // Refused now (review C1, C1-N1): lateDaa 0, a term below 1,200 DAA.
    assert!(with(&|p| p.late_daa = 0).check().is_err());
    assert!(with(&|p| p.term = 1_199).check().is_err());
    let m3 = with(&|p| p.term = 3);
    let lp = pair(&CPair { expiry: 6_400, ..sample_pair(&m3) }, 6_100);
    let lt = epoch_epoch_table();
    let a = att_of(&m3.id(), 6_500, &[(0, FEED), (1, FEED), (2, FEED)], FEED);
    refused("a pair of a market with a term of 3 DAA is not renewed", &keep_alive_tx(&lp, &lt, &a, 6_500, |_| {}), 0);
    // Still inside checkMarket.
    let odd = [
        ("a day of quiet, grace and lateness", with(&|p| { p.quiet_daa = 864_000; p.grace_daa = 864_000; p.late_daa = 864_000; })),
        ("a quiet time above a quarter of the term: a renewal restarts it before it ends", with(&|p| { p.term = 1_200; p.quiet_daa = 400; })),
        ("a keeper fee of 10,000 KAS at least", with(&|p| p.keeper_fee_min = 1_000_000_000_000)),
        ("no maintenance margin", with(&|p| p.mmr_bps = 0)),
        ("a carrier of 10 million KAS", with(&|p| p.carrier = 1_000_000_000_000_000)),
        ("a tolerance of one basis point", with(&|p| p.g_bps = 1)),
        ("no idle time", with(&|p| p.idle_daa = 0)),
    ];
    for (what, m) in &odd {
        assert!(m.check().is_ok(), "{what}");
    }
    // The least term: one time renews a pair once; the next renewal needs a signed price 300 DAA later.
    let m12 = with(&|p| p.term = 1_200);
    let lp = pair(&CPair { expiry: 6_400, ..sample_pair(&m12) }, 6_100);
    let a = att_of(&m12.id(), 6_500, &[(0, FEED), (1, FEED), (2, FEED)], FEED);
    let t = keep_alive_tx(&lp, &lt, &a, 6_500, |_| {});
    must("a renewal on the least term", &t);
    let next = live(&t, 0, 6_501, &lp.s.kept_alive(6_500).unwrap());
    refused("the same time again", &keep_alive_tx(&next, &epoch_table_after(&t, 1, &lt, 6_501), &a, 6_500, |_| {}), 0);
    // The committee's bounds.
    let rp = rparams();
    assert!(CommitteeParams { epoch: 99, ..rp }.check().is_err());
    assert!(CommitteeParams { spread_bps: 1_001, ..rp }.check().is_err());
    assert!(CommitteeParams { epoch_table_value: KAS1 - 1, ..rp }.check().is_err());
    assert!(CommitteeParams { quorum_bps: 5_001, epoch: 100, spread_bps: 1_000, epoch_table_value: 100 * KAS1, epoch_tables: 6, ..rp }.check().is_ok());
    assert!(CommitteeParams { spread_bps: 0, ..rp }.check().is_ok());
}

// ---------------------------------------------------------------- the committee's id

#[test]
fn c1_holds_no_two_objects_of_the_committee_id_are_spent_together_and_none_authors_another() {
    let r = committee(&Committee { p: rparams(), from: 5_000, members: table(TOP3), holds: kpf_protocol::committee::NO_HOLDS });
    let lt = epoch_epoch_table();
    let a = att(&lt.s, 6_500, FEED);
    let ex = Live { op: outpoint(3), value: 12 * KAS1 as u64, daa: 0, id: Some(RID), s: Exit { key: x(&member_kp(3)), amount: 12 * KAS1, release: 6_500, hold: kpf_protocol::committee::NO_HOLD, charge: rparams().charge } };
    let owner = key_spk(&ex.s.key);
    let attest_args = || a.args().into_iter().map(Arg::V).collect::<Vec<_>>();
    let both_fail = |what: &str, t: &T, i: usize, j: usize| {
        let v = run(&t.0, &t.1).expect("bindings");
        assert!(v[i].is_err() && v[j].is_err(), "{what}: {:?} {:?}", v[i], v[j]);
    };

    // A roll and an attest in one transaction: the committee at output 0, the epoch table continuing at 1, three new epoch tables.
    let (_, _, mut outs) = roll_outs(&r, 1);
    outs.insert(1, out(lt.value, epoch_table_spk(&lt.s), bind(1, RID)));
    let k = ck();
    let roll_in = committee_in(&r, "roll", vec![iv(1), bv(&k.epoch_table_t.prefix), bv(&k.epoch_table_t.suffix)]);
    both_fail("a roll with an attest", &build_ext(&[roll_in.clone(), epoch_table_in(&lt, "attest", attest_args()), exec_in(60)], outs, 6_500, &[]), 0, 1);

    // An attest and a withdraw: the epoch table at 0, the exit's payout at 1.
    let outs = vec![out(lt.value, epoch_table_spk(&lt.s), bind(0, RID)), out(ex.value, owner.clone(), None), change(1)];
    both_fail("an attest with a withdraw", &build_ext(&[epoch_table_in(&lt, "attest", attest_args()), exit_in(&ex, "withdraw", vec![]), exec_in(60)], outs, 6_500, &[]), 0, 1);

    // A retire and a withdraw (neither leaves an output of the id).
    let old = epoch_table(&EpochTable { from: 4_000, until: 5_000, ..lt.s });
    let outs = vec![change(1), out(ex.value, owner.clone(), None), change(1)];
    both_fail("a retire with a withdraw", &build_ext(&[epoch_table_in(&old, "retire", vec![]), exit_in(&ex, "withdraw", vec![]), exec_in(60)], outs, 6_500, &[]), 0, 1);

    // Two epoch tables attesting in one transaction.
    let mut lt2 = lt.clone();
    lt2.op = outpoint(4);
    let outs = vec![out(lt.value, epoch_table_spk(&lt.s), bind(0, RID)), out(lt.value, epoch_table_spk(&lt.s), bind(1, RID)), change(1)];
    // (Changed with the cross-margin account, docs/DESIGN-COMMITTEE.md 3b: two epoch tables of a committee attest side by side,
    // one a market, each continuing at its own output index. What must still hold is that no output of the id is made
    // and none is lost: as many outputs of the id as inputs, each epoch table's own.)
    let two = |outs: Vec<_>| build_ext(&[epoch_table_in(&lt, "attest", attest_args()), epoch_table_in(&lt2, "attest", attest_args()), exec_in(60)], outs, 6_500, &[]);
    must("two attests, each epoch table continuing at its own index", &two(outs.clone()));
    let mut third = outs.clone();
    third.push(out(KAS1 as u64, epoch_table_spk(&lt.s), bind(0, RID)));
    both_fail("two attests that write a third output of the id", &two(third), 0, 1);
    let merged = vec![out(2 * lt.value, epoch_table_spk(&lt.s), bind(0, RID)), change(1), change(1)];
    both_fail("two attests merged into one epoch table", &two(merged), 0, 1);
    let swapped = vec![out(lt.value, epoch_table_spk(&lt.s), bind(0, RID)), change(1), out(lt.value, epoch_table_spk(&lt.s), bind(1, RID))];
    assert!(!accepted(&two(swapped)), "two attests, one epoch table continuing at another index");

    // A forged epoch table (one member of the attacker's key holding all the weight) written beside a legitimate spend.
    let mut forged_table = EMPTY_TABLE;
    forged_table[0] = Member { key: x(&keypair(77)), weight: 100 * KAS1 };
    let forged = EpochTable { members: forged_table, total: 100 * KAS1, from: 6_000, until: 7_000, quorum_bps: 6_700, spread_bps: 50 };
    let forged_out = |auth: u16| out(KAS1 as u64, epoch_table_spk(&forged), bind(auth, RID));
    must("the attest alone", &attest_tx(&lt, &a, 6_500, |_| {}));
    must("the retire alone", &retire_tx(&old, 6_500, |_| {}));
    must("the withdraw alone", &withdraw_tx(&ex, 6_500, |_| {}));
    must("the top-up alone", &top_up_tx(&r, 1, KAS1, |_| {}));
    must("the roll alone", &roll_tx(&r, 1, 6_000, |_| {}));
    refused("an attest that also writes a forged epoch table", &attest_tx(&lt, &a, 6_500, |o| o.push(forged_out(0))), 0);
    refused("a retire that writes a forged epoch table", &retire_tx(&old, 6_500, |o| o.push(forged_out(0))), 0);
    refused("a withdraw that writes a forged epoch table", &withdraw_tx(&ex, 6_500, |o| o.push(forged_out(0))), 0);
    refused("a top-up that writes a forged epoch table", &top_up_tx(&r, 1, KAS1, |o| o.push(forged_out(0))), 0);
    let three = committee(&Committee { p: rparams(), from: 5_000, members: table([30, 22, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), holds: kpf_protocol::committee::NO_HOLDS });
    refused("a join of an empty member that writes one", &join_tx(&three, 5, x(&keypair(77)), 10 * KAS1, |o| o.push(forged_out(0))), 0);
    refused("an outbidding join that writes one more", &join_tx(&r, 7, x(&keypair(77)), 10 * KAS1, |o| o.push(forged_out(0))), 0);
    refused("a leave that writes one more", &leave_tx(&r, 1, &member_kp(1), |o| o.push(forged_out(0))), 0);
    refused("a roll that writes a fourth epoch table of another table", &roll_tx(&r, 1, 6_000, |o| o.push(forged_out(0))), 0);
    // A roll whose epoch tables are in order but whose third is the forged one, or one of another spread.
    refused("a roll whose third epoch table is forged", &roll_tx(&r, 1, 6_000, |o| o[3] = forged_out(0)), 0);
    let (_, honest, _) = roll_outs(&r, 1);
    let mut wider = honest;
    wider.spread_bps = 1_000;
    let wider_spk = epoch_table_spk(&wider);
    refused("a roll whose third epoch table has a wider spread", &roll_tx(&r, 1, 6_000, |o| o[3].script_public_key = wider_spk), 0);
    // The exit of a leave written as an epoch table, or the committee's continuation written second.
    refused("a leave whose exit is a forged epoch table", &leave_tx(&r, 1, &member_kp(1), |o| o[1] = out(o[1].value, epoch_table_spk(&forged), bind(0, RID))), 0);
    refused("a leave whose outputs are swapped", &leave_tx(&r, 1, &member_kp(1), |o| o.swap(0, 1)), 0);
}

#[test]
fn c1_holds_stake_leaves_the_committee_only_as_the_exit_of_its_own_member() {
    // Value conservation on every entry, one sompi at a time, and the release of a late committee.
    let r = committee(&Committee { p: rparams(), from: 5_000, members: table(TOP3), holds: kpf_protocol::committee::NO_HOLDS });
    let key = x(&keypair(77));
    // An outbidding join: member 7 (1 KAS) for 10 KAS.
    must("the join", &join_tx(&r, 7, key, 10 * KAS1, |_| {}));
    refused("the committee one sompi short", &join_tx(&r, 7, key, 10 * KAS1, |o| o[0].value -= 1), 0);
    refused("the exit one sompi short", &join_tx(&r, 7, key, 10 * KAS1, |o| o[1].value -= 1), 0);
    refused("a weight above the amount paid", &join_tx(&r, 7, key, 10 * KAS1, |o| {
        let (mut n, _, _) = r.s.join(7, key, 10 * KAS1).unwrap();
        n.members[7].weight += 1;
        o[0].script_public_key = committee_spk(&n);
    }), 0);
    // A leave: the committee keeps everything but the member's weight.
    refused("a leave that takes one sompi more", &leave_tx(&r, 1, &member_kp(1), |o| o[0].value -= 1), 0);
    refused("a leave that pays the exit to another key", &leave_tx(&r, 1, &member_kp(1), |o| o[1].script_public_key = exit_spk(&Exit { key, amount: 22 * KAS1, release: 6_500, hold: kpf_protocol::committee::NO_HOLD, charge: rparams().charge })), 0);
    // A roll moves no stake: the committee keeps its value, the roller funds the epoch tables.
    refused("a roll that pays an epoch table from the committee", &roll_tx(&r, 1, 6_000, |o| o[0].value -= KAS1 as u64), 0);
    refused("an epoch table one sompi short", &roll_tx(&r, 1, 6_000, |o| o[2].value -= 1), 0);
    // A late committee (nobody rolled since 5,000; the present is 9,000): the stake of a member that leaves is released at
    // 6,500, at once. Its member signed in no epoch table after [5,000, 6,000).
    let leave = leave_tx(&r, 1, &member_kp(1), |_| {});
    must("the leave at 9,000", &leave);
    let (_, e) = r.s.leave(1).unwrap();
    assert_eq!(e.release, 6_500);
    must("the withdraw in the next block", &withdraw_tx(&live(&leave, 1, 9_000, &e), 9_000, |_| {}));
    // A roll of several epochs at once, and one that lags behind on purpose; none ahead of the lock time.
    must("m = 4 at 9,000", &roll_tx(&r, 4, 9_000, |_| {}));
    must("m = 1 at 9,000: epoch tables of an epoch that is over", &roll_tx(&r, 1, 9_000, |_| {}));
    refused("m = 5 at 9,999", &roll_tx(&r, 5, 9_999, |_| {}), 0);
}

// ---------------------------------------------------------------- earlier attack classes

#[test]
fn c1_holds_two_pairs_never_claim_into_one_pending_part_and_no_claim_shares_its_id() {
    // Review 7: two pairs of one state proposing one liquidation into one output.
    let lt = epoch_epoch_table();
    let a_pair = pair(&sample_pair(&cmarket()), 6_100);
    let mut b_pair = a_pair.clone();
    b_pair.op = outpoint(20);
    b_pair.id = Some(Hash::from_bytes([0x58; 32]));
    let pr = Propose::liq(&lt, LONG, SIZE, 928_000_000, 6_500);
    let k = kit();
    let (w, fb) = pending_forced(&a_pair.s, &pr.c, pr.proposer);
    let args = |u: i64| vec![iv(SIZE), iv(pr.c.kind), iv(LONG), iv(pr.c.p), iv(6_500), iv(u), bv(&[0u8; 65]), iv(fb), iv(0), bv(&pr.proposer), bv(&k.pending.prefix), bv(&k.pending.suffix)];
    for maker in [a_pair.op, b_pair.op] {
        let mut outs = vec![out(a_pair.value, pend_spk(&w.made_by(&maker)), None), out(lt.value, epoch_table_spk(&lt.s), bind(1, RID)), change(1)];
        seal(&mut outs, 0, 0, a_pair.op);
        let t = build_ext(&[pair_in(&a_pair, "propose", args(1)), attest_in(&lt, &pr.a), pair_in(&b_pair, "propose", args(1)), exec_in(60)], outs, 6_500, &[]);
        let v = run(&t.0, &t.1).expect("bindings");
        assert!(v[0].is_err() || v[2].is_err(), "two pairs accepted one pending part");
    }
    // Review 6: the pending part's id also on another output of the same genesis.
    let w = w.made_by(&a_pair.op);
    let single = {
        let mut outs = vec![out(a_pair.value, pend_spk(&w), None), out(lt.value, epoch_table_spk(&lt.s), bind(1, RID)), change(1)];
        seal(&mut outs, 0, 0, a_pair.op);
        build_ext(&[pair_in(&a_pair, "propose", args(1)), attest_in(&lt, &pr.a), exec_in(60)], outs, 6_500, &[])
    };
    must("the claim alone", &single);
    let mut outs = vec![out(a_pair.value, pend_spk(&w), None), out(lt.value, epoch_table_spk(&lt.s), bind(1, RID)), out(a_pair.value, pend_spk(&w), None), change(1)];
    let g = genesis_id(a_pair.op, &outs, &[0, 2]);
    outs[0].covenant = bind(0, g);
    outs[2].covenant = bind(0, g);
    let t = build_ext(&[pair_in(&a_pair, "propose", args(1)), attest_in(&lt, &pr.a), exec_in(60)], outs, 6_500, &[]);
    refused("a pending part with a twin under its id", &t, 0);
    // The same for the part of an assign (the leaver signs the transaction as it is, twin included).
    let ek = x(&exec_kp());
    let (pay, part, pv, _) = handed(&a_pair, LONG, SIZE, 1_040_000_000, 900_000_000, ek, [0; 32], 0);
    let assign = |twin: bool| {
        let mut outs: Vec<TransactionOutput> = vec![out(pay as u64, key_spk(&a_pair.s.long_key), None), out(pv as u64, pair_spk(&part), None)];
        if twin {
            outs.push(out(pv as u64, pair_spk(&part), None));
        }
        outs.push(change(1));
        let group: Vec<usize> = if twin { vec![1, 2] } else { vec![1] };
        let g = genesis_id(a_pair.op, &outs, &group);
        for j in group {
            outs[j].covenant = bind(0, g);
        }
        build_ext(&[pair_in(&a_pair, "assign", vec![Arg::Sig(long_kp()), iv(LONG), iv(SIZE), iv(1_040_000_000), iv(900_000_000), bv(&ek), iv(1)]), exec_in(60)], outs, 0, &[20])
    };
    must("the assign", &assign(false));
    refused("an assigned part with a twin under its id", &assign(true), 0);
}

#[test]
fn c1_holds_two_inputs_never_share_a_payout_output() {
    // Pair A's short and pair B's long are one key (77). A settles at the entry at input 0, B at input 1. With two
    // outputs, output 1 would be A's short payout (the mirror index) and B's long payout: refused, a mirror index
    // must lie beyond the inputs.
    let k77 = x(&keypair(77));
    let a = pair(&CPair { short_key: k77, ..sample_pair(&cmarket()) }, 6_100);
    let mut b = pair(&CPair { long_key: k77, ..sample_pair(&cmarket()) }, 6_100);
    b.op = outpoint(20);
    b.id = Some(Hash::from_bytes([0x58; 32]));
    let (al, as_) = a.s.lapse_out(a.value);
    let (bl, bs) = b.s.lapse_out(b.value);
    let ins = || [pair_in(&a, "lapseSettle", vec![]), pair_in(&b, "lapseSettle", vec![])];
    let shared = vec![out(al as u64, key_spk(&a.s.long_key), None), out(as_.max(bl) as u64, key_spk(&k77), None)];
    let t = build_ext(&ins(), shared, 15_000, &[]);
    assert!(!accepted(&t), "one output paid two inputs");
    // Laid out in full it is accepted: A's long, B's long, B's short, A's short.
    let full = vec![out(al as u64, key_spk(&a.s.long_key), None), out(bl as u64, key_spk(&k77), None), out(bs as u64, key_spk(&b.s.short_key), None), out(as_ as u64, key_spk(&k77), None)];
    must("two lapse settles side by side", &build_ext(&ins(), full.clone(), 15_000, &[]));
    for j in 0..4 {
        let mut o = full.clone();
        o[j].value -= 1;
        assert!(!accepted(&build_ext(&ins(), o, 15_000, &[])), "output {j} one sompi short");
    }
    // An exit's payout and a pair's payout to the same key at the exit's index: each input has its own index.
    let ex = Live { op: outpoint(3), value: 1_000 * KAS1 as u64, daa: 0, id: Some(RID), s: Exit { key: a.s.long_key, amount: 1_000 * KAS1, release: 6_500, hold: kpf_protocol::committee::NO_HOLD, charge: rparams().charge } };
    let outs = vec![out(al as u64, key_spk(&a.s.long_key), None), change(1), out(as_ as u64, key_spk(&k77), None)];
    let t = build_ext(&[pair_in(&a, "lapseSettle", vec![]), exit_in(&ex, "withdraw", vec![])], outs, 15_000, &[]);
    let v = run(&t.0, &t.1).expect("bindings");
    assert!(v[0].is_ok() && v[1].is_err(), "the exit is not paid by the pair's payout: {:?}", v);
}

// ---------------------------------------------------------------- the epoch table's arithmetic, the committee's economics

#[test]
fn c1_holds_a_epoch_table_counts_signed_members_of_positive_weight_once_within_its_total() {
    let t0 = 6_500;
    let m = cmarket().id();
    let custom = |members: Table, total: i64| epoch_table(&EpochTable { members, total, from: 6_000, until: 7_000, quorum_bps: 6_700, spread_bps: 50 });
    let votes3 = [(0usize, FEED), (1, FEED), (2, FEED)];
    // A member that signed two prices for one time: either is usable, one per attestation (the claimant picks).
    let lt = epoch_epoch_table();
    must("member 2 at 10.00", &attest_tx(&lt, &fx(t0, &votes3, FEED), t0 as u64, |_| {}));
    must("member 2 at 10.01, the price claimed still 10.00", &attest_tx(&lt, &fx(t0, &[(0, FEED), (1, FEED), (2, FEED + 1_000_000)], FEED), t0 as u64, |_| {}));
    // A signature for another signed price, another price, another market, another committee.
    let mut a = fx(t0, &votes3, FEED);
    a.sigs[2] = sign_price(&member_kp(2), &rid(), &m, t0 + 1, FEED).sig;
    refused("a signature for another signed price", &attest_tx(&lt, &a, t0 as u64, |_| {}), 0);
    a.sigs[2] = sign_price(&member_kp(2), &rid(), &m, t0, FEED + 1).sig;
    refused("a signature for another price", &attest_tx(&lt, &a, t0 as u64, |_| {}), 0);
    a.sigs[2] = sign_price(&member_kp(2), &rid(), &[0x4e; 32], t0, FEED).sig;
    refused("a signature for another market", &attest_tx(&lt, &a, t0 as u64, |_| {}), 0);
    a.sigs[2] = sign_price(&member_kp(2), &[0x53; 32], &m, t0, FEED).sig;
    refused("a signature for another committee", &attest_tx(&lt, &a, t0 as u64, |_| {}), 0);
    a.sigs[2] = sign_price(&member_kp(3), &rid(), &m, t0, FEED).sig;
    refused("another member's signature", &attest_tx(&lt, &a, t0 as u64, |_| {}), 0);
    // Bits beyond the fifteen members, a signed price outside the window or ahead of the lock time.
    let mut a = fx(t0, &votes3, FEED);
    a.bits |= 1 << 15;
    refused("a sixteenth bit", &attest_tx(&lt, &a, t0 as u64, |_| {}), 0);
    refused("a signed price at the window's end", &attest_tx(&lt, &fx(7_000, &votes3, FEED), 7_000, |_| {}), 0);
    refused("a signed price before the window", &attest_tx(&lt, &fx(5_999, &votes3, FEED), 6_500, |_| {}), 0);
    refused("a signed price ahead of the lock time", &attest_tx(&lt, &fx(t0, &votes3, FEED), t0 as u64 - 1, |_| {}), 0);
    must("an old signed price of the window, long after", &attest_tx(&lt, &fx(6_000, &votes3, FEED), 7_999, |_| {}));
    // One key in two members counts with both weights (it holds both stakes).
    let mut twice = table(TOP3);
    twice[1].key = twice[0].key;
    let lt2 = custom(twice, 100 * KAS1);
    let mut a = fx(t0, &[(0, FEED), (2, FEED)], FEED);
    a.bits |= 1 << 1;
    a.prices[1] = FEED;
    a.sigs[1] = a.sigs[0];
    must("one key in members 0 and 1", &attest_tx(&lt2, &a, t0 as u64, |_| {}));
    // States only a forged lineage could hold: an empty or negative member in the bits, a total below the signed
    // weight, a total of zero, a spread beyond 100 %.
    let mut neg = table(TOP3);
    neg[3].weight = -12 * KAS1;
    let votes4 = [(0usize, FEED), (1, FEED), (2, FEED), (3, FEED)];
    refused("a member of negative weight signs", &attest_tx(&custom(neg, 76 * KAS1), &fx(t0, &votes4, FEED), t0 as u64, |_| {}), 0);
    let mut empty = table(TOP3);
    empty[3] = Member { key: x(&member_kp(3)), weight: 0 };
    refused("a member of no weight signs", &attest_tx(&custom(empty, 88 * KAS1), &fx(t0, &votes4, FEED), t0 as u64, |_| {}), 0);
    refused("more signed weight than the total", &attest_tx(&custom(table(TOP3), 60 * KAS1), &fx(t0, &votes3, FEED), t0 as u64, |_| {}), 0);
    refused("a total of zero", &attest_tx(&custom(table(TOP3), 0), &fx(t0, &votes3, FEED), t0 as u64, |_| {}), 0);
    let wide = epoch_table(&EpochTable { spread_bps: 10_001, ..lt.s });
    refused("a spread above 100 %", &attest_tx(&wide, &fx(t0, &votes3, FEED), t0 as u64, |_| {}), 0);
    // The largest table the committee writes: 15 members of 2.5 x 10^17 sompi; every member signs; no overflow. And the
    // largest price with the widest spread a committee may have: the products stay below 2^63.
    let mut big = table([1; MEMBERS]);
    for s in big.iter_mut() {
        s.weight = MEMBER_MAX;
    }
    let all: Vec<(usize, i64)> = (0..MEMBERS).map(|i| (i, FEED + i as i64)).collect();
    must("fifteen members at the member limit", &attest_tx(&custom(big, 15 * MEMBER_MAX), &fx(t0, &all, FEED + 7), t0 as u64, |_| {}));
    refused("and a price none of them is near", &attest_tx(&custom(big, 15 * MEMBER_MAX), &fx(t0, &all, FEED * 2), t0 as u64, |_| {}), 0);
    let widest = epoch_table(&EpochTable { spread_bps: 1_000, ..lt.s });
    let at = |price: i64, p: i64| attest_tx(&widest, &fx(t0, &[(0, price), (1, price), (2, price)], p), t0 as u64, |_| {});
    must("9 x 10^14 from prices 10 % below", &at(PRICE_LIMIT / 10 * 9, PRICE_LIMIT));
    refused("and from prices a sompi further", &at(PRICE_LIMIT / 10 * 9 - 1, PRICE_LIMIT), 0);
    refused("a signed price far above 9 x 10^14", &at(i64::MAX, PRICE_LIMIT), 0);
    refused("a negative signed price", &at(-1, 0), 0);
    // A price of zero is attested only by members that signed zero.
    must("zero from zeros", &at(0, 0));
    refused("zero from one sompi", &at(1, 0), 0);
}

#[test]
fn c1_fixed_five_minimum_stakes_bought_into_the_fixture_committee_dictate_nothing() {
    // Review C1 (c1_bounded_five_minimum_stakes_bought_into_the_fixture_committee_dictate_the_next_epoch): five 1-KAS members
    // outbid with 10 KAS each (50 KAS of 145, 34.5 %) and 48 KAS of honest signatures attested a price of one sompi in
    // the next epoch; the newcomer left with its whole stake.
    let mut l = committee(&Committee { p: rparams(), from: 5_000, members: table(TOP3), holds: kpf_protocol::committee::NO_HOLDS });
    let me = keypair(77);
    let mut evicted = 0;
    for i in 7..12 {
        let t = join_tx(&l, i, x(&me), 10 * KAS1, |_| {});
        must("a member of 1 KAS outbid with 10 KAS", &t);
        let (r, e, _) = l.s.join(i, x(&me), 10 * KAS1).unwrap();
        evicted += e.unwrap().amount;
        l = live(&t, 0, 5_900, &r);
    }
    assert_eq!((evicted, l.s.total()), (5 * KAS1, 145 * KAS1));
    let roll = roll_tx(&l, 1, 6_000, |_| {});
    must("the roll at 6,000", &roll);
    let (_, tk) = l.s.roll(1, 6_000, l.value as i64).unwrap();
    let lt = live(&roll, 1, 6_000, &tk);
    assert_eq!(tk.quorum(), 97 * KAS1 + 15_000_000);
    let market = cmarket().id();
    let with_mine = |honest: &[(usize, i64)], mine: i64, p: i64| {
        let mut a = fx(6_500, honest, p);
        for i in 7..12 {
            a.bits |= 1 << i;
            a.prices[i] = mine;
            a.sigs[i] = sign_price(&me, &rid(), &market, 6_500, mine).sig;
        }
        a
    };
    // The step that used to succeed: its five members sign 1 sompi, honest members 0, 2, 5 and 12 (48 KAS) signed the feed.
    let honest = [(0usize, FEED), (2, FEED), (5, FEED), (12, FEED)];
    let a = with_mine(&honest, 1, 1);
    assert!(a.check(&tk, &rid(), 6_500).is_err());
    refused("the next epoch's epoch table attests one sompi", &attest_tx(&lt, &a, 6_500, |_| {}), 0);
    // What 34.5 % buys now: nothing a claimant with honest signatures alone could not attest.
    must("the edge of the feed's band with its members signing there", &attest_tx(&lt, &with_mine(&honest, FEED_HI, FEED_HI), 6_500, |_| {}));
    refused("a sompi beyond it", &attest_tx(&lt, &with_mine(&honest, FEED_HI + 1, FEED_HI + 1), 6_500, |_| {}), 0);
}

#[test]
fn c1_bounded_a_keeper_fee_floor_above_the_carrier_pays_for_liquidating_in_the_smallest_parts() {
    // Unchanged (review C1, C1-N1). A market of the fixture's parameters with a keeper fee of at least 5 KAS. The long
    // of the worked pair is liquidated at 9.28: whole, the fee is 0.5 % of the notional, 46.4 KAS. A part of one unit
    // pays the floor capped by that unit's maintenance slice: 0.4737 KAS a unit, ten times the rate, against a carrier
    // of 0.2 KAS.
    let m5 = CMarket { p: CParams { keeper_fee_min: 500_000_000, ..cmarket().p }, ..cmarket() };
    let lp = pair(&sample_pair(&m5), 6_100);
    let lt = epoch_epoch_table();
    let low = 928_000_000;
    let a = att_of(&m5.id(), 6_500, &[(0, low), (1, low), (2, low)], low);
    assert_eq!(lp.s.fee_of(SIZE, LONG, low), 46 * KAS1 + 40_000_000);
    let unit = 1_000_000;
    assert_eq!(lp.s.fee_of(unit, LONG, low), 47_368_421);
    let pr = liq_with(a, LONG, unit, x(&exec_kp()), 6_500);
    let (lw, rest) = proposed(&lp, &lt, &pr, 6_510);
    must("the one-unit part forfeits", &finalize_tx(&lw, 50, |_| {}));
    assert_eq!((lw.s.fee, lw.s.refund), (47_368_421, 20_000_000));
    assert_eq!(rest.unwrap().s.size, SIZE - unit);
    println!("C1-N1: 1,000 one-unit liquidations pay {} KAS of fees and cost {} KAS of carriers; one whole liquidation pays 46.4 KAS", kas(47_368_421 * 1_000), kas(20_000_000 * 1_000));
}
