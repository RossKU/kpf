//! Review C6 (docs/audit/REVIEW-C6.md): the judged slash after the fixes of review C5 (a verdict takes no more than
//! its NO side had at risk; a held stake shown two prices keeps a tenth under its holds), through the tool's builders
//! on the pinned artifacts. The harness is that of auditC5_builders.rs.
//!
//! `c6_exploit_` the engine accepts the attack; `c6_bounded_` an accepted behaviour, measured; `c6_holds_` refused.

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

// ------------------------------------------------------------------------------------------------ review C6

/// An exit is charged for `signed price`; the charge lands at `at`.
fn charge_exit(w: &World, x: &OLive<Exit>, signed_price: &SignedPrice, pay: [u8; 32], at: u64) -> (OLive<Exit>, OLive<Charge>, [u8; 32]) {
    let (b, c, cid) = charge(&Staked::Exit(x), &MARKET, signed_price, pay, &w.fu(pay), &pay, RATE).expect("the charge of an exit");
    let sg = signed("exit-charge", &b, &w.keys);
    (nth(&sg, 0, x.s.charged(&cid.as_bytes()).unwrap(), at), nth(&sg, 1, c, at), cid.as_bytes())
}

/// (a) C5-F1 after the fix, in numbers. A verdict takes min(stake, NO) and pays a tenth of that: to empty members of
/// 30, 22 and 15 KAS that signed TRUE signed prices the author needs aged judge stakes of 30, 22 and 20 KAS (the judge's least
/// stake for the last; a sompi over 29 and 21 KAS would do: less than 1 KAS left is taken too), all returned whole when nobody answers. It
/// gains 6.7 KAS, 60.3 KAS are burned, and the empty committee is retaken for its least stake as before.
#[test]
fn c6_bounded_a_member_is_emptied_by_silence_only_with_its_weight_in_aged_stake() {
    let w = world();
    let thief = xk(&w.thief);
    let rid = w.rid.as_bytes();
    let mut committee = w.committee.clone();
    let (mut took, mut burned, mut capital) = (0, 0, 0);
    for i in 0..3 {
        let weight = committee.s.members[i].weight;
        let signed_price = sign_price(&w.members[i], &rid, &MARKET, 6_500, 10 * KAS1);
        let (r, ch, cid, _) = w.charge_member(&committee, i, &signed_price, thief, 6_600);
        // The member's weight on NO, and no less than the judge's least stake (20 KAS, for the member of 15).
        let no = weight.max(20 * KAS1);
        let j = w.judge(&ch, &w.stake(&w.thief, no / KAS1, 6_000), None, 6_650);
        assert_eq!((j.decided.s.status, j.decided.s.clock), (CHARGE_REFUTED, no));
        let (b, out) = charge_settle(&Staked::Member(&r, i), &j.decided, &NO_HOLD, &NO_LATER, 6_800, &w.fu(thief), &thief, RATE).expect("the slash");
        let sg = signed("the slash of an honest member", &b, &w.keys);
        assert_eq!(out, Settled::Slashed { stake: weight, taken: weight, reward: weight / 10, burned: weight - weight / 10 });
        let (r2, _, _) = r.s.slash(i, &j.decided.s, &cid, &NO_HOLD, &NO_LATER).unwrap();
        committee = nth(&sg, 0, r2, 6_800);
        took += weight / 10;
        burned += weight - weight / 10;
        capital += no;
        // The opener's stake comes back whole.
        let (b, _) = pool_settle(&j.pool, &[j.receipt.clone()], &w.fu(thief), &thief, RATE).expect("the pool pays the winner");
        assert_eq!(signed("pool-settle", &b, &w.keys).tx.outputs[1].value as i64, no);
    }
    assert_eq!((took, burned, capital), (67 * KAS1 / 10, 67 * KAS1 - 67 * KAS1 / 10, 72 * KAS1));
    assert!(committee.s.members.iter().all(|s| s.weight == 0) && committee.value == COMMITTEE_CARRIER);
    for i in 0..3 {
        let (b, r2, _) = join(&committee, i, thief, 10 * KAS1, &w.fu(thief), &thief, RATE).expect("a join into an empty member");
        committee = nth(&signed("committee-join", &b, &w.keys), 0, r2, 7_000 + 100 * i as u64);
    }
    let (b, _, epoch_table) = roll(&committee, 1, &w.fu(thief), &thief, RATE).expect("the roll");
    signed("committee-roll", &b, &w.keys);
    assert!(epoch_table.total == 30 * KAS1 && epoch_table.members.iter().all(|s| s.weight == 0 || s.key == thief));
    // The floor: 29 KAS on NO leave a member of 30 exactly 1 KAS; a sompi more takes it all.
    assert_eq!((take_of(30 * KAS1, 29 * KAS1, false), take_of(30 * KAS1, 29 * KAS1 + 1, false), take_of(30 * KAS1, 20 * KAS1, false)), (Ok(29 * KAS1), Ok(30 * KAS1), Ok(20 * KAS1)));
    assert!(take_of(30 * KAS1, 0, false).is_err() && take_of(30 * KAS1, -1, false).is_err());
    // With another hold on the stake the last KAS is kept for it (review C6, C6-F1).
    assert_eq!((take_of(30 * KAS1, 40 * KAS1, true), take_of(30 * KAS1, 29 * KAS1 + 1, true), take_of(30 * KAS1, 20 * KAS1, true), take_of(KAS1, 20 * KAS1, true), take_of(KAS1, 20 * KAS1, false)), (Ok(29 * KAS1), Ok(29 * KAS1), Ok(20 * KAS1), Ok(0), Ok(KAS1)));
    // The bet: N at risk and the bond against N / 10. It pays when contests go unanswered more often than
    // (N + bond) / (1.1 N + bond).
    let p = ChargeParams::testnet();
    for member in [100i64, 5_000] {
        let n = member * KAS1;
        println!("C6 (a) testnet: a member of {member} KAS is emptied by one stake of {member} KAS aged {} DAA, returned whole; the author gains {} KAS; answered (by {} KAS on YES) it loses {} KAS; the bet pays above {:.1} % of silence", p.judge.p.age, kas(n / 10), kas(n / 10_000 * p.judge.p.mu_bps), kas(n + p.bond), 100.0 * (n + p.bond) as f64 / (n + n / 10 + p.bond) as f64);
    }
    println!("C6 (a) fixture: 67 KAS of honest stake gone for 72 KAS of aged judge stake (returned): {} KAS to the author, {} KAS burned; the next epoch table is the author's for 30 KAS", kas(took), kas(burned));
}

