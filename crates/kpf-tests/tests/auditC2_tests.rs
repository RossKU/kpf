//! Review C2 of the committee family (docs/audit/REVIEW-C2.md): the family as fixed after review C1, attacked again.
//! The attacks of review C1 and what became of them are in `auditC1_tests.rs` (`c1_fixed_*`) and the walks in
//! `auditC1_walk_tests.rs`.
//!
//! `c2_exploit_*`: the engine accepts a transaction that takes, freezes or breaks a rule the note states.
//! `c2_bounded_*`: an accepted behaviour whose cost or damage is measured here and that the note should state.
//! `c2_holds_*`: an attack that the contracts refuse.
//!
//! Fixture (`committeekit`): the committee's epoch [6,000, 7,000), 100 KAS of weight in 15 members (30, 22, 15, 12, 8, 2, 2,
//! seven of 1, and 2), a quorum of 67 % within a spread of 0.5 %; the market: a lease of 8,000 DAA, a lapse 1,000 after
//! it, 50 DAA to cure, a claim valid within 100 DAA of its time, 20 DAA of quiet, mmr 5 %, g 2 %, a fee of 0.5 %, a
//! carrier of 0.2 KAS. The worked pair: 1,000 units at 10 KAS, the long's bound at 9, the short's at 12.
#![allow(non_snake_case)]

mod committeekit;
mod common;

use committeekit::*;
use common::*;
use kaspa_consensus_core::tx::TransactionInput;
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::*;
use kpf_protocol::committee::*;
use kpf_protocol::oracle::{maker_of, Makers};
use kpf_protocol::state::{KIND_CLOSE, KIND_LIQ, LONG, SHORT};
use kpf_protocol::verify::{run_raw, Meter};

const KAS1: i64 = 100_000_000;
const SIZE: i64 = 1_000_000_000;
const FEED: i64 = 1_000_000_000;
/// The edges of what a quorum that signed 10 KAS attests at a spread of 0.5 %.
const FEED_HI: i64 = 1_005_025_125;
const FEED_LO: i64 = 995_024_876;

// ---------------------------------------------------------------- helpers

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

fn paid_to(t: &T, key: &[u8; 32]) -> i64 {
    let spk = key_spk(key);
    t.0.outputs.iter().filter(|o| o.script_public_key == spk && o.covenant.is_none()).map(|o| o.value as i64).sum()
}

/// The relay fee floor of the transaction at the node's minimum rate, each input's compute budget committed as the
/// tool's builder commits it (an estimate).
fn fee_of(t: &T) -> i64 {
    let units = run_raw(&t.0, &t.1, Meter::Unlimited).expect("bindings");
    let mut tx = t.0.clone();
    for (i, u) in units.iter().enumerate() {
        let inp = &t.0.inputs[i];
        tx.inputs[i] = TransactionInput::new_with_compute_budget(inp.previous_outpoint, inp.signature_script.clone(), inp.sequence, kpf_protocol::tx::budget_for_units(*u.as_ref().expect("an accepted input")));
    }
    kpf_protocol::tx::min_fee(&kpf_protocol::tx::masses(&tx, &t.1), kpf_protocol::tx::MIN_FEE_RATE) as i64
}

fn kas(v: i64) -> String {
    format!("{}.{:08}", v / KAS1, v % KAS1)
}

fn epoch_table_for(t0: i64) -> LEpochTable {
    let from = t0 / 1_000 * 1_000;
    epoch_table(&EpochTable { members: table(TOP3), total: 100 * KAS1, from, until: from + 1_000, quorum_bps: 6_700, spread_bps: 50 })
}

const HONEST3: [(usize, i64); 3] = [(0, FEED), (1, FEED), (2, FEED)];

// ---------------------------------------------------------------- C2-F1: the spread and the tolerance

