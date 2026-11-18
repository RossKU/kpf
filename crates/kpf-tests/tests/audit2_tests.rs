//! Review 2 (docs/audit/REVIEW-2.md). The `exploit_*` tests of the review built transactions that the engine
//! accepted and that broke a stated guarantee. After the fixes (DESIGN 4.5b) each is a `holds_*` test: the
//! transaction is refused, or it is still valid and the former attacker gains nothing (each test says which).
//! The other `holds_*` tests record the new guarantees: a loser receives 0, the winner at least its payoff at the
//! trigger price, the evidence was fillable by conforming matchers for its whole rest window, and the winner
//! cannot trigger a liquidation the stated condition does not justify (review 2 follow-up, item 5).
//! Every input runs its real script, the KOB evidence order and the token program included.

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

const N: i64 = 1_000_000_000; // the whole pair: 1,000 tokens
const HALF: i64 = 400_000_000;
// The base pair (bl 9, bs 12 KAS, mmr 5 %): the long is liquidatable at an ask a with a * 9500 < bl * 10^4, i.e.
// a <= 947,368,421; the short at a bid b with bs * 10^4 < b * 10500, i.e. b >= 1,142,857,143.
const TOP_ASK: i64 = 947_368_421;
const LOW_BID: i64 = 1_142_857_143;
const MAX_IDLE: u64 = 77_760_000;

fn lk() -> Keypair {
    keypair(11)
}
fn sk() -> Keypair {
    keypair(12)
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
    let (tx, e): (Transaction, Vec<UtxoEntry>) = d.build();
    assert_input_fails(&tx, &e, i);
}

fn base() -> Pair {
    pair(&lk(), &sk())
}

fn liq(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side } }
}

fn close(l: &Live, n: i64, side: i64, signer: Keypair) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side, signer } }
}

/// A KpfOpen entrant of `side` on the pair's own market (the fixture orders: long rate 1 KAS, short rate 2 KAS).
fn entrant_for(p: &Pair, side: i64, tag: u8) -> LiveOpen {
    let mut o = if side == LONG { long_order(&keypair(21)) } else { short_order(&keypair(22)) };
    o.m = p.m.clone();
    LiveOpen { value: o.escrow() as u64, o, id: Hash::from_bytes([tag; 32]), op: outpoint(tag) }
}

fn liq_t(l: &Live, n: i64, side: i64, x: i64, k: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::LiqTransfer { side, x, open: entrant_for(&l.p, side, 0x71), k } }
}

/// Ask evidence whose maker and buyer are both `who` (one party on both sides of the fill).
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

fn long_out(d: &Draft) -> i64 {
    d.outs[0].value as i64
}

fn short_out(d: &Draft) -> i64 {
    d.outs[d.outs.len() - 1].value as i64
}

/// The winner's receipt of a P6 draft for a loser `side`.
fn winner_out(d: &Draft, side: i64) -> i64 {
    if side == LONG { short_out(d) } else { long_out(d) }
}

fn loser_out(d: &Draft, side: i64) -> i64 {
    if side == LONG { long_out(d) } else { short_out(d) }
}

// ---------------------------------------------------------------- F1: the loser chooses its own quote

/// F1 after the fix (DESIGN 4.5b). The review's transaction: the long of the DESIGN 5 pair, underwater at a
/// market of 8.5 KAS, rests its own ask at the top of its trigger window (9.4737 KAS), buys it itself and runs P6
/// as executor. Still valid, but the long's payout is 0 and the short receives the whole pair less the keeper fee;
/// the long's whole take is the keeper fee it collects as executor, which any keeper running the liquidation
/// would collect. Choosing the quote moves only that fee: 47.37 KAS at the inflated ask against 42.50 KAS for an
/// honest keeper at the market, against 473.68 KAS taken before the fix. The review's shape (the long paid
/// q(s, a - bl)) is refused.
#[test]
fn holds_self_liquidation_at_an_inflated_ask() {
    let l = live(&base(), 0x61);
    let honest = ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(850_000_000, N), &fk(), 1);
    accept(&honest);
    assert_eq!(long_out(&honest), 0);
    assert_eq!(short_out(&honest), l.value as i64 - liq_fee(&l.p, N, 850_000_000, LONG));
    let d = ev_draft(&[liq(&l, N, LONG)], &own_ask(TOP_ASK, N, &lk()), &lk(), 2);
    accept(&d);
    assert_eq!(long_out(&d), 0, "the loser receives nothing");
    let fee = liq_fee(&l.p, N, TOP_ASK, LONG);
    assert_eq!(short_out(&d), l.value as i64 - fee, "the winner receives the pair less the fee");
    assert!(short_out(&d) >= winner_at_trigger(&l.p, N, LONG));
    // Every input but the pair and every output but the short's is the long's: its net take is the fee.
    assert_eq!(l.value as i64 - short_out(&d), fee);
    assert_eq!(fee, 4_736_842_000);
    assert!(fee - liq_fee(&l.p, N, 850_000_000, LONG) < 50_000_000_000 / 100, "the quote moves only the fee");
    // The review's shape: the long takes q(s, a - bl) out of the pair.
    let stolen = q(N, TOP_ASK - l.p.bl, SCALE, 0);
    let mut old = d.clone();
    old.outs[0].value = stolen as u64;
    let m = old.outs.len();
    old.outs[m - 1].value = (l.p.range_of(N) - stolen) as u64;
    refused(&old, 0);
}

