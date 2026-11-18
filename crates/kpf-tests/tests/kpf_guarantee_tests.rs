//! Guaranteed fast settlement (DESIGN 4.5f): `KpfPending.guarantee`, `KpfLock.dispute` and `KpfLock.release`, positive
//! and negative, with the real KOB order and token contracts executing in the same transaction as the evidence tests do.
//!
//! The base pair is the worked example (1,000 tokens, bl 9, bs 12 KAS, mmr 5 %, g 2 %, W 6,000, R 600, L 864,000, bond 90
//! KAS) in the test market's fast-settlement parameters: lambda 1.1 (11,000 bps), dispute window D 6,600 (W + R), cure
//! grace G 1,200 (2R). A guarantee of the long's liquidation covers the part's range, 3,000 KAS, with a lock of 3,300 KAS;
//! of the long's close at 10.5, the closer's payoff 1,500 KAS, with a lock of 1,650 KAS. Counter quotes as in
//! `kpf_two_step_tests`: the long's liquidation is refuted by a bid >= 9.66315790, the short's by an ask <= 11.20, the
//! long's close at 10.5 by an ask <= 10.29, the short's close at 11 by a bid >= 11.22.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::Transaction;
use kaspa_consensus_core::Hash;
use kpf::evidence::*;
use kpf::paths::*;
use kpf::*;
use secp256k1::Keypair;

fn accept(d: &Draft) -> Transaction {
    let (tx, e) = d.build();
    assert_accepted(&tx, &e);
    let ins: u128 = e.iter().map(|u| u.amount as u128).sum();
    let outs: u128 = tx.outputs.iter().map(|o| o.value as u128).sum();
    assert!(outs <= ins, "no sompi created");
    tx
}

fn refused(d: &Draft, i: usize) {
    let (tx, e) = d.build();
    assert_input_fails(&tx, &e, i);
}

fn fk() -> Keypair {
    keypair(14)
}
fn lk() -> Keypair {
    keypair(11)
}
fn sk() -> Keypair {
    keypair(12)
}
fn gk() -> [u8; 32] {
    xonly(&guarantor_kp())
}

fn base() -> Pair {
    pair(&lk(), &sk())
}

/// The base pair in a UTXO of DAA 1,000 (its clock start S = 1,000).
fn lv(tag: u8) -> Live {
    live(&base(), tag).at(1_000)
}

const FULL: i64 = 1_000_000_000;
const HALF: i64 = 500_000_000;
const LIQ_ASK: i64 = 940_000_000;
const LIQ_BID: i64 = 1_150_000_000;
const CL_BID: i64 = 1_050_000_000;
const CL_ASK: i64 = 1_100_000_000;
const REF_LIQ_LONG: i64 = 966_315_790;
const REF_LIQ_SHORT: i64 = 1_120_000_000;
const REF_CL_LONG: i64 = 1_029_000_000;
const REF_CL_SHORT: i64 = 1_122_000_000;

fn close(l: &Live, n: i64, side: i64) -> PairLeg {
    let signer = if side == LONG { lk() } else { sk() };
    PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side, signer } }
}

fn liq(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side } }
}

fn ask(p: i64, fill: i64) -> Evidence {
    Evidence::ask(p, fill)
}
fn bid(p: i64, fill: i64) -> Evidence {
    Evidence::bid(p, fill)
}

/// The four claims on the base pair, each with its proposal evidence and the counter quote that just refutes it.
fn claims(l: &Live, n: i64) -> Vec<(PairLeg, Evidence, Evidence)> {
    vec![
        (close(l, n, LONG), bid(CL_BID, n), ask(REF_CL_LONG, n)),
        (close(l, n, SHORT), ask(CL_ASK, n), bid(REF_CL_SHORT, n)),
        (liq(l, n, LONG), ask(LIQ_ASK, n), bid(REF_LIQ_LONG, n)),
        (liq(l, n, SHORT), bid(LIQ_BID, n), ask(REF_LIQ_SHORT, n)),
    ]
}

fn one_tick_short(e: &Evidence) -> Evidence {
    let mut x = e.clone();
    match x.kind {
        Kind::Ask => x.ask.price += 1,
        Kind::Bid => {
            x.bid.price -= 1;
            x = Evidence::bid(x.bid.price, x.fill);
        }
    }
    x
}

/// The guarantee of a pending part accepted, and its lock as a live UTXO of DAA score F = P + G (a liquidation can be
/// guaranteed from then; a close from P).
fn guaranteed(w: &LivePend) -> (Draft, LiveLk) {
    let d = guarantee_draft(w, &gk(), &fk(), 5);
    let tx = accept(&d);
    let f = w.daa + if w.w.kind == KIND_LIQ { grace() } else { 1 };
    (d.clone(), live_locks(&tx, &d).remove(0).at(f))
}