#[test]
fn c2_fixed_a_committee_whose_spread_exceeds_a_quarter_of_a_markets_tolerance_prices_none_of_its_pairs() {
    // Review C2 (c2_exploit_on_a_committee_whose_spread_exceeds_a_markets_tolerance_honest_signatures_take_a_healthy_side):
    // a committee with a spread of 10 %, the fixture market (g 2 %), every member signing 10.00. 9.0909 was attested and a
    // keeper forfeited the long's 1,000 KAS for a fee of 45.45 (the short got 2,954.75); 11.1111 was attested and the
    // long closed for 1,888.89 KAS instead of 1,000; the short closed for 2,727.27 instead of 2,000. Now the pair
    // reads the epoch table's spread and requires 4 x spread <= g.
    let wide = CommitteeParams { spread_bps: 1_000, ..rparams() };
    assert!(wide.check().is_ok() && cmarket().check().is_ok() && !cmarket().spread_ok(1_000));
    let r = committee(&Committee { p: wide, from: 5_000, members: table(TOP3), holds: kpf_protocol::committee::NO_HOLDS });
    let roll = roll_tx(&r, 1, 6_000, |_| {});
    must("the committee writes epoch tables with its spread", &roll);
    let (_, tk) = r.s.roll(1, 6_000, r.value as i64).unwrap();
    let lt = live(&roll, 1, 6_000, &tk);
    let lp = opened();
    let (lo, hi) = (909_090_910, 1_111_111_111);
    // The epoch table still attests the two prices; it knows no market's tolerance.
    must("9.0909 from signatures of 10.00", &attest_tx(&lt, &fx(6_500, &HONEST3, lo), 6_500, |_| {}));
    must("11.1111 from signatures of 10.00", &attest_tx(&lt, &fx(6_500, &HONEST3, hi), 6_500, |_| {}));
    // The steps that used to succeed: the three claims. The pair refuses, the epoch table does not.
    let keeper = x(&keypair(77));
    assert!(lp.s.long_liquidatable(lo));
    for (what, pr) in [
        ("the liquidation of the healthy long", liq_with(fx(6_500, &HONEST3, lo), LONG, SIZE, keeper, 6_500)),
        ("the long's close above the feed", close_with(fx(6_500, &HONEST3, hi), LONG, SIZE, &long_kp(), 6_500)),
        ("the short's close below the feed", close_with(fx(6_500, &HONEST3, lo), SHORT, SIZE, &short_kp(), 6_500)),
        ("a claim at the feed itself", close_with(fx(6_500, &HONEST3, FEED), LONG, SIZE, &long_kp(), 6_500)),
    ] {
        let t = propose_tx(&lp, &lt, &pr, |_| {});
        let v = run(&t.0, &t.1).expect("bindings");
        assert!(v[0].is_err() && v[1].is_ok(), "{what}: pair {:?}, epoch table {:?}", v[0], v[1]);
    }
    // Nor is a pair of that market renewed with such an epoch table: on this committee the market is dead (its pairs leave by
    // the entries that need no price, and by the lapse).
    let mut late = tk;
    (late.from, late.until) = (8_000, 9_000);
    let late = epoch_table(&late);
    refused("a renewal", &keep_alive_tx(&lp, &late, &att(&late.s, 8_000, FEED), 8_000, |_| {}), 0);
    must("the lapse settle", &lapse_tx(&lp, 15_000, |_| {}));
    must("an assign", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 20, |_| {}));
    // The margin to the basis point: the fixture is exactly at it (spread 50, g 200).
    let fixture = epoch_epoch_table();
    must("4 x 50 = 200", &propose_tx(&lp, &fixture, &close_with(fx(6_500, &HONEST3, FEED), LONG, SIZE, &long_kp(), 6_500), |_| {}));
    let mut one_more = fixture.s;
    one_more.spread_bps = 51;
    let one_more = epoch_table(&one_more);
    refused("a spread of 51 against a tolerance of 200", &propose_tx(&lp, &one_more, &close_with(fx(6_500, &HONEST3, FEED), LONG, SIZE, &long_kp(), 6_500), |_| {}), 0);
    let m199 = CMarket { p: CParams { g_bps: 199, ..cmarket().p }, ..cmarket() };
    let lp199 = pair(&sample_pair(&m199), 6_100);
    refused("a tolerance of 199 against a spread of 50", &propose_tx(&lp199, &fixture, &close_with(att_of(&m199.id(), 6_500, &HONEST3, FEED), LONG, SIZE, &long_kp(), 6_500), |_| {}), 0);
}

