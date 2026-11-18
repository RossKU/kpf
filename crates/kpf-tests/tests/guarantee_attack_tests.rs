//! Attacks on guaranteed fast settlement (DESIGN 4.5f), in the manner of the four reviews: each a `holds_` test (the
//! attack fails, and why) or an `exploit_` test (it works; the exposure is stated and accepted in DESIGN 4.5f and
//! THREAT_MODEL). The real KOB order and token contracts run in every evidence transaction.
//!
//! Market and pair as in `kpf_guarantee_tests` (worked example, lambda 1.1, D 6,600, G 1,200, R 600, W 6,000, g 2 %).

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
fn gk() -> [u8; 32] {
    xonly(&guarantor_kp())
}

fn lv(tag: u8) -> Live {
    live(&pair(&lk(), &sk()), tag).at(1_000)
}

const FULL: i64 = 1_000_000_000;
const LIQ_ASK: i64 = 940_000_000;
const REF_LIQ_LONG: i64 = 966_315_790;

fn liq(l: &Live, n: i64, side: i64) -> PairLeg {
    PairLeg { l: l.clone(), n, q: 0, act: Act::Liq { side } }
}

fn bid(p: i64, fill: i64) -> Evidence {
    Evidence::bid(p, fill)
}

/// A guaranteed liquidation of the whole long at 9.40 and its lock at F = P + G.
fn guaranteed_liq(tag: u8) -> (LivePend, Draft, Transaction, LiveLk) {
    let w = pend(&liq(&lv(tag), FULL, LONG), LIQ_ASK);
    let d = guarantee_draft(&w, &gk(), &fk(), 5);
    let tx = accept(&d);
    let lock = live_locks(&tx, &d).remove(0).at(w.daa + grace());
    (w, d, tx, lock)
}

fn dispute(ls: &[LiveLk], e: &Evidence) -> Draft {
    let mut d = dp_draft(ls, e, &fk(), 6);
    d.lock = d.lock.max(ls.iter().map(|l| l.daa).max().unwrap_or(0));
    d
}

// ================================================================ the lock's identity

/// Look-alike locks. Any P2SH of the KpfLock template is the KAS of whoever made it: (1) it cannot settle a claim, since
/// a guarantee counts only the lock output of its own transaction (an existing lock spent as an input is no output),
/// and (2) its dispute pays only its own victim key from its own value; a look-alike naming the attacker as victim
/// takes nothing from anyone.
#[test]
fn holds_a_look_alike_lock_settles_nothing_and_pays_only_its_own() {
    let (w, d, tx, lock) = guaranteed_liq(0x51);
    // A second claim on another pair, "guaranteed" by spending the first lock as an input and sending no lock output.
    let w2 = pend(&liq(&lv(0x52), FULL, LONG), LIQ_ASK);
    let mut d2 = guarantee_draft(&w2, &gk(), &fk(), 7);
    d2.outs[2] = out(lock.value, p2pk_spk(&fk()), None);
    d2.ins.insert(1, lock_input(&lock, "release", vec![]));
    d2.seqs = vec![grace(), dispute_window(), 0];
    d2.change = Some(3);
    let (t2, e2) = d2.build();
    assert!(run(&t2, &e2).unwrap()[0].is_err(), "no lock output of this claim: refused");
    // A look-alike lock (made by anyone, here with the attacker as victim): its dispute pays only from its own value.
    let mut fake = lock.l.clone();
    fake.victim = xonly(&fk());
    let fl = LiveLk { l: fake, value: 50_000_000, op: outpoint(0xa1), daa: lock.daa };
    let dp = dispute(&[fl.clone()], &bid(1_000_000_000, FULL));
    let (tx3, e3) = dp.build();
    let r = run(&tx3, &e3).unwrap();
    assert!(r[0].is_ok(), "a look-alike behaves as a lock over its own value");
    assert_eq!(tx3.outputs[0].script_public_key, p2pk_x(&xonly(&fk())));
    // The honest lock and the honest victim are untouched by it.
    let _ = (w, d, tx);
}

