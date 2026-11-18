//! Review 3 (docs/audit/REVIEW-3.md) after the fixes of DESIGN 4.5c. Every `exploit_*` test of the review is now a
//! `holds_*` test: the transaction is refused, or (where 4.5c documents a residual risk) it stays valid and the test
//! asserts exactly what remains possible, with a comment. The other tests record each new rule (the evidence clock,
//! the hand-over rate limit, health at entry, the range condition) at its boundary, and the attacks on the fix itself
//! (item 6 of the fix task: the since sentinel, forged clocks, one lock time for several bounds, splits and
//! re-pairing with different clocks, the rate limit against the idle timeout and liquidation by transfer). Every
//! input runs its real script: the pairs, the KpfOpen orders, the KOB evidence order and the token program.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::Transaction;
use kaspa_consensus_core::Hash;
use kpf::evidence::*;
use kpf::paths::*;
use kpf::*;
use secp256k1::Keypair;

const N: i64 = 1_000_000_000; // the whole pair: 1,000 tokens
const HALF: i64 = 400_000_000;
const MAX_IDLE: u64 = 77_760_000;
const R: u64 = 600; // minRestDaa of the test market

fn lk() -> Keypair {
    keypair(11)
}
fn sk() -> Keypair {
    keypair(12)
}
/// A second key of the long's owner (a self-transfer).
fn lk2() -> Keypair {
    keypair(13)
}
fn fk() -> Keypair {
    keypair(14)
}

fn accept(d: &Draft) -> Transaction {
    let (tx, e) = d.build();
    assert_accepted(&tx, &e);
    let ins: u128 = e.iter().map(|u| u.amount as u128).sum();
    let outs: u128 = tx.outputs.iter().map(|o| o.value as u128).sum();
    assert!(outs <= ins, "outputs {outs} > inputs {ins}");
    tx
}

fn refused(d: &Draft, i: usize) {
    let (tx, e) = d.build();
    assert_input_fails(&tx, &e, i);
}

/// Input `idx` fails; every input in `ok` passes.
fn refused_only(d: &Draft, idx: usize, ok: &[usize]) {
    let (tx, e) = d.build();
    let r = run(&tx, &e).expect("bindings");
    for &o in ok {
        assert!(r[o].is_ok(), "input {o} should accept: {:?}", r[o]);
    }
    assert!(r[idx].is_err(), "input {idx} accepted");
}

fn locked(d: &Draft, lock: u64) -> Draft {
    let mut x = d.clone();
    x.lock = lock;
    x
}

/// The DESIGN 5 pair: 1,000 tokens, bl 9, bs 12, E 10 KAS, mmr 5 % (long trigger 9.4737, short 11.4286).
fn base() -> Pair {
    pair(&lk(), &sk())
}

/// The base pair with both sides listed (long ask 10, short bid 11), minimum parts 0, no tips.
fn listed() -> Pair {
    let mut p = base();
    (p.long_ask, p.long_min, p.long_tip) = (1_000_000_000, 0, 0);
    (p.short_bid, p.short_min, p.short_tip) = (1_100_000_000, 0, 0);
    p
}

/// A live pair UTXO of DAA score `daa`.
fn at(p: &Pair, tag: u8, daa: u64) -> Live {
    live(p, tag).at(daa)
}

fn liq(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side } }
}

fn close(l: &Live, n: i64, side: i64, signer: Keypair) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side, signer } }
}

/// A KpfOpen of `side` on the pair's market (the fixture orders: long limit 10.5, rate 1, min counter rate 1.5;
/// short limit 9.5, rate 2, min counter rate 0.5).
fn order_for(m: &Market, side: i64, tag: u8) -> LiveOpen {
    let mut o = if side == LONG { long_order(&keypair(21)) } else { short_order(&keypair(22)) };
    o.m = m.clone();
    LiveOpen { value: o.escrow() as u64, o, id: Hash::from_bytes([tag; 32]), op: outpoint(tag) }
}

fn with_rate(mut o: LiveOpen, rate: i64) -> LiveOpen {
    o.o.my_rate = rate;
    o.value = o.o.escrow() as u64;
    o
}

fn liq_t(l: &Live, n: i64, side: i64, x: i64, tag: u8) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::LiqTransfer { side, x, open: order_for(&l.p.m, side, tag), k: 0 } }
}

fn liq_to(l: &Live, n: i64, side: i64, x: i64, open: &LiveOpen) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::LiqTransfer { side, x, open: open.clone(), k: 0 } }
}

/// Ask evidence whose maker and buyer are both `who`.
fn own_ask(price: i64, fill: i64, who: &Keypair) -> Evidence {
    let mut e = Evidence::ask(price, fill);
    e.maker = *who;
    e.ask.maker = xonly(who);
    e.counter = *who;
    e
}

/// Bid evidence whose maker and seller are both `who`.
fn own_bid(price: i64, fill: i64, who: &Keypair) -> Evidence {
    let mut e = Evidence::bid(price, fill);
    e.maker = *who;
    e.bid.maker = xonly(who);
    e.counter = *who;
    e
}

/// The evidence order and its custody placed at DAA `daa` (GTC unless changed).
fn placed(mut e: Evidence, daa: u64) -> Evidence {
    (e.daa, e.tok_daa) = (daa, daa);
    e
}

fn with_expiry(mut e: Evidence, expiry: i64) -> Evidence {
    e.ask.expiry_daa = expiry;
    e.bid.expiry_daa = expiry;
    e
}

fn long_out(d: &Draft) -> i64 {
    d.outs[0].value as i64
}

fn short_out(d: &Draft) -> i64 {
    d.outs[d.outs.len() - 1].value as i64
}

/// An evidence order (and an ask's custody) placed at DAA 1,000 with a soft expiry exactly at the end of its own rest
/// window (1,600): an IOC order (kill at max(UTXO DAA, activeFrom) + 600) when `tif` = 1, a GTD order (expiryDaa =
/// 1,600) when `tif` = 0, refundTip 0. From DAA 1,600 on, KOB's conforming matchers no longer fill it.
fn stale(mut e: Evidence, tif: i64) -> Evidence {
    (e.daa, e.tok_daa) = (1_000, 1_000);
    let end = 1_000 + R as i64;
    match e.kind {
        Kind::Ask => {
            e.ask.tif = tif;
            e.ask.refund_tip = 0;
            if tif == 0 {
                e.ask.expiry_daa = end;
            }
        }
        Kind::Bid => {
            e.bid.tif = tif;
            e.bid.refund_tip = 0;
            if tif == 0 {
                e.bid.expiry_daa = end;
            }
        }
    }
    e
}

/// The draft with the pair written to output `idx` carrying `since`.
fn with_since(d: &Draft, idx: usize, since: i64) -> Draft {
    let mut x = d.clone();
    let mut p = x.pairs.iter().find(|(i, _)| *i == idx).expect("a pair output").1.clone();
    p.since = since;
    x.set_pair(idx, &p);
    x
}

