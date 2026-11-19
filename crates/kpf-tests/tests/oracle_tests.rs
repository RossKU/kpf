//! The oracle-market family in the script engine (docs/DESIGN-ORACLE-IMPL.md, third slice): the full lifecycle of a
//! pair bound to an underwriter's slot (two orders, a bound pair, a vouched claim; uncontested finalize; contested and
//! refuted, the underwriter's bond paid, the part restored; contested and upheld, finalize; cure), and the negatives of
//! every rule. States and values come from the library model (`kpf_protocol::oracle`); every accepted transaction pays
//! exactly what the model says, and one sompi less is refused where a value is checked.
//!
//! Fixture market (`oraclekit::omarket`): the slot of slice A (step 100, term 800, tail 200, `a` 10, rho 1), the judge
//! of slice B1 (rounds 100, age 500, mu 1.25), W 200, lateDaa 100, quiet 20, idle 1,000. The orders rest from DAA 0, the
//! open lands at 300 with expiry X = 1,200 (one term after the orders, inside the slot's ring); a claim is valid when its
//! pending part lands at P with P + 200 <= 1,200 <= P + 900, i.e. P in [300, 1,000], and P - t0 <= 100.

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use common::*;
use kaspa_consensus_core::tx::TransactionOutput;
use kaspa_consensus_core::Hash;
use kpf_protocol::judge::{UNIT, YES};
use kpf_protocol::oracle::*;
use oraclekit::*;

/// The worked pair: 1,000 units at 10 KAS, long rate 1 (bl 9), short rate 2 (bs 12), cover rate 6, premium 0.02.
fn liq_long(p: i64, t0: i64) -> ClaimSpec {
    ClaimSpec { kind: LIQ, side: L, n: 1_000_000_000, p, t0, lock: t0 as u64 }
}

/// A false liquidation of the long: 9.2 KAS vouched when the market is at 10.
const FALSE_P: i64 = 920_000_000;

/// The contest of `lw` by staker `seed` with a stake of `v`, accepted at `daa`: (contested part, receipt, dispute).
fn contest_at(lw: &LPend, seed: u8, v: u64, daa: u64) -> (LPend, LStake, LStake) {
    let st = stake(seed, v, 0, 0x90 + seed);
    let (t, w2, r, d) = contest_tx(lw, &st, seed, daa);
    must("contest", &t);
    (live(&t, 0, daa, &w2), live(&t, 1, daa, &r), live(&t, 2, daa, &d))
}

/// Resolves and delivers at `lock`: (pool, decided part).
fn decide_at(ld: &LStake, lw: &LPend, lock: u64) -> (LStake, LPend, T) {
    let (t, pool, w2) = decide_tx(ld, lw, lock);
    must("decide", &t);
    (live(&t, 0, lock, &pool), live(&t, 1, lock, &w2), t)
}

// ---------------------------------------------------------------- layout

#[test]
fn layout_state_at_byte_one_and_lengths() {
    let k = kit();
    for (t, len) in [(&k.open, OPEN_STATE_LEN), (&k.pair, PAIR_STATE_LEN), (&k.pending, PENDING_STATE_LEN)] {
        assert_eq!(t.prefix.len(), 1, "{}: the state starts at byte 1", t.name);
        assert_eq!(t.state_len, len, "{}", t.name);
    }
    let m = omarket();
    assert_eq!(m.bytes().len(), MARKET_LEN);
    let p = sample_pair(&m);
    let rs = k.pair.redeem(&p.state_bytes());
    // The market body hashed into the id, the bond block a slot compares, the judge block a stake compares.
    assert_eq!(&rs[1..34], &[&[0x20u8][..], &m.id()[..]].concat()[..]);
    assert_eq!(*blake3::hash(&rs[34..310]).as_bytes(), m.id());
    assert_eq!(&rs[1..88], &m.bond_market().bytes()[..]);
    assert_eq!([&rs[1..34], &rs[88..142]].concat(), m.judge_market().bytes());
    let sm = kpf_protocol::state::sm8;
    assert_eq!(rs[310], 0x08);
    assert_eq!(&rs[311..319], &sm(p.size));
    assert_eq!(&rs[320..328], &sm(p.bl));
    assert_eq!(&rs[329..337], &sm(p.bs));
    assert_eq!(rs[337], 0x08);
    assert_eq!(&rs[413..445], &p.b.uw_id);
    assert_eq!(&rs[446..478], &p.b.uw_key);
    assert_eq!(rs[478], 0x08);
    assert_eq!(&rs[488..496], &sm(p.b.expiry));
    assert_eq!(rs[505], 0x08);
    // Review 7: the five makers (long, short, parent, host, donor) at [559..724).
    for (i, f) in [p.mk.long, p.mk.short, p.mk.parent, p.mk.host, p.mk.donor].iter().enumerate() {
        assert_eq!(rs[559 + 33 * i], 0x20);
        assert_eq!(&rs[560 + 33 * i..592 + 33 * i], f);
    }
    assert_eq!(rs.len() - k.pair.suffix.len(), 724);
    let w = sample_pending(&p);
    let rw = k.pending.redeem(&w.state_bytes());
    assert_eq!(&rw[409..418], &[&[0x08u8][..], &sm(PENDING)[..]].concat()[..]);
    assert_eq!(rw[418], 0x08);
    assert_eq!(rw[427], 0x20);
    assert_eq!(rw[460], 0x08, "the entry push follows the status window");
    // Review 7: the pending part's maker at [781..814).
    assert_eq!(rw[781], 0x20);
    assert_eq!(&rw[782..814], &w.mk_parent);
    assert_eq!(rw.len() - k.pending.suffix.len(), 814);
    let o = sample_order(&m, 1, [3; 32]);
    let ro = k.open.redeem(&o.state_bytes());
    assert_eq!(&ro[353..361], &sm(o.size_left));
    assert_eq!(OPair::decode_state(&p.state_bytes()).unwrap(), p);
    assert_eq!(OPending::decode_state(&w.state_bytes()).unwrap(), w);
    assert_eq!(OOpen::decode_state(&o.state_bytes()).unwrap(), o);
    assert_eq!(OMarket::decode(&m.bytes()).unwrap(), m);
    // A block whose id is not the hash of its body is refused by the decoder (and by checkMarket on chain).
    let mut bad = m.bytes();
    bad[200] ^= 1;
    assert!(OMarket::decode(&bad).is_err());
}

