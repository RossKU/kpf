//! KpfPair evidence paths (DESIGN 3.3, 3.6, 4.4, 4.5 T4, T6, T7): P5 unilateral close, P6 liquidation (close
//! both), P7 liquidation by transfer, and the evidence block.
//!
//! Every transaction here carries a real KOB evidence order (KobAsk / KobBid compiled from
//! `vendor/kob/contracts/orders`) and its counterparty on KOB's reference KCC-20 program, so the accepted shapes are
//! valid end to end: every input, the KOB order and the token included, passes its own script. A negative case
//! mutates one thing and asserts that the pair input is refused.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::{Transaction, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf::evidence::*;
use kpf::kob::*;
use kpf::paths::*;
use kpf::*;
use secp256k1::Keypair;

fn accept_tx(tx: &Transaction, e: &[UtxoEntry]) {
    assert_accepted(tx, e);
    let ins: u128 = e.iter().map(|u| u.amount as u128).sum();
    let outs: u128 = tx.outputs.iter().map(|o| o.value as u128).sum();
    assert!(outs <= ins, "outputs {outs} > inputs {ins}");
}

fn ok(d: &Draft) {
    let (tx, e) = d.build();
    accept_tx(&tx, &e);
}

fn bad(d: &Draft, i: usize) {
    let (tx, e) = d.build();
    assert_input_fails(&tx, &e, i);
}

fn f() -> Keypair {
    keypair(14)
}

fn lk() -> Keypair {
    keypair(11)
}

fn sk() -> Keypair {
    keypair(12)
}

fn base() -> Pair {
    pair(&lk(), &sk())
}

const N: i64 = 1_000_000_000; // the whole pair: 1,000 tokens
const HALF: i64 = 400_000_000;
// The base pair (bl 9, bs 12 KAS, mmr 5 %): a long is liquidatable at an ask <= 947,368,421, a short at a bid
// >= 1,142,857,143 (DESIGN 5 worked example).
const LIQ_ASK: i64 = 940_000_000;
const LIQ_BID: i64 = 1_150_000_000;

fn close(l: &Live, n: i64, side: i64) -> PairLeg {
    let signer = if side == LONG { lk() } else { sk() };
    PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side, signer } }
}

fn liq(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side } }
}

fn entrant(side: i64, tag: u8) -> LiveOpen {
    let o = if side == LONG { long_order(&keypair(21)) } else { short_order(&keypair(22)) };
    LiveOpen { value: o.escrow() as u64, o, id: Hash::from_bytes([tag; 32]), op: outpoint(tag) }
}

fn liq_t(l: &Live, n: i64, side: i64, x: i64, k: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::LiqTransfer { side, x, open: entrant(side, 0x71), k } }
}

fn d1(g: PairLeg, e: &Evidence) -> Draft {
    ev_draft(&[g], e, &f(), 1)
}

fn ask(p: i64, fill: i64) -> Evidence {
    Evidence::ask(p, fill)
}

fn bid(p: i64, fill: i64) -> Evidence {
    Evidence::bid(p, fill)
}

/// The outputs of the draft's built transaction.
fn outs_len(d: &Draft) -> usize {
    d.outs.len()
}

// ---------------------------------------------------------------- the KOB templates

#[test]
fn kob_templates_match_kobs_own_pins() {
    let (h, pre, suf) = tpl_facts(ask_base());
    assert_eq!((h, pre, suf), (KOB_ASK_TPL, KOB_ASK_PRE, KOB_ASK_SUF), "KobAsk");
    let (h, pre, suf) = tpl_facts(bid_base());
    assert_eq!((h, pre, suf), (KOB_BID_TPL, KOB_BID_PRE, KOB_BID_SUF), "KobBid");
    for (art, len) in [(ask_base(), 276), (bid_base(), 285), (kcc20_base(), 112)] {
        assert_eq!(art.contracts.values().next().unwrap().compiled.state_span.len, len);
    }
    // The look-alike ask has KobAsk's layout but not its template.
    let (h, pre, suf) = tpl_facts(fake_ask_base());
    assert_ne!(h, KOB_ASK_TPL);
    assert_eq!((pre, suf), (KOB_ASK_PRE, KOB_ASK_SUF));
    // Spliced instances equal compiled ones.
    let mut a = ask_of(&market(), [9; 32], 123, 456);
    a.interval = -3;
    assert_eq!(bytecode(&a.art()), bytecode(&compile_src("KobAsk", &kob_source("orders/KobAsk.sil"), &a.ctor())));
    let b = bid_of(&market(), [9; 32], 789);
    assert_eq!(bytecode(&b.art()), bytecode(&compile_src("KobBid", &kob_source("orders/KobBid.sil"), &b.ctor())));
    let t = Tok::p2pk(5, [8; 32], market().ext);
    assert_eq!(bytecode(&t.art()), bytecode(&compile_src("KCC20Ref", &kob_source("kcc20/KCC20Ref.sil"), &t.ctor())));
}

// ---------------------------------------------------------------- end to end, one per path

