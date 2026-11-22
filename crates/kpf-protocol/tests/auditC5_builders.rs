//! Review C5 (docs/audit/REVIEW-C5.md): the judged slash of the committee family through the tool's builders on the
//! pinned artifacts. Every transaction is built by the library, signed and run through `verify::verify`; an attack
//! is a built transaction whose plan, entries or outputs are edited before it is signed.
//!
//! `c5_exploit_` the engine accepts the attack; `c5_bounded_` an accepted behaviour, measured; `c5_holds_` refused.

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

// ------------------------------------------------------------------------------------------------ findings

/// C5-F1, as it was: three aged stakes of the judge's minimum (20 KAS, returned whole) emptied members of 30, 22 and 15
/// KAS that had signed TRUE signed prices, nobody answering: 6.7 KAS to the author, 60.3 KAS burned, and the empty committee was
/// retaken for 30 KAS. Now a verdict takes no more than its NO side had at risk and pays a tenth of no more than that:
/// the same three contests take 20, 20 and 15 KAS, pay 2, 2 and 1.5, and leave members of 10 and 2 KAS with their keys.
/// What an unanswered contest of the member's own size does is measured in auditC6_builders.rs.
#[test]
fn c5_fixed_an_unanswered_contest_takes_no_more_than_its_no_side_had_at_risk() {
    let w = world();
    let thief = xk(&w.thief);
    let rid = w.rid.as_bytes();
    let mut committee = w.committee.clone();
    let mut charges = vec![];
    for i in 0..3 {
        let signed_price = sign_price(&w.members[i], &rid, &MARKET, 6_500, 10 * KAS1);
        let (r, ch, cid, _) = w.charge_member(&committee, i, &signed_price, thief, 6_600 + i as u64);
        assert_eq!(ch.s.at_stake, committee.s.members[i].weight);
        committee = r;
        charges.push((ch, cid));
    }
    let (mut took, mut burned) = (0, 0);
    for (i, (ch, cid)) in charges.iter().enumerate() {
        let opener = w.stake(&w.thief, 20, 6_000);
        let j = w.judge(ch, &opener, None, 6_650);
        assert_eq!((j.decided.s.status, j.decided.s.clock), (CHARGE_REFUTED, 20 * KAS1), "nobody answered: NO leads, with 20 KAS at risk");
        let (b, out) = charge_settle(&Staked::Member(&committee, i), &j.decided, &NO_HOLD, &NO_LATER, 6_800, &w.fu(thief), &thief, RATE).expect("the slash");
        let sg = signed("the slash of an honest member", &b, &w.keys);
        let weight = committee.s.members[i].weight;
        let s = weight.min(20 * KAS1);
        assert_eq!(out, Settled::Slashed { stake: weight, taken: s, reward: s / 10, burned: s - s / 10 });
        assert_eq!((sg.tx.outputs[1].value as i64, sg.tx.outputs[1].script_public_key.clone()), (5 * KAS1 + s / 10, p2pk_spk(&thief)));
        took += s / 10;
        burned += s - s / 10;
        let (r2, _, _) = committee.s.slash(i, &j.decided.s, cid, &NO_HOLD, &NO_LATER).unwrap();
        committee = nth(&sg, 0, r2, 6_800 + i as u64);
        // One sompi more to the author, or one less burned, is refused.
        let n = b.tx.outputs.len();
        let mut t = b.clone();
        t.tx.outputs[1].value += 1;
        t.tx.outputs[n - 1].value -= 1;
        refused("a sompi of the burn to the author", &t, &w.keys, 0);
        let mut t = b.clone();
        t.tx.outputs[0].value -= 1;
        refused("a sompi more out of the committee", &t, &w.keys, 0);
    }
    assert_eq!((took, burned), (55 * KAS1 / 10, 55 * KAS1 - 55 * KAS1 / 10));
    let left: Vec<i64> = committee.s.members[..3].iter().map(|s| s.weight).collect();
    assert_eq!(left, vec![10 * KAS1, 2 * KAS1, 0]);
    assert_eq!((committee.s.members[0].key, committee.s.members[1].key, committee.s.holds), (xk(&w.members[0]), xk(&w.members[1]), NO_HOLDS));
    assert_eq!(committee.value as i64, 12 * KAS1 + COMMITTEE_CARRIER as i64);
    println!("C5-F1 fixed: three unanswered contests of 20 KAS took 55 of 67 KAS: {} KAS to the author, {} KAS burned; the members keep 10, 2 and 0 KAS", kas(took), kas(burned));
}

