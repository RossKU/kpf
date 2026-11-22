//! The committee family in the script engine (docs/DESIGN-COMMITTEE.md 3): the life of a pair priced by a committee's
//! epoch tables (two orders, a pair; a close and a liquidation at an attested price, finalized, cured, void; the lease kept
//! alive and lapsed; a side assigned, transferred, taken by an order) and the negatives of every rule. States and values
//! come from the library model (`kpf_protocol::cmarket`); every accepted transaction pays exactly what the model says,
//! and one sompi less is refused where a value is checked.
//!
//! Fixture (`committeekit::cmarket`): the committee's epoch [6,000, 7,000) (100 KAS of weight, the three largest members 67);
//! a lease of 8,000 DAA, a lapse 1,000 after it, 50 DAA to cure, a claim valid within 100 DAA of its time, 20 DAA of
//! quiet, mmr 5 %, g 2 %. The worked pair: 1,000 units at 10 KAS, the long's bound at 9, the short's at 12, opened at
//! 6,100 with the lease 14,000.

mod committeekit;
mod common;

use committeekit::*;
use common::*;
use kpf_protocol::cmarket::*;
use kpf_protocol::committee::*;
use kpf_protocol::state::{KIND_LIQ, LONG, SHORT};

const KAS1: i64 = 100_000_000;
const SIZE: i64 = 1_000_000_000;

#[test]
fn the_states_are_the_models_bytes() {
    let k = fk();
    assert_eq!(k.kit.pair.state_len, CPAIR_STATE_LEN);
    assert_eq!(k.kit.pending.state_len, CPENDING_STATE_LEN);
    assert_eq!(k.kit.open.state_len, COPEN_STATE_LEN);
    let m = cmarket();
    assert_eq!(m.bytes().len(), CMARKET_LEN);
    assert!(m.check().is_ok());
    assert_eq!(CMarket::decode(&m.bytes()).unwrap(), m);
    let sp = sample_pair(&m);
    assert_eq!(k.kit.pair.state_of(&bytecode(&k.pair)).unwrap(), &sp.state_bytes()[..]);
    assert_eq!(CPair::decode_state(&sp.state_bytes()).unwrap(), sp);
    let so = sample_order(&m, LONG, [3; 32]);
    assert_eq!(k.kit.open.state_of(&bytecode(&k.open)).unwrap(), &so.state_bytes()[..]);
    assert_eq!(COpen::decode_state(&so.state_bytes()).unwrap(), so);
    let (w, _) = sp.pending_of(&k.kit, sp.size, KIND_LIQ, LONG, 900_000_000, 6_500, [5; 32]).unwrap();
    assert_eq!(CPending::decode_state(&w.state_bytes()).unwrap(), w);
    assert_eq!(w.state_bytes().len(), CPENDING_STATE_LEN);
    println!(
        "scripts: KpfPairC {} bytes, KpfPendingC {} bytes, KpfOpenC {} bytes",
        bytecode(&k.pair).len(),
        bytecode(&k.pending).len(),
        bytecode(&k.open).len()
    );
}

#[test]
fn the_attest_signature_script_is_where_the_pair_reads_it() {
    // The layout KpfPairC.priced checks byte by byte: the six arguments, the dispatch tag, the redeem script.
    let lt = epoch_epoch_table();
    let a = att(&lt.s, 6_500, 1_000_000_000);
    let t = attest_tx(&lt, &a, 6_500, |_| {});
    must("the attest alone", &t);
    let ss = &t.0.inputs[0].signature_script;
    let k = kit();
    assert_eq!(ss.len(), ATTEST_HEAD + k.epoch_table.prefix.len() + EPOCH_TABLE_STATE_LEN + k.epoch_table.suffix.len());
    assert_eq!(ss[0], 0x20);
    assert_eq!(&ss[1..33], &cmarket().id());
    assert_eq!(ss[33], 0x08);
    assert_eq!(&ss[34..42], &kpf_protocol::state::sm8(6_500));
    assert_eq!(ss[42], 0x08);
    assert_eq!(&ss[43..51], &kpf_protocol::state::sm8(1_000_000_000));
    assert_eq!(ss[51], 0x08);
    assert_eq!(&ss[60..62], &[0x4c, 0x78]);
    assert_eq!(&ss[182..185], &[0x4d, 0xc0, 0x03]);
    assert_eq!(ss[1145], 0x04);
    assert_eq!(&ss[1146..1150], &k.consts.attest_tag);
    assert_eq!(ss[1150], 0x4d);
    // A node admits at most 15 signature operations in an input's script and counts the epoch table's data signatures.
    assert_eq!(kpf_protocol::verify::data_sig_ops(ss), 15);
}

