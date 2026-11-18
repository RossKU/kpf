//! Review 1 (docs/audit/REVIEW-1.md) after the fixes of DESIGN 4.4 and 4.5a.
//!
//! The exploits of the review are kept as scenarios; each `holds_*` test asserts that the engine now refuses the
//! exploiting transaction, or that the step which used to hurt the victim no longer does, and names the rule that
//! stops it. The other tests pin each new rule:
//!   - tags live for one transaction: no entry copies a tag; every pair output carries tag 0 on each side except
//!     the side handed over in that transaction, and there is no live-tag guard (F1, F2);
//!   - the entry price E changes only with both parties' consent: T1 sets it, P3 and P7 keep it, P4 writes e3
//!     with E(donor) <= e3 <= E(host), every other entry keeps it (F3);
//!   - the liquidation fee is capped at the loser's residual, so the winner never pays (F5);
//!   - KpfOpen.fill and take check the evidence parameters with the pair's bounds (F4).
//! It also re-derives the stale-tag double claim of slice 1b against every entry, and records the review's three
//! unproven attacks.

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
const L_ID: Hash = Hash::from_bytes([0x71; 32]);
const N: i64 = 1_000_000_000;
const HALF: i64 = 400_000_000;
const LIQ_ASK: i64 = 940_000_000;
const LIQ_BID: i64 = 1_150_000_000;

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

/// Input `idx` fails; every input in `ok` passes.
fn refused_only(d: &Draft, idx: usize, ok: &[usize]) {
    let (tx, e) = d.build();
    let r = run(&tx, &e).expect("bindings");
    for &o in ok {
        assert!(r[o].is_ok(), "input {o} should accept: {:?}", r[o]);
    }
    assert!(r[idx].is_err(), "input {idx} accepted");
}

/// Some input of the transaction fails.
fn refused_any(d: &Draft, what: &str) {
    let (tx, e) = d.build();
    let r = run(&tx, &e).expect("bindings");
    assert!(r.iter().any(|x| x.is_err()), "{what}: accepted");
}

fn live0(p: &Pair) -> Live {
    Live { p: p.clone(), value: p.floor() as u64, id: PAIR_ID, op: outpoint(10) }
}

fn live_at(p: &Pair, id: u8) -> Live {
    Live { p: p.clone(), value: p.floor() as u64, id: Hash::from_bytes([id; 32]), op: outpoint(id) }
}

/// A direct-taker transfer of the whole long side of `l` at x: entrant key ek, tag et, new bound nb.
fn take_long(l: &Live, x: i64, nb: i64, ek: &Keypair, et: Hash, fn_: u32) -> (Live, Transaction, Draft) {
    let t = TransferSpec { side: LONG, n: l.p.size, x, nb, ek: xonly(ek), et, k: 0 };
    let d = transfer_draft(l, &t, &Entrant::Direct(*ek), &fk(), fn_);
    let tx = accept_d(&d);
    (live_pairs(&tx, &d).remove(0), tx, d)
}

fn add_margin_draft(l: &Live, signer: &Keypair, side: i64, delta: i64) -> Draft {
    let mut n = l.p.cleared();
    if side == LONG {
        n.bl -= delta;
    } else {
        n.bs += delta;
    }
    let add = q(l.p.size, delta, l.p.m.scale, l.p.m.scale - 1) as u64;
    let ins = vec![pair_input(l, "addMargin", vec![Arg::Sig(*signer), iv(side), iv(delta)]), funder(&fk(), 90, F_FUNDS)];
    let outs = vec![out(l.value + add, p2sh(&n.art()), bind(0, l.id)), out(0, p2pk_spk(&fk()), None)];
    let mut d = Draft::new(ins, outs, Some(1));
    d.pairs.push((0, n));
    d
}

fn coop_draft(l: &Live, mut outs: Vec<TransactionOutput>) -> Draft {
    let ins = vec![pair_input(l, "coopClose", vec![Arg::Sig(lk()), Arg::Sig(sk())]), funder(&fk(), 91, F_FUNDS)];
    let ch = outs.len();
    outs.push(out(0, p2pk_spk(&fk()), None));
    Draft::new(ins, outs, Some(ch))
}

/// The short's own defences on a pair, each accepted: amend (resets the idle age), add margin, and a P5 close at
/// an ask.
fn short_defences_work(part: &Live) {
    accept_d(&amend_draft(part, &sk(), SHORT, (950_000_000, 0, 0), &fk(), 80));
    accept_d(&add_margin_draft(part, &sk(), SHORT, 10_000_000));
    let leg = PairLeg { l: part.clone(), n: part.p.size, q: 0, act: Act::Close { side: SHORT, signer: sk() } };
    accept_d(&ev_draft(&[leg], &Evidence::ask(1_100_000_000, part.p.size), &fk(), 81));
}

/// The old transfer shape (part written with E = x, and the tag of the stayer copied) on a draft built by the
/// fixtures, at output `idx`.
fn with_old_entry(d: &Draft, idx: usize, x: i64) -> Draft {
    let mut p = d.pairs.iter().find(|(i, _)| *i == idx).unwrap().1.clone();
    p.entry = x;
    let mut d2 = d.clone();
    d2.set_pair(idx, &p);
    d2
}