/// C5-F2, as it was: a refuted member of 22 KAS forfeited itself before the slash, took 2.2 KAS as the sender, and the
/// charge (in no stake's holds any more) kept its author's bond of 5 KAS for ever. Now a held member shown two prices
/// keeps a tenth under its holds, nobody is paid, and the charge is settled against what is left.
#[test]
fn c5_fixed_a_charged_member_shown_two_prices_keeps_a_tenth_under_its_holds_and_the_charge_is_paid() {
    let w = world();
    let (mek, off) = (xk(&w.me), xk(&w.members[1]));
    let rid = w.rid.as_bytes();
    let signed_price = sign_price(&w.members[1], &rid, &MARKET, 6_500, 20 * KAS1);
    let (held, ch, cid, _) = w.charge_member(&w.committee, 1, &signed_price, mek, 6_600);
    let opener = w.stake(&w.judge1, 20, 6_000);
    let j = w.judge(&ch, &opener, None, 6_650);
    assert_eq!(j.decided.s.status, CHARGE_REFUTED);
    // The member's own proof against itself lands before the slash.
    let (a, b2) = (sign_price(&w.members[1], &rid, &MARKET, 6_700, 10 * KAS1), sign_price(&w.members[1], &rid, &MARKET, 6_700, 11 * KAS1));
    let (fb, r2) = forfeit_member(&held, 1, &MARKET, &a, &b2, &w.fu(off), &off, RATE).expect("a held member is shown two prices");
    let sg = signed("the member forfeits itself", &fb, &w.keys);
    let back: u64 = sg.tx.outputs.iter().filter(|o| o.script_public_key == p2pk_spk(&off)).map(|o| o.value).sum();
    assert_eq!((back + sg.fee) as i64, 1_000 * KAS1, "the sender is paid nothing");
    assert_eq!(sg.tx.outputs.last().unwrap().value as i64, 22 * KAS1 - 22 * KAS1 / 10);
    assert_eq!((r2.members[1].weight, r2.members[1].key, r2.holds[1]), (22 * KAS1 / 10, off, held.s.holds[1]), "a tenth, the key and the holds stay");
    // The sender takes nothing of it: a sompi less burned is refused.
    let n = fb.tx.outputs.len();
    let mut t = fb.clone();
    t.tx.outputs[n - 1].value -= 1;
    refused("a sompi of a held member's forfeit not burned", &t, &w.keys, 0);
    let mut t = fb.clone();
    t.tx.outputs[0].script_public_key = CObj::Committee(held.s.forfeit(1).unwrap().0.charged(1, &[9; 32]).unwrap()).spk();
    refused("a held member's forfeit that rewrites the holds", &t, &w.keys, 0);
    let kept = nth(&sg, 0, r2, 6_790);
    // The charge takes what is left (2.2 KAS, all of it: 20 KAS were at risk) and pays its author a tenth of the lesser
    // of the stake as it was charged (22) and the NO total (20): 2 KAS and the bond; 0.2 KAS burned.
    let (b, out) = charge_settle(&Staked::Member(&kept, 1), &j.decided, &NO_HOLD, &NO_LATER, 6_800, &w.fu(mek), &mek, RATE).expect("the slash of what is left");
    let sg = signed("the slash after the forfeit", &b, &w.keys);
    assert_eq!(out, Settled::Slashed { stake: 22 * KAS1 / 10, taken: 22 * KAS1 / 10, reward: 2 * KAS1, burned: 2 * KAS1 / 10 });
    assert_eq!((sg.tx.outputs[1].value as i64, sg.tx.outputs[1].script_public_key.clone()), (7 * KAS1, p2pk_spk(&mek)));
    let (r3, _, _) = kept.s.slash(1, &j.decided.s, &cid, &NO_HOLD, &NO_LATER).unwrap();
    assert_eq!((r3.members[1].weight, r3.holds), (0, NO_HOLDS));
}

// ------------------------------------------------------------------------------------------------ what holds

/// The id of the charge at output 1 as that output is now (a genesis authored by the funding input 1), and the committee
/// at output 0 holding it for member `i`.
fn rebirth(b: &mut OBuilt, r: &OLive<Committee>, i: usize) {
    let mut o = b.tx.outputs[1].clone();
    o.covenant = None;
    let id = kaspa_consensus_core::hashing::covenant_id::covenant_id(b.tx.inputs[1].previous_outpoint, std::iter::once((1u32, &o)));
    b.tx.outputs[1].covenant = Some(CovenantBinding { authorizing_input: 1, covenant_id: id });
    b.tx.outputs[0].script_public_key = CObj::Committee(r.s.charged(i, &id.as_bytes()).unwrap()).spk();
}