// ---------------------------------------------------------------- the lifecycle

/// Two orders, a bound pair: the slot counts the pair's cover at X, the underwriter is paid the first premium, each
/// order pays its margin and its half of the premium; a fill that leaves a continuation pays no carrier (review 10:
/// only the part that empties an order takes one), so the executor funds the pair's.
#[test]
fn lifecycle_open_binds_the_pair_to_the_slot() {
    let (lp, sl, t) = opened();
    let p = &lp.s;
    assert_eq!((p.b.uw_id, p.b.uw_key, p.b.expiry), (SLOT_ID.as_bytes(), x(&uw_kp()), 1_200));
    assert_eq!(sl.s.counted_at(1_200), p.cover_of(p.size), "the slot counts q(n, coverRate) at X");
    assert_eq!(p.cover_of(p.size), 600_000_000_000);
    // The orders paid exactly their spend: long 1,000 x (1 + 0.01), short 1,000 x (2 + 0.01) KAS (each has 1,000 left).
    assert_eq!(2_000_000_000_000 - t.0.outputs[0].value, 101_000_000_000);
    assert_eq!(2_000_000_000_000 - t.0.outputs[1].value, 201_000_000_000);
    assert_eq!(t.0.outputs[4].value, 2_000_000_000, "the first term's premium, 1,000 x 0.02 KAS, to the underwriter");
}

/// Uncontested: a vouched liquidation of the long finalizes after W, forfeit and keep, the underwriter's bond and fee
/// back to it. One sompi less for the forfeited part or the underwriter is refused.
#[test]
fn lifecycle_uncontested_claim_finalizes() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq_long(FALSE_P, 450), 500);
    assert_eq!(lw.s.bond, 60_000_000_000, "the bond, 10 % of the cover q(1,000, 6) = 600 KAS");
    // Too early: CSV W - 1.
    refused("finalize at W - 1", &finalize_tx(&lw, 500, 199).0, 0);
    let (t, s) = finalize_tx(&lw, 500, 200);
    must("finalize", &t);
    let Settlement::Forfeit { part, value, uw } = s else { panic!("a forfeit") };
    assert_eq!((part.long_key, part.short_key), (x(&short_kp()), x(&short_kp())));
    assert_eq!(uw, lw.s.bond + lw.s.fee);
    assert_eq!(value, lw.value as i64 - lw.s.bond - lw.s.fee);
    for j in [0usize, 1] {
        let mut t2 = finalize_tx(&lw, 500, 200).0;
        t2.0.outputs[j].value -= 1;
        refused("one sompi less", &rebuild(&t2), 0);
    }
}

/// Contested in time, nobody defends the claim: the judge refutes it, the part is restored, the bond goes to the
/// defender (the long) less the disputer's 20 %; the opening staker gets its stake back as free stake.
#[test]
fn lifecycle_contested_refuted_restores_and_pays_the_wronged_party() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq_long(FALSE_P, 450), 500);
    let (cw, receipt, dispute) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    assert_eq!(cw.s.status, CONTESTED);
    assert_eq!(cw.s.clock, 500);
    // Nothing settles while contested: finalize and cure are refused.
    refused("finalize while contested", &finalize_tx(&cw, 500, 200).0, 0);
    // The round's window: resolve at the dispute's DAA + 100, not one before.
    let (early, _, _) = decide_tx(&dispute, &cw, 619);
    assert!(!accepted(&early), "resolution one DAA early");
    let (pool, dw, _) = decide_at(&dispute, &cw, 620);
    assert_eq!(dw.s.status, REFUTED);
    let (t, s) = finalize_tx(&dw, 500, 0);
    must("restore", &t);
    let Settlement::Restore { part, value, defender, disputer, .. } = s else { panic!("a restore") };
    assert_eq!(part, lp.s.restore_state(lp.s.size));
    assert_eq!(value, lw.value as i64 - lw.s.bond);
    assert_eq!((disputer, defender), (12_000_000_000, 48_000_000_000), "20 % of 600 KAS to the disputer, the rest to the long");
    assert_eq!(t.0.outputs[0].script_public_key, key_spk(&x(&long_kp())));
    // The pool pays the opener back (it won, nobody lost) and closes.
    let (st, p2, pays) = settle_tx(&pool, &[receipt.clone()], 621);
    must("settle", &st);
    assert_eq!(pays, vec![receipt.value]);
    let (ct, burn) = pool_close_tx(&live(&st, 0, 622, &p2));
    must("close", &ct);
    assert_eq!(burn, 0);
}

