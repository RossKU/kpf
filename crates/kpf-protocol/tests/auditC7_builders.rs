//! Review C7 (docs/audit/REVIEW-C7.md): the fix of review C6's C6-F1 (a slash leaves 1 KAS while the stake has another
//! hold), driven through every order of settling two and three charges on a member and on an exit, through the tool's
//! builders on the pinned artifacts. The harness is that of auditC5_builders.rs; a charge's end (refuted with a NO
//! total, upheld, lapsed) is written into its state directly, as the judge's verdict would write it.
//!
//! `c7_holds_` what was attacked and held.

#![allow(dead_code, unused_imports)]

use std::cell::Cell;

use kaspa_consensus_core::tx::{CovenantBinding, TransactionId, TransactionOutpoint, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::committee::*;
use kpf_protocol::committee_build::*;
use kpf_protocol::judge::{JudgeMarket, JudgeParams, Js, NO, YES};
use kpf_protocol::oracle_build::{deposit, pool_close, pool_settle, OBuilt, OLive, OPlan, OSigned, Trust};
use kpf_protocol::tx::{bv, p2pk_spk, p2sh_spk, Arg, KeySet, KeyUtxo, MIN_FEE_RATE};
use kpf_protocol::verify::{data_sig_ops, verify};
use secp256k1::{Keypair, Secp256k1, SecretKey};

const KAS1: i64 = 100_000_000;
const MARKET: [u8; 32] = [0x4d; 32];
const RATE: u64 = MIN_FEE_RATE;

fn kp(seed: u8) -> Keypair {
    Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&[seed; 32]).expect("a secret key"))
}
fn xk(k: &Keypair) -> [u8; 32] {
    k.x_only_public_key().0.serialize()
}
fn b8(v: i64) -> Arg {
    bv(&kpf_protocol::state::sm8(v))
}
fn kas(v: i64) -> f64 {
    v as f64 / KAS1 as f64
}
fn signed(what: &str, b: &OBuilt, keys: &KeySet) -> OSigned {
    let sg = b.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{what}: {:?}", v.errors());
    sg
}
/// The edited transaction is signed and refused by the script of input `input`.
fn refused(what: &str, b: &OBuilt, keys: &KeySet, input: usize) {
    let sg = b.sign(keys).unwrap_or_else(|e| panic!("{what}: the builder did not sign: {e}"));
    let v = verify(&sg.tx, &sg.entries);
    assert!(!v.is_ok(), "{what}: ACCEPTED");
    assert!(!v.input_ok(input), "{what}: refused, but not by input {input}: {:?}", v.errors());
}
fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
    let o = &sg.tx.outputs[i];
    OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
}
/// Input i spends another object: its script, value, DAA score and covenant id.
fn swap(b: &mut OBuilt, i: usize, redeem: Vec<u8>, value: u64, daa: u64, id: Option<Hash>) {
    b.entries[i] = UtxoEntry::new(value, p2sh_spk(&redeem), daa, false, id);
    if let OPlan::Contract { redeem: r, .. } = &mut b.plans[i] {
        *r = redeem;
    }
}
fn set_args(b: &mut OBuilt, i: usize, f: impl FnOnce(&mut Vec<Arg>)) {
    if let OPlan::Contract { args, .. } = &mut b.plans[i] {
        f(args);
    }
}
fn set_entry(b: &mut OBuilt, i: usize, e: &'static str) {
    if let OPlan::Contract { entry, .. } = &mut b.plans[i] {
        *entry = e;
    }
}
/// Input `i` of `from` is added to `to` as its last input.
fn add_input(to: &mut OBuilt, from: &OBuilt, i: usize) -> usize {
    to.tx.inputs.push(from.tx.inputs[i].clone());
    to.entries.push(from.entries[i].clone());
    to.plans.push(from.plans[i].clone());
    to.input_labels.push(from.input_labels[i].clone());
    to.tx.inputs.len() - 1
}

struct World {
    rid: Hash,
    members: Vec<Keypair>,
    me: Keypair,
    judge1: Keypair,
    judge2: Keypair,
    thief: Keypair,
    keys: KeySet,
    jm: JudgeMarket,
    committee: OLive<Committee>,
    n: Cell<u16>,
}