#[test]
fn c2_bounded_the_claimants_choice_of_price_uses_a_quarter_of_the_tolerance_at_most() {
    // Review C2 (c2_bounded_the_claimants_choice_of_price_uses_up_the_tolerance): with g equal to the spread (0.5 %) a
    // closer claiming the band's upper edge was paid the feed less one sompi a unit (the 50 KAS of g gone), and a long
    // at its maintenance was forfeited on a dip of 0.001 %. Such a market has no claim now.
    let lt = epoch_epoch_table();
    let votes = |p: i64| [(0usize, p), (1, p), (2, p)];
    let m50 = CMarket { p: CParams { g_bps: 50, ..cmarket().p }, ..cmarket() };
    let lp50 = pair(&sample_pair(&m50), 6_100);
    refused("a close on a market whose g equals the spread", &propose_tx(&lp50, &lt, &close_with(att_of(&m50.id(), 6_500, &votes(FEED), FEED_HI), LONG, SIZE, &long_kp(), 6_500), |_| {}), 0);
    // The bound at the least margin (the fixture: spread 0.5 %, g 2 %). A close: the long claims the band's upper edge
    // and is paid floor(10.05025125 x 0.98) = 9.84924622: 150.75 KAS below its value at the feed, where the honest
    // attestation of the feed pays 200 KAS below. A quarter of g is the claimant's.
    let lp = opened();
    let paid = |p: i64| {
        let (cw, _) = proposed(&lp, &lt, &close_with(fx(6_500, &votes(FEED), p), LONG, SIZE, &long_kp(), 6_500), 6_510);
        let CSettlement::Close { long, .. } = cw.s.settlement(cw.value, 6_510) else { panic!("a close") };
        lp.s.long_at(FEED) - long
    };
    assert_eq!((paid(FEED), paid(FEED_HI)), (200 * KAS1, 150_753_78_000));
    // A liquidation: a long opened at its maintenance (bound 9.5 at 10). With g inside the trigger it is liquidated
    // from 9.8039 down. At a feed of 9.85 the band's lower edge (9.80099503) liquidates it: 1.5 % of tolerance, not 2.
    let tight = pair(&CPair { bl: 950_000_000, ..sample_pair(&cmarket()) }, 6_100);
    let feed = 985_000_000;
    let lo = ((feed as i128 * 10_000 + 10_049) / 10_050) as i64;
    assert!(!tight.s.long_liquidatable(feed) && tight.s.long_liquidatable(lo) && !tight.s.long_liquidatable(((986_000_000i128 * 10_000 + 10_049) / 10_050) as i64));
    let (lw, _) = proposed(&tight, &lt, &liq_with(fx(6_500, &votes(feed), lo), LONG, SIZE, x(&exec_kp()), 6_500), 6_510);
    must("the long at its maintenance is forfeited at 9.85", &finalize_tx(&lw, 50, |_| {}));
    // The testnet defaults: spread 0.1 %, g 0.5 %: a margin of 5. A claimant has 0.1 %; members that differ by the most
    // one band holds (0.2 %) put the lowest claim 0.3 % below the highest signed price; 0.2 % is left for the feed's
    // move within lateDaa (10 s).
    let (s, g) = (CommitteeParams::testnet().spread_bps, CMarket::testnet([1; 32], [2; 32]).p.g_bps);
    assert_eq!((s, g), (10, 50));
    assert!(CMarket::testnet([1; 32], [2; 32]).spread_ok(s) && !CMarket::testnet([1; 32], [2; 32]).spread_ok(13));
}

#[test]
fn c2_fixed_the_library_finds_a_price_between_two_groups_of_signed_prices() {
    // Review C2 (c2_bounded_honest_prices_wider_than_the_spread_attest_nothing_and_the_library_misses_a_price_between_two):
    // member 0 (30 KAS) signs 10.00, members 1 and 2 (37 KAS) sign 10.08, 0.8 % apart at a spread of 0.5 %; the epoch table
    // attested 10.04 and `Attestation::collect` answered "below the quorum".
    let lt = epoch_epoch_table();
    let m = cmarket().id();
    let votes = [(0usize, FEED), (1, 1_008_000_000), (2, 1_008_000_000)];
    must("a price between two groups of signed prices", &attest_tx(&lt, &fx(6_500, &votes, 1_004_000_000), 6_500, |_| {}));
    let signed = |v: &[(usize, i64)]| -> Vec<SignedPrice> { v.iter().map(|(i, p)| sign_price(&member_kp(*i), &rid(), &m, 6_500, *p)).collect() };
    let a = Attestation::collect(&lt.s, &rid(), &m, 6_500, &signed(&votes)).expect("the collector finds a price");
    assert!(a.p > FEED && a.p < 1_008_000_000 && a.bits == 0b111);
    must("what the collector claims", &attest_tx(&lt, &a, 6_500, |_| {}));
    // The widest two prices one price serves: (1 + s) / (1 - s) apart, 1.005 % at 0.5 %; 0.2 % at the testnet's 0.1 %.
    let widest = (FEED_HI as i128 * 10_050 / 10_000) as i64;
    assert_eq!(widest, 1_010_050_250);
    let edge = [(0usize, FEED), (1, widest), (2, widest)];
    must("the widest two prices", &attest_tx(&lt, &fx(6_500, &edge, FEED_HI), 6_500, |_| {}));
    assert_eq!(Attestation::collect(&lt.s, &rid(), &m, 6_500, &signed(&edge)).expect("found at the edge").p, FEED_HI);
    // A sompi wider nothing is attested, by the epoch table or by the collector, although every member signed.
    let wider = [(0usize, FEED), (1, widest + 1), (2, widest + 1)];
    for p in [FEED_HI - 1, FEED_HI, FEED_HI + 1] {
        refused("a sompi wider", &attest_tx(&lt, &fx(6_500, &wider, p), 6_500, |_| {}), 0);
    }
    assert!(Attestation::collect(&lt.s, &rid(), &m, 6_500, &signed(&wider)).is_err());
}