#[test]
fn p5_long_closes_at_a_bid_end_to_end() {
    let l = live(&base(), 0x31);
    for n in [N, HALF] {
        let d = d1(close(&l, n, LONG), &bid(1_050_000_000, n));
        ok(&d);
        // The long gets exactly q(n, b - bl), the short the rest of the part (and with n = s carrier and excess).
        let lp = q(n, 1_050_000_000 - l.p.bl, SCALE, 0) as u64;
        assert_eq!(d.outs[0].value, lp);
        let sp = if n == N { l.value - lp } else { (l.p.range_of(n) as u64) - lp };
        assert_eq!(d.outs[outs_len(&d) - 1].value, sp);
    }
    // Outside the range the price is clamped: a bid below bl pays the long nothing.
    ok(&d1(close(&l, N, LONG), &bid(500_000_000, N)));
}

#[test]
fn p5_short_closes_at_an_ask_end_to_end() {
    let l = live(&base(), 0x32);
    for n in [N, HALF] {
        let d = d1(close(&l, n, SHORT), &ask(1_100_000_000, n));
        ok(&d);
        let sp = q(n, l.p.bs - 1_100_000_000, SCALE, 0) as u64;
        assert_eq!(d.outs[outs_len(&d) - 1].value, sp);
    }
    ok(&d1(close(&l, N, SHORT), &ask(1_500_000_000, N)));
}

#[test]
fn p6_liquidates_a_long_at_an_ask_end_to_end() {
    let l = live(&base(), 0x33);
    for n in [N, HALF] {
        ok(&d1(liq(&l, n, LONG), &ask(LIQ_ASK, n)));
    }
    // In pieces: half now, then the remainder on its continuation.
    let d = d1(liq(&l, 500_000_000, LONG), &ask(LIQ_ASK, 500_000_000));
    let (tx, e) = d.build();
    accept_tx(&tx, &e);
    let rest = live_pairs(&tx, &d).remove(0);
    assert_eq!(rest.p.size, 500_000_000);
    ok(&d1(liq(&rest, 500_000_000, LONG), &ask(LIQ_ASK, 500_000_000)));
}

#[test]
fn p6_liquidates_a_short_at_a_bid_end_to_end() {
    let l = live(&base(), 0x34);
    for n in [N, HALF] {
        ok(&d1(liq(&l, n, SHORT), &bid(LIQ_BID, n)));
    }
}

#[test]
fn p7_liquidates_by_transfer_end_to_end() {
    let l = live(&base(), 0x35);
    for (n, k) in [(N, 0), (HALF, 0), (HALF, 1)] {
        ok(&d1(liq_t(&l, n, LONG, 950_000_000, k), &ask(LIQ_ASK, n)));
        ok(&d1(liq_t(&l, n, SHORT, 1_140_000_000, k), &bid(LIQ_BID, n)));
    }
}

#[test]
fn p7_winner_keeps_its_position_and_is_paid_at_x_less_fee() {
    // Premise changed by review 2 (DESIGN 4.5b): the entrant's payment less the fee goes to the winner, not the
    // loser.
    let l = live(&base(), 0x36);
    let x = 950_000_000;
    let d = d1(liq_t(&l, N, LONG, x, 0), &ask(LIQ_ASK, N));
    ok(&d);
    let fee = liq_fee(&l.p, N, LIQ_ASK, LONG);
    assert_eq!(d.outs[0].value as i64, q(N, x - l.p.bl, SCALE, 0) - fee);
    assert_eq!(d.outs[0].script_public_key, p2pk_x(&l.p.short_key));
    // Paying the loser instead is refused.
    let mut to_loser = d.clone();
    to_loser.outs[0].script_public_key = p2pk_x(&l.p.long_key);
    bad(&to_loser, 0);
    let part = &d.pairs[0].1;
    // The winner keeps its key and bound; its tag is cleared (DESIGN 4.4) and the entry is kept (DESIGN 4.5a).
    assert_eq!((part.bs, part.short_key, part.tag_s, part.entry), (l.p.bs, l.p.short_key, ZERO, l.p.entry));
    assert!(part.bl <= l.p.bl);
}

// ---------------------------------------------------------------- evidence identity

/// The honest P5 drafts: the short at an ask (with custody) and the long at a bid.
fn p5_ask(e: Evidence) -> Draft {
    d1(close(&live(&base(), 0x37), N, SHORT), &e)
}

fn p5_bid(e: Evidence) -> Draft {
    d1(close(&live(&base(), 0x38), N, LONG), &e)
}

const A: i64 = 1_100_000_000;
const B: i64 = 1_050_000_000;

#[test]
fn evidence_controls_pass() {
    ok(&p5_ask(ask(A, N)));
    ok(&p5_bid(bid(B, N)));
}

#[test]
fn evidence_of_another_token_is_refused() {
    let other = Hash::from_bytes([0x77; 32]);
    // The ask quotes another token; its custody is a token of that id (the KOB side is valid).
    let mut e = ask(A, N);
    e.ask.token_cov_id = other;
    e.tok_cov = other;
    bad(&p5_ask(e.clone()), 0);
    // Only the custody is of another token.
    let mut e = ask(A, N);
    e.tok_cov = other;
    bad(&p5_ask(e), 0);
    // Only the ask's state names another token.
    let mut e = ask(A, N);
    e.ask.token_cov_id = other;
    bad(&p5_ask(e), 0);
    let mut e = bid(B, N);
    e.bid.token_cov_id = other;
    e.tok_cov = other;
    bad(&p5_bid(e), 0);
}

