//! Review 5 (docs/audit/REVIEW-5.md): attacks on guaranteed fast settlement (DESIGN 4.5f) and its interaction with the
//! slow path. Each test is an `exploit_` (the engine accepts the attack; who loses what is stated) or a `holds_` (the
//! engine refuses it). The real KOB order and token contracts run in every evidence transaction.
//!
//! Market and pair as in `kpf_guarantee_tests` (DESIGN 5 worked example: 1,000 tokens, bl 9, bs 12 KAS, lambda 1.1,
//! R 600, W 6,000, D 6,600, G 1,200, g 2 %, mmr 5 %).

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
fn gk() -> [u8; 32] {
    xonly(&guarantor_kp())
}

const FULL: i64 = 1_000_000_000;
const LIQ_ASK: i64 = 940_000_000;
const CL_BID: i64 = 1_050_000_000;
const REF_LIQ_LONG: i64 = 966_315_790;
const REF_CL_LONG: i64 = 1_029_000_000;
const KAS: i64 = 100_000_000;

fn lv_of(p: &Pair, tag: u8) -> Live {
    live(p, tag).at(1_000)
}

fn close_long(l: &Live) -> PairLeg {
    PairLeg { l: l.clone(), n: FULL, q: 0, act: Act::Close { side: LONG, signer: lk() } }
}

fn liq(l: &Live, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n: FULL, q: 0, act: Act::Liq { side } }
}

fn dispute(ls: &[LiveLk], e: &Evidence, f: &Keypair) -> Draft {
    let mut d = dp_draft(ls, e, f, 6);
    d.lock = d.lock.max(ls.iter().map(|l| l.daa).max().unwrap_or(0));
    d
}

/// An ask at `price` for `fill` tokens made by `who`, filled by `who` itself (its tokens return to it, its KAS too).
fn own_ask(who: &Keypair, price: i64, fill: i64) -> Evidence {
    let mut e = Evidence::ask(price, fill);
    e.ask = kpf::kob::ask_of(&market(), xonly(who), price, fill);
    e.maker = *who;
    e.counter = *who;
    e
}

/// A bid at `price` for `fill` tokens made by `who`, filled with `who`'s own tokens.
fn own_bid(who: &Keypair, price: i64, fill: i64) -> Evidence {
    let mut e = Evidence::bid(price, fill);
    e.bid = kpf::kob::bid_of(&market(), xonly(who), price);
    e.maker = *who;
    e.counter = *who;
    e
}

// ================================================================ F1: the disputer chooses the restitution quote