/// (a) A stake is slashed or cleared only by a charge its own `charge` wrote: the bond, the state and the fresh id are
/// the committee's rule at the charge's birth, and the id is what the witness must fold into the member's holds.
#[test]
fn c5_holds_only_a_charge_its_stake_wrote_is_among_its_holds() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let signed_price = sign_price(&w.members[1], &rid, &MARKET, 6_500, 20 * KAS1);
    let (held, ch, cid, cb) = w.charge_member(&w.committee, 1, &signed_price, mek, 6_600);
    let c = ch.s;

    // Its birth. The control: the same transaction with its id recomputed is accepted.
    let mut t = cb.clone();
    rebirth(&mut t, &w.committee, 1);
    signed("control: the charge with its id recomputed", &t, &w.keys);
    let born = |what: &str, edit: &dyn Fn(&mut OBuilt)| {
        let mut t = cb.clone();
        edit(&mut t);
        rebirth(&mut t, &w.committee, 1);
        refused(what, &t, &w.keys, 0);
    };
    born("a charge a sompi below the bond", &|t| t.tx.outputs[1].value -= 1);
    born("a charge born refuted", &|t| t.tx.outputs[1].script_public_key = CObj::Charge(Charge { status: CHARGE_REFUTED, ..c }).spk());
    born("a charge born contested", &|t| t.tx.outputs[1].script_public_key = CObj::Charge(Charge { status: CHARGE_CONTESTED, clock: 1, ..c }).spk());
    born("a charge naming another key", &|t| t.tx.outputs[1].script_public_key = CObj::Charge(Charge { stake_key: xk(&w.members[0]), ..c }).spk());
    born("a charge naming another committee", &|t| t.tx.outputs[1].script_public_key = CObj::Charge(Charge { committee_id: [0x53; 32], ..c }).spk());
    born("a charge of another price than the one signed", &|t| t.tx.outputs[1].script_public_key = CObj::Charge(Charge { price: 21 * KAS1, ..c }).spk());
    born("a charge with another judge", &|t| {
        let p = ChargeParams { judge: JudgeMarket { id: [0x6b; 32], ..w.jm }, ..c.p };
        t.tx.outputs[1].script_public_key = CObj::Charge(Charge { p, ..c }).spk()
    });
    born("a charge that says the member held less", &|t| t.tx.outputs[1].script_public_key = CObj::Charge(Charge { at_stake: KAS1, ..c }).spk());
    born("a charge that says the member held more", &|t| t.tx.outputs[1].script_public_key = CObj::Charge(Charge { at_stake: 1_000 * KAS1, ..c }).spk());
    born("a charge with a shorter wait", &|t| t.tx.outputs[1].script_public_key = CObj::Charge(Charge { p: ChargeParams { daa: 1, ..c.p }, ..c }).spk());
    // k names the committee's own continuation; the charge under the committee's id; a twin of the charge under its id.
    let mut t = cb.clone();
    set_args(&mut t, 0, |a| a[6] = kpf_protocol::tx::iv(0));
    refused("k is the committee's continuation", &t, &w.keys, 0);
    let mut t = cb.clone();
    t.tx.outputs[1].covenant = Some(CovenantBinding { authorizing_input: 0, covenant_id: w.rid });
    t.tx.outputs[0].script_public_key = CObj::Committee(w.committee.s.charged(1, &rid).unwrap()).spk();
    refused("a charge under the committee's own id", &t, &w.keys, 0);
    let mut t = cb.clone();
    let twin = t.tx.outputs[1].clone();
    t.tx.outputs.push(twin);
    t.output_labels.push("twin".into());
    t.entries[1].amount += 5 * KAS1 as u64;
    refused("a twin of the charge under its id", &t, &w.keys, 0);

    // Its end. The charge is refuted; the control is the slash.
    let opener = w.stake(&w.judge1, 20, 6_000);
    let j = w.judge(&ch, &opener, None, 6_650);
    let (slash, _) = charge_settle(&Staked::Member(&held, 1), &j.decided, &NO_HOLD, &NO_LATER, 6_800, &w.fu(mek), &mek, RATE).expect("the slash");
    signed("control: the slash", &slash, &w.keys);
    // The same script and state under an id the committee did not write (a genesis anyone makes), with every witness
    // that names the real id somewhere.
    let forged = Hash::from_bytes([0x66; 32]);
    let d = &j.decided;
    let hold = held.s.holds[1];
    let mut later = NO_LATER;
    later[0] = cid;
    for (what, a, l) in [("no witness", NO_HOLD, NO_LATER), ("the real id after it", NO_HOLD, later), ("the holds before it", hold, NO_LATER), ("the real id before it", cid, NO_LATER), ("the holds before it and the real id after", hold, later)] {
        let mut t = slash.clone();
        swap(&mut t, 1, CObj::Charge(d.s).redeem(), d.value, d.daa, Some(forged));
        set_args(&mut t, 0, |x| {
            x[2] = bv(&a);
            x[3] = bv(&later_bytes(&l));
        });
        refused(&format!("a forged refuted charge, {what}"), &t, &w.keys, 0);
    }
    // The real charge against another member, and with a witness that is not the member's holds.
    let mut t = slash.clone();
    set_args(&mut t, 0, |x| x[1] = b8(0));
    refused("the charge against another member", &t, &w.keys, 0);
    let mut t = slash.clone();
    set_args(&mut t, 0, |x| x[2] = bv(&hold));
    refused("the holds themselves as what came before", &t, &w.keys, 0);
    let mut t = slash.clone();
    set_args(&mut t, 0, |x| x[3] = bv(&later_bytes(&later)));
    refused("the charge's own id once more after it", &t, &w.keys, 0);
}