/// A member left with 1 KAS by a slash (below the committee's least member of 10 KAS) is not stuck: it leaves, and its exit of
/// 1 KAS is withdrawn.
#[test]
fn c6_holds_a_member_left_with_one_kas_leaves_and_is_withdrawn() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let (r, ch, cid, _) = w.charge_member(&w.committee, 0, &sign_price(&w.members[0], &rid, &MARKET, 6_500, 20 * KAS1), mek, 6_600);
    let j = w.judge(&ch, &w.stake(&w.judge1, 29, 6_000), None, 6_650);
    let (b, out) = charge_settle(&Staked::Member(&r, 0), &j.decided, &NO_HOLD, &NO_LATER, 6_800, &w.fu(mek), &mek, RATE).expect("the slash");
    let sg = signed("a slash that leaves 1 KAS", &b, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: 30 * KAS1, taken: 29 * KAS1, reward: 29 * KAS1 / 10, burned: 29 * KAS1 - 29 * KAS1 / 10 });
    let (r2, _, _) = r.s.slash(0, &j.decided.s, &cid, &NO_HOLD, &NO_LATER).unwrap();
    assert_eq!((r2.members[0].weight, r2.members[0].key, r2.holds[0]), (KAS1, xk(&w.members[0]), NO_HOLD));
    let small = nth(&sg, 0, r2, 6_800);
    // One sompi more taken, or the member emptied, is refused.
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = CObj::Committee(Committee { members: w.committee.s.members, holds: NO_HOLDS, ..r2 }.forfeit(0).unwrap().0).spk();
    refused("a slash of 29 KAS that empties a member of 30", &t, &w.keys, 0);
    let (b, _, e) = leave(&small, 0, &w.fu(mek), &mek, RATE).expect("a member of 1 KAS leaves");
    let sg = signed("committee-leave of a member of 1 KAS", &b, &w.keys);
    assert_eq!((e.amount, sg.tx.outputs[1].value as i64), (KAS1, KAS1));
    println!("C6 (b) the leave of a member of 1 KAS: storage mass {}, compute {}", sg.mass.storage, sg.mass.compute);
    let b = withdraw(&nth(&sg, 1, e, 6_900), &w.fu(mek), &mek, RATE).expect("the withdraw of 1 KAS");
    signed("exit-withdraw of 1 KAS", &b, &w.keys);
    // A top-up of 1 KAS is accepted on it, and a roll counts it as it is.
    signed("committee-top-up of a member of 1 KAS", &top_up(&small, 0, KAS1, &w.fu(mek), &mek, RATE).expect("the top-up").0, &w.keys);
    let (b, _, epoch_table) = roll(&small, 1, &w.fu(mek), &mek, RATE).expect("the roll");
    signed("committee-roll", &b, &w.keys);
    assert_eq!((epoch_table.members[0].weight, epoch_table.total), (KAS1, 38 * KAS1));
}