/// P2 add margin of `delta` by the owner of `side`, funded by the executor.
fn add_margin_draft(l: &Live, side: i64, delta: i64) -> Draft {
    let signer = if side == LONG { lk() } else { sk() };
    let mut m = l.clocked().cleared();
    if side == LONG {
        m.bl -= delta;
    } else {
        m.bs += delta;
    }
    let need = q(l.p.size, delta, SCALE, SCALE - 1) as u64;
    let ins = vec![pair_input(l, "addMargin", vec![Arg::Sig(signer), iv(side), iv(delta)]), funder(&fk(), 40, F_FUNDS)];
    let outs = vec![out(l.value + need, p2sh(&m.art()), bind(0, l.id)), out(0, p2pk_spk(&fk()), None)];
    let mut d = Draft::new(ins, outs, Some(1));
    d.pairs.push((0, m));
    d
}

/// The pair outputs of a draft split into the ones whose keys are those of `src` (continuations and remainders)
/// and the others (parts whose side changed hands).
fn kept_and_moved(d: &Draft, src: &Pair) -> (Vec<(usize, Pair)>, Vec<(usize, Pair)>) {
    d.pairs.iter().cloned().partition(|(_, p)| p.long_key == src.long_key && p.short_key == src.short_key)
}

// ================================================================ R3-F1 (Critical): stale evidence

/// R3-F1 after the fix. The review's transactions: the short rests its own IOC (or GTD) ask at 9.45 KAS while the
/// market is at or below 9.45, lets it die at DAA 1,600, and years later liquidates a long that opened at DAA
/// 5,000,000. The pair's evidence clock starts at its own UTXO (since = 0, DAA 5,000,000), so the rest counts from
/// exposed' = 5,000,000 and the order had to be fillable through 5,000,600: it died at 1,600, and the pair refuses.
///
/// What 4.5c leaves possible (residual, documented): a GTC order that is still inside its 90 days when the pair's clock
/// starts did rest R on the pair's clock as a live quote, so it counts even if it is used much later (barrier
/// semantics; DESIGN 4.5c "Residual risk"). The tool refuses it as dead once its 90 days are over (the library test
/// `the_rest_window_is_reported_against_the_daa_asked_about` and `kpf evidence`); the covenant cannot. A pair whose
/// clock starts after the 90 days is refused.
#[test]
fn holds_stale_ask_cannot_liquidate_a_pair_opened_after_it_died() {
    let l = at(&base(), 0x31, 5_000_000);
    for tif in [1, 0] {
        let e = stale(own_ask(945_000_000, N, &sk()), tif);
        let d = ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 1);
        assert!(d.lock > 1_600 + 1_000_000, "the quote has been dead for millions of DAA");
        refused(&d, 0);
    }
    // GTC, inside its 90 days at the clock start: a live quote during the pair's life (residual).
    let e = placed(own_ask(945_000_000, N, &sk()), 1_000);
    let d = locked(&ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 2), 1_000 + 4 * MAX_IDLE);
    accept(&d);
    assert_eq!(long_out(&d), 0);
    // GTC whose 90 days were over before the pair's clock had rested R: refused.
    let late = at(&base(), 0x31, 1_000 + MAX_IDLE - R + 1);
    refused(&locked(&ev_draft(&[liq(&late, N, LONG)], &e, &sk(), 2), 1_000 + 4 * MAX_IDLE), 0);
    let edge = at(&base(), 0x31, 1_000 + MAX_IDLE - R);
    accept(&locked(&ev_draft(&[liq(&edge, N, LONG)], &e, &sk(), 2), 1_000 + 4 * MAX_IDLE));
}

/// R3-F1 on P5 after the fix: the long cannot close at the whole range on its own bid at bs that died before the
/// pair's clock started (GTD and IOC).
#[test]
fn holds_stale_bid_cannot_close_the_long_at_the_whole_range() {
    let l = at(&base(), 0x32, 5_000_000);
    for tif in [0, 1] {
        let mut e = if tif == 0 { stale(own_bid(l.p.bs, N, &lk()), 0) } else { stale(Evidence::ending_bid(l.p.bs, N, 1), 1) };
        e.maker = lk();
        e.bid.maker = xonly(&lk());
        e.counter = lk();
        refused(&ev_draft(&[close(&l, N, LONG, lk())], &e, &lk(), 3), 0);
    }
}

/// R3-F1 control: the same ask, live during the pair's life (placed after the clock start), is accepted. The covenant
/// accepts a live quote and refuses a dead one by the clock, not by reading the time of the fill.
#[test]
fn holds_live_quote_is_the_control_of_r3_f1() {
    let l = at(&base(), 0x33, 5_000_000);
    let e = placed(own_ask(945_000_000, N, &sk()), 5_000_100);
    accept(&ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 4));
}

// ---------------------------------------------------------------- the evidence clock at its boundaries

/// Evidence that rested before the clock started counts only from S: a GTC ask placed at DAA 1,000 against a pair
/// whose clock starts at 5,000,000 needs the lock time 5,000,600; 5,000,599 is refused. With since non-zero the clock
/// is since, not the UTXO's DAA (a remainder created later keeps its pair's clock).
#[test]
fn holds_evidence_resting_before_the_clock_counts_only_from_s() {
    let e = placed(Evidence::ask(940_000_000, N), 1_000);
    let l = at(&base(), 0x34, 5_000_000);
    refused(&locked(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 5), 5_000_000 + R - 1), 0);
    accept(&locked(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 5), 5_000_000 + R));
    let mut p = base();
    p.since = 5_000_000;
    let r = at(&p, 0x35, 9_000_000);
    refused(&locked(&ev_draft(&[liq(&r, N, LONG)], &e, &fk(), 5), 5_000_000 + R - 1), 0);
    accept(&locked(&ev_draft(&[liq(&r, N, LONG)], &e, &fk(), 5), 5_000_000 + R));
    // Barrier semantics (DESIGN 4.5c): a quote that touched the trigger after S counts later too, while its bound is
    // unchanged; the amount does not depend on when.
    let d = locked(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 5), 9_000_000);
    accept(&d);
    assert_eq!(short_out(&d), l.value as i64 - liq_fee(&l.p, N, 940_000_000, LONG));
}

/// Evidence that died before exposed' + R is refused, for each soft expiry: a GTD order (expiryDaa), an IOC order
/// (kill = max(UTXO DAA, activeFrom) + 600) and a GTC order (90 days idle), one DAA inside and one outside. S = 50,000.
#[test]
fn holds_evidence_that_died_before_it_rested_on_the_clock_is_refused() {
    let s = 50_000u64;
    let l = at(&base(), 0x36, s);
    let lock = NOW;
    // GTD placed before S: alive through S + R is required.
    let e = placed(Evidence::ask(940_000_000, N), s - 5_000);
    accept(&locked(&ev_draft(&[liq(&l, N, LONG)], &with_expiry(e.clone(), (s + R) as i64), &fk(), 6), lock));
    refused(&locked(&ev_draft(&[liq(&l, N, LONG)], &with_expiry(e, (s + R) as i64 - 1), &fk(), 6), lock), 0);
    // IOC: placed at S its kill is S + 600 = S + R; placed one DAA earlier it dies one DAA too soon.
    let mut ioc = placed(Evidence::ask(940_000_000, N), s);
    ioc.ask.tif = 1;
    accept(&locked(&ev_draft(&[liq(&l, N, LONG)], &ioc, &fk(), 7), lock));
    let mut ioc = placed(Evidence::ask(940_000_000, N), s - 1);
    ioc.ask.tif = 1;
    refused(&locked(&ev_draft(&[liq(&l, N, LONG)], &ioc, &fk(), 7), lock), 0);
    // GTC: placed at ud, the 90 days end at ud + MAX_IDLE; the pair's clock must leave R before that.
    let ud = 1_000u64;
    let gtc = placed(Evidence::bid(1_150_000_000, N), ud);
    let ok = at(&base(), 0x37, ud + MAX_IDLE - R);
    accept(&locked(&ev_draft(&[liq(&ok, N, SHORT)], &gtc, &fk(), 8), ud + MAX_IDLE + 5));
    let late = at(&base(), 0x37, ud + MAX_IDLE - R + 1);
    refused(&locked(&ev_draft(&[liq(&late, N, SHORT)], &gtc, &fk(), 8), ud + MAX_IDLE + 5), 0);
}

