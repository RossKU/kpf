//! Review C8 (docs/audit/REVIEW-C8.md): cross margin in the script engine. A trim (`KpfPairC.propose` kind 3) against
//! the other party: what it can take, what it leaves, and what the trimmed pair still does.
//!
//! Fixture (`committeekit::cmarket`): mmr 5 %, g 2 %, lateDaa 100, quiet 20; the worked pair: 1,000 units, the long's
//! bound at 9, the short's at 12, entry 10, opened at 6,100.
//!
//! `c8_holds_` an attack the engine refuses; `c8_bounded_` an accepted behaviour, measured.

mod committeekit;
mod common;

use committeekit::*;
use common::*;
use kpf_protocol::cmarket::*;
use kpf_protocol::state::{KIND_CLOSE, LONG, SHORT};

const KAS1: i64 = 100_000_000;
const SIZE: i64 = 1_000_000_000;

fn px(kas: f64) -> i64 {
    (kas * 1e8).round() as i64
}

/// What a close of `side` at the time `p` pays (long, short).
fn close_pays(lp: &LPair, side: i64, p: i64, kp: &secp256k1::Keypair) -> Option<(i64, i64)> {
    let (w, _) = lp.s.pending_of(kit(), SIZE, KIND_CLOSE, side, p, 6_500, x(kp))?;
    match w.settlement(lp.value, 6_510) {
        CSettlement::Close { long, short } => Some((long, short)),
        _ => None,
    }
}

/// A trim takes less than a close at the same time would pay its side, and leaves the other party, at whatever the
/// true price is when it lands, at least the lesser of what it had and what that close would have paid it. So a signed price
/// that is stale within lateDaa gives a trimmer nothing a closer did not have (the limit review C1 left: lateDaa).
#[test]
fn c8_holds_a_trim_leaves_the_other_party_no_worse_than_a_close_at_the_same_price() {
    let lp = opened();
    let lt = epoch_epoch_table();
    let (mut trims, mut worst) = (0, 0i64);
    for side in [LONG, SHORT] {
        let kp = if side == LONG { long_kp() } else { short_kp() };
        for step in 0..=45 {
            // Attested prices from 8.5 to 13 (the short's bound is 12; the market counts up to its priceMax).
            let p = px(8.5) + step * px(0.1);
            let Some(nb) = lp.s.trim_bound_of(side, p) else { continue };
            if !lp.s.trim_ok(side, p, nb) {
                continue;
            }
            trims += 1;
            let part = lp.s.trim_state(side, nb);
            let take = lp.s.trim_take(side, nb);
            let tr = Trim::of(&lt, side, nb, p, 6_500, &kp);
            let lw = trimmed(&lp, &lt, &tr, 6_510);
            assert_eq!(lw.s.settlement(lw.value, 6_510), CSettlement::Trim { part: part.clone(), value: lp.value as i64 - take, take });
            must("the trim at its limit settles", &finalize_tx(&lw, 0, |_| {}));
            // One sompi of price nearer is refused by the pair.
            let nearer = if side == LONG { nb + 1 } else { nb - 1 };
            assert!(!lp.s.trim_ok(side, p, nearer));
            refused("a bound a sompi nearer than the side is healthy at", &trim_tx(&lp, &lt, &Trim::of(&lt, side, nearer, p, 6_500, &kp), |_| {}), 0);
            // The same time as a close.
            let (cl, cs) = close_pays(&lp, side, p, &kp).expect("a side that may trim may close");
            let (mine, theirs) = if side == LONG { (cl, cs) } else { (cs, cl) };
            assert!(take < mine, "{side} at {p}: the trim takes {take}, the close pays {mine}");
            // The true price when the claim lands, anywhere from 7 to 14.
            for k in 0..=70 {
                let t = px(7.0) + k * px(0.1);
                let (before, after) = if side == LONG { (lp.s.short_at(t), part.short_at(t)) } else { (lp.s.long_at(t), part.long_at(t)) };
                assert!(after <= before);
                assert!(after >= before.min(theirs), "{side} at {p}, the price really {t}: {after} left of {before}; a close pays {theirs}");
                worst = worst.max(before - after);
                // Inside the new range nothing changed for the other party.
                if t >= part.bl && t <= part.bs {
                    assert_eq!(after, before);
                }
            }
        }
    }
    assert!(trims >= 20, "{trims}");
    println!("C8: {trims} trims at their limit; the most a stale signed price cost the other party over the grid: {} KAS of a pair of 3,000 (a close at the same time: more)", worst as f64 / KAS1 as f64);
}

