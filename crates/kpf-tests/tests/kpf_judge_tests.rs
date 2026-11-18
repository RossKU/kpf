//! The stake judge in the script engine (`contracts/KpfStake.sil`, `contracts/stubs/KpfClaimStub.sil`;
//! docs/DESIGN-ORACLE-IMPL.md, second slice): every path positive and negative, the boundary of every window, age and
//! delay at one DAA either side, payouts to the sompi. Every transaction is built from the model's `JTx` and run in the
//! engine; the model's verdict and the engine's must agree input by input (`judgekit::agree`), so every test here is
//! also a step-by-step replay of the model.

mod common;
mod judgekit;

use judgekit::*;
use kpf_protocol::judge::*;

/// A table with a dispute opened at the anchor by key 11 with `v` on NO.
fn opened(v: i64) -> JTable {
    let mut t = JTable::new();
    let o = t.stake(11, v, 0);
    assert!(t.open(o, 11, ANCHOR));
    t
}

fn ok(t: &JTable, what: &str, tx: &JTx) -> bool {
    agree(what, &t.eng, tx)
}

fn set_stake(tx: &mut JTx, j: usize, f: impl Fn(&mut Js)) {
    if let Script::Stake(s) = &mut tx.outs[j].script {
        f(s);
    } else {
        panic!("output {j} is not a stake");
    }
}

#[test]
fn a_dispute_opened_answered_resolved_settled_and_closed() {
    let k = jkit();
    println!(
        "KpfStake {} bytes (1 + {} + {}), KpfClaimStub {} bytes (1 + {} + {})",
        k.stake_t.prefix.len() + k.stake_t.state_len + k.stake_t.suffix.len(),
        k.stake_t.state_len,
        k.stake_t.suffix.len(),
        k.claim_t.prefix.len() + k.claim_t.state_len + k.claim_t.suffix.len(),
        k.claim_t.state_len,
        k.claim_t.suffix.len()
    );
    assert_eq!(k.stake_t.prefix.len(), 1);
    assert_eq!(k.stake_t.state_len, STATE_LEN);
    let mut t = opened(10 * UNIT);
    assert_eq!(t.claim_status(), CLAIM_CONTESTED);
    let y = t.stake(12, 13 * UNIT, 0);
    assert!(t.deposit(YES, &[(y, 12)], ANCHOR + 5));
    assert_eq!(t.d().side, YES);
    let end = t.window_end();
    assert!(!t.resolve(end - 1));
    assert!(t.resolve(end));
    assert_eq!(t.claim_status(), CLAIM_UPHELD);
    let rs = t.receipts.clone();
    let paid = t.settle(&rs).expect("settle");
    // YES (13 units) takes NO's 10 less 10 %.
    assert!(paid.contains(&(rs[1], 13 * UNIT + 9 * UNIT)));
    assert!(t.close().is_some());
}

