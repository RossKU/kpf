//! Review T3 (docs/audit/REVIEW-T3-legtime.md): a covered account's leg says the time it entered, and under a call
//! the young legs of an account ask nothing (docs/ARCHITECTURE-CLEARING.md 17.9, F116), at the script engine. The
//! reviewer's file: its world and helpers are its own (after `thin_draw.rs`).

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cite;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::cross::{XLeg, XTerms};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::reserve::{Head, Shard, ShardRow, NOTE_MIN, SHARD_CARRIER};
use kpf_protocol::state::LONG;
use kpf_protocol::thin::{Account, Out, Outs, Pointed, Stated};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{p2pk_spk, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::cell::RefCell;
use kpf_protocol::reserve::Left;
use kpf_protocol::state::SHORT;
use kpf_protocol::covered::{CSaid, ALL, KIND_C_LEAVE};
use kpf_protocol::reserve::{Called, CALL_HOLD};
use kpf_protocol::covered::CallStep;
use kpf_protocol::covered::DIR_SPOT;

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;
const P10: i64 = 1_000_000_000;

fn kp(seed: u8) -> Keypair {
    Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&[seed; 32]).expect("a secret key"))
}
fn xk(k: &Keypair) -> [u8; 32] {
    k.x_only_public_key().0.serialize()
}
fn op(n: u8) -> TransactionOutpoint {
    TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }
}
fn fund(key: [u8; 32], n: u8) -> KeyUtxo {
    // (A coin that landed at the tests' time: an entry of that time, and what is done at will, show one.)
    KeyUtxo { outpoint: op(n), amount: 1_000 * KAS1 as u64, daa: T0 as u64, key }
}
fn accepted(b: &OBuilt, keys: &KeySet) -> bool {
    match b.sign(keys) {
        Ok(sg) => kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(),
        Err(_) => false,
    }
}
fn signed(what: &str, b: &OBuilt, keys: &KeySet) -> OSigned {
    let sg = b.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{what}: {:?}", v.errors());
    for k in 0..b.tx.outputs.len() {
        if Some(k) == b.change || b.output_labels[k] == "filler" {
            continue;
        }
        let mut t = b.clone();
        t.tx.outputs[k].value -= 1;
        assert!(!accepted(&t, keys), "{what}: a sompi less at output {k} ({}) is accepted", b.output_labels[k]);
    }
    println!("{what}: {} bytes, fee {} sompi", sg.mass.size, sg.fee);
    sg
}
fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
    let o = &sg.tx.outputs[i];
    OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
}

struct World {
    c: Keypair,
    w: Keypair,
    f: Keypair,
    members: [Keypair; 3],
    keys: KeySet,
    rid: Hash,
    markets: Vec<CMarket>,
    table: OLive<EpochTable>,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
    chain: RefCell<Vec<(Hash, Hash)>>,
}

impl World {
    fn script(&self, name: &str) -> LibScript {
        self.scripts.iter().find(|s| s.s == name).cloned().unwrap_or_else(|| panic!("{name}"))
    }
    fn payer<'a>(&self, who: &Keypair, funds: &'a [KeyUtxo]) -> Payer<'a> {
        Payer { funds, change_to: xk(who), rate: MIN_FEE_RATE }
    }
    fn terms(&self) -> XTerms {
        let p = &self.markets[0].p;
        XTerms { term: p.term, mmr_bps: p.mmr_bps, g_bps: p.g_bps, liq_fee_bps: p.liq_fee_bps, keeper_fee_min: p.keeper_fee_min, carrier: p.carrier, idle_daa: p.idle_daa, grace_daa: p.grace_daa, late_daa: p.late_daa, quiet_daa: p.quiet_daa }
    }
    fn head(&self, i: usize) -> Head {
        let m = &self.markets[i];
        Head { market: kpf_protocol::reserve::futures_market(&m.id()), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max }
    }
    /// The clearing party's shard: 100 KAS; in market 0 it is 60 units short over 140 of legs, in market 1 it is
    /// 10 long over 30, every leg entered at 10 (what its legs were entered at: 500 KAS short); two rows are empty.
    fn shard(&self) -> OLive<Shard> {
        let scale = self.markets[0].p.scale;
        let rows = [ShardRow { head: self.head(0), net: -60 * scale, gross: 140 * scale }, ShardRow { head: self.head(1), net: 10 * scale, gross: 30 * scale }, ShardRow::EMPTY, ShardRow::EMPTY];
        let s = Shard { t: self.terms(), rows, owner: xk(&self.c), held: 100 * KAS1, expiry: 0, dispute_daa: 50, bond_bps: 1_000, sid: [0x5d; 32], basis: -500 * KAS1, called: 0, owed: 0 };
        OLive { s, value: (s.held + SHARD_CARRIER) as u64, id: Some(Hash::from_bytes([0x73; 32])), op: op(12), daa: 6_100 }
    }
    fn cited(&self, i: usize, price: i64) -> Cited {
        let market = self.markets[i].id();
        let signed_prices: Vec<SignedPrice> = self.members.iter().map(|k| sign_price(k, &self.rid.as_bytes(), &market, T0, price)).collect();
        let att = Attestation::collect(&self.table.s, &self.rid.as_bytes(), &market, T0, &signed_prices).expect("a quorum");
        let funds = vec![fund(xk(&self.f), 90)];
        let (b, note) = note_write(&self.lib, &self.script("KpfTNote"), (&self.table, &att), xk(&self.f), self.payer(&self.f, &funds)).unwrap();
        let s = b.sign(&self.keys).expect("the note's transaction");
        let h = |k: u64| Hash::from_bytes(blake2b(&[&s.tx.id().as_bytes()[..], &k.to_le_bytes()].concat()));
        let lane: Vec<(Hash, u16, u32)> = (0..5).map(|k| (if k == 3 { s.tx.id() } else { h(k) }, 1u16, k as u32 + 1)).collect();
        let (_, activity_path) = cite::merkle(&cite::activity_leaves(&lane), 3);
        let wit = cite::Witness { block: h(100), merge_idx: 4, activity_path, lane_key: cite::native_lane_key(), parent_ref: h(101), context: cite::context(1_700_000_000_000, 6_600, 6_500), blue_score: 6_500, lanes_path: vec![(h(102), false), (h(103), true)], inactivity_shortcut: h(104), payload_and_ctx: h(105), parent_commitment: h(106) };
        self.chain.borrow_mut().push((wit.block, wit.commitment(&s.tx.id(), 1)));
        kpf_protocol::verify::set_chain_view(self.chain.borrow().clone());
        Cited::of(&s.tx, &self.lib, note, wit).unwrap()
    }
    /// The draw `st` stated on the shard (its bytes `said`), landed at 6,510.
    #[allow(dead_code)]
    fn stated(&self, shard: &OLive<Shard>, st: &Stated, said: &[u8]) -> OLive<Stated> {
        let who = if st.claimer == xk(&self.c) { &self.c } else { &self.w };
        let f = [fund(xk(who), 30)];
        let s = &shard.s;
        let b = state_raw(&self.lib, shard, &s.fixed_hash(), &s.moving(), st, said, s.form(), &self.script("KpfTChange"), self.payer(who, &f)).unwrap();
        let sg = b.sign(&self.keys).expect("stated");
        assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "the draw is stated");
        nth(&sg, 0, *st, 6_510)
    }
    fn pointed(&self, st: &OLive<Stated>, point: i64) -> OLive<Pointed> {
        let f = [fund(xk(&self.w), 31)];
        let (b, q) = point_dispute(&self.lib, st, &self.script("KpfTPoint"), xk(&self.w), point, self.payer(&self.w, &f)).unwrap();
        let s = b.sign(&self.keys).expect("pointed");
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok());
        nth(&s, 0, q, 6_520)
    }
}

fn world() -> World {
    let (c, w, f) = (kp(2), kp(3), kp(9));
    let members = [kp(101), kp(102), kp(103)];
    let mut keys = KeySet::new();
    for k in [&c, &w, &f] {
        keys.add_keypair(*k);
    }
    let rid = Hash::from_bytes([0x52; 32]);
    let rp = CommitteeParams::testnet();
    let markets = (0..4u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
    let mut table = EMPTY_TABLE;
    for (i, wt) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: wt * KAS1 };
    }
    let table = OLive { s: EpochTable { members: table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(20), daa: 6_000 };
    let funds = vec![fund(xk(&f), 200)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    World { c, w, f, members, keys, rid, markets, table, lib, scripts: lib_scripts(&gs.tx, 6_000), chain: RefCell::new(vec![]) }
}

fn fleg(w: &World, i: usize, units: i64, dir: i64, entry: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir, entry }
}

/// A shard of 100 KAS with no leg, to book legs in by hand (the statement of an entry does it: stage 4).
fn bare(w: &World) -> OLive<Shard> {
    let x = w.shard();
    OLive { s: Shard { rows: [ShardRow::EMPTY; 4], basis: 0, ..x.s }, ..x }
}

fn entered(legs: &[XLeg], t: i64) -> Vec<XLeg> {
    legs.iter().map(|l| if l.dir == kpf_protocol::covered::DIR_SPOT { *l } else { kpf_protocol::covered::leg_entered(l, t) }).collect()
}

fn cov(w: &World, shard: &OLive<Shard>, legs: &[XLeg], coins: i64) -> OLive<Account> {
    let s = Account { t: w.terms(), legs: legs.to_vec(), a_key: xk(&w.w), b_key: xk(&w.c), a_margin: coins, b_margin: 0, expiry: 0, dispute_daa: 50, bond_bps: 1_000, a_min_counter: legs.len() as i64, b_min_counter: 0, shard: Some(shard.s.sid) };
    OLive { s, value: coins as u64, id: shard.id, op: op(80), daa: 6_100 }
}

/// The statement `st` stated on the account as it is given (what the script of a statement takes, it takes).
fn acct_stated(w: &World, acct: &OLive<Account>, st: &Stated, said: &[u8]) -> OLive<Stated> {
    let who = if st.claimer == xk(&w.c) { &w.c } else if st.claimer == xk(&w.f) { &w.f } else { &w.w };
    let f = [fund(xk(who), 34)];
    let b = account_state(&w.lib, acct, &w.script("KpfTChange"), st, said, w.payer(who, &f)).unwrap();
    let sg = b.sign(&w.keys).expect("stated");
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "the statement is stated: {:?}", v.errors());
    nth(&sg, 0, *st, 6_510)
}


/// A statement of the rule's on a covered account, stated (with `c_state`), and applied after its dispute period.
#[allow(clippy::too_many_arguments)]
fn c_applied(w: &World, what: &str, acct: &OLive<Account>, kind: i64, amount: i64, skip: i64, a: &[i64], claimer: &Keypair) -> (OLive<Stated>, CSaid, kpf_protocol::covered::COutcome, OSigned) {
    let f = [fund(xk(claimer), 33)];
    let (b, st, s, o) = c_state(&w.lib, acct, &w.script("KpfTChange"), kind, amount, skip, a, T0, xk(claimer), w.payer(claimer, &f)).unwrap();
    let sg = signed(&format!("{what}: stated"), &b, &w.keys);
    assert_eq!(acct.s.c_wrong_point(kind, &s, &st.claimer, &st.outs, acct.value as i64, T0, a), None, "{what}");
    let live = nth(&sg, 0, st, 6_510);
    let b = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(claimer, &f)).unwrap();
    let sg2 = signed(&format!("{what}: applied"), &b, &w.keys);
    (live, s, o, sg2)
}

fn out_of<'a>(sg: &'a OSigned, spk: &kaspa_consensus_core::tx::ScriptPublicKey) -> (usize, u64) {
    let k = sg.tx.outputs.iter().position(|o| &o.script_public_key == spk).expect("the output");
    (k, sg.tx.outputs[k].value)
}

/// Whether the transaction pays exactly `value` to the key (the payer's change goes to a key too).
fn pays(sg: &OSigned, key: &[u8; 32], value: i64) -> bool {
    sg.tx.outputs.iter().any(|o| o.script_public_key == p2pk_spk(key) && o.value == value as u64)
}

/// What the id of the shard of these tests is the hash of beside its committee, its terms and its periods.
const SEED: [u8; 32] = [0x5e; 32];

/// A shard of 100 KAS with no leg, whose id is what an entry holds it to: the hash of a seed, of the committee of
/// the markets, and of the terms and periods of the accounts under it.
fn seeded(w: &World) -> OLive<Shard> {
    let x = bare(w);
    let sid = kpf_protocol::reserve::shard_sid(&SEED, &w.markets[0].committee_id, &x.s.terms(), &x.s.moving()[93..120]);
    OLive { s: Shard { sid, ..x.s }, ..x }
}


/// The shard called and its call, as objects (what the settlement of a call writes: the tests above).
fn called_objs(shard: &OLive<Shard>, prices: [i64; 4], daa: u64) -> (OLive<Shard>, OLive<Called>) {
    let fee = shard.s.t.keeper_fee_min.min(shard.s.held);
    let (next, call) = shard.s.called_at(&prices, T0, fee);
    (OLive { s: next, value: shard.value - fee as u64 - CALL_HOLD as u64, id: shard.id, op: op(90), daa }, OLive { s: call, value: CALL_HOLD as u64, id: shard.id, op: op(91), daa })
}