#[test]
fn c2_bounded_a_third_that_a_quorum_needs_pins_the_price_at_the_edge_of_the_spread() {
    // 66 KAS of honest weight sign the feed (members 0, 1, 3 and 14); a quorum needs one more member. Member 2 (15 KAS) signs
    // the highest price that still shares a band with the feed: every attestation that includes it claims the band's
    // upper edge, and no other attestation exists for the time. A needed third moves the price by the spread, no more.
    let lt = epoch_epoch_table();
    let honest = [(0usize, FEED), (1, FEED), (3, FEED), (14, FEED)];
    let top = (FEED_HI as i128 * 10_050 / 10_000) as i64;
    let with = |p2: i64| -> Vec<(usize, i64)> { honest.iter().copied().chain([(2usize, p2)]).collect() };
    refused("the honest 66 KAS alone", &attest_tx(&lt, &fx(6_500, &honest, FEED), 6_500, |_| {}), 0);
    refused("the feed, with member 2 at the top", &attest_tx(&lt, &fx(6_500, &with(top), FEED), 6_500, |_| {}), 0);
    must("the band's upper edge", &attest_tx(&lt, &fx(6_500, &with(top), FEED_HI), 6_500, |_| {}));
    refused("a sompi below it", &attest_tx(&lt, &fx(6_500, &with(top), FEED_HI - 1), 6_500, |_| {}), 0);
    for p in [FEED_HI, FEED_HI + 1] {
        refused("member 2 a sompi higher: nothing", &attest_tx(&lt, &fx(6_500, &with(top + 1), p), 6_500, |_| {}), 0);
    }
    assert_eq!(FEED_HI - FEED, 5_025_125);
}

// ---------------------------------------------------------------- the bound of a signed price against the pair UTXO

#[test]
fn c2_holds_the_bound_of_a_price_refuses_no_claim_that_could_be_valid_and_none_on_an_unaccepted_pair() {
    // A claim is valid iff it lands within lateDaa of its time; it lands no earlier than the pair UTXO was accepted. So
    // a valid claim always passes `t0 + lateDaa >= born`: on numbers, every (born, t0, landing).
    let lp = opened();
    for born in [0u64, 6_100, 6_499, 6_500, 6_600] {
        for t0 in 6_300..6_700i64 {
            for land in born.max(t0 as u64 + 1)..born.max(t0 as u64 + 1) + 3 {
                if land as i64 - t0 <= 100 {
                    assert!(lp.s.price_fresh(t0, born), "born {born}, t0 {t0}, landing {land}");
                }
            }
        }
    }
    // A party's respend does not take a recent signed price from a liquidator: the short amends at 6,520 (the pair's age 20 from
    // 6,500); a signed price of 6,480, before the respend, still liquidates on the new UTXO.
    let lt = epoch_epoch_table();
    let aged = Live { daa: 6_500, ..lp.clone() };
    let amend = amend_tx(&aged, SHORT, 0, 0, 0, &short_kp(), 20, |_| {});
    must("the short's amend", &amend);
    let fresh = live(&amend, 0, 6_520, &lp.s.rest_of(SIZE));
    let pr = Propose { lock: 6_521, ..Propose::liq(&lt, LONG, SIZE, 928_000_000, 6_480) };
    let (lw, _) = proposed(&fresh, &lt, &pr, 6_521);
    assert!(lw.s.valid_at(6_521));
    // An unaccepted pair UTXO (the mempool's DAA score) carries no claim of either kind: nothing is chained on a
    // restore or on a party's respend before a block holds it. A renewal reads no DAA score and is not held back.
    let pending_utxo = Live { daa: u64::MAX, ..fresh.clone() };
    refused("a liquidation chained on an unaccepted pair", &propose_tx(&pending_utxo, &lt, &pr, |_| {}), 0);
    refused("a close chained on an unaccepted pair", &propose_tx(&pending_utxo, &lt, &Propose { lock: 6_521, seq: 0, ..Propose::close(&lt, LONG, SIZE, 1_050_000_000, 6_500, &long_kp()) }, |_| {}), 0);
    let late = epoch_table_for(8_000);
    must("a renewal chained on an unaccepted pair", &keep_alive_tx(&pending_utxo, &late, &att(&late.s, 8_000, FEED), 8_000, |_| {}));
    // The edge: a signed price exactly lateDaa older than the UTXO passes, one more does not.
    let edge = Live { daa: 6_580, ..lp.clone() };
    must("t0 = born - lateDaa", &propose_tx(&edge, &lt, &Propose { lock: 6_590, ..Propose::liq(&lt, LONG, SIZE, 928_000_000, 6_480) }, |_| {}));
    refused("t0 = born - lateDaa - 1", &propose_tx(&edge, &lt, &Propose { lock: 6_590, ..Propose::liq(&lt, LONG, SIZE, 928_000_000, 6_479) }, |_| {}), 0);
}

// ---------------------------------------------------------------- the clamp at priceMax