/// A dispute of the locks on counter-evidence `e`, at a lock time no earlier than the newest lock.
fn dispute(ls: &[LiveLk], e: &Evidence) -> Draft {
    let mut d = dp_draft(ls, e, &fk(), 6);
    d.lock = d.lock.max(ls.iter().map(|l| l.daa).max().unwrap_or(0));
    d
}

/// What the victim of a claim held, valued at the counter quote `c`, had the claim not settled: the loser's side of a
/// liquidation, or what the other party of a close is owed at `c` (the pending part's value less the closer's refund
/// and the closer's payoff at `c`).
fn victim_fair(w: &LivePend, c: i64) -> i64 {
    let p = &w.w;
    let cc = c.clamp(p.bl, p.bs);
    let sc = p.m.scale;
    if p.kind == KIND_LIQ {
        if p.side == LONG {
            q(p.n, cc - p.bl, sc, 0)
        } else {
            q(p.n, p.bs - cc, sc, 0)
        }
    } else {
        let owe = if p.side == LONG { q(p.n, cc - p.bl, sc, 0) } else { q(p.n, p.bs - cc, sc, 0) };
        w.value as i64 - p.refund - owe
    }
}

/// What the victim was paid by the fast settlement itself.
fn victim_settled(w: &LivePend) -> i64 {
    let p = &w.w;
    if p.kind == KIND_LIQ {
        0
    } else {
        let (lp, sp) = p.guarantee_close(w.value);
        if p.side == LONG {
            sp
        } else {
            lp
        }
    }
}

// ================================================================ truthful claims

/// A truthful claim (the market really is where the claim says), for every kind and side, whole and partial: the
/// guarantee settles it exactly as finalize would, without the window, and after D the lock goes back to the guarantor.
#[test]
fn a_truthful_claim_settles_at_once_and_the_lock_is_released_after_the_window() {
    let l = lv(0x31);
    for n in [FULL, HALF] {
        for (g, e, _) in claims(&l, n) {
            accept(&ev_draft(&[g.clone()], &e, &fk(), 1));
            let w = pend(&g, e.price());
            let (d, lock) = guaranteed(&w);
            // The settlement equals finalize's, the proposer paid in full (no offer).
            let f = finalize_draft(&w, &fk(), 3);
            if w.w.kind == KIND_LIQ {
                assert_eq!((d.outs[0].value, d.outs[1].value), (f.outs[0].value, f.outs[1].value), "proposer and forfeited part as at finalize");
                assert_eq!(lock.l.cover, w.w.n * 300, "a liquidation covers the part's range q(n, bs - bl)");
            } else {
                assert_eq!(d.outs[0].value, f.outs[0].value, "the long as at finalize");
                assert_eq!(d.outs.last().unwrap().value, f.outs.last().unwrap().value, "the short as at finalize");
                let owe = if w.w.side == LONG { q(n, 150_000_000, SCALE, 0) } else { q(n, 100_000_000, SCALE, 0) };
                assert_eq!(lock.l.cover, owe, "a close covers the closer's payoff");
            }
            assert_eq!(lock.value as i64, (lock.l.cover * 11 + 9) / 10, "lambda 1.1 of the cover, rounded up");
            assert_eq!(lock.l.victim, w.w.defender_key());
            // The release: at age D, the whole lock to the guarantor; one DAA early it is refused.
            let r = release_draft(&lock, &fk(), 7);
            accept(&r);
            assert_eq!(r.outs[0].value, lock.value);
            let mut early = r.clone();
            early.seqs[0] -= 1;
            refused(&early, 0);
        }
    }
}

/// Worked example (DESIGN 4.5f): the long's whole liquidation at 9.40, guaranteed: the proposer gets 137 KAS at once,
/// the short holds the forfeited part (2,953.2 KAS), the lock holds 3,300 KAS for D; a close of the long at 10.5: the
/// long gets 1,590 KAS at once, the short 1,500.2 KAS, the lock holds 1,650 KAS.
#[test]
fn worked_example_amounts() {
    let l = lv(0x32);
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    let (d, lock) = guaranteed(&w);
    assert_eq!(d.outs[0].value, 13_700_000_000);
    assert_eq!(d.outs[1].value, 295_320_000_000);
    assert_eq!(lock.value, 330_000_000_000);
    let w = pend(&close(&l, FULL, LONG), CL_BID);
    let (d, lock) = guaranteed(&w);
    assert_eq!(d.outs[0].value, 159_000_000_000);
    assert_eq!(d.outs.last().unwrap().value, 150_020_000_000);
    assert_eq!(lock.value, 165_000_000_000);
}

// ================================================================ false claims disputed

