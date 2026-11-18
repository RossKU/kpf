//! Attacks on the two-step evidence mechanism itself (DESIGN 4.5d), in the manner of the three reviews: each attack is a
//! transaction the attacker can build; a `holds_*` test shows it refused or harmless, an `exploit_*` test shows what the
//! engine accepts that the design does not want, with the residual stated. Every input runs its real script, the KOB
//! evidence orders and the token program included.
//!
//! Covered: the pending template's covenant id (twin pieces of one split, look-alike pendings from a genesis, tags);
//! a part locked forever (every branch has an exit that needs nobody's cooperation); the proposer or the challenger
//! being one of the pair's parties; value moved through the bond, the fee or the restored and cured outputs; return
//! states forged at proposal time; the one lock time serving several lower bounds; and the interaction with the
//! per-pair clock and the hand-over rate limit.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::Transaction;
use kaspa_consensus_core::Hash;
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

const N: i64 = 1_000_000_000;
const HALF: i64 = 500_000_000;
const R: u64 = 600;

fn lv(tag: u8, daa: u64) -> Live {
    live(&pair(&lk(), &sk()), tag).at(daa)
}

fn liq(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side } }
}

fn close(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Close { side, signer: if side == LONG { lk() } else { sk() } } }
}

/// Evidence whose maker and counterparty are both `who`.
fn own(mut e: Evidence, who: &Keypair) -> Evidence {
    e.maker = *who;
    e.ask.maker = xonly(who);
    e.bid.maker = xonly(who);
    e.counter = *who;
    e
}

// ================================================================ covenant id, twins, tags

/// The pending part shares the pair's covenant id with the remainder of its split (one id, two templates, X12). The
/// guard OpCovInputCount(selfId) == 1 on every pending entry and every pair entry forbids co-spending them in any
/// combination: a challenge of the pending part with a proposal on the remainder, a cure with an amend.
#[test]
fn holds_pending_twin_and_remainder_are_never_co_spent() {
    let l = lv(0x31, 1_000);
    let d = ev_draft(&[liq(&l, HALF, LONG)], &Evidence::ask(940_000_000, HALF), &fk(), 1);
    let tx = accept(&d);
    let w = live_pendings(&tx, &d).remove(0).at(NOW - R - 10);
    let rest = live_pairs(&tx, &d).remove(0).at(NOW - R - 10);
    assert_eq!(w.id, rest.id, "one covenant id");
    let m = vec![Member::W(PendLeg { w: w.clone(), q: 0 }), Member::P(close(&rest, HALF, LONG))];
    let (t, e) = blk_draft(&m, &Evidence::bid(1_050_000_000, 2 * HALF), &fk(), 2).build();
    let r = run(&t, &e).expect("bindings");
    assert!(r[0].is_err() && r[1].is_err());
    // Each alone is fine.
    accept(&ch_draft(&[w], &Evidence::bid(966_315_790, HALF), &fk(), 3));
    accept(&ev_draft(&[close(&rest, HALF, LONG)], &Evidence::bid(1_050_000_000, HALF), &fk(), 4));
}