/// F1 (High). The restitution H is priced at the counter order the disputer chooses, not at the market: any order that
/// contradicts the claim by g and rested from P qualifies, and the first dispute spends the lock. A party of the claim
/// that disputes first with its own order at the refutation bound (an ask above the market or a bid below it, which no
/// arbitrage fills, so it rests on any book) pays the wronged party the bound's value, and the rest of what it is owed
/// is burned. Nobody gains; the attacker breaks even, the victim loses.
///
/// Close: the long closes at a bid of 10.5 and guarantees its own claim (it is the guarantor); the market then falls
/// to 9.00 (a third party's ask at 9.00 rests from P). A dispute on that ask pays the short H = 1,500 KAS (it ends with
/// its whole side, 3,000.2 KAS). The long disputes first with its own ask at 10.29, filled by itself: H = 210 KAS, so
/// the short ends with 1,710.2 KAS and loses 1,290 KAS, which are burned. The long's net (payout 1,590 - lock 1,650 +
/// reward 150 = 90 KAS, its own bond back) equals what finalize at the true price 9.00 would have paid it: the attack
/// costs it nothing.
///
/// Liquidation: the long's liquidation at 9.40, guaranteed by the winner (the short); the market recovers to 12 (a bid
/// at 12 rests from P): H on it is 3,000 KAS. The short disputes first with its own bid at 9.66315790: H = 663.16 KAS;
/// the long loses 2,336.84 KAS. The short's net: the long's side (worth 3,000 KAS at 12, plus the fee) - lock 3,300 +
/// reward 300 = 0 (+ the fee): again free.
#[test]
fn exploit_the_first_disputer_prices_the_restitution_at_its_own_boundary_order() {
    // ---- close
    let l = lv_of(&pair(&lk(), &sk()), 0x61);
    let w = pend(&close_long(&l), CL_BID);
    let owner = xonly(&lk());
    let g = guarantee_draft(&w, &owner, &lk(), 5);
    let tx = accept(&g);
    let lock = live_locks(&tx, &g).remove(0).at(w.daa + 1);
    let (lp, sp) = w.w.guarantee_close(w.value);
    assert_eq!((lp, sp, lock.value as i64), (1_590 * KAS, 150_020_000_000, 1_650 * KAS));
    // Both orders are valid counter-evidence at the same time; the first dispute to land decides H.
    let market_ask = Evidence::ask(900_000_000, FULL);
    let fair = accept(&dispute(&[lock.clone()], &market_ask, &fk()));
    assert_eq!(fair.outputs[0].value as i64, 1_500 * KAS, "on the market's ask the short is paid 1,500 KAS");
    let mine = own_ask(&lk(), REF_CL_LONG, FULL);
    let cheat = accept(&dispute(&[lock.clone()], &mine, &lk()));
    assert_eq!(cheat.outputs[0].value as i64, 210 * KAS, "on the long's own ask at the bound the short is paid 210 KAS");
    assert_eq!(cheat.outputs[0].script_public_key, p2pk_x(&w.w.short_key));
    let short_fair = sp + 1_500 * KAS;
    let short_cheat = sp + 210 * KAS;
    assert_eq!(short_fair, 300_020_000_000, "the short's whole side at 9.00");
    assert_eq!(short_fair - short_cheat, 1_290 * KAS, "the short loses 1,290 KAS");
    assert_eq!(cheat.outputs.last().unwrap().value as i64, 1_290 * KAS, "and they are burned");
    // The long's net: it was paid lp, funded the lock, kept the reward (its change); its ask fill paid itself.
    let net = lp - lock.value as i64 + lock.l.reward();
    let honest_at_market = w.w.refund; // its payoff at 9.00 = bl is 0, plus its refund
    assert_eq!(net, honest_at_market, "the attack costs the long nothing");

    // ---- liquidation
    let l = lv_of(&pair(&lk(), &sk()), 0x62);
    let w = pend(&liq(&l, LONG), LIQ_ASK);
    let winner = xonly(&sk());
    let mut g = guarantee_draft(&w, &winner, &sk(), 7);
    g.set_args(0, guarantee_args(2, &winner));
    let tx = accept(&g);
    let lock = live_locks(&tx, &g).remove(0).at(w.daa + grace());
    let fair = accept(&dispute(&[lock.clone()], &Evidence::bid(1_200_000_000, FULL), &fk()));
    assert_eq!(fair.outputs[0].value as i64, 3_000 * KAS, "on the market's bid at 12 the long is paid its whole side");
    let cheat = accept(&dispute(&[lock.clone()], &own_bid(&sk(), REF_LIQ_LONG, FULL), &sk()));
    assert_eq!(cheat.outputs[0].value as i64, 66_315_790_000, "on the winner's own bid at the bound: 663.16 KAS");
    assert_eq!(cheat.outputs[0].script_public_key, p2pk_x(&w.w.long_key));
    assert_eq!(3_000 * KAS - cheat.outputs[0].value as i64, 233_684_210_000, "the long loses 2,336.84 KAS");
    // The winner's net: the long's side at the market (3,000 KAS) and the fee ride in the forfeited part it holds.
    let net = 3_000 * KAS - lock.value as i64 + lock.l.reward();
    assert_eq!(net, 0, "the attack costs the winner nothing");
}

// ================================================================ F2: a look-alike pending part the tooling calls safe

