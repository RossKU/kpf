//! Review 4 (docs/audit/REVIEW-4.md) and its fixes (DESIGN 4.5e): attacks on the two-step evidence mechanism
//! (KpfPair.propose, KpfPending) after the restructuring of DESIGN 4.5d. Every input runs its real script in the engine,
//! the KOB evidence orders and the token program included. A `holds_*` test shows an attack refused (each `exploit_*`
//! of the review became one when its finding was fixed); an `accepted_*` test shows behaviour the design accepts and
//! documents.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::Transaction;
use kaspa_consensus_core::Hash;
use kpf::evidence::*;
use kpf::paths::*;
use kpf::*;
use kpf_protocol::payoff::{long_liquidatable, short_liquidatable};
use kpf_protocol::pending::cure_reach_of;
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

/// The worked example with both sides listed (long sells at >= 10, short buys back at <= 11).
fn listed_lv(tag: u8, daa: u64) -> Live {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (1_000_000_000, 0, 0);
    (p.short_bid, p.short_min, p.short_tip) = (1_100_000_000, 0, 0);
    live(&p, tag).at(daa)
}

fn liq(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side } }
}

fn liq_by(l: &Live, n: i64, side: i64, proposer: [u8; 32]) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::LiqBy { side, proposer } }
}

fn close_by(l: &Live, n: i64, side: i64, signer: Keypair) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side, signer } }
}

fn at_lock(d: &Draft, lock: u64) -> Draft {
    let mut x = d.clone();
    x.lock = lock;
    x
}

fn unlisted(p: &Pair) -> bool {
    (p.long_ask, p.long_min, p.long_tip, p.short_bid, p.short_min, p.short_tip) == (0, 0, 0, 0, 0, 0)
}

/// The honest liquidation of the long of the worked example at an ask of 9.40 placed at S + 10, finalized: the short
/// (`sk`) owns both sides of the forfeited part. Returns the ask, the forfeited part at its UTXO DAA S' and S'.
fn forfeited(tag: u8, s: u64) -> (Evidence, Live, u64) {
    let l = lv(tag, s);
    let ask = Evidence::ask(940_000_000, N).placed(s + 10);
    let p = at_lock(&ev_draft(&[liq(&l, N, LONG)], &ask, &fk(), 1), s + 10 + R);
    let ptx = accept(&p);
    let t = s + 10 + R;
    let w = live_pendings(&ptx, &p).remove(0).at(t);
    let fd = finalize_draft(&w, &fk(), 2);
    let ftx = accept(&fd);
    let t = t + window();
    let fp = live_pairs(&ftx, &fd).remove(0).at(t);
    let winner = xonly(&sk());
    assert_eq!((fp.p.long_key, fp.p.short_key, fp.p.since), (winner, winner, 0));
    assert!(fp.p.bl > l.p.bl, "the forfeited long's bound moved inward by the fee");
    assert!(long_liquidatable(&fp.p, 940_000_000), "the old quote still triggers the forfeited part's long by price");
    (ask, fp, t)
}

// ================================================================ R4-F1: one owner of both sides cannot be liquidated

/// R4-F1 (High), fixed (DESIGN 4.5e). Forfeit and keep leaves the winner a part whose two sides are both its own, with
/// the loser's bound moved inward by the fee and a fresh clock S'; the quote that liquidated the old long still triggers
/// the new part's long by price. `propose` now refuses a liquidation when longKey == shortKey: the 9.40 ask, rested on
/// S' as long as one likes, is refused for the whole part, for a part of it, by any proposer; so is a bid against the
/// "short". The control: the same draft on the same state with two distinct keys is accepted, so the keys are the only
/// difference.
#[test]
fn holds_forfeited_part_is_not_liquidatable() {
    let s = 100_000u64;
    let (ask, fp, t) = forfeited(0x51, s);
    let former_loser = xonly(&lk());
    for (k, lock) in [t + R, t + 10 * R, t + 100 * R].into_iter().enumerate() {
        refused(&at_lock(&ev_draft(&[liq_by(&fp, N, LONG, former_loser)], &ask, &fk(), 10 + k as u32), lock), 0);
        refused(&at_lock(&ev_draft(&[liq(&fp, N / 2, LONG)], &ask, &fk(), 20 + k as u32), lock), 0);
    }
    let bid = Evidence::bid(1_150_000_000, N).placed(s + 10);
    assert!(short_liquidatable(&fp.p, 1_150_000_000));
    refused(&at_lock(&ev_draft(&[liq(&fp, N, SHORT)], &bid, &fk(), 30), t + R), 0);
    // Control: two keys, every other byte the same.
    let mut two = fp.clone();
    two.p.long_key = former_loser;
    accept(&at_lock(&ev_draft(&[liq_by(&two, N, LONG, former_loser)], &ask, &fk(), 31), t + R));
}