/// Contested, and the underwriter defends with enough aged stake: the judge upholds the claim, the claim finalizes at
/// once; the defenders take the opener's stake less the burn.
#[test]
fn lifecycle_contested_upheld_finalizes() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq_long(FALSE_P, 450), 500);
    let (cw, r_no, dispute) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    // ceil(1.25 x 10 KAS) = 12.5 KAS takes the lead.
    let yes = stake(31, 20 * UNIT as u64, 0, 0xa1);
    let (t, d2, rs) = deposit_tx(&dispute, YES, &[(yes, 31)], 530);
    must("deposit", &t);
    assert_eq!(d2.side, YES);
    let ld2 = live(&t, 0, 530, &d2);
    let r_yes = live(&t, 1, 530, &rs[0]);
    refused("resolve before the new round's window", &decide_tx(&ld2, &cw, 629).0, 0);
    let (pool, dw, _) = decide_at(&ld2, &cw, 630);
    assert_eq!(dw.s.status, UPHELD);
    let (t, s) = finalize_tx(&dw, 500, 0);
    must("finalize upheld", &t);
    assert!(matches!(s, Settlement::Forfeit { .. }));
    // The winner (YES) is paid its stake plus the loser's 10 KAS less the 10 % burn; the loser's receipt is forfeited.
    let (st, p2, pays) = settle_tx(&pool, &[r_yes.clone(), r_no.clone()], 631);
    must("settle", &st);
    assert_eq!(pays, vec![r_yes.value + 9 * UNIT as u64, 0]);
    let (ct, burn) = pool_close_tx(&live(&st, 0, 632, &p2));
    must("close", &ct);
    assert_eq!(burn, UNIT as u64, "the burn: 10 % of the loser's 10 KAS");
}

/// A true liquidation the loser cures while pending: its bound moves to the cure bound with the margin it adds, the
/// underwriter is repaid its refund; the claim is gone.
#[test]
fn lifecycle_cure() {
    let (lp, _, _) = opened();
    // The cover rate leaves headroom: the cured range must stay within it.
    let lw = proposed(&lp, &liq_long(FALSE_P, 450), 500);
    assert!(lw.s.cure_hash != [0; 32]);
    let (t, cured) = cure_tx(&lw, &long_kp());
    must("cure", &t);
    assert_eq!(cured.bl, lw.s.cure_bound);
    assert!(cured.bs - cured.bl <= cured.b.cover_rate);
    // Only the loser cures, and not while contested.
    refused("cure by the short", &cure_tx(&lw, &short_kp()).0, 0);
    let (cw, _, _) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    let mut t2 = cure_tx(&lw, &long_kp()).0;
    let k = kit();
    t2.0.inputs[0].previous_outpoint = cw.op;
    let _ = k;
    let t3 = {
        let ins = vec![pend_in(&cw, "cure", vec![Arg::Sig(long_kp())]), key_in(outpoint(218), 500_000_000_000, 0, &long_kp())];
        build_ext(&ins, t2.0.outputs.clone(), 0, &[0, 0])
    };
    refused("cure while contested", &t3, 0);
}

/// A close: the long (signing) closes at a price the underwriter vouches; at finalize it is paid at the price moved by
/// g against it, plus its refund (it posted the bond); the short gets the rest.
#[test]
fn lifecycle_close_claim() {
    let (lp, _, _) = opened();
    let c = ClaimSpec { kind: CLOSE, side: L, n: lp.s.size, p: 1_050_000_000, t0: 450, lock: 450 };
    // Without the long's signature, or vouched by another key, the close is refused.
    refused("close without the closer's signature", &propose_tx(&lp, &c, &uw_kp(), Some(&short_kp())).0, 0);
    let lw = proposed(&lp, &c, 500);
    let (t, s) = finalize_tx(&lw, 500, 200);
    must("finalize close", &t);
    let Settlement::Close { long, short } = s else { panic!("a close") };
    // 10.5 x 0.98 = 10.29: the long gets 1,000 x 1.29 + its refund 600; the short the rest of 3,000.2 + 600.
    assert_eq!(long, 129_000_000_000 + 60_000_000_000);
    assert_eq!(short, 300_020_000_000 + 60_000_000_000 - long);
}

// ---------------------------------------------------------------- the vouch

/// Only the pair's underwriter can vouch: input j must be uwKey's own P2PK input signed SIGHASH_ALL.
#[test]
fn a_claim_vouched_by_a_key_that_is_not_the_pairs_underwriter_is_refused() {
    let (lp, _, _) = opened();
    let c = liq_long(FALSE_P, 450);
    // Another key's input.
    refused("vouched by a stranger", &propose_tx(&lp, &c, &keypair(77), None).0, 0);
    // The underwriter of another slot of the same market.
    refused("vouched by another underwriter", &propose_tx(&lp, &c, &keypair(32), None).0, 0);
    // The underwriter's key, but signed SINGLE|ANYONECANPAY (it did not commit to every output).
    let (mut t, _) = propose_tx(&lp, &c, &uw_kp(), None);
    let ins = {
        let k = kit();
        let (w, fb) = lp.s.pending_of(k, c.n, c.kind, c.side, c.p, c.t0).unwrap();
        let _ = w;
        vec![
            pair_in(&lp, "propose", vec![iv(c.n), iv(c.kind), iv(c.side), iv(c.p), iv(c.t0), iv(1), bv(&[0u8; 65]), iv(fb), iv(0), bv(&k.pending.prefix), bv(&k.pending.suffix)]),
            In { outpoint: outpoint(210), utxo: kaspa_consensus_core::tx::UtxoEntry::new(50_000_000_000, p2pk_spk(&uw_kp()), 0, false, None), spend: Spend::P2pkType(uw_kp(), 0x84) },
            exec_in(211),
        ]
    };
    t = build_ext(&ins, t.0.outputs.clone(), c.lock, &[0, 0, 0]);
    let v = verdicts(&t);
    assert!(!v[0] && v[1], "the input's own signature is valid for consensus, the pair refuses what it signed: {v:?}");
    // Not the pair's own input as voucher (j = self).
    let (t, _) = propose_tx(&lp, &c, &uw_kp(), None);
    let mut t0 = t.0.clone();
    let _ = &mut t0;
    must("the honest proposal", &t);
}