// ---------------------------------------------------------------- F1 (Critical)

/// holds_self_tag_freeze_then_idle_theft (F1). Scenario of the exploit: the long lists at ask = bs, takes its
/// own listing as a direct taker at x = bs with tag = the pair's own id, then waits for idle settle.
/// Rules that stop it: (1) there is no live-tag guard (DESIGN 4.4), so the tag freezes nothing: the short's
/// amend, add margin and P5 close all work on the part; (2) the transfer keeps E (DESIGN 4.5a), so the old
/// exploiting transaction, which wrote E = bs into the part, is refused, and idle settle pays the short its
/// payoff at the entry it consented to (2,000 KAS less the long's 1,000 KAS gain at 10 KAS: q(s, bs - E)).
#[test]
fn holds_self_tag_freeze_then_idle_theft() {
    let p = pair(&lk(), &sk());
    let l = live0(&p);
    let d = amend_draft(&l, &lk(), LONG, (p.bs, 0, 0), &fk(), 1);
    let tx = accept_d(&d);
    let l2 = live_pairs(&tx, &d).remove(0);
    // The self-transfer itself is a legitimate fill of the long's own listing. With E = x it is refused.
    let t = TransferSpec { side: LONG, n: N, x: p.bs, nb: p.bl, ek: xonly(&lk()), et: l2.id, k: 0 };
    let dt = transfer_draft(&l2, &t, &Entrant::Direct(lk()), &fk(), 2);
    refused0(&with_old_entry(&dt, 2, p.bs));
    let (part, _, _) = take_long(&l2, p.bs, p.bl, &lk(), l2.id, 2);
    assert_eq!((part.p.tag_l, part.p.tag_s, part.p.entry), (part.id, ZERO, p.entry));
    // The tag written by the attacker blocks nothing.
    short_defences_work(&part);
    // Idle settle pays at the kept entry.
    let tx = accept_d(&idle_draft(&part, &fk(), 3));
    assert_eq!(tx.outputs[0].value as i64, q(N, p.entry - p.bl, SCALE, 0));
    assert_eq!(tx.outputs[2].value as i64, part.value as i64 - q(N, p.entry - p.bl, SCALE, 0));
    assert_eq!(tx.outputs[2].value as i64, p.short_at(p.entry) + C);
}

/// holds_short_self_tag_freeze_then_idle_theft (F1, mirrored): the short lists shortBid = bl, takes it itself at
/// x = 0 with tag = its own id. Rules: no live-tag guard (the long's amend works) and E kept (the old part with
/// E = 0 is refused), so idle settle pays the long q(s, E - bl), not nothing.
#[test]
fn holds_short_self_tag_freeze_then_idle_theft() {
    let p = pair(&lk(), &sk());
    let l = live0(&p);
    let d = amend_draft(&l, &sk(), SHORT, (p.bl, 0, 0), &fk(), 4);
    let tx = accept_d(&d);
    let l2 = live_pairs(&tx, &d).remove(0);
    let t = TransferSpec { side: SHORT, n: N, x: 0, nb: p.bs, ek: xonly(&sk()), et: l2.id, k: 0 };
    let d = transfer_draft(&l2, &t, &Entrant::Direct(sk()), &fk(), 5);
    refused0(&with_old_entry(&d, 2, 0));
    let tx = accept_d(&d);
    let part = live_pairs(&tx, &d).remove(0);
    assert_eq!((part.p.tag_s, part.p.tag_l, part.p.entry), (part.id, ZERO, p.entry));
    accept_d(&amend_draft(&part, &lk(), LONG, (1_100_000_000, 0, 0), &fk(), 6));
    let tx = accept_d(&idle_draft(&part, &fk(), 7));
    assert_eq!(tx.outputs[0].value as i64, q(N, p.entry - p.bl, SCALE, 0));
}

/// holds_third_party_freeze (F1, griefing variant): a taker of an ordinary listing writes et = the pair id.
/// Rule: no live-tag guard. The stayer keeps every defence and the new long can be liquidated.
#[test]
fn holds_third_party_freeze() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (1_000_000_000, 0, 0);
    let l = live0(&p);
    let (part, _, _) = take_long(&l, 1_000_000_000, 850_000_000, &taker(), PAIR_ID, 8);
    assert_eq!(part.p.tag_l, part.id);
    short_defences_work(&part);
    let leg = PairLeg { l: part.clone(), n: N, q: 0, act: Act::Liq { side: LONG } };
    accept_d(&ev_draft(&[leg], &Evidence::ask(800_000_000, N), &fk(), 9));
}

/// holds_frozen_pair_coop_close_still_works: coopClose, the former only release, still works on a self-tagged
/// part (control; it needs both signatures).
#[test]
fn holds_frozen_pair_coop_close_still_works() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (p.bs, 0, 0);
    let l = live0(&p);
    let (part, _, _) = take_long(&l, p.bs, p.bl, &lk(), PAIR_ID, 11);
    let half = part.value / 2;
    accept_d(&coop_draft(&part, vec![out(half, p2pk_spk(&lk()), None), out(part.value - half, p2pk_spk(&sk()), None)]));
}