#[test]
fn evidence_of_another_template_is_refused() {
    // A look-alike ask: KobAsk's source with one constant changed. It runs as an ask (its own input passes).
    let mut e = ask(A, N);
    e.art = Some(fake_ask_base().clone());
    let d = p5_ask(e);
    let (tx, en) = d.build();
    let r = run(&tx, &en).unwrap();
    assert!(r[1].is_ok() && r[2].is_ok(), "the look-alike ask and its custody pass their own scripts");
    assert_input_refused(&tx, &en, 0);
    // A token program other than the market's for the custody: the pair reads it with the wrong template hash.
    let mut m = base();
    m.m.token_tpl_hash = Hash::from_bytes([0x55; 32]);
    bad(&d1(close(&live(&m, 0x39), N, SHORT), &ask(A, N)), 0);
}

#[test]
fn evidence_with_another_extension_commitment_is_refused() {
    let other = Hash::from_bytes([0x66; 32]);
    let mut e = ask(A, N);
    e.ask.ext = other;
    e.tok_ext = other;
    bad(&p5_ask(e), 0);
    let mut e = bid(B, N);
    e.bid.ext = other;
    e.tok_ext = other;
    bad(&p5_bid(e), 0);
}

#[test]
fn evidence_at_another_scale_is_refused() {
    let mut e = ask(A, N);
    e.ask.scale = SCALE * 10;
    bad(&p5_ask(e), 0);
    let mut e = bid(B, N);
    e.bid.scale = SCALE * 10;
    bad(&p5_bid(e), 0);
}

#[test]
fn decaying_or_negative_interval_evidence_is_refused() {
    let mut e = ask(A, N);
    (e.ask.slope, e.ask.price_end, e.ask.decay_step) = (1, A / 2, 1);
    let d = p5_ask(e);
    let (tx, en) = d.build();
    assert!(run(&tx, &en).unwrap()[1].is_ok(), "the decaying ask itself fills");
    assert_input_refused(&tx, &en, 0);
    let mut e = bid(B, N);
    (e.bid.slope, e.bid.price_end, e.bid.decay_step) = (1, B, 1);
    bad(&p5_bid(e), 0);
    for iv in [-1, -1_000_000_000_000] {
        let mut e = ask(A, N);
        e.ask.interval = iv;
        bad(&p5_ask(e), 0);
        let mut e = bid(B, N);
        e.bid.interval = iv;
        bad(&p5_bid(e), 0);
    }
}

#[test]
fn rest_time_one_daa_short_is_refused() {
    let r = rest_daa();
    // Exactly R before the lock time: accepted (order and custody).
    let mut e = ask(A, N);
    (e.daa, e.tok_daa) = (NOW - r, NOW - r);
    ok(&p5_ask(e.clone()));
    // The order one DAA short.
    let mut e1 = e.clone();
    e1.daa = NOW - r + 1;
    bad(&p5_ask(e1), 0);
    // The custody one DAA short.
    let mut e2 = e.clone();
    e2.tok_daa = NOW - r + 1;
    bad(&p5_ask(e2), 0);
    // activeFrom one DAA short (the quote was not open before it).
    let mut e3 = e.clone();
    e3.ask.active_from = (NOW - r + 1) as i64;
    bad(&p5_ask(e3), 0);
    // A bid: the order alone.
    let mut b = bid(B, N);
    b.daa = NOW - r;
    ok(&p5_bid(b.clone()));
    b.daa = NOW - r + 1;
    bad(&p5_bid(b), 0);
}

#[test]
fn lock_time_that_does_not_prove_the_rest_is_refused() {
    let r = rest_daa();
    let mut e = ask(A, N);
    (e.daa, e.tok_daa) = (NOW - r - 50, NOW - r - 50);
    let mut d = p5_ask(e);
    d.lock = NOW - 50;
    ok(&d);
    d.lock = NOW - 51;
    bad(&d, 0);
}

#[test]
fn custody_not_owned_by_the_ask_is_refused() {
    let mut e = ask(A, N);
    e.custody_owner = Some(Hash::from_bytes([0x44; 32]));
    bad(&p5_ask(e), 0);
}

#[test]
fn evidence_without_a_fill_is_refused() {
    // n = 0 in the evidence's first push (KobAsk's refund) is not a fill.
    let mut d = p5_ask(ask(A, N));
    let mut a = d.args(1);
    a[0] = bv(&sm8(0));
    d.set_args(1, a);
    bad(&d, 0);
}

// ---------------------------------------------------------------- the evidence block