/// R4-F1, the extent, fixed. The review drained 62 % of the forfeited part in 39 rounds; now no round is accepted, the
/// part keeps its whole value, and the winner's own exits stay open: a unilateral close proposed by the single owner
/// (its one key signs for the side it closes) is accepted and finalized with both payouts to that key, and a cooperative
/// close with its key twice releases the whole value.
#[test]
fn holds_forfeited_part_is_not_drained_and_its_owner_can_close_it() {
    let s = 100_000u64;
    let (ask, fp, t) = forfeited(0x57, s);
    let start = fp.value;
    let mut rounds = 0u32;
    for k in 0..39u64 {
        let d = at_lock(&ev_draft(&[liq_by(&fp, N, LONG, xonly(&lk()))], &ask, &fk(), 100 + k as u32), t + R + k * (R + window()));
        let (tx, e) = d.build();
        if run(&tx, &e).expect("bindings").iter().all(|r| r.is_ok()) {
            rounds += 1;
        }
    }
    assert_eq!((rounds, fp.value), (0, start), "no round is accepted and the forfeited part keeps its value");
    // The single owner closes its long at a bid of 9.50 (kind 1 is not a liquidation): proposed and finalized.
    let winner = xonly(&sk());
    let bid = Evidence::bid(950_000_000, N).placed(t);
    let d = at_lock(&ev_draft(&[close_by(&fp, N, LONG, sk())], &bid, &fk(), 200), t + R);
    let tx = accept(&d);
    let w = live_pendings(&tx, &d).remove(0).at(t + R);
    assert_eq!((w.w.kind, w.w.long_key, w.w.short_key), (KIND_CLOSE, winner, winner));
    let f = finalize_draft(&w, &fk(), 201);
    let ftx = accept(&f);
    let paid: u64 = ftx.outputs.iter().filter(|o| o.script_public_key == p2pk_x(&winner)).map(|o| o.value).sum();
    assert_eq!(paid, w.value, "both payouts of the close go to the one owner");
    // Or a cooperative close with the one key signing twice.
    let ins = vec![pair_input(&fp, "coopClose", vec![Arg::Sig(sk()), Arg::Sig(sk())]), funder(&fk(), 202, F_FUNDS)];
    let outs = vec![out(fp.value, p2pk_x(&winner), None), out(0, p2pk_spk(&fk()), None)];
    accept(&Draft::new(ins, outs, Some(1)));
}