#[test]
fn c2_bounded_at_priceMax_a_short_is_liquidated_only_if_the_maintenance_exceeds_the_tolerance() {
    let lt = epoch_epoch_table();
    let votes = |p: i64| [(0usize, p), (1, p), (2, p)];
    // priceMax 13 KAS, the fixture's mmr 5 % and g 2 %: a short whose bound is priceMax itself is liquidated by any
    // attested price from 13 / 0.98 / 1.05 up, counted as 13 above it; at exactly priceMax and one sompi above alike.
    let m = CMarket { p: CParams { price_max: 1_300_000_000, ..cmarket().p }, ..cmarket() };
    let lp = pair(&CPair { bs: 1_300_000_000, ..sample_pair(&m) }, 6_100);
    for a in [1_300_000_000i64, 1_300_000_001, 5_000_000_000, PRICE_LIMIT] {
        let (lw, _) = proposed(&lp, &lt, &liq_with(att_of(&m.id(), 6_500, &votes(a), a), SHORT, SIZE, x(&exec_kp()), 6_500), 6_510);
        assert_eq!(lw.s.quote, 1_300_000_000);
        assert_eq!(lw.s.cure(lw.value), None);
    }
    refused("a liquidation of the long at a price above priceMax", &propose_tx(&lp, &lt, &liq_with(att_of(&m.id(), 6_500, &votes(5_000_000_000), 5_000_000_000), LONG, SIZE, x(&exec_kp()), 6_500), |_| {}), 0);
    refused("the short's close at a price above priceMax", &propose_tx(&lp, &lt, &close_with(att_of(&m.id(), 6_500, &votes(5_000_000_000), 5_000_000_000), SHORT, SIZE, &short_kp(), 6_500), |_| {}), 0);
    // A market whose maintenance (1 %) is below its tolerance (2 %): the same short is never liquidated, whatever the
    // feed, and the long's close is paid at 13 x 0.98 = 12.74: 260 KAS of a range it owns whole stay with the short.
    let m1 = CMarket { p: CParams { mmr_bps: 100, ..m.p }, ..m };
    assert!(m1.check().is_ok());
    let lp = pair(&CPair { bs: 1_300_000_000, ..sample_pair(&m1) }, 6_100);
    let a = att_of(&m1.id(), 6_500, &votes(PRICE_LIMIT), PRICE_LIMIT);
    assert!(!lp.s.short_liquidatable(1_300_000_000));
    refused("the short at priceMax is not liquidated at any price", &propose_tx(&lp, &lt, &liq_with(a.clone(), SHORT, SIZE, x(&exec_kp()), 6_500), |_| {}), 0);
    let (cw, _) = proposed(&lp, &lt, &close_with(a, LONG, SIZE, &long_kp(), 6_500), 6_510);
    let CSettlement::Close { long, short } = cw.s.settlement(cw.value, 6_510) else { panic!("a close") };
    assert_eq!((long, short), (3_740 * KAS1, 260 * KAS1 + 20_000_000));
}

// ---------------------------------------------------------------- take's lease rule and a listed side

#[test]
fn c2_bounded_a_listed_side_with_less_than_half_a_term_left_is_renewed_before_a_young_order_takes_it() {
    // The worked pair (lease 14,000), its long listed at 9.9. An order accepted at 10,500 needs a lease of 14,500.
    let listed = pair(&CPair { long_ask: 990_000_000, long_min: 1_000_000, ..sample_pair(&cmarket()) }, 6_100);
    let lo = order(LONG, &keypair(43), 13, 10_500);
    refused("the order does not take a lease of 14,000", &take_tx(&listed, &lo, SIZE, FEED, 10_600, 20, |_| {}), 1);
    // A signing taker is not held back, nor is an order that rested since 10,000.
    must("a transfer to a key", &transfer_tx(&listed, LONG, SIZE, FEED, 900_000_000, 20, |_| {}));
    must("an order accepted at 10,000", &take_tx(&listed, &order(LONG, &keypair(43), 13, 10_000), SIZE, FEED, 10_600, 20, |_| {}));
    // Anyone renews the pair with a signed price of the present (it is inside its renewal window whenever the order refuses
    // it); the order takes it once the renewed UTXO is quietDaa old.
    let lt = epoch_table_for(10_600);
    let renew = keep_alive_tx(&listed, &lt, &att(&lt.s, 10_600, FEED), 10_600, |_| {});
    must("the renewal", &renew);
    let renewed = live(&renew, 0, 10_601, &listed.s.kept_alive(10_600).unwrap());
    assert_eq!((renewed.s.expiry, renewed.s.long_ask), (18_600, 990_000_000));
    refused("before the quiet time of the renewed pair", &take_tx(&renewed, &lo, SIZE, FEED, 10_620, 19, |_| {}), 0);
    must("the order takes the renewed pair", &take_tx(&renewed, &lo, SIZE, FEED, 10_621, 20, |_| {}));
    // Without a committee there is no renewal: then only orders older than the lease less half a term, and signing
    // takers, can take the side.
}

