//! The committee family in the script engine, second part (docs/DESIGN-COMMITTEE.md 2, 3): what a pair accepts as a
//! epoch table and what it does not; the lease (keepAlive, lapseSettle); the hand-overs (assign, transfer, an order's take);
//! the price-free entries (amend, addMargin, mutualClose, coopClose).

mod committeekit;
mod common;

use committeekit::*;
use common::*;
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::*;
use kpf_protocol::committee::*;
use kpf_protocol::oracle::{maker_of, Makers};
use kpf_protocol::state::{LONG, SHORT};
use silverscript_abi::ArtifactValue;

const KAS1: i64 = 100_000_000;
const SIZE: i64 = 1_000_000_000;

fn liq(lt: &LEpochTable) -> Propose {
    Propose::liq(lt, LONG, SIZE, 928_000_000, 6_500)
}

#[test]
fn a_pair_takes_a_price_only_from_a_epoch_table_of_its_committee_spent_by_attest_for_its_market() {
    let lp = opened();
    let lt = epoch_epoch_table();
    must("the committee's epoch table", &propose_tx(&lp, &lt, &liq(&lt), |_| {}));

    // An epoch table of the same template and table under another covenant id (another committee, or a look-alike).
    let mut other = lt.clone();
    other.id = Some(Hash::from_bytes([0x53; 32]));
    let pr = liq(&lt);
    // Its members sign for the other committee's id, so the epoch table itself accepts; the pair refuses the id.
    let m = cmarket().id();
    let s: Vec<SignedPrice> = (0..3).map(|i| sign_price(&member_kp(i), &[0x53; 32], &m, 6_500, 928_000_000)).collect();
    let a = Attestation::collect(&other.s, &[0x53; 32], &m, 6_500, &s).unwrap();
    let t = propose_tx(&lp, &other, &Propose { a, ..pr.clone() }, |o| o[1].covenant = bind(1, Hash::from_bytes([0x53; 32])));
    let v = run(&t.0, &t.1).expect("bindings");
    assert!(v[1].is_ok(), "the foreign epoch table accepts its own members' signatures: {:?}", v[1]);
    assert!(v[0].is_err(), "the pair refuses an epoch table of another committee");

    // An epoch table with no covenant id at all.
    let mut bare = lt.clone();
    bare.id = None;
    assert!(!accepted(&propose_tx(&lp, &bare, &liq(&lt), |o| o[1].covenant = None)), "an epoch table without an id");

    // The attestation is for another market: the epoch table accepts it (its members signed that market), the pair does not.
    let s: Vec<SignedPrice> = (0..3).map(|i| sign_price(&member_kp(i), &rid(), &[0x4e; 32], 6_500, 928_000_000)).collect();
    let a = Attestation::collect(&lt.s, &rid(), &[0x4e; 32], 6_500, &s).unwrap();
    let t = propose_tx(&lp, &lt, &Propose { a, ..liq(&lt) }, |_| {});
    let v = run(&t.0, &t.1).expect("bindings");
    assert!(v[1].is_ok() && v[0].is_err(), "a signed price of another market: epoch table {:?}, pair {:?}", v[1], v[0]);

    // Signatures below the quorum: the epoch table refuses, so the transaction does.
    let s: Vec<SignedPrice> = [0usize, 3, 4].iter().map(|i| sign_price(&member_kp(*i), &rid(), &m, 6_500, 928_000_000)).collect();
    let mut a = att(&lt.s, 6_500, 928_000_000);
    a.bits = 0b11001;
    a.prices = [0; MEMBERS];
    a.sigs = [[0; 64]; MEMBERS];
    for (k, i) in [0usize, 3, 4].iter().enumerate() {
        a.prices[*i] = 928_000_000;
        a.sigs[*i] = s[k].sig;
    }
    let t = propose_tx(&lp, &lt, &Propose { a, ..liq(&lt) }, |_| {});
    let v = run(&t.0, &t.1).expect("bindings");
    assert!(v[1].is_err(), "the epoch table refuses 50 % of the weight");

    // The claim's lock time is before the signed price: the epoch table refuses a signed price of the future.
    refused("a signed price ahead of the lock time", &propose_tx(&lp, &lt, &Propose { lock: 6_499, ..liq(&lt) }, |_| {}), 1);
}