/// R4-F1, a sale after the forfeit: the winner lists the forfeited long and sells it. The buyer enters under the
/// healthy-at-entry rule at its own price x (a bound it is liquidatable at, at x, is refused), with the entry price kept,
/// its side unlisted and a fresh clock S'' (since = 0); the part has two keys again and the ordinary rules apply: the old
/// 9.40 ask counts only for its rest on S'' (refused at S'' + R - 1), and then liquidates a buyer that chose a bound
/// (9.047, healthy at its x = 9.60) the market ask triggers, but never a buyer whose bound is healthy at that ask too.
#[test]
fn holds_sold_side_enters_healthy_and_on_a_fresh_clock() {
    let s = 100_000u64;
    let (ask, fp, t) = forfeited(0x58, s);
    let ad = amend_draft(&fp, &sk(), LONG, (950_000_000, 0, 0), &fk(), 40);
    let atx = accept(&ad);
    let fl = live_pairs(&atx, &ad).remove(0).at(t + 5);
    assert_eq!(fl.p.since as u64, t, "the amend keeps the forfeited part's clock S'");
    let buyer = keypair(16);
    let fb = fl.p.bl;
    // Not healthy at its own x: 9.50 * 0.95 = 9.025 < 9.047.
    let sick = TransferSpec { side: LONG, n: N, x: 950_000_000, nb: fb, ek: xonly(&buyer), et: ZERO, k: 0 };
    refused(&transfer_draft(&fl, &sick, &Entrant::Direct(buyer), &fk(), 41), 0);
    let sell = |nb: i64, fnn: u32| {
        let ts = TransferSpec { side: LONG, n: N, x: 960_000_000, nb, ek: xonly(&buyer), et: ZERO, k: 0 };
        let mut td = transfer_draft(&fl, &ts, &Entrant::Direct(buyer), &fk(), fnn);
        assert_eq!(td.lock, t + 4 * R, "a forfeited side is sold from S' + 4R");
        refused(&at_lock(&td, t + 4 * R - 1), 0);
        td.lock = t + 4 * R;
        let tx = accept(&td);
        live_pairs(&tx, &td).remove(0).at(t + 4 * R)
    };
    let s2 = t + 4 * R;
    let bold = sell(fb, 42);
    assert_eq!((bold.p.long_key, bold.p.short_key, bold.p.since, bold.p.entry), (xonly(&buyer), xonly(&sk()), 0, 1_000_000_000));
    assert!(unlisted(&bold.p), "the entrant's side is unlisted and the forfeited part had no listing of the short");
    refused(&at_lock(&ev_draft(&[liq(&bold, N, LONG)], &ask, &fk(), 43), s2 + R - 1), 0);
    accept(&at_lock(&ev_draft(&[liq(&bold, N, LONG)], &ask, &fk(), 44), s2 + R));
    // A buyer with a bound healthy at the market ask (8.93: 9.40 * 0.95 = 8.93 is not below it).
    let careful = sell(893_000_000, 45);
    refused(&at_lock(&ev_draft(&[liq(&careful, N, LONG)], &ask, &fk(), 46), s2 + 100 * R), 0);
}

/// Item 7 of the fix: can a loser make itself unliquidatable through the one-owner rule? Only by really owning both
/// sides, and then nobody is left who could lose. (a) The liquidatable long buys the short's own listing (11.00) with
/// its key: the short is paid what its listing promises, q(n, bs - x), and leaves; the part is then the long's alone
/// and no liquidation is accepted, but there is no counterparty left to be owed one. (b) The loser fills its own long
/// listing with the short's key as entrant: a gift of its side to the short, who now owns both sides and loses nothing.
/// (c) Two different keys, even if one party controls both, are two owners: the liquidation is accepted (the rule
/// compares keys, and a party that splits its keys keeps the ordinary rules).
#[test]
fn holds_one_owner_only_by_owning_both_sides() {
    let s = 1_000u64;
    let l = listed_lv(0x59, s);
    let ask = Evidence::ask(940_000_000, N).placed(s + 10);
    assert!(long_liquidatable(&l.p, 940_000_000));
    // (a) The loser buys out the winner at the winner's listed price.
    let ts = TransferSpec { side: SHORT, n: N, x: 1_100_000_000, nb: 1_200_000_000, ek: xonly(&lk()), et: ZERO, k: 0 };
    let td = transfer_draft(&l, &ts, &Entrant::Direct(lk()), &fk(), 50);
    let tx = accept(&td);
    assert_eq!(tx.outputs[0].script_public_key, p2pk_x(&xonly(&sk())));
    assert!(tx.outputs[0].value as i64 >= q(N, 100_000_000, SCALE, 0), "the short is paid its listing's price");
    let own = live_pairs(&tx, &td).remove(0).at(s + 4 * R);
    assert_eq!((own.p.long_key, own.p.short_key), (xonly(&lk()), xonly(&lk())));
    refused(&at_lock(&ev_draft(&[liq(&own, N, LONG)], &ask, &fk(), 51), s + 4 * R + R), 0);
    // (b) The loser gives its long to the winner.
    let ts = TransferSpec { side: LONG, n: N, x: 1_000_000_000, nb: 900_000_000, ek: xonly(&sk()), et: ZERO, k: 0 };
    let td = transfer_draft(&l, &ts, &Entrant::Direct(lk()), &fk(), 52);
    let tx = accept(&td);
    let gift = live_pairs(&tx, &td).remove(0);
    assert_eq!((gift.p.long_key, gift.p.short_key), (xonly(&sk()), xonly(&sk())));
    assert!(gift.value >= l.value, "the winner's part holds at least what the pair held");
    // (c) Two keys of one party are two owners.
    let two = live(&pair(&lk(), &keypair(13)), 0x5a).at(s);
    accept(&at_lock(&ev_draft(&[liq(&two, N, LONG)], &ask, &fk(), 53), s + 10 + R));
}

