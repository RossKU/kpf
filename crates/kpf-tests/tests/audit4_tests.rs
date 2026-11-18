//! Review 4 (docs/audit/REVIEW-4.md): attacks on the two-step evidence mechanism (KpfPair.propose, KpfPending) after
//! the restructuring of DESIGN 4.5d. Every input runs its real script in the engine, the KOB evidence orders and the
//! token program included. An `exploit_*` test builds a transaction (or a sequence) the engine accepts although a party
//! loses value it did not consent to lose, or the design says it should not be possible; a `holds_*` test shows an
//! attack refused.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::Transaction;
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

const N: i64 = 1_000_000_000;
const R: u64 = 600;

fn lv(tag: u8, daa: u64) -> Live {
    live(&pair(&lk(), &sk()), tag).at(daa)
}

fn liq(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side } }
}

fn liq_by(l: &Live, n: i64, side: i64, proposer: [u8; 32]) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::LiqBy { side, proposer } }
}

fn close(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side, signer: if side == LONG { lk() } else { sk() } } }
}

fn at_lock(d: &Draft, lock: u64) -> Draft {
    let mut x = d.clone();
    x.lock = lock;
    x
}

// ================================================================ R4-F1: the forfeited part is liquidatable at birth

/// R4-F1 (High). Forfeit and keep leaves the winner a pair whose two sides are both its own, with the loser's bound moved
/// INWARD by the fee (bl' = fb > bl for a long loser) and a fresh clock S' (since = 0). The quote that liquidated the old
/// long therefore liquidates the forfeited part's long again: p * (10^4 - mmr) < bl < fb. Any third party (here the
/// former loser itself, now with nothing at stake) re-proposes the liquidation of the forfeited part once its ask has
/// rested R on S', and anyone finalizes it W later: the winner's part becomes a new forfeited part, again both sides the
/// winner's, minus the keeper fee, which the proposer collects with its bond. Nothing moves between the "long" and the
/// "short" (both are the winner), so the fee is a pure transfer from the winner to the proposer, repeated every R + W
/// while the winner is offline, until the narrowing range fails the range condition. The design states that the winner
/// "carries no price exposure meanwhile" (THREAT_MODEL B4); it does carry this.
/// Who loses: the winner of an honest liquidation, 47 KAS per round on the 1,000-token example (the fee at 9.40), about
/// 1,800 KAS (61 % of the forfeited part) before the range condition stops it.
#[test]
fn exploit_forfeited_part_is_liquidatable_at_birth() {
    let s = 100_000u64;
    let l = lv(0x51, s);
    let ask = Evidence::ask(940_000_000, N).placed(s + 10);
    // The honest liquidation of the long, finalized: the short owns both sides of the forfeited part.
    let p = at_lock(&ev_draft(&[liq(&l, N, LONG)], &ask, &fk(), 1), s + 10 + R);
    let ptx = accept(&p);
    let mut t = s + 10 + R;
    let w = live_pendings(&ptx, &p).remove(0).at(t);
    let fd = finalize_draft(&w, &fk(), 2);
    let ftx = accept(&fd);
    t += window();
    let mut fp = live_pairs(&ftx, &fd).remove(0).at(t);
    let winner = xonly(&sk());
    assert_eq!((fp.p.long_key, fp.p.short_key, fp.p.since), (winner, winner, 0));
    assert!(fp.p.bl > l.p.bl, "the forfeited long's bound moved inward by the fee");
    let start = fp.value as i64;
    let former_loser = xonly(&lk());
    let mut collected = 0i64;
    for round in 0..3u32 {
        let g = liq_by(&fp, N, LONG, former_loser);
        // The same 9.40 ask (it rested before S', so it counts from S'): refused one DAA before S' + R, accepted at it.
        let d = ev_draft(&[g.clone()], &ask, &fk(), 10 + round);
        refused(&at_lock(&d, t + R - 1), 0);
        let d = at_lock(&d, t + R);
        let tx = accept(&d);
        let wp = live_pendings(&tx, &d).remove(0).at(t + R);
        assert_eq!(wp.w.proposer_key, former_loser);
        // Nobody acts for the offline winner: anyone finalizes after W.
        let f = finalize_draft(&wp, &fk(), 20 + round);
        let ftx = accept(&f);
        assert_eq!(f.outs[0].value as i64, wp.w.bond + wp.w.fee, "round {round}: the proposer takes its bond back and the fee");
        assert!(wp.w.fee >= 4_000_000_000, "round {round}: 47 KAS of fee at 9.40 on 1,000 tokens");
        collected += wp.w.fee;
        t += R + window();
        let next = live_pairs(&ftx, &f).remove(0).at(t);
        assert_eq!((next.p.long_key, next.p.short_key), (winner, winner), "round {round}: still both sides the winner's");
        assert_eq!(next.value as i64, fp.value as i64 - wp.w.fee, "round {round}: the winner's part lost exactly the fee");
        fp = next;
    }
    assert_eq!(start - fp.value as i64, collected);
    assert!(collected >= 3 * 4_000_000_000, "the winner paid three keeper fees for liquidating itself");
}

