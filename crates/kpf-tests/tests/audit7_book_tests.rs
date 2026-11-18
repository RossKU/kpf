//! Review 7 (docs/audit/REVIEW-7.md), item "book family": the aliasing class of R7-F1 ("two inputs satisfied by one
//! output") checked against the BOOK-market family (`KpfPair`, `KpfOpen`, `KpfPending`, `KpfLock`), with no contract
//! changed. Every transaction here runs every input in the real script engine with the covenant context
//! (`common::run`); every `holds_` test builds the merge and asserts that the engine refuses it.
//!
//! R7-F1 exists in the oracle family because a part or a pending part is found there by index and script only (the
//! `fresh` scan). In the book family every output an entry checks is tied to the input that checks it, in one of three
//! ways (DESIGN 4.4, 4.5a-4.5g):
//!   (a) counted under the input's own covenant id (`OpCovOutputCount(selfId)` / `OpCovOutputIdx(selfId, k)`) with the
//!       single-input guard `OpCovInputCount(selfId) == 1`. An output carries one covenant id, so it can be counted by
//!       one input only: every continuation, part, remainder, P3 of a host, pending part, restore, cure and forfeit;
//!   (b) a tag (or, for a lock, the claim id in its state) equal to the input's own covenant id, with the same guard:
//!       the pair of `KpfOpen.fill`, the part of `KpfOpen.take`, the P3 a donor claims, the lock of a guarantee;
//!   (c) a positional slot: output `self`, and `M - 1 - self` with `M - 1 - self >= inputs`: every plain payout.
//! The tests below build each merge the oracle exploit uses, for each cell no earlier test covers: two inputs of
//! DIFFERENT ids whose entries compute the same script, and one output for both.
//! Cells already covered elsewhere: `kpf_transfer_tests::p3_two_orders_cannot_claim_one_part` (two takes),
//! `kpf_open_tests::a_pair_output_claimed_by_another_long_is_refused` (two fills),
//! `kpf_close_tests::p4_one_p3_cannot_be_claimed_twice` (two donors), `kpf_guarantee_tests::
//! one_lock_for_two_claims_is_refused_in_one_transaction_and_across_transactions` (two guarantees),
//! `guarantee_attack_tests::holds_two_locks_never_share_a_payout_or_a_burn` (two locks), and the same-id twins of
//! `kpf_pair_tests::twins_with_one_id_are_refused_on_every_path`, `kpf_transfer_tests::
//! p3_same_id_parts_cannot_be_spent_together` and `two_step_attack_tests::holds_pending_twin_and_remainder_are_never_co_spent`.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::Transaction;
use kaspa_consensus_core::Hash;
use kpf::evidence::*;
use kpf::paths::*;
use kpf::*;
use secp256k1::Keypair;

const N: i64 = 1_000_000_000; // 1,000 tokens, the whole fixture pair
const HALF: i64 = 400_000_000;
const X: i64 = 1_000_000_000; // 10 KAS, the fixture pair's entry
const LIQ_ASK: i64 = 940_000_000; // liquidates the fixture long (bl 9 KAS, mmr 5 %)
const REF_LIQ_LONG: i64 = 966_315_790; // the counter bid that just refutes it (g = 2 %)
const L_ID: Hash = Hash::from_bytes([0x41; 32]);
const L2_ID: Hash = Hash::from_bytes([0x43; 32]);

fn accept(d: &Draft) -> Transaction {
    let (tx, e) = d.build();
    assert_accepted(&tx, &e);
    let ins: u128 = e.iter().map(|u| u.amount as u128).sum();
    let outs: u128 = tx.outputs.iter().map(|o| o.value as u128).sum();
    assert!(outs <= ins, "no sompi created");
    tx
}

/// Input `idx` fails a require; every input of `ok` passes.
fn refused_only(d: &Draft, idx: usize, ok: &[usize]) {
    let (tx, e) = d.build();
    let r = run(&tx, &e).expect("bindings");
    for &o in ok {
        assert!(r[o].is_ok(), "input {o} should accept: {:?}", r[o]);
    }
    assert_input_refused(&tx, &e, idx);
}

/// The long of every fixture pair (an offline party).
fn victim() -> Keypair {
    keypair(46)
}

/// The builder of every merge.
fn attacker() -> Keypair {
    keypair(50)
}

fn fk() -> Keypair {
    keypair(14)
}