/// F1 mirrored after the fix: the short, underwater at 12.5 KAS, sells into its own bid at 11.4286 KAS. Valid; the
/// short receives 0, the long the pair less the fee; the short's take is the keeper fee only.
#[test]
fn holds_self_liquidation_at_a_deflated_bid() {
    let l = live(&base(), 0x62);
    let d = ev_draft(&[liq(&l, N, SHORT)], &own_bid(LOW_BID, N, &sk()), &sk(), 2);
    accept(&d);
    assert_eq!(short_out(&d), 0, "the loser receives nothing");
    let fee = liq_fee(&l.p, N, LOW_BID, SHORT);
    assert_eq!(long_out(&d), l.value as i64 - fee);
    assert!(long_out(&d) >= winner_at_trigger(&l.p, N, SHORT));
    let stolen = q(N, l.p.bs - LOW_BID, SCALE, 0);
    let mut old = d.clone();
    old.outs[0].value = (l.p.range_of(N) - stolen) as u64;
    let m = old.outs.len();
    old.outs[m - 1].value = stolen as u64;
    refused(&old, 0);
}

/// The trigger still bounds the window: one sompi above its top the pair refuses.
#[test]
fn holds_self_liquidation_needs_the_trigger() {
    let l = live(&base(), 0x63);
    refused(&ev_draft(&[liq(&l, N, LONG)], &own_ask(TOP_ASK + 1, N, &lk()), &lk(), 1), 0);
    refused(&ev_draft(&[liq(&l, N, SHORT)], &own_bid(LOW_BID - 1, N, &sk()), &sk(), 1), 0);
}