/// A committee of the members `weights` (KAS) whose charges cost 5 KAS, wait 300 DAA for a contest and are judged by
/// stakes of at least 20 KAS aged 200 DAA, in rounds of 100 DAA (mu 1.25, a tenth of the losers' stake burned).
fn world_of(weights: &[i64]) -> World {
    let rid = Hash::from_bytes([0x52; 32]);
    let jm = JudgeMarket { id: [0x6a; 32], p: JudgeParams { mu_bps: 12_500, age: 200, unbond: 200, round: 100, burn_bps: 1_000, min_stake: 20 * KAS1 } };
    let rp = CommitteeParams { charge: ChargeParams { judge: jm, bond: 5 * KAS1, daa: 300 }, ..CommitteeParams::testnet() };
    let members: Vec<Keypair> = (0..weights.len() as u8).map(|i| kp(101 + i)).collect();
    let (me, judge1, judge2, thief) = (kp(77), kp(78), kp(79), kp(90));
    let mut keys = KeySet::new();
    for k in members.iter().chain([&me, &judge1, &judge2, &thief]) {
        keys.add_keypair(*k);
    }
    let mut table = EMPTY_TABLE;
    for (i, w) in weights.iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: w * KAS1 };
    }
    let r = Committee { p: rp, from: 6_000, members: table, holds: NO_HOLDS };
    let total: i64 = weights.iter().sum();
    let committee = OLive { s: r, value: (total * KAS1) as u64 + COMMITTEE_CARRIER, id: Some(rid), op: TransactionOutpoint { transaction_id: TransactionId::from_bytes([1; 32]), index: 0 }, daa: 6_000 };
    World { rid, members, me, judge1, judge2, thief, keys, jm, committee, n: Cell::new(2) }
}
fn world() -> World {
    world_of(&[30, 22, 15])
}

struct Judged {
    decided: OLive<Charge>,
    pool: OLive<Js>,
    receipt: OLive<Js>,
    contest: OBuilt,
    decide: OBuilt,
    contested: OLive<Charge>,
    dispute: OLive<Js>,
}

impl World {
    /// 1,000 KAS of `key` at an outpoint of its own.
    fn fu(&self, key: [u8; 32]) -> Vec<KeyUtxo> {
        let n = self.n.get();
        self.n.set(n + 1);
        let mut id = [0xf0u8; 32];
        id[0] = (n >> 8) as u8;
        id[1] = n as u8;
        vec![KeyUtxo { outpoint: TransactionOutpoint { transaction_id: TransactionId::from_bytes(id), index: 0 }, amount: 1_000 * KAS1 as u64, daa: 0, key }]
    }
    /// A free judge stake of `owner`, accepted at `daa`.
    fn stake(&self, owner: &Keypair, kas: i64, daa: u64) -> OLive<Js> {
        self.stake_of(&self.jm, owner, kas, daa)
    }
    fn stake_of(&self, jm: &JudgeMarket, owner: &Keypair, kas: i64, daa: u64) -> OLive<Js> {
        let mek = xk(&self.me);
        let b = judge_stake(jm, xk(owner), (kas * KAS1) as u64, &self.fu(mek), &mek, RATE).expect("a judge stake");
        let sg = signed("judge stake", &b, &self.keys);
        nth(&sg, 1, Js::free(xk(owner), *jm), daa)
    }
    /// Member `i` is charged for `signed price`; the charge lands at `at`.
    fn charge_member(&self, r: &OLive<Committee>, i: usize, signed_price: &SignedPrice, pay: [u8; 32], at: u64) -> (OLive<Committee>, OLive<Charge>, [u8; 32], OBuilt) {
        let (b, c, cid) = charge(&Staked::Member(r, i), &MARKET, signed_price, pay, &self.fu(pay), &pay, RATE).expect("the charge");
        let sg = signed("committee-charge", &b, &self.keys);
        (nth(&sg, 0, r.s.charged(i, &cid.as_bytes()).unwrap(), at), nth(&sg, 1, c, at), cid.as_bytes(), b)
    }
    /// The contest lands at `at`; `answer` joins YES 20 DAA later; the verdict lands 10 DAA after the round.
    fn judge(&self, ch: &OLive<Charge>, opener: &OLive<Js>, answer: Option<&OLive<Js>>, at: u64) -> Judged {
        let mek = xk(&self.me);
        let (contest, c2, dsp) = charge_contest(ch, opener, 10_000_000, &self.fu(mek), &mek, RATE).expect("the contest");
        let sg = signed("charge-contest", &contest, &self.keys);
        let contested = nth(&sg, 0, c2, at);
        let receipt = nth(&sg, 1, Js::receipt(opener.s.owner, opener.s.m, NO, opener.value as i64, 0, ch.daa as i64), at);
        let mut dispute = nth(&sg, 2, dsp, at);
        if let Some(a) = answer {
            let (b, d2) = deposit(&dispute, YES, &[a.clone()], &self.fu(mek), &mek, RATE, &Trust::Untraced).expect("the answer");
            let sg = signed("the answer on YES", &b, &self.keys);
            dispute = nth(&sg, 0, d2, at + 20);
        }
        let (decide, c3, pool) = charge_decide(&dispute, &contested, &self.fu(mek), &mek, RATE).expect("the verdict");
        let sg = signed("charge-decide", &decide, &self.keys);
        let landed = sg.tx.lock_time + 10;
        Judged { decided: nth(&sg, 1, c3, landed), pool: nth(&sg, 0, pool, landed), receipt, contest, decide, contested, dispute }
    }
}


