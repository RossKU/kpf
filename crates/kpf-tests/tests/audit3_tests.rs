//! Review 3 (docs/audit/REVIEW-3.md). Each `exploit_*` test builds a transaction (or a short sequence) that the
//! engine accepts and that breaks a stated guarantee; its comment says who loses what. Each `holds_*` test records
//! an attack that the contracts refuse, or that is valid but harms nobody. Every input runs its real script: the
//! pairs, the KpfOpen orders, the KOB evidence order and the token program.

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
    let (tx, e) = d.build();
    assert_input_fails(&tx, &e, i);
}

/// The DESIGN 5 pair: 1,000 tokens, bl 9, bs 12, E 10 KAS, mmr 5 % (long trigger 9.4737, short 11.4286).
fn base() -> Pair {
    pair(&lk(), &sk())
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

fn liq_t(l: &Live, n: i64, side: i64, x: i64, tag: u8) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::LiqTransfer { side, x, open: order_for(&l.p.m, side, tag), k: 0 } }
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

fn long_out(d: &Draft) -> i64 {
    d.outs[0].value as i64
}

fn short_out(d: &Draft) -> i64 {
    d.outs[d.outs.len() - 1].value as i64
}

/// An evidence order (and an ask's custody) placed at DAA 1,000 with a soft expiry exactly at the end of its rest
/// window (1,000 + R = 1,600): an IOC order (kill at max(UTXO DAA, activeFrom) + 600) when `tif` = 1, a GTD order
/// (expiryDaa = 1,600) when `tif` = 0. From DAA 1,600 on, KOB's conforming matchers no longer fill it and the
/// reference indexer leaves it out of every book (matcher.md 2.3), and its refundTip is 0, so no refund keeper
/// removes it (matcher.md 5).
fn stale(mut e: Evidence, tif: i64) -> Evidence {
    (e.daa, e.tok_daa) = (1_000, 1_000);
    let end = 1_000 + rest_daa() as i64;
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

/// Sets the UTXO DAA of the pair at input `i` (the DAA at which the pair was created).
fn pair_born_at(d: &mut Draft, i: usize, daa: u64) {
    d.ins[i].utxo.block_daa_score = daa;
}

// ---------------------------------------------------------------- R3-F1 (Critical): stale evidence

/// R3-F1. A quote that rested for R and then went past its soft expiry stays usable as evidence forever.
///
/// The attacker (the short) rests its own IOC ask at 9.45 KAS when the market is at or below 9.45, so nobody
/// takes it: it costs nothing. At DAA 1,600 the IOC kill passes; from then on no conforming matcher fills it and no
/// book shows it, so the arbitrage that DESIGN 3.3 and THREAT_MODEL A1 rely on can never reach it again, and with
/// refundTip 0 no keeper refunds it. Much later (lock time 10,000,000) the market is at 10 KAS and a long opens at
/// 10 with bl 9 (trigger 9.4737): healthy at the true market, and it never was liquidatable during its life (its
/// UTXO is from DAA 5,000,000). The short fills its own dead ask and liquidates it. `evQuote` checks the soft
/// expiry only against the end of the rest window, and lock time only bounds tx.daa from below, so nothing in the
/// transaction can show that the quote is stale.
///
/// Who loses: the long, its whole margin (1,000 KAS), to the short (the pair less the keeper fee, which the short
/// also collects as executor). The same holds for a GTD order (expiryDaa at the end of the window) and for a GTC
/// order past 90 days idle.
#[test]
fn exploit_stale_ask_liquidates_a_pair_opened_after_it_died() {
    let l = live(&base(), 0x31);
    for tif in [1, 0] {
        let e = stale(own_ask(945_000_000, N, &sk()), tif);
        let mut d = ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 1);
        pair_born_at(&mut d, 0, 5_000_000);
        assert!(d.lock > 1_600 + 1_000_000, "the quote has been dead for millions of DAA");
        accept(&d);
        assert_eq!(long_out(&d), 0, "tif {tif}: the healthy long forfeits everything");
        assert_eq!(short_out(&d), l.value as i64 - liq_fee(&l.p, N, 945_000_000, LONG));
        // At the true market (10 KAS) the long is owed 1,000 KAS and is not liquidatable.
        assert_eq!(l.p.long_at(1_000_000_000), 100_000_000_000);
        assert!(!kpf_protocol::payoff::long_liquidatable(&l.p, 1_000_000_000));
    }
    // A GTC ask past 90 days idle: the rest window ended inside the 90 days, the fill comes a year later.
    let mut e = own_ask(945_000_000, N, &sk());
    (e.daa, e.tok_daa) = (1_000, 1_000);
    let mut d = ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 2);
    d.lock = 1_000 + 4 * MAX_IDLE;
    accept(&d);
    assert_eq!(long_out(&d), 0);
}