#[test]
fn fill_below_k_times_the_block_is_refused() {
    let l = live(&base(), 0x3a);
    ok(&d1(liq(&l, N, LONG), &ask(LIQ_ASK, N)));
    bad(&d1(liq(&l, N, LONG), &ask(LIQ_ASK, N - 1)), 0);
    // k = 1.5 and an odd part: n_ev >= ceil(1.5 * 1,000,000,001) = 1,500,000,002.
    let mut p = base();
    p.m.k_bps = 15_000;
    p.size = 2_000_000_000;
    let l = live(&p, 0x3b);
    let n = 1_000_000_001;
    ok(&d1(close(&l, n, SHORT), &ask(A, 1_500_000_002)));
    bad(&d1(close(&l, n, SHORT), &ask(A, 1_500_000_001)), 0);
    // Two pairs on one fill: the fill must cover both.
    let (a, b) = (live(&base(), 0x3c), live(&base(), 0x3d));
    let block = [liq(&a, N, LONG), liq(&b, HALF, LONG)];
    ok(&ev_draft(&block, &ask(LIQ_ASK, N + HALF), &f(), 1));
    let d = ev_draft(&block, &ask(LIQ_ASK, N + HALF - 1), &f(), 1);
    bad(&d, 0);
    bad(&d, 1);
}

#[test]
fn k_out_of_bounds_is_refused() {
    for k in [0, 100_001] {
        let mut p = base();
        p.m.k_bps = k;
        bad(&d1(close(&live(&p, 0x3e), N, SHORT), &ask(A, 10 * N)), 0);
    }
}

#[test]
fn pair_outside_the_block_is_refused() {
    let (a, b) = (live(&base(), 0x41), live(&base(), 0x42));
    // b names the block [1, 2) and is covered alone; a names [0, 2) and reads b's other q.
    let mut gb = liq(&b, N, LONG);
    gb.q = 1;
    let d = ev_draft(&[liq(&a, N, LONG), gb], &ask(LIQ_ASK, N), &f(), 1);
    let (tx, e) = d.build();
    let r = run(&tx, &e).unwrap();
    assert!(r[0].is_err(), "a pair outside the block of the q it reads is refused");
    assert!(r[1].is_ok(), "the block [1, 2) alone is covered");
    // A pair after the evidence naming it.
    let mut g = liq(&b, N, LONG);
    g.q = 0;
    let d = ev_draft_at(&[liq(&a, N, LONG), g], 1, &ask(LIQ_ASK, 2 * N), &f(), 1);
    let (tx, e) = d.build();
    let r = run(&tx, &e).unwrap();
    assert!(r[0].is_ok(), "the block [0, 1) is covered");
    assert!(r[3].is_err(), "a pair after the evidence is refused");
}

#[test]
fn mismatched_q_is_refused() {
    let (a, b) = (live(&base(), 0x43), live(&base(), 0x44));
    ok(&ev_draft(&[liq(&a, N, LONG), liq(&b, N, LONG)], &ask(LIQ_ASK, 2 * N), &f(), 1));
    // b names q = 0 but a names q = 1 (a is not in its own block [1, 2)).
    let mut ga = liq(&a, N, LONG);
    ga.q = 1;
    let d = ev_draft(&[ga, liq(&b, N, LONG)], &ask(LIQ_ASK, 2 * N), &f(), 1);
    bad(&d, 0);
    bad(&d, 1);
    // q beyond the evidence and negative q.
    for qq in [2, 3, -1] {
        let mut g = liq(&a, N, LONG);
        g.q = qq;
        bad(&d1(g, &ask(LIQ_ASK, N)), 0);
    }
}

#[test]
fn zero_or_negative_n_is_refused() {
    let (a, b) = (live(&base(), 0x45), live(&base(), 0x46));
    for n in [0, -1, -N] {
        bad(&d1(liq(&a, n, LONG), &ask(LIQ_ASK, N)), 0);
    }
    // A block member pushing a negative n (it fails itself) must not lower the sum of the others.
    let d = ev_draft(&[liq(&a, N, LONG), liq(&b, -N, LONG)], &ask(LIQ_ASK, N), &f(), 1);
    bad(&d, 0);
}

#[test]
fn non_pair_input_in_the_block_is_refused_unless_it_pushes_n_and_q() {
    let (a, b) = (live(&base(), 0x22), live(&base(), 0x23));
    let d = ev_draft(&[liq(&a, N, LONG), liq(&b, N, LONG)], &ask(LIQ_ASK, 2 * N), &f(), 1);
    ok(&d);
    // Input 1 becomes a plain key input (its signature script is one 65-byte push): the block reader refuses.
    let mut x = d.clone();
    x.ins[1] = p2pk_in(outpoint(0x90), 1_000_000, None, &keypair(15));
    bad(&x, 0);
    // Control: the same plain input just before the block (pair 1 names [1, 2)) does not concern it.
    let mut gb = liq(&b, N, LONG);
    gb.q = 1;
    let mut x = ev_draft(&[liq(&a, N, LONG), gb], &ask(LIQ_ASK, N), &f(), 1);
    x.ins[0] = p2pk_in(outpoint(0x91), 1_000_000, None, &keypair(15));
    let (tx, e) = x.build();
    assert!(run(&tx, &e).unwrap()[1].is_ok());
}

fn block_of(k: usize, base_tag: u8) -> Vec<PairLeg> {
    (0..k).map(|i| liq(&live(&base(), base_tag + i as u8), N, LONG)).collect()
}