/// A trim to a sompi inside the entry leaves that side a sliver of value at the entry, and `checkPart` wants every
/// piece of a split to give such a side PART_FLOOR: no piece does, so nobody splits the pair any more. The other
/// party closed half before the trim; after it, it closes whole or not at all. (Trimmed to the entry or beyond, the
/// side has no value at the entry and splits are allowed again.)
#[test]
fn c8_bounded_a_trim_to_just_inside_the_entry_ends_every_split_of_the_pair() {
    let lp = opened();
    let lt = epoch_epoch_table();
    let half = SIZE / 2;
    let short_closes = |pair: &LPair, n: i64| propose_tx(pair, &lt, &Propose::close(&lt, SHORT, n, px(11.0), 6_600, &short_kp()), |_| {});
    must("control: the short closes half of the pair", &short_closes(&lp, half));
    assert!(lp.s.split_ok(half));
    // The long is healthy down to 10.7065 at 11.5 and asks for a sompi inside the entry of 10.
    let nb = px(10.0) - 1;
    let lw = trimmed(&lp, &lt, &Trim::of(&lt, LONG, nb, px(11.5), 6_500, &long_kp()), 6_510);
    let t = finalize_tx(&lw, 0, |_| {});
    must("the trim settles", &t);
    let part = lp.s.trim_state(LONG, nb);
    let thin = live(&t, 1, 6_520, &part);
    assert!(!part.split_ok(half) && !part.split_ok(SIZE - cmarket().p.min_size) && part.split_ok(SIZE));
    refused("the short closes half of the trimmed pair", &short_closes(&thin, half), 0);
    must("the short closes all of it", &short_closes(&thin, SIZE));
    refused("half of the trimmed long is liquidated", &propose_tx(&thin, &lt, &Propose::liq(&lt, LONG, half, px(10.2), 6_600), |_| {}), 0);
    must("all of it is", &propose_tx(&thin, &lt, &Propose::liq(&lt, LONG, SIZE, px(10.2), 6_600), |_| {}));
    // At the entry itself the side has no value there, and the floor does not apply.
    let at = lp.s.trim_state(LONG, px(10.0));
    assert!(at.split_ok(half));
}