/// R3-F1 on P5. The long rests its own bid at 12 KAS (= bs) while the market is at or above 12, lets it die at the
/// end of its rest window, and closes on it months later when the market is at 10. It is paid the whole range; the
/// short, owed 2,000 KAS at the true market, keeps only the carrier.
#[test]
fn exploit_stale_bid_closes_the_long_at_the_whole_range() {
    let l = live(&base(), 0x32);
    for tif in [0, 1] {
        let e = if tif == 0 { stale(own_bid(l.p.bs, N, &lk()), 0) } else { stale(Evidence::ending_bid(l.p.bs, N, 1), 1) };
        let mut e = e;
        e.maker = lk();
        e.bid.maker = xonly(&lk());
        e.counter = lk();
        let mut d = ev_draft(&[close(&l, N, LONG, lk())], &e, &lk(), 3);
        pair_born_at(&mut d, 0, 5_000_000);
        accept(&d);
        assert_eq!(long_out(&d), l.p.range_of(N), "tif {tif}: the long takes the whole range");
        assert_eq!(short_out(&d), C, "tif {tif}: the short keeps the carrier only");
        assert_eq!(l.p.short_at(1_000_000_000), 200_000_000_000);
    }
}

/// R3-F1, the same quote still visible: the attack needs the quote to be dead. A GTC ask whose 90 days are not
/// over and whose expiry is far ahead is still in every book, so a mispriced one is crossed by matchers (the cost
/// the design relies on). This is the control: the transaction is the same, only the order's own soft expiry
/// differs, and the covenant accepts both.
#[test]
fn holds_live_quote_is_the_control_of_r3_f1() {
    let l = live(&base(), 0x33);
    let mut e = own_ask(945_000_000, N, &sk());
    (e.daa, e.tok_daa) = (1_000, 1_000);
    accept(&ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 4));
}

// ---------------------------------------------------------------- R3-F2 (High): the winner forfeits

/// R3-F2. When a pair's range is narrower than its maintenance band, the side that holds the whole pair at the true
/// market is "liquidatable", and DESIGN 4.5b makes it forfeit everything to the bankrupt side.
///
/// Market mmrBps = 3,000 (inside the documented bound 0..5000), the DESIGN 5 pair (bl 9, bs 12). The long's
/// trigger bl / 0.7 = 12.857 lies above bs. The true market is 12.5: the short is bankrupt, the long is owed the
/// whole range (3,000 KAS). An honest third party's ask at the market (12.5) satisfies 12.5 * 0.7 < 9, so the
/// bankrupt short runs P6 on the long. Before review 2 the long was valued at clamp(12.5) = bs and kept 3,000
/// KAS; now it gets 0 and the short takes the pair less the fee.
#[test]
fn exploit_profitable_long_forfeits_inside_the_maintenance_band() {
    let mut p = base();
    p.m.mmr_bps = 3_000;
    let l = live(&p, 0x34);
    let a = 1_250_000_000;
    assert!(a > p.bs && kpf_protocol::payoff::long_liquidatable(&p, a));
    let d = ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(a, N), &sk(), 5);
    accept(&d);
    assert_eq!(p.long_at(a), p.range_of(N), "at the true market the long holds the whole pair");
    assert_eq!(long_out(&d), 0, "and it receives nothing");
    assert_eq!(short_out(&d), l.value as i64 - liq_fee(&p, N, a, LONG), "the bankrupt short takes the pair");
}