/// A claim's signature binds exactly this part: copied onto a proposal on another part, or onto the same pair after it
/// changed (a new outpoint), the underwriter's input fails (consensus checks its SIGHASH_ALL signature over the new
/// transaction), so the claim cannot be replayed, reused or moved.
#[test]
fn a_claim_reused_on_another_part_or_after_the_part_changed_is_refused() {
    let (lp, _, _) = opened();
    let c = liq_long(FALSE_P, 450);
    let (t, _) = propose_tx(&lp, &c, &uw_kp(), None);
    must("the original", &t);
    let sig = t.0.inputs[1].signature_script.clone();
    // The same pair after an amend by the short (a new outpoint, the listing changed).
    let (at, amended) = amend_tx(&lp, S, &short_kp(), 1_150_000_000, 20);
    must("amend", &at);
    let lp2 = live(&at, 0, 400, &amended);
    let (mut t2, _) = propose_tx(&lp2, &c, &uw_kp(), None);
    t2.0.inputs[1].signature_script = sig.clone();
    let v = verdicts(&t2);
    assert!(!v[1], "the copied vouch fails on the changed pair: {v:?}");
    // Another pair of the same underwriter.
    let (lo, so, sl) = (order(L, &long_kp(), 0x73, 0), order(S, &short_kp(), 0x74, 0), slot(K));
    let (ot, p, _) = open_tx(&lo, &so, &sl, &open_spec(), &|_, _| {});
    must("second open", &ot);
    let other = live(&ot, 2, 300, &p);
    let (mut t3, _) = propose_tx(&other, &c, &uw_kp(), None);
    t3.0.inputs[1].signature_script = sig;
    assert!(!verdicts(&t3)[1], "the copied vouch fails on another pair");
    // The claim landed once: its UTXO set refuses a second spend of the pair.
    let mut u = bondkit::Utxos::default();
    u.add(lp.op, t.1[0].clone());
    u.add(outpoint(210), t.1[1].clone());
    u.add(outpoint(211), t.1[2].clone());
    u.apply(&t.0, 500).unwrap();
    assert!(u.apply(&at.0, 500).is_err(), "the pair is spent: the amend conflicts with the landed claim");
}

// ---------------------------------------------------------------- the cover's expiry and lateness

/// A claim whose window and one step do not fit inside the cover (P + W + step > X, review 10), or whose expiry lies
/// beyond P + term + step, is void: finalize restores the part and returns the bond to its proposer (review 11; lateness
/// voids nothing since review 11). The boundaries are exact.
#[test]
fn cover_expired_or_expiring_inside_the_window_voids_the_claim() {
    let (lp, _, _) = opened();
    // P = 900: P + W + step = X, valid. P = 901: void; P = 1,000 (P + W = X, valid before review 10): void.
    for (pp, valid) in [(900u64, true), (901, false), (1_000, false), (1_300, false)] {
        let lw = proposed(&lp, &liq_long(FALSE_P, pp as i64 - 50), pp);
        let (t, s) = finalize_tx(&lw, pp as i64, 200);
        must("finalize", &t);
        assert_eq!(matches!(s, Settlement::Forfeit { .. }), valid, "P = {pp}");
        if !valid {
            let Settlement::Restore { defender, disputer, .. } = s else { panic!() };
            let Settlement::Restore { proposer, .. } = lw.s.settlement(lw.value, pp as i64) else { panic!() };
            assert_eq!((defender, disputer, proposer), (0, 0, lw.s.bond), "a void claim's bond goes back to its proposer (review 11)");
        }
    }
    // Lateness voids nothing (review 11): P - t0 = 100 and 101 are both valid.
    for (t0, valid) in [(400i64, true), (399, true), (0, true)] {
        let lw = proposed(&lp, &liq_long(FALSE_P, t0), 500);
        let (t, s) = finalize_tx(&lw, 500, 200);
        must("finalize", &t);
        assert_eq!(matches!(s, Settlement::Forfeit { .. }), valid, "t0 = {t0}");
    }
    // A claim with t0 ahead of the lock time is refused at the proposal (CLTV).
    let mut c = liq_long(FALSE_P, 600);
    c.lock = 599;
    refused("t0 ahead of the lock time", &propose_tx(&lp, &c, &uw_kp(), None).0, 0);
    // A pair (written by a genesis) whose expiry lies beyond its UTXO's DAA + term + step: no claim on it can be valid,
    // so since review 11 the liquidation is refused at the proposal (before: proposed, and void at finalize).
    let mut far = lp.clone();
    far.s.b.expiry = 2_000;
    refused("X = 2,000 > 300 + 900: uncounted cover is never relied on", &propose_tx(&far, &liq_long(FALSE_P, 450), &uw_kp(), None).0, 0);
}

