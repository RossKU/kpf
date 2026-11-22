//! Review C3 of the committee family (docs/audit/REVIEW-C3.md): the family as fixed after review C2, attacked again.
//! What became of review C2's attacks is in `auditC2_tests.rs` (`c2_fixed_*`), of review C1's in `auditC1_tests.rs`.
//!
//! `c3_fixed_*`: what review C3 bounded and review C4 found fixed (the old numbers in a comment).
//! `c3_bounded_*`: an accepted behaviour whose cost or damage is measured here and that the note should state.
//! `c3_holds_*`: an attack that the contracts refuse.
#![allow(non_snake_case)]

mod committeekit;
mod common;

use committeekit::*;
use common::*;
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::*;
use kpf_protocol::committee::*;
use kpf_protocol::state::{KIND_CLOSE, LONG, SHORT};

const KAS1: i64 = 100_000_000;
const SIZE: i64 = 1_000_000_000;
const FEED: i64 = 1_000_000_000;

fn order_of(m: &CMarket, side: i64, kp: &secp256k1::Keypair, n: u8, born: u64, expiry: i64, extra: i64) -> LOrder {
    let o = COpen { m: *m, expiry_daa: expiry, ..sample_order(m, side, x(kp)) };
    Live { op: outpoint(n), value: (o.spend(o.size_left) + extra) as u64, daa: born, id: Some(Hash::from_bytes([n; 32])), s: o }
}

/// Inputs: the order (refund), the refunder's key. Outputs: `to_owner` to the owner at 0, the change.
fn refund_tx(lo: &LOrder, to_owner: u64, lock: u64) -> T {
    build_ext(&[order_in(lo, "refund", vec![]), exec_in(60)], vec![out(to_owner, key_spk(&lo.s.owner), None), change(1)], lock, &[])
}

fn att_of(market: &[u8; 32], t0: i64, p: i64) -> Attestation {
    let mut a = Attestation { market: *market, t0, p, bits: 0, prices: [0; MEMBERS], sigs: [[0; 64]; MEMBERS] };
    for i in 0..3 {
        a.bits |= 1 << i;
        a.prices[i] = p;
        a.sigs[i] = sign_price(&member_kp(i), &rid(), market, t0, p).sig;
    }
    a
}

// ---------------------------------------------------------------- the refund's fee (the fix of C2-F2)

#[test]
fn c3_fixed_the_refunders_fee_is_at_most_a_twentieth_of_the_escrow() {
    // Review C3 (c3_bounded_the_refunders_fee_is_the_markets_keeper_fee_floor_up_to_the_whole_escrow): the fee was the
    // market's keeperFeeMin whatever the escrow: on a market with a floor of 2,000 KAS the refunder of a 1,001.2 KAS
    // order paid its owner one sompi. The fee is now min(keeperFeeMin, escrow / 20).
    let owner = keypair(43);
    // The fixture market: keeperFeeMin 0.01 KAS. An order of 1,000 units (1,000.2 KAS and 1 KAS more escrowed) that
    // expired at 12,000: whoever refunds it keeps exactly 0.01 KAS, from the expiry on and not before.
    let lo = order_of(&cmarket(), LONG, &owner, 13, 10_000, 12_000, KAS1);
    let fee = cmarket().p.keeper_fee_min as u64;
    assert_eq!(lo.s.refund_fee(lo.value) as u64, fee);
    must("the refund at the expiry, the fee kept", &refund_tx(&lo, lo.value - fee, 12_000));
    refused("a sompi more for the refunder", &refund_tx(&lo, lo.value - fee - 1, 12_000), 0);
    refused("before the expiry", &refund_tx(&lo, lo.value, 11_999), 0);
    // The owner's payout is fixed to the order's own output index: two orders of one owner are not paid by one output.
    let lo2 = order_of(&cmarket(), LONG, &owner, 14, 10_000, 12_000, KAS1);
    let both = |outs: Vec<kaspa_consensus_core::tx::TransactionOutput>| build_ext(&[order_in(&lo, "refund", vec![]), order_in(&lo2, "refund", vec![]), exec_in(60)], outs, 12_000, &[]);
    must("two refunds, two payouts", &both(vec![out(lo.value - fee, key_spk(&lo.s.owner), None), out(lo2.value - fee, key_spk(&lo.s.owner), None), change(1)]));
    assert!(!accepted(&both(vec![out(lo.value - fee, key_spk(&lo.s.owner), None), change(1), change(1)])), "one payout for two orders");
    // The step that used to succeed: a market with a floor of 2,000 KAS. The refunder keeps a twentieth, 50.06 KAS.
    let dear = CMarket { p: CParams { keeper_fee_min: 2_000 * KAS1, ..cmarket().p }, ..cmarket() };
    let big = order_of(&dear, LONG, &owner, 15, 10_000, 12_000, KAS1);
    refused("the whole escrow to the refunder", &refund_tx(&big, 1, 12_000), 0);
    let cap = big.value / 20;
    assert_eq!((big.s.refund_fee(big.value) as u64, cap), (cap, 5_006_000_000));
    must("a twentieth to the refunder", &refund_tx(&big, big.value - cap, 12_000));
    refused("a sompi more", &refund_tx(&big, big.value - cap - 1, 12_000), 0);
    // The testnet defaults: 0.1 KAS, which is the twentieth of an escrow of 2 KAS; below that the twentieth binds.
    assert_eq!(CMarket::testnet([1; 32], [2; 32]).p.keeper_fee_min, 10_000_000);
    // At the expiry a refund and a fill are two spends of one UTXO: the order is still filled if the fill lands.
    let so = order_of(&cmarket(), SHORT, &short_kp(), 12, 10_000, 9_000_000, KAS1);
    let lo11 = Live { op: outpoint(11), id: Some(Hash::from_bytes([11; 32])), ..lo.clone() };
    must("a fill of the expired order", &open_tx(&lo11, &so, SIZE, FEED, 20_000, 12_000, |_| {}));
}