/// R3-F2 mirrored: mmrBps = 4,999, the DESIGN 5 pair. The short's trigger 12 / 1.4999 = 8.0006 lies below bl. The
/// true market is 8.5: the long is bankrupt, the short is owed the whole range. The bankrupt long liquidates the
/// short at an honest bid of 8.5 and takes the pair.
#[test]
fn exploit_profitable_short_forfeits_inside_the_maintenance_band() {
    let mut p = base();
    p.m.mmr_bps = 4_999;
    let l = live(&p, 0x35);
    let b = 850_000_000;
    assert!(b < p.bl && kpf_protocol::payoff::short_liquidatable(&p, b));
    let d = ev_draft(&[liq(&l, N, SHORT)], &Evidence::bid(b, N), &lk(), 6);
    accept(&d);
    assert_eq!(p.short_at(b), p.range_of(N));
    assert_eq!(short_out(&d), 0);
    assert_eq!(long_out(&d), l.value as i64 - liq_fee(&p, N, b, SHORT));
}

/// R3-F2 in the default market (mmrBps 500): a high-leverage pair, bl 9.6, bs 10.0 (range 0.4 < 5 % of bl). Its
/// short's trigger 10 / 1.05 = 9.5238 is below bl. True market 9.55: the long is bankrupt, the short is owed the
/// whole 400 KAS. The long liquidates the short at an honest bid of 9.55 and takes it.
#[test]
fn exploit_profitable_short_forfeits_on_a_narrow_pair() {
    let mut p = base();
    (p.bl, p.bs, p.entry) = (960_000_000, 1_000_000_000, 980_000_000);
    let l = live(&p, 0x36);
    let b = 955_000_000;
    assert!(b < p.bl && kpf_protocol::payoff::short_liquidatable(&p, b));
    let d = ev_draft(&[liq(&l, N, SHORT)], &Evidence::bid(b, N), &lk(), 7);
    accept(&d);
    assert_eq!(p.short_at(b), 40_000_000_000);
    assert_eq!(short_out(&d), 0);
    assert_eq!(long_out(&d), l.value as i64 - liq_fee(&p, N, b, SHORT));
}

/// R3-F2 boundary: the hole needs the trigger outside the range. In the DESIGN 5 pair at mmrBps 500 an ask above bs
/// never liquidates the long, and a bid below bl never liquidates the short.
#[test]
fn holds_trigger_inside_the_range_protects_the_winner() {
    let l = live(&base(), 0x37);
    refused(&ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(1_250_000_000, N), &sk(), 8), 0);
    refused(&ev_draft(&[liq(&l, N, SHORT)], &Evidence::bid(850_000_000, N), &lk(), 8), 0);
}

// ---------------------------------------------------------------- R3-F3 (Medium): P7 entrant born liquidatable