/// The fixture pair (1,000 tokens, bl 9, bs 12, entry 10 KAS), its long listed at 10 KAS.
fn listed() -> Pair {
    let mut p = pair(&victim(), &attacker());
    (p.long_ask, p.long_min, p.long_tip) = (X, 0, 0);
    p
}

/// Two pairs with one state and two covenant ids: what a market maker's resting order gets when it is filled twice at
/// one price (the setting of R7-F1's first route).
fn twins(p: &Pair, t1: u8, t2: u8) -> (Live, Live) {
    let a = Live { p: p.clone(), value: p.floor() as u64, id: Hash::from_bytes([t1; 32]), op: outpoint(t1), daa: 0 };
    let b = Live { id: Hash::from_bytes([t2; 32]), op: outpoint(t2), ..a.clone() };
    (a, b)
}

/// A direct taker's whole or partial hand-over of the long at 10 KAS to the attacker's key, bound 8.8 KAS, tag 0.
fn long_spec(n: i64) -> TransferSpec {
    TransferSpec { side: LONG, n, x: X, nb: 880_000_000, ek: xonly(&attacker()), et: ZERO, k: 0 }
}

/// KpfOpen.fill arguments of a long order for the pair `p` at output `pair_out`: (pairOut, n, x, bl, bs, counterKey,
/// counterTag, cPre, cSuf), x the pair's entry, the counterparty the pair's short.
fn long_fill_args(p: &Pair, pair_out: i64, counter_tag: Hash) -> Vec<Arg> {
    let (pre, suf) = parts(pair_base());
    vec![iv(pair_out), iv(p.size), iv(p.entry), iv(p.bl), iv(p.bs), bv(&p.short_key), bv(&counter_tag.as_bytes()), bv(&pre), bv(&suf)]
}

fn long_entrant(id: Hash, op: u8) -> LiveOpen {
    let o = long_order(&keypair(48));
    LiveOpen { value: o.escrow() as u64, o, id, op: outpoint(op) }
}

// ---------------------------------------------------------------- KpfPair.transfer (P3): the part and the remainder

/// R7-F1's first route on the book family. Two pairs with one state and two ids; both longs are handed over, whole, to
/// one key at one bound, and the builder writes ONE part (in 0 pair A, 1 pair B, 2 the taker; out 0 A's long, 1 B's
/// long, 2 the part, 3 change). Both entries compute the same part script, but each counts the part under its own id:
/// the part carries A's id, so B finds no output of its id and is refused; bound to B, A is refused. Rule (a).
#[test]
fn holds_two_pairs_never_hand_over_into_one_part() {
    let (a, b) = twins(&listed(), 0x61, 0x62);
    let t = long_spec(N);
    assert_eq!(p2sh(&transfer_out(&a, &t).part.art()), p2sh(&transfer_out(&b, &t).part.art()), "one part script for both");
    let d = transfer_draft(&a, &t, &Entrant::Direct(attacker()), &attacker(), 1);
    accept(&d);
    let ins = vec![d.ins[0].clone(), pair_input(&b, "transfer", d.args(0)), d.ins[1].clone()];
    let outs = vec![d.outs[0].clone(), d.outs[0].clone(), d.outs[2].clone(), d.outs[1].clone()];
    let mut m = Draft::new(ins, outs, Some(3));
    m.lock = d.lock;
    refused_only(&m, 1, &[0]);
    let mut m2 = m.clone();
    m2.outs[2].covenant = bind(1, b.id);
    refused_only(&m2, 0, &[1]);
}

/// The partial variant (`exploit_partial_hand_overs_of_two_pairs_share_one_part` in the oracle family): 400 tokens of
/// each long into one part, each remainder under its own id (out 2 the part, 3 A's remainder, 4 B's remainder). The
/// split needs exactly two outputs of the pair's id; B has one (its remainder) and is refused; with the part bound to
/// B, A has one and is refused.
#[test]
fn holds_two_partial_hand_overs_never_share_one_part() {
    let (a, b) = twins(&listed(), 0x63, 0x64);
    let t = long_spec(HALF);
    let d = transfer_draft(&a, &t, &Entrant::Direct(attacker()), &attacker(), 2);
    accept(&d);
    let (rb, rbv) = transfer_out(&b, &t).rest.expect("a remainder");
    let ins = vec![d.ins[0].clone(), pair_input(&b, "transfer", d.args(0)), d.ins[1].clone()];
    let outs = vec![
        d.outs[0].clone(),
        d.outs[0].clone(),
        d.outs[2].clone(),
        d.outs[3].clone(),
        out(rbv as u64, p2sh(&rb.art()), bind(1, b.id)),
        d.outs[1].clone(),
    ];
    let mut m = Draft::new(ins, outs, Some(5));
    m.lock = d.lock;
    refused_only(&m, 1, &[0]);
    let mut m2 = m.clone();
    m2.outs[2].covenant = bind(1, b.id);
    refused_only(&m2, 0, &[1]);
}