/// C6-F1, as it was: a slash that took a member whole zeroed its holds. Member 1 (22 KAS) charged itself for the time its
/// accuser had charged, contested with its own 22 KAS, slashed itself first (all 22 KAS: 2.2 KAS and its bond to
/// itself), and the accuser's charge, in no stake's holds any more, kept its bond of 5 KAS for ever. Now a slash
/// leaves 1 KAS while the stake has another hold: the member's own verdict takes 21 KAS, and the accuser's charge takes
/// the last KAS and gets its bond back.
#[test]
fn c6_fixed_a_member_slashed_by_its_own_charge_keeps_one_kas_for_its_accusers_charge() {
    let w = world();
    let (mek, own) = (xk(&w.me), xk(&w.thief));
    let rid = w.rid.as_bytes();
    let signed_price = sign_price(&w.members[1], &rid, &MARKET, 6_500, 20 * KAS1);
    let (h1, cha, ida, _) = w.charge_member(&w.committee, 1, &signed_price, mek, 6_600);
    let (h2, chb, idb, _) = w.charge_member(&h1, 1, &signed_price, own, 6_601);
    let ja = w.judge(&cha, &w.stake(&w.judge1, 20, 6_000), None, 6_650);
    let jb = w.judge(&chb, &w.stake(&w.thief, 22, 6_000), None, 6_650);
    assert_eq!((ja.decided.s.status, jb.decided.s.status), (CHARGE_REFUTED, CHARGE_REFUTED));
    let ids = [ida, idb];
    let (wb, lb) = hold_witness(&ids, &idb).unwrap();
    // The member's own slash lands first: 21 of the 22 KAS (its tenth is still of 22: 2.2 KAS), 18.8 burned.
    let (sb, out) = charge_settle(&Staked::Member(&h2, 1), &jb.decided, &wb, &lb, 6_800, &w.fu(own), &own, RATE).expect("the member's own slash");
    let sg = signed("the member slashes itself", &sb, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: 22 * KAS1, taken: 21 * KAS1, reward: 22 * KAS1 / 10, burned: 21 * KAS1 - 22 * KAS1 / 10 });
    let (r3, _, _) = h2.s.slash(1, &jb.decided.s, &idb, &wb, &lb).unwrap();
    assert_eq!((r3.members[1].weight, r3.members[1].key, r3.holds[1]), (KAS1, xk(&w.members[1]), hold_add(&NO_HOLD, &ida)), "1 KAS, the key and the accuser's hold stay");
    // The step that used to lock the bond: the same slash emptying the member is refused.
    let mut t = sb.clone();
    let emptied = Committee { members: { let mut x = h2.s.members; x[1] = EMPTY_TABLE[1]; x }, holds: NO_HOLDS, ..h2.s };
    t.tx.outputs[0].script_public_key = CObj::Committee(emptied).spk();
    t.tx.outputs[0].value -= KAS1 as u64;
    let n = t.tx.outputs.len();
    t.tx.outputs[n - 1].value += KAS1 as u64;
    refused("a slash that empties a member with another hold", &t, &w.keys, 0);
    // The witness is not a choice: the member's own charge shown as its only hold is not among its holds.
    let mut t = sb.clone();
    set_args(&mut t, 0, |x| {
        x[2] = bv(&NO_HOLD);
        x[3] = bv(&later_bytes(&NO_LATER));
    });
    refused("the last of two holds shown as the only one", &t, &w.keys, 0);
    // The accuser: the last KAS (its tenth of 2 KAS is no more than what is taken) and its bond.
    let left = nth(&sg, 0, r3, 6_790);
    let (sa, out) = charge_settle(&Staked::Member(&left, 1), &ja.decided, &NO_HOLD, &NO_LATER, 6_800, &w.fu(mek), &mek, RATE).expect("the accuser's slash");
    let sg = signed("the accuser's slash of what was kept for it", &sa, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: KAS1, taken: KAS1, reward: KAS1, burned: 0 });
    assert_eq!((sg.tx.outputs[1].value as i64, sg.tx.outputs[1].script_public_key.clone()), (6 * KAS1, p2pk_spk(&mek)));
    let (r4, _, _) = left.s.slash(1, &ja.decided.s, &ida, &NO_HOLD, &NO_LATER).unwrap();
    assert_eq!((r4.members[1].weight, r4.holds), (0, NO_HOLDS));
    println!("C6-F1 fixed: the member lost 22 KAS and took 2.2 back; its accuser got its bond and 1 KAS (2 KAS had it settled first)");
}