/// (b) `KpfCharge.settle` names its stake by the committee's id, a script length, a tag and its own index: nothing of
/// the committee's id satisfies it but the stake that holds it running `slash` or `clear` on it, in the status and at the
/// time that entry serves.
#[test]
fn c5_holds_a_charge_leaves_only_with_its_own_stake() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let signed_price = sign_price(&w.members[1], &rid, &MARKET, 6_500, 20 * KAS1);
    let (held, ch, _, _) = w.charge_member(&w.committee, 1, &signed_price, mek, 6_600);
    let j = w.judge(&ch, &w.stake(&w.judge1, 20, 6_000), None, 6_650);
    let (slash, _) = charge_settle(&Staked::Member(&held, 1), &j.decided, &NO_HOLD, &NO_LATER, 6_800, &w.fu(mek), &mek, RATE).expect("the slash");
    let slash_sg = signed("control: the slash", &slash, &w.keys);

    // r names a key input, and itself.
    let mut t = slash.clone();
    set_args(&mut t, 1, |a| a[0] = b8(2));
    refused("settle beside a key input", &t, &w.keys, 1);
    set_args(&mut t, 1, |a| a[0] = b8(1));
    refused("settle naming itself", &t, &w.keys, 1);
    // The committee runs another entry and the charge settles beside it.
    let beside = |what: &str, mut t: OBuilt| {
        signed(&format!("control: {what}"), &t, &w.keys);
        let c = add_input(&mut t, &slash, 1);
        refused(&format!("a charge settled beside the committee's {what}"), &t, &w.keys, c);
    };
    beside("topUp", top_up(&held, 1, KAS1, &w.fu(mek), &mek, RATE).expect("top-up").0);
    beside("charge", charge(&Staked::Member(&held, 1), &MARKET, &sign_price(&w.members[1], &rid, &MARKET, 6_501, 10 * KAS1), mek, &w.fu(mek), &mek, RATE).expect("charge").0);
    beside("leave", leave(&held, 1, &w.fu(mek), &mek, RATE).expect("leave").0);
    let two = (sign_price(&w.members[1], &rid, &MARKET, 6_700, 10 * KAS1), sign_price(&w.members[1], &rid, &MARKET, 6_700, 11 * KAS1));
    beside("forfeit", forfeit_member(&held, 1, &MARKET, &two.0, &two.1, &w.fu(mek), &mek, RATE).expect("forfeit").0);
    beside("roll", roll(&held, 1, &w.fu(mek), &mek, RATE).expect("roll").0);

    // A status the entry does not serve. The transaction is built for a charge of the status the entry wants, and the
    // real charge is put in its place.
    let put = |what: &str, stake: &Staked, real: &OLive<Charge>, as_status: i64, lock: u64, input: usize| {
        let fake = OLive { s: Charge { status: as_status, clock: if as_status == CHARGE_REFUTED { 20 * KAS1 } else { real.s.clock }, ..real.s }, ..real.clone() };
        let (mut t, _) = charge_settle(stake, &fake, &NO_HOLD, &NO_LATER, lock, &w.fu(mek), &mek, RATE).unwrap_or_else(|e| panic!("{what}: {e}"));
        signed(&format!("control: {what}, were it so"), &t, &w.keys);
        swap(&mut t, 1, CObj::Charge(real.s).redeem(), real.value, real.daa, real.id);
        refused(what, &t, &w.keys, input);
    };
    put("a refuted charge cleared as lapsed", &Staked::Member(&held, 1), &j.decided, CHARGE_LAPSED, 6_800, 0);
    put("a refuted charge cleared as upheld", &Staked::Member(&held, 1), &j.decided, CHARGE_UPHELD, 6_800, 0);
    put("a contested charge slashed", &Staked::Member(&held, 1), &j.contested, CHARGE_REFUTED, 6_800, 0);
    put("a contested charge cleared", &Staked::Member(&held, 1), &j.contested, CHARGE_LAPSED, 6_800, 1);
    put("a pending charge slashed", &Staked::Member(&held, 1), &ch, CHARGE_REFUTED, 6_800, 0);
    // Member 0 signed a true signed price; the contest is answered and the charge upheld: not slashed.
    let (held0, ch0, _, _) = w.charge_member(&held, 0, &sign_price(&w.members[0], &rid, &MARKET, 6_500, 10 * KAS1), mek, 6_610);
    let j0 = w.judge(&ch0, &w.stake(&w.judge1, 20, 6_000), Some(&w.stake(&w.judge2, 25, 6_000)), 6_650);
    assert_eq!(j0.decided.s.status, CHARGE_UPHELD);
    put("an upheld charge slashed", &Staked::Member(&held0, 0), &j0.decided, CHARGE_REFUTED, 6_900, 0);
    put("an upheld charge cleared as lapsed (its bond burned)", &Staked::Member(&held0, 0), &j0.decided, CHARGE_LAPSED, 6_900, 0);

    // A pending charge before chargeDaa, on a member and on an exit.
    let (held2, ch2, id2, _) = w.charge_member(&held0, 2, &sign_price(&w.members[2], &rid, &MARKET, 6_500, 10 * KAS1), mek, 6_620);
    let (clear, _) = charge_settle(&Staked::Member(&held2, 2), &ch2, &NO_HOLD, &NO_LATER, 6_920, &w.fu(mek), &mek, RATE).expect("the lapse");
    let clear_sg = signed("control: a pending charge cleared 300 DAA after it landed", &clear, &w.keys);
    assert_eq!(clear_sg.tx.lock_time, 6_920);
    let mut t = clear.clone();
    t.tx.lock_time = 6_919;
    refused("a pending charge cleared a DAA early", &t, &w.keys, 0);
    let (lb, _, e) = leave(&held2, 2, &w.fu(mek), &mek, RATE).expect("a held member leaves");
    let exit = nth(&signed("committee-leave of a held member", &lb, &w.keys), 1, e, 6_700);
    let (xclear, _) = charge_settle(&Staked::Exit(&exit), &ch2, &NO_HOLD, &NO_LATER, 6_920, &w.fu(mek), &mek, RATE).expect("the lapse, of an exit");
    let xclear_sg = signed("control: the same through the exit", &xclear, &w.keys);
    let mut t = xclear.clone();
    t.tx.lock_time = 6_919;
    refused("a pending charge cleared a DAA early, of an exit", &t, &w.keys, 0);
    put("a pending charge slashing an exit", &Staked::Exit(&exit), &ch2, CHARGE_REFUTED, 6_920, 0);
    // Two charges of one member in one transaction: the stake runs one entry, on one of them.
    let (held3, ch3, id3, _) = w.charge_member(&held2, 2, &sign_price(&w.members[2], &rid, &MARKET, 6_501, 10 * KAS1), mek, 6_630);
    let ids = [id2, id3];
    let (a, later) = hold_witness(&ids, &id2).unwrap();
    let (first, _) = charge_settle(&Staked::Member(&held3, 2), &ch2, &a, &later, 6_930, &w.fu(mek), &mek, RATE).expect("the first of two");
    signed("control: the first of two charges cleared", &first, &w.keys);
    let (a, later) = hold_witness(&ids, &id3).unwrap();
    let (second, _) = charge_settle(&Staked::Member(&held3, 2), &ch3, &a, &later, 6_930, &w.fu(mek), &mek, RATE).expect("the second of two");
    let mut t = first.clone();
    let c = add_input(&mut t, &second, 1);
    refused("a second charge settled beside the clear of the first", &t, &w.keys, c);
    set_args(&mut t, 0, |x| x[0] = b8(c as i64));
    refused("the stake naming the second while the first settles too", &t, &w.keys, 0);

    // (g) The pinned KpfCharge was compiled with the lengths and tags of the pinned committee and exit: its script holds
    // them, and they are those of the scripts that just ran.
    let cr = CObj::Charge(ch.s).redeem();
    let has = |n: &[u8]| cr.windows(n.len()).any(|x| x == n);
    let le2 = |v: usize| vec![0x02, v as u8, (v >> 8) as u8];
    let (rl, el) = (CObj::Committee(held.s).redeem().len(), CObj::Exit(exit.s).redeem().len());
    assert_eq!((rl, el), (17_417, 4_639));
    assert!(has(&le2(rl)) || has(&le2(318 + rl)), "the committee's length is in the charge's script");
    assert!(has(&le2(el)) || has(&le2(318 + el)), "the exit's length is in the charge's script");
    for (what, sg) in [("committee slash", &slash_sg), ("committee clear", &clear_sg), ("exit clear", &xclear_sg)] {
        let ss = &sg.tx.inputs[0].signature_script;
        assert_eq!(ss[310], 0x04, "{what}");
        assert!(has(&ss[310..315]), "{what}: its tag is in the charge's script");
        assert_eq!(data_sig_ops(ss), 3, "{what}: three data signatures in the script");
    }
    assert_eq!(slash_sg.tx.inputs[0].signature_script.len(), 318 + rl);
    assert_eq!(xclear_sg.tx.inputs[0].signature_script.len(), 318 + el);
}