// ---------------------------------------------------------------- payouts below the dust floor

#[test]
fn c2_bounded_a_payout_below_the_dust_floor_goes_to_the_other_side_and_is_worth_less_than_its_transaction() {
    let lt = epoch_epoch_table();
    let votes = |p: i64| [(0usize, p), (1, p), (2, p)];
    // (a) A close of a part. A market with a minimum size of 0.02 unit; a pair of 1,000 units, the long's bound at 0 and
    // the short's at 20, entered at 10 (each side holds 0.2 KAS in a part of 0.02 unit, the part floor). At 19.41 the
    // short has 0.98 KAS a unit left and is not liquidatable. The long closes 0.02 unit: the short's share of the part,
    // 0.01956 KAS, is below the dust floor and goes to the closer.
    let m = CMarket { p: CParams { min_size: 20_000, ..cmarket().p }, ..cmarket() };
    let lp = pair(&CPair { bl: 0, bs: 2_000_000_000, ..sample_pair(&m) }, 6_100);
    let price = 1_941_000_000;
    assert!(!lp.s.short_liquidatable(price) && lp.s.split_ok(20_000) && !lp.s.split_ok(19_999));
    let pr = close_with(att_of(&m.id(), 6_500, &votes(price), price), LONG, 20_000, &long_kp(), 6_500);
    let claim = propose_tx(&lp, &lt, &pr, |_| {});
    let (cw, _) = proposed(&lp, &lt, &pr, 6_510);
    let fin = finalize_tx(&cw, 0, |_| {});
    must("the close of 0.02 unit", &fin);
    assert_eq!(cw.s.settlement(cw.value, 6_510), CSettlement::Close { long: 60_000_000, short: 0 });
    let short_share = 20_000 * (2_000_000_000 - cw.s.close_price()) / 1_000_000;
    assert_eq!(short_share, 1_956_400);
    assert_eq!(paid_to(&fin, &x(&long_kp())), 60_000_000);
    assert_eq!(paid_to(&fin, &x(&short_kp())), 0);
    let fees = fee_of(&claim) + fee_of(&fin);
    assert!(fees > short_share, "taking {short_share} sompi costs {fees}");
    // (b) A lapse. A pair whose long holds less than the dust floor at the entry (a state no order, hand-over or cure
    // of this family writes: every piece keeps 0.2 KAS of each side at the entry): the long is paid nothing.
    let thin = pair(&CPair { entry: 900_001_000, ..sample_pair(&cmarket()) }, 6_100);
    assert_eq!(thin.s.lapse_out(thin.value), (0, thin.value as i64));
    let lapse = lapse_tx(&thin, 15_000, |_| {});
    must("the lapse of a pair whose long holds 0.01 KAS at the entry", &lapse);
    assert_eq!(paid_to(&lapse, &x(&long_kp())), 0);
    assert!(!sample_pair(&cmarket()).split_ok(100_000) && CPair { entry: 900_001_000, ..sample_pair(&cmarket()) }.split_ok(SIZE));
    // (c) An assign at a price a sompi above the leaver's bound: the leaver's payout of 0.00001 KAS stays in the part
    // (the leaver signed it).
    let lp = opened();
    let t = assign_tx(&lp, LONG, SIZE, 900_001_000, 850_000_000, &long_kp(), 20, |_| {});
    must("an assign whose payout is below the dust floor", &t);
    assert_eq!(paid_to(&t, &x(&long_kp())), 0);
    println!("C2 (dust): a part's close takes {} KAS of the other side for {} KAS of relay fees", kas(short_share), kas(fees));
}

// ---------------------------------------------------------------- parameters a wallet must still refuse

#[test]
fn c2_bounded_a_quiet_time_of_a_quarter_term_or_more_lets_anyone_hold_a_pair_by_renewing_it() {
    // A market inside checkMarket: term 1,200, quiet 400. A renewal needs no quiet time and moves the lease by a quarter
    // of a term at least, so one is possible every 300 DAA; every party entry needs 400 DAA of age.
    let m = CMarket { p: CParams { term: 1_200, quiet_daa: 400, ..cmarket().p }, ..cmarket() };
    assert!(m.check().is_ok());
    let votes = [(0usize, FEED), (1, FEED), (2, FEED)];
    let mut lp = pair(&CPair { expiry: 6_400, ..sample_pair(&m) }, 6_000);
    let mut t0 = 6_500i64;
    let mut fee = 0;
    for _ in 0..4 {
        let lt = epoch_table_for(t0);
        let t = keep_alive_tx(&lp, &lt, &att_of(&m.id(), t0, &votes, FEED), t0 as u64, |_| {});
        must("a renewal 300 DAA after the last", &t);
        fee = fee_of(&t);
        lp = live(&t, 0, t0 as u64 + 1, &lp.s.kept_alive(t0).unwrap());
        // 299 DAA later (when the next renewal lands) no party entry has been possible.
        refused("the long's assign at the pair's age 399", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 399, |_| {}), 0);
        must("it needs the age 400", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 400, |_| {}));
        t0 += 300;
    }
    println!("C2-N: on a market with quietDaa >= termDaa / 4 one renewal every quarter term ({} KAS of relay fee) keeps every party entry out; the lease never ends either", kas(fee));
}