/// A trim cannot shelter a pair from a liquidation that is due: the side that is liquidated at the present signed price is
/// not healthy at it from any nearer bound, and with a signed price of a better moment (still within lateDaa) its trim only
/// brings its bound nearer. The pending trim is settled by anyone at once, and the liquidation follows on the
/// trimmed pair with no quiet time.
#[test]
fn c8_bounded_a_trim_delays_a_due_liquidation_by_one_transaction_and_pays_for_it() {
    let lp = opened();
    let lt = epoch_epoch_table();
    // At 9.2 the long (bound 9) is liquidated: 9.2 x 1.02 x 0.95 < 9.
    assert!(lp.s.long_liquidatable(px(9.2)));
    must("control: the liquidation at 9.2", &propose_tx(&lp, &lt, &Propose::liq(&lt, LONG, SIZE, px(9.2), 6_560), |_| {}));
    // At that time no trim of the long is accepted, to any bound.
    assert_eq!(lp.s.trim_bound_of(LONG, px(9.2)).filter(|nb| lp.s.trim_ok(LONG, px(9.2), *nb)), None);
    refused("a trim by a side that is liquidated at the signed price", &trim_tx(&lp, &lt, &Trim::of(&lt, LONG, px(9.0) + 1, px(9.2), 6_560, &long_kp()), |_| {}), 0);
    // With the signed price of 60 DAA before (11.5; both signed prices are within lateDaa of the pair's present) it trims to 10.
    let lw = trimmed(&lp, &lt, &Trim::of(&lt, LONG, px(10.0), px(11.5), 6_500, &long_kp()), 6_565);
    // The keeper's liquidation of the pair is gone with the pair; it settles the trim (no age is asked) ...
    let t = finalize_tx(&lw, 0, |_| {});
    must("anyone settles the trim at once", &t);
    let part = lp.s.trim_state(LONG, px(10.0));
    let thin = live(&t, 1, 6_566, &part);
    // ... and liquidates the trimmed pair with the signed price it had (no quiet time; the signed price is within lateDaa of the new UTXO).
    let liq = Propose::liq(&lt, LONG, SIZE, px(9.2), 6_560);
    must("the liquidation follows on the trimmed pair", &propose_tx(&thin, &lt, &liq, |_| {}));
    // What the trim took at a signed price the price had left: 1,000 KAS of a side worth 200 KAS at 9.2; the short's side is
    // worth 2,800 KAS at 9.2 and the trimmed pair holds 2,000. A close at the same stale signed price would have left it 730.
    let (cl, cs) = close_pays(&lp, LONG, px(11.5), &long_kp()).unwrap();
    assert_eq!((lp.s.trim_take(LONG, px(10.0)), lp.s.long_at(px(9.2)), lp.s.short_at(px(9.2)), part.short_at(px(9.2))), (1_000 * KAS1, 200 * KAS1, 2_800 * KAS1, 2_000 * KAS1));
    assert_eq!((cl, cs), (2_270 * KAS1, 730 * KAS1 + cmarket().p.carrier), "the short's 730 KAS and the pair's carrier");
    // The same trim landing after lateDaa is void, and a void trim is restored by anyone at once.
    let late = trimmed(&lp, &lt, &Trim::of(&lt, LONG, px(10.0), px(11.5), 6_500, &long_kp()), 6_601);
    assert!(matches!(late.s.settlement(late.value, 6_601), CSettlement::Restore { .. }));
    must("a void trim is restored at once", &finalize_tx(&late, 0, |_| {}));
}

/// One key on both sides of a pair (a wound-up pair): it trims either side, taking what is its own, and is not
/// liquidated.
#[test]
fn c8_holds_a_pair_of_one_key_trims_and_is_not_liquidated() {
    let lp0 = opened();
    let lt = epoch_epoch_table();
    let both = CPair { short_key: lp0.s.long_key, ..lp0.s.clone() };
    let lp = pair(&both, 6_100);
    for (side, nb, p) in [(LONG, px(10.0), px(11.5)), (SHORT, px(11.0), px(9.5))] {
        let lw = trimmed(&lp, &lt, &Trim::of(&lt, side, nb, p, 6_500, &long_kp()), 6_510);
        must("the one owner's trim settles", &finalize_tx(&lw, 0, |_| {}));
        assert_eq!(lw.s.proposer, lp.s.long_key);
    }
    refused("a trim of the short signed by another key", &trim_tx(&lp, &lt, &Trim::of(&lt, SHORT, px(11.0), px(9.5), 6_500, &short_kp()), |_| {}), 0);
    refused("a pair of one key liquidated", &propose_tx(&lp, &lt, &Propose::liq(&lt, LONG, SIZE, px(9.2), 6_560), |_| {}), 0);
}