/// A token leg: `units` of a token priced by market `i`, held by the vault `[0x99; 32]`, at a discount of 20 %.
fn tleg(w: &World, i: usize, units: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: [0x99; 32], scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir: DIR_SPOT, entry: 2_000 }
}

// ================================================================================================================
// Review T3: the reviewer's own helpers.

use kpf_protocol::covered::{leg_entered, leg_more, leg_time};
use kpf_protocol::oracle_build::OPlan;
use kpf_protocol::tx::Arg;
use silverscript_abi::ArtifactValue;

/// T of docs 17.9 with the terms of these tests: twice a lateness (100) and a dispute period (50): 300.
fn t_young(w: &World) -> i64 {
    2 * (w.terms().late_daa + 50)
}

/// The shard called at the time `time` at `prices`, and its call, as objects (what the settlement of a call writes).
fn called_at(shard: &OLive<Shard>, prices: [i64; 4], time: i64, daa: u64) -> (OLive<Shard>, OLive<Called>) {
    let fee = shard.s.t.keeper_fee_min.min(shard.s.held);
    let (next, call) = shard.s.called_at(&prices, time, fee);
    (OLive { s: next, value: shard.value - fee as u64 - CALL_HOLD as u64, id: shard.id, op: op(90), daa }, OLive { s: call, value: CALL_HOLD as u64, id: shard.id, op: op(91), daa })
}

/// A built transaction with its input `i` landed at `daa` (what `OpTxInputDaaScore` reads of it).
fn landed(b: &OBuilt, i: usize, daa: u64) -> OBuilt {
    let mut t = b.clone();
    t.entries[i].block_daa_score = daa;
    t
}

/// The arguments of the contract input `i` (input 1 is the script's copy in every transaction of these tests).
fn args_of(b: &mut OBuilt, i: usize) -> &mut Vec<Arg> {
    let OPlan::Contract { args, .. } = &mut b.plans[i] else { panic!("input {i} is not a contract input") };
    args
}
fn with_arg(b: &OBuilt, i: usize, a: Arg) -> OBuilt {
    let mut t = b.clone();
    args_of(&mut t, 1)[i] = a;
    t
}
fn bytes_arg(b: &[u8]) -> Arg {
    Arg::V(ArtifactValue::Bytes(b.to_vec()))
}
/// Input `i` of a built transaction made the object whose state hashes to `h`.
fn object_made(b: &mut OBuilt, lib: &[u8; 32], i: usize, h: &[u8; 32]) {
    b.entries[i].script_public_key = hash_spk(lib, h);
    let OPlan::Contract { redeem, .. } = &mut b.plans[i] else { panic!("input {i} is not an object") };
    *redeem = kpf_protocol::thin_artifacts::kit().object(lib, h);
}

/// An account of the shard's owner's own (both keys the owner's), as an output stands on the chain.
fn own_acct(w: &World, shard: &OLive<Shard>, legs: &[XLeg], coins: i64, n: u8) -> OLive<Account> {
    let x = cov(w, shard, legs, coins);
    OLive { s: Account { a_key: xk(&w.c), ..x.s.clone() }, op: op(n), ..x }
}

// ================================================================================================================
// F116 at the engine.