/// (c) The judge with a charge as its claim: who may contest, what the verdict needs, and that the pool lives on
/// without the charge and has no place in its settlement.
#[test]
fn c5_holds_the_judge_serves_a_charge_as_it_served_a_claim() {
    let w = world();
    let mek = xk(&w.me);
    let rid = w.rid.as_bytes();
    let stake_redeem = |s: &Js| kpf_protocol::oracle_artifacts::kit().stake.redeem(&s.state_bytes());
    let (held, ch, _, _) = w.charge_member(&w.committee, 1, &sign_price(&w.members[1], &rid, &MARKET, 6_500, 20 * KAS1), mek, 6_600);
    let j = w.judge(&ch, &w.stake(&w.judge1, 20, 6_000), None, 6_650);

    // The contest.
    let other = w.stake_of(&JudgeMarket { id: [0x6b; 32], ..w.jm }, &w.judge1, 20, 6_000);
    let mut t = j.contest.clone();
    swap(&mut t, 1, stake_redeem(&other.s), other.value, other.daa, None);
    refused("a contest by a stake of another judge", &t, &w.keys, 0);
    let mut t = j.contest.clone();
    t.entries[1].block_daa_score = 6_400;
    signed("control: a stake accepted exactly 200 DAA before the charge", &t, &w.keys);
    t.entries[1].block_daa_score = 6_401;
    refused("a contest by a stake a DAA too young", &t, &w.keys, 1);
    let mut t = j.contest.clone();
    t.tx.outputs[0].value -= 1;
    refused("a contest that keeps a sompi of the bond", &t, &w.keys, 0);
    let mut t = j.contest.clone();
    set_args(&mut t, 0, |a| a[1] = b8(3));
    refused("the dispute named at the change", &t, &w.keys, 0);
    let mut t = j.contest.clone();
    swap(&mut t, 0, CObj::Charge(j.contested.s).redeem(), j.contested.value, j.contested.daa, j.contested.id);
    refused("a second contest", &t, &w.keys, 0);
    let mut t = j.contest.clone();
    swap(&mut t, 0, CObj::Charge(j.decided.s).redeem(), j.decided.value, j.decided.daa, j.decided.id);
    refused("a contest of a decided charge", &t, &w.keys, 0);

    // The verdict.
    let mut t = j.decide.clone();
    t.tx.lock_time -= 1;
    refused("the verdict a DAA before the round has closed", &t, &w.keys, 0);
    let mut t = j.decide.clone();
    t.tx.outputs[1].value -= 1;
    refused("a verdict that keeps a sompi of the bond", &t, &w.keys, 1);
    let mut t = j.decide.clone();
    t.tx.outputs[1].script_public_key = CObj::Charge(Charge { status: CHARGE_UPHELD, ..j.decided.s }).spk();
    refused("NO leads and the charge is written upheld", &t, &w.keys, 1);
    assert_eq!(j.decided.s.clock, 20 * KAS1, "the verdict carries what NO had at risk");
    for no in [20 * KAS1 + 1, 40 * KAS1, 20 * KAS1 - 1, 6_600] {
        let mut t = j.decide.clone();
        t.tx.outputs[1].script_public_key = CObj::Charge(Charge { clock: no, ..j.decided.s }).spk();
        refused("the verdict written with another NO total", &t, &w.keys, 1);
    }
    let mut t = j.decide.clone();
    t.tx.outputs[1].script_public_key = CObj::Charge(Charge { pay_key: xk(&w.thief), ..j.decided.s }).spk();
    refused("the verdict rewrites who is paid", &t, &w.keys, 1);
    // The dispute of another charge (of the same committee and judge) delivers nothing here.
    let (_, chb, _, _) = w.charge_member(&held, 0, &sign_price(&w.members[0], &rid, &MARKET, 6_500, 10 * KAS1), mek, 6_610);
    let jb = w.judge(&chb, &w.stake(&w.judge2, 20, 6_000), None, 6_650);
    let mut t = j.decide.clone();
    t.tx.inputs[0] = jb.decide.tx.inputs[0].clone();
    t.entries[0] = jb.decide.entries[0].clone();
    t.plans[0] = jb.decide.plans[0].clone();
    refused("the verdict of another charge's dispute", &t, &w.keys, 1);
    // A charge is decided only with its dispute resolving: `decide` beside the dispute's deposit is refused.
    let (dep, _) = deposit(&j.dispute, YES, &[w.stake(&w.judge2, 25, 6_000)], &w.fu(mek), &mek, RATE, &Trust::Untraced).expect("an answer");
    signed("control: an answer on YES", &dep, &w.keys);
    let mut t = dep.clone();
    let c = add_input(&mut t, &j.decide, 1);
    set_args(&mut t, c, |a| a[0] = b8(0));
    refused("a charge decided beside its dispute's deposit", &t, &w.keys, c);

    // After the verdict the pool pays its receipts and closes while the charge still waits for its stake, and the
    // slash runs after the pool is gone.
    let (ps, pool2) = pool_settle(&j.pool, &[j.receipt.clone()], &w.fu(mek), &mek, RATE).expect("the pool pays");
    let sg = signed("pool-settle", &ps, &w.keys);
    let pc = pool_close(&nth(&sg, 0, pool2, 6_800), &mek, RATE).expect("the pool closes");
    signed("pool-close", &pc, &w.keys);
    let (slash, _) = charge_settle(&Staked::Member(&held, 1), &j.decided, &NO_HOLD, &NO_LATER, 6_900, &w.fu(mek), &mek, RATE).expect("the slash");
    signed("the slash after the pool has closed", &slash, &w.keys);
    // The pool or a receipt beside the charge's settlement: the charge is the one input of its id there.
    let mut t = slash.clone();
    add_input(&mut t, &ps, 0);
    refused("the pool in the transaction that slashes", &t, &w.keys, 1);
    let mut t = slash.clone();
    add_input(&mut t, &ps, 1);
    refused("a receipt in the transaction that slashes", &t, &w.keys, 1);
}