/// The engine refuses a finalize that settles a void claim (the model's restore is the only valid branch), and one
/// that restores a valid claim.
#[test]
fn finalize_takes_the_branch_validity_dictates() {
    let (lp, _, _) = opened();
    let valid = proposed(&lp, &liq_long(FALSE_P, 450), 500);
    let void = proposed(&lp, &liq_long(FALSE_P, 950), 1_001);
    // Swap the outputs: the valid part restored, the void part settled.
    let (tv, _) = finalize_tx(&valid, 500, 200);
    let (tx_void, _) = finalize_tx(&void, 1_001, 200);
    let mut a = finalize_tx(&valid, 500, 200).0;
    a.0.outputs = tx_void.0.outputs.clone();
    // Review 6: every claim has its own covenant id, so the swapped outputs are rebound to this part's id.
    rebind(&mut a, valid.id.unwrap());
    refused("a valid claim restored", &rebuild(&a), 0);
    let mut b = finalize_tx(&void, 1_001, 200).0;
    b.0.outputs = tv.0.outputs.clone();
    rebind(&mut b, void.id.unwrap());
    refused("a void claim settled", &rebuild(&b), 0);
}

// ---------------------------------------------------------------- the contest window

/// A contest accepted one DAA after the window (C = P + W) is ignored: the claim settles as if uncontested even though
/// the judge refutes it. At C = P + W - 1 the same verdict restores the part.
#[test]
fn contest_one_daa_after_the_window_does_not_stop_the_claim() {
    let (lp, _, _) = opened();
    for (c, status) in [(699u64, REFUTED), (700, UPHELD)] {
        let lw = proposed(&lp, &liq_long(FALSE_P, 450), 500);
        let (cw, _, dispute) = contest_at(&lw, 70, 10 * UNIT as u64, c);
        let (_, dw, _) = decide_at(&dispute, &cw, c + 100);
        assert_eq!(dw.s.status, status, "contest at {c}");
        assert_eq!(dw.s.contest_in_time(500, c as i64), status == REFUTED);
    }
}

/// The verdict reaches only the contested part: a dispute of another claim, a resolution naming another input, or a
/// decide without its dispute's resolution is refused; the second part of a split pair cannot take the first part's
/// verdict (the dispute recorded the contested part's script).
#[test]
fn a_verdict_delivered_to_the_wrong_part_is_refused() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq_long(FALSE_P, 450), 500);
    let (cw, _, dispute) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    // Another pair's contested claim.
    let (lo, so, sl) = (order(L, &long_kp(), 0x73, 0), order(S, &short_kp(), 0x74, 0), slot(K));
    let (ot, p, _) = open_tx(&lo, &so, &sl, &open_spec(), &|_, _| {});
    must("second open", &ot);
    let other = proposed(&live(&ot, 2, 300, &p), &liq_long(FALSE_P, 450), 500);
    let (cw2, _, dispute2) = contest_at(&other, 71, 10 * UNIT as u64, 520);
    not_accepted("dispute of claim 2 resolving onto claim 1", &decide_tx(&dispute2, &cw, 620).0);
    not_accepted("dispute of claim 1 resolving onto claim 2", &decide_tx(&dispute, &cw2, 620).0);
    // A look-alike contested part of the same id and template, other state (a genesis): the dispute's script hash.
    let mut fake = cw.clone();
    fake.s.quote = 1_000_000_000;
    fake.op = outpoint(0x5f);
    let (ft, _, _) = decide_tx(&dispute, &fake, 620);
    refused("resolve onto a look-alike part", &ft, 0);
    // The honest delivery.
    let (_, dw, _) = decide_at(&dispute, &cw, 620);
    assert_eq!(dw.s.status, REFUTED);
}

// ---------------------------------------------------------------- binding and the bounds

/// The underwriter can never be a party: an order refuses a slot whose owner is its owner or its counterparty, a
/// transfer refuses the underwriter as entrant.
#[test]
fn an_underwriter_that_is_also_a_party_is_refused() {
    // The short order's owner is the underwriter.
    let (lo, so, sl) = (order(L, &long_kp(), 0x71, 0), order(S, &uw_kp(), 0x72, 0), slot(K));
    let (t, _, _) = open_tx(&lo, &so, &sl, &open_spec(), &|_, _| {});
    let v = verdicts(&t);
    assert!(!v[0] && !v[1], "both orders refuse an underwriter-party: {v:?}");
    // A transfer to the underwriter.
    let (lp, _, _) = opened();
    let (at, amended) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("list the long", &at);
    let lp2 = live(&at, 0, 400, &amended);
    refused("transfer to the underwriter", &transfer_tx(&lp2, L, 1_000_000_000, 900_000_000, &uw_kp(), 20).0, 0);
    must("transfer to a stranger", &transfer_tx(&lp2, L, 1_000_000_000, 900_000_000, &keypair(45), 20).0);
}