/// C6-F2. The proof against a held member is the same two signatures every time: replayed, each forfeit burns nine
/// tenths of what the last left, down to 1 KAS. "A held member keeps a tenth" holds for one forfeit; after two, a member
/// of 22 KAS holds 1 KAS, and the charge's author, whose tenth was 2 KAS, is paid the 1 KAS that is left.
#[test]
fn c6_bounded_a_held_forfeit_replays_down_to_one_kas_and_the_author_is_paid_what_is_left() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let (held, ch, _, _) = w.charge_member(&w.committee, 1, &sign_price(&w.members[1], &rid, &MARKET, 6_500, 20 * KAS1), mek, 6_600);
    let j = w.judge(&ch, &w.stake(&w.judge1, 20, 6_000), None, 6_650);
    let (a, b2) = (sign_price(&w.members[1], &rid, &MARKET, 6_700, 10 * KAS1), sign_price(&w.members[1], &rid, &MARKET, 6_700, 11 * KAS1));
    let mut r = held.clone();
    let (mut left, mut last) = (vec![], None);
    for n in 0..4u64 {
        let Ok((fb, r2)) = forfeit_member(&r, 1, &MARKET, &a, &b2, &w.fu(mek), &mek, RATE) else { break };
        let sg = signed("the same proof again", &fb, &w.keys);
        left.push(r2.members[1].weight);
        assert_eq!(r2.holds[1], held.s.holds[1]);
        r = nth(&sg, 0, r2, 6_790 + n);
        last = Some(fb);
    }
    assert_eq!(left, vec![22 * KAS1 / 10, KAS1], "22 KAS, then 2.2, then 1, and no further");
    // The engine, for the third: the second forfeit against the member of 1 KAS.
    let mut t = last.unwrap();
    swap(&mut t, 0, CObj::Committee(r.s).redeem(), r.value, r.daa, r.id);
    t.entries[1].amount += 2 * KAS1 as u64;
    refused("a held member of 1 KAS shown two prices", &t, &w.keys, 0);
    // The author: all that is left, and nothing burned.
    let (b, out) = charge_settle(&Staked::Member(&r, 1), &j.decided, &NO_HOLD, &NO_LATER, 6_800, &w.fu(mek), &mek, RATE).expect("the slash of what is left");
    let sg = signed("the slash after two forfeits", &b, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: KAS1, taken: KAS1, reward: KAS1, burned: 0 });
    assert_eq!((sg.tx.outputs[1].value as i64, sg.tx.outputs[1].script_public_key.clone()), (6 * KAS1, p2pk_spk(&mek)));
    // A charge made after a forfeit measures its tenth by what was left.
    let (_, later_charge, _, _) = w.charge_member(&r, 1, &sign_price(&w.members[1], &rid, &MARKET, 6_501, 20 * KAS1), mek, 6_800);
    assert_eq!(later_charge.s.at_stake, KAS1);
    // What a held stake keeps: a tenth, at least 1 KAS, and less than it had.
    assert_eq!((kept_of(100 * KAS1), kept_of(5 * KAS1), kept_of(KAS1 + 1)), (Ok(10 * KAS1), Ok(KAS1), Ok(KAS1)));
    assert!(kept_of(KAS1).is_err() && kept_of(KAS1 / 2).is_err());
    // The author's tenth: of the lesser of the stake as charged and the NO total, and no more than what is taken.
    assert_eq!((reward_of(20 * KAS1, 22 * KAS1, 20 * KAS1), reward_of(KAS1, 22 * KAS1, 20 * KAS1), reward_of(22 * KAS1, 22 * KAS1, 500 * KAS1)), (Ok(2 * KAS1), Ok(KAS1), Ok(22 * KAS1 / 10)));
    println!("C6-F2: two forfeits with one proof left 1 KAS of 22; the author's tenth fell from 2 KAS to 1 (on a member of 5,000 KAS: from 500 KAS to 1)");
}

