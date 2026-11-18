//! Review 2 (docs/audit/REVIEW-2.md). Each `exploit_*` test builds a transaction that the engine accepts and that
//! breaks a stated guarantee; each `holds_*` test records an attack that the contracts refuse or that does no harm.
//! Every input runs its real script, the KOB evidence order and the token program included.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::{Transaction, UtxoEntry};
use kpf::evidence::*;
use kpf::paths::*;
use kpf::*;
use secp256k1::Keypair;

const N: i64 = 1_000_000_000; // the whole pair: 1,000 tokens
// The base pair (bl 9, bs 12 KAS, mmr 5 %): the long is liquidatable at an ask a with a * 9500 < bl * 10^4, i.e.
// a <= 947,368,421; the short at a bid b with bs * 10^4 < b * 10500, i.e. b >= 1,142,857,143.
const TOP_ASK: i64 = 947_368_421;
const LOW_BID: i64 = 1_142_857_143;

fn lk() -> Keypair {
    keypair(11)
}
fn sk() -> Keypair {
    keypair(12)
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

// ---------------------------------------------------------------- F1: the loser chooses its own quote

/// F1 (Critical). The long of the DESIGN 5 pair is underwater: the market is at 8.5 KAS, below bl = 9 KAS, so the
/// short is owed the whole range (3,000 KAS). The long rests an ask of its own at 9.4737 KAS, just under its trigger
/// and far above the market, so no rational buyer takes it; after R it buys that ask itself (maker, buyer and
/// executor are the long) and runs P6 against its own pair. P6 settles at the ask, the bound favourable to the
/// liquidated party, so the long takes q(s, a - bl) = 473.68 KAS out of the pair (the keeper fee goes to the long as
/// executor). Who loses: the short, which did not act, receives 2,526.32 KAS instead of 3,000 KAS.
#[test]
fn exploit_self_liquidation_at_an_inflated_ask() {
    let l = live(&base(), 0x61);
    // Honest liquidation at a market ask of 8.5 KAS: the short gets the whole range.
    let honest = ev_draft(&[liq(&l, N, LONG)], &Evidence::ask(850_000_000, N), &keypair(14), 1);
    accept(&honest);
    assert_eq!(short_out(&honest), l.p.range_of(N));
    // The long's own ask at the top of the trigger window, filled by the long, executed by the long.
    let d = ev_draft(&[liq(&l, N, LONG)], &own_ask(TOP_ASK, N, &lk()), &lk(), 2);
    accept(&d);
    let stolen = q(N, TOP_ASK - l.p.bl, SCALE, 0);
    assert_eq!(short_out(&d), l.p.range_of(N) - stolen);
    assert_eq!(stolen, 47_368_421_000, "473.68 KAS moved from the short to the long");
    // The long's own payout plus the fee (its change as executor) is the whole stolen amount and the carrier.
    let fee = fee_of(&l.p.m, N, TOP_ASK);
    assert_eq!(long_out(&d), l.value as i64 - (l.p.range_of(N) - stolen) - fee);
}

/// F1, mirrored. The short is underwater (market 12.5 KAS, above bs = 12 KAS: the long is owed the whole range).
/// The short rests a bid of its own at 11.4286 KAS, just over its trigger and below the market, so no rational
/// seller takes it, sells into it itself and runs P6 against its own pair. Who loses: the long receives
/// q(s, b - bl) = 2,428.57 KAS instead of 3,000 KAS; the short takes 571.43 KAS it was not owed.
#[test]
fn exploit_self_liquidation_at_a_deflated_bid() {
    let l = live(&base(), 0x62);
    let honest = ev_draft(&[liq(&l, N, SHORT)], &Evidence::bid(1_250_000_000, N), &keypair(14), 1);
    accept(&honest);
    assert_eq!(long_out(&honest), l.p.range_of(N));
    let d = ev_draft(&[liq(&l, N, SHORT)], &own_bid(LOW_BID, N, &sk()), &sk(), 2);
    accept(&d);
    let stolen = q(N, l.p.bs - LOW_BID, SCALE, 0);
    assert_eq!(long_out(&d), l.p.range_of(N) - stolen);
    assert_eq!(stolen, 57_142_857_000);
}

/// F1 is bounded by the trigger: one sompi above the top of the window the pair refuses, so the loser can lift its
/// settlement price by at most bl * mmrBps / (10^4 - mmrBps) per token.
#[test]
fn holds_self_liquidation_needs_the_trigger() {
    let l = live(&base(), 0x63);
    refused(&ev_draft(&[liq(&l, N, LONG)], &own_ask(TOP_ASK + 1, N, &lk()), &lk(), 1), 0);
    refused(&ev_draft(&[liq(&l, N, SHORT)], &own_bid(LOW_BID - 1, N, &sk()), &sk(), 1), 0);
}

// ---------------------------------------------------------------- F2: soft-expired orders count as evidence

/// F2 (High). KOB's matcher spec (vendor/kob/docs/spec/matcher.md 2.3) forbids conforming matchers to fill an order
/// whose soft expiry has passed (expiryDaa, the IOC/FOK kill, 90 days idle) and the reference indexer leaves such
/// orders out of its books; the covenants still accept the fill. KPF reads only activeFrom, the UTXO DAA, the
/// interval and the custody DAA, so an ask that expired the moment it was placed (expiryDaa = 1) and an IOC ask past
/// its kill both count as having rested R. Neither was ever offered to a conforming matcher, so the arbitrage that
/// DESIGN 3.3 and THREAT_MODEL A1 rely on to make a mispriced print costly does not apply. Who loses: the long of a
/// healthy pair (market 10 KAS) is liquidated at 9.4 KAS by a counterparty that printed the quote to itself; the
/// long gets q(s, 0.4 KAS) less the fee instead of 1,000 KAS of equity at the market.
#[test]
fn exploit_expired_ask_counts_as_rested_evidence() {
    let l = live(&base(), 0x64);
    let mut e = own_ask(940_000_000, N, &sk());
    e.ask.expiry_daa = 1;
    let d = ev_draft(&[liq(&l, N, LONG)], &e, &sk(), 1);
    accept(&d);
    let fee = fee_of(&l.p.m, N, 940_000_000);
    // With n = s the long's residual is the pair's value less the short's q(s, bs - a) and the fee (carrier included).
    assert_eq!(long_out(&d), l.value as i64 - q(N, l.p.bs - 940_000_000, SCALE, 0) - fee);
    assert_eq!(long_out(&d), q(N, 40_000_000, SCALE, 0) + C - fee);
    // An IOC ask (tif 1), killed IOC_LIFE = 600 DAA after it became fillable, still counts after R = 600.
    let mut ioc = own_ask(940_000_000, N, &sk());
    ioc.ask.tif = 1;
    accept(&ev_draft(&[liq(&l, N, LONG)], &ioc, &sk(), 2));
}

/// F2 on P5. The long closes unilaterally on an expired bid of its own at bs = 12 KAS (market 10 KAS): no
/// conforming seller could have sold into it, so the long takes the whole range. Who loses: the short, which did
/// not act, gets the carrier only instead of 2,000 KAS of margin at the market.
#[test]
fn exploit_expired_bid_closes_the_long_at_the_whole_range() {
    let l = live(&base(), 0x65);
    let mut e = own_bid(l.p.bs, N, &lk());
    e.bid.expiry_daa = 1;
    let d = ev_draft(&[close(&l, N, LONG, lk())], &e, &lk(), 1);
    accept(&d);
    assert_eq!(long_out(&d), l.p.range_of(N));
    assert_eq!(short_out(&d), l.value as i64 - l.p.range_of(N));
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