/// A false claim, fast-settled and then disputed inside the window with exactly the counter-evidence that would have
/// refuted it by a challenge: the wronged party is paid the restitution H = the whole cover (DESIGN 4.5g, review 5 F1;
/// before, H was its side's value at the counter quote, which the first disputer chose), so it ends with at least what
/// its side was worth at the counter quote and at every other price; the reward R = cover * (lambda - 1) is the
/// executor's; the burn is the rest (here, without an offer, only the rounding of lambda: no burn output). Every kind
/// and side, whole and partial, at the refutation bound and at a quote far beyond it: the same H.
#[test]
fn a_false_claim_disputed_in_the_window_makes_the_victim_whole() {
    let l = lv(0x33);
    for n in [FULL, HALF] {
        for (g, e, c) in claims(&l, n) {
            let w = pend(&g, e.price());
            let (_, lock) = guaranteed(&w);
            let far = match c.kind {
                Kind::Ask => ask(if g.act.kind_side() == (KIND_CLOSE, LONG) { 950_000_000 } else { 1_000_000_000 }, n),
                Kind::Bid => bid(if g.act.kind_side() == (KIND_CLOSE, SHORT) { 1_190_000_000 } else { 1_100_000_000 }, n),
            };
            for counter in [c.clone(), far] {
                let d = dispute(&[lock.clone()], &counter);
                let tx = accept(&d);
                let h = lock.l.restitution();
                assert!(h > 0 && h == lock.l.cover);
                assert_eq!(tx.outputs[0].value as i64, h, "the victim is paid the cover");
                assert_eq!(tx.outputs[0].script_public_key, p2pk_x(&w.w.defender_key()));
                for p in [0, 900_000_000, 966_315_790, 1_000_000_000, 1_029_000_000, 1_100_000_000, 1_200_000_000, counter.price(), 5_000_000_000] {
                    assert!(victim_settled(&w) + h >= victim_fair(&w, p), "the victim ends at least whole at {p}");
                }
                // Burn accounting: the lock is H + burn + reward exactly; without an offer the burn is the rounding of
                // lambda (below dust: no burn output, it stays with the executor).
                assert_eq!(lock.l.reward(), lock.l.cover / 10);
                assert_eq!(lock.l.burn(lock.value), 0);
                assert!(lock.value as i64 - h - lock.l.reward() <= 1);
                assert!(tx.outputs.iter().all(|o| o.script_public_key != kpf_protocol::lock::burn_spk()));
                // One sompi less to the victim is refused.
                let mut x = d.clone();
                x.outs[0].value -= 1;
                refused(&x, 0);
            }
        }
    }
}

/// The margin g at its boundary for disputes, as for challenges: one tick in the claim's favour is refused; and the
/// counter-evidence must be of the opposite kind.
#[test]
fn a_dispute_one_tick_short_of_the_margin_is_refused() {
    let l = lv(0x34);
    for (g, e, c) in claims(&l, FULL) {
        let w = pend(&g, e.price());
        let (_, lock) = guaranteed(&w);
        accept(&dispute(&[lock.clone()], &c));
        refused(&dispute(&[lock.clone()], &one_tick_short(&c)), 0);
    }
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    let (_, lock) = guaranteed(&w);
    refused(&dispute(&[lock], &ask(REF_LIQ_LONG, FULL)), 0);
}

/// The clock, life and coverage rules of a challenge hold for a dispute: the counter-evidence counts only from the
/// claim's P (an order resting since long before P has rested only P + R - 1 at a lock time one short), it must stay
/// fillable through its rest end plus L, and its fill must cover k_c times n (two locks in one block: both n).
#[test]
fn a_dispute_needs_the_clock_life_and_coverage_of_a_challenge() {
    let l = lv(0x35);
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    let (_, lock) = guaranteed(&w);
    let c = bid(1_000_000_000, FULL);
    // Clock: the order rested since NOW - R - 50 < P; it counts from P = NOW - R - 10, so its rest ends at NOW.
    let rest_end = w.daa + rest_daa();
    let mut d = dispute(&[lock.clone()], &c);
    d.lock = rest_end;
    accept(&d);
    d.lock = rest_end - 1;
    refused(&d, 0);
    // An order placed after P counts from its own DAA.
    let late = c.clone().placed(w.daa + 100);
    let mut d = dispute(&[lock.clone()], &late);
    d.lock = w.daa + 100 + rest_daa();
    accept(&d);
    d.lock -= 1;
    refused(&d, 0);
    // Life: fillable through rest end + L, one DAA less refused.
    let life = rest_end as i64 + market().min_evidence_life;
    for (exp, ok) in [(life, true), (life - 1, false)] {
        let mut x = c.clone();
        x.bid.expiry_daa = exp;
        let d = dispute(&[lock.clone()], &x);
        if ok {
            accept(&d);
        } else {
            refused(&d, 0);
        }
    }
    // An IOC bid can never serve (L >= 600).
    refused(&dispute(&[lock.clone()], &Evidence::ending_bid(1_000_000_000, FULL, 1)), 0);
    // Coverage: a fill of k_c * n - 1 is refused; two locks in one block need the sum.
    refused(&dispute(&[lock.clone()], &bid(1_000_000_000, FULL - 1)), 0);
    let w2 = pend(&liq(&lv(0x36), FULL, LONG), LIQ_ASK);
    let (_, lock2) = guaranteed(&w2);
    refused(&dispute(&[lock.clone(), lock2.clone()], &bid(1_000_000_000, 2 * FULL - 1)), 0);
    accept(&dispute(&[lock, lock2], &bid(1_000_000_000, 2 * FULL)));
}