#[test]
fn eight_liquidations_on_one_evidence_end_to_end() {
    let legs = block_of(8, 0x50);
    ok(&ev_draft(&legs, &ask(LIQ_ASK, 8 * N), &f(), 1));
    let d = ev_draft(&legs, &ask(LIQ_ASK, 8 * N - 1), &f(), 1);
    let (tx, e) = d.build();
    let r = run(&tx, &e).unwrap();
    assert!((0..8).all(|i| r[i].is_err()), "a fill one unit short refuses the whole block");
}

#[test]
fn nine_pairs_in_a_block_are_refused() {
    let legs = block_of(9, 0x60);
    let d = ev_draft(&legs, &ask(LIQ_ASK, 9 * N), &f(), 1);
    let (tx, e) = d.build();
    let r = run(&tx, &e).unwrap();
    assert!((0..9).all(|i| r[i].is_err()), "a block of nine is refused");
}

/// One print cannot act on more size than it covers, whatever the arrangement: three pairs of sizes 1,000, 600
/// and 400 tokens, before or around the evidence, each naming any block start; whichever pairs pass, their
/// sizes sum to at most n_ev (k = 1).
#[test]
fn one_print_never_covers_more_than_its_fill() {
    let sizes = [N, 600_000_000, 400_000_000];
    let pairs: Vec<Live> = sizes.iter().enumerate().map(|(i, &s)| live(&base().with_size(s), 0x80 + i as u8)).collect();
    let mut passing_shapes = 0;
    for ev_at in [3usize, 2, 1, 0] {
        for fill in [N, 600_000_000 + 400_000_000 - 1, 2 * N] {
            for code in 0..(6 * 6 * 6) {
                let qs = [code % 6, (code / 6) % 6, code / 36];
                let legs: Vec<PairLeg> = (0..3)
                    .map(|i| {
                        let mut g = liq(&pairs[i], sizes[i], LONG);
                        g.q = qs[i] as i64;
                        g
                    })
                    .collect();
                let d = ev_draft_at(&legs, ev_at, &ask(LIQ_ASK, fill), &f(), 1);
                let (tx, e) = d.build();
                let r = run(&tx, &e).unwrap();
                let at = |j: usize| if j < ev_at { j } else { j + 2 };
                let acted: i64 = (0..3).filter(|&j| r[at(j)].is_ok()).map(|j| sizes[j]).sum();
                assert!(acted <= fill, "ev at {ev_at}, q {qs:?}, fill {fill}: pairs of {acted} acted");
                if acted > 0 {
                    passing_shapes += 1;
                }
            }
        }
    }
    assert!(passing_shapes > 0, "no arrangement passed: the test is vacuous");
}

// ---------------------------------------------------------------- wrong bound

#[test]
fn wrong_bound_is_refused() {
    let l = live(&base(), 0x47);
    // P5: the long needs a bid, the short an ask.
    bad(&d1(close(&l, N, LONG), &ask(B, N)), 0);
    bad(&d1(close(&l, N, SHORT), &bid(A, N)), 0);
    // P6 / P7: a long is liquidated on an ask, a short on a bid.
    bad(&d1(liq(&l, N, LONG), &bid(LIQ_ASK, N)), 0);
    bad(&d1(liq(&l, N, SHORT), &ask(LIQ_BID, N)), 0);
    bad(&d1(liq_t(&l, N, LONG, 950_000_000, 0), &bid(LIQ_ASK, N)), 0);
    bad(&d1(liq_t(&l, N, SHORT, 1_140_000_000, 0), &ask(LIQ_BID, N)), 0);
}

#[test]
fn unknown_path_or_a_liquidation_posing_as_p5_is_refused() {
    let l = live(&base(), 0x24);
    let d = d1(liq(&l, N, LONG), &ask(LIQ_ASK, N));
    ok(&d);
    for path in [0, 4, 8, -6] {
        let mut x = d.clone();
        let mut a = x.args(0);
        a[ARG_PATH] = iv(path);
        x.set_args(0, a);
        bad(&x, 0);
    }
    // The same transaction as path 5 needs the owner's signature.
    let mut x = d.clone();
    let mut a = x.args(0);
    a[ARG_PATH] = iv(5);
    x.set_args(0, a);
    bad(&x, 0);
    // P7 arguments with path 6 settle as P6: the outputs of P7 do not satisfy it.
    let d = d1(liq_t(&l, N, LONG, 950_000_000, 0), &ask(LIQ_ASK, N));
    ok(&d);
    let mut x = d.clone();
    let mut a = x.args(0);
    a[ARG_PATH] = iv(6);
    x.set_args(0, a);
    bad(&x, 0);
}

// ---------------------------------------------------------------- P5

#[test]
fn p5_by_a_non_owner_is_refused() {
    let l = live(&base(), 0x48);
    for (side, signer, e) in [(LONG, sk(), bid(B, N)), (LONG, f(), bid(B, N)), (SHORT, lk(), ask(A, N)), (SHORT, keypair(13), ask(A, N))] {
        bad(&d1(PairLeg { l: l.clone(), n: N, q: 0, act: Act::Close { side, signer } }, &e), 0);
    }
    // A side code other than 1 or 2.
    bad(&d1(PairLeg { l: l.clone(), n: N, q: 0, act: Act::Close { side: 3, signer: lk() } }, &bid(B, N)), 0);
}