/// An exit is charged for `signed price`; the charge lands at `at`.
fn charge_exit(w: &World, x: &OLive<Exit>, signed_price: &SignedPrice, pay: [u8; 32], at: u64) -> (OLive<Exit>, OLive<Charge>, [u8; 32]) {
    let (b, c, cid) = charge(&Staked::Exit(x), &MARKET, signed_price, pay, &w.fu(pay), &pay, RATE).expect("the charge of an exit");
    let sg = signed("exit-charge", &b, &w.keys);
    (nth(&sg, 0, x.s.charged(&cid.as_bytes()).unwrap(), at), nth(&sg, 1, c, at), cid.as_bytes())
}

// ------------------------------------------------------------------------------------------------ review C7

/// How a charge ended: refuted with a NO total (KAS), upheld, or lapsed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum End {
    Refuted(i64),
    Upheld,
    Lapsed,
}

fn ended(ch: &OLive<Charge>, e: End) -> OLive<Charge> {
    let (status, clock) = match e {
        End::Refuted(no) => (CHARGE_REFUTED, no * KAS1),
        End::Upheld => (CHARGE_UPHELD, 30 * KAS1),
        End::Lapsed => (CHARGE_LAPSED, 30 * KAS1),
    };
    OLive { s: Charge { status, clock, ..ch.s }, ..ch.clone() }
}

fn permutations(n: usize) -> Vec<Vec<usize>> {
    if n == 1 {
        return vec![vec![0]];
    }
    let mut all = vec![];
    for p in permutations(n - 1) {
        for at in 0..n {
            let mut q = p.clone();
            q.insert(at, n - 1);
            all.push(q);
        }
    }
    all
}

fn assignments(kinds: &[End], n: usize) -> Vec<Vec<End>> {
    let mut all = vec![vec![]];
    for _ in 0..n {
        all = all.into_iter().flat_map(|v: Vec<End>| kinds.iter().map(move |k| [v.clone(), vec![*k]].concat())).collect();
    }
    all
}

/// What one settlement paid: (taken from the stake, to the author beyond its bond, the bond's fate checked).
fn check_bond(sg: &OSigned, ch: &OLive<Charge>, out: &Settled, key: &[u8; 32]) {
    let o = &sg.tx.outputs[1];
    match out {
        Settled::Slashed { reward, .. } => assert_eq!((o.value as i64, o.script_public_key.clone()), (ch.value as i64 + reward, p2pk_spk(&ch.s.pay_key)), "the author's bond and tenth"),
        Settled::Cleared { bond_to_key: true } => assert_eq!((o.value, o.script_public_key.clone()), (ch.value, p2pk_spk(key)), "the bond to the charged key"),
        Settled::Cleared { bond_to_key: false } => assert_eq!((o.value, o.script_public_key.clone()), (ch.value, p2sh_spk(&[0x6a])), "the bond burned"),
    }
}