/// The window: counter-evidence must finish its rest by F + D - R (the lock is liable for the fixed window
/// [P, F + D - R], whenever the release lands); one DAA later it is refused. A release needs age D: one DAA early it is
/// refused, at D it is accepted; at D both are valid and exactly one can land (they spend one UTXO).
#[test]
fn a_dispute_after_the_window_is_refused_and_a_release_one_daa_early() {
    let l = lv(0x37);
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    let (_, lock) = guaranteed(&w);
    let c = bid(1_000_000_000, FULL);
    let rest_end = w.daa + rest_daa();
    // The latest F for which this evidence (rest end P + R) is inside the window: F + D - R = rest end.
    let f_edge = rest_end + rest_daa() - dispute_window();
    let mut d = dispute(&[lock.clone().at(f_edge)], &c);
    d.lock = rest_end;
    accept(&d);
    let mut d = dispute(&[lock.clone().at(f_edge - 1)], &c);
    d.lock = rest_end;
    refused(&d, 0);
    // The lock time plays no part in the window: a later lock time does not reopen it.
    let mut d = dispute(&[lock.clone().at(f_edge - 1)], &c);
    d.lock = rest_end + 1_000_000;
    refused(&d, 0);
    // Release at D, not at D - 1.
    let r = release_draft(&lock, &fk(), 7);
    accept(&r);
    let mut early = r.clone();
    early.seqs[0] = dispute_window() - 1;
    refused(&early, 0);
    // A lock in the mempool (its DAA unknown, read as -1) cannot be disputed.
    let mut d = dispute(&[lock.clone().at(u64::MAX)], &c);
    d.lock = rest_end;
    refused(&d, 0);
}

// ================================================================ the guarantee's own checks

/// The lock must hold ceil(cover * lambda) + offer: one sompi short is refused, for every claim; a lock of another
/// claim, another owner or another template is refused.
#[test]
fn a_lock_one_sompi_short_or_of_another_claim_is_refused() {
    let l = lv(0x38);
    for n in [FULL, HALF] {
        for (g, e, _) in claims(&l, n) {
            let w = pend(&g, e.price());
            let d = guarantee_draft(&w, &gk(), &fk(), 5);
            accept(&d);
            let j = d.locks[0].0;
            let mut x = d.clone();
            x.outs[j].value -= 1;
            refused(&x, 0);
            // Another victim, cover, clock or claim id in the lock.
            let base_lock = d.locks[0].1.clone();
            let muts: Vec<Box<dyn Fn(&mut Lock)>> = vec![
                Box::new(|k| k.victim = xonly(&keypair(13))),
                Box::new(|k| k.cover -= 1),
                Box::new(|k| k.clock += 1),
                Box::new(|k| k.claim_id = Hash::from_bytes([0x77; 32])),
                Box::new(|k| k.quote += 1),
                Box::new(|k| k.status = 2),
                Box::new(|k| k.m.dispute_daa += 1),
            ];
            for (k, m) in muts.iter().enumerate() {
                let mut lk2 = base_lock.clone();
                m(&mut lk2);
                let mut x = d.clone();
                x.outs[j].script_public_key = p2sh(&lk2.art());
                let (tx, en) = x.build();
                assert!(run(&tx, &en).unwrap()[0].is_err(), "lock mutation {k} must be refused");
            }
            // The owner key is the guarantor's choice: any key the argument names.
            let other = xonly(&keypair(15));
            let mut x = d.clone();
            let mut lk3 = base_lock.clone();
            lk3.owner = other;
            x.outs[j].script_public_key = p2sh(&lk3.art());
            x.set_args(0, guarantee_args(j as i64, &other));
            accept(&x);
        }
    }
    // Another lock template (one suffix byte changed).
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    let d = guarantee_draft(&w, &gk(), &fk(), 5);
    let t = kpf_protocol::artifacts::lock_template();
    let mut suf = t.suffix.clone();
    let last = suf.len() - 1;
    suf[last] ^= 1;
    let mut x = d.clone();
    x.set_args(0, vec![iv(2), bv(&gk()), bv(&t.prefix), bv(&suf)]);
    x.outs[2].script_public_key = kaspa_txscript::pay_to_script_hash_script(&[t.prefix.clone(), d.locks[0].1.state_bytes(), suf].concat());
    refused(&x, 0);
}

/// The settlement of a guarantee is finalize's, to the sompi: the proposer, the forfeited part, the closer and the other
/// party each one sompi short is refused.
#[test]
fn the_settlement_of_a_guarantee_is_exact() {
    let l = lv(0x39);
    for n in [FULL, HALF] {
        for (g, e, _) in claims(&l, n) {
            let w = pend(&g, e.price());
            let d = guarantee_draft(&w, &gk(), &fk(), 5);
            accept(&d);
            let idx: Vec<usize> = if w.w.kind == KIND_LIQ { vec![0, 1] } else { vec![0, d.outs.len() - 1] };
            for i in idx {
                let mut x = d.clone();
                x.outs[i].value -= 1;
                refused(&x, 0);
            }
        }
    }
}