// ---------------------------------------------------------------- F2 (High)

/// holds_token_id_tag_blocks_evidence (F2): a direct taker writes et = the market token's covenant id. Rule: no
/// live-tag guard, so the evidence transactions that used to be refused (they all carry a token input of that
/// id) are accepted: liquidation of the new long at an ask, the short's P5 at an ask, the long's P5 at a bid.
#[test]
fn holds_token_id_tag_blocks_evidence() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (1_000_000_000, 0, 0);
    let l = live0(&p);
    let tok = market().token_cov_id;
    let (part, _, _) = take_long(&l, 1_000_000_000, p.bl, &taker(), tok, 12);
    assert_eq!(part.p.tag_l, tok);
    let leg = |x: &Live| PairLeg { l: x.clone(), n: N, q: 0, act: Act::Liq { side: LONG } };
    accept_d(&ev_draft(&[leg(&part)], &Evidence::ask(LIQ_ASK, N), &fk(), 14));
    let close_s = PairLeg { l: part.clone(), n: N, q: 0, act: Act::Close { side: SHORT, signer: sk() } };
    accept_d(&ev_draft(&[close_s], &Evidence::ask(1_100_000_000, N), &fk(), 15));
    let close_l = PairLeg { l: part.clone(), n: N, q: 0, act: Act::Close { side: LONG, signer: taker() } };
    accept_d(&ev_draft(&[close_l], &Evidence::bid(1_050_000_000, N), &fk(), 16));
    accept_d(&amend_draft(&part, &sk(), SHORT, (950_000_000, 0, 0), &fk(), 17));
}

// ---------------------------------------------------------------- F3 (Medium)

/// holds_entry_price_self_transfer (F3): the long takes its own listing at x = bs (no tag trick). Rule: P3
/// keeps E (DESIGN 4.5a); the exploiting part with E = bs is refused, and idle settle pays the short at the
/// consented E.
#[test]
fn holds_entry_price_self_transfer() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (p.bs, 0, 0);
    let l = live0(&p);
    let t = TransferSpec { side: LONG, n: N, x: p.bs, nb: p.bl, ek: xonly(&lk()), et: ZERO, k: 0 };
    refused0(&with_old_entry(&transfer_draft(&l, &t, &Entrant::Direct(lk()), &fk(), 18), 2, p.bs));
    let (part, _, _) = take_long(&l, p.bs, p.bl, &lk(), ZERO, 18);
    assert_eq!(part.p.entry, p.entry);
    let tx = accept_d(&idle_draft(&part, &fk(), 19));
    assert_eq!(tx.outputs[2].value as i64, p.short_at(p.entry) + C);
}

/// holds_ordinary_transfer_tag_does_not_freeze: control. With et = 0 the stayer keeps its defences.
#[test]
fn holds_ordinary_transfer_tag_does_not_freeze() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (1_000_000_000, 0, 0);
    let l = live0(&p);
    let (part, _, _) = take_long(&l, 1_000_000_000, 850_000_000, &taker(), ZERO, 20);
    short_defences_work(&part);
}

// ---------------------------------------------------------------- tags live for one transaction

/// Pairs with both sides listed and old tags (0x1a / 0x1b) in their state.
fn listed() -> Pair {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (1_000_000_000, 0, 0);
    (p.short_bid, p.short_min, p.short_tip) = (1_050_000_000, 0, 0);
    p
}