/// A loser that liquidates itself at any quote in its trigger window receives 0, whole or in part, by P6 or P7;
/// any value moved to it is refused.
#[test]
fn holds_self_liquidating_loser_receives_zero_at_any_quote() {
    let l = live(&base(), 0x64);
    for n in [N, HALF] {
        for a in [1, 500_000_000, l.p.bl - 1, l.p.bl, 920_000_000, TOP_ASK] {
            let d = ev_draft(&[liq(&l, n, LONG)], &own_ask(a, n, &lk()), &lk(), 3);
            accept(&d);
            assert_eq!(long_out(&d), 0, "P6 long at {a}, n {n}");
            // What leaves the pair to the long's other outputs (the executor's change) is exactly the fee.
            let rest: i64 = d.pairs.iter().map(|(i, _)| d.outs[*i].value as i64).sum();
            assert_eq!(l.value as i64 - short_out(&d) - rest, liq_fee(&l.p, n, a, LONG), "P6 long at {a}, n {n}");
            let mut x = d.clone();
            x.outs[0].value = 1;
            let m = x.outs.len();
            x.outs[m - 1].value -= 1;
            refused(&x, 0);
        }
        for b in [LOW_BID, 1_150_000_000, l.p.bs, l.p.bs + 1, 2_000_000_000] {
            let d = ev_draft(&[liq(&l, n, SHORT)], &own_bid(b, n, &sk()), &sk(), 4);
            accept(&d);
            assert_eq!(short_out(&d), 0, "P6 short at {b}, n {n}");
        }
        // P7 on the loser's own print: the pair pays the entrant's payment to the winner; paying it to the loser is
        // refused. With a third party's print and executor, no output at all goes to the loser's key.
        let d = ev_draft(&[liq_t(&l, n, LONG, 950_000_000, 0)], &own_ask(TOP_ASK, n, &lk()), &lk(), 5);
        accept(&d);
        assert_eq!(d.outs[0].script_public_key, p2pk_x(&l.p.short_key));
        let mut x = d.clone();
        x.outs[0].script_public_key = p2pk_x(&l.p.long_key);
        refused(&x, 0);
        let d = ev_draft(&[liq_t(&l, n, LONG, 950_000_000, 0)], &Evidence::ask(TOP_ASK, n), &fk(), 5);
        let tx = accept(&d);
        assert!(tx.outputs.iter().all(|o| o.script_public_key != p2pk_x(&l.p.long_key)), "P7: nothing paid to the loser");
        // Premise changed by review 3 (F3, DESIGN 4.5c): the short entrant must be healthy at the bid itself, so it enters
        // at x = 10.00000001 (bound 12.00000001), not at x = 10, where its bound 12 equals the loser's and the same
        // bid liquidates it.
        let xs = 1_000_000_001;
        let d = ev_draft(&[liq_t(&l, n, SHORT, xs, 0)], &own_bid(LOW_BID, n, &sk()), &sk(), 6);
        accept(&d);
        assert_eq!(d.outs[0].script_public_key, p2pk_x(&l.p.long_key));
        let mut x = d.clone();
        x.outs[0].script_public_key = p2pk_x(&l.p.short_key);
        refused(&x, 0);
        let d = ev_draft(&[liq_t(&l, n, SHORT, xs, 0)], &Evidence::bid(LOW_BID, n), &fk(), 6);
        let tx = accept(&d);
        assert!(tx.outputs.iter().all(|o| o.script_public_key != p2pk_x(&l.p.short_key)), "P7: nothing paid to the loser");
    }
}

/// Pairs at the mmrBps bounds (0, 1, 500, 4999): the base pair and one at the price bounds of DESIGN 5
/// (bl 9 * 10^13, bs = priceMax 10^14, one token).
fn mmr_pairs() -> Vec<Pair> {
    let mut v = vec![];
    for m in [0, 1, 500, 4999] {
        let mut p = base();
        p.m.mmr_bps = m;
        v.push(p.clone());
        let mut big = p;
        (big.size, big.bl, big.bs, big.entry) = (MIN_SIZE, 90_000_000_000_000, PRICE_MAX, 95_000_000_000_000);
        v.push(big);
    }
    v
}

/// Liquidatable quotes of a loser `side` of p, sorted from the most to the least favourable to the loser.
fn window(p: &Pair, side: i64) -> Vec<i64> {
    let m = p.m.mmr_bps;
    let mut v: Vec<i64> = if side == LONG {
        let top = ((p.bl * 10_000 - 1) / (10_000 - m)).min(PRICE_MAX);
        vec![top, (top + p.bl) / 2, p.bl, p.bl - 1, p.bl / 2, 1]
    } else {
        let bot = p.bs * 10_000 / (10_000 + m) + 1;
        vec![bot, (bot + p.bs) / 2, p.bs, p.bs + 1, (p.bs + PRICE_MAX) / 2, PRICE_MAX]
    };
    let (bl, bs) = (p.bl as i128, p.bs as i128);
    v.retain(|&x| {
        let x128 = x as i128;
        x >= 1 && x <= PRICE_MAX && if side == LONG { x128 * (10_000 - m as i128) < bl * 10_000 } else { bs * 10_000 < x128 * (10_000 + m as i128) }
    });
    v.dedup();
    v
}