/// Item 7: a loser cannot take both sides by a forfeit in the other direction on a stale quote. The liquidatable long
/// proposes the liquidation of the short at a bid of 11.50 that rested on S (the price has since fallen); the market's
/// ask of 9.40 refutes it (9.40 * 1.05 <= 12 * 0.98), the part is restored with both keys and the clock S, and the long's
/// own liquidation at that ask is accepted in the next transaction.
#[test]
fn holds_loser_cannot_take_both_sides_by_a_stale_claim() {
    let s = 1_000u64;
    let l = lv(0x5b, s);
    let stale = Evidence::bid(1_150_000_000, N).placed(s + 10);
    let d = at_lock(&ev_draft(&[liq_by(&l, N, SHORT, xonly(&lk()))], &stale, &fk(), 60), s + 10 + R);
    let tx = accept(&d);
    let p_daa = s + 10 + R;
    let w = live_pendings(&tx, &d).remove(0).at(p_daa);
    let c = at_lock(&ch_draft(&[w], &Evidence::ask(940_000_000, N).placed(p_daa), &fk(), 61), p_daa + R);
    let ctx = accept(&c);
    let r = live_pairs(&ctx, &c).remove(0).at(p_daa + R);
    assert_eq!((r.p.long_key, r.p.short_key, r.p.since), (xonly(&lk()), xonly(&sk()), s as i64));
    accept(&at_lock(&ev_draft(&[liq(&r, N, LONG)], &Evidence::ask(940_000_000, N).placed(s + 10), &fk(), 62), p_daa + R));
}

// ================================================================ R4-F3: the cure clears the claim by g