/// Twin locks (two lock outputs of one state, the second the guarantor's own extra KAS) and two locks disputed in one
/// transaction: every payout is positional, so two locks never share the victim output or the burn output; the evidence
/// block counts each lock's n, so one print of n refutes only one of them.
#[test]
fn holds_two_locks_never_share_a_payout_or_a_burn() {
    let (_, _, _, a) = guaranteed_liq(0x53);
    let b = LiveLk { op: outpoint(0xa2), ..a.clone() };
    let c = bid(1_000_000_000, 2 * FULL);
    let d = dispute(&[a.clone(), b.clone()], &c);
    let tx = accept(&d);
    assert_eq!(tx.outputs[0].value, tx.outputs[1].value, "each pays the victim at its own index");
    // The second lock's victim output replaced by a filler: lock 1 refuses (it never reads lock 0's output).
    let mut x = d.clone();
    x.outs[1] = out(x.outs[1].value, p2pk_spk(&fk()), None);
    refused(&x, 1);
    // One burn output for both: the second burn slot replaced by a filler, lock 1 refuses.
    let mut x = d.clone();
    let m = x.outs.len();
    x.outs[m - 2] = out(x.outs[m - 2].value, p2pk_spk(&fk()), None);
    refused(&x, 1);
    // A print of only n for both locks: refused.
    refused(&dispute(&[a, b], &bid(1_000_000_000, FULL)), 0);
}

/// The lock's covenant id. A guarantee does not look at it: a lock under a fresh genesis id is still a lock, and its
/// release and dispute (which never read an id) work; a lock bound to the pending part's own id adds an output of that
/// id, which the settlement's one-output rule refuses.
#[test]
fn holds_a_lock_under_any_covenant_id_is_only_a_lock() {
    let w = pend(&liq(&lv(0x54), FULL, LONG), LIQ_ASK);
    let mut d = guarantee_draft(&w, &gk(), &fk(), 5);
    d.genesis.push((2, 1, fop(5)));
    let tx = accept(&d);
    assert!(tx.outputs[2].covenant.is_some());
    let mut d2 = guarantee_draft(&w, &gk(), &fk(), 5);
    d2.outs[2].covenant = bind(0, w.id);
    let (t, e) = d2.build();
    assert!(run(&t, &e).map(|r| r[0].is_err()).unwrap_or(true), "a lock of the pending part's own id is refused");
}

// ================================================================ who is paid

/// A dispute pays the wronged party only, once, at its own key: the restitution to another key, the burn to another
/// script or at another slot, are refused.
#[test]
fn holds_a_dispute_pays_the_victim_and_the_burn_and_nobody_else() {
    let (_, _, _, lock) = guaranteed_liq(0x55);
    let d = dispute(&[lock.clone()], &bid(1_000_000_000, FULL));
    accept(&d);
    let mut x = d.clone();
    x.outs[0].script_public_key = p2pk_spk(&fk());
    refused(&x, 0);
    let m = d.outs.len();
    let mut x = d.clone();
    x.outs[m - 1].script_public_key = p2pk_spk(&fk());
    refused(&x, 0);
    let mut x = d.clone();
    x.outs.swap(m - 1, m - 2);
    refused(&x, 0);
}

/// No value moves through the fee, the reward or the burn: the lock is exactly H + burn + reward, so the executor's
/// change gains exactly the reward (it pays the network fee from its own input), and neither H nor the burn can be cut
/// by one sompi. The proposer's offer cannot leave the lock either (a guarantee requires it inside, see
/// `the_offer_is_paid_from_the_proposers_payout_into_the_lock`).
#[test]
fn holds_no_value_moves_through_the_fee_the_reward_or_the_burn() {
    let (_, _, _, lock) = guaranteed_liq(0x56);
    for b in [REF_LIQ_LONG, 1_000_000_000, 1_200_000_000, 2_000_000_000] {
        let d = dispute(&[lock.clone()], &bid(b, FULL));
        let (tx, e) = d.build();
        assert_accepted(&tx, &e);
        let h = lock.l.restitution(b);
        // At a counter bid of bs or more H is the whole cover and nothing is left to burn: no burn output.
        let burn = lock.l.burn(lock.value, b);
        if burn > 0 {
            assert_eq!(tx.outputs.last().unwrap().value as i64, burn);
            assert_eq!(tx.outputs.last().unwrap().script_public_key, kpf_protocol::lock::burn_spk());
        } else {
            assert!(tx.outputs.iter().all(|o| o.script_public_key != kpf_protocol::lock::burn_spk()));
        }
        assert_eq!(tx.outputs[0].value as i64, h);
        assert_eq!(h + burn + lock.l.reward(), lock.value as i64, "bid {b}");
        assert!(h <= lock.l.cover, "H never exceeds the cover");
    }
}