/// F2 (High; the engine half, the tooling half is `kpf-cli/tests/audit5_cli.rs`). A look-alike pending part (a genesis
/// of the KpfPending template with consistent restore, cure and forfeit hashes, which is all the tooling checks) whose
/// market block has mmrBps = -20,000 and gBps = 20,000, values `Market::validate` and `KpfOpen.checkMarket` refuse.
/// It claims that the short of the DESIGN 5 pair is liquidatable at a bid of 11.50. `KpfPending.guarantee` re-checks
/// only lambda and D, so it settles it; the library's contradiction bound finds no quote that disputes it (the tool
/// prints "no quote in 0..=priceMax disputes this claim") and `check_guarantee` passes; yet `KpfLock.dispute` accepts
/// any ask at or above bs (c * (10^4 - 20,000) <= bs * (10^4 - 20,000)). The
/// restitution is 0 (the "victim" is the attacker's own key anyway), the disputer keeps R, the rest burns: the
/// guarantor loses its whole 3,300 KAS lock, the attacker gains 300 KAS (less any offer it set as bait), and it gets
/// its own pending value back through the settlement.
#[test]
fn exploit_a_look_alike_part_with_an_out_of_range_market_takes_the_guarantors_lock() {
    const LIQ_BID: i64 = 1_150_000_000;
    let mut p = pair(&lk(), &sk());
    p.m.mmr_bps = -20_000;
    p.m.g_bps = 20_000;
    assert!(p.m.validate().is_err(), "no real market has these parameters");
    let l = lv_of(&p, 0x63);
    let w = pend(&liq(&l, SHORT), LIQ_BID);
    // What the tooling sees.
    let lw = kpf_protocol::tx::LivePending { w: w.w.clone(), value: w.value, id: w.id, op: w.op, daa: w.daa };
    assert!(w.w.hashes_match(), "its hashes match the pair its state describes");
    assert!(kpf_protocol::evidence::check_guarantee(&lw, &gk()).is_ok(), "the guarantee builder accepts it");
    assert_eq!(w.w.contradiction_bound(), None, "the tool reports that no quote disputes it");
    // An honest guarantor settles it.
    let g = guarantee_draft(&w, &gk(), &fk(), 5);
    let tx = accept(&g);
    let lock = live_locks(&tx, &g).remove(0).at(w.daa + grace());
    assert_eq!(lock.value as i64, 3_300 * KAS);
    // The attacker disputes with its own ask at 13 KAS (above bs), filled by itself.
    let attacker = keypair(66);
    let d = dispute(&[lock.clone()], &own_ask(&attacker, 1_300_000_000, FULL), &attacker);
    let t = accept(&d);
    assert!(t.outputs.iter().all(|o| o.script_public_key != p2pk_x(&gk())), "nothing returns to the guarantor");
    let burn = t.outputs.last().unwrap();
    assert_eq!(burn.script_public_key, kpf_protocol::lock::burn_spk());
    assert_eq!(burn.value as i64, 3_000 * KAS, "the lock less the reward is burned");
    assert_eq!(lock.l.reward(), 300 * KAS, "the attacker keeps 300 KAS of the guarantor's lock");
    // A real market's part (the same claim, validated parameters) is not disputable by that ask.
    let l2 = lv_of(&pair(&lk(), &sk()), 0x64);
    let w2 = pend(&liq(&l2, SHORT), LIQ_BID);
    assert!(w2.w.contradiction_bound().is_some());
    let g2 = guarantee_draft(&w2, &gk(), &fk(), 8);
    let t2 = accept(&g2);
    let lock2 = live_locks(&t2, &g2).remove(0).at(w2.daa + grace());
    refused(&dispute(&[lock2], &own_ask(&attacker, 1_300_000_000, FULL), &attacker), 0);
}

// ================================================================ holds