/// C6-F3. The NO total is the opener's one UTXO unless YES answers: while NO leads nobody can add to it. A member that
/// signed a false price and is charged contests its own charge first with the judge's least stake: the verdict is
/// the true one, NO, and takes 20 of its 30 KAS. The author must charge again for what is left.
#[test]
fn c6_bounded_whoever_contests_first_sets_what_the_verdict_takes() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let signed_price = sign_price(&w.members[0], &rid, &MARKET, 6_500, 20 * KAS1);
    let (held, ch, cid, _) = w.charge_member(&w.committee, 0, &signed_price, mek, 6_600);
    // The member's own stake of 20 KAS contests; the author's 40 KAS come a block later.
    let j = w.judge(&ch, &w.stake(&w.thief, 20, 6_000), None, 6_650);
    let big = w.stake(&w.judge1, 40, 6_000);
    assert!(deposit(&j.dispute, NO, &[big.clone()], &w.fu(mek), &mek, RATE, &Trust::Untraced).is_err(), "NO leads: nothing joins it");
    let (mut t, _) = deposit(&j.dispute, YES, &[big.clone()], &w.fu(mek), &mek, RATE, &Trust::Untraced).expect("only YES can be joined");
    signed("control: the same stake on YES", &t, &w.keys);
    set_args(&mut t, 0, |a| a[0] = b8(NO));
    t.tx.outputs[1].script_public_key = p2sh_spk(&kpf_protocol::oracle_artifacts::kit().stake.redeem(&Js::receipt(big.s.owner, w.jm, NO, big.value as i64, 0, 6_600).state_bytes()));
    refused("a deposit on the leading side", &t, &w.keys, 0);
    assert_eq!((j.decided.s.status, j.decided.s.clock), (CHARGE_REFUTED, 20 * KAS1));
    let (b, out) = charge_settle(&Staked::Member(&held, 0), &j.decided, &NO_HOLD, &NO_LATER, 6_800, &w.fu(mek), &mek, RATE).expect("the slash");
    let sg = signed("the slash", &b, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: 30 * KAS1, taken: 20 * KAS1, reward: 2 * KAS1, burned: 18 * KAS1 });
    let (r2, _, _) = held.s.slash(0, &j.decided.s, &cid, &NO_HOLD, &NO_LATER).unwrap();
    let after = nth(&sg, 0, r2, 6_800);
    // Nothing forbids a second charge for the same time: the author goes again, for the 10 KAS that are left.
    let (_, again, _, _) = w.charge_member(&after, 0, &signed_price, mek, 6_900);
    assert_eq!(again.s.at_stake, 10 * KAS1);
    let p = ChargeParams::testnet();
    println!("C6-F3 testnet: a member of 5,000 KAS that contests each charge first with {} KAS loses {} KAS a charge: {} charges, a round of {} DAA each (they may run at once: {} aged stakes of the member's own), to take it all", kas(p.judge.p.min_stake), kas(p.judge.p.min_stake), 5_000 * KAS1 / p.judge.p.min_stake, p.judge.p.round, 5_000 * KAS1 / p.judge.p.min_stake);
}