/// A look-alike KpfPending made by a genesis (any id, any state, any hashes): its entries can only pay out its own
/// value. Its restore hash may name a pair with a tag of a live KpfOpen; that order then accepts the output only if it
/// is exactly the order's own terms, fully funded (out of the look-alike's value): the look-alike only gives. A genuine
/// pending never carries a tag on any state it may become (tags 0 everywhere).
#[test]
fn holds_look_alike_pending_only_gives() {
    let l = lv(0x32, 1_000);
    let g = liq(&l, N, LONG);
    let real_w = pend(&g, 940_000_000);
    let mut w = real_w.clone();
    // The attacker's state: a restore pair with the attacker as the long, funded by the attacker's own look-alike.
    let mut forged = w.w.restore_pair().unwrap();
    forged.long_key = xonly(&keypair(66));
    w.w.restore_hash = kpf_protocol::pending::script_hash(&forged);
    w.id = Hash::from_bytes([0x99; 32]);
    let mut d = ch_draft(&[real_w], &Evidence::bid(966_315_790, N), &fk(), 1);
    let args = d.args(0);
    d.ins[0] = pending_input(&w, "challenge", args);
    d.outs[0].covenant = bind(0, w.id);
    d.set_pair(0, &forged);
    let tx = accept(&d);
    // The output is a pair funded by the look-alike itself; no other input lost anything.
    assert_eq!(tx.outputs[0].value as i64, w.value as i64 - w.w.bond);
    // A genuine pending: every state it may become carries tag 0 on both sides.
    let real = pend(&g, 940_000_000);
    for s in [real.w.restore_pair(), real.w.cure_pair(), real.w.forfeit_pair()].into_iter().flatten() {
        assert_eq!((s.tag_l, s.tag_s), (ZERO, ZERO));
    }
}

// ================================================================ no part is locked forever

/// Every branch has an exit that needs nobody's cooperation: after W anyone finalizes, with no key and no evidence,
/// even when the proposer's key is not a valid x-only key (its payout is then unspendable, its own loss) and when the
/// remainder of the split is gone. A close finalizes too. Before W the parties can still challenge or cure.
#[test]
fn holds_every_pending_part_has_an_exit() {
    let l = lv(0x33, 1_000);
    for pk in [[0xffu8; 32], [0u8; 32]] {
        let g = PairLeg { l: l.clone(), n: HALF, q: 0, act: Act::LiqBy { side: LONG, proposer: pk } };
        accept(&ev_draft(&[g.clone()], &Evidence::ask(940_000_000, HALF), &fk(), 1));
        let w = pend(&g, 940_000_000);
        // An executor with no stake finalizes.
        accept(&finalize_draft(&w, &keypair(90), 2));
    }
    let w = pend(&close(&l, N, SHORT), 1_100_000_000);
    accept(&finalize_draft(&w, &keypair(91), 3));
}

// ================================================================ the proposer or the challenger is a party

/// The loser proposes its own liquidation (it is its own keeper): it funds the bond, and at finalize it receives the
/// bond back and the fee, which comes out of its own forfeited side; the winner keeps the part less that fee, as from any
/// keeper. The winner proposing gets the fee from the loser's side too: in both cases the winner holds at least its
/// payoff at the trigger price.
#[test]
fn holds_proposer_is_a_party() {
    let l = lv(0x34, 1_000);
    for proposer in [xonly(&lk()), xonly(&sk())] {
        let g = PairLeg { l: l.clone(), n: N, q: 0, act: Act::LiqBy { side: LONG, proposer } };
        let w = pend(&g, 940_000_000);
        let fd = finalize_draft(&w, &fk(), 1);
        accept(&fd);
        assert_eq!(fd.outs[0].value as i64, w.w.bond + w.w.fee);
        assert!(fd.outs[1].value as i64 >= winner_at_trigger(&l.p, N, LONG));
    }
}

/// The proposer challenges itself to cancel its own claim: it pays the counter fill and the fees and gets its own bond
/// back; the part is restored (its clock S kept). Two colluders passing the bond move value only between themselves.
#[test]
fn holds_self_challenge_only_returns_the_own_bond() {
    let l = lv(0x35, 1_000);
    let g = liq(&l, N, LONG);
    let w = pend(&g, 940_000_000);
    let d = ch_draft(&[w.clone()], &own(Evidence::bid(966_315_790, N), &proposer_kp()), &proposer_kp(), 1);
    accept(&d);
    assert_eq!(d.outs[0].value as i64, w.value as i64 - w.w.bond);
    assert_eq!(d.pairs[0].1, w.w.restore_pair().unwrap());
}