// ---------------------------------------------------------------- KpfPair.closeHost (P4): P3

/// REVIEW-7's unproven `closeHost` merge, on the book family: two identical hosts (two ids) and one donor writing one
/// P3 (in 0 host 1, 1 host 2, 2 the donor, 3 F; out 0 and 1 the hosts' longs, 2 the donor's short, 3 P3, 4 change).
/// The donor's claim is fine (P3's tag is its id), but each host counts P3 under its own id: bound to host 1, host 2
/// is refused; bound to host 2, host 1 is refused. Rule (a).
#[test]
fn holds_two_hosts_never_share_one_p3() {
    let mut hp = pair(&keypair(11), &keypair(12));
    (hp.long_ask, hp.long_min, hp.long_tip) = (1_000_000_000, 100_000_000, 1_000_000);
    (hp.short_bid, hp.short_min, hp.short_tip) = (800_000_000, 0, 0);
    let mut dp = pair(&keypair(21), &keypair(22));
    (dp.bl, dp.bs, dp.entry) = (850_000_000, 1_150_000_000, X);
    (dp.long_ask, dp.long_min, dp.long_tip) = (1_300_000_000, 200_000_000, 0);
    (dp.short_bid, dp.short_min, dp.short_tip) = (1_030_000_000, 100_000_000, 2_000_000);
    let (h1, h2) = twins(&hp, 0x65, 0x66);
    let d = Live { p: dp.clone(), value: dp.floor() as u64, id: Hash::from_bytes([0x67; 32]), op: outpoint(0x67), daa: 0 };
    let cs = CloseSpec { n: N, x: 1_020_000_000, e3: X };
    let base = close_draft(&h1, &d, &cs, &fk(), 3);
    accept(&base);
    let ins = vec![base.ins[0].clone(), pair_input(&h2, "closeHost", base.args(0)), pair_input(&d, "closeDonor", donor_args(&h1, &cs, 3)), base.ins[2].clone()];
    let outs = vec![base.outs[0].clone(), base.outs[0].clone(), base.outs[1].clone(), base.outs[2].clone(), base.outs[3].clone()];
    let mut m = Draft::new(ins, outs, Some(4));
    m.lock = base.lock;
    refused_only(&m, 1, &[0, 2]);
    let mut m2 = m.clone();
    m2.outs[3].covenant = bind(1, h2.id);
    refused_only(&m2, 0, &[1, 2]);
}

/// Cross-kind: a host's P3 that is at the same time a resting long order's take part. Everything a take rebuilds can
/// equal P3 (the donor's long is the order's owner at the order's bound, unlisted, at an entry the order accepts), so
/// only the tag decides, and a tag names one claimer: (a) P3 tagged by the donor: host and donor pass, the order is
/// refused; (b) P3 tagged by the order (the host's free `tl3`): host and order pass, the donor is refused. In 0 the
/// host, 1 the donor, 2 the order, 3 F; out 0 the host's long, 1 the donor's short, 2 the order's rest, 3 P3, 4 change.
#[test]
fn holds_a_host_p3_is_claimed_by_the_donor_or_by_a_take_never_both() {
    let lo = long_entrant(L_ID, 0x41);
    let mut hp = pair(&keypair(11), &keypair(12));
    (hp.long_ask, hp.long_min, hp.long_tip) = (X, 0, 0);
    let mut dp = pair(&keypair(48), &keypair(22));
    (dp.short_bid, dp.short_min, dp.short_tip) = (1_020_000_000, 0, 0);
    assert_eq!(dp.long_key, lo.o.owner);
    assert_eq!(dp.bl, X - lo.o.my_rate, "the donor's long sits at the order's own bound");
    let h = Live { p: hp.clone(), value: hp.floor() as u64, id: Hash::from_bytes([0x68; 32]), op: outpoint(0x68), daa: 0 };
    let d = Live { p: dp.clone(), value: dp.floor() as u64, id: Hash::from_bytes([0x69; 32]), op: outpoint(0x69), daa: 0 };
    let cs = CloseSpec { n: N, x: 1_020_000_000, e3: X };
    let co = close_out(&h, &d, &cs);
    let mk = |p3: &Pair, tl3: Hash| {
        let mut ha = host_args(&d, &cs, 0);
        ha[9] = bv(&tl3.as_bytes());
        let (oo, _) = order_out(&lo, N, 2);
        let ins = vec![
            pair_input(&h, "closeHost", ha),
            pair_input(&d, "closeDonor", donor_args(&h, &cs, 3)),
            open_input(&lo, "take", take_args(p3, LONG, 3, N, X)),
            funder(&fk(), 4, F_FUNDS),
        ];
        let outs = vec![
            out(co.host_pay as u64, p2pk_x(&h.p.long_key), None),
            out(co.donor_pay as u64, p2pk_x(&d.p.short_key), None),
            oo,
            out(co.p3_value as u64, p2sh(&p3.art()), bind(0, h.id)),
            out(0, p2pk_spk(&fk()), None),
        ];
        let mut m = Draft::new(ins, outs, Some(4));
        m.lock = h.handover_from().max(d.handover_from());
        m
    };
    assert_eq!(co.p3.tag_l, d.id);
    refused_only(&mk(&co.p3, d.id), 2, &[0, 1]);
    let mut p3o = co.p3.clone();
    p3o.tag_l = lo.id;
    refused_only(&mk(&p3o, lo.id), 1, &[0, 2]);
}

