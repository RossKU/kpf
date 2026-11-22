//! Cross margin in the script engine (docs/DESIGN-COMMITTEE.md 3a): a trim (`KpfPairC.propose` kind 3) takes out of a
//! side the margin it has above maintenance at an attested price, through a pending part that is void unless it
//! landed within lateDaa of its time, and the same owner adds it to another pair. States and values come from the
//! library model; every accepted transaction pays exactly what the model says and one sompi more is refused.
//!
//! Fixture (`committeekit::cmarket`): mmr 5 %, g 2 %, a claim valid within 100 DAA of its time, 20 DAA of quiet. The
//! worked pair: 1,000 units, the long's bound at 9, the short's at 12, entry 10, opened at 6,100.

mod committeekit;
mod common;

use committeekit::*;
use common::*;
use kpf_protocol::cmarket::*;
use kpf_protocol::state::{KIND_TRIM, LONG, SHORT};

const KAS1: i64 = 100_000_000;
const SIZE: i64 = 1_000_000_000;

#[test]
fn a_side_takes_out_the_margin_it_has_above_maintenance() {
    let lp = opened();
    let lt = epoch_epoch_table();
    // At 11.5 the long is healthy down to floor(floor(11.5 x 0.98) x 0.95) = 10.7065: the nearest bound it may ask.
    assert_eq!(lp.s.trim_bound_of(LONG, 1_150_000_000), Some(1_070_650_000));
    // It asks for 10.5: 1,000 x (10.5 - 9) = 1,500 KAS leave the pair.
    let tr = Trim::of(&lt, LONG, 1_050_000_000, 1_150_000_000, 6_500, &long_kp());
    let lw = trimmed(&lp, &lt, &tr, 6_510);
    assert_eq!((lw.s.kind, lw.s.cure_bound, lw.s.proposer, lw.s.fee, lw.s.refund), (KIND_TRIM, 1_050_000_000, x(&long_kp()), 0, 0));
    assert_eq!(lw.value, lp.value);
    let part = lp.s.trim_state(LONG, 1_050_000_000);
    assert_eq!((part.bl, part.bs, part.entry, part.expiry), (1_050_000_000, lp.s.bs, lp.s.entry, lp.s.expiry));
    assert_eq!(lw.s.settlement(lw.value, 6_510), CSettlement::Trim { part: part.clone(), value: lp.value as i64 - 1_500 * KAS1, take: 1_500 * KAS1 });
    assert_eq!(lw.s.finalize_age(6_510), 0);
    must("a trim settles at once", &finalize_tx(&lw, 0, |_| {}));
    refused("one sompi more to the owner", &finalize_tx(&lw, 0, |o| { o[0].value += 1; o[1].value -= 1; }), 0);
    refused("one sompi less to the owner", &finalize_tx(&lw, 0, |o| o[0].value -= 1), 0);
    refused("the freed margin to another key", &finalize_tx(&lw, 0, |o| o[0].script_public_key = key_spk(&x(&exec_kp()))), 0);
    refused("the pair kept at its old bound", &finalize_tx(&lw, 0, |o| o[1].script_public_key = pair_spk(&lp.s.restore_state(SIZE))), 0);
    refused("a bound nearer than the one asked for", &finalize_tx(&lw, 0, |o| o[1].script_public_key = pair_spk(&lp.s.trim_state(LONG, 1_060_000_000))), 0);
    // The other party's side is worth the same at every price inside the new range.
    for px in [1_050_000_000, 1_100_000_000, 1_150_000_000, 1_200_000_000] {
        assert_eq!(part.short_at(px), lp.s.short_at(px));
    }
    // The same trim landing 101 DAA after its time is void: the pair is restored whole, and nothing is paid.
    let late = trimmed(&lp, &lt, &tr, 6_601);
    assert_eq!(late.s.settlement(late.value, 6_601), CSettlement::Restore { part: lp.s.restore_state(SIZE), value: lp.value as i64 });
    must("a void trim restores the pair", &finalize_tx(&late, 0, |_| {}));
    let paid = |o: &mut Vec<_>| *o = vec![out(1_500 * KAS1 as u64, key_spk(&x(&long_kp())), None), out(lp.value - 1_500 * KAS1 as u64, pair_spk(&part), bind(0, late.id.unwrap())), change(1)];
    refused("a void trim paid as a trim", &finalize_tx(&late, 0, paid), 0);
    must("valid at t0 + lateDaa", &finalize_tx(&trimmed(&lp, &lt, &tr, 6_600), 0, |_| {}));
}