#[test]
fn a_look_alike_of_the_epoch_table_is_not_a_epoch_table() {
    let lp = opened();
    let lt = epoch_epoch_table();
    let pr = liq(&lt);
    let (w, fb) = pending_forced(&lp.s, &pr.c, pr.proposer);
    let w = w.made_by(&lp.op);
    let args = |u: i64| {
        let k = kit();
        vec![iv(SIZE), iv(pr.c.kind), iv(LONG), iv(pr.c.p), iv(6_500), iv(u), bv(&[0u8; 65]), iv(fb), iv(0), bv(&pr.proposer), bv(&k.pending.prefix), bv(&k.pending.suffix)]
    };
    // The probe C1Attest has attest's arguments and checks, but it is another template: under the committee's id (what
    // only a broken lineage could hold) the pair still refuses it.
    let probe = compile(
        "probes/C1Attest.sil",
        &[ArtifactValue::Bytes(table_bytes(&lt.s.members)), int(lt.s.total), int(lt.s.from), int(lt.s.until), int(lt.s.quorum_bps)],
    );
    let fake = contract_in(outpoint(2), lt.value, Some(RID), &probe, "attest", pr.a.args());
    let mut outs = vec![out(lp.value, pend_spk(&w), None), out(lt.value, p2sh(&probe), bind(1, RID)), change(1)];
    seal(&mut outs, 0, 0, lp.op);
    let t = build_ext(&[pair_in(&lp, "propose", args(1)), fake, exec_in(60)], outs.clone(), 6_500, &[]);
    let v = run(&t.0, &t.1).expect("bindings");
    assert!(v[1].is_ok(), "the probe accepts: {:?}", v[1]);
    assert!(v[0].is_err(), "the pair refuses another template under the committee's id");

    // The epoch table input named is the executor's key, or the pair itself.
    for u in [2i64, 0, 3, -1] {
        let mut outs = vec![out(lp.value, pend_spk(&w), None), out(lt.value, epoch_table_spk(&lt.s), bind(1, RID)), change(1)];
        seal(&mut outs, 0, 0, lp.op);
        let t = build_ext(&[pair_in(&lp, "propose", args(u)), attest_in(&lt, &pr.a), exec_in(60)], outs, 6_500, &[]);
        assert!(!accepted(&t), "epoch table index {u}");
    }

    // An epoch table that is being retired gives no price: the transaction that retires it holds no continuation, and the
    // pair reads attest's tag.
    let old = epoch_table(&EpochTable { from: 4_000, until: 5_000, ..lt.s });
    let mut outs = vec![out(lp.value, pend_spk(&w), None), change(1), change(1)];
    seal(&mut outs, 0, 0, lp.op);
    let t = build_ext(&[pair_in(&lp, "propose", args(1)), epoch_table_in(&old, "retire", vec![]), exec_in(60)], outs, 6_500, &[]);
    let v = run(&t.0, &t.1).expect("bindings");
    assert!(v[1].is_ok() && v[0].is_err(), "a retiring epoch table: epoch table {:?}, pair {:?}", v[1], v[0]);
}

#[test]
fn one_attestation_prices_several_pairs() {
    // Two pairs liquidated by one epoch table in one transaction: inputs pair A, the epoch table, pair B, the executor.
    let lt = epoch_epoch_table();
    let a_pair = opened();
    let mut b_pair = pair(&sample_pair(&cmarket()), 6_100);
    b_pair.op = outpoint(20);
    b_pair.id = Some(Hash::from_bytes([0x58; 32]));
    let pr = liq(&lt);
    let k = kit();
    let mk = |lp: &LPair, kout: i64| {
        let (w, fb) = pending_forced(&lp.s, &pr.c, pr.proposer);
        (w.made_by(&lp.op), vec![iv(SIZE), iv(pr.c.kind), iv(LONG), iv(pr.c.p), iv(6_500), iv(1), bv(&[0u8; 65]), iv(fb), iv(kout), bv(&pr.proposer), bv(&k.pending.prefix), bv(&k.pending.suffix)])
    };
    let (wa, args_a) = mk(&a_pair, 0);
    let (wb, args_b) = mk(&b_pair, 2);
    let mut outs = vec![out(a_pair.value, pend_spk(&wa), None), out(lt.value, epoch_table_spk(&lt.s), bind(1, RID)), out(b_pair.value, pend_spk(&wb), None), change(1)];
    seal(&mut outs, 0, 0, a_pair.op);
    seal(&mut outs, 2, 2, b_pair.op);
    let t = build_ext(&[pair_in(&a_pair, "propose", args_a), attest_in(&lt, &pr.a), pair_in(&b_pair, "propose", args_b), exec_in(60)], outs, 6_500, &[]);
    must("two pairs, one epoch table", &t);
}