/// F116, whole, at the engine, in the numbers of a shard of these tests. A shard that holds 20 KAS; an honest
/// account long 5 units from 10, entered long before; the owner's own account long 1,000 units from 10. The call
/// is at 13, at the time T0: the honest leg gained 15 KAS, the owner's 3,000.
///  - The owner's leg entered a block before the call's time: its step asks nothing (its coins go back to the
///    owner's key), the honest note is the only claim, and it is paid its 15 KAS whole.
///  - The same with the owner's leg entered T before the call's time, which is what F116 was for every leg: its
///    note asks 3,000 KAS, and the honest claim of 15 is paid 0.099 KAS.
/// What decides between the two is eight bytes of the owner's leg, which `KpfTEnterS` wrote.
#[test]
fn t3_f116_the_owners_young_leg_asks_nothing_and_the_honest_claim_is_paid_whole() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let fee = base.s.t.keeper_fee_min;
    let t = t_young(&w);
    assert_eq!(t, 300);
    let (honest, big) = (fleg(&w, 0, 5, LONG, P10), fleg(&w, 0, 1_000, LONG, P10));
    let books = base.s.booked(&honest).unwrap().booked(&big).unwrap();
    let shard = OLive { s: Shard { held: 20 * KAS1, ..books }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let p13 = [P10 * 13 / 10, 0, 0, 0];
    assert!(shard.s.call_due(&p13));
    let (called, call) = called_objs(&shard, p13, 6_600);
    assert_eq!(call.s.time, T0);
    let step_s = w.script("KpfTCStep");
    let pay_s = w.script("KpfTCallPay");
    let a_h = cov(&w, &shard, &entered(&[honest], T0 - 2 * t), 20 * KAS1);
    let mut paid_honest = vec![];
    for (what, time, asks) in [("young: it entered a block before the call's time", T0 - 1, 0), ("old: it entered T before the call's time", T0 - t, 3_000 * KAS1)] {
        let a_o = own_acct(&w, &shard, &entered(&[big], time), 6_000 * KAS1, 81);
        // The honest account's step: its note asks the 15 KAS.
        let (b, s_h) = call_step(&w.lib, &a_h, &a_h.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
        let sg_h = signed(&format!("F116, the owner's leg {what}: the honest account stepped"), &b, &w.keys);
        assert_eq!((s_h.gain, s_h.note.asked), (15 * KAS1, 15 * KAS1));
        // The owner's account's step.
        let (b, s_o) = call_step(&w.lib, &a_o, &a_o.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
        let sg_o = signed(&format!("F116, the owner's leg {what}: the owner's account stepped"), &b, &w.keys);
        assert_eq!((s_o.gain, s_o.pay, s_o.note.asked, s_o.user), (asks, 0, asks, 6_000 * KAS1 - NOTE_MIN - fee), "{what}");
        assert!(pays(&sg_o, &xk(&w.c), s_o.user), "the owner has its coins back, less a note's least and the fee");
        // The two notes applied to the called shard: the legs out of the books; what they ask, claims.
        let (b, sh1, c_h) = left_apply(&w.lib, &called, &nth(&sg_h, 2, s_h.note.clone(), 6_610), &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
        let sg = signed("the honest note applied", &b, &w.keys);
        let (sh1, c_h) = (nth(&sg, 0, sh1, 6_620), nth(&sg, 2, c_h.expect("a claim"), 6_620));
        let (b, sh2, c_o) = left_apply(&w.lib, &sh1, &nth(&sg_o, 2, s_o.note.clone(), 6_610), &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
        let sg = signed("the owner's note applied", &b, &w.keys);
        let sh2 = nth(&sg, 0, sh2, 6_630);
        assert_eq!((sh2.s.rows, sh2.s.owed, sh2.s.held, c_o.is_some()), ([ShardRow::EMPTY; 4], 15 * KAS1 + asks, 20 * KAS1 - fee, asks > 0), "{what}: the books are empty, and what is owed by claims");
        // The honest claim paid.
        let (b, _, paid) = claim_pay(&w.lib, &sh2, &c_h, &pay_s, w.payer(&w.f, &ff)).unwrap();
        let sg = signed(&format!("F116, the owner's leg {what}: the honest claim paid"), &b, &w.keys);
        assert!(pays(&sg, &xk(&w.w), paid));
        paid_honest.push(paid);
    }
    println!("T3 F116 at the engine: an honest claim of 15 KAS on a called shard of 19.9 KAS, beside the owner's long of 1,000 units that gained 3,000 KAS: paid {} sompi where the owner's leg is young, {} where it is old", paid_honest[0], paid_honest[1]);
    assert_eq!(paid_honest[0], 15 * KAS1, "F116 is open: the honest claim is not paid whole beside the owner's young leg");
    assert!(paid_honest[1] < KAS1 / 10 && paid_honest[1] > 9 * KAS1 / 100, "(what F116 was) {}", paid_honest[1]);
}

// ================================================================================================================
// B1. The time's source.

/// B1, the entry's time against its landing. `KpfTEnterS` holds the leg's time to the statement's `t0`; `t0` is
/// held from above by an input that landed at or after it, and from below, where the entry is STATED, only by the
/// account's time and by the shard's time less two dispute periods: on a new account under a shard that says no
/// time an entry of the time 0 is stated at 6,500 (run). What holds it is where it is applied: a statement that
/// landed more than a lateness after its time is neither applied (`KpfTChangeSettle`) nor disputed (`KpfTPoint`:
/// so it never "stands again", where its lateness is not read any more); it is ended (`KpfTChangeLate`), and the
/// leg is undone. Run here, each at the engine past the builder: the entry of the time 0 that landed at 6,510 is
/// not applied and not disputed (each taken with the same statement landed at 60: the controls), and is ended.
/// An entry of the time 6,410 that landed at 6,510, exactly a lateness, is applied: the account's leg says 6,410.
/// One that landed at 6,511 is not.
/// So a leg's time is at most a lateness (100) before its landing: with T = 300 an owner has an "old" leg no
/// sooner than 200 blocks after the transaction that put it in the books (finding L2: not T).
#[test]
fn t3_b1_an_entrys_time_is_within_a_lateness_of_its_landing_where_the_leg_comes_to_stand() {
    let w = world();
    let shard = seeded(&w);
    let acct = cov(&w, &shard, &[], 20 * KAS1);
    let leg = fleg(&w, 0, 10, LONG, P10);
    let (f, ff) = ([fund(xk(&w.w), 33)], [fund(xk(&w.f), 36)]);
    let enter = w.script("KpfTEnterS");
    let late = w.terms().late_daa;
    assert_eq!((shard.s.expiry, acct.s.b_min_counter, late), (0, 0, 100));
    // An entry of the time 0, stated with the chain at 6,500.
    let (b, st, said, marked, _, _) = c_enter_state(&w.lib, &acct, &shard, &enter, SEED, leg, 5 * KAS1, &[], 0, w.payer(&w.w, &f)).unwrap();
    let sg = signed("an entry of the time 0, stated at 6,500", &b, &w.keys);
    assert_eq!((leg_time(&said.leg), leg_time(marked.0.legs.last().unwrap()), st.t0), (0, 0, 0));
    let live = nth(&sg, 0, st, 6_510);
    let settle = w.script("KpfTChangeSettle");
    // Not applied: the builder; and its transaction for the same statement landed at 60, shown landed at 6,510.
    assert!(stated_settle(&w.lib, &live, &settle, w.payer(&w.f, &ff)).is_err());
    let early = OLive { daa: 60, ..live.clone() };
    let b = stated_settle(&w.lib, &early, &settle, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys), "(control) the entry of the time 0, had it landed at 60, is applied");
    assert!(!accepted(&landed(&b, 0, 6_510), &w.keys), "an entry that landed 6,510 blocks after its time is applied");
    assert!(!accepted(&landed(&b, 0, 101), &w.keys) && accepted(&landed(&b, 0, 100), &w.keys), "the edge: a lateness, and a block more");
    // Not disputed (so it is not answered right and does not stand again, where its lateness is the mark).
    assert!(point_dispute(&w.lib, &live, &w.script("KpfTPoint"), xk(&w.f), 0, w.payer(&w.f, &ff)).is_err());
    let (b, _) = point_dispute(&w.lib, &early, &w.script("KpfTPoint"), xk(&w.f), 0, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys), "(control) had it landed at 60, it is disputed");
    assert!(!accepted(&landed(&b, 0, 6_510), &w.keys) && !accepted(&landed(&b, 0, 101), &w.keys), "an entry out of its time is disputed");
    // Ended, by anyone: the marked account again.
    let b = stated_late(&w.lib, &live, &w.script("KpfTChangeLate"), xk(&w.f), w.payer(&w.f, &ff)).unwrap();
    let ended = signed("the entry of the time 0, ended as late", &b, &w.keys);
    assert_eq!(&ended.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &marked.hash()));

    // An entry of the time 6,410 that landed at 6,510: applied. The account's leg says 6,410.
    let (b, st, said, _, after, _) = c_enter_state(&w.lib, &acct, &shard, &enter, SEED, leg, 5 * KAS1, &[], T0 - 90, w.payer(&w.w, &f)).unwrap();
    let sg = signed("an entry of the time 6,410, stated at 6,500", &b, &w.keys);
    let live = nth(&sg, 0, st, 6_510);
    let b = stated_settle(&w.lib, &live, &settle, w.payer(&w.f, &ff)).unwrap();
    let ap = signed("the entry of the time 6,410 that landed at 6,510, applied", &b, &w.keys);
    assert_eq!((leg_time(&said.leg), leg_time(&after.legs[0]), &ap.tx.outputs[0].script_public_key), (T0 - 90, T0 - 90, &hash_spk(&w.lib, &after.hash())));
    assert!(stated_settle(&w.lib, &OLive { daa: 6_511, ..live.clone() }, &settle, w.payer(&w.f, &ff)).is_err() && !accepted(&landed(&b, 0, 6_511), &w.keys), "a block later than a lateness");
    // The time that account's next statement of its user's is held to: 400 after the entry's time, T is 300.
    assert_eq!((after.b_min_counter - leg_time(&after.legs[0]), t_young(&w)), (400, 300));
}

/// B1 and B3 (a): what carries a leg from one account to another, or rewrites it, carries its time. At the engine:
///  - a join (`KpfTJoinC`) of an account whose leg is old with one whose leg is young: the one account has the two
///    legs as they were, and under the call its step asks the old leg's gain alone; the joined account written
///    with the young leg said old is refused;
///  - a hand-over (`KpfTPass`): the leg as it was; written with another time, refused; stepped in its new hands,
///    the young leg asks nothing;
///  - a mark (`KpfTCRow` of kind 13): the leg at its new entry says the time it said; a mark whose chain is of the
///    leg with another time has its row answered wrong;
///  - a split (`KpfTCSplit`, `KpfTCRow`): the new account's leg is the leg; a split that names the leg by a chain
///    of another time has its row answered wrong.
/// And a mark and a split are a user's statements: stated at or after the account's time, which an entry writes
/// 400 blocks after its own (twice a lateness and two dispute periods), T being 300. No leg is marked or split
/// while it is young.
#[test]
fn t3_b1_b3_a_join_a_hand_over_a_mark_and_a_split_carry_the_legs_time() {
    use kpf_protocol::covered::{chain_of_one, KIND_C_MARK, KIND_C_SPLIT};
    let w = world();
    let base = seeded(&w);
    let t = t_young(&w);
    let (ff, fw) = ([fund(xk(&w.f), 36)], [fund(xk(&w.w), 37)]);
    let raw = fleg(&w, 0, 10, LONG, P10);
    let (l_old, l_young) = (leg_entered(&raw, T0 - t), leg_entered(&raw, T0 - t + 1));
    let shard = OLive { s: Shard { held: 20 * KAS1, ..base.s.booked(&raw).unwrap().booked(&raw).unwrap() }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let p13 = [P10 * 13 / 10, 0, 0, 0];
    let (_, call) = called_objs(&shard, p13, 6_600);
    let step_s = w.script("KpfTCStep");
    let x_old = cov(&w, &shard, &[l_old], 20 * KAS1);
    let x_young = OLive { op: op(81), ..cov(&w, &shard, &[l_young], 20 * KAS1) };
    let asks = |a: &OLive<Account>, what: &str| -> i64 {
        let (b, st) = call_step(&w.lib, a, &a.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
        assert!(accepted(&b, &w.keys), "{what}");
        assert_eq!(st.gain, st.note.asked, "{what}");
        st.note.asked
    };
    assert_eq!((asks(&x_old, "the old leg's account"), asks(&x_young, "the young leg's account")), (30 * KAS1, 0));
    // A join, either way round.
    for (first, second, legs) in [(&x_old, &x_young, vec![l_old, l_young]), (&x_young, &x_old, vec![l_young, l_old])] {
        let (jb, joined) = account_join(&w.lib, first, second, &shard, &w.script("KpfTJoinC"), w.payer(&w.w, &fw)).unwrap();
        let js = signed("an account of an old leg and one of a young leg, joined", &jb, &w.keys);
        assert_eq!(joined.legs, legs);
        let one = nth(&js, 0, joined.clone(), 6_400);
        assert_eq!(asks(&one, "the joined account"), 30 * KAS1, "the old leg's gain, and nothing for the young leg");
        let mut laundered = jb.clone();
        laundered.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Account { legs: vec![l_old, l_old], ..joined.clone() }.hash());
        assert!(!accepted(&laundered, &w.keys), "a join writes the young leg with the old leg's time");
    }
    // A hand-over.
    let (pb, mine) = account_pass(&w.lib, &x_young, &w.script("KpfTPass"), xk(&w.f), 0, w.payer(&w.f, &ff)).unwrap();
    let ps = signed("a young leg's account handed over", &pb, &w.keys);
    assert_eq!(mine.legs, vec![l_young]);
    assert_eq!(asks(&nth(&ps, 0, mine.clone(), 6_400), "the account in its new hands"), 0);
    let mut laundered = pb.clone();
    laundered.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Account { legs: vec![l_old], ..mine.clone() }.hash());
    assert!(!accepted(&laundered, &w.keys), "a hand-over writes the leg with another time");

    // A mark: an account whose leg entered at 6,100 says the time 6,500 (400 after), and is marked at 6,500.
    let entered_at = T0 - 400;
    let l_m = leg_entered(&raw, entered_at);
    let quiet = |legs: &[XLeg], coins: i64| {
        let x = cov(&w, &shard, legs, coins);
        OLive { s: Account { b_min_counter: T0, ..x.s.clone() }, ..x }
    };
    let m_acct = quiet(&[l_m], 40 * KAS1);
    let up12 = [P10 * 12 / 10];
    let ne = m_acct.s.mark_entry(&l_m, up12[0]).unwrap();
    let (live, said, o, _) = c_applied(&w, "a gain taken out of a leg, which keeps its time", &m_acct, KIND_C_MARK, 0, 1, &up12, &w.w);
    let after = o.after.clone().unwrap();
    assert_eq!((after.legs.clone(), leg_time(&after.legs[0])), (vec![XLeg { entry: ne, ..l_m }], entered_at));
    assert!(c_state(&w.lib, &OLive { s: Account { b_min_counter: T0 + 1, ..m_acct.s.clone() }, ..m_acct.clone() }, &w.script("KpfTChange"), KIND_C_MARK, 0, 1, &up12, T0, xk(&w.w), w.payer(&w.w, &fw)).is_err(), "a mark before the account's time");
    let row_script = w.script("KpfTCRow");
    let q = w.pointed(&live, 1);
    let (b, right) = c_row(&w.lib, &q, &said, 1, &[0; 32], &l_m, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
    assert!(right);
    signed("the row of a mark answered right", &b, &w.keys);
    // (The same mark, its chain of the leg at its new entry with another time: the row is wrong.)
    for other in [T0 - t - 1_000, T0, 0] {
        let mut rows = said.rows.clone();
        rows[0].h = chain_of_one(&XLeg { entry: ne, ..leg_entered(&raw, other) });
        let lie = CSaid { rows, ..said.clone() };
        let st = Stated { arg: lie.arg(), ..live.s };
        let q = w.pointed(&acct_stated(&w, &m_acct, &st, &lie.said()), 1);
        let (b, right) = c_row(&w.lib, &q, &lie, 1, &[0; 32], &l_m, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
        assert!(!right && accepted(&b, &w.keys), "a mark that rewrites the leg's time to {other}: its row is answered wrong");
        let (b, _) = c_row(&w.lib, &q, &lie, 1, &[0; 32], &l_m, &row_script, xk(&w.f), w.payer(&w.w, &fw), Some(true)).unwrap();
        assert!(!accepted(&b, &w.keys), "a mark that rewrites the leg's time to {other}: its row is answered right");
    }

    // A split: the short leaves for an account of its own, as it is.
    let l_s = leg_entered(&fleg(&w, 1, 5, SHORT, P10), entered_at + 3);
    let s_acct = quiet(&[l_m, l_s], 40 * KAS1);
    let a = [P10, P10];
    let (live, said, o, _) = c_applied(&w, "a leg leaves for an account of its own, with its time", &s_acct, KIND_C_SPLIT, 10 * KAS1, 2, &a, &w.w);
    let (stays, new) = (o.after.clone().unwrap(), o.split.clone().unwrap());
    assert_eq!((stays.legs.clone(), new.legs.clone(), leg_time(&new.legs[0])), (vec![l_m], vec![l_s], entered_at + 3));
    let q = w.pointed(&live, 3);
    let (b, right) = c_row(&w.lib, &q, &said, 3, &said.rows[0].g, &l_s, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
    assert!(right);
    signed("the row of the leg that leaves answered right", &b, &w.keys);
    let lie = CSaid { lc: chain_of_one(&leg_entered(&l_s, 0)), ..said.clone() };
    let st = Stated { arg: lie.arg(), ..live.s };
    let q = w.pointed(&acct_stated(&w, &s_acct, &st, &lie.said()), 3);
    let (b, right) = c_row(&w.lib, &q, &lie, 3, &said.rows[0].g, &l_s, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
    assert!(!right && accepted(&b, &w.keys), "a split that names the leg by a chain of another time: its row is answered wrong");
}

// ================================================================================================================
// B2. The young rule itself.

/// B2, at the engine. The eight bytes read as a number: the largest (young), the largest below nothing (old), a
/// nothing with the sign's bit set (old): each account is stepped (no leg makes its account one no step takes).
/// An account whose legs are all young and gained: its user has its coins back less a note's least and the fee
/// (the step is paid as any), the note asks nothing and holds a note's least, and applied it takes the legs out of
/// the called shard's books and makes no claim. A young loss larger than the coins: the coins pay, a note's least
/// staying. Young legs beside a token leg: the account goes on with its tokens; a young gain asks nothing, and is
/// NOT set against what the account owes (an old gain is); a young loss the coins do not pay is owed.
#[test]
fn t3_b2_the_young_rule_at_its_edges_and_beside_tokens_and_what_is_owed() {
    let w = world();
    let base = seeded(&w);
    let t = t_young(&w);
    let ff = [fund(xk(&w.f), 36)];
    let fee = base.s.t.keeper_fee_min;
    let (long, short, tok) = (fleg(&w, 0, 10, LONG, P10), fleg(&w, 0, 10, SHORT, P10), tleg(&w, 1, 10));
    let books = base.s.booked(&long).unwrap().booked(&long).unwrap();
    let shard = OLive { s: Shard { held: 20 * KAS1, ..books }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let p13 = [P10 * 13 / 10, 0, 0, 0];
    let (called, call) = called_objs(&shard, p13, 6_600);
    let step_s = w.script("KpfTCStep");
    let stepped = |what: &str, a: &OLive<Account>| -> (CallStep, OSigned) {
        let (b, st) = call_step(&w.lib, a, &a.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap_or_else(|e| panic!("{what}: {e}"));
        (st, signed(what, &b, &w.keys))
    };
    // The bytes at their ends.
    let mut zero_below = leg_more(0, 0, 0, 0);
    zero_below[7] = 0x80;
    for (what, more, read, gain) in [
        ("the largest time", leg_more(i64::MAX, 0, 0, 0), i64::MAX, 0),
        ("the largest time below nothing", leg_more(-i64::MAX, 0, 0, 0), -i64::MAX, 30 * KAS1),
        ("eight bytes of 0xff", [0xff; 32], -i64::MAX, 30 * KAS1),
        ("nothing, the sign's bit set", zero_below, 0, 30 * KAS1),
        ("a block below nothing", leg_more(-1, 0, 0, 0), -1, 30 * KAS1),
        ("the edge, T before the call's time", leg_more(T0 - t, 0, 0, 0), T0 - t, 30 * KAS1),
        ("a block after the edge", leg_more(T0 - t + 1, 0, 0, 0), T0 - t + 1, 0),
    ] {
        let l = XLeg { committee: more, ..long };
        assert_eq!(leg_time(&l), read, "{what}");
        let (st, _) = stepped(&format!("a leg that says {what}"), &cov(&w, &shard, &[l], 40 * KAS1));
        assert_eq!((st.gain, st.note.asked), (gain, gain), "{what}");
    }
    // (The day, the strike and the premium beside the time are read by no step.)
    let (st, _) = stepped("an old leg that says a day, a strike and a premium", &cov(&w, &shard, &[XLeg { committee: leg_more(0, 7_000, P10, 5), ..long }], 40 * KAS1));
    assert_eq!(st.gain, 30 * KAS1);

    // Every leg young, and gained: the coins back, the note of nothing, the legs out of the books.
    let young = leg_entered(&long, T0 - 1);
    let (st, sg) = stepped("an account of two young legs that gained", &cov(&w, &shard, &[young, young], 40 * KAS1));
    assert_eq!((st.gain, st.pay, st.note.asked, st.fee, st.user), (0, 0, 0, fee, 40 * KAS1 - NOTE_MIN - fee));
    assert!(pays(&sg, &xk(&w.w), st.user) && out_of(&sg, &hash_spk(&w.lib, &st.note.hash())).1 == NOTE_MIN as u64);
    let (b, next, claim) = left_apply(&w.lib, &called, &nth(&sg, 2, st.note.clone(), 6_610), &w.script("KpfTLeftAll"), w.payer(&w.f, &ff)).unwrap();
    signed("the note of young legs that gained, applied to the called shard", &b, &w.keys);
    assert_eq!((next.rows, next.owed, next.held, claim), ([ShardRow::EMPTY; 4], 0, called.s.held, None));
    // A young loss larger than the coins.
    let (st, sg) = stepped("a young short that lost more than its account has", &cov(&w, &shard, &[leg_entered(&short, T0 - 1)], 5 * KAS1));
    assert_eq!((st.gain, st.pay, st.note.asked, st.user), (-30 * KAS1, 5 * KAS1 - NOTE_MIN, -(5 * KAS1 - NOTE_MIN), 0));
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &st.note.hash())).1, (5 * KAS1) as u64);

    // Beside a token leg, and beside what the account owes.
    let owing = |legs: &[XLeg], coins: i64, owes: i64| {
        let x = cov(&w, &shard, legs, coins);
        OLive { s: Account { b_margin: owes, ..x.s.clone() }, ..x }
    };
    let (st, _) = stepped("a young long that gained, beside a token leg", &owing(&[young, tok], 5 * KAS1, 0));
    let after = st.after.clone().expect("the account goes on with its tokens");
    assert_eq!((st.note.asked, after.legs.clone(), after.b_margin, st.user), (0, vec![tok], 0, 0));
    let (st, _) = stepped("a young long that gained, on an account that owes 6 KAS", &owing(&[young, tok], 5 * KAS1, 6 * KAS1));
    assert_eq!((st.note.asked, st.after.as_ref().map(|x| x.b_margin)), (0, Some(6 * KAS1)), "a young gain is set against nothing the account owes");
    let (st, _) = stepped("an old long that gained, on an account that owes 6 KAS", &owing(&[leg_entered(&long, T0 - t), tok], 5 * KAS1, 6 * KAS1));
    assert_eq!((st.note.asked, st.after.as_ref().map(|x| x.b_margin)), (24 * KAS1, Some(0)), "(an old gain is: review 64, F112)");
    let (st, _) = stepped("a young short that lost, beside a token leg, on few coins", &owing(&[leg_entered(&short, T0 - 1), tok], 5 * KAS1, 0));
    let after = st.after.clone().unwrap();
    assert_eq!(st.pay + after.b_margin, 30 * KAS1, "what a young leg lost is paid or owed, whole");
    assert!(st.pay > 0 && after.b_margin > 0);
    // An old loss and a young gain, a young loss and an old gain, beside a token leg.
    let (st, _) = stepped("an old short that lost and a young long that gained, beside a token leg", &owing(&[leg_entered(&short, 0), young, tok], 50 * KAS1, 0));
    assert_eq!((st.gain, st.pay), (-30 * KAS1, 30 * KAS1));
    let (st, _) = stepped("an old long that gained and a young short that lost, beside a token leg", &owing(&[leg_entered(&long, 0), leg_entered(&short, T0), tok], 50 * KAS1, 0));
    assert_eq!((st.gain, st.pay, st.note.asked), (0, 0, 0));
}

/// B2, T. The step reads T of the account's own moving part (its lateness and its dispute period, bytes 94 to 102
/// and 103 to 111), and is shown nothing of the shard's id. Run: an account that says no lateness and no dispute
/// period under the same call has T = 0, and its leg of a block before the call's time is old; one whose dispute
/// period is half the largest number is stepped by no transaction (the sum does not fit: the engine refuses it;
/// the control is the same transaction for an account that differs in another number).
/// No such account stands under a shard: the shard's id is the hash of the terms and of bytes 93 to 120 of the
/// moving part, which hold the three periods, and `KpfTOpenC` (read) and `KpfTEnterS` (run here) hold an
/// account's own to it: an entry into an account of other periods under the shard's id is refused, with the shard
/// written as that account's periods would have it.
#[test]
fn t3_b2_an_account_has_its_shards_periods_and_the_step_reads_the_accounts() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let f = [fund(xk(&w.w), 33)];
    let long = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: Shard { held: 20 * KAS1, ..base.s.booked(&long).unwrap() }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let (_, call) = called_objs(&shard, [P10 * 13 / 10, 0, 0, 0], 6_600);
    let step_s = w.script("KpfTCStep");
    let young = leg_entered(&long, T0 - 1);
    let acct = cov(&w, &shard, &[young], 40 * KAS1);
    let (b, st) = call_step(&w.lib, &acct, &acct.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys) && st.gain == 0);
    // No lateness and no dispute period said: T is nothing, and the leg is old.
    let quick = OLive { s: Account { t: XTerms { late_daa: 0, ..acct.s.t }, dispute_daa: 0, ..acct.s.clone() }, ..acct.clone() };
    let (b, st) = call_step(&w.lib, &quick, &quick.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys) && st.gain == 30 * KAS1, "the step reads the periods the account says");
    // A dispute period beyond what is counted: no step. (The moving part is the step's argument 6. The leg of this
    // part says a time after the call's: young whatever T is, so that the step is the same transaction but for
    // the periods said.)
    let after_call = OLive { s: Account { legs: vec![leg_entered(&long, T0 + 1)], ..quick.s.clone() }, ..quick.clone() };
    let (b, st) = call_step(&w.lib, &after_call, &after_call.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys) && st.gain == 0);
    let other = |x: Account| {
        let mut t = b.clone();
        object_made(&mut t, &w.lib, 0, &x.hash());
        args_of(&mut t, 1)[6] = bytes_arg(&x.moving());
        t
    };
    assert!(accepted(&other(Account { expiry: 77, ..after_call.s.clone() }), &w.keys), "(control) the same step for an account that differs in a number no step reads");
    let fits = Account { dispute_daa: i64::MAX / 2, ..after_call.s.clone() };
    assert!(fits.call_step(&call.s, 40 * KAS1).is_some_and(|s| s.gain == 0) && accepted(&other(fits), &w.keys), "the largest T that fits: the leg is young");
    let huge = Account { dispute_daa: i64::MAX / 2 + 1, ..after_call.s.clone() };
    assert!(huge.call_step(&call.s, 40 * KAS1).is_none() && !accepted(&other(huge), &w.keys), "an account whose T does not fit is stepped");
    // (Periods below nothing, by the model alone: the call's time less T does not fit.)
    assert!(Account { dispute_daa: -(i64::MAX / 2), ..after_call.s.clone() }.call_step(&call.s, 40 * KAS1).is_none());

    // No account of other periods enters under the shard's id.
    let enter = w.script("KpfTEnterS");
    let seeded = seeded(&w);
    let by_hand = |a: &OLive<Account>| {
        let (st, said, _, _) = a.s.c_enter(long, 0, 0, &[], T0, a.value as i64).unwrap();
        let next = Shard { expiry: T0 + a.s.t.late_daa + 2 * a.s.dispute_daa, ..seeded.s.booked(&long).unwrap() };
        c_enter_state_as(&w.lib, a, &seeded, &enter, SEED, &st, &said, &next, w.payer(&w.w, &f))
    };
    let plain = cov(&w, &seeded, &[], 20 * KAS1);
    assert!(accepted(&by_hand(&plain).unwrap(), &w.keys), "(control) an entry written by hand");
    for (what, odd) in [
        ("no dispute period", Account { dispute_daa: 0, ..plain.s.clone() }),
        ("a dispute period a block shorter", Account { dispute_daa: 49, ..plain.s.clone() }),
        ("a longer dispute period", Account { dispute_daa: 51, ..plain.s.clone() }),
        ("another bond rate", Account { bond_bps: 999, ..plain.s.clone() }),
    ] {
        let b = by_hand(&OLive { s: odd, ..plain.clone() });
        assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "an account of {what} than its shard's takes a leg");
    }
}