#[test]
fn a_trim_is_not_cured() {
    // `cure` moves a liquidated side to its cure bound for margin added, at any time and whatever the claim's
    // validity. A trim's cure bound is the bound it asks for: were it curable, its owner would take the margin out of
    // a void claim (a signed price of a better time) by signing.
    let lp = opened();
    let lt = epoch_epoch_table();
    let tr = Trim::of(&lt, LONG, 1_050_000_000, 1_150_000_000, 6_500, &long_kp());
    for landed in [6_510, 6_601] {
        let lw = trimmed(&lp, &lt, &tr, landed);
        assert!(lw.s.cure(lw.value).is_none());
        let part = lp.s.trim_state(LONG, 1_050_000_000);
        let taken = |o: &mut Vec<_>| *o = vec![change(1), out(lp.value - 1_500 * KAS1 as u64, pair_spk(&part), bind(0, lw.id.unwrap())), change(1)];
        refused("a trim cured by its owner", &cure_tx(&lw, &long_kp(), taken), 0);
        refused("a trim cured, the pair's value kept", &cure_tx(&lw, &long_kp(), |o| o[1] = out(lp.value, pair_spk(&part), bind(0, lw.id.unwrap()))), 0);
    }
}

#[test]
fn a_trim_needs_its_owner_the_quiet_time_and_a_bound_it_is_healthy_at() {
    let lp = opened();
    let lt = epoch_epoch_table();
    let good = Trim::of(&lt, LONG, 1_050_000_000, 1_150_000_000, 6_500, &long_kp());
    must("the long's trim", &trim_tx(&lp, &lt, &good, |_| {}));
    refused("signed by the short", &trim_tx(&lp, &lt, &Trim { signer: short_kp(), ..good.clone() }, |_| {}), 0);
    refused("signed by a stranger", &trim_tx(&lp, &lt, &Trim { signer: exec_kp(), ..good.clone() }, |_| {}), 0);
    refused("before the quiet time", &trim_tx(&lp, &lt, &Trim { seq: 19, ..good.clone() }, |_| {}), 0);
    refused("a pending part of one sompi less", &trim_tx(&lp, &lt, &good, |o| o[0].value -= 1), 0);
    // Another price than the attested one: the claim names 11.6, the signatures are for 11.5.
    refused("another price than the attested one", &trim_tx(&lp, &lt, &Trim { p: 1_160_000_000, nb: 1_075_000_000, ..good.clone() }, |_| {}), 0);
    // The nearest bound is accepted and one sompi nearer is refused.
    must("the nearest bound", &trim_tx(&lp, &lt, &Trim { nb: 1_070_650_000, ..good.clone() }, |_| {}));
    assert!(!lp.s.trim_ok(LONG, 1_150_000_000, 1_070_650_001));
    refused("one sompi nearer", &trim_tx(&lp, &lt, &Trim { nb: 1_070_650_001, ..good.clone() }, |_| {}), 0);
    // A trim moves the bound inward, and by enough to free an output's worth (0.02 KAS: 2,000 sompi a unit here).
    refused("the bound where it is", &trim_tx(&lp, &lt, &Trim { nb: 900_000_000, ..good.clone() }, |_| {}), 0);
    refused("the bound moved outward", &trim_tx(&lp, &lt, &Trim { nb: 890_000_000, ..good.clone() }, |_| {}), 0);
    refused("less than 0.02 KAS freed", &trim_tx(&lp, &lt, &Trim { nb: 900_001_999, ..good.clone() }, |_| {}), 0);
    must("0.02 KAS freed", &trim_tx(&lp, &lt, &Trim { nb: 900_002_000, ..good.clone() }, |_| {}));
    // The whole pair only.
    refused("a part of the pair", &trim_tx(&lp, &lt, &Trim { n: 400_000_000, ..good.clone() }, |_| {}), 0);
    // The short's mirror at 10.2: ceil(ceil(10.2 x 1.02) x 1.05) = 10.9242.
    assert_eq!(lp.s.trim_bound_of(SHORT, 1_020_000_000), Some(1_092_420_000));
    let sh = |nb: i64| Trim::of(&lt, SHORT, nb, 1_020_000_000, 6_500, &short_kp());
    must("the short's nearest bound", &trim_tx(&lp, &lt, &sh(1_092_420_000), |_| {}));
    refused("the short one sompi nearer", &trim_tx(&lp, &lt, &sh(1_092_419_999), |_| {}), 0);
    refused("the short's trim signed by the long", &trim_tx(&lp, &lt, &Trim { signer: long_kp(), ..sh(1_092_420_000) }, |_| {}), 0);
    refused("the short's bound moved outward", &trim_tx(&lp, &lt, &sh(1_210_000_000), |_| {}), 0);
    // A side that has nothing above maintenance trims nothing: at 9.6 the long's nearest bound is below its own.
    assert_eq!(lp.s.trim_bound_of(LONG, 960_000_000), None);
    refused("a trim of a side at its maintenance", &trim_tx(&lp, &lt, &Trim::of(&lt, LONG, 900_002_000, 960_000_000, 6_500, &long_kp()), |_| {}), 0);
}