/// The false challenge by the deserving loser on a thin book (DESIGN 4.5d section 5b): the long is truly liquidatable
/// (the honest keeper's ask at 9.30 rests on the pair's clock), but the long rests its own bid at 9.6632 (above its
/// trigger by g), fills it itself and refutes the honest claim, collecting the keeper's bond. The engine accepts: on a
/// book with nobody to sell into that bid, the self-fill costs the loser only fees. The honest keeper loses its bond;
/// the restored part keeps the clock S, so the keeper may re-propose at once on the same ask, and the loser may repeat
/// the false challenge each round (each costs it a fill of k_c * n at its bid and fees, and pays it the keeper's bond).
/// On a book with arbitrageurs the bid is a gift of (9.6632 - market) per token for k_c * n and the attack costs more
/// than the bond. This is an accepted economic limit of the design (it "fails" on testnet-10, DESIGN 4.5d residual
/// risk 1), not a defect to fix: it stays recorded as an exploit.
#[test]
fn exploit_false_challenge_by_the_loser_on_a_thin_book() {
    let l = lv(0x36, 1_000);
    let g = liq(&l, N, LONG);
    accept(&ev_draft(&[g.clone()], &Evidence::ask(930_000_000, N), &fk(), 1));
    let w = pend(&g, 930_000_000);
    let d = ch_draft(&[w.clone()], &own(Evidence::bid(966_315_790, N), &lk()), &lk(), 2);
    let tx = accept(&d);
    // The loser's change holds the keeper's bond; the loser's side is back on the pair's clock S.
    let restored = live_pairs(&tx, &d).remove(0);
    assert_eq!((restored.p.long_key, restored.p.bl, restored.p.since), (l.p.long_key, l.p.bl, l.clock()));
    assert_eq!(d.outs[0].value as i64, w.value as i64 - w.w.bond);
    // The same residual for a close: the long closes honestly at the market bid of 10; on a thin book the short rests its
    // own ask at 9.80 (0.98 of the bid), fills it itself and refutes the close, taking the long's bond. The long keeps
    // its exits that need no evidence (transfer, cooperative close, idle settle).
    let wc = pend(&close(&l, N, LONG), 1_000_000_000);
    let d = ch_draft(&[wc.clone()], &own(Evidence::ask(980_000_000, N), &sk()), &sk(), 3);
    accept(&d);
    assert_eq!(d.outs[0].value as i64, wc.value as i64 - wc.w.bond);
}

// ================================================================ value through the bond, the fee and the outputs

/// The bond, the fee and the refund cannot be diverted: a challenge whose restored pair holds one sompi less than the
/// value less the bond, a finalize whose proposer takes one sompi of the forfeited part, a cure whose refund goes to
/// another key, and a close's finalize that pays the closer's refund to the other party's slot are all refused.
#[test]
fn holds_no_value_moves_through_the_bond_or_the_fee() {
    let l = lv(0x37, 1_000);
    let g = liq(&l, HALF, LONG);
    let w = pend(&g, 940_000_000);
    let mut d = ch_draft(&[w.clone()], &Evidence::bid(966_315_790, HALF), &fk(), 1);
    d.outs[0].value -= 1;
    refused(&d, 0);
    let mut f = finalize_draft(&w, &fk(), 2);
    f.outs[0].value += 1;
    f.outs[1].value -= 1;
    refused(&f, 0);
    let mut c = cure_draft(&w, &lk(), &fk(), 3);
    c.outs[0].script_public_key = p2pk_x(&xonly(&keypair(66)));
    refused(&c, 0);
    let wc = pend(&close(&l, HALF, LONG), 1_050_000_000);
    let mut f = finalize_draft(&wc, &fk(), 4);
    let m = f.outs.len();
    let refund = wc.w.refund as u64;
    f.outs[0].value -= refund;
    f.outs[m - 1].value += refund;
    refused(&f, 0);
}