/// (a) Every order of settling two and three charges on one member of 22 KAS, each refuted (with 20, 22 or 40 KAS on
/// NO), upheld or lapsed: every charge is settled and its bond paid where its end says; the stake is gone only with
/// its last hold; what is left under no hold is at least 1 KAS, which the key takes out by leaving.
#[test]
fn c7_holds_every_order_of_settling_the_charges_of_a_member_settles_them_all() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let member = xk(&w.members[1]);
    let mut committee = w.committee.clone();
    let (mut chs, mut starts) = (vec![], vec![]);
    for n in 0..3i64 {
        let (r, ch, cid, _) = w.charge_member(&committee, 1, &sign_price(&w.members[1], &rid, &MARKET, 6_500 + n, 20 * KAS1), mek, 6_600 + n as u64);
        committee = r;
        chs.push((ch, cid));
        starts.push(committee.clone());
    }
    let (two, three) = (starts[1].clone(), starts[2].clone());
    let kinds = [End::Refuted(20), End::Refuted(22), End::Refuted(40), End::Upheld, End::Lapsed];
    let (mut runs, mut txs, mut emptied, mut kept_one, mut zero_takes) = (0, 0, 0, 0, 0);
    for (n, start) in [(2usize, &two), (3, &three)] {
        for ends in assignments(if n == 2 { &kinds } else { &kinds[1..] }, n) {
            for order in permutations(n) {
                let mut r = start.clone();
                let mut open: Vec<[u8; 32]> = chs[..n].iter().map(|c| c.1).collect();
                let mut total_taken = 0;
                for (step, &k) in order.iter().enumerate() {
                    let (ch, cid) = (ended(&chs[k].0, ends[k]), chs[k].1);
                    let (a, later) = hold_witness(&open, &cid).expect("a witness");
                    let w_before = r.s.members[1].weight;
                    assert!(w_before >= KAS1, "{ends:?} {order:?}: a hold on a stake of {w_before}");
                    let (b, out) = charge_settle(&Staked::Member(&r, 1), &ch, &a, &later, 7_000, &w.fu(mek), &mek, RATE).unwrap_or_else(|e| panic!("{ends:?} {order:?} step {step}: {e}"));
                    let sg = signed("a settlement", &b, &w.keys);
                    txs += 1;
                    check_bond(&sg, &ch, &out, &member);
                    let next = match out {
                        Settled::Slashed { stake, taken, reward, burned } => {
                            let others = open.len() > 1;
                            assert_eq!((stake, taken, reward + burned), (w_before, take_of(w_before, ch.s.clock, others).unwrap(), taken));
                            assert!(!others || w_before - taken >= KAS1, "{ends:?} {order:?}: a slash with other holds leaves the floor");
                            zero_takes += (taken == 0) as u32;
                            total_taken += taken;
                            r.s.slash(1, &ch.s, &cid, &a, &later).unwrap().0
                        }
                        Settled::Cleared { .. } => r.s.clear(1, &cid, &a, &later).unwrap(),
                    };
                    open.retain(|x| *x != cid);
                    assert_eq!(next.holds[1], open.iter().fold(NO_HOLD, |h, x| hold_add(&h, x)));
                    assert_eq!(next.members[1].weight, w_before - if let Settled::Slashed { taken, .. } = out { taken } else { 0 });
                    r = nth(&sg, 0, next, 7_000 + step as u64);
                    assert_eq!(r.value as i64, start.value as i64 - total_taken, "the committee keeps its value less what was taken");
                }
                let left = r.s.members[1].weight;
                assert!(open.is_empty() && r.s.holds == NO_HOLDS && (left == 0 || left >= KAS1), "{ends:?} {order:?}: {left} left");
                if left == 0 {
                    emptied += 1;
                } else if left == KAS1 {
                    kept_one += 1;
                }
                runs += 1;
            }
        }
    }
    // A member left with 1 KAS and no hold (its last charge was cleared, not slashed) leaves and is withdrawn.
    let ends = [End::Refuted(40), End::Lapsed];
    let mut r = two.clone();
    let mut open: Vec<[u8; 32]> = chs[..2].iter().map(|c| c.1).collect();
    for k in 0..2 {
        let (ch, cid) = (ended(&chs[k].0, ends[k]), chs[k].1);
        let (a, later) = hold_witness(&open, &cid).unwrap();
        let (b, out) = charge_settle(&Staked::Member(&r, 1), &ch, &a, &later, 7_000, &w.fu(mek), &mek, RATE).unwrap();
        let sg = signed("a settlement", &b, &w.keys);
        let next = if let Settled::Slashed { .. } = out { r.s.slash(1, &ch.s, &cid, &a, &later).unwrap().0 } else { r.s.clear(1, &cid, &a, &later).unwrap() };
        open.retain(|x| *x != cid);
        r = nth(&sg, 0, next, 7_000);
    }
    assert_eq!((r.s.members[1].weight, r.s.holds[1]), (KAS1, NO_HOLD));
    let (b, _, e) = leave(&r, 1, &w.fu(mek), &mek, RATE).expect("the member of 1 KAS leaves");
    let sg = signed("committee-leave", &b, &w.keys);
    signed("exit-withdraw", &withdraw(&nth(&sg, 1, e, 7_100), &w.fu(mek), &mek, RATE).expect("the withdraw"), &w.keys);
    println!("C7 (a) a member: {runs} orders, {txs} settlements, all accepted; {emptied} end with the member empty, {kept_one} with 1 KAS under no hold; {zero_takes} slashes took nothing and returned the bond");
    assert!(zero_takes > 0 && emptied > 0 && kept_one > 0);
}