/// Every honest draft of every entry: (name, draft, the (side, tag) its hand-over writes).
fn every_entry() -> Vec<(&'static str, Draft, Vec<(i64, Hash)>)> {
    let l = live_at(&listed(), 0x31);
    let o_long = LiveOpen { value: long_order(&keypair(21)).escrow() as u64, o: long_order(&keypair(21)), id: L_ID, op: outpoint(0x61) };
    let o_short = LiveOpen { value: short_order(&keypair(22)).escrow() as u64, o: short_order(&keypair(22)), id: Hash::from_bytes([0x72; 32]), op: outpoint(0x62) };
    let mut v: Vec<(&'static str, Draft, Vec<(i64, Hash)>)> = vec![];
    v.push(("amend", amend_draft(&l, &lk(), LONG, (1_010_000_000, 0, 0), &fk(), 1), vec![]));
    v.push(("addMargin", add_margin_draft(&l, &sk(), SHORT, 10_000_000), vec![]));
    for n in [N, HALF] {
        let t = TransferSpec { side: LONG, n, x: 1_000_000_000, nb: 900_000_000, ek: o_long.o.owner, et: L_ID, k: 0 };
        v.push(("transfer long by KpfOpen", transfer_draft(&l, &t, &Entrant::Open(o_long.clone()), &fk(), 2), vec![(LONG, L_ID)]));
        let t = TransferSpec { side: SHORT, n, x: 1_050_000_000, nb: 1_200_000_000, ek: xonly(&taker()), et: PAIR_ID, k: 1 };
        v.push(("transfer short direct", transfer_draft(&l, &t, &Entrant::Direct(taker()), &fk(), 3), vec![(SHORT, PAIR_ID)]));
        v.push(("mutualClose", mutual_draft(&l, n, 1_020_000_000, &fk(), 4), vec![]));
        let mut dp = pair(&keypair(31), &keypair(32));
        (dp.bl, dp.bs, dp.short_bid) = (850_000_000, 1_150_000_000, 1_030_000_000);
        let d = live_at(&dp, 0x33);
        v.push(("closeHost / closeDonor", close_draft(&l, &d, &CloseSpec { n, x: 1_020_000_000, e3: 1_000_000_000 }, &fk(), 5), vec![(LONG, d.id)]));
        let leg = |act: Act| PairLeg { l: l.clone(), n, q: 0, act };
        v.push(("P5 long", ev_draft(&[leg(Act::Close { side: LONG, signer: lk() })], &Evidence::bid(1_050_000_000, n), &fk(), 6), vec![]));
        v.push(("P5 short", ev_draft(&[leg(Act::Close { side: SHORT, signer: sk() })], &Evidence::ask(1_100_000_000, n), &fk(), 7), vec![]));
        v.push(("P6 long", ev_draft(&[leg(Act::Liq { side: LONG })], &Evidence::ask(LIQ_ASK, n), &fk(), 8), vec![]));
        v.push(("P6 short", ev_draft(&[leg(Act::Liq { side: SHORT })], &Evidence::bid(LIQ_BID, n), &fk(), 9), vec![]));
        let p7l = Act::LiqTransfer { side: LONG, x: 950_000_000, open: o_long.clone(), k: 0 };
        v.push(("P7 long", ev_draft(&[leg(p7l)], &Evidence::ask(LIQ_ASK, n), &fk(), 10), vec![(LONG, L_ID)]));
        let p7s = Act::LiqTransfer { side: SHORT, x: 1_140_000_000, open: o_short.clone(), k: 1 };
        v.push(("P7 short", ev_draft(&[leg(p7s)], &Evidence::bid(LIQ_BID, n), &fk(), 11), vec![(SHORT, o_short.id)]));
    }
    v
}

/// holds_no_entry_copies_a_tag (DESIGN 4.4): on every continuation of every entry, full and partial, both tags
/// are 0 except the side handed over in that transaction, and a continuation that copies the input's old tag on a
/// side that was not handed over is refused.
#[test]
fn holds_no_entry_copies_a_tag() {
    let old = listed();
    for (name, d, handed) in every_entry() {
        accept_d(&d);
        for (idx, p) in &d.pairs {
            for (side, t) in [(LONG, p.tag_l), (SHORT, p.tag_s)] {
                assert!(t == ZERO || handed.contains(&(side, t)), "{name}: output {idx} side {side} carries tag {t}");
            }
            for (side, old_tag) in [(LONG, old.tag_l), (SHORT, old.tag_s)] {
                let tag = if side == LONG { p.tag_l } else { p.tag_s };
                if tag != ZERO {
                    continue;
                }
                let mut c = p.clone();
                if side == LONG {
                    c.tag_l = old_tag;
                } else {
                    c.tag_s = old_tag;
                }
                let mut d2 = d.clone();
                d2.set_pair(*idx, &c);
                refused_any(&d2, &format!("{name}: output {idx} copies the old tag on side {side}"));
            }
        }
    }
}

/// holds_entry_kept_by_every_entry_but_t1_and_p4 (DESIGN 4.5a): every continuation of every entry other than
/// P4's P3 keeps the input's E (transfer part and remainder, P7 part and remainder, every remainder, amend, add
/// margin), and one written with another E (the old P3/P7 shape E = x included) is refused.
#[test]
fn holds_entry_kept_by_every_entry_but_t1_and_p4() {
    let old = listed();
    for (name, d, handed) in every_entry() {
        for (idx, p) in &d.pairs {
            let is_p3 = name.starts_with("closeHost") && handed.contains(&(LONG, p.tag_l)) && p.tag_l != ZERO;
            if is_p3 {
                continue;
            }
            assert_eq!(p.entry, old.entry, "{name}: output {idx} changed E");
            for e in [old.entry + 1, old.entry - 1, 1_000_000_000 + 20_000_000, 950_000_000] {
                if e == old.entry {
                    continue;
                }
                let mut c = p.clone();
                c.entry = e;
                let mut d2 = d.clone();
                d2.set_pair(*idx, &c);
                refused_any(&d2, &format!("{name}: output {idx} with E = {e}"));
            }
        }
    }
}

/// The stale-tag double payment of slice 1b, re-derived against every entry. An older pair Q carries the stale
/// tag tagL = id(L') of a live long KpfOpen L' whose owner already is Q's long at L''s rate. For every entry of
/// Q that does not hand its long over to L' (amend, add margin, a short transfer, P5, P6, P7 of the short, and
/// Q as the donor of P4, whose long moves into the host's P3), and for every pair output of that transaction, L'
/// is added (last input) with its `take` pointed at that output. With the output as the entry writes it, L'
/// refuses: no entry copies a tag, and the only tags it writes are the hand-over's (DESIGN 4.4). With the stale tag
/// copied into the output (the shape L' would accept), the pair refuses. Input order does not matter to either
/// check (every index is an argument); a second test puts L' first.
#[test]
fn holds_stale_tag_never_satisfies_a_live_claimer() {
    // Q: the base pair, long unlisted (so L''s rebuilt long side matches), short listed, stale tagL = id(L').
    let mut qp = pair(&lk(), &sk());
    qp.tag_l = L_ID;
    (qp.short_bid, qp.short_min, qp.short_tip) = (1_050_000_000, 0, 0);
    let q = live_at(&qp, 0x41);
    // L': owner = Q's long, rate 1 KAS, so a long at bl = 9 KAS is "its" position at x = 10 KAS (E = 10 >= x).
    let mut lo = long_order(&lk());
    lo.size_left = 4 * N;
    let l1 = LiveOpen { value: lo.escrow() as u64, o: lo, id: L_ID, op: outpoint(0x51) };
    let x0 = qp.bl + l1.o.my_rate;
    let short_ent = LiveOpen { value: short_order(&keypair(22)).escrow() as u64, o: short_order(&keypair(22)), id: Hash::from_bytes([0x73; 32]), op: outpoint(0x52) };
    let mut host = pair(&keypair(31), &keypair(32));
    (host.long_ask, host.long_min, host.long_tip) = (1_000_000_000, 0, 0);
    let host = live_at(&host, 0x42);
    let leg = |n: i64, act: Act| PairLeg { l: q.clone(), n, q: 0, act };
    let cases: Vec<(&str, Draft, usize)> = vec![
        ("amend", amend_draft(&q, &sk(), SHORT, (1_000_000_000, 0, 0), &fk(), 1), 0),
        ("addMargin", add_margin_draft(&q, &sk(), SHORT, 10_000_000), 0),
        ("transfer short", transfer_draft(&q, &TransferSpec { side: SHORT, n: HALF, x: 1_050_000_000, nb: 1_200_000_000, ek: xonly(&taker()), et: ZERO, k: 0 }, &Entrant::Direct(taker()), &fk(), 2), 0),
        ("P5 short", ev_draft(&[leg(HALF, Act::Close { side: SHORT, signer: sk() })], &Evidence::ask(1_100_000_000, HALF), &fk(), 3), 1),
        ("P6 short", ev_draft(&[leg(HALF, Act::Liq { side: SHORT })], &Evidence::bid(LIQ_BID, HALF), &fk(), 4), 1),
        ("P7 short", ev_draft(&[leg(HALF, Act::LiqTransfer { side: SHORT, x: 1_140_000_000, open: short_ent, k: 0 })], &Evidence::bid(LIQ_BID, HALF), &fk(), 5), 0),
        ("P4 donor", close_draft(&host, &q, &CloseSpec { n: HALF, x: 1_020_000_000, e3: 1_000_000_000 }, &fk(), 6), 0),
    ];
    let mut checked = 0;
    for (name, d, tail) in cases {
        accept_d(&d);
        for (idx, p) in d.pairs.clone() {
            if p.long_key != qp.long_key || p.bl != qp.bl || p.long_ask != 0 {
                continue; // L' could not match this output even with its tag.
            }
            let n = p.size;
            let li = d.ins.len();
            let pos = d.outs.len() - tail;
            let mut dd = d.clone();
            let (oo, _) = order_out(&l1, n, li as u16);
            dd.outs.insert(pos, oo);
            if let Some(c) = dd.change {
                if c >= pos {
                    dd.change = Some(c + 1);
                }
            }
            for e in dd.pairs.iter_mut() {
                if e.0 >= pos {
                    e.0 += 1;
                }
            }
            let at = if idx >= pos { idx + 1 } else { idx };
            let mut stale = p.clone();
            (stale.tag_l, stale.tag_s) = (L_ID, ZERO);
            dd.ins.push(open_input(&l1, "take", take_args(&stale, LONG, at as i64, n, x0)));
            let others: Vec<usize> = (0..li).collect();
            refused_only(&dd, li, &others);
            let mut d2 = dd.clone();
            d2.set_pair(at, &stale);
            let (tx, e) = d2.build();
            let r = run(&tx, &e).unwrap();
            assert!(r[li].is_ok(), "{name}: L' should accept the stale-tag output {idx}: {:?}", r[li]);
            assert!(r[..li].iter().any(|x| x.is_err()), "{name}: the stale-tag output {idx} was accepted by the pair");
            checked += 1;
        }
    }
    assert!(checked >= 9, "only {checked} outputs checked");
}

/// As above with the claimer before the pair in the inputs: in 0 L' take, 1 Q amend, 2 F.
#[test]
fn holds_stale_tag_claimer_first_in_the_inputs() {
    let mut qp = pair(&lk(), &sk());
    qp.tag_l = L_ID;
    let q = live_at(&qp, 0x43);
    let mut lo = long_order(&lk());
    lo.size_left = 4 * N;
    let l1 = LiveOpen { value: lo.escrow() as u64, o: lo, id: L_ID, op: outpoint(0x53) };
    let a = amend_draft(&q, &sk(), SHORT, (1_000_000_000, 0, 0), &fk(), 7);
    let cont = a.pairs[0].1.clone();
    let mut stale = cont.clone();
    stale.tag_l = L_ID;
    let (oo, _) = order_out(&l1, N, 0);
    let mk = |p: &Pair| {
        let ins = vec![open_input(&l1, "take", take_args(&stale, LONG, 1, N, qp.bl + l1.o.my_rate)), a.ins[0].clone(), a.ins[1].clone()];
        let outs = vec![oo.clone(), out(q.value, p2sh(&p.art()), bind(1, q.id)), out(0, p2pk_spk(&fk()), None)];
        Draft::new(ins, outs, Some(2))
    };
    refused_only(&mk(&cont), 0, &[1]);
    refused_only(&mk(&stale), 1, &[0]);
}

// ---------------------------------------------------------------- entry price of close-versus-close

fn e3_pairs(eh: i64, ed: i64) -> (Live, Live) {
    let mut h = pair(&lk(), &sk());
    (h.long_ask, h.entry) = (1_000_000_000, eh);
    let mut d = pair(&keypair(21), &keypair(22));
    (d.bl, d.bs, d.short_bid, d.entry) = (850_000_000, 1_150_000_000, 1_030_000_000, ed);
    (live_at(&h, 0x5a), live_at(&d, 0x5b))
}

fn cvc(h: &Live, d: &Live, e3: i64) -> Draft {
    close_draft(h, d, &CloseSpec { n: HALF, x: 1_020_000_000, e3 }, &fk(), 3)
}

/// holds_e3_bounds (DESIGN 4.5a): P3's entry e3 must satisfy E(donor) <= e3 <= E(host). The host (whose short
/// stays) checks e3 <= E(host); the donor (whose long moves) checks e3 >= E(donor).
#[test]
fn holds_e3_bounds() {
    let (eh, ed) = (1_050_000_000, 980_000_000);
    let (h, d) = e3_pairs(eh, ed);
    for e3 in [ed, (ed + eh) / 2, eh] {
        let dr = cvc(&h, &d, e3);
        accept_d(&dr);
        assert_eq!(dr.pairs[0].1.entry, e3);
    }
    refused_only(&cvc(&h, &d, eh + 1), 0, &[1]);
    refused_only(&cvc(&h, &d, ed - 1), 1, &[0]);
    // The fill price is not the entry: x = 10.2 KAS lies inside the bounds, but P3 written with E = x while the
    // pairs are told e3 = E(donor) is refused by both.
    let mut dr = cvc(&h, &d, ed);
    let mut p3 = dr.pairs[0].1.clone();
    p3.entry = 1_020_000_000;
    dr.set_pair(2, &p3);
    let (tx, e) = dr.build();
    let r = run(&tx, &e).unwrap();
    assert!(r[0].is_err() && r[1].is_err());
    // The two pairs told different e3: the bytes differ, the one whose e3 is not written refuses.
    let mut dr = cvc(&h, &d, ed);
    let mut a = dr.args(1);
    a[2] = iv(eh);
    dr.set_args(1, a);
    refused_only(&dr, 1, &[0]);
}

/// holds_e3_refused_when_donor_entry_above_host_entry (DESIGN 4.5a): with E(donor) > E(host) no e3 exists, and
/// every candidate is refused by one of the two pairs.
#[test]
fn holds_e3_refused_when_donor_entry_above_host_entry() {
    let (eh, ed) = (1_000_000_000, 1_100_000_000);
    let (h, d) = e3_pairs(eh, ed);
    for e3 in [eh - 1, eh, (eh + ed) / 2, ed, ed + 1] {
        refused_any(&cvc(&h, &d, e3), &format!("e3 = {e3}"));
    }
    refused_only(&cvc(&h, &d, eh), 1, &[0]);
    refused_only(&cvc(&h, &d, ed), 0, &[1]);
}

// ---------------------------------------------------------------- the fee (F5)

/// holds_fee_capped_at_the_loser_residual (F5, DESIGN 4.5a): a long liquidated at an ask just above its
/// bankruptcy price owes a fee (4.5 KAS per token at 9 KAS, 0.5 %) far above its residual (0.001 KAS per token).
/// The fee is capped at the residual: the winner (the short) gets exactly q(n, bs - a), whole or in part, and a
/// winner short of anything is refused.
#[test]
fn holds_fee_capped_at_the_loser_residual() {
    let p = pair(&lk(), &sk());
    let a = p.bl + 1_000;
    for n in [N, HALF] {
        let l = live_at(&p, 0x61);
        let fee = fee_of(&p.m, n, a);
        assert!(fee > q(n, a - p.bl, SCALE, 0) + C);
        let d = ev_draft(&[PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side: LONG } }], &Evidence::ask(a, n), &fk(), 12);
        let tx = accept_d(&d);
        let m = tx.outputs.len();
        assert_eq!(tx.outputs[m - 1].value as i64, q(n, p.bs - a, SCALE, 0), "the winner gets its whole distance");
        assert_eq!(tx.outputs[0].value, 0, "the loser's residual all went to the fee (or stayed under dust)");
        let mut x = d.clone();
        x.outs[m - 1].value -= 1;
        refused0(&x);
    }
}