// ---------------------------------------------------------------- a market its committee cannot price

#[test]
fn c3_holds_on_a_committee_too_wide_for_a_market_a_pair_keeps_every_exit_that_needs_no_price() {
    // The fix of C2-F1 makes a market on a committee whose spread exceeds a quarter of its g a market without prices:
    // no claim and no renewal. Its pairs are not locked: both parties' entries work, and the lapse ends them.
    let mut wide = epoch_epoch_table().s;
    wide.spread_bps = 51;
    let lt = epoch_table(&wide);
    let listed = pair(&CPair { long_ask: 990_000_000, long_min: 1_000_000, short_bid: 1_010_000_000, short_min: 1_000_000, ..sample_pair(&cmarket()) }, 6_100);
    let a = att_of(&cmarket().id(), 6_500, FEED);
    let close = Propose { c: Claim { kind: KIND_CLOSE, side: LONG, n: SIZE, p: FEED, t0: 6_500 }, a: a.clone(), signer: Some(long_kp()), proposer: x(&long_kp()), lock: 6_500, seq: 20 };
    refused("no close", &propose_tx(&listed, &lt, &close, |_| {}), 0);
    let mut late = wide;
    (late.from, late.until) = (8_000, 9_000);
    refused("no renewal", &keep_alive_tx(&listed, &epoch_table(&late), &att_of(&cmarket().id(), 8_000, FEED), 8_000, |_| {}), 0);
    must("an assign", &assign_tx(&listed, LONG, SIZE, 1_040_000_000, 900_000_000, &long_kp(), 20, |_| {}));
    must("a transfer of the listed side", &transfer_tx(&listed, LONG, SIZE, FEED, 900_000_000, 20, |_| {}));
    must("a mutual close", &mutual_tx(&listed, FEED, 20, |_| {}));
    must("a top-up of margin", &add_margin_tx(&listed, SHORT, 50_000_000, &short_kp(), 20, |_| {}));
    must("a cooperative close", &coop_tx(&listed, &long_kp(), &short_kp(), |_| {}));
    must("the lapse settle at 15,000", &lapse_tx(&listed, 15_000, |_| {}));
    refused("and not before", &lapse_tx(&listed, 14_999, |_| {}), 0);
    // Nobody is liquidated on such a market and every pair ends at its entry: it prices nothing, and takes nothing.
    assert_eq!(listed.s.lapse_out(listed.value), (1_000 * KAS1, 2_000 * KAS1 + 20_000_000));
}

// ---------------------------------------------------------------- the committee's backing over a life

#[test]
fn c3_holds_the_backing_check_stops_no_honest_sequence_of_the_committee() {
    // An empty committee of 1 KAS; fifteen joins fill it; the least member is outbid again and again; members top up and
    // leave; the committee rolls in between. Every entry writes a table its value covers, with one KAS to spare.
    let p = rparams();
    let mut l: LCommittee = Live { op: outpoint(1), value: KAS1 as u64, daa: 0, id: Some(RID), s: Committee::empty(p, 5_000) };
    let mut now = 5_000u64;
    let mut exits = 0i64;
    for i in 0..MEMBERS {
        let amount = (10 + i as i64) * KAS1;
        let t = join_tx(&l, i, x(&member_kp(i)), amount, |_| {});
        must("a join of an empty member", &t);
        now += 20;
        l = live(&t, 0, now, &l.s.join(i, x(&member_kp(i)), amount).unwrap().0);
    }
    for round in 0..20 {
        let least = (0..MEMBERS).min_by_key(|j| l.s.members[*j].weight).unwrap();
        let bid = l.s.least_bid(least).unwrap();
        let (r, e, _) = l.s.join(least, x(&member_kp(15 + round % 5)), bid).unwrap();
        let t = join_tx(&l, least, x(&member_kp(15 + round % 5)), bid, |_| {});
        must("the least member outbid at its least bid", &t);
        refused("a sompi below the least bid", &join_tx(&l, least, x(&member_kp(15)), bid - 1, |_| {}), 0);
        exits += e.unwrap().amount;
        now += 20;
        l = live(&t, 0, now, &r);
        assert!(l.s.backed(l.value as i64));
        if round % 4 == 0 {
            let t = roll_tx(&l, 1, now.max(l.s.from as u64 + 1_000), |_| {});
            must("a roll", &t);
            now = now.max(l.s.from as u64 + 1_000);
            l = live(&t, 0, now, &l.s.roll(1, now as i64, l.value as i64).unwrap().0);
            now += 20;
        }
        if round % 5 == 1 {
            let t = top_up_tx(&l, 0, 3 * KAS1, |_| {});
            must("a top-up", &t);
            now += 20;
            l = live(&t, 0, now, &l.s.top_up(0, 3 * KAS1).unwrap());
        }
    }
    // Every member leaves, down to the empty committee and its one KAS.
    for i in 0..MEMBERS {
        let kp = (0..20).map(member_kp).find(|k| x(k) == l.s.members[i].key).unwrap();
        let (r, e) = l.s.leave(i).unwrap();
        let t = leave_tx(&l, i, &kp, |_| {});
        must("a leave", &t);
        exits += e.amount;
        now += 20;
        l = live(&t, 0, now, &r);
    }
    assert_eq!((l.s.total(), l.value as i64), (0, KAS1));
    assert!(exits > 300 * KAS1);
}