#[test]
fn open_positive_and_negative() {
    // Accepted at the age boundary; refused one DAA late, unaccepted, below the minimum, by another signer, with a
    // SINGLE|ANYONECANPAY signature, from an unbonding stake, of another judge, onto a claim that is not pending.
    let mut t = JTable::new();
    let late = t.stake(11, 10 * UNIT, 1);
    let tx = t.a.open_tx(t.claim, late, 11, 50_000_000, &Real);
    assert!(!ok(&t, "unaged by one DAA", &tx));
    let fresh = t.a.stake(11, 10 * UNIT, -1);
    let tx = t.a.open_tx(t.claim, fresh, 11, 50_000_000, &Real);
    assert!(!ok(&t, "unaccepted stake", &tx));
    let small = t.stake(11, UNIT - 1, 0);
    let tx = t.a.open_tx(t.claim, small, 11, 50_000_000, &Real);
    assert!(!ok(&t, "one sompi under the minimum", &tx));
    let s = t.stake(11, UNIT, 0);
    let tx = t.a.open_tx(t.claim, s, 12, 50_000_000, &Real);
    assert!(!ok(&t, "another signer", &tx));
    let mut tx = t.a.open_tx(t.claim, s, 11, 50_000_000, &Real);
    tx.ins[1].sighash = SIGHASH_SINGLE_ACP;
    assert!(!ok(&t, "open needs SIGHASH_ALL", &tx));
    let mut tx = t.a.open_tx(t.claim, s, 11, 50_000_000, &Real);
    if let Script::Stake(x) = &mut tx.ins[1].utxo.script {
        x.status = STATUS_UNBONDING;
    }
    assert!(!ok(&t, "an unbonding stake", &tx));
    let mut tx = t.a.open_tx(t.claim, s, 11, 50_000_000, &Real);
    for j in [1, 2] {
        set_stake(&mut tx, j, |x| x.m.id = [0x4b; 32]);
    }
    if let Script::Stake(x) = &mut tx.ins[1].utxo.script {
        x.m.id = [0x4b; 32];
    }
    assert!(!ok(&t, "a stake of another judge", &tx));
    // A forged tally: the dispute claims one sompi more on NO than the stake.
    let mut tx = t.a.open_tx(t.claim, s, 11, 50_000_000, &Real);
    set_stake(&mut tx, 2, |x| x.no += 1);
    assert!(!ok(&t, "a forged tally", &tx));
    // A twin: a fourth output of the claim's id.
    let mut tx = t.a.open_tx(t.claim, s, 11, 50_000_000, &Real);
    let twin = tx.outs[1].clone();
    tx.outs.push(twin);
    assert!(!ok(&t, "a twin receipt", &tx));
    // An unaccepted claim (no anchor).
    let mut tx = t.a.open_tx(t.claim, s, 11, 50_000_000, &Real);
    tx.ins[0].daa = -1;
    assert!(!ok(&t, "an unaccepted claim", &tx));
    // At the boundary it opens, and the claim cannot be contested again.
    assert!(t.open(s, 11, ANCHOR));
    let s2 = t.stake(12, UNIT, 0);
    let tx = t.a.open_tx(t.claim, s2, 12, 50_000_000, &Real);
    assert!(!ok(&t, "a contested claim is not pending", &tx));
    let d = t.d();
    assert_eq!((d.side, d.no, d.no_u, d.yes, d.anchor, d.start, d.round), (NO, UNIT, 1, 0, ANCHOR, -1, 0));
}

#[test]
fn deposit_positive_and_negative() {
    let mut t = opened(10 * UNIT);
    let id = t.dispute.unwrap();
    // The leader (NO) cannot add stake.
    let n = t.stake(13, 5 * UNIT, 0);
    let tx = t.a.deposit_tx(id, NO, &[(n, 13)], SIGHASH_SINGLE_ACP);
    assert!(!ok(&t, "the leader adds stake", &tx));
    // Age boundary, unaccepted, minimum.
    let late = t.stake(12, 13 * UNIT, 1);
    { let tx = t.a.deposit_tx(id, YES, &[(late, 12)], SIGHASH_SINGLE_ACP); assert!(!ok(&t, "unaged by one DAA", &tx)); }
    let fresh = t.a.stake(12, 13 * UNIT, -1);
    { let tx = t.a.deposit_tx(id, YES, &[(fresh, 12)], SIGHASH_SINGLE_ACP); assert!(!ok(&t, "unaccepted", &tx)); }
    let small = t.stake(12, UNIT - 1, 0);
    { let tx = t.a.deposit_tx(id, YES, &[(small, 12)], SIGHASH_SINGLE_ACP); assert!(!ok(&t, "below the minimum", &tx)); }
    // Signatures: another key, SIGHASH_NONE refused; SIGHASH_ALL and SINGLE|ANYONECANPAY accepted.
    let y = t.stake(12, 13 * UNIT, 0);
    let tx = t.a.deposit_tx(id, YES, &[(y, 14)], SIGHASH_SINGLE_ACP);
    assert!(!ok(&t, "another key's signature", &tx));
    let tx = t.a.deposit_tx(id, YES, &[(y, 12)], 0x02);
    assert!(!ok(&t, "SIGHASH_NONE", &tx));
    { let tx = t.a.deposit_tx(id, YES, &[(y, 12)], SIGHASH_ALL); assert!(ok(&t, "SIGHASH_ALL", &tx)); }
    // A commitment for another round or another anchor, a receipt worth less, a forged continuation.
    let mut tx = t.a.deposit_tx(id, YES, &[(y, 12)], SIGHASH_SINGLE_ACP);
    tx.ins[1].spend = Spend::Commit { side: YES, round: 1, anchor: ANCHOR };
    set_stake(&mut tx, 1, |x| x.round = 1);
    assert!(!ok(&t, "a commitment for another round", &tx));
    let mut tx = t.a.deposit_tx(id, YES, &[(y, 12)], SIGHASH_SINGLE_ACP);
    tx.ins[1].spend = Spend::Commit { side: YES, round: 0, anchor: ANCHOR - 1 };
    set_stake(&mut tx, 1, |x| x.anchor = ANCHOR - 1);
    assert!(!ok(&t, "a commitment for another dispute of the claim", &tx));
    let mut tx = t.a.deposit_tx(id, YES, &[(y, 12)], SIGHASH_SINGLE_ACP);
    tx.outs[1].value -= 1;
    assert!(!ok(&t, "a receipt one sompi short", &tx));
    let mut tx = t.a.deposit_tx(id, YES, &[(y, 12)], SIGHASH_SINGLE_ACP);
    set_stake(&mut tx, 0, |x| x.yes += 1);
    assert!(!ok(&t, "a forged tally", &tx));
    let mut tx = t.a.deposit_tx(id, YES, &[(y, 12)], SIGHASH_SINGLE_ACP);
    let twin = tx.outs[1].clone();
    tx.outs.insert(2, twin);
    assert!(!ok(&t, "a twin receipt", &tx));
    // An unbonding stake.
    let mut tx = t.a.deposit_tx(id, YES, &[(y, 12)], SIGHASH_SINGLE_ACP);
    if let Script::Stake(x) = &mut tx.ins[1].utxo.script {
        x.status = STATUS_UNBONDING;
    }
    assert!(!ok(&t, "an unbonding stake", &tx));
    // At the boundary of age it is accepted.
    assert!(t.deposit(YES, &[(y, 12)], ANCHOR + 1));
    assert_eq!(t.d().side, YES);
}