/// holds_fee_floor_never_reaches_the_winner_in_pieces (F5): a market whose fee floor (10 KAS) exceeds the
/// loser's residual of a minimum-size piece; the long is liquidated one token at a time. Every piece pays the
/// winner exactly q(1 token, bs - a); over the whole pair the winner receives q(s, bs - a), no fee.
#[test]
fn holds_fee_floor_never_reaches_the_winner_in_pieces() {
    let mut p = pair(&lk(), &sk()).with_size(5 * MIN_SIZE);
    p.m.keeper_fee_min = 1_000_000_000;
    let a = p.bl + 1_000_000;
    let mut l = live_at(&p, 0x62);
    let mut winner = 0i64;
    for i in 0..5 {
        let n = MIN_SIZE;
        let d = ev_draft(&[PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side: LONG } }], &Evidence::ask(a, n), &fk(), 20 + i);
        let tx = accept_d(&d);
        let m = tx.outputs.len();
        let got = tx.outputs[m - 1].value as i64;
        assert_eq!(got, q(n, p.bs - a, SCALE, 0), "piece {i}");
        winner += got;
        let rest = live_pairs(&tx, &d);
        if rest.is_empty() {
            assert_eq!(i, 4);
            break;
        }
        l = rest[0].clone();
    }
    assert_eq!(winner, q(p.size, p.bs - a, SCALE, 0));
}