// ---------------------------------------------------------------- KpfOpen.fill against a hand-over part

/// REVIEW-7's unproven cross-kind merge, on the book family: a long order's `fill` pointed at a pair's hand-over part
/// whose script is exactly the pair the fill rebuilds (the part's entrant is the order's owner at the order's bound,
/// tag = the order's id, the stayer tag 0, both sides unlisted, entry = the fill price). In 0 the pair's transfer, 1 the
/// order's fill, 2 F; out 0 the leaver, 1 the order's rest, 2 the part, 3 change. Under the pair's id the pair passes
/// and the fill is refused (its pair's id must be carried by no input); under a fresh genesis id the fill passes and
/// the pair is refused (no output of its id). Rules (a) against the fill's freshness check.
#[test]
fn holds_an_order_fill_never_accepts_a_pairs_hand_over_part() {
    let p = listed();
    let a = Live { p: p.clone(), value: p.floor() as u64, id: Hash::from_bytes([0x6a; 32]), op: outpoint(0x6a), daa: 0 };
    let lo = long_entrant(L_ID, 0x41);
    let t = TransferSpec { side: LONG, n: N, x: X, nb: X - lo.o.my_rate, ek: lo.o.owner, et: lo.id, k: 0 };
    let to = transfer_out(&a, &t);
    let mut fp = pair(&keypair(1), &keypair(2));
    fp.m = lo.o.m.clone();
    (fp.size, fp.bl, fp.bs, fp.entry, fp.long_key, fp.short_key, fp.tag_l, fp.tag_s) = (N, X - lo.o.my_rate, p.bs, X, lo.o.owner, p.short_key, lo.id, ZERO);
    assert_eq!(p2sh(&fp.art()), p2sh(&to.part.art()), "the fill rebuilds exactly the hand-over part");
    let pa = vec![iv(LONG), iv(N), iv(X), iv(t.nb), bv(&t.ek), bv(&t.et.as_bytes()), iv(0)];
    let (oo, _) = order_out(&lo, N, 1);
    let ins = vec![pair_input(&a, "transfer", pa), open_input(&lo, "fill", long_fill_args(&to.part, 2, ZERO)), funder(&fk(), 5, F_FUNDS)];
    let outs = vec![
        out(to.pay as u64, p2pk_x(&to.leaver), None),
        oo,
        out(to.part_value as u64, p2sh(&to.part.art()), bind(0, a.id)),
        out(0, p2pk_spk(&fk()), None),
    ];
    let mut m = Draft::new(ins, outs, Some(3));
    m.lock = a.handover_from();
    refused_only(&m, 1, &[0]);
    let mut m2 = m.clone();
    m2.outs[2].covenant = None;
    m2.genesis.push((2, 2, fop(5)));
    refused_only(&m2, 0, &[1]);
}

// ---------------------------------------------------------------- KpfOpen escrow continuations