#[test]
fn p5_payout_one_sompi_short_is_refused() {
    let l = live(&base(), 0x49);
    for (side, e) in [(LONG, bid(B, N)), (SHORT, ask(A, N))] {
        for n in [N, HALF] {
            let d = d1(close(&l, n, side), &e.clone());
            let m = outs_len(&d);
            for j in [0, m - 1] {
                let mut x = d.clone();
                x.outs[j].value -= 1;
                bad(&x, 0);
            }
            if n < N {
                // The remainder one sompi below what it must keep.
                let r = d.pairs[0].0;
                let mut x = d.clone();
                x.outs[r].value -= 1;
                bad(&x, 0);
            }
        }
    }
}

#[test]
fn p5_remainder_must_keep_every_field() {
    let l = live(&base(), 0x4a);
    let d = d1(close(&l, HALF, LONG), &bid(B, HALF));
    let r = d.pairs[0].0;
    let mut x = d.clone();
    let mut p = d.pairs[0].1.clone();
    p.short_key = xonly(&keypair(13));
    x.set_pair(r, &p);
    bad(&x, 0);
}

// ---------------------------------------------------------------- P6

fn long_trigger(p: &Pair) -> i64 {
    (p.bl * 10_000 - 1) / (10_000 - p.m.mmr_bps)
}

fn short_trigger(p: &Pair) -> i64 {
    p.bs * 10_000 / (10_000 + p.m.mmr_bps) + 1
}

#[test]
fn p6_trigger_exactly_at_and_one_tick_before() {
    let l = live(&base(), 0x4b);
    let a = long_trigger(&l.p);
    assert_eq!(a, 947_368_421);
    ok(&d1(liq(&l, N, LONG), &ask(a, N)));
    bad(&d1(liq(&l, N, LONG), &ask(a + 1, N)), 0);
    let b = short_trigger(&l.p);
    assert_eq!(b, 1_142_857_143);
    ok(&d1(liq(&l, N, SHORT), &bid(b, N)));
    bad(&d1(liq(&l, N, SHORT), &bid(b - 1, N)), 0);
    // P7 shares the trigger.
    ok(&d1(liq_t(&l, N, LONG, a, 0), &ask(a, N)));
    bad(&d1(liq_t(&l, N, LONG, a + 1, 0), &ask(a + 1, N)), 0);
    ok(&d1(liq_t(&l, N, SHORT, 1_140_000_000, 0), &bid(b, N)));
    bad(&d1(liq_t(&l, N, SHORT, 1_140_000_000, 0), &bid(b - 1, N)), 0);
}

#[test]
fn p6_out_of_range_parameters_are_refused() {
    let mut p = base();
    p.m.mmr_bps = 5_000;
    bad(&d1(liq(&live(&p, 0x4c), N, LONG), &ask(100_000_000, N)), 0);
    let mut p = base();
    p.m.liq_fee_bps = 10_001;
    bad(&d1(liq(&live(&p, 0x4c), N, LONG), &ask(LIQ_ASK, N)), 0);
    let mut p = base();
    p.m.keeper_fee_min = -1;
    bad(&d1(liq(&live(&p, 0x4c), N, LONG), &ask(LIQ_ASK, N)), 0);
}

/// Premise changed by review 2 (F1, DESIGN 4.5b): the loser of P6 receives 0 and the winner the whole liquidated
/// value less the fee, the fee capped at the maintenance slice. Before, the winner was owed its distance to the
/// quote and the loser took the rest less the fee.
#[test]
fn p6_fee_above_the_formula_is_refused() {
    let l = live(&base(), 0x4d);
    for n in [N, HALF] {
        let avail = if n == N { l.value as i64 } else { l.p.range_of(n) };
        // Long liquidated: the winner (short) is at M - 1, the loser at output 0 gets nothing.
        let d = d1(liq(&l, n, LONG), &ask(LIQ_ASK, n));
        ok(&d);
        let m = outs_len(&d);
        assert_eq!(d.outs[0].value, 0);
        assert_eq!(d.outs[m - 1].value as i64, avail - liq_fee(&l.p, n, LIQ_ASK, LONG));
        let mut x = d.clone();
        x.outs[m - 1].value -= 1;
        bad(&x, 0);
        // Short liquidated: the winner (long) is at output 0.
        let d = d1(liq(&l, n, SHORT), &bid(LIQ_BID, n));
        ok(&d);
        let m = outs_len(&d);
        assert_eq!(d.outs[m - 1].value, 0);
        assert_eq!(d.outs[0].value as i64, avail - liq_fee(&l.p, n, LIQ_BID, SHORT));
        let mut x = d.clone();
        x.outs[0].value -= 1;
        bad(&x, 0);
    }
}

/// Premise changed by review 2: one sompi moved from the winner to the loser is refused (the loser is owed 0).
#[test]
fn p6_value_moved_from_the_winner_to_the_loser_is_refused() {
    let l = live(&base(), 0x4e);
    let d = d1(liq(&l, N, LONG), &ask(LIQ_ASK, N));
    let m = outs_len(&d);
    let mut x = d.clone();
    x.outs[m - 1].value -= 1;
    x.outs[0].value += 1;
    bad(&x, 0);
    let d = d1(liq(&l, N, SHORT), &bid(LIQ_BID, N));
    let mut x = d.clone();
    x.outs[0].value -= 1;
    x.outs[m - 1].value += 1;
    bad(&x, 0);
}