/// R4-F1, the extent: the rounds continue at the same ask until the forfeited range fails the range condition
/// fb * (10^4 + mmr) < bs * (10^4 - mmr); every round costs the winner the 47 KAS fee.
#[test]
fn exploit_forfeited_part_is_drained_until_the_range_condition() {
    let s = 100_000u64;
    let l = lv(0x57, s);
    let ask = Evidence::ask(940_000_000, N).placed(s + 10);
    let p = at_lock(&ev_draft(&[liq(&l, N, LONG)], &ask, &fk(), 1), s + 10 + R);
    let ptx = accept(&p);
    let mut t = s + 10 + R;
    let w = live_pendings(&ptx, &p).remove(0).at(t);
    let fd = finalize_draft(&w, &fk(), 2);
    let ftx = accept(&fd);
    t += window();
    let mut fp = live_pairs(&ftx, &fd).remove(0).at(t);
    let start = fp.value as i64;
    let mut rounds = 0u32;
    loop {
        let d = at_lock(&ev_draft(&[liq_by(&fp, N, LONG, xonly(&lk()))], &ask, &fk(), 100 + rounds), t + R);
        let (tx, e) = d.build();
        if !run(&tx, &e).expect("bindings").iter().all(|r| r.is_ok()) {
            break;
        }
        let wp = live_pendings(&tx, &d).remove(0).at(t + R);
        let f = finalize_draft(&wp, &fk(), 200 + rounds);
        let ftx = accept(&f);
        t += R + window();
        fp = live_pairs(&ftx, &f).remove(0).at(t);
        rounds += 1;
        assert!(rounds < 200);
    }
    let lost = start - fp.value as i64;
    let p0 = &fp.p;
    assert!(p0.bl * (10_000 + 500) >= p0.bs * (10_000 - 500), "stopped by the range condition");
    println!("R4-F1: {rounds} rounds, the winner lost {lost} sompi of {start}");
    assert!(rounds >= 30 && lost * 2 > start, "most of the forfeited part goes in keeper fees");
}

// ================================================================ R4-F3: a cure is reopened by the next quote

/// R4-F3 (Low). The cure moves the loser's bound exactly to the bound at which the PROPOSED quote no longer triggers
/// (DESIGN 4.5d), and the cured part keeps the old clock S. An ask one sompi lower that rested on S is therefore
/// evidence against the cured part at once, with no rest: the keeper proposes it in the very next transaction after the
/// cure (lock time = the cure's own DAA). The loser cannot cure further in one step (more margin is a separate add
/// margin, which races the next proposal), and a finalized second claim forfeits the margin the loser has just added.
/// Who loses: the loser's cure margin (70 KAS on the example) when it misses the next window; each round otherwise costs
/// it a cure transaction while the proposer gets its bond back (no cure fee).
#[test]
fn exploit_cure_is_reopened_by_a_lower_rested_ask() {
    let s = 100_000u64;
    let l = lv(0x52, s);
    let a1 = Evidence::ask(940_000_000, N).placed(s + 10);
    let a2 = Evidence::ask(939_999_999, N).placed(s + 10).other(0xd5);
    let g = liq(&l, N, LONG);
    let p_daa = s + 10 + R;
    let p = at_lock(&ev_draft(&[g.clone()], &a1, &fk(), 1), p_daa);
    let ptx = accept(&p);
    let w = live_pendings(&ptx, &p).remove(0).at(p_daa);
    let cd = cure_draft(&w, &lk(), &fk(), 2);
    let ctx = accept(&cd);
    let cure_daa = p_daa + 20;
    let cured = live_pairs(&ctx, &cd).remove(0).at(cure_daa);
    assert_eq!((cured.p.bl, cured.p.since), (893_000_000, s as i64), "cured to 8.93 with the clock S kept");
    let added = cured.value as i64 - l.value as i64;
    assert!(added >= 7_000_000_000, "the loser added 70 KAS");
    // The next ask, one sompi lower, rested on S long ago: a proposal at the cure's own DAA is accepted.
    let g2 = liq(&cured, N, LONG);
    let d2 = at_lock(&ev_draft(&[g2.clone()], &a2, &fk(), 3), cure_daa);
    let tx2 = accept(&d2);
    let w2 = live_pendings(&tx2, &d2).remove(0).at(cure_daa);
    assert_eq!(w2.w.cure_bound, 892_999_999, "the next cure moves the bound by one sompi of quote");
    // Unanswered, it forfeits the long's whole part, the margin just added included.
    let f = finalize_draft(&w2, &fk(), 4);
    let ftx = accept(&f);
    let fp = live_pairs(&ftx, &f).remove(0);
    assert_eq!((fp.p.long_key, fp.p.short_key), (xonly(&sk()), xonly(&sk())));
}