#[test]
fn two_orders_open_a_pair() {
    let lp = opened();
    assert_eq!(lp.value as i64, 3_000 * KAS1 + 20_000_000);
    assert_eq!((lp.s.bl, lp.s.bs, lp.s.expiry), (900_000_000, 1_200_000_000, 14_000));
    let (lo, so) = (order(LONG, &long_kp(), 11, 6_000), order(SHORT, &short_kp(), 12, 6_000));
    let open = |xe: i64, lock: u64| open_tx(&lo, &so, SIZE, 1_000_000_000, xe, lock, |_| {});
    // The lease: at least a term after the orders were accepted (6,000 + 8,000), at most a term from the lock time.
    refused("a lease one DAA short of a term after the orders", &open(13_999, 6_100), 0);
    must("a lease a term after the lock time", &open(14_100, 6_100));
    refused("a lease one DAA beyond a term from the lock time", &open(14_101, 6_100), 0);
    must("the same lease with a later lock time", &open(14_101, 6_101));
    // The pair holds its range and a carrier.
    let t = open_tx(&lo, &so, SIZE, 1_000_000_000, 14_000, 6_100, |o| o[2].value -= 1);
    refused("a pair of one sompi less", &t, 0);
    // The pair's lease is the one both orders were given.
    let other = opened_pair(&lo, &so, SIZE, 1_000_000_000, 14_050);
    let t = open_tx(&lo, &so, SIZE, 1_000_000_000, 14_000, 6_100, |o| o[2].script_public_key = pair_spk(&other));
    refused("a pair written with another lease", &t, 0);
}

#[test]
fn a_close_at_the_attested_price_settles_at_once() {
    let lp = opened();
    let lt = epoch_epoch_table();
    // The long closes at 10.5 KAS, attested for the time 6,500; the claim lands at 6,510.
    let pr = Propose::close(&lt, LONG, SIZE, 1_050_000_000, 6_500, &long_kp());
    let (lw, rest) = proposed(&lp, &lt, &pr, 6_510);
    assert!(rest.is_none());
    assert!(lw.s.valid_at(6_510));
    // Paid at 10.5 less g (2 %): 10.29, so the long gets 1,000 x (10.29 - 9) = 1,290 KAS and the short the rest.
    assert_eq!(lw.s.close_price(), 1_029_000_000);
    assert_eq!(lw.s.settlement(lw.value, 6_510), CSettlement::Close { long: 1_290 * KAS1, short: 1_710 * KAS1 + 20_000_000 });
    must("finalize at once (no age)", &finalize_tx(&lw, 0, |_| {}));
    refused("the long one sompi more than its due leaves the short short", &finalize_tx(&lw, 0, |o| { o[0].value += 1; o[2].value -= 1; }), 0);
    refused("the long one sompi less", &finalize_tx(&lw, 0, |o| o[0].value -= 1), 0);
    refused("the long's payout to another key", &finalize_tx(&lw, 0, |o| o[0].script_public_key = key_spk(&x(&exec_kp()))), 0);
    refused("the short's payout to another key", &finalize_tx(&lw, 0, |o| o[2].script_public_key = key_spk(&x(&exec_kp()))), 0);
    // The same claim landing 101 DAA after its time is void: the pair is restored whole.
    let (late, _) = proposed(&lp, &lt, &pr, 6_601);
    assert!(!late.s.valid_at(6_601));
    assert_eq!(late.s.settlement(late.value, 6_601), CSettlement::Restore { part: lp.s.restore_state(SIZE), value: lp.value as i64 });
    must("a void close restores the pair", &finalize_tx(&late, 0, |_| {}));
    refused("a void close restored with one sompi less", &finalize_tx(&late, 0, |o| o[0].value -= 1), 0);
    // At exactly lateDaa it is valid.
    let (edge, _) = proposed(&lp, &lt, &pr, 6_600);
    assert!(edge.s.valid_at(6_600));
    must("valid at t0 + lateDaa", &finalize_tx(&edge, 0, |_| {}));
}