#[test]
fn the_lead_requirement_to_the_sompi_and_seven_stakes_per_deposit() {
    let mut t = opened(10 * UNIT + 3);
    let need = jparams().need(10 * UNIT + 3).unwrap();
    // Seven stakes one sompi short in all: the lead stays; the window keeps the opening's start.
    let per = (need - 1) / 7;
    let mut ps: Vec<(u64, u8)> = (0..6).map(|i| (t.stake(20 + i, per, 0), 20 + i)).collect();
    ps.push((t.stake(26, need - 1 - 6 * per, 0), 26));
    let end0 = t.window_end();
    assert!(t.deposit(YES, &ps, ANCHOR + 7));
    assert_eq!(t.d().side, NO, "one sompi under keeps the lead");
    assert_eq!(t.d().start, ANCHOR, "the round's start, read exactly from the opening's UTXO");
    assert_eq!(t.window_end(), end0);
    // Eight stakes in one deposit: refused (seven at most).
    let id = t.dispute.unwrap();
    let ps8: Vec<(u64, u8)> = (0..8).map(|i| (t.stake(30 + i, UNIT, 0), 30 + i)).collect();
    { let tx = t.a.deposit_tx(id, YES, &ps8, SIGHASH_SINGLE_ACP); assert!(!ok(&t, "eight stakes", &tx)); }
    // One sompi more takes the lead and starts a round at the deposit's own DAA.
    let one = t.stake(40, UNIT, 0);
    assert!(t.deposit(YES, &[(one, 40)], ANCHOR + 9));
    assert_eq!((t.d().side, t.d().start, t.d().round), (YES, -1, 1));
    assert_eq!(t.window_end(), ANCHOR + 9 + jparams().round);
}