/// The clock is preserved by amend, add margin and every remainder: each output that changes no hands carries
/// since = S (here S = 2,000, the UTXO's DAA, and for the second pair S = since = 1,500 on a UTXO of DAA 7,000), and
/// an output with since 0 (a restart) or S + 1 is refused by the pair. The parts that change hands carry since 0 and
/// one with since = S (no restart) is refused. Listed: P1, P2, P3 (remainder), P4 (host and donor remainders), P4'
/// (remainder), P5, P6, P7 (remainder).
#[test]
fn holds_the_clock_is_kept_by_every_output_that_changes_no_hands() {
    let mut second = listed();
    second.since = 1_500;
    for (l, s) in [(at(&listed(), 0x40, 2_000), 2_000i64), (at(&second, 0x41, 7_000), 1_500)] {
        assert_eq!(l.clock(), s);
        let check = |d: &Draft, input: usize, name: &str| {
            let (kept, moved) = kept_and_moved(d, &l.p);
            assert!(!kept.is_empty() || !moved.is_empty(), "{name}: no pair output");
            accept(d);
            for (idx, p) in &kept {
                assert_eq!(p.since, s, "{name}: output {idx} keeps the clock");
                refused(&with_since(d, *idx, 0), input);
                refused(&with_since(d, *idx, s + 1), input);
            }
            // The outputs where one side of this pair changed hands (another pair's remainder shares no key).
            for (idx, p) in moved.iter().filter(|(_, p)| p.long_key == l.p.long_key || p.short_key == l.p.short_key) {
                assert_eq!(p.since, 0, "{name}: output {idx} changes hands");
                refused(&with_since(d, *idx, s), input);
            }
        };
        check(&amend_draft(&l, &lk(), LONG, (1_020_000_000, 0, 0), &fk(), 10), 0, "P1 amend");
        check(&add_margin_draft(&l, LONG, 100_000_000), 0, "P2 add margin");
        check(&add_margin_draft(&l, SHORT, 100_000_000), 0, "P2 add margin (short)");
        let t = TransferSpec { side: LONG, n: HALF, x: 1_000_000_000, nb: 900_000_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
        check(&transfer_draft(&l, &t, &Entrant::Direct(keypair(16)), &fk(), 11), 0, "P3 transfer");
        let mut dp = listed();
        (dp.bl, dp.bs, dp.long_key, dp.short_key) = (850_000_000, 1_150_000_000, xonly(&keypair(17)), xonly(&keypair(18)));
        let donor = at(&dp, 0x42, 2_500);
        let cd = close_draft(&l, &donor, &CloseSpec { n: HALF, x: 1_050_000_000, e3: 1_000_000_000 }, &fk(), 12);
        check(&cd, 0, "P4 host");
        let (dkept, _) = kept_and_moved(&cd, &donor.p);
        for (idx, p) in &dkept {
            assert_eq!(p.since, donor.clock(), "P4 donor remainder keeps the donor's clock");
            refused(&with_since(&cd, *idx, 0), 1);
        }
        check(&mutual_draft(&l, HALF, 1_050_000_000, &fk(), 13), 0, "P4' mutual close");
        check(&ev_draft(&[close(&l, HALF, LONG, lk())], &Evidence::bid(1_050_000_000, HALF), &fk(), 14), 0, "P5 close");
        check(&ev_draft(&[liq(&l, HALF, LONG)], &Evidence::ask(940_000_000, HALF), &fk(), 15), 0, "P6 liquidation");
        check(&ev_draft(&[liq_t(&l, HALF, LONG, 950_000_000, 0x71)], &Evidence::ask(940_000_000, HALF), &fk(), 16), 0, "P7 liquidation by transfer");
    }
}

/// Add margin needs no restart: it moves the trigger, and evidence must satisfy the trigger of the state it is used
/// against. A long touched at 9.4 KAS adds 0.5 KAS of margin (bl 8.5, trigger 8.947): the same live quote no longer
/// liquidates the continuation, which keeps the clock S.
#[test]
fn holds_add_margin_cures_a_touch_without_restarting_the_clock() {
    let l = at(&base(), 0x43, 2_000);
    let e = Evidence::ask(940_000_000, N);
    accept(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 17));
    let d = add_margin_draft(&l, LONG, 50_000_000);
    let tx = accept(&d);
    let c = live_pairs(&tx, &d).remove(0).at(3_000);
    assert_eq!((c.p.since, c.clock()), (2_000, 2_000));
    refused(&ev_draft(&[liq(&c, N, LONG)], &e, &fk(), 17), 0);
}