/// The trimmed pair, its entry now outside its range, through the entries the family's own test does not drive:
/// listings written before the trim serve after it at the side's new worth, the other side's margin, a
/// mutual close at a price between the old bound and the new, and the lapse.
#[test]
fn c8_holds_the_trimmed_pair_serves_its_listings_and_every_exit() {
    let lp = opened();
    let lt = epoch_epoch_table();
    // Both sides list before the trim: the long asks 11, the short bids 11.2, in parts of half, no tip.
    let t = amend_tx(&lp, LONG, px(11.0), SIZE / 2, 0, &long_kp(), 20, |_| {});
    must("the long lists", &t);
    let mut s1 = lp.s.rest_of(SIZE);
    s1.long_ask = px(11.0);
    s1.long_min = SIZE / 2;
    let l1 = live(&t, 0, 6_200, &s1);
    let t = amend_tx(&l1, SHORT, px(11.2), SIZE / 2, 0, &short_kp(), 20, |_| {});
    must("the short lists", &t);
    let mut s2 = s1.clone();
    s2.short_bid = px(11.2);
    s2.short_min = SIZE / 2;
    let listed = live(&t, 0, 6_300, &s2);
    // The long trims to 10.5 at 11.5: both listings are in the trimmed state.
    let lw = trimmed(&listed, &lt, &Trim::of(&lt, LONG, px(10.5), px(11.5), 6_500, &long_kp()), 6_510);
    let t = finalize_tx(&lw, 0, |_| {});
    must("the trim settles", &t);
    let part = listed.s.trim_state(LONG, px(10.5));
    assert_eq!((part.long_ask, part.long_min, part.short_bid, part.short_min, part.entry), (px(11.0), SIZE / 2, px(11.2), SIZE / 2, px(10.0)));
    let thin = live(&t, 1, 6_520, &part);
    // The long's listing: taken whole at its ask, the leaving long is paid the side's worth from its NEW bound.
    // (The entrant is healthy at 11 from 10.45; from the trimmed bound of 10.5 itself it is not, and is refused.)
    refused("an entrant at the trimmed bound, not healthy at the ask", &transfer_tx(&thin, LONG, SIZE, px(11.0), px(10.5), 20, |_| {}), 0);
    let whole = transfer_tx(&thin, LONG, SIZE, px(11.0), px(10.45), 20, |_| {});
    must("the trimmed long's listing is taken at its ask", &whole);
    assert_eq!(whole.0.outputs[0].value as i64, part.long_at(px(11.0)));
    assert_eq!(part.long_at(px(11.0)), 500 * KAS1);
    refused("below its ask", &transfer_tx(&thin, LONG, SIZE, px(11.0) - 1, px(10.45), 20, |_| {}), 0);
    // The short's listing, taken at 10.4, below the long's new bound of 10.5: the leaving short is paid its side's
    // worth at 10.4 from ITS bound (1,600 KAS), by the entrant, though the trimmed pair holds 1,500 KAS in all. The
    // short's bid means after the trim what it meant before; the trim took nothing from it.
    let t = transfer_tx(&thin, SHORT, SIZE, px(10.4), thin.s.bs, 20, |_| {});
    must("the short's listing is taken below the trimmed range", &t);
    assert_eq!((t.0.outputs[0].value as i64, thin.value as i64 - cmarket().p.carrier), (1_600 * KAS1, 1_500 * KAS1));
    refused("the leaving short paid what the pair holds, not what its bid says", &transfer_tx(&thin, SHORT, SIZE, px(10.4), thin.s.bs, 20, |o| o[0].value = 1_500 * KAS1 as u64), 0);
    // The other side adds margin, both close together inside the new range, the pair lapses.
    must("the short adds margin", &add_margin_tx(&thin, SHORT, px(1.0), &short_kp(), 20, |_| {}));
    must("a mutual close at 11.1", &mutual_tx(&thin, px(11.1), 20, |_| {}));
    assert_eq!(part.lapse_out(thin.value), (0, thin.value as i64));
    must("the lapse", &lapse_tx(&thin, 15_000, |_| {}));
    refused("the lapse paying the trimmed long", &lapse_tx(&thin, 15_000, |o| { o[0] = out(KAS1 as u64, key_spk(&part.long_key), None); o[2].value -= KAS1 as u64; }), 0);
}