#[test]
fn c2_bounded_a_roll_costs_its_roller_the_epoch_tables_and_whoever_retires_them_keeps_them() {
    // Unchanged by decision (review C1, C1-N2), with the epoch tables five times dearer: the roller funds `epoch tables` x
    // epochTableValue (3 KAS on the fixture, 4 KAS an epoch on the testnet defaults: 1,152 KAS a day), and two epochs later
    // anyone spends each epoch table to outputs of its own.
    let r = committee(&Committee { p: rparams(), from: 5_000, members: table(TOP3), holds: kpf_protocol::committee::NO_HOLDS });
    let roll = roll_tx(&r, 1, 6_000, |_| {});
    must("the roll", &roll);
    let roller_pays: u64 = roll.0.outputs[1..4].iter().map(|o| o.value).sum();
    assert_eq!((roller_pays as i64, roll.0.outputs[0].value), (3 * KAS1, r.value));
    let (_, tk) = r.s.roll(1, 6_000, r.value as i64).unwrap();
    let lt = live(&roll, 1, 6_000, &tk);
    let stranger = keypair(77);
    let retire = retire_tx(&lt, 8_000, |o| *o = vec![out(KAS1 as u64 - 1_000_000, p2pk_spk(&stranger), None)]);
    must("a stranger retires an epoch table to its own key", &retire);
    refused("not before two epochs", &retire_tx(&lt, 7_999, |_| {}), 0);
    let p = CommitteeParams::testnet();
    assert_eq!(p.epoch_tables * p.epoch_table_value * (864_000 / p.epoch), 1_152 * KAS1);
}

// ---------------------------------------------------------------- an order's expiry

#[test]
fn c2_bounded_an_orders_expiry_stops_neither_a_fill_nor_a_take() {
    // The note (section 4): an order that rested longer than its least lease allows can be entered into a pair about
    // to lapse, and "an order's expiry is its owner's bound on that". The order's script reads its expiry in `refund`
    // only: until someone spends it by `refund` (anyone may, nobody is paid to), an expired order is filled and taken
    // like any other. A long order accepted at 10,000 that expired at 12,000, taken with no pair at 15,100 into the
    // lease 14,000: settled at the entry in the next block, its tip and its carrier (10.2 KAS) the taker's.
    let owner = keypair(43);
    let o = COpen { tip: 1_000_000, expiry_daa: 12_000, ..sample_order(&cmarket(), LONG, x(&owner)) };
    let lo: LOrder = Live { op: outpoint(13), value: (o.spend(o.size_left) + KAS1) as u64, daa: 10_000, id: Some(Hash::from_bytes([13; 32])), s: o };
    let me = x(&exec_kp());
    let part = CPair {
        m: lo.s.m,
        size: SIZE,
        bl: FEED - lo.s.my_rate,
        bs: 1_200_000_000,
        entry: FEED,
        long_key: x(&owner),
        short_key: me,
        expiry: 14_000,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers { long: maker_of(&lo.op), ..Makers::default() },
    };
    let k = kit();
    let take = {
        let mut outs = vec![out(lo.value - lo.s.spend(SIZE) as u64, key_spk(&lo.s.owner), None), out(part.floor() as u64, pair_spk(&part), None), change(1)];
        seal(&mut outs, 1, 1, outpoint(60));
        let args = vec![iv(1), iv(SIZE), iv(FEED), iv(FEED), iv(1_200_000_000), bv(&me), bv(&[0u8; 32]), iv(0), iv(0), iv(0), iv(14_000), bv(&k.pair.prefix), bv(&k.pair.suffix)];
        build_ext(&[order_in(&lo, "take", args), exec_in(60)], outs, 15_100, &[])
    };
    must("the take of an order 3,100 DAA after its expiry", &take);
    let lpart = live(&take, 1, 15_100, &part);
    must("its lapse settle in the next block", &lapse_tx(&lpart, 15_100, |_| {}));
    let (_, short) = part.lapse_out(lpart.value);
    assert_eq!(short - (part.floor() - lo.s.spend(SIZE)), 10 * KAS1 + 20_000_000);
    // The refund that would have ended it was valid from 12,000, for anyone.
    let refund = build_ext(&[order_in(&lo, "refund", vec![]), exec_in(60)], vec![out(lo.value, key_spk(&lo.s.owner), None), change(1)], 12_000, &[]);
    must("the refund at the expiry", &refund);
    // Worse than the tip: the feed has fallen to 9.20 since the order expired. The taker fills it at its limit, 10 (a
    // fill needs both sides healthy at the fill price, not at the feed), with a full lease, and liquidates the order's
    // side with a true signed price 50 DAA later: the order's whole margin, 1,000 KAS, for an order its owner believed dead.
    let filled = CPair { expiry: 23_100, ..part.clone() };
    let fill = {
        let mut outs = vec![out(lo.value - lo.s.spend(SIZE) as u64, key_spk(&lo.s.owner), None), out(filled.floor() as u64, pair_spk(&filled), None), change(1)];
        seal(&mut outs, 1, 1, outpoint(60));
        let args = vec![iv(1), iv(SIZE), iv(FEED), iv(filled.bl), iv(filled.bs), bv(&me), bv(&[0u8; 32]), iv(23_100), bv(&k.pair.prefix), bv(&k.pair.suffix)];
        build_ext(&[order_in(&lo, "fill", args), exec_in(60)], outs, 15_100, &[])
    };
    must("the fill of the expired order at its limit", &fill);
    let lp = live(&fill, 1, 15_100, &filled);
    let lt = epoch_table_for(15_150);
    assert!(filled.long_liquidatable(920_000_000));
    let (lw, _) = proposed(&lp, &lt, &Propose::liq(&lt, LONG, SIZE, 920_000_000, 15_150), 15_151);
    must("the order's side is forfeited", &finalize_tx(&lw, 50, |_| {}));
    let CSettlement::Forfeit { part: f, .. } = lw.s.settlement(lw.value, 15_151) else { panic!("a forfeit") };
    assert_eq!((f.long_key, f.short_key), (me, me));
}