#[test]
fn a_trimmed_range_stays_wider_than_both_maintenance_bands() {
    // With the price above the short's bound the long would be healthy at 11.6, but a range [11.6, 12] is narrower
    // than the two bands (bl x 1.05 < bs x 0.95), where neither side could be liquidated: the nearest bound is the
    // widest that keeps the rule, 10.857142855.
    let lp = opened();
    let lt = epoch_epoch_table();
    assert_eq!(lp.s.trim_bound_of(LONG, 1_250_000_000), Some(1_085_714_285));
    let at = |nb: i64| Trim::of(&lt, LONG, nb, 1_250_000_000, 6_500, &long_kp());
    must("the widest that keeps both bands", &trim_tx(&lp, &lt, &at(1_085_714_285), |_| {}));
    assert!(lp.s.trim_state(LONG, 1_085_714_286).close_ok(LONG, 1_250_000_000) && !lp.s.trim_state(LONG, 1_085_714_286).range_ok());
    refused("one sompi nearer", &trim_tx(&lp, &lt, &at(1_085_714_286), |_| {}), 0);
}

#[test]
fn margin_moves_from_one_pair_to_another_and_the_trimmed_pair_lives_on() {
    let lp = opened();
    let lt = epoch_epoch_table();
    // The long takes 1,500 KAS out of this pair ...
    let lw = trimmed(&lp, &lt, &Trim::of(&lt, LONG, 1_050_000_000, 1_150_000_000, 6_500, &long_kp()), 6_510);
    let t = finalize_tx(&lw, 0, |_| {});
    must("the trim settles", &t);
    let part = lp.s.trim_state(LONG, 1_050_000_000);
    let trimmed_pair = live(&t, 1, 6_520, &part);
    assert_eq!(trimmed_pair.value as i64, lp.value as i64 - 1_500 * KAS1);
    // ... and the same amount moves the bound of a side of another pair outward by 1.5 (the same owner's short here).
    let other = opened();
    must("the freed margin is added to another pair", &add_margin_tx(&other, SHORT, 150_000_000, &short_kp(), 20, |_| {}));
    refused("one sompi less than the margin moved", &add_margin_tx(&other, SHORT, 150_000_000, &short_kp(), 20, |o| o[0].value -= 1), 0);
    // The trimmed pair is a pair: its long is liquidated from its new bound (10.5 / 0.969 = 10.836) and not before.
    assert!(part.long_liquidatable(1_083_000_000) && !lp.s.long_liquidatable(1_083_000_000) && !part.long_liquidatable(1_084_000_000));
    must("the trimmed long is liquidated at 10.83", &propose_tx(&trimmed_pair, &lt, &Propose::liq(&lt, LONG, SIZE, 1_083_000_000, 6_530), |_| {}));
    refused("and not at 10.84", &propose_tx(&trimmed_pair, &lt, &Propose::liq(&lt, LONG, SIZE, 1_084_000_000, 6_530), |_| {}), 0);
    // It trims again when the price has moved on, closes, and takes margin back.
    must("a second trim at 11.9", &trim_tx(&trimmed_pair, &lt, &Trim::of(&lt, LONG, 1_080_000_000, 1_190_000_000, 6_600, &long_kp()), |_| {}));
    // (Healthy down to 11.07 at 11.9, but a range [11, 12] would be narrower than the two bands.)
    refused("a second trim that would leave the range too narrow", &trim_tx(&trimmed_pair, &lt, &Trim::of(&lt, LONG, 1_100_000_000, 1_190_000_000, 6_600, &long_kp()), |_| {}), 0);
    must("the short closes the trimmed pair", &propose_tx(&trimmed_pair, &lt, &Propose::close(&lt, SHORT, SIZE, 1_100_000_000, 6_600, &short_kp()), |_| {}));
    must("the long adds margin back", &add_margin_tx(&trimmed_pair, LONG, 150_000_000, &long_kp(), 20, |_| {}));
    // A pair nobody keeps alive settles at its entry clamped to the range: the entry (10) is below the trimmed bound,
    // so the long, which took its margin out above the entry, is owed nothing more.
    assert_eq!(part.lapse_out(trimmed_pair.value), (0, trimmed_pair.value as i64));
    must("the lapse of a trimmed pair", &lapse_tx(&trimmed_pair, 15_000, |_| {}));
}