// ---------------------------------------------------------------- KpfOpen evidence parameters (F4)

fn bad_markets() -> Vec<(&'static str, Market)> {
    let m = market();
    let mut v = vec![];
    let mut add = |name: &'static str, f: &dyn Fn(&mut Market)| {
        let mut x = m.clone();
        f(&mut x);
        v.push((name, x));
    };
    add("kBps 0", &|x| x.k_bps = 0);
    add("kBps above 10^5", &|x| x.k_bps = 100_001);
    add("mmrBps negative", &|x| x.mmr_bps = -1);
    add("mmrBps 5000", &|x| x.mmr_bps = 5_000);
    add("liqFeeBps negative", &|x| x.liq_fee_bps = -1);
    add("liqFeeBps above 10^4", &|x| x.liq_fee_bps = 10_001);
    add("keeperFeeMin negative", &|x| x.keeper_fee_min = -1);
    add("minRestDaa negative", &|x| x.min_rest_daa = -1);
    add("minRestDaa at the lock-time threshold", &|x| x.min_rest_daa = 500_000_000_000);
    v
}

fn edge_markets() -> Vec<Market> {
    let m = market();
    let mut a = m.clone();
    (a.k_bps, a.mmr_bps, a.liq_fee_bps, a.keeper_fee_min, a.min_rest_daa) = (100_000, 4_999, 10_000, 0, 0);
    let mut b = m.clone();
    (b.k_bps, b.mmr_bps, b.liq_fee_bps, b.min_rest_daa) = (1, 0, 0, 499_999_999_999);
    vec![a, b]
}