/// (a) The same on an exit of 22 KAS: its continuation carries what is left and the other holds, it is gone only with
/// its last hold, and what is left under no hold is withdrawn.
#[test]
fn c7_holds_every_order_of_settling_the_charges_of_an_exit_settles_them_all() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let (lb, _, e) = leave(&w.committee, 1, &w.fu(mek), &mek, RATE).expect("the leave");
    let mut exit = nth(&signed("committee-leave", &lb, &w.keys), 1, e, 6_550);
    let (mut chs, mut starts) = (vec![], vec![]);
    for n in 0..3i64 {
        let (x, ch, cid) = charge_exit(&w, &exit, &sign_price(&w.members[1], &rid, &MARKET, 6_500 + n, 20 * KAS1), mek, 6_600 + n as u64);
        exit = x;
        chs.push((ch, cid));
        starts.push(exit.clone());
    }
    let kinds = [End::Refuted(20), End::Refuted(22), End::Refuted(40), End::Upheld, End::Lapsed];
    let (mut runs, mut txs, mut gone, mut withdrawn) = (0, 0, 0, 0);
    for n in [2usize, 3] {
        for ends in assignments(if n == 2 { &kinds } else { &kinds[1..] }, n) {
            for order in permutations(n) {
                let mut x = Some(starts[n - 1].clone());
                let mut open: Vec<[u8; 32]> = chs[..n].iter().map(|c| c.1).collect();
                for (step, &k) in order.iter().enumerate() {
                    let live = x.clone().unwrap_or_else(|| panic!("{ends:?} {order:?}: the exit is gone with {} holds open", open.len()));
                    let (ch, cid) = (ended(&chs[k].0, ends[k]), chs[k].1);
                    let (a, later) = hold_witness(&open, &cid).expect("a witness");
                    let (b, out) = charge_settle(&Staked::Exit(&live), &ch, &a, &later, 7_000, &w.fu(mek), &mek, RATE).unwrap_or_else(|e| panic!("{ends:?} {order:?} step {step}: {e}"));
                    let sg = signed("a settlement", &b, &w.keys);
                    txs += 1;
                    check_bond(&sg, &ch, &out, &live.s.key);
                    let next = match out {
                        Settled::Slashed { taken, .. } => {
                            assert_eq!(taken, take_of(live.s.amount, ch.s.clock, open.len() > 1).unwrap());
                            live.s.slash(&ch.s, &cid, &a, &later).unwrap().0
                        }
                        Settled::Cleared { .. } => Some(live.s.clear(&cid, &a, &later).unwrap()),
                    };
                    open.retain(|y| *y != cid);
                    x = next.map(|s| {
                        let at = sg.tx.outputs.iter().position(|o| o.script_public_key == CObj::Exit(s).spk()).expect("the exit's continuation");
                        assert_eq!((sg.tx.outputs[at].value as i64, s.hold), (s.amount, open.iter().fold(NO_HOLD, |h, y| hold_add(&h, y))));
                        assert!(s.amount >= KAS1);
                        nth(&sg, at, s, 7_000 + step as u64)
                    });
                    assert!(x.is_some() || sg.tx.outputs.iter().all(|o| o.covenant.is_none()));
                }
                assert!(open.is_empty());
                match x {
                    None => gone += 1,
                    Some(l) => {
                        if withdrawn < 3 {
                            signed("exit-withdraw of what the charges left", &withdraw(&l, &w.fu(mek), &mek, RATE).expect("the withdraw"), &w.keys);
                        }
                        assert_eq!(l.s.withdraw(l.s.release), Ok(l.s.amount));
                        withdrawn += 1;
                    }
                }
                runs += 1;
            }
        }
    }
    println!("C7 (a) an exit: {runs} orders, {txs} settlements, all accepted; {gone} end with the exit gone, {withdrawn} with a rest its key withdraws");
    assert!(gone > 0 && withdrawn > 0);
}