// ================================================================================================================
// B3. The owner's ways around it.

/// B3 (b), the call's time. A call is a statement (kind 11): its time is the statement's `t0`, which nothing holds
/// where it is stated; it is applied only where it landed at its time or within the shard's lateness after it, is
/// disputed only so, and is otherwise ended (`KpfTChangeSettle`, `KpfTPoint`, `KpfTChangeLate`). At the engine: a
/// call of the time 6,410 that landed at 6,510 is applied, and the call written says 6,410; shown landed at 6,511
/// it is neither applied nor disputed, and is ended; a call of a time a block ahead of its landing the same.
/// So a call's time is within a lateness BEFORE its landing, at its caller's choice, and never after it. No caller
/// makes a leg old by the time it says (an earlier time makes more legs young, a later one is not applied). What
/// the choice does, run below with steps under calls of two times:
///  - a leg that entered at 6,150 is old at a call that landed at 6,510 and says 6,500, and young at one that
///    landed in the same block and says 6,410: its 30 KAS are asked, or nothing (finding L3);
///  - an entry of the time 6,410 that landed at 6,510 (a lateness back: `t3_b1_...` applies one) is old at a call
///    of the time 6,710, two hundred blocks after the entry's landing, and young at a call that landed at 6,710
///    and says 6,610 (finding L2).
#[test]
fn t3_b3_a_calls_time_is_within_a_lateness_before_its_landing_at_the_callers_choice() {
    let w = world();
    let shard = w.shard();
    let fee = shard.s.t.keeper_fee_min;
    let up = [P10 * 11 / 10, P10, 0, 0];
    assert!(shard.s.call_due(&up));
    let (f, ff) = ([fund(xk(&w.w), 30)], [fund(xk(&w.f), 31)]);
    let (change, settle, point, late_s) = (w.script("KpfTChange"), w.script("KpfTChangeSettle"), w.script("KpfTPoint"), w.script("KpfTChangeLate"));
    // A call of the time 6,410 that landed at 6,510.
    let (b, st, _) = shard_call(&w.lib, &shard, &change, up, T0 - 90, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    let sg = signed("a call of the time 6,410, stated", &b, &w.keys);
    let live = nth(&sg, 0, st, 6_510);
    let b = stated_settle(&w.lib, &live, &settle, w.payer(&w.f, &ff)).unwrap();
    let ap = signed("a call of the time 6,410 that landed at 6,510, applied", &b, &w.keys);
    let (next, call) = shard.s.called_at(&up, T0 - 90, fee);
    assert_eq!((next.called, call.time), (T0 - 90, T0 - 90));
    assert_eq!((out_of(&ap, &hash_spk(&w.lib, &call.hash())).1, &ap.tx.outputs[0].script_public_key), (CALL_HOLD as u64, &hash_spk(&w.lib, &next.hash())), "the call says the statement's time");
    // The same, landed a block later: not applied, not disputed, ended.
    let later = OLive { daa: 6_511, ..live.clone() };
    assert!(!accepted(&landed(&b, 0, 6_511), &w.keys) && stated_settle(&w.lib, &later, &settle, w.payer(&w.f, &ff)).is_err(), "a call that landed more than a lateness after its time is applied");
    let (db, _) = point_dispute(&w.lib, &live, &point, xk(&w.f), 0, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&db, &w.keys) && !accepted(&landed(&db, 0, 6_511), &w.keys) && point_dispute(&w.lib, &later, &point, xk(&w.f), 0, w.payer(&w.f, &ff)).is_err());
    let lb = stated_late(&w.lib, &later, &late_s, xk(&w.f), w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&lb, &w.keys) && stated_late(&w.lib, &live, &late_s, xk(&w.f), w.payer(&w.f, &ff)).is_err());
    // A call of a time a block ahead of its landing.
    let (b, st, _) = shard_call(&w.lib, &shard, &change, up, 6_511, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    let ahead = nth(&signed("a call of the time 6,511, stated", &b, &w.keys), 0, st, 6_510);
    assert!(stated_settle(&w.lib, &ahead, &settle, w.payer(&w.f, &ff)).is_err() && point_dispute(&w.lib, &ahead, &point, xk(&w.f), 0, w.payer(&w.f, &ff)).is_err());
    let b = stated_settle(&w.lib, &OLive { daa: 6_511, ..ahead.clone() }, &settle, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys) && !accepted(&landed(&b, 0, 6_510), &w.keys), "a call of a time ahead of its landing is applied");
    assert!(accepted(&stated_late(&w.lib, &ahead, &late_s, xk(&w.f), w.payer(&w.f, &ff)).unwrap(), &w.keys));

    // What the caller's choice does to a leg.
    let base = seeded(&w);
    let long = fleg(&w, 0, 10, LONG, P10);
    let books = OLive { s: Shard { held: 20 * KAS1, ..base.s.booked(&long).unwrap() }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let p13 = [P10 * 13 / 10, 0, 0, 0];
    let step_s = w.script("KpfTCStep");
    let asks = |entered_at: i64, call_time: i64| -> i64 {
        let a = cov(&w, &books, &[leg_entered(&long, entered_at)], 40 * KAS1);
        let (_, call) = called_at(&books, p13, call_time, call_time as u64 + 60);
        let (b, st) = call_step(&w.lib, &a, &a.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
        assert!(accepted(&b, &w.keys));
        st.note.asked
    };
    assert_eq!((asks(6_150, 6_500), asks(6_150, 6_410)), (30 * KAS1, 0), "L3: a leg 360 blocks old where the call landed is young by the time the caller says");
    assert_eq!((asks(6_410, 6_710), asks(6_410, 6_610), asks(6_410, 6_709)), (30 * KAS1, 0, 0), "L2: an entry a lateness back is old 200 blocks after its landing");
}

/// Finding L1 (Medium). The rule of 17.9 is in `KpfTCStep` and nowhere else: a leg that leaves a called shard's
/// account by a STATEMENT asks what a leaving asks, whatever its time. F95 (open by decision) is the way: a
/// leaving stated after the call's time is ended by a step within its dispute period, and applied where nobody
/// steps. An account's user states nothing before the account's time, which an entry writes 400 blocks after its
/// own (T is 300): so the path is open from 400 blocks after the entry, call or no call.
/// Here: a long of 10 units from 10 that entered at 6,201; a call of the time 6,500 at 12 (the leg is young by a
/// block: the step asks nothing, run). At 6,601 its user states its leaving at 15 (not a block sooner: run). A
/// step over the statement asks nothing (run: the answer, within 50 blocks). Nobody steps: applied at 6,651, its
/// note asks 48.5 KAS, and applied to the called shard it is a claim of 48.5 KAS, at one share with every honest
/// claim. Passes while the leaving's note is counted as a claim.
/// The time whoever watches has: the call is an object no sooner than a dispute period after its time, and its
/// owner's own dispute of it at the last block holds that to a lateness and two dispute periods; the account
/// must be stepped between then and the leaving's settling. For the owner's leg that made the call due (entered
/// at the call's time) that is 450 - 200 = 250 blocks; for a leg that entered T before the call, 151 - 200: none.
#[test]
fn t3_finding_l1_a_young_legs_gain_is_a_claim_where_its_user_leaves_by_statement_and_nobody_steps() {
    let w = world();
    let base = seeded(&w);
    let t = t_young(&w);
    let (ff, fw) = ([fund(xk(&w.f), 36)], [fund(xk(&w.w), 37)]);
    let raw = fleg(&w, 0, 10, LONG, P10);
    let te = T0 - t + 1;
    let quiet = te + 2 * (w.terms().late_daa + 2 * 50);
    assert_eq!((te, quiet), (6_201, 6_601));
    let shard = OLive { s: Shard { held: 10 * KAS1, ..base.s.booked(&raw).unwrap() }, value: (10 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let x = cov(&w, &shard, &[leg_entered(&raw, te)], 50 * KAS1);
    let acct = OLive { s: Account { b_min_counter: quiet, ..x.s.clone() }, ..x };
    let (called, call) = called_objs(&shard, [P10 * 12 / 10, 0, 0, 0], 6_560);
    let step_s = w.script("KpfTCStep");
    // Stepped: the young leg asks nothing.
    let (b, at_call) = call_step(&w.lib, &acct, &acct.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys));
    assert_eq!((at_call.gain, at_call.note.asked), (0, 0));
    // Its user's leaving at 15, at the account's time and not before.
    let change = w.script("KpfTChange");
    let at15 = [P10 * 15 / 10];
    assert!(c_state(&w.lib, &acct, &change, KIND_C_LEAVE, 0, ALL, &at15, quiet - 1, xk(&w.w), w.payer(&w.w, &fw)).is_err(), "a leaving before the account's time");
    let (b, st, _, o) = c_state(&w.lib, &acct, &change, KIND_C_LEAVE, 0, ALL, &at15, quiet, xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    let live = nth(&signed("L1: the user's leaving of a young leg, stated under the call", &b, &w.keys), 0, st, quiet as u64);
    // (The answer: within its period anyone steps the account over it, and the leg asks nothing.)
    let (sb, over) = call_step(&w.lib, &live, &acct.s, Some((&st, None)), &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&sb, &w.keys) && over.note.asked == 0, "(the answer) a step over the statement");
    // Nobody did: applied, and its note is a claim on the called shard.
    let ab = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &fw)).unwrap();
    let ap = signed("L1: the leaving applied", &ab, &w.keys);
    let note = o.note.clone().expect("the note that the leg left");
    let k = out_of(&ap, &hash_spk(&w.lib, &note.hash())).0;
    let (nb, next, claim) = left_apply(&w.lib, &called, &nth(&ap, k, note.clone(), quiet as u64 + 50), &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    signed("L1: its note applied to the called shard", &nb, &w.keys);
    let claim = claim.expect("a claim");
    println!("T3 L1: a leg that is young at the call: its step asks {} sompi; its user's leaving, stated 101 blocks after the call's time and applied 50 later, asks {}: a claim of {} on the called shard (owed by claims {})", at_call.note.asked, note.asked, claim.amount, next.owed);
    assert_eq!((note.asked, claim.amount, next.owed, next.rows), (4_850_000_000, 4_850_000_000, 4_850_000_000, [ShardRow::EMPTY; 4]), "L1 is closed: the leaving of a young leg is no claim");
}

// ================================================================================================================
// B4. The committee shown apart.

/// B4, `KpfTStepPrice` on a covered account's leg (kind 8, a leaving): whoever answers shows the seed of the
/// account's shard, and the citation's committee is held into the id with it. At the engine:
///  - the right seed: answered right (the control); every other seed shown with the same transaction is refused
///    (nothing, another, the committee's id, the shard's id);
///  - the same answer for an account that differs in a number the id is not of: taken (the control of the way
///    the next two are written: the object, the moving part shown, and the statement written again);
///  - an account under the shard of ANOTHER SEED and the same committee: answered with its own seed, taken; with
///    this shard's seed, refused;
///  - an account under a shard of another committee (the same seed): the citation of this world's committee
///    answers no price of it, whatever seed is shown.
#[test]
fn t3_b4_a_price_of_a_covered_account_is_answered_with_the_seed_of_its_own_shard_and_its_committees_citation() {
    let w = world();
    let base = seeded(&w);
    let raw = fleg(&w, 0, 10, LONG, P10);
    let leg = leg_entered(&raw, 0);
    let shard = OLive { s: base.s.booked(&raw).unwrap(), ..base.clone() };
    let acct = cov(&w, &shard, &[leg], 100 * KAS1);
    let a = [P10 * 12 / 10];
    let f = [fund(xk(&w.w), 36)];
    let (live, s, _, _) = c_applied(&w, "a leaving", &acct, KIND_C_LEAVE, 0, 1, &a, &w.w);
    let (cite1, price_s) = (w.script("KpfTCite"), w.script("KpfTStepPrice"));
    let q = w.pointed(&live, 2);
    let near = w.cited(0, a[0] * 1_004 / 1_000);
    let (b, right) = c_price_seeded(&w.lib, &q, &acct.s, &s, &price_s, (&cite1, &near), SEED, xk(&w.f), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    signed("a price of a covered account's futures leg, answered with its shard's seed", &b, &w.keys);
    let n = args_of(&mut b.clone(), 1).len();
    assert!(matches!(&args_of(&mut b.clone(), 1)[n - 1], Arg::V(ArtifactValue::Bytes(x)) if x[..] == SEED[..]), "the seed is the last argument");
    let committee = w.markets[0].committee_id;
    let sid_of = |seed: &[u8; 32], committee: &[u8; 32]| kpf_protocol::reserve::shard_sid(seed, committee, &acct.s.terms(), &acct.s.moving()[93..120]);
    assert_eq!(acct.s.shard, Some(sid_of(&SEED, &committee)));
    for other in [[0u8; 32], [0x5f; 32], committee, shard.s.sid] {
        assert!(c_price_seeded(&w.lib, &q, &acct.s, &s, &price_s, (&cite1, &near), other, xk(&w.f), w.payer(&w.w, &f), None).is_err(), "the builder answers with another seed");
        assert!(!accepted(&with_arg(&b, n - 1, bytes_arg(&other)), &w.keys), "a price of a covered account answered with another seed shown");
    }
    // The same answer written for another account: the object disputed, the moving part shown (argument 5), the
    // seed, and the statement as it stands again.
    let for_account = |x: &Account, seed: [u8; 32]| {
        let st2 = Stated { position: x.hash(), ..live.s };
        let mut t = b.clone();
        object_made(&mut t, &w.lib, 0, &Pointed { st: st2, disputer: q.s.disputer, point: q.s.point }.hash());
        args_of(&mut t, 1)[5] = bytes_arg(&x.moving());
        args_of(&mut t, 1)[n - 1] = bytes_arg(&seed);
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &st2.again().hash());
        t
    };
    assert!(accepted(&for_account(&acct.s, SEED), &w.keys), "(control) written again for the same account");
    assert!(accepted(&for_account(&Account { expiry: 77, ..acct.s.clone() }, SEED), &w.keys), "(control) an account that differs in a number the id is not of");
    let seed2 = [0x5f; 32];
    let sister = Account { shard: Some(sid_of(&seed2, &committee)), ..acct.s.clone() };
    assert!(accepted(&for_account(&sister, seed2), &w.keys), "an account of the shard of another seed, answered with that seed");
    assert!(!accepted(&for_account(&sister, SEED), &w.keys), "an account of another shard of the same committee, answered with this shard's seed");
    let foreign = Account { shard: Some(sid_of(&SEED, &[0x77; 32])), ..acct.s.clone() };
    for seed in [SEED, [0x77; 32], committee, [0u8; 32]] {
        assert!(!accepted(&for_account(&foreign, seed), &w.keys), "a citation of another committee than the shard's answers a price of its account");
    }
}

/// B4, the rows. A leg's row is found by its market's name, its scale and its price's limit, and not by the
/// committee in the row's head (`KpfTCStep`, `KpfTLeft`, `KpfTLeftAll`, `KpfTLeftVoid`). What that rests on is
/// that a shard has one row a market, each with the shard's one committee in its head, which `KpfTEnterS` holds
/// (`t1b_fixed_f80_...` in `auditT1_builders.rs`: another committee shown, a head written with another, a row of
/// another committee entered or doubled, all refused). Run here on objects no script writes, so that it is known
/// what the change gave up:
///  - a shard whose row of the leg's market says another committee in its head: the note of a leg of that market
///    is applied and takes the units out (it was "a leg of no row" before the change);
///  - a call whose body has the leg's market in two rows, at 13 and at 20: the step names either, and the note
///    asks 30 KAS or 100 KAS as whoever steps chooses.
#[test]
fn t3_b4_a_row_is_found_without_its_committee_and_what_holds_one_row_a_market() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let raw = fleg(&w, 0, 10, LONG, P10);
    let leg = leg_entered(&raw, 0);
    // A row of another committee, and the note of a leg of its market.
    let booked = base.s.booked(&raw).unwrap();
    let mut odd = booked;
    odd.rows[0].head.committee = [0x99; 32];
    let odd = OLive { s: odd, ..base.clone() };
    let note = Left { sid: odd.s.sid, legs: vec![leg], key: xk(&w.w), asked: 0 };
    let note_live = OLive { s: note.clone(), value: NOTE_MIN as u64, id: odd.id, op: op(75), daa: 6_050 };
    let (b, next, rest) = left_apply(&w.lib, &odd, &note_live, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    signed("a note of a leg applied to a row whose head says another committee", &b, &w.keys);
    assert_eq!((next.rows, next.basis, rest), ([ShardRow::EMPTY; 4], 0, None));
    // (And it is not spent as a note of no leg of the books.)
    let void = left_void(&w.lib, &odd, &note_live, 0, &w.script("KpfTLeftVoid"), w.payer(&w.f, &ff));
    assert!(void.is_err() || !accepted(&void.unwrap(), &w.keys), "a note of a leg of the books is spent");

    // Two rows of one market in a call's body.
    let shard = OLive { s: Shard { held: 20 * KAS1, ..booked }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let (_, call) = called_objs(&shard, [P10 * 13 / 10, 0, 0, 0], 6_600);
    let two = OLive { s: Called { heads: [w.head(0), w.head(0), Head::EMPTY, Head::EMPTY], prices: [P10 * 13 / 10, P10 * 2, 0, 0], ..call.s }, ..call.clone() };
    let a = cov(&w, &shard, &[leg], 40 * KAS1);
    let step_s = w.script("KpfTCStep");
    let (b, first) = call_step(&w.lib, &a, &a.s, None, &two, &step_s, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys));
    assert_eq!((first.at.clone(), first.note.asked), (vec![0], 30 * KAS1));
    let at20 = a.s.call_step(&Called { prices: [P10 * 2, P10 * 2, 0, 0], ..two.s }, a.value as i64).unwrap();
    let second = CallStep { at: vec![1], ..at20 };
    assert_eq!(second.note.asked, 100 * KAS1);
    let b = call_step_as(&w.lib, &a, &a.s, None, &two, &second, &step_s, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys), "(of a call no script writes) the step names the second row of the leg's market");
    // (A row of the market's name and another scale or limit is no row of the leg: the step is refused.)
    let mut other_scale = two.clone();
    other_scale.s.heads[1].scale += 1;
    let b = call_step_as(&w.lib, &a, &a.s, None, &other_scale, &second, &step_s, w.payer(&w.f, &ff));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a row of another scale");
}