fn orders_of(m: &Market) -> (LiveOpen, LiveOpen) {
    let mut lo = long_order(&keypair(21));
    lo.m = m.clone();
    let mut so = short_order(&keypair(22));
    so.m = m.clone();
    (
        LiveOpen { value: lo.escrow() as u64, o: lo, id: Hash::from_bytes([0x81; 32]), op: outpoint(0x81) },
        LiveOpen { value: so.escrow() as u64, o: so, id: Hash::from_bytes([0x82; 32]), op: outpoint(0x82) },
    )
}

/// holds_fill_refuses_malformed_evidence_parameters (F4): T1 between two orders of a market whose evidence
/// parameters the pair would refuse on P5/P6/P7: both orders refuse. The bounds themselves are accepted.
#[test]
fn holds_fill_refuses_malformed_evidence_parameters() {
    for (name, m) in bad_markets() {
        let (lo, so) = orders_of(&m);
        let (tx, e) = open_draft(&lo, &so, N, 1_000_000_000, &fk(), 30).build();
        let r = run(&tx, &e).unwrap();
        assert!(r[0].is_err() && r[1].is_err(), "{name}: {r:?}");
    }
    for m in edge_markets() {
        let (lo, so) = orders_of(&m);
        accept_d(&open_draft(&lo, &so, N, 1_000_000_000, &fk(), 31));
    }
}