/// The late-submission rule (O§1.4): a pending part accepted after its proposal's evidence stopped being fillable by
/// conforming matchers (P > visibleUntil) cannot be guaranteed; it keeps the slow path. P = visibleUntil is accepted; a
/// part in the mempool (P read as -1) is refused.
#[test]
fn a_part_accepted_after_its_evidence_died_cannot_be_guaranteed() {
    let l = lv(0x3a);
    let g = liq(&l, FULL, LONG);
    let w = pend(&g, LIQ_ASK);
    let vis = w.w.visible_until as u64;
    accept(&guarantee_draft(&w.clone().at(vis), &gk(), &fk(), 5));
    refused(&guarantee_draft(&w.clone().at(vis + 1), &gk(), &fk(), 5), 0);
    refused(&guarantee_draft(&w.clone().at(u64::MAX), &gk(), &fk(), 5), 0);
    // The slow path of the late part is unchanged: finalize after W.
    accept(&finalize_draft(&w.at(vis + 1), &fk(), 6));
}

/// visibleUntil, as propose writes it: the earliest of the evidence's expiryDaa and its 90-day idle limit (an IOC/FOK
/// kill cannot be evidence while L >= 600); a pending state with any other value is refused at propose.
#[test]
fn propose_writes_visible_until_from_the_evidence() {
    let l = lv(0x3b);
    let g = liq(&l, FULL, LONG);
    for exp in [1_000_000_000_000, 20_000_000] {
        let mut e = ask(LIQ_ASK, FULL);
        e.ask.expiry_daa = exp;
        let d = ev_draft(&[g.clone()], &e, &fk(), 1);
        accept(&d);
        let w = &d.pendings[0].1;
        assert_eq!(w.visible_until, exp.min(e.daa as i64 + 77_760_000));
        for v in [w.visible_until - 1, w.visible_until + 1] {
            let mut w2 = w.clone();
            w2.visible_until = v;
            let mut x = d.clone();
            x.outs[0].script_public_key = p2sh(&w2.art());
            refused(&x, 0);
        }
    }
}

/// The cure grace G (DESIGN 4.5f): a liquidation can be guaranteed only once the pending part is G old; within G the
/// loser can still cure. A close needs no grace (its non-actor has no cure on the slow path either).
#[test]
fn a_liquidation_is_guaranteed_only_after_the_cure_grace() {
    let l = lv(0x3c);
    for (g, e, _) in claims(&l, FULL) {
        let w = pend(&g, e.price());
        let d = guarantee_draft(&w, &gk(), &fk(), 5);
        if w.w.kind == KIND_LIQ {
            let mut x = d.clone();
            x.seqs[0] = grace() - 1;
            refused(&x, 0);
            accept(&d);
            // Within the grace the cure is accepted (and after it too, until something else lands).
            let signer = if w.w.side == LONG { lk() } else { sk() };
            accept(&cure_draft(&w, &signer, &fk(), 6));
        } else {
            let mut x = d.clone();
            x.seqs = vec![0, 0];
            accept(&x);
        }
    }
}

/// The proposer's offer (DESIGN 4.5f): up to bond - 0.02 KAS, written at propose; a guarantee pays the proposer its
/// payout less the offer, and the lock must hold the offer on top of lambda x cover (it returns to the guarantor with
/// the lock). Finalize pays the proposer in full whatever the offer.
#[test]
fn the_offer_is_paid_from_the_proposers_payout_into_the_lock() {
    let l = lv(0x3d);
    let g = liq(&l, FULL, LONG);
    let e = ask(LIQ_ASK, FULL);
    let mut d = ev_draft(&[g.clone()], &e, &fk(), 1);
    let bond = d.pendings[0].1.bond;
    for (offer, ok) in [(bond - kpf_protocol::state::PLAIN_DUST, true), (bond - kpf_protocol::state::PLAIN_DUST + 1, false), (-1, false), (1_000_000_000, true)] {
        let mut a = d.args(0);
        a[ARG_OFFER] = iv(offer);
        d.set_args(0, a);
        let mut w = d.pendings[0].1.clone();
        w.offer = offer;
        let mut x = d.clone();
        x.outs[0].script_public_key = p2sh(&w.art());
        if ok {
            accept(&x);
        } else {
            refused(&x, 0);
        }
    }
    // A pending part with a 10 KAS offer: the guarantee pays the proposer 137 - 10 KAS, the lock 3,300 + 10 KAS.
    let mut w = pend(&g, LIQ_ASK);
    w.w.offer = 1_000_000_000;
    let d = guarantee_draft(&w, &gk(), &fk(), 5);
    accept(&d);
    assert_eq!(d.outs[0].value, 12_700_000_000);
    assert_eq!(d.outs[2].value, 331_000_000_000);
    let mut x = d.clone();
    x.outs[2].value -= 1;
    refused(&x, 0);
    let f = finalize_draft(&w, &fk(), 6);
    accept(&f);
    assert_eq!(f.outs[0].value, 13_700_000_000, "finalize ignores the offer");
    // A close with an offer: the closer pays it.
    let mut w = pend(&close(&l, FULL, LONG), CL_BID);
    w.w.offer = 1_000_000_000;
    let d = guarantee_draft(&w, &gk(), &fk(), 5);
    accept(&d);
    assert_eq!(d.outs[0].value, 158_000_000_000);
    assert_eq!(d.outs[1].value, 166_000_000_000);
}