#[test]
fn an_attested_price_keeps_a_pair_alive() {
    let lp = opened();
    let lt = epoch_epoch_table();
    // The lease is 14,000; a signed price at t0 gives t0 + 8,000, which must be at least 16,000: t0 >= 8,000.
    assert_eq!(lp.s.keep_alive_from(), 8_000);
    let late = epoch_table(&EpochTable { from: 8_000, until: 9_000, ..lt.s });
    let a = att(&late.s, 8_000, 1_000_000_000);
    must("a signed price at 8,000", &keep_alive_tx(&lp, &late, &a, 8_000, |_| {}));
    assert_eq!(lp.s.kept_alive(8_000).unwrap().expiry, 16_000);
    // A signed price too early to move the lease by a quarter of a term.
    let a0 = att(&lt.s, 6_500, 1_000_000_000);
    assert!(lp.s.kept_alive(6_500).is_none());
    refused("a signed price at 6,500", &keep_alive_tx(&lp, &lt, &a0, 6_500, |_| {}), 0);
    let early = epoch_table(&EpochTable { from: 7_000, until: 8_000, ..lt.s });
    refused("a signed price at 7,999", &keep_alive_tx(&lp, &early, &att(&early.s, 7_999, 1_000_000_000), 7_999, |_| {}), 0);
    // The price of the time does not matter; the market does.
    must("any price", &keep_alive_tx(&lp, &late, &att(&late.s, 8_000, 5), 8_000, |_| {}));
    let m2 = [0x4e; 32];
    let s: Vec<SignedPrice> = (0..3).map(|i| sign_price(&member_kp(i), &rid(), &m2, 8_000, 1)).collect();
    let foreign = Attestation::collect(&late.s, &rid(), &m2, 8_000, &s).unwrap();
    refused("a signed price of another market", &keep_alive_tx(&lp, &late, &foreign, 8_000, |_| {}), 0);
    // Nothing but the lease changes, and no value leaves.
    refused("one sompi less", &keep_alive_tx(&lp, &late, &a, 8_000, |o| o[0].value -= 1), 0);
    for (what, bad) in [
        ("a longer lease than the signed price gives", CPair { expiry: 16_001, ..lp.s.rest_of(SIZE) }),
        ("the long's bound moved", CPair { expiry: 16_000, bl: lp.s.bl - 1, ..lp.s.rest_of(SIZE) }),
        ("the short's key replaced", CPair { expiry: 16_000, short_key: [1; 32], ..lp.s.rest_of(SIZE) }),
        ("a maker left in place", CPair { expiry: 16_000, ..lp.s.clone() }),
    ] {
        refused(what, &keep_alive_tx(&lp, &late, &a, 8_000, |o| o[0].script_public_key = pair_spk(&bad)), 0);
    }
    // A listing survives the renewal.
    let mut listed = lp.clone();
    listed.s = CPair { long_ask: 1_100_000_000, long_min: 1_000_000, long_tip: 7, ..lp.s.rest_of(SIZE) };
    must("a listed pair", &keep_alive_tx(&listed, &late, &a, 8_000, |_| {}));
}

#[test]
fn a_pair_nobody_kept_alive_settles_at_its_entry() {
    let lp = opened();
    assert_eq!(lp.s.lapse_from(), 15_000);
    assert_eq!(lp.s.lapse_out(lp.value), (1_000 * KAS1, 2_000 * KAS1 + 20_000_000));
    must("at 15,000", &lapse_tx(&lp, 15_000, |_| {}));
    refused("at 14,999", &lapse_tx(&lp, 14_999, |_| {}), 0);
    refused("the long one sompi less", &lapse_tx(&lp, 15_000, |o| o[0].value -= 1), 0);
    refused("the short one sompi less", &lapse_tx(&lp, 15_000, |o| o[2].value -= 1), 0);
    refused("the short's payout to another key", &lapse_tx(&lp, 15_000, |o| o[2].script_public_key = key_spk(&x(&exec_kp()))), 0);
}