/// The winner's receipt in P6 as a function of the quote, long and short, partial and whole, at the boundary
/// values of mmrBps and at the price bounds: it equals the liquidated value less the fee (so the quote enters only
/// through the fee) and is never below the winner's payoff at the trigger price. The loser receives 0. One sompi
/// less for the winner is refused. For a long loser the receipt never falls as the ask falls (moves against the
/// loser). For a short loser a higher bid (against the loser) raises the rate fee, so the receipt falls by
/// exactly that fee difference, never below the trigger payoff: the fee rule of DESIGN 4.5b is proportional to
/// the quote.
#[test]
fn holds_winner_receipt_depends_on_the_quote_only_through_the_fee() {
    let mut tag = 0x10u8;
    for p in mmr_pairs() {
        tag += 1;
        let l = live(&p, tag);
        if !p.range_ok() {
            // Premise changed by review 3 (F2, DESIGN 4.5c): a pair whose range is not wider than both maintenance
            // bands (here mmr 49.99 %) is not liquidatable at all, at any quote of either window.
            for side in [LONG, SHORT] {
                for quote in window(&p, side) {
                    let e = if side == LONG { Evidence::ask(quote, p.size) } else { Evidence::bid(quote, p.size) };
                    refused(&ev_draft(&[liq(&l, p.size, side)], &e, &fk(), 7), 0);
                }
            }
            continue;
        }
        for side in [LONG, SHORT] {
            for n in [p.size, (p.size / 2 / MIN_SIZE).max(1) * MIN_SIZE] {
                if n < p.size && p.size - n < MIN_SIZE {
                    continue;
                }
                let avail = if n == p.size { l.value as i64 } else { p.range_of(n) };
                let mut last = i64::MIN;
                for quote in window(&p, side) {
                    let e = if side == LONG { Evidence::ask(quote, n) } else { Evidence::bid(quote, n) };
                    let d = ev_draft(&[liq(&l, n, side)], &e, &fk(), 7);
                    accept(&d);
                    let w = winner_out(&d, side);
                    let what = format!("mmr {} side {side} n {n} quote {quote}", p.m.mmr_bps);
                    assert_eq!(loser_out(&d, side), 0, "{what}");
                    assert_eq!(w + liq_fee(&p, n, quote, side), avail, "{what}");
                    assert!(liq_fee(&p, n, quote, side) <= slice_of(&p, n, side), "{what}");
                    assert!(w >= winner_at_trigger(&p, n, side), "{what}");
                    if side == LONG {
                        assert!(w >= last, "{what}: the receipt fell as the ask fell");
                    }
                    last = w;
                    let mut x = d.clone();
                    if side == LONG {
                        let m = x.outs.len();
                        x.outs[m - 1].value -= 1;
                    } else {
                        x.outs[0].value -= 1;
                    }
                    refused(&x, 0);
                }
            }
        }
    }
}

/// The winner of P7, long and short, partial and whole, at the boundary values of mmrBps: it receives the
/// entrant's payment less the fee (never below 0), keeps its bound and key in the part, and that cash plus its
/// part's payoff at x is at least its payoff at the trigger price. The loser receives 0.
#[test]
fn holds_p7_winner_at_least_its_trigger_payoff() {
    let mut tag = 0x20u8;
    for m in [0, 1, 500, 4999] {
        let mut p = base();
        p.m.mmr_bps = m;
        tag += 1;
        let l = live(&p, tag);
        if !p.range_ok() {
            // Premise changed by review 3 (F2, DESIGN 4.5c): at mmr 49.99 % the base pair is not wider than both
            // maintenance bands, so neither side is liquidatable by transfer.
            refused(&ev_draft(&[liq_t(&l, N, LONG, 1_000_000_000, 0)], &Evidence::ask(window(&p, LONG)[0], N), &fk(), 8), 0);
            continue;
        }
        // The fixture entrants accept a long at x <= E = 10 KAS and a short at x >= 10 KAS.
        let x = 1_000_000_000;
        for (side, quotes) in [(LONG, [x.min(window(&p, LONG)[0]), 500_000_000]), (SHORT, [x.max(window(&p, SHORT)[0]), 2_000_000_000])] {
            for n in [N, HALF] {
                for quote in quotes {
                    let e = if side == LONG { Evidence::ask(quote, n) } else { Evidence::bid(quote, n) };
                    // Premise changed by review 3 (F3, DESIGN 4.5c): the entrant must be healthy at the quote, so x is
                    // also bounded by the entrant's health (the default price of the tool, `p7_default_x`).
                    let rate = if side == LONG { 100_000_000 } else { 200_000_000 };
                    let hb = kpf_protocol::evidence::entrant_health_bound(&p.m, side, quote, rate);
                    let xx = if side == LONG { x.max(quote).min(hb) } else { x.min(quote).max(hb) };
                    let d = ev_draft(&[liq_t(&l, n, side, xx, 0)], &e, &fk(), 8);
                    let tx = accept(&d);
                    let what = format!("mmr {m} side {side} n {n} quote {quote}");
                    let pay = p7_pay(&l, n, side, xx, quote);
                    let got = if pay < DUST { 0 } else { pay };
                    assert_eq!(d.outs[0].value as i64, got, "{what}");
                    assert_eq!(d.outs[0].script_public_key, p2pk_x(&winner_key(&p, side)), "{what}");
                    let loser = if side == LONG { p.long_key } else { p.short_key };
                    assert!(tx.outputs.iter().all(|o| o.script_public_key != p2pk_x(&loser)), "{what}");
                    let part = d.pairs.iter().find(|(_, q)| q.size == n && (q.long_key != p.long_key || q.short_key != p.short_key)).unwrap().1.clone();
                    let c = xx.clamp(p.bl, p.bs);
                    let at_x = if side == LONG {
                        assert_eq!((part.bs, part.short_key), (p.bs, p.short_key));
                        q(n, p.bs - c, SCALE, 0)
                    } else {
                        assert_eq!((part.bl, part.long_key), (p.bl, p.long_key));
                        q(n, c - p.bl, SCALE, 0)
                    };
                    assert!(pay < DUST || at_x + got >= winner_at_trigger(&p, n, side), "{what}");
                }
            }
        }
    }
}