// ================================================================ one lock, two claims

/// One lock cannot settle two claims: it names one claim (its id, its state and P), so in one transaction the second
/// part refuses it; across transactions a guarantee reads only its own transaction's outputs, so a lock created earlier
/// is no lock for a later claim, and a copy of its script names the first claim.
#[test]
fn one_lock_for_two_claims_is_refused_in_one_transaction_and_across_transactions() {
    let a = pend(&liq(&lv(0x41), FULL, LONG), LIQ_ASK);
    let b = pend(&liq(&lv(0x42), FULL, LONG), LIQ_ASK);
    // One transaction: inputs a, b, F; outputs a's settlement, b's settlement, one lock of a.
    let da = guarantee_draft(&a, &gk(), &fk(), 5);
    let db = guarantee_draft(&b, &gk(), &fk(), 5);
    let mut ins = vec![da.ins[0].clone(), db.ins[0].clone(), da.ins[1].clone()];
    ins[1].spend = Spend::Signed { art: b.w.art(), entry: "guarantee", args: guarantee_args(4, &gk()) };
    // b's proposer and forfeited part at b's own slot 1 cannot coexist with a's forfeited part at its id; use the
    // positional layout: out 0 a's proposer, 1 b's proposer, 2 a's part, 3 b's part, 4 the one lock (of a), 5 change.
    ins[0].spend = Spend::Signed { art: a.w.art(), entry: "guarantee", args: guarantee_args(4, &gk()) };
    let outs = vec![
        da.outs[0].clone(),
        db.outs[0].clone(),
        da.outs[1].clone(),
        { let mut o = db.outs[1].clone(); o.covenant = bind(1, b.id); o },
        da.outs[2].clone(),
        out(0, p2pk_spk(&fk()), None),
    ];
    let mut d = Draft::new(ins, outs, Some(5));
    d.seqs = vec![grace(), grace(), 0];
    let (tx, e) = d.build();
    let r = run(&tx, &e).unwrap();
    assert!(r[0].is_ok(), "a's guarantee accepts its own lock: {:?}", r[0]);
    assert!(r[1].is_err(), "b refuses a lock that names a");
    // Across transactions: a's lock, accepted earlier, copied for b (the same script and value) still names a.
    let mut x = db.clone();
    x.outs[2].script_public_key = da.outs[2].script_public_key.clone();
    refused(&x, 0);
}

// ================================================================ who may guarantee

/// The guarantor may be anyone: the proposer, a party of the pair (the winner or the loser), or the disputer. Each is
/// accepted; the guarantee binds the lock, not the guarantor's identity. A guarantor that disputes its own lock gets
/// the reward back at most: since DESIGN 4.5g it loses exactly the cover, all of it paid to the wronged party (before, it
/// lost lambda x cover - R - H more than a quote-priced restitution, burned).
#[test]
fn the_guarantor_may_be_the_proposer_a_party_or_the_disputer() {
    let l = lv(0x43);
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    for owner in [xonly(&proposer_kp()), xonly(&lk()), xonly(&sk()), xonly(&fk())] {
        let mut d = guarantee_draft(&w, &owner, &fk(), 5);
        d.set_args(0, guarantee_args(2, &owner));
        accept(&d);
    }
    // The guarantor disputes its own lock: its change gains only the reward, while H goes to the long and the rest burns.
    let f_kp = fk();
    let (_, lock) = {
        let d = guarantee_draft(&w, &xonly(&f_kp), &f_kp, 5);
        let tx = accept(&d);
        (d.clone(), live_locks(&tx, &d).remove(0).at(w.daa + grace()))
    };
    let c = bid(1_000_000_000, FULL);
    let d = dispute(&[lock.clone()], &c);
    let (tx, e) = d.build();
    assert_accepted(&tx, &e);
    let ins: i64 = e.iter().map(|u| u.amount as i64).sum();
    // DESIGN 4.5g: H is the cover; without an offer nothing is left to burn.
    assert_eq!(tx.outputs[0].value as i64, lock.l.cover);
    let paid_out_of_lock = tx.outputs[0].value as i64 + lock.l.burn(lock.value);
    assert_eq!(lock.value as i64 - paid_out_of_lock, lock.l.reward(), "the self-disputing guarantor recovers exactly the reward");
    assert_eq!(lock.l.reward(), lock.value as i64 - lock.l.restitution(), "it loses exactly the restitution, which the victim gets");
    let _ = ins;
}