/// (d) The shield and what a hold costs. A charge is taken out of the holds with the ids made after it, at most
/// eight: nine later charges block it until one of them is settled, and each lapses `chargeDaa` after it landed, its
/// bond burned. It delays; it does not release the stake.
#[test]
fn c5_bounded_a_charge_waits_behind_nine_later_ones_until_one_lapses() {
    let w = world();
    let (mek, acc) = (xk(&w.me), xk(&w.thief));
    let rid = w.rid.as_bytes();
    let mut committee = w.committee.clone();
    let (mut ids, mut chs) = (vec![], vec![]);
    for n in 0..10u64 {
        // The first is the real one (20 for a feed of 10); the nine after it are an accomplice's, on true signed prices.
        let signed_price = sign_price(&w.members[1], &rid, &MARKET, 6_500 + n as i64, if n == 0 { 20 } else { 10 } * KAS1);
        let (r, ch, cid, _) = w.charge_member(&committee, 1, &signed_price, if n == 0 { mek } else { acc }, 6_600 + n);
        committee = r;
        ids.push(cid);
        chs.push(ch);
    }
    assert_eq!(committee.s.holds[1], ids.iter().fold(NO_HOLD, |h, x| hold_add(&h, x)));
    let j = w.judge(&chs[0], &w.stake(&w.judge1, 20, 6_000), None, 6_650);
    assert_eq!(j.decided.s.status, CHARGE_REFUTED);
    assert!(hold_witness(&ids, &ids[0]).is_none(), "nine charges were made after it");
    // With eight after it the slash is accepted; the same transaction against the member as it is (nine after) is not.
    let mut eight = committee.clone();
    eight.s.holds[1] = ids[..9].iter().fold(NO_HOLD, |h, x| hold_add(&h, x));
    let (a, later) = hold_witness(&ids[..9], &ids[0]).unwrap();
    let (t9, _) = charge_settle(&Staked::Member(&eight, 1), &j.decided, &a, &later, 6_800, &w.fu(mek), &mek, RATE).expect("eight later charges");
    signed("control: a charge with eight later ones is slashed", &t9, &w.keys);
    let mut t = t9.clone();
    swap(&mut t, 0, CObj::Committee(committee.s).redeem(), committee.value, committee.daa, committee.id);
    refused("a charge with nine later ones", &t, &w.keys, 0);
    // The last of the nine lapses 300 DAA after it landed (at 6,609): anyone ends it, and the slash goes through.
    let (a9, l9) = hold_witness(&ids, &ids[9]).unwrap();
    assert!(charge_settle(&Staked::Member(&committee, 1), &chs[9], &a9, &l9, 6_908, &w.fu(mek), &mek, RATE).is_err());
    let (b, out) = charge_settle(&Staked::Member(&committee, 1), &chs[9], &a9, &l9, 6_909, &w.fu(mek), &mek, RATE).expect("the last lapses");
    let sg = signed("the last of the nine is cleared", &b, &w.keys);
    assert_eq!(out, Settled::Cleared { bond_to_key: false });
    let r2 = nth(&sg, 0, committee.s.clear(1, &ids[9], &a9, &l9).unwrap(), 6_910);
    let (b, out) = charge_settle(&Staked::Member(&r2, 1), &j.decided, &a, &later, 6_920, &w.fu(mek), &mek, RATE).expect("the slash, 309 DAA after the last shield charge");
    signed("the slash behind the shield", &b, &w.keys);
    assert!(matches!(out, Settled::Slashed { stake, .. } if stake == 22 * KAS1));
    // 20 of the 22 KAS are taken (the NO total); the member keeps 2 KAS and the eight other holds, which lapse and are
    // cleared as any other (review C5: the member was emptied and the eight kept their bonds for ever).
    let sg = signed("the slash behind the shield, again", &b, &w.keys);
    let (r3, s, _) = r2.s.slash(1, &j.decided.s, &ids[0], &a, &later).unwrap();
    assert_eq!((s, r3.members[1].weight, r3.holds[1]), (20 * KAS1, 2 * KAS1, ids[1..9].iter().fold(NO_HOLD, |h, x| hold_add(&h, x))));
    let r3 = nth(&sg, 0, r3, 6_930);
    let (a8, l8) = hold_witness(&ids[1..9], &ids[8]).unwrap();
    let (b, _) = charge_settle(&Staked::Member(&r3, 1), &chs[8], &a8, &l8, 6_930, &w.fu(mek), &mek, RATE).expect("a shield charge is cleared after the slash");
    signed("a hold that outlived a partial slash is cleared", &b, &w.keys);
    let p = ChargeParams::testnet();
    let day = 864_000;
    println!("C5 (d) fixture: a hold nobody contests costs {} KAS per {} DAA; the shield 9 bonds = {} KAS per {} DAA of delay", kas(5 * KAS1), 300, kas(45 * KAS1), 300);
    println!("C5 (d) testnet: a hold nobody contests costs {} KAS per {} DAA = {} KAS a day; the shield {} KAS an hour = {} KAS a day", kas(p.bond), p.daa, kas(p.bond * day / p.daa), kas(9 * p.bond), kas(9 * p.bond * day / p.daa));
    println!("C5 (d) testnet: a charge contested at the last DAA and left unanswered holds a stake for chargeDaa + roundDaa = {} DAA ({} h) for one bond: {} KAS a day, with a judge stake of {} KAS (returned) per charge", p.daa + p.judge.p.round, (p.daa + p.judge.p.round) / 36_000, kas(p.bond * day / (p.daa + p.judge.p.round)), kas(p.judge.p.min_stake));
}