/// The return states cannot be forged at proposal time: the pair computes them from its own script. A proposer that
/// writes a restore state with its own key as the long, a cure state at a wider bound, or a forfeit state paying the
/// loser, is refused by the pair even with the matching hash in the pending state.
#[test]
fn holds_return_states_are_fixed_by_the_pair() {
    let l = lv(0x38, 1_000);
    let g = liq(&l, N, LONG);
    let d = ev_draft(&[g.clone()], &Evidence::ask(940_000_000, N), &fk(), 1);
    accept(&d);
    let w0 = d.pendings[0].1.clone();
    let lp = l.clocked();
    let mut r = w0.restore_pair().unwrap();
    r.long_key = xonly(&keypair(66));
    let mut c = w0.cure_pair().unwrap();
    c.bl -= 1;
    let mut f = w0.forfeit_pair().unwrap();
    f.long_key = lp.long_key;
    for (k, s) in [r, c, f].into_iter().enumerate() {
        let mut w = w0.clone();
        let h = kpf_protocol::pending::script_hash(&s);
        match k {
            0 => w.restore_hash = h,
            1 => w.cure_hash = h,
            _ => w.forfeit_hash = h,
        }
        let mut x = d.clone();
        x.outs[0].script_public_key = p2sh(&w.art());
        refused(&x, 0);
    }
}

// ================================================================ the clock and the hand-over limit

/// A live pair with both sides listed (long ask 10, short bid 11; no minimum part, no tip): a side may be transferred.
fn listed_lv(tag: u8, daa: u64) -> Live {
    let mut p = pair(&lk(), &sk());
    (p.long_ask, p.long_min, p.long_tip) = (1_000_000_000, 0, 0);
    (p.short_bid, p.short_min, p.short_tip) = (1_100_000_000, 0, 0);
    live(&p, tag).at(daa)
}

/// An ask of `e` that conforming matchers stop filling at DAA `x` (good-till-DAA).
fn expiring(mut e: Evidence, x: i64) -> Evidence {
    e.ask.expiry_daa = x;
    e
}

/// (a) A self-refuted claim does not delay a liquidation (DESIGN 4.5d: a restore keeps the pair's clock S). A truly
/// liquidatable long (an honest ask at 9.40 rests on the pair's clock) proposes a close of its own at its own bid of
/// 10.5 (rested R, self-filled) and refutes it itself with its own ask at 10.29 that rested R after the proposal; it
/// pays two fills to itself and fees, and gets its bond back. The engine accepts every step. The restored part carries
/// since = S, so the keeper's ask, which had already rested on S, is accepted in the very next proposal at the
/// restore's own DAA: the cycle gains the loser no rest time. What remains is the race of B1 (the threat model): while
/// the fake close is pending no liquidation can be proposed, for about R; and the keeper's own ask at 9.40 refutes the
/// fake close from P + R (it contradicts a close at 10.5 by more than g), taking the loser's bond instead.
#[test]
fn holds_self_refuted_claim_does_not_delay_the_liquidation() {
    let s = 100_000u64;
    let l = lv(0x39, s);
    let keeper_ask = Evidence::ask(940_000_000, N).placed(s + 10);
    // The keeper could liquidate at s + 10 + R.
    let mut k = ev_draft(&[liq(&l, N, LONG)], &keeper_ask, &fk(), 1);
    k.lock = s + 10 + R;
    accept(&k);
    // The long's own close at its own bid of 10.5, proposed first (it races the keeper, B1).
    let fake_bid = own(Evidence::bid(1_050_000_000, N), &lk()).placed(s + 10);
    let mut p = ev_draft(&[close(&l, N, LONG)], &fake_bid, &lk(), 2);
    p.lock = s + 10 + R;
    accept(&p);
    let p_daa = s + 10 + R;
    let w = pend_of(&close(&l, N, LONG), 1_050_000_000, p_daa);
    assert_eq!(w.w.since, s as i64, "the pending part records S");
    // The keeper's own ask refutes the fake close once it has rested R on P (it rested before P, so it counts from P).
    let mut kc = ch_draft(&[w.clone()], &keeper_ask, &fk(), 5);
    kc.lock = p_daa + R - 1;
    refused(&kc, 0);
    kc.lock = p_daa + R;
    accept(&kc);
    // Or the long refutes it first with its own ask at 10.29 rested R after P; it is its own challenger.
    let own_ask = own(Evidence::ask(1_029_000_000, N), &lk()).placed(p_daa);
    let mut c = ch_draft(&[w], &own_ask, &lk(), 3);
    c.lock = p_daa + R;
    let tx = accept(&c);
    let t = p_daa + R;
    let restored = live_pairs(&tx, &c).remove(0).at(t);
    assert_eq!((restored.p.since, restored.clock()), (s as i64, s as i64), "the restore keeps S, written explicitly");
    // The keeper's live ask is accepted in the very next proposal, at the restore's own DAA: no rest restarts.
    let mut k2 = ev_draft(&[liq(&restored, N, LONG)], &keeper_ask, &fk(), 4);
    k2.lock = t;
    accept(&k2);
    // A second fake close in the same cycle gains nothing either: its restore keeps S again.
    let w2 = pend_of(&close(&restored, N, LONG), 1_050_000_000, t);
    assert_eq!(w2.w.restore_pair().unwrap().since, s as i64);
}