// ---------------------------------------------------------------- a committee that did not begin empty

#[test]
fn c2_fixed_a_committee_that_did_not_begin_empty_accepts_no_stake_and_pays_none_out() {
    // Review C2 (c2_bounded_who_joins_a_committee_that_did_not_begin_empty_funds_a_forged_members_exit): a genesis wrote a
    // committee whose member 0 claimed 100 KAS over a UTXO of 1 KAS; a newcomer joined member 1 with 100 KAS and the forged
    // member left with an exit of 100 KAS, the newcomer's. Every entry now writes a table its value covers.
    let forged = Committee { p: rparams(), from: 5_000, members: table([100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), holds: kpf_protocol::committee::NO_HOLDS };
    let l = Live { value: KAS1 as u64, ..committee(&forged) };
    assert!(!forged.backed(l.value as i64));
    let victim = x(&keypair(77));
    // The step that used to succeed.
    refused("the newcomer's join", &join_tx(&l, 1, victim, 100 * KAS1, |_| {}), 0);
    refused("a top-up of the forged member", &top_up_tx(&l, 0, KAS1, |_| {}), 0);
    refused("the forged member's leave", &leave_tx(&l, 0, &member_kp(0), |_| {}), 0);
    refused("a roll", &roll_tx(&l, 1, 6_000, |_| {}), 0);
    // Forged weights no sum would catch: a negative one beside a matching positive one, one above the member limit.
    let mut neg = table([100, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    neg[1].weight = -100 * KAS1;
    let ln = Live { value: KAS1 as u64, ..committee(&Committee { p: rparams(), from: 5_000, members: neg, holds: kpf_protocol::committee::NO_HOLDS }) };
    refused("a join beside a negative weight", &join_tx(&ln, 2, victim, 100 * KAS1, |_| {}), 0);
    refused("the leave of the member it would pay", &leave_tx(&ln, 0, &member_kp(0), |_| {}), 0);
    let mut big = table([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    big[0].weight = MEMBER_MAX + 1;
    let lb = Live { value: (MEMBER_MAX + 1 + KAS1) as u64, ..committee(&Committee { p: rparams(), from: 5_000, members: big, holds: kpf_protocol::committee::NO_HOLDS }) };
    refused("a join beside a weight above the member limit", &join_tx(&lb, 2, victim, 100 * KAS1, |_| {}), 0);
    // A committee that holds more than its table (a donation) is not held up, and the surplus stays in it.
    let rich = Live { value: (150 * KAS1) as u64, ..committee(&forged) };
    let join = join_tx(&rich, 1, victim, 100 * KAS1, |_| {});
    must("a join of a committee that holds 50 KAS more than its table", &join);
    let (joined, _, _) = forged.join(1, victim, 100 * KAS1).unwrap();
    let leave = leave_tx(&live(&join, 0, 5_100, &joined), 0, &member_kp(0), |_| {});
    must("the first member leaves with its 100 KAS", &leave);
    assert_eq!((leave.0.outputs[0].value as i64, leave.0.outputs[1].value as i64), (150 * KAS1, 100 * KAS1));
}