/// Premise changed by review 2 (DESIGN 4.5b; review 1 F5 had capped the fee at the loser's residual, so a bankrupt
/// piece paid none). A bankrupt piece now pays the fee out of the part like any other, and the winner still
/// receives at least its payoff at the trigger price.
#[test]
fn p6_bankrupt_piece_pays_the_capped_fee() {
    let l = live(&base(), 0x4f);
    let a = 500_000_000;
    let d = d1(liq(&l, HALF, LONG), &ask(a, HALF));
    ok(&d);
    let m = outs_len(&d);
    let fee = liq_fee(&l.p, HALF, a, LONG);
    assert_eq!(fee, fee_of(&l.p.m, HALF, a));
    assert_eq!(d.outs[0].value, 0);
    assert_eq!(d.outs[m - 1].value as i64, l.p.range_of(HALF) - fee);
    assert!(d.outs[m - 1].value as i64 >= winner_at_trigger(&l.p, HALF, LONG));
    let mut x = d.clone();
    x.outs[m - 1].value -= 1;
    bad(&x, 0);
}

#[test]
fn p6_keeper_fee_floor_applies_up_to_the_maintenance_slice() {
    let mut p = base();
    p.m.keeper_fee_min = 10_000_000_000; // 100 KAS, above the rate fee of 47 KAS, below the slice of 473.68 KAS
    let l = live(&p, 0x2a);
    let d = d1(liq(&l, N, LONG), &ask(LIQ_ASK, N));
    ok(&d);
    let m = outs_len(&d);
    assert_eq!(d.outs[m - 1].value as i64, l.value as i64 - p.m.keeper_fee_min);
    // A floor above the slice is capped at the slice q(n, T - bl), T = floor(bl * 10^4 / 9500) = 947,368,421.
    p.m.keeper_fee_min = 100_000_000_000; // 1,000 KAS
    let l = live(&p, 0x2a);
    let d = d1(liq(&l, N, LONG), &ask(LIQ_ASK, N));
    ok(&d);
    let slice = q(N, 947_368_421 - p.bl, SCALE, 0);
    assert_eq!(slice_of(&p, N, LONG), slice);
    assert_eq!(d.outs[m - 1].value as i64, l.value as i64 - slice);
    let mut x = d.clone();
    x.outs[m - 1].value -= 1;
    bad(&x, 0);
}

// ---------------------------------------------------------------- P7

#[test]
fn p7_downgrade_is_refused() {
    let l = live(&base(), 0x2b);
    // A long entrant whose bound x - myRate is above bl.
    let mut g = liq_t(&l, N, LONG, 950_000_000, 0);
    if let Act::LiqTransfer { open, .. } = &mut g.act {
        open.o.my_rate = 40_000_000; // bl' = 910,000,000 > 900,000,000
        open.value = open.o.escrow() as u64;
    }
    bad(&d1(g, &ask(LIQ_ASK, N)), 0);
    // A short entrant whose bound x + myRate is below bs.
    let mut g = liq_t(&l, N, SHORT, 1_140_000_000, 0);
    if let Act::LiqTransfer { open, .. } = &mut g.act {
        open.o.my_rate = 50_000_000; // bs' = 1,190,000,000 < 1,200,000,000
        open.o.min_counter_rate = 0;
        open.value = open.o.escrow() as u64;
    }
    bad(&d1(g, &bid(LIQ_BID, N)), 0);
}

#[test]
fn p7_price_on_the_wrong_side_of_the_quote_is_refused() {
    let l = live(&base(), 0x2c);
    ok(&d1(liq_t(&l, N, LONG, LIQ_ASK, 0), &ask(LIQ_ASK, N)));
    bad(&d1(liq_t(&l, N, LONG, LIQ_ASK - 1, 0), &ask(LIQ_ASK, N)), 0);
    ok(&d1(liq_t(&l, N, SHORT, LIQ_BID, 0), &bid(LIQ_BID, N)));
    bad(&d1(liq_t(&l, N, SHORT, LIQ_BID + 1, 0), &bid(LIQ_BID, N)), 0);
}

#[test]
fn p7_entrant_whose_bound_does_not_match_its_rate_is_refused_by_the_entrant() {
    let l = live(&base(), 0x2d);
    let d = d1(liq_t(&l, N, LONG, 950_000_000, 0), &ask(LIQ_ASK, N));
    ok(&d);
    // The pair is told a lower bound (still no downgrade for the short) than the entrant's own rate gives.
    let mut x = d.clone();
    let mut a = x.args(0);
    a[ARG_NBD] = iv(800_000_000);
    x.set_args(0, a);
    let pi = x.pairs[0].0;
    let mut part = x.pairs[0].1.clone();
    part.bl = 800_000_000;
    x.set_pair(pi, &part);
    x.outs[pi].value = part.floor() as u64;
    let (tx, e) = x.build();
    let r = run(&tx, &e).unwrap();
    assert!(r[0].is_ok(), "the pair accepts any non-downgrading bound");
    assert!(r[3].is_err(), "the KpfOpen refuses a bound other than x - myRate");
}