/// R4-F3 (Low), fixed (DESIGN 4.5e). A long's liquidation at an ask p = 9.40 with g = 200 bps: the reach of the claim
/// is pg = ceil(p * (10^4 - g) / 10^4) = 9.212 and the cure bound cb = floor(pg * (10^4 - m) / 10^4) = 8.7514. The
/// cured part is liquidatable at an ask a iff a * (10^4 - m) < cb * 10^4. At the cure's own DAA, with every ask rested
/// on the kept clock S: the review's ask one sompi below p is refused, the ask at pg (equality: within g) is refused, and
/// the ask one sompi beyond, 9.21199999, is accepted. The cure adds q(n, 9 - 8.7514) = 248.6 KAS.
#[test]
fn holds_cure_is_not_reopened_within_g() {
    let s = 100_000u64;
    let l = lv(0x52, s);
    let a1 = Evidence::ask(940_000_000, N).placed(s + 10);
    let p_daa = s + 10 + R;
    let p = at_lock(&ev_draft(&[liq(&l, N, LONG)], &a1, &fk(), 1), p_daa);
    let ptx = accept(&p);
    let w = live_pendings(&ptx, &p).remove(0).at(p_daa);
    assert_eq!((cure_reach_of(&w.w.m, LONG, 940_000_000), w.w.cure_bound), (921_200_000, 875_140_000));
    let cd = cure_draft(&w, &lk(), &fk(), 2);
    let ctx = accept(&cd);
    let cure_daa = p_daa + 20;
    let cured = live_pairs(&ctx, &cd).remove(0).at(cure_daa);
    assert_eq!((cured.p.bl, cured.p.since), (875_140_000, s as i64), "cured to 8.7514 with the clock S kept");
    assert!(cured.value as i64 - l.value as i64 >= q(N, 900_000_000 - 875_140_000, SCALE, 0), "the loser added 248.6 KAS");
    let try_at = |price: i64, tag: u8, fnn: u32| at_lock(&ev_draft(&[liq(&cured, N, LONG)], &Evidence::ask(price, N).placed(s + 10).other(tag), &fk(), fnn), cure_daa);
    refused(&try_at(939_999_999, 0xd5, 3), 0);
    refused(&try_at(921_200_000, 0xd6, 4), 0);
    accept(&try_at(921_199_999, 0xd7, 5));
    // The cured part still has every exit: its long proposes a close at a bid and it is finalized.
    let d = at_lock(&ev_draft(&[close_by(&cured, N, LONG, lk())], &Evidence::bid(950_000_000, N).placed(s + 10), &fk(), 6), cure_daa);
    let tx = accept(&d);
    accept(&finalize_draft(&live_pendings(&tx, &d).remove(0).at(cure_daa), &fk(), 7));
}

/// R4-F3 for a short: a liquidation at a bid p = 11.50: pg = floor(p * (10^4 + g) / 10^4) = 11.73 and the cure bound
/// cb = ceil(pg * (10^4 + m) / 10^4) = 12.3165. The cured short is liquidatable at a bid b iff cb * 10^4 < b * (10^4 + m):
/// the bid at pg (equality) is refused, one sompi beyond it is accepted.
#[test]
fn holds_cure_of_a_short_is_not_reopened_within_g() {
    let s = 100_000u64;
    let l = lv(0x5c, s);
    let p_daa = s + 10 + R;
    let p = at_lock(&ev_draft(&[liq(&l, N, SHORT)], &Evidence::bid(1_150_000_000, N).placed(s + 10), &fk(), 1), p_daa);
    let ptx = accept(&p);
    let w = live_pendings(&ptx, &p).remove(0).at(p_daa);
    assert_eq!((cure_reach_of(&w.w.m, SHORT, 1_150_000_000), w.w.cure_bound), (1_173_000_000, 1_231_650_000));
    let cd = cure_draft(&w, &sk(), &fk(), 2);
    let ctx = accept(&cd);
    let cured = live_pairs(&ctx, &cd).remove(0).at(p_daa + 20);
    assert_eq!(cured.p.bs, 1_231_650_000);
    let try_at = |price: i64, tag: u8, fnn: u32| at_lock(&ev_draft(&[liq(&cured, N, SHORT)], &Evidence::bid(price, N).placed(s + 10).other(tag), &fk(), fnn), p_daa + 20);
    refused(&try_at(1_150_000_001, 0xd5, 3), 0);
    refused(&try_at(1_173_000_000, 0xd6, 4), 0);
    accept(&try_at(1_173_000_001, 0xd7, 5));
}