// ================================================================ every ordering on one claim

/// Challenge, cure, guarantee and finalize all spend the one pending UTXO, so exactly one of them lands (each is valid
/// alone, here at age W where all four are). What each leaves behind admits only its own continuation: a restored or
/// cured part is a KpfPair (no guarantee, no dispute), a lock is a KpfLock (no challenge, no cure, no finalize), a
/// finalized claim leaves no lock to dispute. On a lock, dispute and release spend one UTXO: exactly one lands.
#[test]
fn every_ordering_of_challenge_cure_guarantee_finalize_dispute_release() {
    let l = lv(0x44);
    let g = liq(&l, FULL, LONG);
    let w = pend(&g, LIQ_ASK);
    let counter = bid(REF_LIQ_LONG, FULL);
    // At age W every path is open on the same UTXO.
    let ch = ch_draft(&[w.clone()], &counter, &fk(), 2);
    let cu = cure_draft(&w, &lk(), &fk(), 3);
    let mut gu = guarantee_draft(&w, &gk(), &fk(), 4);
    gu.seqs[0] = window();
    let fi = finalize_draft(&w, &fk(), 5);
    let spent: Vec<_> = [&ch, &cu, &gu, &fi].iter().map(|d| d.ins.iter().find(|i| i.outpoint == w.op).map(|i| i.outpoint)).collect();
    assert!(spent.iter().all(|s| *s == Some(w.op)), "all four spend the pending UTXO");
    let txc = accept(&ch);
    accept(&cu);
    let txg = accept(&gu);
    accept(&fi);
    // After a challenge: the restored pair has no guarantee entry (its ABI is the pair's).
    let pair_entries: Vec<String> = pair_base().contracts.values().next().unwrap().entries.keys().cloned().collect();
    assert!(!pair_entries.iter().any(|e| e == "guarantee" || e == "dispute" || e == "release"));
    let restored = live_pairs(&txc, &ch).remove(0);
    assert_eq!(restored.p.long_key, l.p.long_key);
    // After a guarantee: the lock's ABI has no challenge, cure or finalize; the dispute uses the same counter-evidence.
    let lock_entries: Vec<String> = kpf_protocol::artifacts::lock_base().contracts.values().next().unwrap().entries.keys().cloned().collect();
    assert_eq!(lock_entries, vec!["dispute".to_string(), "release".to_string()]);
    let lock = live_locks(&txg, &gu).remove(0).at(w.daa + window());
    let dp = dispute(&[lock.clone()], &counter);
    let rl = release_draft(&lock, &fk(), 8);
    assert_eq!(dp.ins[0].outpoint, rl.ins[0].outpoint, "dispute and release spend the one lock");
    accept(&dp);
    accept(&rl);
    // A challenged claim cannot be guaranteed, and a guaranteed one cannot be challenged: the pending UTXO is gone; a
    // challenge naming the lock's outpoint with the pending's script is not the lock's script.
    let mut fake = ch.clone();
    fake.ins[0].outpoint = lock.op;
    fake.ins[0].utxo = lock_input(&lock, "dispute", vec![]).utxo;
    let (tx, e) = fake.build();
    // (The restored output it binds to the pending's id has no authorizing input of that id any more: consensus refuses
    // the bindings, or the input fails.)
    assert!(run(&tx, &e).map(|r| r[0].is_err()).unwrap_or(true), "the lock is not a pending part");
}

// ================================================================ the forfeited part while disputable

/// The winner of a fast-settled liquidation holds the forfeited part (both sides its own) at once and may act on it while
/// the lock is still disputable: a cooperative close with its own key twice, a split by a close, a sale of a side after
/// the hand-over limit. A later successful dispute pays the wronged long from the lock, in full, whatever became of the
/// part: restitution never touches the part.
#[test]
fn the_winner_acts_on_the_forfeited_part_while_the_lock_is_disputable() {
    let l = lv(0x45);
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    let d = guarantee_draft(&w, &gk(), &fk(), 5);
    let tx = accept(&d);
    let part = live_pairs(&tx, &d).remove(0).at(w.daa + grace());
    assert_eq!((part.p.long_key, part.p.short_key), (l.p.short_key, l.p.short_key));
    let lock = live_locks(&tx, &d).remove(0).at(w.daa + grace());
    // The winner closes the whole forfeited part cooperatively (both signatures its own).
    let ins = vec![pair_input(&part, "coopClose", vec![Arg::Sig(sk()), Arg::Sig(sk())]), funder(&fk(), 9, F_FUNDS)];
    let outs = vec![out(part.value - 1_000_000, p2pk_x(&part.p.short_key), None), out(0, p2pk_spk(&fk()), None)];
    accept(&Draft::new(ins, outs, Some(1)));
    // Liquidating the forfeited part again is refused (one owner, DESIGN 4.5e).
    let mut d2 = ev_draft(&[liq(&part, FULL, LONG)], &ask(LIQ_ASK, FULL), &fk(), 10);
    d2.lock = NOW + grace() + 10_000;
    refused(&d2, 0);
    // The dispute pays the long in full regardless.
    let c = bid(1_000_000_000, FULL);
    let dp = dispute(&[lock.clone()], &c);
    let txd = accept(&dp);
    assert_eq!(txd.outputs[0].value as i64, q(FULL, 300_000_000, SCALE, 0), "the long is paid the cover (DESIGN 4.5g): 3,000 KAS");
    assert_eq!(txd.outputs[0].script_public_key, p2pk_x(&l.p.long_key));
}