// ================================================================ R4-F5: W = R leaves no slack

/// R4-F5 (Informational). KpfOpen.checkMarket accepts challengeDaa = minRestDaa (W >= R). Then the earliest challenge
/// with counter-evidence placed at P (tx.daa >= P + R by CLTV) and the earliest finalize (age >= W = R by CSV) open at
/// the same DAA: a defender that only learns of the proposal at P has zero slack, not "W - R" for detection and relay.
#[test]
fn exploit_window_equal_to_rest_leaves_no_slack() {
    let mut p = pair(&lk(), &sk());
    p.m.challenge_daa = p.m.min_rest_daa;
    let l = live(&p, 0x53).at(1_000);
    let p_daa = NOW - R;
    let w = pend_of(&liq(&l, N, LONG), 940_000_000, p_daa);
    assert_eq!(w.w.m.challenge_daa as u64, R);
    let c = ch_draft(&[w.clone()], &Evidence::bid(980_000_000, N).placed(p_daa), &fk(), 1);
    refused(&at_lock(&c, p_daa + R - 1), 0);
    accept(&at_lock(&c, p_daa + R));
    let mut f = finalize_draft(&w, &fk(), 2);
    f.seqs = vec![R, 0];
    accept(&f);
}

// ================================================================ holds

/// The counter-evidence must cover k_c * n: a counter bid that refutes by price but fills one base unit less than the
/// part is refused by the engine (the tooling side of this is R4-F2).
#[test]
fn holds_counter_fill_must_cover_kc_n() {
    let l = lv(0x54, 1_000);
    let w = pend(&liq(&l, N, LONG), 940_000_000);
    refused(&ch_draft(&[w.clone()], &Evidence::bid(980_000_000, N - 1), &fk(), 1), 0);
    accept(&ch_draft(&[w], &Evidence::bid(980_000_000, N), &fk(), 2));
}

/// The four contradiction rows at equality and one tick on the wrong side (DESIGN 4.5d: equality contradicts).
#[test]
fn holds_contradiction_rows_at_the_tick() {
    let l = lv(0x55, 1_000);
    let cases: Vec<(PairLeg, i64, Evidence, Evidence)> = vec![
        (liq(&l, N, LONG), 940_000_000, Evidence::bid(966_315_790, N), Evidence::bid(966_315_789, N)),
        (liq(&l, N, SHORT), 1_150_000_000, Evidence::ask(1_120_000_000, N), Evidence::ask(1_120_000_001, N)),
        (close(&l, N, LONG), 1_050_000_000, Evidence::ask(1_029_000_000, N), Evidence::ask(1_029_000_001, N)),
        (close(&l, N, SHORT), 1_100_000_000, Evidence::bid(1_122_000_000, N), Evidence::bid(1_121_999_999, N)),
    ];
    for (k, (g, quote, yes, no)) in cases.into_iter().enumerate() {
        let w = pend(&g, quote);
        accept(&ch_draft(&[w.clone()], &yes, &fk(), 10 + k as u32));
        refused(&ch_draft(&[w], &no, &fk(), 20 + k as u32), 0);
    }
}

/// A cure moves the bound exactly to cureBound: the proposed quote no longer triggers the cured part, one sompi below
/// it does (the bound is tight, not loose in either direction).
#[test]
fn holds_cure_bound_is_tight() {
    let l = lv(0x56, 1_000);
    let w = pend(&liq(&l, N, LONG), 940_000_000);
    let cd = cure_draft(&w, &lk(), &fk(), 1);
    let ctx = accept(&cd);
    let cured = live_pairs(&ctx, &cd).remove(0).at(1_000);
    refused(&ev_draft(&[liq(&cured, N, LONG)], &Evidence::ask(940_000_000, N), &fk(), 2), 0);
    accept(&ev_draft(&[liq(&cured, N, LONG)], &Evidence::ask(939_999_999, N), &fk(), 3));
}