/// (e) A held stake through a roll, a top-up and an outbid, and the heaviest scripts of the round through
/// `verify::verify`: the hold follows the stake into its exit, the new holder of the member starts with none, and the
/// exit is slashed for all it holds.
#[test]
fn c5_bounded_a_held_stake_keeps_its_hold_wherever_it_goes_and_the_heaviest_shapes() {
    let mut w = world_of(&[30, 22, 15, 12, 12, 11, 11, 11, 11, 11, 11, 11, 11, 11, 10]);
    w.committee.s.p.epoch_tables = 6;
    let (mek, thief) = (xk(&w.me), xk(&w.thief));
    let rid = w.rid.as_bytes();
    let report = |what: &str, sg: &OSigned| {
        let m = &sg.mass;
        println!("C5 (e) {what}: {} bytes, signature script of input 0 {} bytes, compute mass {}, transient {}, storage {}, fee {} sompi", m.size, sg.tx.inputs[0].signature_script.len(), m.compute, m.transient_normalized, m.storage, sg.fee);
        assert!(m.priority_mass < 500_000, "{what}: {} grams", m.priority_mass);
        m.priority_mass
    };
    let signed_price = sign_price(&w.members[14], &rid, &MARKET, 6_500, 20 * KAS1);
    let (held, ch, cid, cb) = w.charge_member(&w.committee, 14, &signed_price, mek, 6_600);
    report("charge of a member", &signed("committee-charge", &cb, &w.keys));
    // A charge asks the committee for no rest (its input's sequence is 0) and writes a new committee UTXO, whose age starts
    // again for join, topUp and leave: each charge costs its author the bond.
    assert_eq!(cb.tx.inputs[0].sequence, 0);
    let hold = held.s.holds[14];
    // A roll: six epoch tables, the holds as they were, the charged member in the epoch table with its weight.
    let (b, r2, epoch_table) = roll(&held, 1, &w.fu(mek), &mek, RATE).expect("the roll of a committee with a hold");
    let sg = signed("committee-roll", &b, &w.keys);
    report("roll, six epoch tables", &sg);
    assert_eq!((r2.holds, epoch_table.members[14].weight), (held.s.holds, 10 * KAS1));
    let rolled = nth(&sg, 0, r2, 9_000);
    // A top-up: the hold stays and the added stake is under it.
    let (b, r3) = top_up(&rolled, 14, KAS1, &w.fu(mek), &mek, RATE).expect("the top-up of a held member");
    let sg = signed("committee-top-up", &b, &w.keys);
    assert_eq!(r3.holds[14], hold);
    let topped = nth(&sg, 0, r3, 9_100);
    // An outbid: the exit carries the hold and the 11 KAS; the member's new holder has none.
    let (b, r4, x) = join(&topped, 14, thief, 12 * KAS1, &w.fu(thief), &thief, RATE).expect("the outbid of a held member");
    let sg = signed("committee-join over a held member", &b, &w.keys);
    assert_eq!(b.tx.inputs[0].sequence, 20, "a join waits for the committee's rest");
    report("join over a taken member", &sg);
    let x = x.expect("the outbid holder's exit");
    assert_eq!((x.hold, x.amount, r4.holds[14], r4.members[14].key), (hold, 11 * KAS1, NO_HOLD, thief));
    let exit = nth(&sg, 1, x, 9_200);
    assert!(withdraw(&exit, &w.fu(mek), &mek, RATE).is_err(), "a held exit is not withdrawn");
    // The charge is refuted; neither the member's new holder nor the committee is touched: the exit goes, all 11 KAS of it.
    let j = w.judge(&ch, &w.stake(&w.judge1, 20, 6_000), None, 6_650);
    report("contest", &signed("charge-contest", &j.contest, &w.keys));
    report("verdict", &signed("charge-decide", &j.decide, &w.keys));
    let after = nth(&sg, 0, r4, 9_200);
    assert!(charge_settle(&Staked::Member(&after, 14), &j.decided, &NO_HOLD, &NO_LATER, 9_300, &w.fu(mek), &mek, RATE).is_err(), "the new holder is not slashed for the old one's signed price");
    let (b, out) = charge_settle(&Staked::Exit(&exit), &j.decided, &NO_HOLD, &NO_LATER, 9_300, &w.fu(mek), &mek, RATE).expect("the slash of the exit");
    let sg = signed("slash of an exit", &b, &w.keys);
    report("slash of an exit", &sg);
    // All 11 KAS are taken (20 were at risk); the author's tenth is of the 10 KAS the member held when it was charged.
    assert_eq!(out, Settled::Slashed { stake: 11 * KAS1, taken: 11 * KAS1, reward: KAS1, burned: 10 * KAS1 });
    // The engine, for the new holder: the slash built for the old committee against the committee as it is.
    let (mut t, _) = charge_settle(&Staked::Member(&topped, 14), &j.decided, &NO_HOLD, &NO_LATER, 9_300, &w.fu(mek), &mek, RATE).expect("the slash, had the member not been outbid");
    report("slash of a member", &signed("control: the slash of the member before the outbid", &t, &w.keys));
    swap(&mut t, 0, CObj::Committee(after.s).redeem(), after.value, after.daa, after.id);
    t.entries[2].amount += 11 * KAS1 as u64;
    refused("the old holder's charge against the member's new holder", &t, &w.keys, 0);
    // The heaviest signature script: a held member leaves (the committee's script and the exit's template).
    let (b, _, e) = leave(&held, 14, &w.fu(mek), &mek, RATE).expect("a held member leaves");
    let sg = signed("committee-leave of a held member", &b, &w.keys);
    let heaviest = report("leave of a held member", &sg);
    assert_eq!(e.hold, hold);
    assert!(sg.tx.inputs[0].signature_script.len() > 19_000);
    assert_eq!(data_sig_ops(&sg.tx.inputs[0].signature_script), 3);
    println!("C5 (e) the heaviest of these: {heaviest} grams of a block's 500,000 (a standard transaction: 100,000)");
    let _ = cid;
}