/// Two long orders with one state and two ids each fill their own new pair, and the builder writes ONE escrow
/// continuation (identical script for both) and keeps the second order's rest (in 0 order 1, 1 order 2, 2 and 3 the
/// funders of the two pair geneses; out 0 the shared rest, 1 the attacker, 2 and 3 the pairs, 4 change). Each order
/// counts its rest under its own id: order 2 finds none and is refused. Rule (a).
#[test]
fn holds_two_orders_never_share_one_escrow_continuation() {
    let (lo1, lo2) = (long_entrant(L_ID, 0x41), long_entrant(L2_ID, 0x43));
    let pf = |id: Hash| {
        let mut p = pair(&keypair(48), &keypair(47));
        (p.tag_l, p.tag_s) = (id, ZERO);
        p
    };
    let (p1, p2) = (pf(lo1.id), pf(lo2.id));
    let (c1, _) = order_out(&lo1, N, 0);
    let (c2, _) = order_out(&lo2, N, 1);
    assert_eq!(c1.script_public_key, c2.script_public_key, "one continuation script for both");
    let ins = vec![
        open_input(&lo1, "fill", long_fill_args(&p1, 2, ZERO)),
        open_input(&lo2, "fill", long_fill_args(&p2, 3, ZERO)),
        funder(&fk(), 6, F_FUNDS),
        funder(&fk(), 7, F_FUNDS),
    ];
    let mk = |second: kaspa_consensus_core::tx::TransactionOutput| {
        let outs = vec![
            c1.clone(),
            second,
            out(p1.floor() as u64, p2sh(&p1.art()), None),
            out(p2.floor() as u64, p2sh(&p2.art()), None),
            out(0, p2pk_spk(&fk()), None),
        ];
        let mut m = Draft::new(ins.clone(), outs, Some(4));
        m.genesis = vec![(2, 2, fop(6)), (3, 3, fop(7))];
        m
    };
    accept(&mk(c2.clone()));
    refused_only(&mk(out(c2.value, p2pk_spk(&attacker()), None)), 1, &[0]);
}

// ---------------------------------------------------------------- KpfPair amend and idle settle

/// Two pairs with one state and two ids, both amended by their (common) long owner, ONE continuation (in 0 A, 1 B, 2 F;
/// out 0 the continuation, 1 change): B counts none of its id and is refused. Rule (a), as for add margin and every
/// remainder of mutual close and close donor.
#[test]
fn holds_two_pairs_never_amend_into_one_continuation() {
    let (a, b) = twins(&pair(&victim(), &attacker()), 0x6b, 0x6c);
    let d = amend_draft(&a, &victim(), LONG, (X, 0, 0), &fk(), 8);
    accept(&d);
    let ins = vec![d.ins[0].clone(), pair_input(&b, "amend", d.args(0)), d.ins[1].clone()];
    let m = Draft::new(ins, d.outs.clone(), Some(1));
    refused_only(&m, 1, &[0]);
}

/// Positional slots (rule (c)): two idle-settled pairs with one state cannot share one pair of payouts. A pays its long
/// at output 0 and its short at M - 1 = 3; B (input 1) must pay its long at output 1 and its short at M - 2, so with A's
/// payouts only, B is refused.
#[test]
fn holds_two_idle_settles_never_share_payouts() {
    let (a, b) = twins(&pair(&victim(), &attacker()), 0x6d, 0x6e);
    let (lp, sp) = idle_out(&a);
    assert!(lp > 0 && sp > 0);
    let ins = vec![pair_input(&a, "idleSettle", vec![]), pair_input(&b, "idleSettle", vec![]), funder(&fk(), 9, F_FUNDS)];
    let outs = vec![
        out(lp as u64, p2pk_x(&a.p.long_key), None),
        out(0, p2pk_spk(&fk()), None),
        out(1_000_000, p2pk_spk(&fk()), None),
        out(sp as u64, p2pk_x(&a.p.short_key), None),
    ];
    let mut m = Draft::new(ins, outs, Some(1));
    m.seqs = vec![IDLE_SEQ, IDLE_SEQ, 0];
    refused_only(&m, 1, &[0]);
}

// ---------------------------------------------------------------- KpfPair.propose into KpfPending

fn lv(tag: u8) -> Live {
    live(&pair(&victim(), &attacker()), tag).at(1_000)
}

fn liq_long(l: &Live, n: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side: LONG } }
}