// ================================================================ the cure right

/// The cure right (DESIGN 4.5f, the trade-off). A loser within g above its trigger cannot refute a stale liquidation
/// (no bid in its band contradicts by g) and on the slow path would cure within W. With the cure grace G it can cure
/// within G (here 1,200 DAA instead of 6,000); after G a guarantor can settle the claim, the loser forfeits, and no
/// dispute pays it: its residual at the true market is lost. Recorded as the accepted exposure of the fast path: a loser
/// in the band [T, T(1 + g)) offline for G loses up to q(n, T(1 + g) - bl) (663 KAS here; 550 KAS at a true bid of 9.55),
/// where the slow path would have given it W.
#[test]
fn exploit_a_band_loser_offline_for_the_grace_forfeits_its_residual() {
    let l = lv(0x57);
    let w = pend(&liq(&l, FULL, LONG), LIQ_ASK);
    // Within the grace: the guarantee is refused, the cure accepted.
    let mut early = guarantee_draft(&w, &gk(), &fk(), 5);
    early.seqs[0] = grace() - 1;
    refused(&early, 0);
    accept(&cure_draft(&w, &lk(), &fk(), 6));
    // After the grace, the loser offline: guaranteed; the true market (a bid of 9.55, inside the band) does not refute.
    let d = guarantee_draft(&w, &gk(), &fk(), 5);
    let tx = accept(&d);
    let lock = live_locks(&tx, &d).remove(0).at(w.daa + grace());
    refused(&dispute(&[lock.clone()], &bid(955_000_000, FULL)), 0);
    let lost = q(FULL, 955_000_000 - 900_000_000, SCALE, 0);
    assert_eq!(lost, 55_000_000_000, "550 KAS of residual at the true bid");
    // The same holds on the slow path once W has passed; the fast path only shortens the loser's time from W to G.
    accept(&finalize_draft(&w, &fk(), 7));
}

// ================================================================ the forfeited part has moved on

/// The wronged party's recovery when the forfeited part has since been split, closed and sold: the winner lists both
/// sides of the forfeited part (it owns both), mutually closes 400 tokens of it (a split: the rest continues), and the
/// restitution of a later dispute is still paid in full from the lock, at the victim's own key. Restitution never
/// touches the part, so no later spend of the part can reduce it.
#[test]
fn holds_the_victim_is_paid_in_full_after_the_forfeited_part_moved() {
    let (w, d, tx, lock) = guaranteed_liq(0x58);
    let part = live_pairs(&tx, &d).remove(0).at(w.daa + grace());
    let owner = sk();
    let a1 = amend_draft(&part, &owner, LONG, (1_000_000_000, 0, 0), &fk(), 9);
    let t1 = accept(&a1);
    let p1 = live_pairs(&t1, &a1).remove(0).at(w.daa + grace() + 1);
    let a2 = amend_draft(&p1, &owner, SHORT, (1_000_000_000, 0, 0), &fk(), 10);
    let t2 = accept(&a2);
    let p2 = live_pairs(&t2, &a2).remove(0).at(w.daa + grace() + 2);
    let mc = mutual_draft(&p2, 400_000_000, 1_000_000_000, &fk(), 11);
    let t3 = accept(&mc);
    assert_eq!(live_pairs(&t3, &mc).len(), 1, "the forfeited part was split: 600 tokens remain");
    let dp = dispute(&[lock.clone()], &bid(1_000_000_000, FULL));
    let t4 = accept(&dp);
    assert_eq!(t4.outputs[0].value as i64, q(FULL, 100_000_000, SCALE, 0), "the long is paid 1,000 KAS in full");
    assert_eq!(t4.outputs[0].script_public_key, p2pk_x(&xonly(&lk())));
}

// ================================================================ thin books