// ---------------------------------------------------------------- F2: soft-expired orders as evidence

/// F2 after the fix: the review's transactions are refused. A healthy long cannot be liquidated on an ask that
/// expired when it was placed (expiryDaa = 1), and an IOC ask counts only while its kill is at or after the end of
/// the rest window.
#[test]
fn holds_expired_ask_is_not_rested_evidence() {
    let l = live(&base(), 0x64);
    let mut e = own_ask(940_000_000, N, &sk());
    e.ask.expiry_daa = 1;
    refused(&ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 1), 0);
    // An IOC ask killed one DAA before the end of its rest window (its custody arrived one DAA later).
    let mut ioc = own_ask(940_000_000, N, &sk());
    ioc.ask.tif = 1;
    ioc.tok_daa = ioc.daa + 1;
    refused(&ev_draft(&[liq(&l, N, LONG)], &ioc, &sk(), 2), 0);
}

/// F2 on P5 after the fix: the long cannot close on its own expired bid at bs.
#[test]
fn holds_expired_bid_cannot_close_the_long_at_the_whole_range() {
    let l = live(&base(), 0x65);
    let mut e = own_bid(l.p.bs, N, &lk());
    e.bid.expiry_daa = 1;
    refused(&ev_draft(&[close(&l, N, LONG, lk())], &e, &lk(), 1), 0);
}

/// The end of the rest window in the fixtures: the order and its custody are DAA `daa`, interval 0, activeFrom 0.
fn rest_end(e: &Evidence) -> i64 {
    e.daa as i64 + rest_daa() as i64
}

/// expiryDaa one DAA inside (exactly the end of the rest window) and one outside, for an ask (P6) and a bid (P5).
#[test]
fn holds_expiry_boundary() {
    let l = live(&base(), 0x66);
    let mut e = Evidence::ask(940_000_000, N);
    e.ask.expiry_daa = rest_end(&e);
    accept(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 1));
    e.ask.expiry_daa -= 1;
    refused(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 1), 0);
    let mut e = Evidence::bid(1_050_000_000, N);
    e.bid.expiry_daa = rest_end(&e);
    accept(&ev_draft(&[close(&l, N, LONG, lk())], &e, &fk(), 2));
    e.bid.expiry_daa -= 1;
    refused(&ev_draft(&[close(&l, N, LONG, lk())], &e, &fk(), 2), 0);
}