/// An open needs the slot to count the cover: a slot too small (cover above rho x K), a bind for another output or a
/// smaller cover, a slot of another market, an expiry less than one term after the order, all refused; a twin output
/// of the new id is refused by the scan.
#[test]
fn a_bind_beyond_rho_times_the_bond_or_a_twin_is_refused() {
    let (lo, so) = (order(L, &long_kp(), 0x71, 0), order(S, &short_kp(), 0x72, 0));
    let sp = open_spec();
    let cover = q(sp.n, sp.cover_rate, 1_000_000) as u64;
    // rho = 1: a slot of exactly the cover binds, one sompi less does not.
    let (t, _, _) = open_tx(&lo, &so, &slot(cover), &sp, &|_, _| {});
    must("slot of exactly the cover", &t);
    let (t, _, _) = open_tx(&lo, &so, &slot(cover - 1), &sp, &|_, _| {});
    refused("slot one sompi short", &t, 2);
    // A twin of the pair in the genesis group.
    let (t, _, _) = open_tx(&lo, &so, &slot(K), &sp, &|o, p| o.push(TransactionOutput { value: 1_000_000, script_public_key: pair_spk(p), covenant: twin_binding() }));
    let v = verdicts(&t);
    assert!(!v[0] && !v[1], "a twin output of the new id: {v:?}");
    // Nine outputs: beyond the scan.
    let (t, _, _) = open_tx(&lo, &so, &slot(K), &sp, &|o, _| {
        for _ in 0..3 {
            o.push(change(1_000_000));
        }
    });
    assert!(!verdicts(&t)[0], "more than 8 outputs");
    // An expiry less than one term after the orders: refused by both.
    let (lo2, so2) = (order(L, &long_kp(), 0x71, 401), order(S, &short_kp(), 0x72, 401));
    let (t, _, _) = open_tx(&lo2, &so2, &slot(K), &sp, &|_, _| {});
    let v = verdicts(&t);
    assert!(!v[0] && !v[1], "X = 1,200 < 401 + 800: {v:?}");
    // A slot of another market (another rho): its bond block differs.
    let mut sl = slot(K);
    sl.s.m = omarket_with(kpf_protocol::bond::BondParams { rho_bps: 20_000, ..omarket().bond }).bond_market();
    let (t, _, _) = open_tx(&lo, &so, &sl, &sp, &|_, _| {});
    let v = verdicts(&t);
    assert!(!v[0] && !v[1], "slot of another market: {v:?}");
}

/// Parallel opens on one slot conflict (one UTXO); split bonds each count within their own value.
#[test]
fn parallel_transactions_and_split_bonds_stay_within_rho() {
    let sp = open_spec();
    let cover = q(sp.n, sp.cover_rate, 1_000_000) as u64;
    let sl = slot(cover);
    let (t1, _, _) = open_tx(&order(L, &long_kp(), 0x71, 0), &order(S, &short_kp(), 0x72, 0), &sl, &sp, &|_, _| {});
    let (t2, _, _) = open_tx(&order(L, &long_kp(), 0x73, 0), &order(S, &short_kp(), 0x74, 0), &sl, &sp, &|_, _| {});
    must("open 1", &t1);
    must("open 2 alone", &t2);
    let mut u = bondkit::Utxos::default();
    for t in [&t1, &t2] {
        for (i, inp) in t.0.inputs.iter().enumerate() {
            u.add(inp.previous_outpoint, t.1[i].clone());
        }
    }
    u.apply(&t1.0, 300).unwrap();
    assert!(u.apply(&t2.0, 300).is_err(), "the second open spends the same slot UTXO");
    // The continuation of the first counts the full capacity: a second bind on it is refused.
    let sl2 = live(&t1, 3, 300, &open_tx(&order(L, &long_kp(), 0x71, 0), &order(S, &short_kp(), 0x72, 0), &sl, &sp, &|_, _| {}).2);
    let mut sl2 = sl2;
    sl2.daa = 300;
    let (t3, _, _) = open_tx(&order(L, &long_kp(), 0x73, 0), &order(S, &short_kp(), 0x74, 0), &sl2, &sp, &|_, _| {});
    refused("a second bind beyond rho x K", &t3, 2);
}

/// A pair opened on a market whose block differs by one byte from the order's is refused: every order rebuilds the
/// pair from its own block, and a block whose id is not the hash of its body fails checkMarket.
#[test]
fn a_pair_on_a_market_block_one_byte_off_is_refused() {
    let (lo, mut so, sl) = (order(L, &long_kp(), 0x71, 0), order(S, &short_kp(), 0x72, 0), slot(K));
    // One byte of the feed hash: another market (another id). The long rebuilds the pair from its block, the short
    // from its own: no output satisfies both.
    so.s.m.feed_hash[0] ^= 1;
    let (t, _, _) = open_tx(&lo, &so, &sl, &open_spec(), &|_, _| {});
    let v = verdicts(&t);
    assert!(!(v[0] && v[1]), "one byte off: {v:?}");
    // The id field itself one byte off (the body unchanged): checkMarket refuses the order.
    let mut o = order(L, &long_kp(), 0x71, 0);
    o.s.m.feed_hash = omarket().feed_hash;
    let mut bytes = o.s.state_bytes();
    bytes[5] ^= 1;
    let art = kpf_protocol::artifacts::splice(&ok().open, &bytes);
    let k = kit();
    let ins = vec![
        In { outpoint: o.op, utxo: kaspa_consensus_core::tx::UtxoEntry::new(o.value, p2sh(&art), 0, false, o.id), spend: Spend::Signed { art, entry: "cancel", args: vec![Arg::Sig(long_kp())] } },
        exec_in(230),
    ];
    let _ = k;
    must("a malformed order can still be cancelled", &build_ext(&ins, vec![change(1_000_000)], 0, &[0, 0]));
}

// ---------------------------------------------------------------- renewal, lapse, the price-free exits

/// Renewal in the last step before X: the slot counts the cover at X + term, the premium leaves the margins (half each,
/// bounds inward), nobody else signs. Too early (t < X - step), another slot, or a continuation worth one sompi less
/// than the premium allows: refused.
#[test]
fn renewal_moves_the_expiry_and_pays_the_premium_from_the_margins() {
    let (lp, sl, _) = opened();
    let (t, r, b2) = renew_tx(&lp, &sl, 1_100, 8);
    must("renew at X - step", &t);
    assert_eq!(r.b.expiry, 2_000);
    assert_eq!((r.bl, r.bs), (lp.s.bl + 1_000_000, lp.s.bs - 1_000_000));
    assert_eq!(lp.value - t.0.outputs[0].value, 2_000_000_000, "the premium, 1,000 x 0.02 KAS");
    assert_eq!(b2.counted_at(2_000), lp.s.cover_of(lp.s.size));
    assert_eq!(b2.counted_at(1_200), lp.s.cover_of(lp.s.size), "the old bucket stays until its liability ends");
    refused("renew one DAA too early", &renew_tx(&lp, &sl, 1_099, 8).0, 1);
    let mut t2 = renew_tx(&lp, &sl, 1_100, 8).0;
    t2.0.outputs[0].value -= 1;
    refused("one sompi more than the premium", &rebuild(&t2), 0);
    let mut other = sl.clone();
    other.id = Some(Hash::from_bytes([0x52; 32]));
    refused("another slot", &renew_tx(&lp, &other, 1_100, 8).0, 0);
}