/// (b) A proposer with stale evidence (the market is at 10 KAS; an ask at 9.40 still rests on the pair's clock S)
/// re-proposes the same liquidation after each refutation: the restore keeps S, so it may do so at once. Every round
/// it funds a bond B into the pending part, and every refutation (a fresh bid at 9.80 rested R on P) moves that bond to
/// the challenger. After n rounds the victim's pair holds exactly what it held before (n = s: the pending part holds
/// v + B and the restored pair v), its state and its clock are unchanged, the proposer has paid n * B and the
/// challenger has received n * B (no output pays the proposer).
#[test]
fn holds_a_stale_proposer_pays_a_bond_every_round() {
    let s = 100_000u64;
    let l0 = lv(0x3e, s);
    let stale = Evidence::ask(940_000_000, N).placed(s + 10);
    let proposer = xonly(&proposer_kp());
    let (mut cur, mut t) = (l0.clone(), s + 10 + R);
    let (mut paid, mut received) = (0i64, 0i64);
    for round in 0..3u32 {
        let mut p = ev_draft(&[liq(&cur, N, LONG)], &stale, &fk(), 10 + round);
        p.lock = t;
        let ptx = accept(&p);
        let w = live_pendings(&ptx, &p).remove(0).at(t);
        assert_eq!(w.value as i64 - cur.value as i64, w.w.bond, "round {round}: the proposer funds B");
        paid += w.w.bond;
        let counter = Evidence::bid(980_000_000, N).placed(t);
        let mut c = ch_draft(&[w.clone()], &counter, &fk(), 20 + round);
        c.lock = t + R;
        let (ctx, e) = c.build();
        assert_accepted(&ctx, &e);
        let ch = c.change.expect("the challenger's change");
        assert!(ctx.outputs.iter().all(|o| o.script_public_key != p2pk_x(&proposer)), "round {round}: nothing to the proposer");
        // The challenger's change gains the bond over what the counter fill alone leaves it.
        let leg_in: i64 = e[1..e.len() - 1].iter().map(|u| u.amount as i64).sum();
        let leg_out: i64 = ctx.outputs.iter().enumerate().filter(|(k, _)| *k != 0 && *k != ch).map(|(_, o)| o.value as i64).sum();
        let gain = ctx.outputs[ch].value as i64 - (F_FUNDS - FEE) as i64;
        assert_eq!(gain, w.w.bond + leg_in - leg_out, "round {round}: the bond is the challenger's");
        received += w.w.bond;
        let next = live_pairs(&ctx, &c).remove(0).at(t + R);
        assert_eq!(next.value, l0.value, "round {round}: the victim's pair holds exactly what it held");
        assert_eq!(next.p, l0.clocked().cleared(), "round {round}: state and clock unchanged (tags cleared, as on every continuation)");
        cur = next;
        t += R;
    }
    assert_eq!((paid, received), (3 * bond_of(&l0.clocked(), N), paid));
}