/// The IOC/FOK kill max(UTXO DAA, activeFrom) + 600 one DAA inside and one outside the rest window (R = 600), for
/// IOC and FOK asks (P6 of a long) and bids (P6 of a short). The window is moved by one DAA without moving the
/// kill: for an ask by its custody arriving one DAA after the order, for a bid by an interval of 1 (exposed =
/// UTXO DAA + interval; the bid's own CSV for the interval is given). activeFrom moves the kill with the window,
/// so a later activeFrom stays inside. A GTC order has no kill.
#[test]
fn holds_ioc_and_fok_kill_boundary() {
    let l = live(&base(), 0x67);
    assert_eq!(rest_daa(), 600, "the kill equals R in this market");
    for tif in [1, 2] {
        let mut e = Evidence::ask(940_000_000, N);
        e.ask.tif = tif;
        accept(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 1));
        e.tok_daa += 1;
        refused(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 1), 0);
        let mut e = Evidence::ask(940_000_000, N);
        e.ask.tif = tif;
        e.ask.active_from = e.daa as i64 + 10;
        accept(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 1));
        let mut e = Evidence::ending_bid(1_150_000_000, N, tif);
        accept(&ev_draft(&[liq(&l, N, SHORT)], &e, &fk(), 2));
        e.bid.interval = 1;
        let mut d = ev_draft(&[liq(&l, N, SHORT)], &e, &fk(), 2);
        d.seqs = vec![0, 1];
        let (tx, en) = d.build();
        assert!(run(&tx, &en).unwrap()[1].is_ok(), "the bid itself fills");
        assert_input_fails(&tx, &en, 0);
    }
    let mut e = Evidence::ask(940_000_000, N);
    e.tok_daa += 1;
    accept(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 3));
}

/// The 90-day idle limit: the rest window ending exactly 77,760,000 DAA after the order's UTXO is accepted, one
/// DAA later is refused (the window is moved with activeFrom; the lock time is past both).
#[test]
fn holds_ninety_day_idle_boundary() {
    let l = live(&base(), 0x68);
    let r = rest_daa();
    for side in [LONG, SHORT] {
        for (af, good) in [(1_000 + MAX_IDLE - r, true), (1_000 + MAX_IDLE - r + 1, false)] {
            let mut e = if side == LONG { Evidence::ask(940_000_000, N) } else { Evidence::bid(1_150_000_000, N) };
            (e.daa, e.tok_daa) = (1_000, 1_000);
            e.ask.active_from = af as i64;
            e.bid.active_from = af as i64;
            let mut d = ev_draft(&[liq(&l, N, side)], &e, &fk(), 4);
            d.lock = 1_000 + MAX_IDLE + 10;
            if good {
                accept(&d);
            } else {
                refused(&d, 0);
            }
        }
    }
}

// ---------------------------------------------------------------- item 5: the winner triggers what it should not

/// The wrong side of the evidence: a long is liquidated only on an ask and a short only on a bid. A bid below the
/// long's trigger (the long's adverse bound, which proves nothing about an upper bound) and an ask above the
/// short's are refused, as is a liquidation naming the other side as loser.
#[test]
fn holds_winner_cannot_use_the_wrong_side_of_the_evidence() {
    let l = live(&base(), 0x69);
    refused(&ev_draft(&[liq(&l, N, LONG)], &Evidence::bid(800_000_000, N), &sk(), 1), 0);
    refused(&ev_draft(&[liq(&l, N, SHORT)], &Evidence::ask(1_300_000_000, N), &lk(), 1), 0);
    // An ask that liquidates the long does not liquidate the short (side swapped), and vice versa.
    refused(&ev_draft(&[liq(&l, N, SHORT)], &Evidence::ask(940_000_000, N), &lk(), 1), 0);
    refused(&ev_draft(&[liq(&l, N, LONG)], &Evidence::bid(1_150_000_000, N), &sk(), 1), 0);
    // A side value other than 1 or 2.
    let mut d = ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(940_000_000, N), &sk(), 1);
    let mut a = d.args(0);
    a[4] = iv(3);
    d.set_args(0, a);
    refused(&d, 0);
}

/// A bid posing as an ask: a KobBid instance (or the bid's look-alike KobAsk template) at the evidence index of a
/// long liquidation is read with KobAsk's template and refused; an ask whose custody is not its own is refused.
#[test]
fn holds_winner_cannot_pass_a_bid_or_a_look_alike_as_an_ask() {
    let l = live(&base(), 0x6a);
    let mut e = Evidence::ask(940_000_000, N);
    e.art = Some(fake_ask_base().clone());
    refused(&ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 1), 0);
    let mut e = Evidence::ask(940_000_000, N);
    e.custody_owner = Some(Hash::from_bytes([0x44; 32]));
    refused(&ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 1), 0);
}