#[test]
fn a_close_needs_its_owner_the_quiet_time_and_the_attested_price() {
    let lp = opened();
    let lt = epoch_epoch_table();
    let good = Propose::close(&lt, LONG, SIZE, 1_050_000_000, 6_500, &long_kp());
    must("the long's close", &propose_tx(&lp, &lt, &good, |_| {}));
    refused("signed by the short", &propose_tx(&lp, &lt, &Propose { signer: Some(short_kp()), ..good.clone() }, |_| {}), 0);
    refused("signed by a stranger", &propose_tx(&lp, &lt, &Propose { signer: Some(exec_kp()), ..good.clone() }, |_| {}), 0);
    refused("before the quiet time", &propose_tx(&lp, &lt, &Propose { seq: 19, ..good.clone() }, |_| {}), 0);
    // A price the epoch table did not attest: the claim names 10.6, the signatures are for 10.5.
    let mut c = good.c;
    c.p = 1_060_000_000;
    refused("another price than the attested one", &propose_tx(&lp, &lt, &Propose { c, ..good.clone() }, |_| {}), 0);
    // Another signed price than the attested one.
    let mut c = good.c;
    c.t0 = 6_501;
    assert!(!accepted(&propose_tx(&lp, &lt, &Propose { c, lock: 6_501, ..good.clone() }, |_| {})), "another signed price");
    // A close that would leave the closer below its maintenance at the price it is paid: the long's bound is 9, so
    // floor(p x 0.98) x 0.95 >= 9 needs p >= 9.667...; 9.66 is refused, 9.67 accepted.
    let low = Propose::close(&lt, LONG, SIZE, 966_000_000, 6_500, &long_kp());
    assert!(!lp.s.close_ok(LONG, 966_000_000));
    refused("a close below the closer's maintenance", &propose_tx(&lp, &lt, &low, |_| {}), 0);
    assert!(lp.s.close_ok(LONG, 967_000_000));
    must("a close just above it", &propose_tx(&lp, &lt, &Propose::close(&lt, LONG, SIZE, 967_000_000, 6_500, &long_kp()), |_| {}));
    // The short's mirror: bs 12 >= ceil(p x 1.02) x 1.05 needs p <= 11.204...
    assert!(lp.s.close_ok(SHORT, 1_120_000_000) && !lp.s.close_ok(SHORT, 1_121_000_000));
    must("the short's close at 11.2", &propose_tx(&lp, &lt, &Propose::close(&lt, SHORT, SIZE, 1_120_000_000, 6_500, &short_kp()), |_| {}));
    refused("the short's close at 11.21", &propose_tx(&lp, &lt, &Propose::close(&lt, SHORT, SIZE, 1_121_000_000, 6_500, &short_kp()), |_| {}), 0);
    // The pending part holds the pair's whole value.
    refused("a pending part of one sompi less", &propose_tx(&lp, &lt, &good, |o| o[0].value -= 1), 0);
}

#[test]
fn a_liquidation_at_the_attested_price_forfeits_after_the_grace() {
    let lp = opened();
    let lt = epoch_epoch_table();
    // The long's trigger: bl / (1 - mmr) = 9.4737; with g inside, ceil(p x 1.02) x 0.95 < 9 needs p < 9.2879.
    assert!(lp.s.long_liquidatable(928_000_000) && !lp.s.long_liquidatable(929_000_000));
    refused("9.29 does not liquidate the long", &propose_tx(&lp, &lt, &Propose::liq(&lt, LONG, SIZE, 929_000_000, 6_500), |_| {}), 0);
    let pr = Propose::liq(&lt, LONG, SIZE, 928_000_000, 6_500);
    let (lw, _) = proposed(&lp, &lt, &pr, 6_510);
    // The fee: 0.5 % of 9.28 x 1,000 = 46.4 KAS, within the maintenance slice (473.68 KAS); forfeit and keep.
    assert_eq!(lw.s.fee, 46 * KAS1 + 40_000_000);
    assert_eq!(lw.s.proposer, x(&exec_kp()));
    let CSettlement::Forfeit { part, value, fee } = lw.s.settlement(lw.value, 6_510) else { panic!("a forfeit") };
    assert_eq!((part.long_key, part.short_key), (x(&short_kp()), x(&short_kp())));
    assert_eq!((value, fee), (lw.value as i64 - lw.s.fee, lw.s.fee));
    refused("finalize before the grace", &finalize_tx(&lw, 49, |_| {}), 0);
    must("finalize after the grace", &finalize_tx(&lw, 50, |_| {}));
    refused("the fee one sompi short", &finalize_tx(&lw, 50, |o| o[0].value -= 1), 0);
    refused("the fee to another key", &finalize_tx(&lw, 50, |o| o[0].script_public_key = key_spk(&x(&long_kp()))), 0);
    refused("the forfeited part one sompi short", &finalize_tx(&lw, 50, |o| o[1].value -= 1), 0);
    refused("the part restored instead of forfeited", &finalize_tx(&lw, 50, |o| o[1].script_public_key = pair_spk(&lp.s.restore_state(SIZE))), 0);
    // The short's mirror: bs 12 < floor(p x 0.98) x 1.05 needs p > 11.6618.
    assert!(lp.s.short_liquidatable(1_167_000_000) && !lp.s.short_liquidatable(1_166_000_000));
    must("11.67 liquidates the short", &propose_tx(&lp, &lt, &Propose::liq(&lt, SHORT, SIZE, 1_167_000_000, 6_500), |_| {}));
    refused("11.66 does not", &propose_tx(&lp, &lt, &Propose::liq(&lt, SHORT, SIZE, 1_166_000_000, 6_500), |_| {}), 0);
    // A void liquidation (landed 101 after its time) restores the pair, after the grace too.
    let (late, _) = proposed(&lp, &lt, &pr, 6_601);
    must("a void liquidation restores", &finalize_tx(&late, 50, |_| {}));
    refused("a void liquidation paid as a forfeit", &finalize_tx(&late, 50, |o| { *o = vec![out(late.s.fee as u64, key_spk(&late.s.proposer), None), out((late.value as i64 - late.s.fee) as u64, pair_spk(&part), bind(0, late.id.unwrap())), change(1)]; }), 0);
}