/// A lapsed pair: once tx.daa >= X + tail and the pair is idleDaa old, anyone settles it at its entry price. Not one
/// DAA earlier, not with a younger UTXO.
#[test]
fn a_lapsed_pair_settles_at_its_entry_price() {
    let (lp, _, _) = opened();
    // Review 6 (R6-F3): idleDaa is counted from the cover's end by the calendar (X + tail + idle = 2,400), not from the
    // UTXO's age, so no sequence is needed and none helps.
    must("lapse settle", &lapse_tx(&lp, 2_400, 0));
    refused("before X + tail + idleDaa", &lapse_tx(&lp, 2_399, 1_000_000), 0);
    let (l, s) = lp.s.lapse_out(lp.value);
    assert_eq!((l, s), (100_000_000_000, 200_020_000_000), "at E = 10 KAS: the long its margin, the short its margin and the carrier");
}

/// The price-free exits need no underwriter: amend, transfer to a signing taker, mutual close, cooperative close; every
/// party entry but the cooperative close waits quietDaa after the last change.
#[test]
fn the_price_free_exits_need_no_underwriter() {
    let (lp, _, _) = opened();
    must("coop close", &coop_tx(&lp, 100_000_000_000));
    refused("amend one DAA before quiet", &amend_tx(&lp, L, &long_kp(), 1_000_000_000, 19).0, 0);
    let (at, a1) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("amend the long", &at);
    let lp1 = live(&at, 0, 400, &a1);
    let (at2, a2) = amend_tx(&lp1, S, &short_kp(), 1_000_000_000, 20);
    must("amend the short", &at2);
    let lp2 = live(&at2, 0, 420, &a2);
    refused("mutual close before quiet", &mutual_tx(&lp2, 1_000_000_000, 19), 0);
    must("mutual close", &mutual_tx(&lp2, 1_000_000_000, 20));
    must("transfer to a signing taker", &transfer_tx(&lp1, L, 1_000_000_000, 900_000_000, &keypair(45), 20).0);
}

/// Close-versus-close between two pairs of one underwriter: the leavers are paid, the stayers form P3 under the host's
/// id with the host's binding. The donor refuses a P3 whose binding differs from its own in any field (review 8);
/// both refuse before quietDaa.
#[test]
fn close_versus_close_keeps_the_stayers_with_their_underwriter() {
    let list = |lp: &LPair, side: i64| {
        let (t, a) = amend_tx(lp, side, &if side == L { long_kp() } else { short_kp() }, 1_000_000_000, 20);
        must("list", &t);
        live(&t, 0, 400, &a)
    };
    let (h, _, _) = opened();
    let (lo, so) = (order(L, &keypair(43), 0x73, 0), order(S, &keypair(44), 0x74, 0));
    let (ot, p, _) = open_tx(&lo, &so, &slot(K), &open_spec(), &|_, _| {});
    must("second open", &ot);
    let host = list(&h, L);
    let d0 = live(&ot, 2, 300, &p);
    let (t, a) = amend_tx(&d0, S, &keypair(44), 1_000_000_000, 20);
    must("list the donor's short", &t);
    let donor = live(&t, 0, 400, &a);
    let (t, p3) = close_vs_close_tx(&host, &donor, 1_000_000_000, 1_000_000_000);
    must("close-versus-close", &t);
    assert_eq!((p3.long_key, p3.short_key, p3.b), (x(&keypair(43)), x(&short_kp()), host.s.b));
    // A donor of another underwriter's slot: its long would move to an underwriter it never accepted.
    let mut other = donor.clone();
    other.s.b.uw_id = [0x52; 32];
    refused("donor of another slot", &close_vs_close_tx(&host, &other, 1_000_000_000, 1_000_000_000).0, 1);
    // A donor whose cover expires before the host's: P3 would be covered longer than the donor's counted cover.
    let mut early = donor.clone();
    early.s.b.expiry = 1_100;
    refused("P3 expiry beyond the donor's", &close_vs_close_tx(&host, &early, 1_000_000_000, 1_000_000_000).0, 1);
    // A donor with a lower premium than the host's.
    let mut cheap = donor.clone();
    cheap.s.b.premium = 1_000_000;
    refused("P3 premium above the donor's", &close_vs_close_tx(&host, &cheap, 1_000_000_000, 1_000_000_000).0, 1);
    // Review 8 (R8-F1): P3's binding is the donor's own, byte for byte, so the directions accepted before are refused
    // too: a donor whose cover runs longer than the host's (P3 would shorten the long's cover), one whose premium is
    // higher, one whose cover rate is lower (P3 would carry the host's larger rate, review 8 note 2).
    let mut late = donor.clone();
    late.s.b.expiry = host.s.b.expiry + 100;
    refused("P3 expiry before the donor's", &close_vs_close_tx(&host, &late, 1_000_000_000, 1_000_000_000).0, 1);
    let mut dear = donor.clone();
    dear.s.b.premium = host.s.b.premium + 1;
    refused("P3 premium below the donor's", &close_vs_close_tx(&host, &dear, 1_000_000_000, 1_000_000_000).0, 1);
    let mut narrow = donor.clone();
    narrow.s.b.cover_rate = host.s.b.cover_rate - 1;
    refused("P3 cover rate above the donor's", &close_vs_close_tx(&host, &narrow, 1_000_000_000, 1_000_000_000).0, 1);
    // Quiet.
    let mut young = close_vs_close_tx(&host, &donor, 1_000_000_000, 1_000_000_000).0;
    young.0.inputs[0].sequence = 19;
    refused("host before quietDaa", &rebuild(&young), 0);
}