/// A quote read from a different market: another token, scale or extension commitment, for both liquidations.
#[test]
fn holds_winner_cannot_use_a_quote_of_another_market() {
    let l = live(&base(), 0x6b);
    let other = Hash::from_bytes([0x77; 32]);
    let mut e = Evidence::ask(940_000_000, N);
    (e.ask.token_cov_id, e.tok_cov) = (other, other);
    refused(&ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 1), 0);
    let mut e = Evidence::ask(940_000_000, N);
    e.ask.scale = SCALE * 10;
    refused(&ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 1), 0);
    let mut e = Evidence::ask(940_000_000, N);
    (e.ask.ext, e.tok_ext) = (other, other);
    refused(&ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 1), 0);
    let mut e = Evidence::bid(1_150_000_000, N);
    (e.bid.token_cov_id, e.tok_cov) = (other, other);
    refused(&ev_draft(&[liq(&l, N, SHORT)], &e, &lk(), 1), 0);
    let mut e = Evidence::bid(1_150_000_000, N);
    e.bid.scale = SCALE * 10;
    refused(&ev_draft(&[liq(&l, N, SHORT)], &e, &lk(), 1), 0);
    let mut e = Evidence::bid(1_150_000_000, N);
    (e.bid.ext, e.tok_ext) = (other, other);
    refused(&ev_draft(&[liq(&l, N, SHORT)], &e, &lk(), 1), 0);
}

/// Evidence of another block: one fill of n cannot liquidate two pairs (the block must be covered k times), a pair
/// that names a block it is not in is refused, and an evidence index that points at another pair (not an order)
/// is refused.
#[test]
fn holds_winner_cannot_reuse_evidence_of_another_block() {
    let (a, b) = (live(&base(), 0x6c), live(&base(), 0x6d));
    let d = ev_draft(&[liq(&a, N, LONG), liq(&b, N, LONG)], &Evidence::ask(940_000_000, N), &sk(), 1);
    refused(&d, 0);
    refused(&d, 1);
    let mut gb = liq(&b, N, LONG);
    gb.q = 1;
    let d = ev_draft(&[liq(&a, N, LONG), gb], &Evidence::ask(940_000_000, N), &sk(), 1);
    refused(&d, 0);
    // ev = the other pair's index.
    let mut d = ev_draft(&[liq(&a, N, LONG), liq(&b, N, LONG)], &Evidence::ask(940_000_000, 2 * N), &sk(), 1);
    let mut x = d.args(0);
    x[5] = iv(1);
    d.set_args(0, x);
    refused(&d, 0);
}

/// The boundary of the trigger inequality and the rounding of T. Where bl * 10^4 / (10^4 - mmrBps) is exact
/// (bl 9 KAS, mmr 10 %: 10 KAS) the strict inequality refuses a = T and accepts T - 1; where it is not (mmr 5 %)
/// the largest liquidatable ask is floor(T). Mirrored for a short (bs 12 KAS, mmr 20 %: 10 KAS exact). The rounding
/// of T only moves the fee cap, and always in the winner's favour: the cap is at most the exact slice.
#[test]
fn holds_trigger_boundary_and_rounding_of_t() {
    let mut p = base();
    p.m.mmr_bps = 1_000;
    let l = live(&p, 0x6e);
    assert_eq!(trigger_of(&p, LONG), 1_000_000_000);
    refused(&ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(1_000_000_000, N), &sk(), 1), 0);
    let d = ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(999_999_999, N), &sk(), 1);
    accept(&d);
    assert!(short_out(&d) >= q(N, p.bs - 1_000_000_000, SCALE, 0) + C);
    let mut p = base();
    p.m.mmr_bps = 2_000;
    // Premise changed by review 3 (F2, DESIGN 4.5c): at 20 % the base pair (bl 9) is narrower than both maintenance
    // bands (9 * 1.2 > 12 * 0.8) and not liquidatable; bl 7 keeps the exact short trigger of 10 KAS inside the range.
    p.bl = 700_000_000;
    let l = live(&p, 0x6f);
    assert_eq!(trigger_of(&p, SHORT), 1_000_000_000);
    refused(&ev_draft(&[liq(&l, N, SHORT)], &Evidence::bid(1_000_000_000, N), &lk(), 1), 0);
    let d = ev_draft(&[liq(&l, N, SHORT)], &Evidence::bid(1_000_000_001, N), &lk(), 1);
    accept(&d);
    assert!(long_out(&d) >= q(N, 1_000_000_000 - p.bl, SCALE, 0) + C);
    // Not exact: floor for a long, ceil for a short; both quotes at T are liquidatable.
    let l = live(&base(), 0x70);
    assert_eq!((trigger_of(&l.p, LONG), trigger_of(&l.p, SHORT)), (TOP_ASK, LOW_BID));
    accept(&ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(TOP_ASK, N), &sk(), 1));
    accept(&ev_draft(&[liq(&l, N, SHORT)], &Evidence::bid(LOW_BID, N), &lk(), 1));
    // Premise changed by review 3: T is no longer clamped, so the slices are bounded only for the pairs that can be
    // liquidated at all (range wider than both maintenance bands).
    for p in mmr_pairs().into_iter().filter(|p| p.range_ok()) {
        let m = p.m.mmr_bps as i128;
        let (bl, bs, s) = (p.bl as i128, p.bs as i128, p.size as i128);
        let sc = p.m.scale as i128;
        // Exact slices in rationals, scaled by (10^4 - m) * scale: s * (T* - bl) with T* = bl * 10^4 / (10^4 - m).
        let exact_long = s * bl * m; // = s * (bl * 10^4 - bl * (10^4 - m))
        assert!((slice_of(&p, p.size, LONG) as i128) * (10_000 - m) * sc <= exact_long.min(s * (bs - bl) * (10_000 - m)));
        let exact_short = s * bs * m; // s * (bs - T*) * (10^4 + m)
        assert!((slice_of(&p, p.size, SHORT) as i128) * (10_000 + m) * sc <= exact_short.min(s * (bs - bl) * (10_000 + m)));
    }
}