/// The cure margin as an inequality, for both sides over a sweep of claim quotes (the library mirrors the contract,
/// which the two tests above hold to the engine). Long loser at ask p: pg = ceil(p (10^4 - g) / 10^4), cb = floor(pg
/// (10^4 - m) / 10^4); no ask a >= pg (a * 10^4 >= p * (10^4 - g)) liquidates the cured part, and the largest ask that
/// does, floor-adjacent to cb * 10^4 / (10^4 - m), is below pg. Short loser at bid p: pg = floor(p (10^4 + g) / 10^4), cb =
/// ceil(pg (10^4 + m) / 10^4); no bid b <= pg liquidates it, the smallest that does is above pg. At pg - 1 (long) or
/// pg + 1 (short) the cured part is liquidatable unless the rounding of cb leaves it one tick short (then the first
/// quote that triggers is one sompi further).
#[test]
fn holds_cure_margin_inequality() {
    let l = lv(0x5d, 1_000);
    let m = l.p.m.clone();
    let mut p = 300_000_000i64;
    while p < 1_190_000_000 {
        p += 7_654_321;
        // Long loser.
        let mut lp = l.p.clone();
        lp.bl = 1_190_000_000.min(p * 10_000 / (10_000 - m.mmr_bps) + 1);
        if long_liquidatable(&lp, p) {
            let (pg, cb) = (cure_reach_of(&m, LONG, p), cure_bound_of(&m, LONG, p).unwrap());
            let mut c = lp.clone();
            c.bl = cb;
            assert!(pg as i128 * 10_000 >= p as i128 * (10_000 - m.g_bps) as i128 && (pg as i128 - 1) * 10_000 < p as i128 * (10_000 - m.g_bps) as i128);
            for a in [pg, pg + 1, p - 1, p] {
                assert!(!long_liquidatable(&c, a), "long: ask {a} within g of {p} reopens the cure to {cb}");
            }
            assert!(long_liquidatable(&c, pg - 1) || long_liquidatable(&c, pg - 2), "long: a quote beyond g of {p} must still trigger");
        }
        // Short loser.
        let mut sp = l.p.clone();
        sp.bs = 1_200_000_000;
        let b = p.max(sp.bs * 10_000 / (10_000 + m.mmr_bps) + 1);
        if short_liquidatable(&sp, b) {
            let (pg, cb) = (cure_reach_of(&m, SHORT, b), cure_bound_of(&m, SHORT, b).unwrap());
            let mut c = sp.clone();
            c.bs = cb;
            for x in [pg, pg - 1, b + 1, b] {
                assert!(!short_liquidatable(&c, x), "short: bid {x} within g of {b} reopens the cure to {cb}");
            }
            assert!(short_liquidatable(&c, pg + 1) || short_liquidatable(&c, pg + 2), "short: a quote beyond g of {b} must still trigger");
        }
    }
}

/// Item 7: the margin can remove a cure that the old bound allowed (a short's cure bound beyond priceMax), and then the
/// part keeps its other exits. Market priceMax 12.50, a short's liquidation at a bid of 11.80: the old bound
/// ceil(11.80 * 1.05) = 12.39 fitted, the new one ceil(12.036 * 1.05) = 12.6378 does not, so the claim has no cure; it is
/// still refuted by an ask <= 11.20 and still finalized after W.
#[test]
fn holds_without_a_cure_challenge_and_finalize_remain() {
    let mut p = pair(&lk(), &sk());
    p.m.price_max = 1_250_000_000;
    let l = live(&p, 0x5e).at(1_000);
    let g = liq(&l, N, SHORT);
    // The contract writes exactly this pending state (no cure hash): the proposal is accepted with it.
    accept(&ev_draft(&[g.clone()], &Evidence::bid(1_180_000_000, N), &fk(), 0));
    let w = pend(&g, 1_180_000_000);
    assert!(!w.w.can_cure() && w.w.cure_bound == 0, "no cure beyond priceMax");
    assert!(((1_180_000_000i64 * 10_500 + 9_999) / 10_000) <= p.m.price_max, "the review-4 bound would have cured");
    accept(&ch_draft(&[w.clone()], &Evidence::ask(1_120_000_000, N), &fk(), 1));
    accept(&finalize_draft(&w, &fk(), 2));
}

// ================================================================ unlisted return states (DESIGN 4.5e)