/// The slot cannot unbond while it counts the pair's cover; once X + tail has passed it can, and a claim still pending
/// then keeps its own bond: refuted, the underwriter pays it all the same.
#[test]
fn the_underwriter_unbonding_with_a_claim_pending() {
    let (lp, sl, _) = opened();
    let lw = proposed(&lp, &liq_long(FALSE_P, 950), 1_000);
    let (cw, _, dispute) = contest_at(&lw, 70, 10 * UNIT as u64, 1_010);
    // The slot counts the cover until X + tail = 1,400.
    refused("unbond with the cover counted", &unbond_tx(&sl, 12, 1_399), 0);
    must("unbond after the liability ended", &unbond_tx(&sl, 12, 1_400));
    // The dispute resolves later; the bond is in the part, not in the slot.
    let (_, dw, _) = decide_at(&dispute, &cw, 1_500);
    let (t, s) = finalize_tx(&dw, 1_000, 0);
    must("restore after the slot left", &t);
    let Settlement::Restore { defender, disputer, .. } = s else { panic!() };
    assert_eq!(defender + disputer, lw.s.bond);
}

/// A refuted claim's restored part is unlisted, tags 0, keeps the binding and the entry, and can be claimed again.
#[test]
fn a_restored_part_comes_back_unlisted_and_bound() {
    let (lp, _, _) = opened();
    let (at, listed) = amend_tx(&lp, L, &long_kp(), 1_100_000_000, 20);
    must("list", &at);
    let lp1 = live(&at, 0, 350, &listed);
    let lw = proposed(&lp1, &liq_long(FALSE_P, 450), 500);
    let (cw, _, dispute) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    let (_, dw, _) = decide_at(&dispute, &cw, 620);
    let (t, s) = finalize_tx(&dw, 500, 0);
    must("restore", &t);
    let Settlement::Restore { part, .. } = s else { panic!() };
    assert_eq!((part.long_ask, part.short_bid, part.mk), (0, 0, Makers::default()), "unlisted, every maker zero");
    assert_eq!((part.b, part.entry), (lp.s.b, lp.s.entry));
    let back = live(&t, 1, 640, &part);
    proposed(&back, &liq_long(FALSE_P, 600), 650);
}

/// A part whose two keys are equal cannot be liquidated; a liquidation that does not trigger with the tolerance g is
/// refused; a close needs its closer.
#[test]
fn propose_refuses_what_the_book_market_refuses() {
    let (lp, _, _) = opened();
    // 9.4: ceil(9.4 x 1.02) x 0.95 = 9.10 > 9: no trigger with the tolerance (the book market would liquidate at 9.47).
    refused("no trigger within g", &propose_tx(&lp, &liq_long(940_000_000, 450), &uw_kp(), None).0, 0);
    // The boundary: 9.288 triggers? ceil(p * 1.02) * 0.95 < 9  <=>  ceil(p * 1.02) <= 9.47368421
    assert!(lp.s.long_liquidatable(928_000_000) && !lp.s.long_liquidatable(929_000_000));
    proposed(&lp, &liq_long(928_000_000, 450), 500);
    refused("one tick above the trigger", &propose_tx(&lp, &liq_long(929_000_000, 450), &uw_kp(), None).0, 0);
    let mut one = lp.clone();
    one.s.short_key = one.s.long_key;
    refused("one owner", &propose_tx(&one, &liq_long(FALSE_P, 450), &uw_kp(), None).0, 0);
}

/// Rebuilds a transaction whose outputs were edited (the signatures commit to the outputs, so a test that edits an
/// output must sign again): the inputs are re-derived from the entries and signature plans of the same builder.
fn rebuild(t: &T) -> T {
    // The builders sign with SIGHASH_ALL; an edited output invalidates a signed input (a P2PK funder), which would hide
    // the contract's own verdict. Re-signing needs the plans, so the funders here are re-signed by replaying the P2PK
    // signature over the edited transaction.
    let mut tx = t.0.clone();
    for (i, e) in t.1.iter().enumerate() {
        if let Some(k) = kpf_protocol::tx::p2pk_key(&e.script_public_key) {
            let kp = [41u8, 42, 31, 60, 45, 77, 32].iter().map(|s| keypair(*s)).find(|kp| x(kp) == k);
            if let Some(kp) = kp {
                let sig = sign_type(&tx, &t.1, i, &kp, 0x01);
                tx.inputs[i].signature_script = push(&sig);
            }
        }
    }
    tx.finalize();
    (tx, t.1.clone())
}

/// Every output that carries a covenant id is rebound to `id` (authorizing input 0).
fn rebind(t: &mut T, id: kaspa_consensus_core::Hash) {
    for o in t.0.outputs.iter_mut() {
        if let Some(c) = o.covenant.as_mut() {
            c.covenant_id = id;
            c.authorizing_input = 0;
        }
    }
}