// ---------------------------------------------------------------- close-versus-close halves

/// The donor half of P4 run without any host: its long moves into a fresh pair created by an outsider's genesis,
/// whose short (the outsider) posts the margin. The engine accepts it, but it is exactly a transfer of the donor's
/// short listing: the short leaves at x <= shortBid, the long keeps its key, bound and listing, faces bs3 >= bs and
/// e3 >= E. Its payoff at every price and its idle payout do not fall. No finding.
#[test]
fn holds_close_donor_half_alone_is_only_a_transfer() {
    let mut p = base();
    (p.short_bid, p.short_min, p.short_tip) = (1_100_000_000, 0, 0);
    let donor = live(&p, 0x66);
    let out_kp = keypair(30);
    let (x, bs3, e3) = (1_100_000_000, 1_300_000_000, 1_250_000_000);
    let mut p3 = p.cleared();
    p3.bs = bs3;
    p3.short_key = xonly(&out_kp);
    (p3.short_bid, p3.short_min, p3.short_tip) = (0, 0, 0);
    p3.tag_l = donor.id;
    p3.entry = e3;
    let pay = owed(N, p.bs - x, 0, SCALE) + (donor.value as i64 - p.range_of(N));
    let args = vec![iv(N), iv(x), iv(e3), iv(1), iv(bs3), bv(&xonly(&out_kp)), iv(0), iv(0), iv(0)];
    let ins = vec![pair_input(&donor, "closeDonor", args), funder(&out_kp, 7, F_FUNDS)];
    let outs = vec![
        out(pay as u64, p2pk_x(&p.short_key), None),
        out(p3.floor() as u64, p2sh(&p3.art()), None),
        out(0, p2pk_spk(&out_kp), None),
    ];
    let mut d = Draft::new(ins, outs, Some(2));
    d.genesis.push((1, 1, fop(7)));
    // The donor's short hands over: it waits for the pair's hand-over limit S + 4R (review 3, DESIGN 4.5c).
    d.lock = donor.handover_from();
    accept(&d);
    for price in [0, 800_000_000, 950_000_000, 1_000_000_000, 1_200_000_000, 1_250_000_000, 2_000_000_000] {
        assert!(p3.long_at(price) >= p.long_at(price), "long worse off at {price}");
    }
    let idle = |pp: &Pair| q(pp.size, pp.entry.clamp(pp.bl, pp.bs) - pp.bl, SCALE, 0);
    assert!(idle(&p3) >= idle(&p));
    // Mutation: an e3 below the donor's entry is refused (the long's idle payout would fall).
    let mut low = d.clone();
    let mut p3l = p3.clone();
    p3l.entry = p.entry - 1;
    low.set_args(0, vec![iv(N), iv(x), iv(p.entry - 1), iv(1), iv(bs3), bv(&xonly(&out_kp)), iv(0), iv(0), iv(0)]);
    low.outs[1].script_public_key = p2sh(&p3l.art());
    refused(&low, 0);
}