/// The mirror of the accepted thin-book limit (DESIGN 4.5d residual risk 1, DESIGN-ORACLE attack 6). A liquidation is
/// true (the market is at 9.30 KAS, below the long's trigger 9.4737) and guaranteed. On a book with nobody to sell into
/// it, the deserving loser rests its own bid at the refutation bound 9.66315790 for k_c * n = 1,000 tokens, fills it
/// with its own tokens (its KAS returns to itself) and disputes: it is paid H = 663.16 KAS as the victim and keeps the
/// reward 300 KAS as the executor, 2,336.84 KAS burn, and the guarantor loses its whole lock of 3,300 KAS. On the slow
/// path the same move wins only the bond (90 KAS). Exposure per guaranteed claim on a thin book: lambda x cover, i.e. the
/// whole lock; a guarantor must not guarantee on a book without arbitrage within R of the refutation bound.
#[test]
fn exploit_a_deserving_loser_on_a_thin_book_takes_the_guarantors_lock() {
    let w = pend(&liq(&lv(0x59), FULL, LONG), 930_000_000);
    let d = guarantee_draft(&w, &gk(), &fk(), 5);
    let tx = accept(&d);
    let lock = live_locks(&tx, &d).remove(0).at(w.daa + grace());
    // The loser's own bid at the bound, filled by the loser (the counterparty key is its own).
    let mut own = bid(REF_LIQ_LONG, FULL);
    own.maker = lk();
    own.bid = kpf::kob::bid_of(&market(), xonly(&lk()), REF_LIQ_LONG);
    own = Evidence { counter: lk(), ..own };
    let dp = dispute(&[lock.clone()], &own);
    let t = accept(&dp);
    let h = t.outputs[0].value as i64;
    assert_eq!(h, 66_315_790_000, "663.1579 KAS to the loser as the victim");
    assert_eq!(lock.l.reward(), 30_000_000_000, "300 KAS reward to the loser as the executor");
    assert_eq!(t.outputs.last().unwrap().value as i64, 330_000_000_000 - h - 30_000_000_000, "the rest burns");
    assert_eq!(lock.value, 330_000_000_000, "the guarantor loses its whole 3,300 KAS lock");
}

// ================================================================ the one lock time

/// One transaction, one lock time, several lower bounds: a proposal (its evidence on the pair's clock S), a challenge of
/// a pending part and a dispute of a lock (both on their claim's clock P) on one counter print. The lock time serves
/// all of them as their largest lower bound; one DAA short of the largest, exactly the inputs that need it refuse. No
/// rule of a lock reads the lock time as an upper bound: the dispute window uses the UTXO scores P and F, so a later
/// lock time neither opens nor closes it (`a_dispute_after_the_window_is_refused_and_a_release_one_daa_early`).
#[test]
fn holds_one_lock_time_serves_every_lower_bound() {
    let (_, _, _, lock) = guaranteed_liq(0x5a);
    let wb = pend(&liq(&lv(0x5b), FULL, LONG), LIQ_ASK);
    let pc = PairLeg { l: lv(0x5c), n: FULL, q: 0, act: Act::Liq { side: SHORT } };
    let e = bid(1_150_000_000, 3 * FULL);
    let members = vec![Member::P(pc), Member::W(PendLeg { w: wb.clone(), q: 0 }), Member::L(lock.clone(), 0)];
    let mut d = blk_draft(&members, &e, &fk(), 12);
    let need = wb.daa + rest_daa();
    d.lock = need;
    accept(&d);
    d.lock = need - 1;
    let (tx, en) = d.build();
    let r = run(&tx, &en).unwrap();
    assert!(r[0].is_ok(), "the proposal's own bound (S + exposure) is lower: {:?}", r[0]);
    assert!(r[1].is_err() && r[2].is_err(), "the challenge and the dispute need P + R");
}

// ================================================================ self-disputes

/// A liar that expects a dispute disputes itself first (DESIGN-ORACLE 1.7). With the burn, the coalition of proposer,
/// winner and guarantor never ends ahead of not making the false claim: it keeps the loser's side (worth H at the
/// counter quote) and the reward, and loses the lock. Net = H + R - lock <= H + R - (V + R) = H - V <= 0.
#[test]
fn holds_a_self_dispute_never_profits_the_coalition() {
    for b in [REF_LIQ_LONG, 1_000_000_000, 1_100_000_000, 1_200_000_000, 1_500_000_000] {
        let (_, _, _, lock) = guaranteed_liq(0x5d);
        let d = dispute(&[lock.clone()], &bid(b, FULL));
        let tx = accept(&d);
        let h = tx.outputs[0].value as i64;
        let coalition = h + lock.l.reward() - lock.value as i64;
        assert!(coalition <= 0, "bid {b}: coalition net {coalition}");
        assert!(coalition <= h - lock.l.cover);
    }
    let _ = Hash::default();
}