#[test]
fn p7_winner_payout_one_sompi_short_and_stayer_changes_are_refused() {
    let l = live(&base(), 0x2e);
    for (side, x, e) in [(LONG, 950_000_000, ask(LIQ_ASK, N)), (SHORT, 1_140_000_000, bid(LIQ_BID, N))] {
        let d = d1(liq_t(&l, HALF, side, x, 0), &e);
        ok(&d);
        let mut y = d.clone();
        y.outs[0].value -= 1;
        bad(&y, 0);
        // The stayer's key in the part changed.
        let (pi, mut part) = d.pairs.iter().find(|(_, p)| p.size == HALF).cloned().unwrap();
        if side == LONG {
            part.short_key = xonly(&keypair(13));
        } else {
            part.long_key = xonly(&keypair(13));
        }
        let mut y = d.clone();
        y.set_pair(pi, &part);
        bad(&y, 0);
    }
}

// ---------------------------------------------------------------- guards on every new entry

fn tagged(tag_l: Hash) -> Live {
    let mut p = base();
    p.tag_l = tag_l;
    live(&p, 0x2f)
}

/// Premise changed by review 1 (F1, F2, DESIGN 4.4): the live-tag guard is gone, so a tag that names an input
/// of the transaction (the evidence order, the token, the entrant, the pair itself) no longer blocks any evidence
/// path; instead every continuation clears the tags (asserted by the builders' expected outputs), and one that
/// copies the old tag is refused.
#[test]
fn tags_never_block_an_evidence_entry() {
    let e_a = ask(A, N);
    ok(&d1(close(&tagged(e_a.id), N, SHORT), &e_a));
    let e_b = bid(B, N);
    ok(&d1(close(&tagged(e_b.id), N, LONG), &e_b));
    let e = ask(LIQ_ASK, N);
    ok(&d1(liq(&tagged(e.id), N, LONG), &e));
    ok(&d1(liq(&tagged(market().token_cov_id), N, LONG), &ask(LIQ_ASK, N)));
    let own = tagged(ZERO);
    ok(&d1(liq(&tagged(own.id), N, LONG), &ask(LIQ_ASK, N)));
    // P7 with the tag naming the entrant itself.
    ok(&d1(liq_t(&tagged(Hash::from_bytes([0x71; 32])), N, LONG, 950_000_000, 0), &ask(LIQ_ASK, N)));
    // A remainder (P5, P6) or the winner's side of a P7 part that copies the old tag is refused.
    let l = tagged(e.id);
    for g in [close(&l, HALF, SHORT), liq(&l, HALF, LONG), liq_t(&l, HALF, LONG, 950_000_000, 0)] {
        let d = d1(g, &ask(LIQ_ASK, HALF));
        ok(&d);
        for (pi, p) in d.pairs.clone() {
            let mut y = d.clone();
            let mut c = p.clone();
            if c.long_key == l.p.long_key {
                c.tag_l = l.p.tag_l;
            } else {
                c.tag_s = l.p.tag_s;
            }
            y.set_pair(pi, &c);
            bad(&y, 0);
        }
    }
}

#[test]
fn single_input_guard_on_every_evidence_entry() {
    // Two inputs of one pair covenant id in one block.
    let a = live(&base(), 0x26);
    let mut b = live(&base(), 0x27);
    b.id = a.id;
    let cases: Vec<(Vec<PairLeg>, Evidence)> = vec![
        (vec![close(&a, N, SHORT), close(&b, N, SHORT)], ask(A, 2 * N)),
        (vec![close(&a, N, LONG), close(&b, N, LONG)], bid(B, 2 * N)),
        (vec![liq(&a, N, LONG), liq(&b, N, LONG)], ask(LIQ_ASK, 2 * N)),
        (vec![liq_t(&a, N, LONG, 950_000_000, 0), liq(&b, N, LONG)], ask(LIQ_ASK, 2 * N)),
    ];
    for (legs, e) in cases {
        let d = ev_draft(&legs, &e, &f(), 1);
        let (tx, en) = d.build();
        let r = run(&tx, &en).expect("covenant context");
        assert!(r[0].is_err() && r[1].is_err(), "two inputs of one pair id");
    }
    // Controls with distinct ids.
    let b = live(&base(), 0x27);
    ok(&ev_draft(&[liq(&a, N, LONG), liq(&b, N, LONG)], &ask(LIQ_ASK, 2 * N), &f(), 1));
}

/// The pair bound checks run on the evidence entries too (DESIGN 5).
#[test]
fn malformed_pair_is_refused_on_evidence_entries() {
    let mut p = base();
    p.bl = p.bs;
    bad(&d1(close(&live(&p, 0x25), N, SHORT), &ask(A, N)), 0);
    let mut p = base();
    p.bs = PRICE_MAX + 1;
    bad(&d1(liq(&live(&p, 0x25), N, LONG), &ask(LIQ_ASK, N)), 0);
}