// ================================================================ boundary values and overflow

/// lambda at its bounds: 1 (no reward) and 3; a look-alike pending part with lambda outside
/// [1, 3] or a dispute window outside the CSV range cannot be guaranteed (the lock it would create could overflow or
/// never be released), while its slow path stays.
#[test]
fn boundary_lambda_and_window() {
    for (lb, ok) in [(10_000, true), (30_000, true), (9_999, false), (30_001, false)] {
        let mut p = base();
        p.m.lock_bps = lb;
        let l = live(&p, 0x46).at(1_000);
        let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
        let d = guarantee_draft(&w, &gk(), &fk(), 5);
        if ok {
            let tx = accept(&d);
            let lock = live_locks(&tx, &d).remove(0).at(w.daa + grace());
            assert_eq!(lock.value as i64, (3 * 10i64.pow(11) / 10_000) * lb);
            let dp = dispute(&[lock.clone()], &bid(1_000_000_000, FULL));
            let txd = accept(&dp);
            assert_eq!(lock.l.reward(), 3 * 10i64.pow(11) / 10_000 * (lb - 10_000));
            // DESIGN 4.5g: H is the cover, so at lambda 1 and 3 (no offer) nothing is left to burn.
            assert_eq!(txd.outputs[0].value as i64, lock.l.cover);
            assert_eq!(lock.value as i64 - lock.l.restitution() - lock.l.reward(), 0);
            assert!(txd.outputs.iter().all(|o| o.script_public_key != kpf_protocol::lock::burn_spk()));
        } else {
            refused(&d, 0);
            accept(&finalize_draft(&w, &fk(), 6));
        }
    }
    for dd in [-1, 4_294_967_296] {
        let mut p = base();
        p.m.dispute_daa = dd;
        let l = live(&p, 0x47).at(1_000);
        let w = pend(&close(&l, FULL, LONG), CL_BID);
        refused(&guarantee_draft(&w, &gk(), &fk(), 5), 0);
        accept(&finalize_draft(&w, &fk(), 6));
    }
}

/// The largest cover a real lock can carry: a part worth the whole KAS supply (2.9 * 10^18 sompi) at lambda 3. Every
/// product stays in 63 bits (the split forms of KpfPending.guarantee and KpfLock.dispute); the guarantee runs. A close
/// whose cover is 0 locks the 0.1 KAS floor.
#[test]
fn boundary_large_cover_and_zero_cover() {
    // A close of the long at a bid at or below bl pays the long nothing: cover 0, the lock is the 0.1 KAS floor.
    let l = lv(0x48);
    let w = pend(&close(&l, FULL, LONG), 800_000_000);
    let d = guarantee_draft(&w, &gk(), &fk(), 5);
    let tx = accept(&d);
    assert_eq!(d.locks[0].1.cover, 0);
    assert_eq!(d.outs[1].value as i64, kpf_protocol::lock::LOCK_FLOOR);
    let mut x = d.clone();
    x.outs[1].value -= 1;
    refused(&x, 0);
    // No counter ask contradicts a close at a quote that clamps to bl (as for a challenge): its lock is never disputable.
    let lock = live_locks(&tx, &d).remove(0).at(w.daa + 1);
    refused(&dispute(&[lock], &ask(700_000_000, FULL)), 0);
    // A part worth the KAS supply: 29,000 tokens over the range [0, 10^6 KAS], the short liquidated at a bid of 990,000 KAS.
    let mut p = base();
    (p.size, p.bl, p.bs, p.entry) = (29_000 * SCALE, 0, PRICE_MAX, 50_000_000_000_000);
    p.m.lock_bps = 30_000;
    let l = Live { value: p.floor() as u64, ..live(&p, 0x49).at(1_000) };
    let w = pend(&liq(&l, p.size, SHORT), 99_000_000_000_000);
    let lock = w.w.lock_of(gk(), w.id, w.daa, w.value);
    assert_eq!(lock.cover, 2_900_000_000_000_000_000);
    assert_eq!(w.w.lock_min(w.value), 8_700_000_000_000_000_000);
    let mut d = guarantee_draft(&w, &gk(), &fk(), 5);
    d.ins[1].utxo.amount = 8_800_000_000_000_000_000;
    accept(&d);
    let mut x = d.clone();
    x.outs[2].value -= 1;
    refused(&x, 0);
}