#[test]
fn resolve_boundaries_twice_by_anyone_and_onto_the_right_claim() {
    let mut t = opened(10 * UNIT);
    // A partial answer at end - 1 does not move the window.
    let end = t.window_end();
    let p = t.stake(12, 5 * UNIT, 0);
    assert!(t.deposit(YES, &[(p, 12)], end - 1));
    assert_eq!(t.window_end(), end);
    let id = t.dispute.unwrap();
    { let tx = t.a.resolve_tx(id, t.claim, end - 1); assert!(!ok(&t, "one DAA early", &tx)); }
    // Not onto another UTXO of the claim's id, nor onto a claim of another id.
    let look = t.a.claim(CLAIM_ID, 9, ANCHOR);
    let mut tx = t.a.resolve_tx(id, look, end);
    tx.ins[1].utxo.script = Script::Claim(ClaimStub { m: jmarket(), owner: key_of(9), status: CLAIM_CONTESTED });
    if let Script::Claim(c) = &mut tx.ins[1].utxo.script {
        c.owner = key_of(8);
    }
    if let Script::Claim(c) = &mut tx.outs[1].script {
        c.owner = key_of(8);
    }
    assert!(!ok(&t, "another UTXO of the claim's id", &tx));
    let other = t.a.claim([0xc2; 32], 9, ANCHOR);
    let mut tx = t.a.resolve_tx(id, other, end);
    tx.ins[1].utxo.script = Script::Claim(ClaimStub { m: jmarket(), owner: key_of(9), status: CLAIM_CONTESTED });
    assert!(!ok(&t, "a claim of another id", &tx));
    // Without the claim.
    let mut tx = t.a.resolve_tx(id, t.claim, end);
    tx.ins.remove(1);
    tx.outs.remove(1);
    tx.ins[0].spend = Spend::Resolve { claim: 0 };
    assert!(!ok(&t, "no claim", &tx));
    // A verdict that is not the leader's.
    let mut tx = t.a.resolve_tx(id, t.claim, end);
    set_stake(&mut tx, 0, |x| x.side = YES);
    if let Script::Claim(c) = &mut tx.outs[1].script {
        c.status = CLAIM_UPHELD;
    }
    assert!(!ok(&t, "the trailing side's verdict", &tx));
    // At the window's end, by anyone (no signature): the leader wins; then never again.
    assert!(t.resolve(end));
    assert_eq!(t.claim_status(), CLAIM_REFUTED);
    let r = t.d();
    assert_eq!((r.status, r.side, r.w_left, r.l_left, r.base, r.start), (STATUS_RESOLVED, NO, 10, 5 * UNIT, 50_000_000, ANCHOR));
    let mut tx = t.a.resolve_tx(t.dispute.unwrap(), t.claim, end + 1);
    tx.ins[1].utxo.script = Script::Claim(ClaimStub { m: jmarket(), owner: key_of(9), status: CLAIM_CONTESTED });
    assert!(!ok(&t, "resolved twice", &tx));
}

#[test]
fn a_claim_decides_only_from_its_disputes_resolution() {
    let mut t = opened(10 * UNIT);
    let id = t.dispute.unwrap();
    // The claim's decide beside a deposit (not a resolve) of its dispute.
    let y = t.stake(12, 13 * UNIT, 0);
    let mut tx = t.a.deposit_tx(id, YES, &[(y, 12)], SIGHASH_SINGLE_ACP);
    let cu = t.a.w.utxos[&t.claim].clone();
    let decided = match &cu.out.script {
        Script::Claim(c) => ClaimStub { status: CLAIM_REFUTED, ..c.clone() },
        _ => unreachable!(),
    };
    tx.ins.insert(3, t.a.w.input(t.claim, Spend::Decide { d: 0 }, None, SIGHASH_ALL, 0));
    tx.outs.insert(3, JOut { script: Script::Claim(decided), value: cu.out.value, cov: cu.out.cov });
    assert!(!ok(&t, "decided from a deposit", &tx));
}