/// A listing present when the claim is proposed is copied into the pending state (it is self-describing) but not into
/// the pair the part returns to: the restored pair of a refuted claim is unlisted, so the old long listing at 10 cannot
/// be filled after the hand-over limit, until the long lists again with its signature.
#[test]
fn holds_restored_part_is_unlisted() {
    let l = listed_lv(0x61, 1_000);
    let w = pend(&liq(&l, N, LONG), 940_000_000);
    assert_eq!((w.w.long_ask, w.w.short_bid), (1_000_000_000, 1_100_000_000), "the pending state records the listings");
    let c = ch_draft(&[w], &Evidence::bid(980_000_000, N), &fk(), 1);
    let ctx = accept(&c);
    let r = live_pairs(&ctx, &c).remove(0).at(NOW);
    assert!(unlisted(&r.p), "the restored pair is unlisted");
    let ts = TransferSpec { side: LONG, n: N, x: 1_000_000_000, nb: 900_000_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    refused(&at_lock(&transfer_draft(&r, &ts, &Entrant::Direct(keypair(16)), &fk(), 2), NOW), 0);
    let ad = amend_draft(&r, &lk(), LONG, (1_000_000_000, 0, 0), &fk(), 3);
    let atx = accept(&ad);
    let relisted = live_pairs(&atx, &ad).remove(0).at(NOW);
    accept(&at_lock(&transfer_draft(&relisted, &ts, &Entrant::Direct(keypair(16)), &fk(), 4), NOW));
}

/// The cured pair is unlisted too: the short's listing at 11 (which the short could not withdraw while the part was
/// pending) is not fillable on the cured pair.
#[test]
fn holds_cured_part_is_unlisted() {
    let l = listed_lv(0x62, 1_000);
    let w = pend(&liq(&l, N, LONG), 940_000_000);
    let cd = cure_draft(&w, &lk(), &fk(), 1);
    let ctx = accept(&cd);
    let cured = live_pairs(&ctx, &cd).remove(0).at(NOW);
    assert!(unlisted(&cured.p));
    let ts = TransferSpec { side: SHORT, n: N, x: 1_100_000_000, nb: 1_200_000_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    refused(&at_lock(&transfer_draft(&cured, &ts, &Entrant::Direct(keypair(16)), &fk(), 2), NOW), 0);
    // The pending part's own state cannot be swapped for a listed restore: a restore output that keeps the listings is
    // refused by the challenge.
    let w2 = pend(&liq(&l, N, LONG), 940_000_000);
    let mut c = ch_draft(&[w2.clone()], &Evidence::bid(980_000_000, N), &fk(), 3);
    let mut listed = w2.w.restore_pair().unwrap();
    (listed.long_ask, listed.short_bid) = (1_000_000_000, 1_100_000_000);
    c.set_pair(0, &listed);
    refused(&c, 0);
}

// ================================================================ R4-F5: W >= 2R

fn orders(m: &Market) -> (LiveOpen, LiveOpen) {
    let mut lo = long_order(&keypair(21));
    lo.m = m.clone();
    let mut so = short_order(&keypair(22));
    so.m = m.clone();
    (
        LiveOpen { value: lo.escrow() as u64, o: lo, id: Hash::from_bytes([0x81; 32]), op: outpoint(0x81) },
        LiveOpen { value: so.escrow() as u64, o: so, id: Hash::from_bytes([0x82; 32]), op: outpoint(0x82) },
    )
}

fn opens(m: &Market, fnn: u32) -> bool {
    let (lo, so) = orders(m);
    let (tx, e) = open_draft(&lo, &so, N, 1_000_000_000, &fk(), fnn).build();
    let r = run(&tx, &e).unwrap();
    assert_eq!(r[0].is_ok(), r[1].is_ok(), "both orders check the same market bounds");
    r[0].is_ok() && r[1].is_ok()
}

/// R4-F5 (Informational), fixed (DESIGN 4.5e). KpfOpen.checkMarket and the library require challengeDaa >= 2 *
/// minRestDaa: W = R and W = 2R - 1 are refused by both orders of a fill, W = 2R is accepted. On such a market a counter
/// order placed at P refutes from P + R, and finalize opens only R later.
#[test]
fn holds_window_below_twice_the_rest_is_refused() {
    let m = market();
    for (w, ok) in [(R as i64, false), (2 * R as i64 - 1, false), (2 * R as i64, true)] {
        let mut x = m.clone();
        x.challenge_daa = w;
        assert_eq!(opens(&x, 70 + w as u32), ok, "W = {w}");
        assert_eq!(x.validate().is_ok(), ok, "library, W = {w}");
    }
    let mut p = pair(&lk(), &sk());
    p.m.challenge_daa = 2 * R as i64;
    let l = live(&p, 0x53).at(1_000);
    let p_daa = NOW - R;
    let w = pend_of(&liq(&l, N, LONG), 940_000_000, p_daa);
    let c = ch_draft(&[w.clone()], &Evidence::bid(980_000_000, N).placed(p_daa), &fk(), 1);
    refused(&at_lock(&c, p_daa + R - 1), 0);
    accept(&at_lock(&c, p_daa + R));
    let mut f = finalize_draft(&w, &fk(), 2);
    f.seqs = vec![2 * R - 1, 0];
    refused(&f, 0);
    f.seqs = vec![2 * R, 0];
    accept(&f);
}

/// Unproven item of the review, now a rule (DESIGN 4.5e): bondMin is bounded (10^12 sompi, 10,000 KAS), so no market an
/// order creates makes every proposal unfundable.
#[test]
fn holds_bond_floor_is_bounded() {
    let m = market();
    for (b, ok) in [(1_000_000_000_000i64, true), (1_000_000_000_001, false), (2_900_000_000_000_000_000, false)] {
        let mut x = m.clone();
        x.bond_min = b;
        assert_eq!(opens(&x, 80 + (b % 7) as u32), ok, "bondMin = {b}");
        assert_eq!(x.validate().is_ok(), ok, "library, bondMin = {b}");
    }
}

// ================================================================ accepted by design

/// Unproven item of the review, accepted and documented (DESIGN 4.5e): a proposal (and a challenge) is permissionless,
/// so whoever relays one can copy it with its own payout key and its own funds. Both copies are valid; they spend the
/// same pair UTXO, so at most one is mined, and the copier earns the keeper's reward. No party's funds move otherwise:
/// the pending parts of the two differ only in the proposer key.
#[test]
fn accepted_relay_copy_with_another_payout_key() {
    let l = lv(0x63, 1_000);
    let ask = Evidence::ask(940_000_000, N);
    let a = ev_draft(&[liq_by(&l, N, LONG, xonly(&keypair(77)))], &ask, &fk(), 1);
    let b = ev_draft(&[liq_by(&l, N, LONG, xonly(&keypair(78)))], &ask, &keypair(79), 2);
    let (ta, tb) = (accept(&a), accept(&b));
    assert_eq!(ta.inputs[0].previous_outpoint, tb.inputs[0].previous_outpoint, "the copies conflict on the pair input");
    let (wa, wb) = (&a.pendings[0].1, &b.pendings[0].1);
    assert_ne!(wa.proposer_key, wb.proposer_key);
    let mut x = wb.clone();
    x.proposer_key = wa.proposer_key;
    assert_eq!(&x, wa, "the copy differs only in who is paid the bond and the fee");
}

// ================================================================ holds kept from the review

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
        (close_by(&l, N, LONG, lk()), 1_050_000_000, Evidence::ask(1_029_000_000, N), Evidence::ask(1_029_000_001, N)),
        (close_by(&l, N, SHORT, sk()), 1_100_000_000, Evidence::bid(1_122_000_000, N), Evidence::bid(1_121_999_999, N)),
    ];
    for (k, (g, quote, yes, no)) in cases.into_iter().enumerate() {
        let w = pend(&g, quote);
        accept(&ch_draft(&[w.clone()], &yes, &fk(), 10 + k as u32));
        refused(&ch_draft(&[w], &no, &fk(), 20 + k as u32), 0);
    }
}