/// The dispute window is measured on the lock's own UTXO score F and the claim's P only: the DAA score of the
/// disputer's funding input (very old, or unknown, read as -1) and of the counter order (unaccepted, read as -1:
/// counted from P, never earlier) move nothing. At the window's edge F + D - R the same dispute is accepted whatever
/// the funding score, and one DAA of F later refused.
#[test]
fn holds_the_window_ignores_funding_scores_and_an_unaccepted_counter_order_counts_from_p() {
    let l = lv_of(&pair(&lk(), &sk()), 0x65);
    let w = pend(&liq(&l, LONG), LIQ_ASK);
    let g = guarantee_draft(&w, &gk(), &fk(), 5);
    let tx = accept(&g);
    let lock = live_locks(&tx, &g).remove(0);
    let c = Evidence::bid(1_000_000_000, FULL);
    let rest_end = w.daa + rest_daa();
    let f_edge = rest_end + rest_daa() - dispute_window();
    for fund_daa in [0u64, 1, u64::MAX] {
        let mut d = dispute(&[lock.clone().at(f_edge)], &c, &fk());
        d.lock = rest_end;
        let last = d.ins.len() - 1;
        d.ins[last].utxo.block_daa_score = fund_daa;
        accept(&d);
        let mut d = dispute(&[lock.clone().at(f_edge - 1)], &c, &fk());
        d.lock = rest_end;
        d.ins[last].utxo.block_daa_score = fund_daa;
        refused(&d, 0);
    }
    // A counter order the node has not accepted (score -1) counts from P: one DAA short of P + R refused.
    let unaccepted = c.clone().placed(u64::MAX);
    let mut d = dispute(&[lock.clone().at(w.daa + grace())], &unaccepted, &fk());
    d.lock = rest_end;
    accept(&d);
    d.lock = rest_end - 1;
    refused(&d, 0);
}

/// Once a lock exists the slow path's entries cannot act on the claim, and a second lock cannot be made for it: the
/// pending UTXO is spent. A cure signed by the loser after the guarantee names an outpoint that no longer exists; on
/// the forfeited part (a KpfPair) there is no cure entry, and its owner (both sides the winner's) cannot be made to
/// repay anything by a second guarantee of the same claim (the guarantee entry exists only on KpfPending).
#[test]
fn holds_no_second_guarantee_or_late_cure_reaches_a_fast_settled_claim() {
    let l = lv_of(&pair(&lk(), &sk()), 0x66);
    let w = pend(&liq(&l, LONG), LIQ_ASK);
    let g = guarantee_draft(&w, &gk(), &fk(), 5);
    let tx = accept(&g);
    let part = live_pairs(&tx, &g).remove(0).at(w.daa + grace());
    let pair_entries: Vec<String> = pair_base().contracts.values().next().unwrap().entries.keys().cloned().collect();
    assert!(!pair_entries.iter().any(|e| e == "guarantee" || e == "cure" || e == "challenge" || e == "finalize"));
    assert_eq!(part.p.long_key, part.p.short_key, "one owner: not liquidatable again (DESIGN 4.5e)");
    // The same pending state at a new outpoint is a look-alike only its author funds; the original lock still names
    // the original claim id, so a second lock of the same claim id needs a pending UTXO of that id, which is spent.
    let lock = live_locks(&tx, &g).remove(0);
    assert_eq!(lock.l.claim_id, w.id);
}

/// The victim's restitution is at least the value at the refutation bound whatever order is used, and never more
/// than the cover: across a sweep of counter quotes beyond the bound, for each of the four claims, H is monotone in
/// the counter quote's distance from the bound (the attack of F1 is the worst case, not worse).
#[test]
fn holds_the_restitution_is_never_below_its_value_at_the_bound() {
    let l = lv_of(&pair(&lk(), &sk()), 0x67);
    let w = pend(&liq(&l, LONG), LIQ_ASK);
    let lk_ = lock_of(&w, &gk(), w.daa + grace());
    let at_bound = lk_.l.restitution(REF_LIQ_LONG);
    for b in [REF_LIQ_LONG, 970_000_000, 1_000_000_000, 1_100_000_000, 1_200_000_000, 1_500_000_000] {
        let h = lk_.l.restitution(b);
        assert!(h >= at_bound && h <= lk_.l.cover);
    }
    let w = pend(&close_long(&l), CL_BID);
    let lk_ = lock_of(&w, &gk(), w.daa + 1);
    let at_bound = lk_.l.restitution(REF_CL_LONG);
    for a in [REF_CL_LONG, 1_000_000_000, 950_000_000, 900_000_000, 500_000_000] {
        let h = lk_.l.restitution(a);
        assert!(h >= at_bound && h <= lk_.l.cover);
    }
}