#[test]
fn a_side_is_assigned_in_one_transaction() {
    let lp = opened();
    // The long signs its side over to the executor at 10.4 KAS: it is paid 1,000 x (10.4 - 9) = 1,400 KAS at once.
    let t = assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 20, |_| {});
    must("the long assigns", &t);
    assert_eq!(t.0.outputs[0].value as i64, 1_400 * KAS1);
    refused("signed by the short", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &short_kp(), 20, |_| {}), 0);
    refused("signed by the entrant", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &exec_kp(), 20, |_| {}), 0);
    refused("before the quiet time", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 19, |_| {}), 0);
    refused("the leaver one sompi less", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 20, |o| o[0].value -= 1), 0);
    refused("the part one sompi less", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 20, |o| o[1].value -= 1), 0);
    // The entrant posts at least the leaver's margin: a bound nearer than the leaver's is refused, a farther one is not.
    refused("an entrant with less margin", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_001, &long_kp(), 20, |_| {}), 0);
    must("an entrant with more margin", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 850_000_000, &long_kp(), 20, |_| {}));
    // The entrant is healthy at the price: at x = 9.4 a bound of 9 leaves less than the maintenance (9.4 x 0.95 = 8.93).
    refused("an entrant below its maintenance at x", &assign_tx(&lp, LONG, SIZE, 940_000_000, 900_000_000, &long_kp(), 20, |_| {}), 0);
    must("the same price with a farther bound", &assign_tx(&lp, LONG, SIZE, 940_000_000, 893_000_000, &long_kp(), 20, |_| {}));
    // The short's mirror, and a part.
    must("the short assigns", &assign_tx(&lp, SHORT, SIZE, 1_040_000_000, 1_200_000_000, &short_kp(), 20, |_| {}));
    refused("a short entrant with less margin", &assign_tx(&lp, SHORT, SIZE, 1_040_000_000, 1_199_999_999, &short_kp(), 20, |_| {}), 0);
    let part = assign_tx(&lp, LONG, 400_000_000, 1_040_000_000, 900_000_000, &long_kp(), 20, |_| {});
    must("the long assigns 400 of 1,000", &part);
    assert_eq!(part.0.outputs[0].value as i64, 560 * KAS1);
    refused("a remainder of another size", &assign_tx(&lp, LONG, 400_000_000, 1_040_000_000, 900_000_000, &long_kp(), 20, |o| o[2].script_public_key = pair_spk(&lp.s.rest_of(600_000_001))), 0);
    // The part is the pair's state with the entrant's key and bound, the stayer untouched, the pair as its parent.
    let (_, written, _, _) = handed(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, x(&exec_kp()), [0; 32], 0);
    assert_eq!((written.long_key, written.short_key, written.entry, written.expiry), (x(&exec_kp()), x(&short_kp()), lp.s.entry, lp.s.expiry));
    assert_eq!(written.mk, Makers { parent: maker_of(&lp.op), ..Makers::default() });
    let stolen = CPair { short_key: x(&exec_kp()), ..written.clone() };
    refused("the stayer's key replaced", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 20, |o| o[1].script_public_key = pair_spk(&stolen)), 0);
    let moved = CPair { bs: written.bs - 1, ..written.clone() };
    refused("the stayer's bound moved", &assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 20, |o| o[1].script_public_key = pair_spk(&moved)), 0);
    // The part must start a fresh lineage: one that keeps the pair's id is refused.
    let t = assign_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 20, |_| {});
    let mut kept = t.0.clone();
    kept.outputs[1].covenant = bind(0, lp.id.unwrap());
    assert!(run(&kept, &t.1).map(|v| v[0].is_err()).unwrap_or(true), "a part under the pair's own id");
}