/// holds_take_refuses_malformed_evidence_parameters (F4): a transfer into a pair of such a market. The pair's
/// transfer does not read the evidence parameters and accepts; the order refuses.
#[test]
fn holds_take_refuses_malformed_evidence_parameters() {
    let all: Vec<(&str, Market)> = bad_markets().into_iter().chain(edge_markets().into_iter().map(|m| ("edge", m))).collect();
    for (name, m) in all {
        let mut p = pair(&lk(), &sk());
        p.m = m.clone();
        (p.long_ask, p.long_min, p.long_tip) = (1_000_000_000, 0, 0);
        let l = live_at(&p, 0x63);
        let (lo, _) = orders_of(&m);
        let t = TransferSpec { side: LONG, n: HALF, x: 1_000_000_000, nb: 900_000_000, ek: lo.o.owner, et: lo.id, k: 0 };
        let d = transfer_draft(&l, &t, &Entrant::Open(lo), &fk(), 32);
        if name == "edge" {
            accept_d(&d);
        } else {
            refused_only(&d, 1, &[0]);
        }
    }
}

/// holds_take_refuses_an_entry_below_its_price (DESIGN deviation in KpfOpen.take): a long order accepts the
/// kept E only at e >= x, a short order only at e <= x, so an absent maker is never idle-settled below the price
/// it entered at. The pair itself accepts the transfer.
#[test]
fn holds_take_refuses_an_entry_below_its_price() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.short_bid) = (1_000_000_000, 1_050_000_000);
    p.entry = 999_999_999;
    let l = live_at(&p, 0x64);
    let lo = orders_of(&market()).0;
    let t = TransferSpec { side: LONG, n: HALF, x: 1_000_000_000, nb: 900_000_000, ek: lo.o.owner, et: lo.id, k: 0 };
    refused_only(&transfer_draft(&l, &t, &Entrant::Open(lo.clone()), &fk(), 33), 1, &[0]);
    p.entry = 1_000_000_000;
    accept_d(&transfer_draft(&live_at(&p, 0x64), &t, &Entrant::Open(lo), &fk(), 33));
    // Short: E above x is refused, E = x accepted.
    let mut so = short_order(&keypair(22));
    (so.limit, so.my_rate) = (1_000_000_000, 300_000_000);
    let so = LiveOpen { value: so.escrow() as u64, o: so, id: Hash::from_bytes([0x83; 32]), op: outpoint(0x83) };
    p.entry = 1_050_000_001;
    let t = TransferSpec { side: SHORT, n: HALF, x: 1_050_000_000, nb: 1_350_000_000, ek: so.o.owner, et: so.id, k: 0 };
    refused_only(&transfer_draft(&live_at(&p, 0x65), &t, &Entrant::Open(so.clone()), &fk(), 34), 1, &[0]);
    p.entry = 1_050_000_000;
    accept_d(&transfer_draft(&live_at(&p, 0x65), &t, &Entrant::Open(so), &fk(), 34));
}

// ---------------------------------------------------------------- the review's unproven attacks

/// holds_take_part_with_attacker_twin_tag (review 1, unproven 1). The attack: an honest KpfOpen takes a listing
/// of a pair whose covenant id Y an attacker also holds in a twin UTXO, with counterTag = Y, so that the old
/// live-tag guard (OpCovInputCount(Y) >= 1, the part itself) would freeze the honest order's new position. Rules
/// that stop it: take no longer has a counterTag argument and rebuilds the stayer's tag as 0, and the pair writes
/// 0 there too, so the part cannot carry Y on the stayer's side (both refuse); and with no live-tag guard a tag
/// of value Y would freeze nothing anyway (holds_self_tag_freeze_then_idle_theft). The twin UTXO itself is not
/// needed: no check reads a tag as a live id any more.
#[test]
fn holds_take_part_with_attacker_twin_tag() {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (1_000_000_000, 0, 0);
    let l = live0(&p);
    let lo = orders_of(&market()).0;
    let t = TransferSpec { side: LONG, n: HALF, x: 1_000_000_000, nb: 900_000_000, ek: lo.o.owner, et: lo.id, k: 0 };
    let d = transfer_draft(&l, &t, &Entrant::Open(lo.clone()), &fk(), 35);
    accept_d(&d);
    let mut part = d.pairs[0].1.clone();
    part.tag_s = PAIR_ID;
    let mut d2 = d.clone();
    d2.set_pair(2, &part);
    let (tx, e) = d2.build();
    let r = run(&tx, &e).unwrap();
    assert!(r[0].is_err() && r[1].is_err(), "{r:?}");
}

/// holds_close_donor_p3_with_inherited_id_tag (review 1, unproven 2). The attack: closeDonor's P3 written with
/// the short's tag ts3 = pid, the covenant id P3 lives under (an id the attacker could hold a twin of), freezing
/// the moved long as in F1. Rules that stop it: closeDonor has no ts3 argument and rebuilds the host short's tag
/// as 0, and closeHost writes 0 there, so both refuse such a P3.
#[test]
fn holds_close_donor_p3_with_inherited_id_tag() {
    let (h, d) = e3_pairs(1_000_000_000, 1_000_000_000);
    let dr = cvc(&h, &d, 1_000_000_000);
    accept_d(&dr);
    let mut p3 = dr.pairs[0].1.clone();
    p3.tag_s = h.id;
    let mut d2 = dr.clone();
    d2.set_pair(2, &p3);
    let (tx, e) = d2.build();
    let r = run(&tx, &e).unwrap();
    assert!(r[0].is_err() && r[1].is_err(), "{r:?}");
}