#[test]
fn the_loser_cures_a_pending_liquidation() {
    let lp = opened();
    let lt = epoch_epoch_table();
    let (lw, _) = proposed(&lp, &lt, &Propose::liq(&lt, LONG, SIZE, 928_000_000, 6_500), 6_510);
    let (cured, value) = lw.s.cure(lw.value).expect("a cure");
    // The cure bound: ceil(9.28 x 0.98) x 0.95 = 8.63968: no claim within g of 9.28 reopens the cured part.
    assert_eq!(cured.bl, 863_968_000);
    assert!(!cured.long_liquidatable(928_000_000));
    assert_eq!(value, lw.value as i64 + 1_000 * (900_000_000 - 863_968_000));
    must("the long cures", &cure_tx(&lw, &long_kp(), |_| {}));
    refused("the short cannot cure the long", &cure_tx(&lw, &short_kp(), |_| {}), 0);
    refused("one sompi less than the margin moved", &cure_tx(&lw, &long_kp(), |o| o[1].value -= 1), 0);
    refused("a cure that keeps the old bound", &cure_tx(&lw, &long_kp(), |o| o[1].script_public_key = pair_spk(&lp.s.restore_state(SIZE))), 0);
    // A close has no cure.
    let (cw, _) = proposed(&lp, &lt, &Propose::close(&lt, LONG, SIZE, 1_050_000_000, 6_500, &long_kp()), 6_510);
    assert!(cw.s.cure(cw.value).is_none());
    refused("a close is not cured", &cure_tx(&cw, &long_kp(), |_| {}), 0);
}

#[test]
fn a_part_of_a_pair_is_claimed_and_the_rest_continues() {
    let lp = opened();
    let lt = epoch_epoch_table();
    let n = 400_000_000;
    let pr = Propose::liq(&lt, LONG, n, 928_000_000, 6_500);
    let (lw, rest) = proposed(&lp, &lt, &pr, 6_510);
    let rest = rest.unwrap();
    // The part holds its range and the carrier the proposer funded; the remainder keeps the pair's own carrier.
    assert_eq!(lw.s.refund, 20_000_000);
    assert_eq!(lw.value as i64, 400 * 3 * KAS1 + 20_000_000);
    assert_eq!(rest.value as i64, 600 * 3 * KAS1 + 20_000_000);
    refused("a part of one sompi less", &propose_tx(&lp, &lt, &pr, |o| o[0].value -= 1), 0);
    refused("a remainder of one sompi less", &propose_tx(&lp, &lt, &pr, |o| o[2].value -= 1), 0);
    refused("a remainder of another size", &propose_tx(&lp, &lt, &pr, |o| o[2].script_public_key = pair_spk(&lp.s.rest_of(SIZE - n + 1))), 0);
    must("the part forfeits", &finalize_tx(&lw, 50, |_| {}));
    // A cure repays the proposer its carrier.
    must("the part is cured", &cure_tx(&lw, &long_kp(), |_| {}));
    refused("a cure that does not repay the proposer's carrier", &cure_tx(&lw, &long_kp(), |o| o[0].value -= 1), 0);
    // A close of a part returns the closer its carrier with its payout.
    let (cw, _) = proposed(&lp, &lt, &Propose::close(&lt, LONG, n, 1_050_000_000, 6_500, &long_kp()), 6_510);
    assert_eq!(cw.s.settlement(cw.value, 6_510), CSettlement::Close { long: 400 * 129 * KAS1 / 100 + 20_000_000, short: 400 * 171 * KAS1 / 100 });
    must("the part closes", &finalize_tx(&cw, 0, |_| {}));
    // A part below the market's minimum, and a split that leaves less than the part floor on a side, are refused.
    refused("a part below minSize", &propose_tx(&lp, &lt, &Propose::liq(&lt, LONG, 999_999, 928_000_000, 6_500), |_| {}), 0);
}