#[test]
fn a_listed_side_is_transferred_or_taken_by_an_order() {
    let lp = opened();
    // Unlisted: no transfer.
    refused("an unlisted side", &transfer_tx(&lp, LONG, SIZE, 1_040_000_000, 900_000_000, 20, |_| {}), 0);
    // The long lists at 10.3 with a tip of 0.01 per unit.
    let t = amend_tx(&lp, LONG, 1_030_000_000, 1_000_000, 1_000_000, &long_kp(), 20, |_| {});
    must("the long lists its side", &t);
    refused("the short lists the long's side", &amend_tx(&lp, LONG, 1_030_000_000, 1_000_000, 1_000_000, &short_kp(), 20, |_| {}), 0);
    refused("before the quiet time", &amend_tx(&lp, LONG, 1_030_000_000, 1_000_000, 1_000_000, &long_kp(), 19, |_| {}), 0);
    let listed = live(&t, 0, 6_200, &CPair { long_ask: 1_030_000_000, long_min: 1_000_000, long_tip: 1_000_000, ..lp.s.rest_of(SIZE) });
    must("a taker at 10.4", &transfer_tx(&listed, LONG, SIZE, 1_040_000_000, 900_000_000, 20, |_| {}));
    refused("a taker below the ask", &transfer_tx(&listed, LONG, SIZE, 1_029_999_999, 900_000_000, 20, |_| {}), 0);
    refused("the leaver one sompi less", &transfer_tx(&listed, LONG, SIZE, 1_040_000_000, 900_000_000, 20, |o| o[0].value -= 1), 0);
    // A long order takes the listed long side at 10.4: its bound is x - myRate = 9.4... the order's limit is 10, so it
    // takes only at or below 10; relist at 9.9.
    let t = amend_tx(&lp, LONG, 990_000_000, 1_000_000, 0, &long_kp(), 20, |_| {});
    let cheap = live(&t, 0, 6_200, &CPair { long_ask: 990_000_000, long_min: 1_000_000, long_tip: 0, ..lp.s.rest_of(SIZE) });
    let lo = order(LONG, &keypair(43), 13, 6_150);
    let take = take_tx(&cheap, &lo, SIZE, 1_000_000_000, 6_300, 20, |_| {});
    must("an order takes the listed side at 10", &take);
    refused("above the order's limit", &take_tx(&cheap, &lo, SIZE, 1_000_000_001, 6_300, 20, |_| {}), 1);
    // The order checks the part it enters: the stayer's key, the lease and its own bound are what the pair writes.
    let (_, part, _, _) = handed(&cheap, LONG, SIZE, 1_000_000_000, 900_000_000, lo.s.owner, maker_of(&lo.op), 0);
    assert_eq!(part.mk, Makers { long: maker_of(&lo.op), parent: maker_of(&cheap.op), ..Makers::default() });
    let t = take_tx(&cheap, &lo, SIZE, 1_000_000_000, 6_300, 20, |o| o[2].value -= 1);
    assert!(!accepted(&t), "a part of one sompi less");
    let t = take_tx(&cheap, &lo, SIZE, 1_000_000_000, 6_300, 20, |o| o[1].value -= 1);
    refused("the order's refund one sompi less", &t, 1);
}

#[test]
fn margin_is_added_and_a_pair_is_closed_by_both_sides() {
    let lp = opened();
    must("the long adds 0.5 of margin", &add_margin_tx(&lp, LONG, 50_000_000, &long_kp(), 20, |_| {}));
    refused("the short adds to the long's side", &add_margin_tx(&lp, LONG, 50_000_000, &short_kp(), 20, |_| {}), 0);
    refused("one sompi less than the margin", &add_margin_tx(&lp, LONG, 50_000_000, &long_kp(), 20, |o| o[0].value -= 1), 0);
    refused("a negative delta", &add_margin_tx(&lp, LONG, -50_000_000, &long_kp(), 20, |_| {}), 0);
    must("the short adds any amount up to priceMax", &add_margin_tx(&lp, SHORT, 5_000_000_000, &short_kp(), 20, |_| {}));
    refused("below zero", &add_margin_tx(&lp, LONG, 900_000_001, &long_kp(), 20, |_| {}), 0);
    // Mutual close: both listed and crossing.
    let both = pair(&CPair { long_ask: 1_020_000_000, long_min: 1_000_000, short_bid: 1_040_000_000, short_min: 1_000_000, ..lp.s.rest_of(SIZE) }, 6_100);
    must("at 10.3", &mutual_tx(&both, 1_030_000_000, 20, |_| {}));
    refused("below the long's ask", &mutual_tx(&both, 1_019_999_999, 20, |_| {}), 0);
    refused("above the short's bid", &mutual_tx(&both, 1_040_000_001, 20, |_| {}), 0);
    refused("the long one sompi less", &mutual_tx(&both, 1_030_000_000, 20, |o| o[0].value -= 1), 0);
    refused("the short one sompi less", &mutual_tx(&both, 1_030_000_000, 20, |o| o[2].value -= 1), 0);
    refused("an unlisted pair", &mutual_tx(&lp, 1_030_000_000, 20, |_| {}), 0);
    // Cooperative close: both keys, any split.
    must("both sign", &coop_tx(&lp, &long_kp(), &short_kp(), |_| {}));
    refused("the long alone", &coop_tx(&lp, &long_kp(), &long_kp(), |_| {}), 0);
    refused("the short alone", &coop_tx(&lp, &short_kp(), &short_kp(), |_| {}), 0);
}