/// The clock restarts on each hand-over, and evidence that rested before it no longer counts for the part that
/// changed hands. T1: KpfOpen writes since = 0 and refuses any other value. T2, T3 and T7: the part (re-paired output)
/// carries since 0, and the same live ask that liquidates the old pair counts against the part only from its own UTXO.
#[test]
fn holds_the_clock_restarts_on_each_hand_over() {
    // T1.
    let (lo, mut so) = (order_for(&market(), LONG, 0x72), order_for(&market(), SHORT, 0x73));
    so.o.size_left = N;
    so.value = so.o.escrow() as u64;
    let d = open_draft(&lo, &so, N, 1_000_000_000, &fk(), 20);
    assert_eq!(d.pairs[0].1.since, 0);
    let tx = accept(&d);
    let x = with_since(&d, 2, 7);
    let (t2, e2) = x.build();
    let r = run(&t2, &e2).unwrap();
    assert!(r[0].is_err() && r[1].is_err(), "both orders refuse a new pair with a clock of its own choosing");
    let opened = live_pairs(&tx, &d).remove(0).at(4_000);
    assert_eq!(opened.clock(), 4_000);
    // T2 at DAA 4,400 (S + 4R of a pair of DAA 2,000).
    let l = at(&listed(), 0x44, 2_000);
    let ask = placed(Evidence::ask(940_000_000, N), 2_100);
    accept(&locked(&ev_draft(&[liq(&l, N, LONG)], &ask, &fk(), 21), 2_100 + R));
    let t = TransferSpec { side: LONG, n: N, x: 1_000_000_000, nb: 900_000_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    let d = transfer_draft(&l, &t, &Entrant::Direct(keypair(16)), &fk(), 22);
    assert_eq!(d.lock, 2_000 + 4 * R);
    let tx = accept(&d);
    let part = live_pairs(&tx, &d).remove(0).at(4_400);
    assert_eq!(part.p.since, 0);
    refused(&locked(&ev_draft(&[liq(&part, N, LONG)], &ask, &fk(), 23), 4_400 + R - 1), 0);
    accept(&locked(&ev_draft(&[liq(&part, N, LONG)], &ask, &fk(), 23), 4_400 + R));
    // T3: the re-paired output.
    let mut dp = listed();
    (dp.bl, dp.bs, dp.long_key, dp.short_key) = (850_000_000, 1_150_000_000, xonly(&keypair(17)), xonly(&keypair(18)));
    let donor = at(&dp, 0x45, 2_000);
    let cd = close_draft(&l, &donor, &CloseSpec { n: N, x: 1_050_000_000, e3: 1_000_000_000 }, &fk(), 24);
    let tx = accept(&cd);
    let p3 = live_pairs(&tx, &cd).remove(0).at(4_400);
    assert_eq!(p3.p.since, 0, "P3 restarts its clock");
    let bid = placed(Evidence::bid(1_150_000_000, N), 2_100);
    refused(&locked(&ev_draft(&[liq(&p3, N, SHORT)], &bid, &fk(), 25), 4_400 + R - 1), 0);
    // T7: the part of a liquidation by transfer.
    let e = placed(Evidence::ask(940_000_000, N), 2_100);
    let d = locked(&ev_draft(&[liq_t(&l, N, LONG, 950_000_000, 0x74)], &e, &fk(), 26), 2_100 + R);
    let tx = accept(&d);
    let p7 = live_pairs(&tx, &d).into_iter().find(|x| x.p.long_key != l.p.long_key).unwrap();
    assert_eq!(p7.p.since, 0);
}

// ---------------------------------------------------------------- the hand-over rate limit

/// A touched long tries to restart its clock by transferring to itself (a second key, at a price it pays itself)
/// before S + 4R: refused. In the window before it, a keeper liquidates it with the evidence that touched it. At
/// S + 4R the self-transfer is accepted (4.5c: hand-overs are rate-limited, not forbidden), the clock restarts, and a
/// keeper liquidates the new part with evidence that rested R on the new clock (the same live order).
///
/// Residual, as 4.5c states it ("a side cannot dodge liquidation by handing itself over faster than evidence can
/// rest"): a quote that starts resting in the last R before S + 4R and dies before resting R after the restart is
/// dodged. Here an ask placed at S + 3.5R that dies at S + 4.5R would have liquidated the old pair at S + 4.5R; after
/// the self-transfer at S + 4R it is refused against the part.
#[test]
fn holds_self_transfer_before_s_plus_4r_is_refused_and_keepers_keep_the_window() {
    let s = 100_000u64;
    let mut p = base();
    (p.long_ask, p.long_min, p.long_tip) = (948_000_000, 0, 0);
    let l = at(&p, 0x46, s);
    let touch = placed(Evidence::ask(940_000_000, N), s + 100);
    // The self-transfer at 9.48 KAS (healthy at its own price: 9.48 * 0.95 = 9.006 >= bl 9).
    let t = TransferSpec { side: LONG, n: N, x: 948_000_000, nb: 900_000_000, ek: xonly(&lk2()), et: ZERO, k: 0 };
    let d = transfer_draft(&l, &t, &Entrant::Direct(lk2()), &fk(), 30);
    assert_eq!(d.lock, s + 4 * R);
    refused(&locked(&d, s + 4 * R - 1), 0);
    // The keeper's window: the touch has rested by S + 700.
    accept(&locked(&ev_draft(&[liq(&l, N, LONG)], &touch, &fk(), 31), s + 100 + R));
    // At S + 4R the restart is allowed.
    let tx = accept(&d);
    let part = live_pairs(&tx, &d).remove(0).at(s + 4 * R);
    assert_eq!((part.p.since, part.p.long_key), (0, xonly(&lk2())));
    refused(&locked(&ev_draft(&[liq(&part, N, LONG)], &touch, &fk(), 32), s + 5 * R - 1), 0);
    accept(&locked(&ev_draft(&[liq(&part, N, LONG)], &touch, &fk(), 32), s + 5 * R));
    // The residual: a late touch that dies within R of the restart.
    let late = with_expiry(placed(Evidence::ask(940_000_000, N), s + 4 * R - R / 2), (s + 4 * R + R / 2) as i64);
    accept(&locked(&ev_draft(&[liq(&l, N, LONG)], &late, &fk(), 33), s + 4 * R + R / 2));
    refused(&locked(&ev_draft(&[liq(&part, N, LONG)], &late, &fk(), 33), s + 4 * R + R / 2), 0);
    // And the new part cannot be handed over again before its own S' + 4R.
    let mut pl = part.clone();
    pl.p.long_ask = 948_000_000;
    let again = TransferSpec { side: LONG, n: N, x: 948_000_000, nb: 900_000_000, ek: xonly(&lk()), et: ZERO, k: 0 };
    let d2 = transfer_draft(&pl, &again, &Entrant::Direct(lk()), &fk(), 34);
    assert_eq!(d2.lock, s + 8 * R);
}

/// The rate limit binds close-versus-close on each pair by its own clock: host S = 3,000, donor since = 1,500 (on a
/// UTXO of DAA 8,000). The lock time must reach max(S_h, S_d) + 4R = 5,400; at 5,399 the host refuses and the donor
/// (3,900) accepts. P3 restarts (since 0); it may not carry either old clock; the remainders keep their own clocks.
#[test]
fn holds_close_versus_close_with_two_different_clocks() {
    let h = at(&listed(), 0x47, 3_000);
    let mut dp = listed();
    (dp.bl, dp.bs, dp.since, dp.long_key, dp.short_key) = (850_000_000, 1_150_000_000, 1_500, xonly(&keypair(17)), xonly(&keypair(18)));
    let d = at(&dp, 0x48, 8_000);
    let cd = close_draft(&h, &d, &CloseSpec { n: HALF, x: 1_050_000_000, e3: 1_000_000_000 }, &fk(), 35);
    assert_eq!(cd.lock, 3_000 + 4 * R);
    refused_only(&locked(&cd, 3_000 + 4 * R - 1), 0, &[1]);
    accept(&cd);
    let p3 = cd.pairs.iter().find(|(i, _)| *i == 2).unwrap().1.clone();
    assert_eq!(p3.since, 0);
    for old in [3_000, 1_500] {
        let x = with_since(&cd, 2, old);
        let (tx, e) = x.build();
        let r = run(&tx, &e).unwrap();
        assert!(r[0].is_err() && r[1].is_err(), "P3 with the old clock {old} is refused by both pairs");
    }
    let (hk, _) = kept_and_moved(&cd, &h.p);
    let (dk, _) = kept_and_moved(&cd, &d.p);
    assert_eq!((hk[0].1.since, dk[0].1.since), (3_000, 1_500));
}

/// A split whose remainder (old clock) and part (new clock) are later re-paired with other pairs: each re-pairing
/// restarts the re-paired output, and no output can take over the other piece's clock. The part and the remainder of
/// one split share a covenant id, so they cannot be re-paired with each other (the single-input guard).
#[test]
fn holds_split_pieces_with_different_clocks_re_paired() {
    let l = at(&listed(), 0x49, 2_000);
    let t = TransferSpec { side: SHORT, n: HALF, x: 1_100_000_000, nb: 1_200_000_000, ek: xonly(&keypair(19)), et: ZERO, k: 0 };
    let d = transfer_draft(&l, &t, &Entrant::Direct(keypair(19)), &fk(), 36);
    let tx = accept(&d);
    let pieces = live_pairs(&tx, &d);
    let part = pieces.iter().find(|x| x.p.short_key != l.p.short_key).unwrap().clone().at(4_400);
    let rest = pieces.iter().find(|x| x.p.short_key == l.p.short_key).unwrap().clone().at(4_400);
    assert_eq!((part.clock(), rest.clock()), (4_400, 2_000));
    // The remainder (old clock) hosts a re-pairing with a third pair.
    let mut dp = listed();
    (dp.bl, dp.bs, dp.size, dp.long_key, dp.short_key) = (850_000_000, 1_150_000_000, 600_000_000, xonly(&keypair(17)), xonly(&keypair(18)));
    let donor = at(&dp, 0x4a, 2_000);
    let cd = close_draft(&rest, &donor, &CloseSpec { n: 600_000_000, x: 1_050_000_000, e3: 1_000_000_000 }, &fk(), 37);
    assert_eq!(cd.lock, 4_400);
    accept(&cd);
    assert_eq!(cd.pairs.iter().find(|(i, _)| *i == 2).unwrap().1.since, 0);
    refused(&with_since(&cd, 2, part.clock()), 0);
    // The two pieces of one split as host and donor: both refused (they share an id).
    let mut rp = rest.clone();
    rp.p.long_ask = 1_000_000_000;
    let mut pp = part.clone();
    (pp.p.short_bid, pp.p.bl) = (1_100_000_000, 850_000_000);
    pp.id = rp.id;
    let (tx, e) = close_draft(&rp, &pp, &CloseSpec { n: HALF, x: 1_050_000_000, e3: 1_000_000_000 }, &fk(), 38).build();
    let r = run(&tx, &e);
    assert!(r.is_err() || r.unwrap().iter().take(2).all(|x| x.is_err()));
}

/// Liquidation by transfer of a part handed over moments ago: the part (DAA 4,400) cannot be liquidated before it has
/// R of evidence on its own clock (4,999 refused, 5,000 accepted with a live order placed before the hand-over), and
/// P7 is not rate-limited, while a transfer of that part would wait until 4,400 + 4R. The winner's part of the P7
/// restarts its clock as well (4.5c: the part of T7 carries since 0).
#[test]
fn holds_liquidation_by_transfer_of_a_part_handed_over_moments_ago() {
    let l = at(&listed(), 0x4b, 2_000);
    let t = TransferSpec { side: LONG, n: N, x: 1_000_000_000, nb: 900_000_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    let d = transfer_draft(&l, &t, &Entrant::Direct(keypair(16)), &fk(), 39);
    let tx = accept(&d);
    let part = live_pairs(&tx, &d).remove(0).at(4_400);
    let e = placed(Evidence::ask(940_000_000, N), 3_000);
    refused(&locked(&ev_draft(&[liq_t(&part, N, LONG, 950_000_000, 0x75)], &e, &fk(), 40), 4_400 + R - 1), 0);
    let d7 = locked(&ev_draft(&[liq_t(&part, N, LONG, 950_000_000, 0x75)], &e, &fk(), 40), 4_400 + R);
    let tx7 = accept(&d7);
    assert!(d7.lock < part.handover_from(), "P7 runs before a transfer of the part could");
    let new = live_pairs(&tx7, &d7).remove(0);
    assert_eq!((new.p.since, new.p.short_key), (0, part.p.short_key), "the winner stays, on a fresh clock");
}

/// The rate limit counts from S, not from the latest UTXO: an amend at DAA 4,000 keeps S = 2,000, so a transfer is
/// allowed at 4,400 (not 6,400). The idle timeout is independent of the rate limit: a part handed over moments ago
/// can be idle-settled once its own UTXO is idleDaa old (CSV), with no hand-over limit.
#[test]
fn holds_rate_limit_counts_from_the_clock_and_leaves_the_idle_timeout_alone() {
    let l = at(&listed(), 0x4c, 2_000);
    let d = amend_draft(&l, &lk(), LONG, (1_000_000_000, 0, 0), &fk(), 41);
    let tx = accept(&d);
    let a = live_pairs(&tx, &d).remove(0).at(4_000);
    assert_eq!(a.clock(), 2_000);
    let t = TransferSpec { side: LONG, n: N, x: 1_000_000_000, nb: 900_000_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    let td = transfer_draft(&a, &t, &Entrant::Direct(keypair(16)), &fk(), 42);
    assert_eq!(td.lock, 4_400);
    let tx = accept(&td);
    let part = live_pairs(&tx, &td).remove(0).at(4_400);
    accept(&idle_draft(&part, &fk(), 43));
}

// ================================================================ R3-F2 (High): the winner forfeits

/// R3-F2 after the fix. The review's three pairs (mmr 30 % and 49.99 % on the DESIGN 5 pair; bl 9.6, bs 10 at 5 %)
/// are narrower than both maintenance bands, so neither side is liquidatable at any quote, by P6 or P7; the side
/// owed the whole pair at the true market keeps it.
#[test]
fn holds_profitable_side_never_forfeits_in_a_narrow_pair() {
    let mut a = base();
    a.m.mmr_bps = 3_000;
    let mut b = base();
    b.m.mmr_bps = 4_999;
    let mut c = base();
    (c.bl, c.bs, c.entry) = (960_000_000, 1_000_000_000, 980_000_000);
    for (p, tag) in [(a, 0x50u8), (b, 0x51), (c, 0x52)] {
        assert!(!p.range_ok());
        let l = live(&p, tag);
        for ask in [1_250_000_000, 950_000_000, 1] {
            refused(&ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(ask, N), &sk(), 50), 0);
        }
        for bid in [850_000_000, 955_000_000, 2_000_000_000] {
            refused(&ev_draft(&[liq(&l, N, SHORT)], &Evidence::bid(bid, N), &lk(), 51), 0);
        }
        refused(&ev_draft(&[liq_t(&l, N, LONG, 1_000_000_000, 0x76)], &Evidence::ask(850_000_000, N), &fk(), 52), 0);
        // The exits that need no liquidation still work: a unilateral close at the adverse bound.
        accept(&ev_draft(&[close(&l, N, LONG, lk())], &Evidence::bid(p.bs, N), &fk(), 53));
    }
}

/// R3-F2 boundary: the range condition bl * (10^4 + mmr) < bs * (10^4 - mmr) at one sompi. bs = 12 KAS, mmr 5 %:
/// bl = 1,085,714,285 passes and its long is liquidatable below its trigger; bl + 1 fails and is not.
#[test]
fn holds_range_condition_at_its_boundary() {
    let mut p = base();
    p.bl = 1_085_714_285;
    assert!(p.range_ok());
    let l = live(&p, 0x53);
    accept(&ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(1_100_000_000, N), &sk(), 54));
    p.bl += 1;
    assert!(!p.range_ok());
    refused(&ev_draft(&[liq(&live(&p, 0x53), N, LONG)], &Evidence::ask(1_100_000_000, N), &sk(), 54), 0);
    // The DESIGN 5 pair at 5 %: an ask above bs and a bid below bl never liquidate (review 3's own boundary).
    let l = live(&base(), 0x54);
    refused(&ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(1_250_000_000, N), &sk(), 55), 0);
    refused(&ev_draft(&[liq(&l, N, SHORT)], &Evidence::bid(850_000_000, N), &lk(), 55), 0);
}