/// R7-F1's underwriter route on the book family: two pairs with one state (two ids) propose the same liquidation in
/// one block, and the builder writes ONE pending part (the pending script copies the pair state without its id, so it
/// is the same for both) and keeps the other (out 1 the attacker's). The pending part is counted under the proposing
/// pair's own id (n = s: one output of the id; n < s: two, the pending part and the remainder): pair B finds none
/// (or only its remainder) and is refused. Rule (a).
#[test]
fn holds_two_pairs_never_propose_into_one_pending_part() {
    for (n, tags) in [(N, (0x71, 0x72)), (HALF, (0x73, 0x74))] {
        let (a, b) = (lv(tags.0), lv(tags.1));
        let d = ev_draft(&[liq_long(&a, n), liq_long(&b, n)], &Evidence::ask(LIQ_ASK, 2 * n), &fk(), 10);
        accept(&d);
        assert_eq!(d.outs[0].script_public_key, d.outs[1].script_public_key, "one pending script for both");
        let mut m = d.clone();
        m.outs[1] = out(m.outs[1].value, p2pk_spk(&attacker()), None);
        refused_only(&m, 1, &[0]);
    }
}

// ---------------------------------------------------------------- KpfPending: restore, cure, forfeit

fn twin_pendings(t1: u8, t2: u8) -> (LivePend, LivePend) {
    let (a, b) = (lv(t1), lv(t2));
    let (wa, wb) = (pend(&liq_long(&a, N), LIQ_ASK), pend(&liq_long(&b, N), LIQ_ASK));
    assert_eq!(wa.w, wb.w, "one pending state, two ids");
    assert_ne!(wa.id, wb.id);
    (wa, wb)
}

/// Two pending parts with one state (two ids) refuted by one counter print, ONE restored pair (out 1 the attacker's):
/// each challenge counts its restore under its own id, so the second is refused. Rule (a).
#[test]
fn holds_two_challenges_never_restore_into_one_pair() {
    let (wa, wb) = twin_pendings(0x31, 0x32);
    let d = ch_draft(&[wa.clone(), wb.clone()], &Evidence::bid(REF_LIQ_LONG, 2 * N), &fk(), 11);
    accept(&d);
    assert_eq!(d.outs[0].script_public_key, d.outs[1].script_public_key, "one restore script for both");
    let mut m = d.clone();
    m.outs[1] = out(m.outs[1].value, p2pk_spk(&attacker()), None);
    refused_only(&m, 1, &[0]);
}

/// Two cures of pending parts with one state (the loser's owner signs both), ONE cured pair (in 0 and 1 the parts, 2 F;
/// out 0 and 1 the proposer's refunds at the parts' own slots, 2 the cured pair, 3 change): the second is refused.
#[test]
fn holds_two_cures_never_share_one_cured_pair() {
    let (wa, wb) = twin_pendings(0x33, 0x34);
    accept(&cure_draft(&wa, &victim(), &fk(), 12));
    let c = wa.w.cure_pair().expect("cure state matches its hash");
    let ins = vec![pending_input(&wa, "cure", vec![Arg::Sig(victim())]), pending_input(&wb, "cure", vec![Arg::Sig(victim())]), funder(&fk(), 13, F_FUNDS)];
    let refund = out(wa.w.refund as u64, p2pk_x(&wa.w.proposer_key), None);
    let outs = vec![refund.clone(), refund, out(wa.w.cure_value(wa.value) as u64, p2sh(&c.art()), bind(0, wa.id)), out(0, p2pk_spk(&fk()), None)];
    let m = Draft::new(ins, outs, Some(3));
    refused_only(&m, 1, &[0]);
}

/// Two finalized liquidations with one state, ONE forfeited pair (out 0 and 1 the proposer's bond and fee at the parts'
/// own slots, 2 the forfeited pair, 3 change): the second is refused.
#[test]
fn holds_two_finalizes_never_share_one_forfeited_pair() {
    let (wa, wb) = twin_pendings(0x35, 0x36);
    accept(&finalize_draft(&wa, &fk(), 14));
    let fpp = wa.w.forfeit_pair().expect("forfeit state matches its hash");
    let (fv, pay) = wa.w.finalize_forfeit(wa.value);
    let ins = vec![pending_input(&wa, "finalize", vec![]), pending_input(&wb, "finalize", vec![]), funder(&fk(), 15, F_FUNDS)];
    let paid = out(pay as u64, p2pk_x(&wa.w.proposer_key), None);
    let outs = vec![paid.clone(), paid, out(fv as u64, p2sh(&fpp.art()), bind(0, wa.id)), out(0, p2pk_spk(&fk()), None)];
    let mut m = Draft::new(ins, outs, Some(3));
    m.seqs = vec![window(), window(), 0];
    refused_only(&m, 1, &[0]);
}