#[test]
fn a_market_takes_prices_only_from_epoch_tables_whose_spread_leaves_its_tolerance_a_margin() {
    // Review C2 (C2-F1). The fixture market's g is 2 %: an epoch table with a spread of 0.5 % prices it (4 x 50 = 200), one
    // with a spread of 0.51 % does not, though the epoch table itself accepts the attestation.
    let lp = opened();
    let lt = epoch_epoch_table();
    assert!(cmarket().spread_ok(50) && !cmarket().spread_ok(51));
    let wide = epoch_table(&EpochTable { spread_bps: 51, ..lt.s });
    let pr = Propose { a: att(&wide.s, 6_500, 928_000_000), ..liq(&lt) };
    let t = propose_tx(&lp, &wide, &pr, |_| {});
    let v = run(&t.0, &t.1).expect("bindings");
    assert!(v[1].is_ok(), "the epoch table attests: {:?}", v[1]);
    assert!(v[0].is_err(), "the pair refuses an epoch table whose spread leaves g no margin");
    must("the fixture epoch table", &propose_tx(&lp, &lt, &liq(&lt), |_| {}));
    // The same for a renewal of the lease.
    let late = epoch_table(&EpochTable { from: 8_000, until: 9_000, ..lt.s });
    let late_wide = epoch_table(&EpochTable { from: 8_000, until: 9_000, spread_bps: 51, ..lt.s });
    must("a renewal on the fixture epoch table", &keep_alive_tx(&lp, &late, &att(&late.s, 8_000, 1_000_000_000), 8_000, |_| {}));
    refused("a renewal on the wide epoch table", &keep_alive_tx(&lp, &late_wide, &att(&late_wide.s, 8_000, 1_000_000_000), 8_000, |_| {}), 0);
}

#[test]
fn an_expired_order_is_returned_by_anyone_for_the_keepers_fee() {
    // Review C2 (C2-F2): nothing proves the present is before a time, so a fill cannot refuse an expired order; the
    // expiry binds when a refund lands, and whoever lands it keeps keeperFeeMin (0.01 KAS on the fixture).
    let mut lo = order(LONG, &long_kp(), 11, 6_000);
    lo.s.expiry_daa = 9_000;
    let fee = cmarket().p.keeper_fee_min as u64;
    let refund = |lock: u64, pay: u64, to: [u8; 32]| build_ext(&[order_in(&lo, "refund", vec![]), exec_in(60)], vec![out(pay, key_spk(&to), None), change(1)], lock, &[]);
    must("at the expiry, the escrow less the fee", &refund(9_000, lo.value - fee, lo.s.owner));
    refused("one DAA before the expiry", &refund(8_999, lo.value - fee, lo.s.owner), 0);
    refused("one sompi more than the fee kept", &refund(9_000, lo.value - fee - 1, lo.s.owner), 0);
    refused("to another key", &refund(9_000, lo.value, x(&exec_kp())), 0);
    // Until a refund lands the order can still be filled, at its limit, whatever the feed is.
    let so = order(SHORT, &short_kp(), 12, 6_000);
    must("an expired order is still filled", &open_tx(&lo, &so, SIZE, 1_000_000_000, 22_000, 14_000, |_| {}));
}