/// A narrow pair is refused at creation: T1 at x = 10, mmr 5 %, with bl = 9.5 and bs = 10.5 has both sides healthy at
/// x but bl * 1.05 = bs * 0.95 exactly: both orders refuse. One sompi wider is accepted. KpfOpen.take refuses a part
/// that is narrow (a look-alike pair; a real pair's transfer cannot narrow it), while the pair itself accepts.
#[test]
fn holds_narrow_pairs_are_refused_at_creation() {
    let mk = |lr: i64, sr: i64| {
        let mut lo = with_rate(order_for(&market(), LONG, 0x77), lr);
        lo.o.min_counter_rate = 0;
        lo.value = lo.o.escrow() as u64;
        let mut so = with_rate(order_for(&market(), SHORT, 0x78), sr);
        (so.o.size_left, so.o.min_counter_rate) = (N, 0);
        so.value = so.o.escrow() as u64;
        open_draft(&lo, &so, N, 1_000_000_000, &fk(), 56)
    };
    let (tx, e) = mk(50_000_000, 50_000_000).build();
    let r = run(&tx, &e).unwrap();
    assert!(r[0].is_err() && r[1].is_err(), "equality is narrow: {r:?}");
    accept(&mk(50_000_000, 50_000_001));
    // take into a narrow part.
    let mut np = base();
    (np.bl, np.bs, np.long_ask, np.long_min) = (950_000_000, 1_000_000_000, 1_000_000_000, 0);
    let l = live(&np, 0x55);
    let mut lo = with_rate(order_for(&market(), LONG, 0x79), 50_000_000);
    lo.o.min_counter_rate = 0;
    lo.value = lo.o.escrow() as u64;
    let t = TransferSpec { side: LONG, n: HALF, x: 1_000_000_000, nb: 950_000_000, ek: lo.o.owner, et: lo.id, k: 0 };
    refused_only(&transfer_draft(&l, &t, &Entrant::Open(lo), &fk(), 57), 1, &[0]);
}