/// (a) The witness is not a choice between "takes all" and "leaves 1 KAS": `rest` is zero exactly when the charge is
/// the stake's only hold.
#[test]
fn c7_holds_the_witness_cannot_say_whether_other_holds_exist() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let (h1, c1, id1, _) = w.charge_member(&w.committee, 1, &sign_price(&w.members[1], &rid, &MARKET, 6_500, 20 * KAS1), mek, 6_600);
    let (h2, c2, id2, _) = w.charge_member(&h1, 1, &sign_price(&w.members[1], &rid, &MARKET, 6_501, 20 * KAS1), mek, 6_601);
    let (r1, r2) = (ended(&c1, End::Refuted(40)), ended(&c2, End::Refuted(40)));
    let ids = [id1, id2];
    // The only hold of a member: all 22 KAS. Shown with anything before or after it, it is not among the holds.
    let (b, out) = charge_settle(&Staked::Member(&h1, 1), &r1, &NO_HOLD, &NO_LATER, 7_000, &w.fu(mek), &mek, RATE).unwrap();
    signed("control: the only hold takes all", &b, &w.keys);
    assert!(matches!(out, Settled::Slashed { taken, .. } if taken == 22 * KAS1));
    let mut later = NO_LATER;
    later[0] = id2;
    for (what, a, l) in [("something before it", id2, NO_LATER), ("something after it", NO_HOLD, later), ("itself before it", id1, NO_LATER)] {
        let mut t = b.clone();
        set_args(&mut t, 0, |x| {
            x[2] = bv(&a);
            x[3] = bv(&later_bytes(&l));
        });
        refused(&format!("the only hold shown with {what}"), &t, &w.keys, 0);
    }
    // Either of two holds: 21 KAS, whichever goes first; the transaction that takes 22 is refused for both.
    for (ch, cid) in [(&r1, id1), (&r2, id2)] {
        let (a, l) = hold_witness(&ids, &cid).unwrap();
        let (b, out) = charge_settle(&Staked::Member(&h2, 1), ch, &a, &l, 7_000, &w.fu(mek), &mek, RATE).unwrap();
        signed("control: one of two holds leaves 1 KAS", &b, &w.keys);
        assert!(matches!(out, Settled::Slashed { taken, .. } if taken == 21 * KAS1));
        let mut t = b.clone();
        let emptied = Committee { members: { let mut x = h2.s.members; x[1] = EMPTY_TABLE[1]; x }, holds: NO_HOLDS, ..h2.s };
        t.tx.outputs[0].script_public_key = CObj::Committee(emptied).spk();
        t.tx.outputs[0].value -= KAS1 as u64;
        let n = t.tx.outputs.len();
        t.tx.outputs[n - 1].value += KAS1 as u64;
        refused("one of two holds taking all", &t, &w.keys, 0);
        let mut t = b.clone();
        set_args(&mut t, 0, |x| {
            x[2] = bv(&NO_HOLD);
            x[3] = bv(&later_bytes(&NO_LATER));
        });
        refused("one of two holds shown as the only one", &t, &w.keys, 0);
    }
    // A stake at the floor with another hold: nothing is taken, the bond goes back, the hold ends.
    let mut floor = h2.clone();
    floor.s.members[1].weight = KAS1;
    floor.value -= 21 * KAS1 as u64;
    let (a, l) = hold_witness(&ids, &id1).unwrap();
    let (b, out) = charge_settle(&Staked::Member(&floor, 1), &r1, &a, &l, 7_000, &w.fu(mek), &mek, RATE).expect("a slash that takes nothing");
    let sg = signed("a slash of a stake at the floor with another hold", &b, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: KAS1, taken: 0, reward: 0, burned: 0 });
    assert_eq!((sg.tx.outputs[1].value, sg.tx.outputs[1].script_public_key.clone(), sg.tx.outputs[0].value), (c1.value, p2pk_spk(&mek), floor.value));
    let mut t = b.clone();
    t.tx.outputs[1].value -= 1;
    refused("the bond a sompi short", &t, &w.keys, 0);
    let mut t = b.clone();
    t.tx.outputs[0].value -= 1;
    refused("a sompi out of a stake nothing is taken from", &t, &w.keys, 0);
}