#[test]
fn settlement_pays_uneven_stakes_to_the_sompi() {
    // NO 10.123456789 units; YES answers with 4.700000001, 5.3 and 3 units (13.000000001 >= ceil(1.25 x NO)); nobody
    // answers back; YES wins. Burn 10 %; weights 4, 5, 3 units.
    let mut t = opened(10_123_456_789);
    let ys = [(t.stake(21, 4_700_000_001, 0), 21), (t.stake(22, 5_300_000_000, 0), 22), (t.stake(23, 3_000_000_000, 0), 23)];
    assert!(t.deposit(YES, &ys, ANCHOR + 1));
    let end = t.window_end();
    assert!(t.resolve(end));
    let l: u128 = 10_123_456_789;
    let lnet = l - l * 1_000 / 10_000;
    let want: Vec<i64> = [(4u128, 4_700_000_001u128), (5, 5_300_000_000), (3, 3_000_000_000)].iter().map(|(u, v)| (v + u * lnet / 12) as i64).collect();
    assert_eq!(want, vec![7_737_037_038, 9_096_296_296, 5_277_777_777]);
    // A winner first, before any loser is forfeited: the pool (0.5 KAS) cannot pay its share.
    let rs = t.receipts.clone();
    let pool = t.dispute.unwrap();
    { let tx = t.a.settle_tx(pool, &rs[1..2]); assert!(!ok(&t, "a share before the forfeit", &tx)); }
    // A winner paid one sompi short, a winner paid to another key.
    let mut tx = t.a.settle_tx(pool, &rs);
    tx.outs[1].value -= 1;
    assert!(!ok(&t, "a share one sompi short", &tx));
    let mut tx = t.a.settle_tx(pool, &rs);
    set_stake(&mut tx, 1, |x| x.owner = key_of(99));
    assert!(!ok(&t, "paid to another key", &tx));
    // All four in one transaction (winners first, the loser forfeited).
    let paid = t.settle(&rs).expect("settle");
    let got: Vec<i64> = paid.iter().filter(|(_, v)| *v > 0).map(|(_, v)| *v).collect();
    assert_eq!(got, want);
    // The burn: 10 % of NO plus one sompi of rounding; one sompi short refused; then closed.
    let mut tx = t.a.close_tx(t.dispute.unwrap());
    assert_eq!(tx.outs[1].value, 1_012_345_678 + 1);
    tx.outs[1].value -= 1;
    assert!(!ok(&t, "the burn one sompi short", &tx));
    assert!(t.close().is_some());
}

#[test]
fn receipts_cannot_leave_before_the_verdict_and_settle_only_with_their_pool() {
    let mut t = opened(10 * UNIT);
    let y = t.stake(12, 13 * UNIT, 0);
    assert!(t.deposit(YES, &[(y, 12)], ANCHOR + 1));
    let rs = t.receipts.clone();
    // A receipt beside its open dispute (a deposit's or a resolve's input): refused by the count of the id's inputs.
    let id = t.dispute.unwrap();
    let mut tx = t.a.resolve_tx(id, t.claim, t.window_end());
    tx.ins.insert(2, t.a.w.input(rs[0], Spend::Redeem { d: 0 }, None, SIGHASH_ALL, 0));
    assert!(!ok(&t, "a receipt redeemed while the dispute is open", &tx));
    // Nor withdrawn by its owner (a receipt is not unbonding), nor unbonded, however old.
    for sp in [Spend::Withdraw, Spend::Unbond] {
        let f = t.a.fund();
        let v = t.a.w.utxos[&rs[1]].out.value;
        let tx = JTx {
            ins: vec![t.a.w.input(rs[1], sp, Some(12), SIGHASH_ALL, jparams().unbond), t.a.w.input(f, Spend::Key, Some(FUNDER), SIGHASH_ALL, 0)],
            outs: vec![JOut { script: Script::Key(12), value: v, cov: None }],
            lock: 0,
        };
        assert!(!ok(&t, "a committed stake leaves", &tx));
    }
    let end = t.window_end();
    assert!(t.resolve(end));
    // A receipt beside another receipt (no pool), and settle of a receipt of another dispute of this claim.
    let pool = t.dispute.unwrap();
    let mut tx = t.a.settle_tx(pool, &rs);
    tx.ins[0] = t.a.w.input(rs[1], Spend::Redeem { d: 1 }, None, SIGHASH_ALL, 0);
    tx.ins[1].spend = Spend::Redeem { d: 0 };
    tx.ins.remove(2);
    tx.outs[0] = JOut { script: Script::Key(60), value: 1, cov: None };
    assert!(!ok(&t, "two receipts and no pool", &tx));
    let alien = t.a.w.add(
        JOut { script: Script::Stake(Js::receipt(key_of(30), jmarket(), YES, 50 * UNIT, 0, ANCHOR - 1)), value: 50 * UNIT, cov: Some(CLAIM_ID) },
        ANCHOR,
    );
    let tx = t.a.settle_tx(pool, &[alien]);
    assert!(!ok(&t, "a receipt of another dispute of the claim", &tx));
    // Close before every receipt is settled.
    { let tx = t.a.close_tx(pool); assert!(!ok(&t, "close early", &tx)); }
    // One at a time, loser first.
    assert!(t.settle(&rs[..1]).is_some());
    assert!(t.settle(&rs[1..]).is_some());
    assert!(t.close().is_some());
}