/// R3-F3. Liquidation by transfer may hand the loser's part to a resting long order with exactly the loser's
/// bankruptcy price, so the entrant is liquidatable by the very quote of the transaction, and under DESIGN 4.5b it
/// then forfeits its whole margin.
///
/// The pair's long is liquidatable at an ask of 9.4 (true market). The executor fills the fixture long order (limit
/// 10.5, rate 1, its terms all respected: x = 10 <= limit, counter rate 2 >= 1.5, E = 10 >= x) at x_open = 10, so
/// its bound is 9.0 = the loser's bl, while the pair's own x is 9.4 (x >= the ask): the winner receives
/// q(n, 0.4) - fee and the executor keeps the 600 KAS between the two prices. In the next transaction a second
/// resting ask at 9.4 liquidates the entrant, and the short takes the entrant's 1,000 KAS margin too. The
/// reference tool picks this x by default (`p7_default_x` takes min(limit, bl + myRate, ...)), see R3-T1.
///
/// Who loses: the KpfOpen maker, its whole margin (1,000 KAS) plus its tip, one transaction after it entered at a
/// price the same transaction proved to be 6 % above the market.
#[test]
fn exploit_p7_enters_a_kpfopen_that_the_same_quote_liquidates() {
    let l = live(&base(), 0x38);
    let quote = 940_000_000;
    let mut d = ev_draft(&[liq_t(&l, N, LONG, 1_000_000_000, 0x71)], &Evidence::ask(quote, N), &fk(), 9);
    // The pair's x is the ask; the order still takes at its own x = 10.
    let mut a = d.args(0);
    a[ARG_X] = iv(quote);
    d.set_args(0, a);
    d.outs[0].value = p7_pay(&l, N, LONG, quote, quote) as u64;
    let tx = accept(&d);
    let part = live_pairs(&tx, &d).into_iter().find(|x| x.p.long_key != l.p.long_key).expect("the entrant's part");
    assert_eq!(part.p.bl, l.p.bl, "the entrant holds exactly the loser's bankruptcy price");
    assert!(kpf_protocol::payoff::long_liquidatable(&part.p, quote), "and the same quote liquidates it");
    // The next transaction: another resting ask at the same price.
    let mut e2 = Evidence::ask(quote, N);
    e2.id = Hash::from_bytes([0xe5; 32]);
    let d2 = ev_draft(&[liq(&part, N, LONG)], &e2, &sk(), 10);
    let tx2 = accept(&d2);
    assert_eq!(long_out(&d2), 0, "the entrant forfeits its margin");
    let owner = p2pk_x(&part.p.long_key);
    assert!(tx2.outputs.iter().all(|o| o.script_public_key != owner || o.value == 0), "nothing is paid to the entrant's maker");
    // What the maker paid into the part: its margin q(n, myRate) = 1,000 KAS (plus its tip and a carrier).
    let entrant = order_for(&l.p.m, LONG, 0x71);
    assert_eq!(q(N, entrant.o.my_rate, SCALE, 0), 100_000_000_000);
}

/// R3-F3 control: the no-downgrade rule bounds the entrant at the loser's bl. An entrant bound above it (x_open =
/// 10.5 with rate 1, bound 9.5) is refused by the pair.
#[test]
fn holds_p7_entrant_bound_never_above_the_losers() {
    let l = live(&base(), 0x39);
    refused(&ev_draft(&[liq_t(&l, N, LONG, 1_050_000_000, 0x72)], &Evidence::ask(940_000_000, N), &fk(), 11), 0);
}

// ---------------------------------------------------------------- R3-F4 (Low): an order filled at its limit

/// R3-F4. T1 lets the executor choose x anywhere between the two limits. Filling the fixture long order (limit
/// 10.5, rate 1 = 10 % margin) at its limit puts its bl at 9.5 and its trigger at 10.0. With the market at 10.0
/// (an honest ask at 9.99), the long is liquidatable at once and forfeits its whole 1,000 KAS margin to the short
/// order. Before DESIGN 4.5b it kept q(n, 9.99 - 9.5) = 490 KAS; the spot analogue (a bid filled at its limit 5 %
/// above the market) loses 510. KpfOpen has no check that its own side is not already liquidatable at its fill
/// price, and the reference tool does not warn (R3-T1).
#[test]
fn exploit_order_filled_at_its_limit_forfeits_its_margin_at_once() {
    let lo = order_for(&market(), LONG, 0x73);
    let mut so = order_for(&market(), SHORT, 0x74);
    so.o.size_left = N;
    so.value = so.o.escrow() as u64;
    let x = lo.o.limit;
    let d = open_draft(&lo, &so, N, x, &fk(), 12);
    let tx = accept(&d);
    let pl = live_pairs(&tx, &d).remove(0);
    assert_eq!((pl.p.bl, pl.p.bs), (950_000_000, 1_250_000_000));
    let a = 999_000_000;
    assert!(kpf_protocol::payoff::long_liquidatable(&pl.p, a));
    let d2 = ev_draft(&[liq(&pl, N, LONG)], &Evidence::ask(a, N), &fk(), 13);
    accept(&d2);
    assert_eq!(long_out(&d2), 0);
}