// ================================================================================================================
// B5. Sizes.

/// B5: the six scripts that changed, in bytes, and the two transactions the change made longer. Before (commit
/// 73176f7, by the previous reviewer's tests in `auditT1_builders.rs`): KpfTCStep 4,113; KpfTEnterS 2,323;
/// KpfTLeft 2,212; KpfTLeftAll 3,825; KpfTLeftVoid 1,747; KpfTStepPrice 1,825. A step's transaction is its script
/// longer (716 bytes); an entry's stating is its script longer and an argument of 33 bytes more (77 bytes).
#[test]
fn t3_b5_the_sizes_of_the_scripts_that_changed_and_of_an_entry_and_a_step() {
    let w = world();
    let kit = kpf_protocol::thin_artifacts::kit();
    let sizes: Vec<(&str, usize)> = ["KpfTCStep", "KpfTEnterS", "KpfTLeft", "KpfTLeftAll", "KpfTLeftVoid", "KpfTStepPrice"].iter().map(|n| (*n, kit.script(n).unwrap().len())).collect();
    // (Second round, the books' hash, 17.8a: KpfTEnterS 2,463 from 2,367; KpfTLeft 2,374 from 2,220; KpfTLeftAll
    // 4,226 from 3,873; KpfTLeftVoid 2,017 from 1,955. And the others of the nine, from the artifacts of a1cd15a:
    // KpfTShardOpen 730 from 679; KpfTCallPay 1,691 from 1,562; KpfTShardEnd 3,234 from 3,162; KpfTShardPrice
    // 1,810 from 1,772; KpfTCallEnd 3,695 from 3,622.)
    assert_eq!(sizes, vec![("KpfTCStep", 4829), ("KpfTEnterS", 2463), ("KpfTLeft", 2374), ("KpfTLeftAll", 4226), ("KpfTLeftVoid", 2017), ("KpfTStepPrice", 1821)]);
    let books: Vec<(&str, usize)> = ["KpfTShardOpen", "KpfTCallPay", "KpfTShardEnd", "KpfTShardPrice", "KpfTCallEnd"].iter().map(|n| (*n, kit.script(n).unwrap().len())).collect();
    assert_eq!(books, vec![("KpfTShardOpen", 730), ("KpfTCallPay", 1691), ("KpfTShardEnd", 3234), ("KpfTShardPrice", 1810), ("KpfTCallEnd", 3695)]);
    let base = seeded(&w);
    let (f, ff) = ([fund(xk(&w.w), 33)], [fund(xk(&w.f), 36)]);
    let leg = fleg(&w, 0, 10, LONG, P10);
    let acct = cov(&w, &base, &[], 20 * KAS1);
    let (b, ..) = c_enter_state(&w.lib, &acct, &base, &w.script("KpfTEnterS"), SEED, leg, 5 * KAS1, &[], T0, w.payer(&w.w, &f)).unwrap();
    let entry = signed("an entry stated", &b, &w.keys);
    let legs6: Vec<XLeg> = (0..6).map(|i| leg_entered(&fleg(&w, i % 2, 1, if i % 2 == 0 { LONG } else { SHORT }, P10), if i < 3 { 0 } else { T0 })).collect();
    let mut books = base.s;
    for l in &legs6 {
        books = books.booked(l).unwrap();
    }
    let shard = OLive { s: Shard { held: 20 * KAS1, ..books }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let (_, call) = called_objs(&shard, [P10 * 13 / 10, P10, 0, 0], 6_600);
    let mut steps = vec![];
    for (what, legs, coins) in [("one leg, a note's least", vec![legs6[0]], NOTE_MIN), ("one leg, 40 KAS", vec![legs6[0]], 40 * KAS1), ("six legs, three of them young, 40 KAS", legs6.clone(), 40 * KAS1)] {
        let a = cov(&w, &shard, &legs, coins);
        let (b, _) = call_step(&w.lib, &a, &a.s, None, &call, &w.script("KpfTCStep"), w.payer(&w.f, &ff)).unwrap();
        let sg = signed(&format!("a step under a call: {what}"), &b, &w.keys);
        steps.push((what, sg.mass.size, sg.mass.compute, sg.mass.storage));
    }
    println!("T3 B5: the scripts {sizes:?}; an entry stated: {} bytes (compute {}, storage {}); steps (what, bytes, compute, storage): {steps:?}", entry.mass.size, entry.mass.compute, entry.mass.storage);
    assert_eq!((entry.mass.size, steps[0].1, steps[1].1, steps[2].1), (5_040, 6_890, 6_942, 7_538));
}

// ================================================================================================================
// What the rule does to an honest user, and what a statement nobody disputes does to the rule.

/// Finding L4 (Medium; the rule is the paper's own, N79: run here because the paper's reason is no script's, and
/// because of what the change made of this play). "An owner does not gain by bringing it about (it loses its
/// shard)" (17.9). At the engine an owner brings a call about with a leg of its own, for fees, and has its shard
/// afterwards, open, with what the young legs gained in it.
/// A shard that holds 20 KAS and one user: long 5 units from 10, entered 100 blocks ago. The price is 13: the user
/// gained 15 KAS. The shard is not due for a call (its equity of 5 KAS is above its lower level), and the user's
/// leaving would be paid 14.35 KAS, whole (the model). The owner signs in a long of 1,000 units of its own, from
/// 13: the shard is due at once. Called (by the owner, or by whoever watches, as 17.9 asks of them): both legs are
/// young. The user's step asks nothing; the owner's asks nothing, and its coins go back to it; both notes
/// applied, the books are empty and nothing is owed; the call ends, and the shard is the owner's again, not
/// called, holding 19.9 KAS. The user has its coins less the fee and a note's least, and nothing of its 15 KAS.
/// The same play on the same user with a leg that entered T before the call, which is what the scripts did for
/// every leg before the change: the user's note asks 15 KAS, the owner's leg (it gained nothing) asks nothing,
/// and the claim is paid 15 KAS, whole. So for a leg younger than T this play costs the user everything it
/// gained where before the change it cost it nothing; and what it gained is the owner's.
/// Passes while the shard ends open and unowing, having paid the young user nothing.
#[test]
fn t3_finding_l4_an_owner_brings_a_call_about_with_its_own_leg_and_keeps_what_young_legs_gained() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let fee = base.s.t.keeper_fee_min;
    let p13 = [P10 * 13 / 10, 0, 0, 0];
    let (user_leg, big) = (fleg(&w, 0, 5, LONG, P10), fleg(&w, 0, 1_000, LONG, p13[0]));
    let alone = Shard { held: 20 * KAS1, ..base.s.booked(&user_leg).unwrap() };
    let with_own = Shard { held: 20 * KAS1, ..alone.booked(&big).unwrap() };
    assert!(!alone.call_due(&p13) && with_own.call_due(&p13), "the owner's own leg makes the call due");
    // (With no call: the user's leaving at 13 asks 15 KAS less the band of 65, and the shard pays it whole.)
    let young_user = leg_entered(&user_leg, T0 - 100);
    let shard0 = OLive { s: alone, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let a_u0 = cov(&w, &shard0, &[young_user], 20 * KAS1);
    let (_, _, o) = a_u0.s.c_state(KIND_C_LEAVE, 0, ALL, &[p13[0]], T0, a_u0.value as i64, xk(&w.w)).unwrap();
    let leaving = o.note.unwrap();
    let (_, _, paid, rest) = alone.left(&leaving).unwrap();
    assert_eq!((leaving.asked, paid, rest), (1_435_000_000, 1_435_000_000, None));

    let shard = OLive { s: with_own, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let (called, call) = called_objs(&shard, p13, 6_600);
    let step_s = w.script("KpfTCStep");
    let pay_s = w.script("KpfTCallPay");
    let a_o = own_acct(&w, &shard, &[leg_entered(&big, T0 - 1)], 6_000 * KAS1, 81);
    for (what, entered_at, asks) in [("young", T0 - 100, 0), ("old, as every leg was before the change", T0 - t_young(&w), 15 * KAS1)] {
        let a_u = cov(&w, &shard, &[leg_entered(&user_leg, entered_at)], 20 * KAS1);
        let (b, s_u) = call_step(&w.lib, &a_u, &a_u.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
        let sg_u = signed(&format!("L4, the user's leg {what}: the user's account stepped"), &b, &w.keys);
        let (b, s_o) = call_step(&w.lib, &a_o, &a_o.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
        let sg_o = signed(&format!("L4, the user's leg {what}: the owner's account stepped"), &b, &w.keys);
        assert_eq!((s_u.note.asked, s_u.user, s_o.note.asked, s_o.user), (asks, 20 * KAS1 - NOTE_MIN - fee, 0, 6_000 * KAS1 - NOTE_MIN - fee), "{what}");
        assert!(pays(&sg_u, &xk(&w.w), s_u.user) && pays(&sg_o, &xk(&w.c), s_o.user));
        let (b, sh1, c_u) = left_apply(&w.lib, &called, &nth(&sg_u, 2, s_u.note.clone(), 6_610), &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
        let sg1 = signed("L4: the user's note applied", &b, &w.keys);
        let sh1 = nth(&sg1, 0, sh1, 6_620);
        let (b, sh2, c_o) = left_apply(&w.lib, &sh1, &nth(&sg_o, 2, s_o.note.clone(), 6_610), &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
        let sh2 = nth(&signed("L4: the owner's note applied", &b, &w.keys), 0, sh2, 6_630);
        assert_eq!((c_o, sh2.s.rows, sh2.s.owed, sh2.s.held), (None, [ShardRow::EMPTY; 4], asks, 20 * KAS1 - fee), "{what}");
        if let Some(claim) = c_u {
            // (Before the change: the user's claim, paid whole.)
            let (b, _, got) = claim_pay(&w.lib, &sh2, &nth(&sg1, 2, claim, 6_620), &pay_s, w.payer(&w.f, &ff)).unwrap();
            let sg = signed("L4: the old leg's claim paid", &b, &w.keys);
            assert!(got == 15 * KAS1 && pays(&sg, &xk(&w.w), got), "{what}");
            continue;
        }
        let (b, open) = call_off(&w.lib, &sh2, &call, &pay_s, w.payer(&w.f, &ff)).unwrap();
        let sg = signed("L4: the call ends", &b, &w.keys);
        println!("T3 L4: a user's young leg gained 15 KAS (with no call its leaving is paid {paid} sompi, whole; with a leg older than T under the same call, 1500000000); the owner's own leg makes the call due; after the call the user has {} of its 2,000,000,000 sompi of coins and nothing else, and the shard is open, holds {} and owes {}", s_u.user, open.held, open.owed);
        assert_eq!((open.called, open.owed, open.held, open.rows), (0, 0, 20 * KAS1 - fee, [ShardRow::EMPTY; 4]), "L4 is closed: the owner does not have its shard and the young gains after the call");
        assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (sh2.value + CALL_HOLD as u64, &hash_spk(&w.lib, &open.hash())));
    }
}

/// B1, what holds the time between the statement of an entry and the account it becomes. `KpfTEnterS` holds the
/// leg SAID to the statement's time, and writes the marked account with it; the account the statement becomes
/// (`outs`) it takes as given, its first byte read. That account is held to the marked account's legs by the
/// answer of the statement's end (`KpfTCEnter`), which is made only where someone disputes it. Run: an entry
/// whose account-to-be has the leg saying the time 0 is stated (taken by `KpfTEnterS`); the model names its end
/// as the wrong point; disputed there it is answered wrong (and not right); not disputed for a dispute period it
/// is applied, and the account stands with a leg that says 0: old at every call.
/// The same holds of every statement on an account (a take's, a leaving's, a split's, a mark's `outs` are the
/// claimer's words until disputed): it is the form's rule, and no finding of this change. It is said here
/// because the rule of 17.9 now rests on it for the owner's own account: whoever watches disputes, within a
/// dispute period (50 blocks), every statement on an account that does not write its legs as they were.
#[test]
fn t3_b1_the_account_an_entry_becomes_is_held_to_the_leg_said_where_its_end_is_disputed() {
    let w = world();
    let shard = seeded(&w);
    let acct = cov(&w, &shard, &[], 20 * KAS1);
    let leg = fleg(&w, 0, 10, LONG, P10);
    let (f, ff) = ([fund(xk(&w.w), 33)], [fund(xk(&w.f), 36)]);
    let enter = w.script("KpfTEnterS");
    let (_, st, said, marked, after, next) = c_enter_state(&w.lib, &acct, &shard, &enter, SEED, leg, 5 * KAS1, &[], T0, w.payer(&w.w, &f)).unwrap();
    assert_eq!(leg_time(&after.legs[0]), T0);
    let aged = Account { legs: vec![leg_entered(&leg, 0)], ..after.clone() };
    let lie = Stated { outs: Outs::of(&[Out { key: false, to: aged.hash(), value: st.held }]).unwrap(), ..st };
    let b = c_enter_state_as(&w.lib, &acct, &shard, &enter, SEED, &lie, &said, &next, w.payer(&w.w, &f)).unwrap();
    let sg = signed("an entry that becomes an account whose leg says another time, stated", &b, &w.keys);
    assert_eq!(Account::c_enter_wrong_point(&marked, &said, &lie.claimer, &lie.outs, lie.held, T0, &[P10]), Some(0));
    let live = nth(&sg, 0, lie, 6_510);
    // Disputed at its end: wrong.
    let q = w.pointed(&live, 0);
    let (b, right) = c_enter_end(&w.lib, &q, &marked, &said, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(!right);
    signed("its end answered wrong", &b, &w.keys);
    let (b, _) = c_enter_end(&w.lib, &q, &marked, &said, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.f, &ff), Some(true)).unwrap();
    assert!(!accepted(&b, &w.keys));
    // Not disputed: applied. The account's leg is old from its first block.
    let b = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.f, &ff)).unwrap();
    let ap = signed("not disputed, it is applied", &b, &w.keys);
    assert_eq!(&ap.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &aged.hash()));
    let books = OLive { s: Shard { held: 20 * KAS1, ..next }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..shard.clone() };
    let (_, call) = called_objs(&books, [P10 * 13 / 10, 0, 0, 0], 6_600);
    let stood = nth(&ap, 0, aged.clone(), 6_560);
    let (b, step) = call_step(&w.lib, &stood, &stood.s, None, &call, &w.script("KpfTCStep"), w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys) && step.note.asked == 30 * KAS1, "a leg that entered at 6,500 by a statement nobody disputed asks its gain at a call of 6,500");
}

// ================================================================================================================
// Second round: the books' hash (docs 17.8a, commit 1 of four). A shard's fixed part is the hash of its terms and of
// its books: the sum of the rows' gross units, the hash of the rows, and the hash of the claims' numbers.

use kpf_protocol::reserve::Rest;
use kpf_protocol::state::sm8;

fn p33b(h: &[u8; 32]) -> Vec<u8> {
    [&[0x20u8][..], &h[..]].concat()
}
/// The 41 bytes a script that shows the rows is shown beside them: the sum of the gross (a push of eight bytes) and
/// the hash of the claims' numbers.
fn head_of(gross: i64, claims: &[u8; 32]) -> Vec<u8> {
    [&[0x08u8][..], &sm8(gross)[..], &claims[..]].concat()
}
/// The state's hash of the shard `s` with the 41 bytes `head` where its books' own would be, written out as the
/// scripts hash it (the reviewer's own: `Shard::hash` is asserted against it).
fn shard_hash_with(s: &Shard, head: &[u8]) -> [u8; 32] {
    let books = blake2b(&[&head[..9], &p33b(&blake2b(&s.rows_bytes()))[..], &[0x20u8][..], &head[9..41]].concat());
    blake2b(&[p33b(&blake2b(&[s.terms(), p33b(&books)].concat())), s.moving()].concat())
}
/// The state's hash of the shard `s` in the form of before this change: its fixed part of its terms and the hash
/// of its rows alone.
fn shard_hash_old(s: &Shard) -> [u8; 32] {
    blake2b(&[p33b(&blake2b(&[s.terms(), p33b(&blake2b(&s.rows_bytes()))].concat())), s.moving()].concat())
}
fn units(s: &Shard) -> i64 {
    s.rows.iter().map(|r| r.gross).sum()
}
fn coin(key: [u8; 32], n: u8, daa: i64) -> KeyUtxo {
    KeyUtxo { daa: daa as u64, ..fund(key, n) }
}
fn note_of<T>(shard: &OLive<Shard>, s: T, value: i64, n: u8) -> OLive<T> {
    OLive { s, value: value as u64, id: shard.id, op: op(n), daa: 6_050 }
}

/// One transaction of each entry that shows a shard's rows, as its builder writes it: (what, the transaction, the
/// place of the 41 bytes among the script's arguments, the input that is the shard or the statement on it, the
/// shard shown, and the object's hash for a shard of a given state's hash).
#[allow(clippy::type_complexity)]
fn shows_the_rows(w: &World) -> Vec<(&'static str, OBuilt, usize, usize, Shard, Box<dyn Fn([u8; 32]) -> [u8; 32]>)> {
    let same = || -> Box<dyn Fn([u8; 32]) -> [u8; 32]> { Box::new(|h| h) };
    let (f, ff, fc) = ([fund(xk(&w.w), 33)], [fund(xk(&w.f), 36)], [fund(xk(&w.c), 30)]);
    let mut v: Vec<(&'static str, OBuilt, usize, usize, Shard, Box<dyn Fn([u8; 32]) -> [u8; 32]>)> = vec![];
    let base = seeded(w);
    let (l0, l1) = (fleg(w, 0, 10, LONG, P10), fleg(w, 1, 4, SHORT, P10));
    let booked = OLive { s: base.s.booked(&l0).unwrap().booked(&l0).unwrap().booked(&l1).unwrap(), ..base.clone() };
    // KpfTEnterS.state: a leg into a row that is there.
    let acct = cov(w, &booked, &[], 20 * KAS1);
    let (b, ..) = c_enter_state(&w.lib, &acct, &booked, &w.script("KpfTEnterS"), SEED, l0, 5 * KAS1, &[], T0, w.payer(&w.w, &f)).unwrap();
    v.push(("KpfTEnterS.state", b, 7, 2, booked.s, same()));
    // KpfTLeft.apply and KpfTLeftAll.apply.
    let one = note_of(&booked, Left { sid: booked.s.sid, legs: entered(&[l1], 0), key: xk(&w.w), asked: 0 }, NOTE_MIN, 75);
    let (b, ..) = left_apply(&w.lib, &booked, &one, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    v.push(("KpfTLeft.apply", b, 5, 0, booked.s, same()));
    let two = note_of(&booked, Left { sid: booked.s.sid, legs: entered(&[l0, l1, l0], 7), key: xk(&w.w), asked: 0 }, NOTE_MIN, 75);
    let (b, ..) = left_apply(&w.lib, &booked, &two, &w.script("KpfTLeftAll"), w.payer(&w.f, &ff)).unwrap();
    v.push(("KpfTLeftAll.apply", b, 5, 0, booked.s, same()));
    // KpfTLeftVoid.void: a note of a leg of a market that has no row.
    let ghost = note_of(&booked, Left { sid: booked.s.sid, legs: entered(&[fleg(w, 2, 3, SHORT, P10)], 0), key: xk(&w.w), asked: 0 }, NOTE_MIN, 75);
    let b = left_void(&w.lib, &booked, &ghost, 0, &w.script("KpfTLeftVoid"), w.payer(&w.f, &ff)).unwrap();
    v.push(("KpfTLeftVoid.void", b, 5, 0, booked.s, same()));
    // KpfTLeft.rest: a note of the rest, of a shard that owes it and is not called.
    let owing = OLive { s: Shard { owed: 20 * KAS1, ..booked.s }, ..booked.clone() };
    let rest = Rest { sid: owing.s.sid, key: xk(&w.w), amount: 20 * KAS1 };
    let (b, ..) = rest_apply(&w.lib, &owing, &note_of(&owing, rest, rest.value(), 96), &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    v.push(("KpfTLeft.rest", b, 5, 0, owing.s, same()));
    // KpfTCallPay.claim and .off: a called shard of no row.
    let called = OLive { s: Shard { owed: 20 * KAS1, called: T0, ..base.s }, ..base.clone() };
    let (b, ..) = claim_pay(&w.lib, &called, &note_of(&called, Rest { sid: called.s.sid, ..rest }, rest.value(), 96), &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).unwrap();
    v.push(("KpfTCallPay.claim", b, 5, 0, called.s, same()));
    let done = OLive { s: Shard { owed: 0, ..called.s }, ..called.clone() };
    let call = OLive { s: Called { sid: done.s.sid, time: T0, heads: [Head::EMPTY; 4], prices: [0; 4] }, value: CALL_HOLD as u64, id: done.id, op: op(91), daa: 6_600 };
    let (b, _) = call_off(&w.lib, &done, &call, &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).unwrap();
    v.push(("KpfTCallPay.off", b, 5, 0, done.s, same()));
    // The answers of a draw's end and of a row's price, and of a call's end: the object is the disputed point of a
    // statement on the shard.
    let shard = w.shard();
    let s = shard.s;
    let prices = [P10, P10, 0, 0];
    let (_, st, d) = shard_draw(&w.lib, &shard, &w.script("KpfTChange"), KAS1, prices, T0, w.payer(&w.c, &fc)).unwrap();
    let live = w.stated(&shard, &st, &d.said());
    let pointed_of = |st: Stated, point: i64| -> Box<dyn Fn([u8; 32]) -> [u8; 32]> { Box::new(move |h| Pointed { st: Stated { position: h, ..st }, disputer: xk(&kp(3)), point }.hash()) };
    let q = w.pointed(&live, 0);
    assert_eq!(pointed_of(st, 0)(s.hash()), q.s.hash());
    let (b, right) = shard_end(&w.lib, &q, &s, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    v.push(("KpfTShardEnd.answer", b, 5, 0, s, pointed_of(st, 0)));
    let q = w.pointed(&live, 1);
    let c = w.cited(0, P10);
    let (b, right) = shard_price(&w.lib, &q, &s, &d, 1, &w.script("KpfTShardPrice"), Some((&w.script("KpfTCite"), &c)), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    v.push(("KpfTShardPrice.answer", b, 5, 0, s, pointed_of(st, 1)));
    let up = [P10 * 11 / 10, P10, 0, 0];
    let (_, st, d) = shard_call(&w.lib, &shard, &w.script("KpfTChange"), up, T0, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    let q = w.pointed(&w.stated(&shard, &st, &d.said()), 0);
    let (b, right) = call_end(&w.lib, &q, &s, &d, &w.script("KpfTCallEnd"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(right);
    v.push(("KpfTCallEnd.answer", b, 5, 0, s, pointed_of(st, 0)));
    v
}

/// Second round, B (1) and (3). Each of the ten entries of the eight scripts that show a shard's rows, in a
/// transaction of its builder (taken: the control): the argument after the rows is the shard's 41 bytes; shown
/// another sum (one more, one less, nothing, the largest, below nothing), another hash of the claims (a bit, all
/// of nothing), another first byte, or the 41 bytes of another shard: refused. With the shard of the OLD form in
/// its place (its fixed part of the terms and the rows' hash alone; for an answer, the statement on such a
/// shard), with the right 41 bytes and with any of the others: refused. An account, and a note, in the shard's
/// place: refused.
#[test]
fn t3r2_every_script_that_shows_the_rows_holds_the_41_bytes_and_takes_no_shard_of_the_old_form() {
    let w = world();
    let cases = shows_the_rows(&w);
    assert_eq!(cases.len(), 10);
    let stray_account = cov(&w, &seeded(&w), &[], 20 * KAS1).s.hash();
    let stray_note = Rest { sid: [0x5d; 32], key: xk(&w.w), amount: KAS1 }.hash();
    let (mut refused, mut old) = (0, 0);
    for (what, b, at, obj, s, obj_of) in &cases {
        assert!(accepted(b, &w.keys), "{what}: the control");
        let head = s.books_head();
        assert_eq!((head.len(), shard_hash_with(s, &head), units(s)), (41, s.hash(), s.gross_sum()), "{what}");
        assert!(matches!(&args_of(&mut b.clone(), 1)[*at], Arg::V(ArtifactValue::Bytes(x)) if x[..] == head[..]) && matches!(&args_of(&mut b.clone(), 1)[*at - 1], Arg::V(ArtifactValue::Bytes(x)) if x[..] == s.rows_bytes()[..]), "{what}: the 41 bytes are argument {at}, after the rows");
        let (g, claims) = (s.gross_sum(), s.claims_hash());
        let mut flipped = claims;
        flipped[31] ^= 1;
        let mut tag = head.clone();
        tag[0] = 0x09;
        let mut heads = vec![head_of(g + 1, &claims), head_of(g - 1, &claims), head_of(i64::MAX, &claims), head_of(-g - 1, &claims), head_of(g, &flipped), head_of(g, &[0; 32]), tag, Shard { rows: [ShardRow { head: w.head(3), net: 0, gross: 77 }, ShardRow::EMPTY, ShardRow::EMPTY, ShardRow::EMPTY], ..*s }.books_head()];
        if g != 0 {
            heads.push(head_of(0, &claims));
        }
        for h in &heads {
            assert!(!accepted(&with_arg(b, *at, bytes_arg(h)), &w.keys), "{what}: shown other 41 bytes than its shard's ({:?})", &h[..9]);
            refused += 1;
        }
        // The shard of the old form, and what is no shard.
        for (which, hash) in [("the shard in the form of before", shard_hash_old(s)), ("an account", stray_account), ("a note", stray_note)] {
            let mut t = b.clone();
            object_made(&mut t, &w.lib, *obj, &obj_of(hash));
            assert!(!accepted(&t, &w.keys), "{what}: {which} in the shard's place");
            for h in heads.iter().take(6) {
                assert!(!accepted(&with_arg(&t, *at, bytes_arg(h)), &w.keys), "{what}: {which} in the shard's place, other 41 bytes shown");
            }
            old += 1;
        }
        // (The control of the way an object is put in a place: the shard's own hash, and the transaction is taken.)
        let mut t = b.clone();
        object_made(&mut t, &w.lib, *obj, &obj_of(s.hash()));
        assert!(accepted(&t, &w.keys), "{what}: (control) the shard put in its own place");
    }
    println!("T3 second round: ten entries that show the rows: {refused} transactions with other 41 bytes refused; {old} objects that are no shard of this form refused in the shard's place, each with six other 41 bytes too");
}

/// Second round, B (2): the sum kept through a shard's life, at the engine. A shard of no row (the sum nothing):
/// a long of 10 units enters a new row (10); a short of 4 enters the row that is there (14); a long of 3 enters a
/// second row (17); each entry's shard written with the sum a unit more or less, or left as it was, is refused.
/// A note of a leg of no row is spent (17, the shard as it is). The shard is called by a statement, applied
/// (`KpfTChangeSettle` carries the hash it is given: the called shard has 17). An account of the long and the
/// short, two legs of ONE row, and an account of the third leg are stepped; the first note is applied by
/// `KpfTLeftAll` (3), the second by `KpfTLeft` (nothing, no row); the claim is paid; the call ends. Every shard
/// the builders return has the sum of its four rows' gross units in its 41 bytes, and the engine takes every
/// transaction: the scripts' running sum is the model's.
#[test]
fn t3r2_the_sum_of_the_gross_is_kept_through_entries_notes_a_call_and_its_end() {
    let w = world();
    let scale = w.markets[0].p.scale;
    let ff = [fund(xk(&w.f), 36)];
    let enter = w.script("KpfTEnterS");
    let mut shard = seeded(&w);
    let said = |s: &Shard| -> i64 { kpf_protocol::state::int8(&s.books_head()[1..9]) };
    assert_eq!((said(&shard.s), units(&shard.s)), (0, 0));
    let legs = [fleg(&w, 0, 10, LONG, P10), fleg(&w, 0, 4, SHORT, P10), fleg(&w, 1, 3, LONG, P10)];
    let mut sum = 0;
    for (i, leg) in legs.iter().enumerate() {
        let t0 = T0 + 100 * i as i64;
        let acct = OLive { op: op(80 + i as u8), ..cov(&w, &shard, &[], 20 * KAS1) };
        let (b, _, _, _, _, next) = c_enter_state(&w.lib, &acct, &shard, &enter, SEED, *leg, 0, &[], t0, w.payer(&w.w, &[coin(xk(&w.w), 33, t0)])).unwrap();
        let sg = signed(&format!("entry {i}: {} units", leg.size / scale), &b, &w.keys);
        sum += leg.size;
        assert_eq!((said(&next), units(&next), next.gross_sum()), (sum, sum, sum), "entry {i}");
        assert_eq!(&sg.tx.outputs[2].script_public_key, &hash_spk(&w.lib, &next.hash()));
        for other in [sum + 1, sum - 1, sum - leg.size, 0, sum + leg.size] {
            let mut t = b.clone();
            t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &shard_hash_with(&next, &head_of(other, &next.claims_hash())));
            assert!(other == sum || !accepted(&t, &w.keys), "entry {i}: the shard written with a sum of {other}");
        }
        shard = nth(&sg, 2, next, t0 as u64 + 10);
    }
    assert_eq!((said(&shard.s), shard.s.rows[0].gross, shard.s.rows[1].gross), (17 * scale, 14 * scale, 3 * scale));
    // A note of a leg of no row: spent, the shard as it is.
    let ghost = note_of(&shard, Left { sid: shard.s.sid, legs: entered(&[fleg(&w, 2, 3, SHORT, P10)], 0), key: xk(&w.w), asked: 0 }, NOTE_MIN, 75);
    let b = left_void(&w.lib, &shard, &ghost, 0, &w.script("KpfTLeftVoid"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("a note of a leg of no row, spent", &b, &w.keys);
    assert_eq!(&sg.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &shard.s.hash()));
    let shard = nth(&sg, 0, shard.s, 6_720);
    // Called, by a statement: market 0 at 30 (the shard is 6 units short there from 10: it has lost 120 KAS).
    let up = [P10 * 3, P10, 0, 0];
    let fw = [coin(xk(&w.w), 30, 6_730)];
    let (b, st, _) = shard_call(&w.lib, &shard, &w.script("KpfTChange"), up, 6_730, xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    let live = nth(&signed("the call stated", &b, &w.keys), 0, st, 6_735);
    let ap = signed("the call applied", &stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.f, &ff)).unwrap(), &w.keys);
    let fee = shard.s.t.keeper_fee_min;
    let (next, call) = shard.s.called_at(&up, 6_730, fee);
    assert_eq!((said(&next), &ap.tx.outputs[0].script_public_key), (17 * scale, &hash_spk(&w.lib, &next.hash())));
    let mut called = nth(&ap, 0, next, 6_800);
    let call = nth(&ap, out_of(&ap, &hash_spk(&w.lib, &call.hash())).0, call, 6_800);
    // The accounts stepped (their legs entered long before), and their notes applied.
    let step_s = w.script("KpfTCStep");
    let a_two = cov(&w, &called, &entered(&legs[..2], 0), 50 * KAS1);
    let a_one = OLive { op: op(84), ..cov(&w, &called, &entered(&legs[2..], 0), 20 * KAS1) };
    let mut claims = vec![];
    for (what, a, script, after) in [("two legs of one row", &a_two, "KpfTLeftAll", 3), ("one leg", &a_one, "KpfTLeft", 0)] {
        let (b, step) = call_step(&w.lib, a, &a.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
        let sg = signed(&format!("the account of {what} stepped"), &b, &w.keys);
        let (b, next, claim) = left_apply(&w.lib, &called, &nth(&sg, 2, step.note.clone(), 6_810), &w.script(script), w.payer(&w.f, &ff)).unwrap();
        let sg = signed(&format!("the note of {what} applied"), &b, &w.keys);
        assert_eq!((said(&next), units(&next)), (after * scale, after * scale), "{what}");
        for other in [after * scale + 1, after * scale - 1, 17 * scale] {
            let mut t = b.clone();
            t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_hash_with(&next, &head_of(other, &next.claims_hash())));
            assert!(!accepted(&t, &w.keys), "{what}: the shard written with a sum of {other}");
        }
        if let Some(c) = claim {
            claims.push(nth(&sg, 2, c, 6_820));
        }
        called = nth(&sg, 0, next, 6_820);
    }
    assert_eq!((called.s.rows, said(&called.s), claims.len(), called.s.owed), ([ShardRow::EMPTY; 4], 0, 1, 120 * KAS1));
    let (b, next, paid) = claim_pay(&w.lib, &called, &claims[0], &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("the claim paid", &b, &w.keys);
    assert_eq!((paid, next.held, next.owed, said(&next)), (100 * KAS1 - fee, 0, 0, 0));
    let (b, open) = call_off(&w.lib, &nth(&sg, 0, next, 6_830), &call, &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).unwrap();
    signed("the call ends", &b, &w.keys);
    assert_eq!((open.called, said(&open), open.rows), (0, 0, [ShardRow::EMPTY; 4]));
}

/// Second round, B (2), the ends of the sum. Four rows at the largest a row takes (2^60 - 1 units each, the last
/// a unit short): an entry of that unit is taken, and the sum is 4 (2^60 - 1), which a script's number holds
/// (below 2^62); a unit more into any row is refused by the row's own bound. So the model's saturating sum never
/// saturates on a shard a script wrote, and the script's addition never fails. A note takes a leg out at the
/// top: the sum falls with the row.
/// What holds the sum is the hash and two steps (an entry adds the leg's size where it writes the row, a note
/// takes it off): NO script compares the sum with the rows it is shown. Run on objects no script writes: a
/// called shard of no row whose books say a sum of 5 has its call ended; a shard whose row has 10 units and
/// whose books say 3 has the note of the 10 applied, and says 7 below nothing. Nothing reaches such a state
/// (`KpfTShardOpen` writes nothing over four empty rows: read); it is said for the commit that will read the sum
/// in the rows' place.
#[test]
fn t3r2_the_sum_at_its_ends_and_what_holds_it_to_the_rows() {
    use kpf_protocol::cross::MARGIN_MAX;
    let w = world();
    let base = seeded(&w);
    let (f, ff) = ([fund(xk(&w.w), 33)], [fund(xk(&w.f), 36)]);
    let scale = w.markets[0].p.scale;
    let last = fleg(&w, 3, 1, LONG, P10);
    let full = |i: usize, gross: i64| ShardRow { head: w.head(i), net: 0, gross };
    let top = OLive { s: Shard { rows: [full(0, MARGIN_MAX), full(1, MARGIN_MAX), full(2, MARGIN_MAX), full(3, MARGIN_MAX - scale)], ..base.s }, ..base.clone() };
    assert_eq!((top.s.gross_sum(), units(&top.s)), (4 * MARGIN_MAX - scale, 4 * MARGIN_MAX - scale));
    assert!(4 * (MARGIN_MAX as i128) < i64::MAX as i128);
    let acct = cov(&w, &top, &[], 20 * KAS1);
    let (b, _, _, _, _, next) = c_enter_state(&w.lib, &acct, &top, &w.script("KpfTEnterS"), SEED, last, 0, &[], T0, w.payer(&w.w, &f)).unwrap();
    signed("the last unit four full rows take", &b, &w.keys);
    assert_eq!((next.gross_sum(), next.rows[3].gross), (4 * MARGIN_MAX, MARGIN_MAX));
    let brim = OLive { s: next, ..top.clone() };
    for i in 0..4 {
        assert!(c_enter_state(&w.lib, &acct, &brim, &w.script("KpfTEnterS"), SEED, fleg(&w, i, 1, LONG, P10), 0, &[], T0 + 100, w.payer(&w.w, &[coin(xk(&w.w), 33, T0 + 100)])).is_err(), "a unit more into row {i}");
    }
    let n = note_of(&brim, Left { sid: brim.s.sid, legs: entered(&[last], 0), key: xk(&w.w), asked: 0 }, NOTE_MIN, 75);
    let (b, after, _) = left_apply(&w.lib, &brim, &n, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    signed("a leg out of the fullest books", &b, &w.keys);
    assert_eq!(after.gross_sum(), 4 * MARGIN_MAX - scale);

    // Objects no script writes: books that say another sum than their rows have.
    // (a) A called shard of no row, its books saying 5: its call is ended, and it goes on saying 5.
    let done = OLive { s: Shard { owed: 0, called: T0, ..base.s }, ..base.clone() };
    let call = OLive { s: Called { sid: done.s.sid, time: T0, heads: [Head::EMPTY; 4], prices: [0; 4] }, value: CALL_HOLD as u64, id: done.id, op: op(91), daa: 6_600 };
    let (b, open) = call_off(&w.lib, &done, &call, &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys));
    let five = head_of(5, &done.s.claims_hash());
    let mut t = with_arg(&b, 5, bytes_arg(&five));
    object_made(&mut t, &w.lib, 0, &shard_hash_with(&done.s, &five));
    assert!(!accepted(&t, &w.keys), "(the output is still the model's)");
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_hash_with(&open, &five));
    let off_with_a_sum = accepted(&t, &w.keys);
    // (b) A row of 10 units, the books saying 3: the note of the 10 is applied, and the books say 7 below nothing.
    let l = fleg(&w, 0, 10, LONG, P10);
    let ten = OLive { s: base.s.booked(&l).unwrap(), ..base.clone() };
    let n = note_of(&ten, Left { sid: ten.s.sid, legs: entered(&[l], 0), key: xk(&w.w), asked: 0 }, NOTE_MIN, 75);
    let (b, after, _) = left_apply(&w.lib, &ten, &n, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys));
    let three = head_of(3 * scale, &ten.s.claims_hash());
    let mut t = with_arg(&b, 5, bytes_arg(&three));
    object_made(&mut t, &w.lib, 0, &shard_hash_with(&ten.s, &three));
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_hash_with(&after, &head_of(-7 * scale, &ten.s.claims_hash())));
    let below_nothing = accepted(&t, &w.keys);
    println!("T3 second round: of objects no script writes: a call ended on books of no row that say a sum of 5: {off_with_a_sum}; a note of 10 units applied to books that say 3, which then say 7 below nothing: {below_nothing}");
    assert!(off_with_a_sum && below_nothing, "a script compares the sum it is shown with the rows (then say so in 17.8a)");
}

/// Second round, B (3) and (4): the scripts that carry a shard's 32 bytes unopened. `KpfTAddC` takes the fixed
/// part whole and writes it again: it cannot tell a shard of the old form (run: the owner adds to an object that
/// hashes as a shard of the form of before, and it goes on in that form). The same by reading of `KpfTOwed.apply`
/// and `KpfTMarkNote.apply` (the 32 bytes under the terms, which their argument still calls `rowsHash`),
/// `KpfTJoinC` (the fixed part whole) and `KpfTChange` (the 32 bytes of the form shown). None of them, and no
/// script outside the eight, computes a hash of rows: nothing compares the 32 bytes with one. A shard of the old
/// form is written by no script of this library (`KpfTShardOpen` writes the new: run in `thin_draw.rs`), and
/// none of the eight opens one (above): its owner could add to it and state on it, and no more.
#[test]
fn t3r2_a_script_that_carries_the_32_bytes_unopened_cannot_tell_the_old_form() {
    let w = world();
    let shard = w.shard();
    let s = shard.s;
    let fc = [fund(xk(&w.c), 30)];
    let (b, next) = shard_fund(&w.lib, &shard, &w.script("KpfTAddC"), 5 * KAS1, w.payer(&w.c, &fc)).unwrap();
    signed("the owner adds to its shard", &b, &w.keys);
    // (Arguments of KpfTAddC.add: 3 the fixed part, 4 the moving part.)
    assert!(matches!(&args_of(&mut b.clone(), 1)[3], Arg::V(ArtifactValue::Bytes(x)) if x[..] == s.fixed_hash()[..]));
    let old_fixed = blake2b(&[s.terms(), p33b(&blake2b(&s.rows_bytes()))].concat());
    assert_ne!(old_fixed, s.fixed_hash());
    let mut t = with_arg(&b, 3, bytes_arg(&old_fixed));
    assert!(!accepted(&t, &w.keys), "(the object is still the shard of this form)");
    object_made(&mut t, &w.lib, 0, &shard_hash_old(&s));
    assert!(!accepted(&t, &w.keys), "(the output is still of this form)");
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_hash_old(&next));
    assert!(accepted(&t, &w.keys), "KpfTAddC opens the fixed part");
    // No contract outside the eight hashes rows (the sources, as they are in the tree).
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts");
    let mut opens = vec![];
    for e in std::fs::read_dir(&dir).expect("the contracts") {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "sil") {
            let src = std::fs::read_to_string(&p).unwrap();
            let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            if code.contains("blake2b(rw)") || code.contains("blake2b(rows)") || code.contains("byte[408]") {
                opens.push(p.file_stem().unwrap().to_string_lossy().to_string());
            }
            // (Whoever hashes the rows hashes the 41 bytes with them, every time.)
            assert_eq!(code.matches("blake2b(rw)").count(), code.matches("gb.slice(0, 9) + p33(blake2b(rw)) + byte[](0x20) + gb.slice(9, 41)").count(), "{}", p.display());
            assert_eq!(code.matches("blake2b(rows)").count(), 0, "{}", p.display());
        }
    }
    opens.sort();
    assert_eq!(opens, vec!["KpfTCallEnd", "KpfTCallPay", "KpfTEnterS", "KpfTLeft", "KpfTLeftAll", "KpfTLeftVoid", "KpfTShardEnd", "KpfTShardPrice"]);
}