#[test]
fn unbond_and_withdraw_after_the_delay() {
    let mut t = JTable::new();
    let s = t.stake(11, 10 * UNIT, 0);
    { let tx = t.a.unbond_tx(s, 12); assert!(!ok(&t, "unbond by another key", &tx)); }
    let tx = t.a.unbond_tx(s, 11);
    let hs = t.run("unbond", &tx, ANCHOR).expect("unbond");
    let u = hs[0];
    let p = jparams();
    { let tx = t.a.withdraw_tx(u, 11, p.unbond - 1); assert!(!ok(&t, "withdraw one DAA early", &tx)); }
    { let tx = t.a.withdraw_tx(u, 12, p.unbond); assert!(!ok(&t, "withdraw by another key", &tx)); }
    // An unbonding stake cannot join a dispute.
    let o = t.stake(13, 10 * UNIT, 0);
    assert!(t.open(o, 13, ANCHOR));
    let mut tx = t.a.deposit_tx(t.dispute.unwrap(), YES, &[(u, 11)], SIGHASH_SINGLE_ACP);
    tx.ins[1].daa = ANCHOR - p.age;
    assert!(!ok(&t, "an unbonding stake commits", &tx));
    let tx = t.a.withdraw_tx(u, 11, p.unbond);
    assert!(t.run("withdraw", &tx, ANCHOR + p.unbond).is_some());
    // A free stake cannot withdraw without unbonding.
    let f = t.stake(14, 10 * UNIT, 0);
    { let tx = t.a.withdraw_tx(f, 14, p.unbond); assert!(!ok(&t, "withdraw a free stake", &tx)); }
}

#[test]
fn many_stakers_on_both_sides_one_staker_and_one_key_on_both_sides() {
    // Many: 49 stakers in seven deposits over three lead changes, then nobody: YES wins; 50 receipts settle in batches
    // of seven.
    let mut t = opened(10 * UNIT);
    let mut seed = 20u8;
    let mut batch = |t: &mut JTable, n: usize, v: i64| -> Vec<(u64, u8)> {
        (0..n)
            .map(|_| {
                seed += 1;
                (t.stake(seed, v, 0), seed)
            })
            .collect()
    };
    // YES 7 + 7 units (the second batch takes the lead: 14 >= 12.5); NO 7 + 7 (24 >= 17.5); YES 7 + 7 + 7 (35 >= 30).
    for (side, n) in [(YES, 2), (NO, 2), (YES, 3)] {
        for j in 0..n {
            let b = batch(&mut t, 7, UNIT + 7);
            assert!(t.deposit(side, &b, ANCHOR + 1));
            assert_eq!(t.d().side == side, j == n - 1, "the lead changes with the last batch only");
        }
    }
    assert_eq!(t.d().round, 3);
    let end = t.window_end();
    assert!(t.resolve(end));
    let rs = t.receipts.clone();
    assert_eq!(rs.len(), 50);
    // Losers first so that every winner can be paid.
    let (mut l, mut w): (Vec<u64>, Vec<u64>) = rs.iter().partition(|h| t.a.js(**h).side == NO);
    l.append(&mut w);
    for c in l.chunks(7) {
        assert!(t.settle(c).is_some());
    }
    assert!(t.close().is_some());

    // One key on both sides: key 11 opens NO, answers itself on YES; YES wins; it gets its stake back less the burn.
    let mut t = opened(10 * UNIT);
    let y = t.stake(11, 13 * UNIT, 0);
    assert!(t.deposit(YES, &[(y, 11)], ANCHOR + 1));
    let end = t.window_end();
    assert!(t.resolve(end));
    let rs = t.receipts.clone();
    let paid = t.settle(&rs).expect("settle");
    assert_eq!(paid.iter().map(|(_, v)| *v).sum::<i64>(), 23 * UNIT - UNIT);
    assert!(t.close().is_some());
}