/// (b) An exit slashed in part continues under the committee's id with what is left, its release and its other holds;
/// the second charge takes the rest and nothing of the id is left.
#[test]
fn c6_holds_an_exit_slashed_in_part_continues_with_what_is_left_and_its_other_holds() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let (lb, _, e) = leave(&w.committee, 0, &w.fu(mek), &mek, RATE).expect("the leave");
    let exit = nth(&signed("committee-leave", &lb, &w.keys), 1, e, 6_550);
    let (x1, cha, ida) = charge_exit(&w, &exit, &sign_price(&w.members[0], &rid, &MARKET, 6_500, 20 * KAS1), mek, 6_600);
    let (x2, chb, idb) = charge_exit(&w, &x1, &sign_price(&w.members[0], &rid, &MARKET, 6_501, 20 * KAS1), mek, 6_601);
    assert_eq!((cha.s.at_stake, chb.s.at_stake), (30 * KAS1, 30 * KAS1));
    let ja = w.judge(&cha, &w.stake(&w.judge1, 20, 6_000), None, 6_650);
    let jb = w.judge(&chb, &w.stake(&w.judge2, 20, 6_000), None, 6_650);
    let ids = [ida, idb];
    let (a, later) = hold_witness(&ids, &ida).unwrap();
    let (b, out) = charge_settle(&Staked::Exit(&x2), &ja.decided, &a, &later, 6_800, &w.fu(mek), &mek, RATE).expect("the first slash");
    let sg = signed("the slash of an exit in part", &b, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: 30 * KAS1, taken: 20 * KAS1, reward: 2 * KAS1, burned: 18 * KAS1 });
    let (rest, _, _) = x2.s.slash(&ja.decided.s, &ida, &a, &later).unwrap();
    let rest = rest.expect("the exit continues");
    assert_eq!((rest.amount, rest.hold, rest.release, rest.key), (10 * KAS1, hold_add(&NO_HOLD, &idb), exit.s.release, exit.s.key));
    let at = sg.tx.outputs.iter().position(|o| o.script_public_key == CObj::Exit(rest).spk()).expect("the exit's continuation");
    assert_eq!((sg.tx.outputs[at].value as i64, sg.tx.outputs[at].covenant.map(|c| c.covenant_id)), (10 * KAS1, Some(w.rid)));
    let edit = |what: &str, f: &dyn Fn(&mut OBuilt)| {
        let mut t = b.clone();
        f(&mut t);
        refused(what, &t, &w.keys, 0);
    };
    edit("the exit keeps a sompi less", &|t| t.tx.outputs[at].value -= 1);
    edit("the exit continues without its other hold", &|t| t.tx.outputs[at].script_public_key = CObj::Exit(Exit { hold: NO_HOLD, ..rest }).spk());
    edit("the exit continues with the hold it was just rid of", &|t| t.tx.outputs[at].script_public_key = CObj::Exit(Exit { hold: x2.s.hold, ..rest }).spk());
    edit("the exit continues with a sompi more of amount", &|t| t.tx.outputs[at].script_public_key = CObj::Exit(Exit { amount: rest.amount + 1, ..rest }).spk());
    edit("the exit continues with a sompi less of amount", &|t| t.tx.outputs[at].script_public_key = CObj::Exit(Exit { amount: rest.amount - 1, ..rest }).spk());
    edit("the exit continues released", &|t| t.tx.outputs[at].script_public_key = CObj::Exit(Exit { release: 0, ..rest }).spk());
    edit("the exit continues with another key", &|t| t.tx.outputs[at].script_public_key = CObj::Exit(Exit { key: mek, ..rest }).spk());
    edit("the author a sompi more", &|t| {
        let n = t.tx.outputs.len();
        t.tx.outputs[n - 1].value -= 1;
        t.tx.outputs[1].value += 1;
    });
    let x3 = nth(&sg, at, rest, 6_810);
    assert!(withdraw(&x3, &w.fu(mek), &mek, RATE).is_err(), "still held");
    // The second charge: the 10 KAS that are left, 2 KAS to the author (a tenth of the lesser of 30 and 20), 8 burned.
    let (b, out) = charge_settle(&Staked::Exit(&x3), &jb.decided, &NO_HOLD, &NO_LATER, 6_900, &w.fu(mek), &mek, RATE).expect("the second slash");
    let sg = signed("the slash of what is left of an exit", &b, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: 10 * KAS1, taken: 10 * KAS1, reward: 2 * KAS1, burned: 8 * KAS1 });
    assert!(sg.tx.outputs.iter().all(|o| o.covenant.is_none()));
    // Had the second hold ended otherwise, what is left is withdrawn at the release.
    let free = OLive { s: Exit { hold: NO_HOLD, ..rest }, ..x3.clone() };
    let b = withdraw(&free, &w.fu(mek), &mek, RATE).expect("the withdraw of what a slash left");
    let sg = signed("exit-withdraw", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value as i64, sg.tx.lock_time as i64), (10 * KAS1, rest.release));
    // A held exit shown two prices keeps a tenth under its hold, as a member does.
    let (p1, p2) = (sign_price(&w.members[0], &rid, &MARKET, 6_700, 10 * KAS1), sign_price(&w.members[0], &rid, &MARKET, 6_700, 11 * KAS1));
    let fb = forfeit_exit(&x2, &MARKET, &p1, &p2, &w.fu(mek), &mek, RATE).expect("a held exit is shown two prices");
    let sg = signed("exit-forfeit of a held exit", &fb, &w.keys);
    let kept = x2.s.forfeit_rest().unwrap().expect("a held exit continues");
    assert_eq!((kept.amount, kept.hold), (3 * KAS1, x2.s.hold));
    assert!(sg.tx.outputs.iter().any(|o| o.script_public_key == CObj::Exit(kept).spk() && o.value as i64 == 3 * KAS1));
    assert_eq!(sg.tx.outputs.last().unwrap().value as i64, 27 * KAS1);
}