/// Add margin, transfer and re-pairing never narrow a pair: starting from the narrowest pair the condition allows
/// (bl = 1,085,714,285, bs = 12 KAS), every output of P2, P3 (both sides) and P4 still satisfies it, as the algebra of
/// no-downgrade says (the new bounds only move outward).
#[test]
fn holds_add_margin_transfer_and_re_pairing_keep_the_range() {
    let mut p = listed();
    (p.bl, p.long_ask, p.short_bid) = (1_085_714_285, 1_150_000_000, 1_150_000_000);
    assert!(p.range_ok());
    let l = at(&p, 0x56, 2_000);
    let mut outs: Vec<Pair> = vec![];
    for side in [LONG, SHORT] {
        let d = add_margin_draft(&l, side, 1);
        accept(&d);
        outs.extend(d.pairs.iter().map(|(_, q)| q.clone()));
    }
    let t = TransferSpec { side: LONG, n: HALF, x: 1_150_000_000, nb: 1_085_714_285, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    let d = transfer_draft(&l, &t, &Entrant::Direct(keypair(16)), &fk(), 58);
    accept(&d);
    outs.extend(d.pairs.iter().map(|(_, q)| q.clone()));
    let t = TransferSpec { side: SHORT, n: HALF, x: 1_150_000_000, nb: 1_207_500_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    let d = transfer_draft(&l, &t, &Entrant::Direct(keypair(16)), &fk(), 59);
    accept(&d);
    outs.extend(d.pairs.iter().map(|(_, q)| q.clone()));
    let mut dp = listed();
    (dp.bl, dp.bs, dp.short_bid, dp.long_key, dp.short_key) = (1_000_000_000, 1_200_000_000, 1_150_000_000, xonly(&keypair(17)), xonly(&keypair(18)));
    let donor = at(&dp, 0x57, 2_000);
    let cd = close_draft(&l, &donor, &CloseSpec { n: HALF, x: 1_150_000_000, e3: 1_000_000_000 }, &fk(), 60);
    accept(&cd);
    outs.extend(cd.pairs.iter().map(|(_, q)| q.clone()));
    assert!(outs.len() >= 8);
    for q in &outs {
        assert!(q.range_ok(), "an output narrower than both maintenance bands: bl {} bs {}", q.bl, q.bs);
    }
}

// ================================================================ R3-F3 (Medium): P7 entrant born liquidatable

/// R3-F3 after the fix. The review's transaction: the long order (rate 1) takes the loser's part with the loser's bl 9
/// at a 9.4 KAS ask, with the pair's x at the quote (the executor keeping the 600 KAS gap) or at 10. The pair now
/// requires the entrant to be healthy at the quote itself (DESIGN 4.5c, read at the quote, see the `// DESIGN:` note in
/// KpfPair.onEvidence): both are refused. At x = 9.93 (bound 8.93 = floor(9.4 * 0.95)) it is accepted, and the same
/// 9.4 ask does not liquidate the entrant afterwards (it is not liquidatable, and the ask rested before its clock).
#[test]
fn holds_p7_entrant_is_healthy_at_the_quote() {
    let l = live(&base(), 0x38);
    let quote = 940_000_000;
    let mut d = ev_draft(&[liq_t(&l, N, LONG, 1_000_000_000, 0x71)], &Evidence::ask(quote, N), &fk(), 9);
    refused(&d, 0);
    let mut a = d.args(0);
    a[ARG_X] = iv(quote);
    d.set_args(0, a);
    d.outs[0].value = p7_pay(&l, N, LONG, quote, quote) as u64;
    refused(&d, 0);
    let x = kpf_protocol::evidence::entrant_health_bound(&l.p.m, LONG, quote, 100_000_000);
    assert_eq!(x, 993_000_000);
    let d = ev_draft(&[liq_t(&l, N, LONG, x, 0x71)], &Evidence::ask(quote, N), &fk(), 9);
    let tx = accept(&d);
    let part = live_pairs(&tx, &d).into_iter().find(|p| p.p.long_key != l.p.long_key).expect("the entrant's part").at(NOW);
    assert_eq!(part.p.bl, 893_000_000);
    assert!(!kpf_protocol::payoff::long_liquidatable(&part.p, quote));
    let mut e2 = Evidence::ask(quote, N);
    e2.id = Hash::from_bytes([0xe5; 32]);
    refused(&locked(&ev_draft(&[liq(&part, N, LONG)], &e2, &sk(), 10), NOW + R), 0);
}

/// R3-F3 control: the no-downgrade rule still bounds the entrant at the loser's bl (an entrant bound above it is
/// refused), and the health rule binds independently of it.
#[test]
fn holds_p7_entrant_bound_never_above_the_losers() {
    let l = live(&base(), 0x39);
    refused(&ev_draft(&[liq_t(&l, N, LONG, 1_050_000_000, 0x72)], &Evidence::ask(940_000_000, N), &fk(), 11), 0);
}

// ================================================================ R3-F4 (Low): an order filled at its limit

/// R3-F4 after the fix. The fixture long order (limit 10.5, rate 1) filled at its limit is healthy at its own entry
/// price (bl 9.5, trigger 10.0 < 10.5), so KpfOpen accepts it. The review's immediate liquidation used an ask that
/// rested before the pair existed: refused by the clock. What 4.5c leaves possible (documented; the order ticket of
/// `kpf create-order` warns): if the market really is below the trigger for R after the open, a live ask liquidates the
/// long and it forfeits its margin (limit slippage under forfeiture). An order whose own rate is below the maintenance
/// of its limit (0.5 KAS at 10.5) is refused at its limit and accepted at a lower price.
#[test]
fn holds_order_filled_at_its_limit_is_healthy_at_entry() {
    let lo = order_for(&market(), LONG, 0x73);
    let mut so = order_for(&market(), SHORT, 0x74);
    so.o.size_left = N;
    so.value = so.o.escrow() as u64;
    let x = lo.o.limit;
    let d = open_draft(&lo, &so, N, x, &fk(), 12);
    let tx = accept(&d);
    let pl = live_pairs(&tx, &d).remove(0).at(NOW - 100);
    assert_eq!((pl.p.bl, pl.p.bs), (950_000_000, 1_250_000_000));
    let a = 999_000_000;
    let before = Evidence::ask(a, N); // placed at NOW - R - 50, before the pair
    refused(&ev_draft(&[liq(&pl, N, LONG)], &before, &fk(), 13), 0);
    // Residual: a live ask still resting R after the open liquidates it.
    let d2 = locked(&ev_draft(&[liq(&pl, N, LONG)], &before, &fk(), 13), NOW - 100 + R);
    accept(&d2);
    assert_eq!(long_out(&d2), 0);
    // A rate below the maintenance of the limit.
    let thin = with_rate(order_for(&market(), LONG, 0x75), 50_000_000);
    let mut so2 = so.clone();
    so2.o.min_counter_rate = 0;
    so2.value = so2.o.escrow() as u64;
    let (t1, e1) = open_draft(&thin, &so2, N, 1_050_000_000, &fk(), 14).build();
    assert!(run(&t1, &e1).unwrap()[0].is_err(), "0.5 KAS is below 5 % of 10.5");
    accept(&open_draft(&thin, &so2, N, 1_000_000_000, &fk(), 14));
}

// ---------------------------------------------------------------- health at entry, one tick each side and path

/// An entrant unhealthy at entry by one sompi is refused, each side and each path; at the boundary it is accepted.
/// T1 (both orders check the whole new pair): long rate 0.49999999 / 0.5 and short rate 0.49999999 / 0.5 at x = 10.
/// T2 order taker at x = 9.4 (long, rate 0.46999999 / 0.47) and x = 11.5 (short, rate 0.57499999 / 0.575); T2
/// signing taker with bound 8.93000001 / 8.93 and 12.07499999 / 12.075. T7 at an ask of 9.4 (entrant x 9.93000001 /
/// 9.93) and a bid of 11.5 (x 10.07499999 / 10.075).
#[test]
fn holds_entrant_unhealthy_by_one_tick_is_refused() {
    // T1.
    for side in [LONG, SHORT] {
        for (rate, ok) in [(49_999_999, false), (50_000_000, true)] {
            let mut lo = order_for(&market(), LONG, 0x80);
            let mut so = order_for(&market(), SHORT, 0x81);
            (lo.o.min_counter_rate, so.o.min_counter_rate, so.o.size_left) = (0, 0, N);
            if side == LONG {
                lo.o.my_rate = rate;
            } else {
                so.o.my_rate = rate;
            }
            (lo.value, so.value) = (lo.o.escrow() as u64, so.o.escrow() as u64);
            let d = open_draft(&lo, &so, N, 1_000_000_000, &fk(), 60);
            if ok {
                accept(&d);
            } else {
                let (tx, e) = d.build();
                let r = run(&tx, &e).unwrap();
                assert!(r[0].is_err() && r[1].is_err(), "T1 side {side} rate {rate}");
            }
        }
    }
    // T2, order taker.
    let mut p = base();
    (p.long_ask, p.short_bid) = (940_000_000, 1_150_000_000);
    let l = live(&p, 0x58);
    for (rate, ok) in [(46_999_999, false), (47_000_000, true)] {
        let lo = with_rate(order_for(&market(), LONG, 0x82), rate);
        let t = TransferSpec { side: LONG, n: HALF, x: 940_000_000, nb: 940_000_000 - rate, ek: lo.o.owner, et: lo.id, k: 0 };
        let d = transfer_draft(&l, &t, &Entrant::Open(lo), &fk(), 61);
        if ok {
            accept(&d);
        } else {
            let (tx, e) = d.build();
            let r = run(&tx, &e).unwrap();
            assert!(r[0].is_err() && r[1].is_err(), "T2 long order rate {rate}: the pair and the order refuse");
        }
    }
    for (rate, ok) in [(57_499_999, false), (57_500_000, true)] {
        let so = with_rate(order_for(&market(), SHORT, 0x83), rate);
        let t = TransferSpec { side: SHORT, n: HALF, x: 1_150_000_000, nb: 1_150_000_000 + rate, ek: so.o.owner, et: so.id, k: 0 };
        let d = transfer_draft(&l, &t, &Entrant::Open(so), &fk(), 62);
        if ok {
            accept(&d);
        } else {
            let (tx, e) = d.build();
            let r = run(&tx, &e).unwrap();
            assert!(r[0].is_err() && r[1].is_err(), "T2 short order rate {rate}");
        }
    }
    // T2, signing taker.
    for (side, x, bad, good) in [(LONG, 940_000_000, 893_000_001, 893_000_000), (SHORT, 1_150_000_000, 1_207_499_999, 1_207_500_000)] {
        let t = |nb: i64| TransferSpec { side, n: HALF, x, nb, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
        refused(&transfer_draft(&l, &t(bad), &Entrant::Direct(keypair(16)), &fk(), 63), 0);
        accept(&transfer_draft(&l, &t(good), &Entrant::Direct(keypair(16)), &fk(), 63));
    }
    // T7.
    let l = live(&base(), 0x59);
    refused(&ev_draft(&[liq_t(&l, N, LONG, 993_000_001, 0x84)], &Evidence::ask(940_000_000, N), &fk(), 64), 0);
    accept(&ev_draft(&[liq_t(&l, N, LONG, 993_000_000, 0x84)], &Evidence::ask(940_000_000, N), &fk(), 64));
    refused(&ev_draft(&[liq_t(&l, N, SHORT, 1_007_499_999, 0x85)], &Evidence::bid(1_150_000_000, N), &fk(), 65), 0);
    accept(&ev_draft(&[liq_t(&l, N, SHORT, 1_007_500_000, 0x85)], &Evidence::bid(1_150_000_000, N), &fk(), 65));
}

// ================================================================ attacks on the fix (item 6)

/// The since = 0 sentinel versus a UTXO whose DAA score is 0 or unknown.
/// - Unknown: a mempool spend of an unaccepted parent reads the DAA score as u64::MAX, i.e. -1 in the script. Every
///   entry that uses the clock refuses it (amend, transfer, the evidence paths), so a mempool and consensus never read
///   a pair's clock differently; a pair with since non-zero does not read its UTXO's DAA and is unaffected.
/// - Zero: a pair UTXO of DAA 0 with since 0 has S = 0 and writes since = 0 into its continuation, whose clock then
///   restarts at its own UTXO. Only the genesis block has DAA 0 on a network and no pair can sit in it, so this exists
///   in the engine only (a holds_ test, not an exploit): shown with a GTD ask that died at DAA 700.
#[test]
fn holds_since_sentinel_against_unknown_and_zero_daa() {
    let unknown = at(&listed(), 0x5a, u64::MAX);
    refused(&amend_draft(&unknown, &lk(), LONG, (1_020_000_000, 0, 0), &fk(), 70), 0);
    let t = TransferSpec { side: LONG, n: HALF, x: 1_000_000_000, nb: 900_000_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    let mut td = transfer_draft(&live(&listed(), 0x5a), &t, &Entrant::Direct(keypair(16)), &fk(), 71);
    td.ins[0].utxo.block_daa_score = u64::MAX;
    td.lock = 2_400;
    refused(&td, 0);
    refused(&ev_draft(&[liq(&unknown, N, LONG)], &Evidence::ask(940_000_000, N), &fk(), 72), 0);
    let mut p = listed();
    p.since = 7_000;
    accept(&amend_draft(&at(&p, 0x5b, u64::MAX), &lk(), LONG, (1_020_000_000, 0, 0), &fk(), 73));
    // DAA 0.
    let z = at(&listed(), 0x5c, 0);
    let dead = with_expiry(placed(Evidence::ask(940_000_000, N), 100), 700);
    accept(&locked(&ev_draft(&[liq(&z, N, LONG)], &dead, &fk(), 74), 700));
    let d = amend_draft(&z, &lk(), LONG, (1_020_000_000, 0, 0), &fk(), 75);
    assert_eq!(d.pairs[0].1.since, 0, "S = 0 is written as the sentinel");
    let tx = accept(&d);
    let c = live_pairs(&tx, &d).remove(0).at(5_000);
    refused(&locked(&ev_draft(&[liq(&c, N, LONG)], &dead, &fk(), 74), 5_600), 0);
}

/// A pair created outside KpfOpen with a forged since in the future (S = 499,999,998,000, just below the lock-time
/// threshold): no evidence can rest on its clock for centuries (P5, P6 refused now) and no hand-over is ever possible
/// (S + 4R is past the lock-time threshold), so no KpfOpen
/// or other entrant can ever enter it. Its parties keep the evidence-free exits (amend keeps the clock; idle settle;
/// cooperative close). Such a pair can only be made by an author who funds both margins; it binds only that author's
/// own look-alike.
#[test]
fn holds_forged_future_since_freezes_evidence_and_hand_overs() {
    let mut p = listed();
    p.since = 499_999_998_000;
    let l = at(&p, 0x5d, 2_000);
    refused(&ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(940_000_000, N), &fk(), 76), 0);
    refused(&ev_draft(&[close(&l, N, LONG, lk())], &Evidence::bid(1_050_000_000, N), &fk(), 77), 0);
    let t = TransferSpec { side: LONG, n: HALF, x: 1_000_000_000, nb: 900_000_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    refused(&locked(&transfer_draft(&l, &t, &Entrant::Direct(keypair(16)), &fk(), 78), 499_999_999_999), 0);
    let d = amend_draft(&l, &lk(), LONG, (1_020_000_000, 0, 0), &fk(), 79);
    accept(&d);
    assert_eq!(d.pairs[0].1.since, 499_999_998_000);
    accept(&idle_draft(&l, &fk(), 80));
}

/// A pair created outside KpfOpen with a forged since in the far past (since = 1, on a UTXO of DAA 5,000,000): the
/// stale ask that died at DAA 1,600 counts on its clock and liquidates it. Who loses: only the parties of that
/// look-alike, whose margins its author funded (DESIGN 3.4: a look-alike can only give); `kpf` shows the clock. Any
/// KpfOpen (or signing taker) that takes a part of it gets since = 0, written by the order and by the pair, and the
/// stale ask is refused for that part.
#[test]
fn holds_forged_past_since_binds_only_the_look_alike() {
    let mut p = listed();
    p.since = 1;
    let l = at(&p, 0x5e, 5_000_000);
    let e = stale(own_ask(945_000_000, N, &sk()), 0);
    accept(&ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 81));
    let lo = order_for(&market(), LONG, 0x86);
    let t = TransferSpec { side: LONG, n: N, x: 1_000_000_000, nb: 900_000_000, ek: lo.o.owner, et: lo.id, k: 0 };
    let d = transfer_draft(&l, &t, &Entrant::Open(lo), &fk(), 82);
    let tx = accept(&d);
    let part = live_pairs(&tx, &d).remove(0).at(6_000_000);
    assert_eq!(part.p.since, 0);
    refused(&ev_draft(&[liq(&part, N, LONG)], &e, &sk(), 83), 0);
}

/// One lock time serves several lower bounds at once (the evidence rest on the pair's clock, the hand-over limit, an
/// order's activeFrom); each is checked by its own covenant against the same lock time, so satisfying one never
/// satisfies another: the transaction needs their maximum. T2 with an entrant whose activeFrom is 1,000 after the
/// hand-over limit; T2 with activeFrom before the limit; P7 with an entrant active 500 after the rest end.
#[test]
fn holds_one_lock_time_must_cover_every_bound() {
    let l = at(&listed(), 0x5f, 2_000);
    let limit = 2_000 + 4 * R;
    let mut lo = order_for(&market(), LONG, 0x87);
    lo.o.active_from = (limit + 1_000) as i64;
    let t = TransferSpec { side: LONG, n: HALF, x: 1_000_000_000, nb: 900_000_000, ek: lo.o.owner, et: lo.id, k: 0 };
    let d = transfer_draft(&l, &t, &Entrant::Open(lo.clone()), &fk(), 84);
    refused_only(&locked(&d, limit), 1, &[0]);
    accept(&locked(&d, limit + 1_000));
    lo.o.active_from = (limit - 1_000) as i64;
    let d = transfer_draft(&l, &t, &Entrant::Open(lo), &fk(), 85);
    refused_only(&locked(&d, limit - 1_000), 0, &[1]);
    accept(&locked(&d, limit));
    // P7: the evidence proves NOW, the entrant needs NOW + 500.
    let lv = live(&base(), 0x60);
    let mut e7 = order_for(&market(), LONG, 0x88);
    e7.o.active_from = (NOW + 500) as i64;
    let d = ev_draft(&[liq_to(&lv, N, LONG, 950_000_000, &e7)], &Evidence::ask(940_000_000, N), &fk(), 86);
    refused_only(&d, 3, &[0]);
    accept(&locked(&d, NOW + 500));
}