/// (c) The hand-over rate limit S + 4R cannot be bypassed by a propose-and-restore cycle: a part proposed and
/// restored before S + 4R keeps S, so its transfer is refused at S + 4R - 1 and accepted at S + 4R, exactly as for
/// the pair that was never proposed against. (No cycle restarts the clock, so none gives a side a fresh clock to hide
/// behind; only a hand-over does, and it waits for S + 4R.)
#[test]
fn holds_propose_and_restore_does_not_bypass_the_hand_over_limit() {
    let s = 100_000u64;
    let l = listed_lv(0x3f, s);
    let p_daa = s + 10 + R;
    let w = pend_of(&liq(&l, N, LONG), 940_000_000, p_daa);
    let mut c = ch_draft(&[w], &Evidence::bid(980_000_000, N).placed(p_daa), &fk(), 1);
    c.lock = p_daa + R;
    let tx = accept(&c);
    let restored = live_pairs(&tx, &c).remove(0).at(p_daa + R);
    assert!(p_daa + R < s + 4 * R);
    let t = TransferSpec { side: LONG, n: N, x: 1_000_000_000, nb: 900_000_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    let mut d = transfer_draft(&restored, &t, &Entrant::Direct(keypair(16)), &fk(), 2);
    assert_eq!(d.lock, s + 4 * R, "the limit is that of the pair's clock S");
    accept(&d);
    d.lock = s + 4 * R - 1;
    refused(&d, 0);
    // The same for the pair before any proposal.
    let mut d0 = transfer_draft(&l, &t, &Entrant::Direct(keypair(16)), &fk(), 3);
    assert_eq!(d0.lock, s + 4 * R);
    d0.lock -= 1;
    refused(&d0, 0);
}

/// (d) Neither a restore nor a cure moves a clock, backwards or forwards. On a pair whose since (1,500) differs from its
/// UTXO's DAA (7,000), the pending part records S = 1,500, and both its restore and its cure state carry since = S;
/// a challenge or a cure whose pair output carries since 0 (a restart at its own UTXO), S - 1, S + 1 or the old
/// UTXO's DAA is refused, for a liquidation and for a close.
#[test]
fn holds_restore_and_cure_never_move_the_clock() {
    let mut p = pair(&lk(), &sk());
    p.since = 1_500;
    let l = live(&p, 0x40).at(7_000);
    assert_eq!(l.clock(), 1_500);
    let bad = [0i64, 1_499, 1_501, 7_000];
    for (g, q, counter) in [
        (liq(&l, N, LONG), 940_000_000, Evidence::bid(966_315_790, N)),
        (close(&l, N, LONG), 1_050_000_000, Evidence::ask(1_029_000_000, N)),
    ] {
        let w = pend(&g, q);
        assert_eq!(w.w.since, 1_500);
        let d = ch_draft(&[w.clone()], &counter, &fk(), 1);
        accept(&d);
        assert_eq!(d.pairs[0].1.since, 1_500);
        for b in bad {
            let mut y = d.pairs[0].1.clone();
            y.since = b;
            let mut x = d.clone();
            x.set_pair(0, &y);
            refused(&x, 0);
        }
        if w.w.kind == KIND_LIQ {
            let cd = cure_draft(&w, &lk(), &fk(), 2);
            accept(&cd);
            assert_eq!(cd.pairs[0].1.since, 1_500);
            for b in bad {
                let mut y = cd.pairs[0].1.clone();
                y.since = b;
                let mut x = cd.clone();
                x.set_pair(1, &y);
                refused(&x, 0);
            }
        }
    }
}

/// A restore keeps the clock, so it must never carry a part's old evidence to a NEW owner of a side who entered after
/// that evidence rested. A new owner enters only by a hand-over (T1, P3, P4) or by a forfeit, and each writes since = 0
/// on the part that changed hands; a proposal on that part records the part's own clock S' (its UTXO's DAA), and its
/// restore keeps S', never the S of the pair before the hand-over (the pending state copies the clock of the UTXO it
/// spends, and no entry rewrites it). Shown with an ask that rested on the old clock S and that conforming matchers
/// stop filling before S' + R + L: it liquidates the old pair, it is refused against the transferred part and against
/// the forfeited part, and still refused against each of them after a propose-and-restore cycle.
#[test]
fn holds_old_evidence_never_reaches_a_new_owner_through_a_restore() {
    let s = 100_000u64;
    let life = market().min_evidence_life as u64;
    let old = expiring(Evidence::ask(940_000_000, N).placed(s + 10), (s + 10 + R + life + 500) as i64);
    let fresh_ask = |at: u64| Evidence::ask(940_000_000, N).placed(at).other(0xd1);
    let at_lock = |d: &Draft, lock: u64| {
        let mut x = d.clone();
        x.lock = lock;
        x
    };
    // The old pair: the old ask is evidence on S.
    let l = listed_lv(0x41, s);
    accept(&at_lock(&ev_draft(&[liq(&l, N, LONG)], &old, &fk(), 1), s + 10 + R));
    // Case 1, a transfer: a new long enters at S + 4R; the part restarts at its own UTXO S'.
    let ts = TransferSpec { side: LONG, n: N, x: 1_000_000_000, nb: 900_000_000, ek: xonly(&keypair(16)), et: ZERO, k: 0 };
    let td = transfer_draft(&l, &ts, &Entrant::Direct(keypair(16)), &fk(), 2);
    let tx = accept(&td);
    let s1 = s + 4 * R;
    let part = live_pairs(&tx, &td).remove(0).at(s1);
    // Case 2, a forfeit: the short wins the long side (finalize at S + 10 + R + W); the part restarts at S''.
    let pd = at_lock(&ev_draft(&[liq(&l, N, LONG)], &old, &fk(), 3), s + 10 + R);
    let ptx = accept(&pd);
    let w0 = live_pendings(&ptx, &pd).remove(0).at(s + 10 + R);
    let fd = finalize_draft(&w0, &fk(), 4);
    let ftx = accept(&fd);
    let s2 = s + 10 + R + window();
    let forfeited = live_pairs(&ftx, &fd).remove(0).at(s2);
    for (name, np, s_new) in [("transfer", part, s1), ("forfeit", forfeited, s2)] {
        assert_eq!((np.p.since, np.clock()), (0, s_new as i64), "{name}: a fresh clock");
        refused(&at_lock(&ev_draft(&[liq(&np, N, LONG)], &old, &fk(), 5), s_new + R + 1_000), 0);
        // A proposal on the new part with fresh evidence (rested R on S'), refuted, restored.
        let p_daa = s_new + R;
        let g = liq(&np, N, LONG);
        accept(&at_lock(&ev_draft(&[g.clone()], &fresh_ask(s_new), &fk(), 6), p_daa));
        let w = pend_of(&g, 940_000_000, p_daa);
        assert_eq!(w.w.since, s_new as i64, "{name}: the pending part records the new part's clock");
        let mut c = ch_draft(&[w], &Evidence::bid(980_000_000, N).placed(p_daa), &fk(), 7);
        c.lock = p_daa + R;
        let ctx = accept(&c);
        let restored = live_pairs(&ctx, &c).remove(0).at(p_daa + R);
        assert_eq!(restored.p.since, s_new as i64, "{name}: the restore keeps the new part's clock, not the old S");
        assert_ne!(restored.p.since, s as i64);
        refused(&at_lock(&ev_draft(&[liq(&restored, N, LONG)], &old, &fk(), 8), p_daa + R + 1_000), 0);
        // A live quote of the old book still counts, but only from the new part's clock (4.5c), as without the cycle.
        let live_old = Evidence::ask(940_000_000, N).placed(s + 10).other(0xd2);
        accept(&at_lock(&ev_draft(&[liq(&restored, N, LONG)], &live_old, &fk(), 9), p_daa + R));
    }
}

/// The forfeited part restarts its clock (since = 0), so the winner can sell the forfeited side only after its own
/// S' + 4R: the rate limit is not bypassed by a liquidation. A cure keeps S (adding margin never restarts the clock),
/// so a cured part's hand-over limit is that of the old pair.
#[test]
fn holds_rate_limit_after_forfeit_and_cure() {
    let s = 50_000u64;
    let l = lv(0x3a, s);
    let w = pend(&liq(&l, N, LONG), 940_000_000);
    let d = finalize_draft(&w, &fk(), 1);
    let tx = accept(&d);
    let fp = live_pairs(&tx, &d).remove(0).at(NOW + 7_000);
    assert_eq!(fp.handover_from(), NOW + 7_000 + 4 * R);
    let c = cure_draft(&w, &lk(), &fk(), 2);
    let tx = accept(&c);
    let cp = live_pairs(&tx, &c).remove(0).at(NOW + 7_000);
    assert_eq!(cp.handover_from(), s + 4 * R, "a cure keeps the pair's clock");
}

/// One lock time serves several lower bounds; each covenant checks its own, so the transaction needs their maximum.
/// A block that proposes on one pair (rest on its clock, NOW) and challenges another part (rest on P + R = NOW + 300)
/// needs NOW + 300; a finalize is a relative lower bound of its own input (CSV), checked independently of the lock time.
#[test]
fn holds_one_lock_time_covers_every_bound_of_the_block() {
    let a = lv(0x3b, 1_000);
    let b = lv(0x3c, 1_000);
    let w = pend_of(&close(&b, N, LONG), 1_050_000_000, NOW - R + 300);
    let m = vec![Member::P(liq(&a, N, LONG)), Member::W(PendLeg { w, q: 0 })];
    let mut d = blk_draft(&m, &Evidence::ask(940_000_000, 2 * N), &fk(), 1);
    let (t, e) = d.build();
    let r = run(&t, &e).unwrap();
    assert!(r[0].is_ok() && r[1].is_err());
    d.lock = NOW + 300;
    accept(&d);
    let wf = pend(&liq(&a, N, LONG), 940_000_000);
    let mut f = finalize_draft(&wf, &fk(), 2);
    f.lock = 0;
    accept(&f);
    f.seqs = vec![window() - 1, 0];
    f.lock = u32::MAX as u64;
    refused(&f, 0);
}

/// A liquidatable loser races the keeper with a close of its own (B1 of the threat model, unchanged by the two-step
/// rule): while its close is pending the part cannot be proposed against. The keeper refutes the close if the loser's
/// bid was stale (an ask at most 0.98 of it); if the bid was the true market, the loser closes at it and keeps its
/// residual instead of forfeiting, as under 4.5c when its unilateral close landed first.
#[test]
fn holds_loser_close_races_the_liquidation_as_before() {
    let l = lv(0x3d, 1_000);
    let wc = pend(&close(&l, N, LONG), 950_000_000);
    // The keeper's liquidation proposal against the pending part runs the pair's script on the pending UTXO: refused.
    let as_pair = Live { p: wc.w.restore_pair().unwrap(), value: wc.value, id: wc.id, op: wc.op, daa: wc.daa };
    let mut k = ev_draft(&[liq(&as_pair, N, LONG)], &Evidence::ask(940_000_000, N), &fk(), 1);
    k.ins[0].utxo.script_public_key = p2sh(&wc.w.art());
    refused(&k, 0);
    // A bid of 9.50 is refuted by an ask at 9.31 or below; the true market ask of 9.40 does not refute it.
    refused(&ch_draft(&[wc.clone()], &Evidence::ask(940_000_000, N), &fk(), 2), 0);
    accept(&ch_draft(&[wc.clone()], &Evidence::ask(931_000_000, N), &fk(), 2));
    accept(&finalize_draft(&wc, &fk(), 3));
}