// ---------------------------------------------------------------- refused or harmless

/// One evidence serving a unilateral close (a short at an ask) and a liquidation (a long at the same ask) must
/// cover k times both parts: with n_ev = n both pairs are refused, with 2n both are accepted.
#[test]
fn holds_one_evidence_for_a_close_and_a_liquidation_counts_both() {
    let (a, b) = (live(&base(), 0x3a), live(&base(), 0x3b));
    let legs = [close(&a, N, SHORT, sk()), liq(&b, N, LONG)];
    let d = ev_draft(&legs, &Evidence::ask(940_000_000, N), &fk(), 14);
    refused(&d, 0);
    refused(&d, 1);
    accept(&ev_draft(&legs, &Evidence::ask(940_000_000, 2 * N), &fk(), 14));
}

/// Which DAA counts for an evidence order that continues a partial fill: its own UTXO DAA (the continuation is a
/// new UTXO) and, for an ask, its new custody's DAA. A continuation or a custody one DAA short of R is refused.
#[test]
fn holds_fresh_continuation_or_custody_restarts_the_rest() {
    let l = live(&base(), 0x3c);
    let r = rest_daa();
    let mut e = Evidence::ask(940_000_000, N);
    (e.daa, e.tok_daa) = (NOW - r, NOW - r);
    accept(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 15));
    e.daa = NOW - r + 1;
    refused(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 15), 0);
    let mut e = Evidence::ask(940_000_000, N);
    (e.daa, e.tok_daa) = (NOW - r, NOW - r + 1);
    refused(&ev_draft(&[liq(&l, N, LONG)], &e, &fk(), 15), 0);
    let mut e = Evidence::bid(1_150_000_000, N);
    e.daa = NOW - r + 1;
    refused(&ev_draft(&[liq(&l, N, SHORT)], &e, &fk(), 16), 0);
}

/// mmrBps = 0: a long is liquidatable only below bl (bankrupt), the slice is 0, so the keeper fee is 0 and the
/// short receives the whole pair.
#[test]
fn holds_mmr_zero_liquidates_only_the_bankrupt_and_pays_no_fee() {
    let mut p = base();
    p.m.mmr_bps = 0;
    let l = live(&p, 0x3d);
    refused(&ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(p.bl, N), &sk(), 17), 0);
    let d = ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(p.bl - 1, N), &sk(), 17);
    accept(&d);
    assert_eq!(short_out(&d), l.value as i64);
}

/// Partial liquidation in minimum pieces: each piece pays at most its own maintenance slice, and the winner of
/// every piece receives at least its payoff at the trigger, whatever keeperFeeMin is (here 5 KAS per one-token
/// piece, far above the 0.47 KAS slice).
#[test]
fn holds_minimum_pieces_never_charge_the_winner_beyond_the_slice() {
    let mut p = base();
    p.m.keeper_fee_min = 500_000_000;
    let l = live(&p, 0x3e);
    let piece = MIN_SIZE;
    let d = ev_draft(&[liq(&l, piece, LONG)], &Evidence::ask(940_000_000, piece), &fk(), 18);
    accept(&d);
    let fee = liq_fee(&p, piece, 940_000_000, LONG);
    assert_eq!(fee, slice_of(&p, piece, LONG));
    assert_eq!(short_out(&d), p.range_of(piece) - fee);
    assert!(short_out(&d) >= winner_at_trigger(&p, piece, LONG));
}