/// (e) What review C5 left to reading.
#[test]
fn c6_holds_what_review_c5_left_to_reading() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let (h1, c1, _, _) = w.charge_member(&w.committee, 2, &sign_price(&w.members[2], &rid, &MARKET, 6_500, 10 * KAS1), mek, 6_620);
    let (h2, _, id2, _) = w.charge_member(&h1, 2, &sign_price(&w.members[2], &rid, &MARKET, 6_501, 10 * KAS1), mek, 6_630);
    // `fold` skips zero ids wherever they stand: the first of two charges, its later id in the last place.
    let mut gap = NO_LATER;
    gap[7] = id2;
    let (b, _) = charge_settle(&Staked::Member(&h2, 2), &c1, &NO_HOLD, &gap, 6_930, &w.fu(mek), &mek, RATE).expect("a witness with gaps");
    signed("a witness whose later id stands last", &b, &w.keys);
    // A zero id in the charge's own place is no charge: the stake naming a key input as its charge.
    let (clear, _) = charge_settle(&Staked::Member(&h1, 2), &c1, &NO_HOLD, &NO_LATER, 6_920, &w.fu(mek), &mek, RATE).expect("the lapse");
    signed("control: the clear", &clear, &w.keys);
    let mut t = clear.clone();
    set_args(&mut t, 0, |a| a[0] = b8(2));
    refused("the stake naming a key input as its charge", &t, &w.keys, 0);
    // A pending charge contested in the transaction that clears it: the committee wants the bond burned at the output
    // the charge wants its continuation at.
    let (mut t, _, _) = charge_contest(&c1, &w.stake(&w.judge1, 20, 6_000), 10_000_000, &w.fu(mek), &mek, RATE).expect("a contest");
    signed("control: the contest", &t, &w.keys);
    let r = add_input(&mut t, &clear, 0);
    set_args(&mut t, r, |a| a[0] = b8(0));
    let mut ro = clear.tx.outputs[0].clone();
    ro.covenant = Some(CovenantBinding { authorizing_input: r as u16, covenant_id: w.rid });
    t.tx.outputs.push(ro);
    t.output_labels.push("committee".into());
    t.tx.lock_time = 6_920;
    refused("a charge contested in the transaction that clears it", &t, &w.keys, r);
    // Two stakes of one committee id in one transaction: the committee clears while an exit of its id is withdrawn; and
    // while an exit of its id is charged (one output could then be two stakes' charge).
    let (lb, _, e) = leave(&w.committee, 0, &w.fu(mek), &mek, RATE).expect("the leave");
    let exit = nth(&signed("committee-leave", &lb, &w.keys), 1, e, 6_550);
    let wd = withdraw(&exit, &w.fu(mek), &mek, RATE).expect("the withdraw");
    signed("control: the withdraw", &wd, &w.keys);
    let mut t = clear.clone();
    let x = add_input(&mut t, &wd, 0);
    t.tx.lock_time = wd.tx.lock_time;
    refused("a committee and an exit of one id in one transaction (the committee)", &t, &w.keys, 0);
    refused("a committee and an exit of one id in one transaction (the exit)", &t, &w.keys, x);
    let (xc, _, _) = charge(&Staked::Exit(&exit), &MARKET, &sign_price(&w.members[0], &rid, &MARKET, 6_500, 10 * KAS1), mek, &w.fu(mek), &mek, RATE).expect("the charge of an exit");
    let (mut t, _, _) = charge(&Staked::Member(&w.committee, 0), &MARKET, &sign_price(&w.members[0], &rid, &MARKET, 6_500, 10 * KAS1), mek, &w.fu(mek), &mek, RATE).expect("the charge of the member");
    let x = add_input(&mut t, &xc, 0);
    refused("a member and an exit charged in one transaction (the committee)", &t, &w.keys, 0);
    refused("a member and an exit charged in one transaction (the exit)", &t, &w.keys, x);
}
