//! Review T7 (docs/audit/REVIEW-T7-second.md): the second class of claims and the row's gate
//! (docs/ARCHITECTURE-CLEARING.md 17.8c, pieces (b) and (c)), at the script engine. The reviewer's file: its world
//! and helpers are its own (after `thin_draw.rs`, by way of `auditT3_legtime.rs`).
#![allow(dead_code, unused_imports)]

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cite;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::cross::{XLeg, XTerms};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::reserve::{RowHead, Shard, ShardRow, NOTE_MIN, SHARD_CARRIER};
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
    fn head(&self, i: usize) -> RowHead {
        let m = &self.markets[i];
        RowHead { key: kpf_protocol::books::Key::futures(m.id()), scale: m.p.scale, price_max: m.p.price_max }
    }
    /// The clearing party's shard: 100 KAS; in market 0 it is 60 units short over 140 of legs, in market 1 it is
    /// 10 long over 30, every leg entered at 10 (what its legs were entered at: 500 KAS short); two rows are empty.
    fn shard(&self) -> OLive<Shard> {
        let scale = self.markets[0].p.scale;
        let rows = [ShardRow::of(self.head(0), -60 * scale, 140 * scale), ShardRow::of(self.head(1), 10 * scale, 30 * scale), ShardRow::EMPTY, ShardRow::EMPTY];
        let s = Shard { t: self.terms(), rows, owner: xk(&self.c), held: 100 * KAS1, expiry: 0, dispute_daa: 50, bond_bps: 1_000, sid: [0x5d; 32], basis: -500 * KAS1, called: 0, owed: 0, claims: Default::default() };
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
    // (In the order of their keys: the rows a test puts at the places 0, 1, 2 are the first rows of a trace. Docs 17.8d.)
    let mut markets: Vec<CMarket> = (0..4u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
    markets.sort_by_key(|m| kpf_protocol::books::Key::futures(m.id()).numbers());
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
// Review T7 (docs 17.8c, pieces (b) and (c)): what these tests need of the second class and of the gate.

use kpf_protocol::reserve::{Claim, Claims, NOTE_LEFT, NOTE_STEP};

/// The note a call's step wrote at output 2 of `sg` (tag 9: what is asked, and what the young legs gained less
/// their band), applied to its called shard: the shard after and the claim written, of either class. The tests of
/// before the second class applied it as an account's own note (tag 6), which is an object the step does not
/// write: the builder made that input, and the engine took it. Here the step's output is held to the note's hash,
/// and the same transaction with the kind said 6 is refused.
#[allow(clippy::type_complexity)]
fn step_apply_t7(w: &World, shard: &OLive<Shard>, sg: &OSigned, step: &CallStep, daa: u64, p: Payer) -> (OBuilt, Shard, Option<Claim>) {
    assert_eq!(&sg.tx.outputs[2].script_public_key, &hash_spk(&w.lib, &step.note_hash()), "the step's note is of tag 9");
    assert_ne!(step.note_hash(), step.note.hash());
    let note = nth(sg, 2, step.note.clone(), daa);
    let (b, next, claim) = left_apply_c(&w.lib, shard, &note, NOTE_STEP, step.young, &w.script(left_script(note.s.legs.len())), p).unwrap();
    let mut as6 = b.clone();
    assert!(matches!(&args_of(&mut as6, 1)[11], Arg::V(ArtifactValue::Int(9))), "argument 11 is the note's kind");
    args_of(&mut as6, 1)[11] = Arg::V(ArtifactValue::Int(6));
    args_of(&mut as6, 1)[12] = Arg::V(ArtifactValue::Int(0));
    assert!(accepted(&b, &w.keys) && !accepted(&as6, &w.keys), "a step's note applied as an account's own");
    (b, next, claim)
}

/// The row's gate at the engine (docs 17.8c): whether `KpfTEnterS` takes the entry of `leg` into row `r` of a
/// shard of the books `books` that holds `held`. The entry is stated past the model where the model refuses (the
/// shard after is the one a shard that holds enough would write), and the engine and the model agree.
fn gate_takes(w: &World, base: &OLive<Shard>, held: i64, books: Shard, leg: XLeg, r: usize) -> bool {
    let shard = OLive { s: Shard { held, ..books }, value: (held + SHARD_CARRIER) as u64, ..base.clone() };
    let acct = cov(w, &shard, &[], 2_000 * KAS1);
    let f = [fund(xk(&w.w), 33)];
    let (st, said, _, _) = acct.s.c_enter(leg, 0, r as i64, &[], T0, acct.value as i64).unwrap();
    let rich = Shard { held: i64::MAX / 4, ..shard.s }.entered(&leg, r, T0).expect("a shard that holds enough takes the leg");
    let next = Shard { held, ..rich };
    let model = shard.s.entered(&leg, r, T0);
    let b = c_enter_state_as(&w.lib, &acct, &shard, &w.script("KpfTEnterS"), SEED, leg.committee, &st, &said, &next, w.payer(&w.w, &f)).unwrap();
    let taken = accepted(&b, &w.keys);
    assert_eq!(taken, model.is_ok(), "the gate: the engine and the model");
    taken
}

/// The upper requirement of a row of `net` and `gross` units (whole units of the tests' markets) at the price `p`,
/// with the terms of these tests: 5.5 % of the worth of the net and 2.25 % of the worth of the gross, each rounded up.
fn row_need(net: i64, gross: i64, p: i64) -> i64 {
    let up = |w: i64, r: i64| (w / 10_000) * r + ((w % 10_000) * r + 9_999) / 10_000;
    up(net * p, 550) + up(gross * p, 225)
}


fn coin(key: [u8; 32], n: u8, daa: i64) -> KeyUtxo {
    KeyUtxo { daa: daa as u64, ..fund(key, n) }
}

/// Signed and taken by the engine (without the print and the sompi-less runs of `signed`).
fn ok(what: &str, b: &OBuilt, keys: &KeySet) -> OSigned {
    let sg = b.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{what}: {:?}", v.errors());
    sg
}

/// What a play under a call came to: the shard's numbers when every note was applied (what it holds, what it owes
/// the first class and the second), and for each account, in the order given: what its step paid in, its claim's
/// two amounts, and what the claim was paid.
#[derive(Debug, Clone)]
struct Played {
    held: i64,
    c1: i64,
    c2: i64,
    of: Vec<(i64, i64, i64, i64)>,
    end: Shard,
}

/// A call at the engine, whole: the shard called at `time` at `prices` (written as the settlement of a call writes
/// it); each account stepped (`KpfTCStep`); each step's note applied as the step wrote it (`KpfTLeft` /
/// `KpfTLeftAll`, tag 9); from the time of the claims each claim paid (`KpfTCallPay.claim`), in the order given.
/// Every transaction is taken by the engine. A claim whose part is below a note's least is paid nothing here.
fn play(w: &World, what: &str, shard: &OLive<Shard>, accts: &[&OLive<Account>], prices: [i64; 4], time: i64) -> Played {
    let ff = [fund(xk(&w.f), 36)];
    let (mut sh, call) = called_at(shard, prices, time, time as u64 + 60);
    let (step_s, pay_s) = (w.script("KpfTCStep"), w.script("KpfTCallPay"));
    let mut claims = vec![];
    let mut of = vec![];
    for (i, a) in accts.iter().enumerate() {
        let (b, st) = call_step(&w.lib, a, &a.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap_or_else(|e| panic!("{what}: the step of account {i}: {e}"));
        let sg = ok(&format!("{what}: account {i} stepped"), &b, &w.keys);
        let (b, next, claim) = step_apply_t7(w, &sh, &sg, &st, time as u64 + 70, w.payer(&w.f, &ff));
        let sg2 = ok(&format!("{what}: the note of account {i} applied"), &b, &w.keys);
        sh = nth(&sg2, 0, next, time as u64 + 80);
        of.push((st.pay, claim.map(|c| c.amount1).unwrap_or(0), claim.map(|c| c.amount2).unwrap_or(0), 0));
        claims.push(claim.map(|c| nth(&sg2, 2, c, time as u64 + 80)));
    }
    assert_eq!(sh.s.rows, [ShardRow::EMPTY; 4], "{what}: every leg is out of the books");
    let (held, c1, c2) = (sh.s.held, sh.s.owed, sh.s.claims.owed2);
    let due = sh.s.claims_time().unwrap();
    let fl = [coin(xk(&w.f), 36, due)];
    for (i, c) in claims.iter().enumerate() {
        let Some(c) = c else { continue };
        if let Ok((b, next, got, _)) = claim_pay_c(&w.lib, &sh, c, &pay_s, w.payer(&w.f, &fl)) {
            let sg = ok(&format!("{what}: the claim of account {i} paid"), &b, &w.keys);
            sh = nth(&sg, 0, next, due as u64 + 5);
            of[i].3 = got;
        }
    }
    Played { held, c1, c2, of, end: sh.s }
}

fn kas(x: i64) -> String {
    format!("{}.{:04}", x / KAS1, (x % KAS1).abs() / 10_000)
}

// ================================================================================================================
// B1 (ii): the way round the gate. A long and a short of one row, then one of them leaves.

/// B1 (ii), finding S1. The gate is at an entry and nowhere else, and "the row is asked no more with the leg than
/// without it" lets the second side of a pair in whatever the shard holds. A leg that LEAVES raises its row's
/// requirement with no gate. At the engine, in the shard of L4 (20 KAS, a user long 5 units from 10, the price 13):
///  1. the owner's long of 1,000 units at the lower edge (12.935) enters only where the shard holds the row's
///     requirement with it, 1,007.47 KAS, to the sompi (the first arm);
///  2. the owner's short of 1,000 units at the upper edge (13.065), from another account of its own, enters a shard
///     that holds 1 KAS (the second arm: the row is asked 592.98 where it was asked 1,017.59);
///  3. the row set off, the owner draws down to the upper requirement of the hedged books (the model's `draw_max`);
///  4. its short leaves by its own statement at the lower edge (stated and applied at the engine; the note is
///     applied to the shard, which pays it): the row is one-sided again, 1,005 units, and the shard holds far less
///     than the gate asked for that row. A call is due at once.
/// Then the call, at the engine, with the owner's long old or young as the owner likes (it waits hedged, at no
/// risk and with no call due, as long as it likes), the user's leg young or old, and the price as it was or 10 %
/// up between the short's leaving and the call.
#[test]
fn t7_b1_ii_a_pair_entered_and_one_side_leaving_leaves_a_row_the_gate_never_saw() {
    let w = world();
    let base = seeded(&w);
    let p13 = P10 * 13 / 10;
    let (lo, hi) = (p13 / 1_000 * 995, p13 / 1_000 * 1_005);
    let (user_leg, own_long, own_short) = (fleg(&w, 0, 5, LONG, P10), fleg(&w, 0, 1_000, LONG, lo), fleg(&w, 0, 1_000, SHORT, hi));
    let with_user = Shard { held: 20 * KAS1, ..base.s.booked(&user_leg).unwrap() };
    assert!(!with_user.call_due(&[p13, 0, 0, 0]), "the shard of L4 is not due for a call with the user alone");

    // 1. The long: the first arm.
    let need1 = row_need(1_005, 1_005, lo);
    assert!(!gate_takes(&w, &base, need1 - 1, with_user, own_long, 0) && gate_takes(&w, &base, need1, with_user, own_long, 0), "the long enters where the shard holds the row's requirement, to the sompi");
    let with_long = Shard { held: need1, ..with_user }.entered(&own_long, 0, T0).unwrap();
    // 2. The short: the second arm, whatever the shard holds.
    let (asked_with, asked_without) = (row_need(5, 2_005, hi), row_need(1_005, 1_005, hi));
    assert!(asked_with < asked_without);
    assert!(gate_takes(&w, &base, KAS1, Shard { expiry: 0, ..with_long }, own_short, 0), "the short that sets the row off enters a shard of 1 KAS");
    let hedged = Shard { expiry: 0, ..Shard { expiry: 0, ..with_long }.entered(&own_short, 0, T0).unwrap() };
    assert!(!hedged.call_due(&[p13, 0, 0, 0]));
    // 3. The draw (the model: `KpfTShardEnd`'s rule; a draw is a statement, run at the engine by reviews T1 and T5).
    let can = hedged.draw_max(&[p13, 0, 0, 0]);
    assert!(hedged.draw(can, [p13, 0, 0, 0], T0, hedged.held + SHARD_CARRIER).is_ok() && hedged.draw(can + 1, [p13, 0, 0, 0], T0, hedged.held + SHARD_CARRIER).is_err());
    let drawn = Shard { held: hedged.held - can, ..hedged };
    // (Hedged and drawn down, no call is due at 13, 20 % above it, or at any price below: the owner waits as long
    // as it likes, and its legs are as old as it likes. The rate on the gross is of the price: at twice the price
    // a call is due.)
    assert!(!drawn.call_due(&[p13, 0, 0, 0]) && !drawn.call_due(&[p13 / 10 * 12, 0, 0, 0]) && !drawn.call_due(&[p13 / 2, 0, 0, 0]) && !drawn.call_due(&[p13 / 100, 0, 0, 0]) && drawn.call_due(&[p13 * 2, 0, 0, 0]));
    // 4. The short leaves by its own statement, at the lower edge: stated, applied, its note applied to the shard.
    let live = OLive { s: drawn, value: (drawn.held + SHARD_CARRIER) as u64, ..base.clone() };
    let b_acct = own_acct(&w, &live, &entered(&[own_short], T0 - 400), 800 * KAS1, 82);
    let (_, _, o, sg) = c_applied(&w, "S1: the owner's short leaves by its own statement", &b_acct, KIND_C_LEAVE, 0, ALL, &[lo], &w.c);
    let note = o.note.clone().expect("the note that the short left");
    let k = out_of(&sg, &hash_spk(&w.lib, &note.hash())).0;
    let ff = [fund(xk(&w.f), 36)];
    let (b, naked, claim) = left_apply_c(&w.lib, &live, &nth(&sg, k, note.clone(), 6_600), NOTE_LEFT, 0, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    ok("S1: the note of the short's leaving applied: no gate", &b, &w.keys);
    let scale = w.markets[0].p.scale;
    assert_eq!((claim, naked.rows[0].net, naked.rows[0].gross, naked.called), (None, -1_005 * scale, 1_005 * scale, 0), "the row is one-sided, and the shard paid the note");
    let paid_short = drawn.held - naked.held;
    // The row as it stands is one the gate was never asked: an entry that made it would have needed `need1` held.
    assert!(naked.held < need1 * 75 / 100, "the shard holds {} of the {} the gate asks for this row", naked.held, need1);
    assert!(naked.call_due(&[p13, 0, 0, 0]) && naked.call_due(&[hi, 0, 0, 0]) && naked.call_due(&[lo, 0, 0, 0]), "a call is due at once, at either edge");
    println!("T7 B1 (ii): the gate asked {} KAS for the owner's long of 1,000 units at 12.935 (7.75 % of 13,000); the short entered at no cost; hedged, the owner drew {}; the short left and was paid {}: the row is one-sided and the shard holds {} KAS ({} % of the row's worth at 13), with a call due at once. The owner's own coins in the shard: {} (it paid in {}, drew {}, was paid {}).", kas(need1), kas(can), kas(paid_short), kas(naked.held), naked.held * 100 / (1_005 * p13), kas(naked.held - 20 * KAS1), kas(need1 - 20 * KAS1), kas(can), kas(paid_short));

    // The call. (T is 300: a leg entered at T0 - 300 is old at a call of T0, one entered at T0 - 100 young.)
    let shard = OLive { s: Shard { expiry: 0, ..naked }, value: (naked.held + SHARD_CARRIER) as u64, ..base.clone() };
    let up10 = p13 / 1_000 * 1_100 / 1_000 * 1_005;
    let mut rows = vec![];
    for (price, moved) in [(hi, "none"), (up10, "10 % up")] {
        for (a_old, u_old) in [(true, true), (true, false), (false, true), (false, false)] {
            let a = own_acct(&w, &shard, &entered(&[own_long], if a_old { T0 - 300 } else { T0 - 100 }), 800 * KAS1, 81);
            let u = cov(&w, &shard, &entered(&[user_leg], if u_old { T0 - 300 } else { T0 - 100 }), 20 * KAS1);
            let what = format!("S1: the price moved {moved}, the owner's long {}, the user's leg {}", if a_old { "old" } else { "young" }, if u_old { "old" } else { "young" });
            // (The owner's claim is shown first.)
            let p = play(&w, &what, &shard, &[&a, &u], [price, 0, 0, 0], T0);
            let (own, user) = (p.of[0], p.of[1]);
            println!("T7 B1 (ii), {what}: the shard holds {} and owes {} (first) and {} (second); the owner's claim ({}, {}) is paid {}; the user's ({}, {}) is paid {}", kas(p.held), kas(p.c1), kas(p.c2), kas(own.1), kas(own.2), kas(own.3), kas(user.1), kas(user.2), kas(user.3));
            rows.push((moved, a_old, u_old, own, user, p.held));
        }
    }
    // With no move of the price every claim is whole: the owner's edges are within what it left in the shard.
    for r in rows.iter().filter(|r| r.0 == "none") {
        assert_eq!((r.3 .3, r.4 .3), (r.3 .1 + r.3 .2, r.4 .1 + r.4 .2), "no move: whole");
        // (An OLD leg of the owner's is of the first class, with no band: its two edges, 130 KAS. A young one: its
        // band takes them back.)
        assert_eq!((r.3 .1, r.3 .2), if r.1 { (130 * KAS1, 0) } else { (0, 0) });
    }
    // The price 10 % up between the short's leaving and the call (which is due all the while):
    //  - the owner's long OLD: a claim of the FIRST class of 1,436.5 KAS on a shard that holds about 617: an old
    //    user's claim of 21.86 is paid at one share (F116's shape: the leg entered more than T before the call
    //    while the shard "was not called for T", being hedged); a young user's, of the second class, NOTHING;
    //  - the owner's long YOUNG: a claim of the second class; an old user is whole; a young user is paid at one
    //    share of what is left.
    let at = |a_old: bool, u_old: bool| rows.iter().find(|r| r.0 == "10 % up" && r.1 == a_old && r.2 == u_old).unwrap();
    let (oo, oy, yo, yy) = (at(true, true), at(true, false), at(false, true), at(false, false));
    assert!(oo.3 .1 > 1_400 * KAS1 && oo.4 .1 > 21 * KAS1 && oo.4 .3 < oo.4 .1 * 6 / 10, "the old user beside the owner's old leg: {:?}", oo.4);
    assert_eq!((oy.4 .1, oy.4 .3), (0, 0), "S1 is closed: the young user beside the owner's old leg is paid something: {:?}", oy.4);
    assert!(oy.4 .2 > 20 * KAS1);
    assert_eq!(yo.4 .3, yo.4 .1, "the old user beside the owner's young leg is whole");
    assert!(yy.4 .3 < yy.4 .2 * 6 / 10, "the young user beside the owner's young leg: {:?}", yy.4);
}

// ================================================================================================================
// B1 (i): L4 as a game, with the gate and the second class.

/// B1 (i). The shard of L4 (20 KAS; a user long 5 units from 10, young; the price 13: it gained 15), paid up by its
/// owner to what the gate asks for the owner's long of 1,000 units at the lower edge of 13 (1,007.47 KAS). The
/// owner's leg (or a long and a short in two accounts of its own, the short at the upper edge) young; the price
/// moves before the call's time; the call is stated at the upper edge of the moved price. Every step, note and
/// claim at the engine; the call itself is written as its settlement writes it, WHETHER OR NOT IT IS DUE (said for
/// each row: with the gate's requirement held, a call of these books is due only once the price has moved).
#[test]
fn t7_b1_i_l4_as_a_game_with_the_gate_and_the_second_class() {
    let w = world();
    let base = seeded(&w);
    let p13 = P10 * 13 / 10;
    let (lo, hi) = (p13 / 1_000 * 995, p13 / 1_000 * 1_005);
    let (user_leg, own_long, own_short) = (fleg(&w, 0, 5, LONG, P10), fleg(&w, 0, 1_000, LONG, lo), fleg(&w, 0, 1_000, SHORT, hi));
    let need1 = row_need(1_005, 1_005, lo);
    let with_user = base.s.booked(&user_leg).unwrap();
    assert!(gate_takes(&w, &base, need1, with_user, own_long, 0) && !gate_takes(&w, &base, need1 - 1, with_user, own_long, 0));
    let mut table = vec![];
    for both in [false, true] {
        let books = if both { with_user.booked(&own_long).unwrap().booked(&own_short).unwrap() } else { with_user.booked(&own_long).unwrap() };
        let shard = OLive { s: Shard { held: need1, ..books }, value: (need1 + SHARD_CARRIER) as u64, ..base.clone() };
        for (moved, bps) in [("none", 0i64), ("1 % up", 100), ("2 % up", 200), ("5 % up", 500), ("8 % up", 800), ("10 % up", 1_000), ("1 % down", -100)] {
            let market = p13 / 10_000 * (10_000 + bps);
            let price = market / 1_000 * 1_005;
            let due = shard.s.call_due(&[price, 0, 0, 0]);
            let u = cov(&w, &shard, &entered(&[user_leg], T0 - 100), 20 * KAS1);
            let a = own_acct(&w, &shard, &entered(&[own_long], T0 - 1), 800 * KAS1, 81);
            let b = own_acct(&w, &shard, &entered(&[own_short], T0 - 1), 800 * KAS1, 82);
            let what = format!("L4 as a game: the owner's {}, the price {moved}", if both { "long and short" } else { "long" });
            let accts: Vec<&OLive<Account>> = if both { vec![&a, &b, &u] } else { vec![&a, &u] };
            let p = play(&w, &what, &shard, &accts, [price, 0, 0, 0], T0);
            let user = *p.of.last().unwrap();
            let gained = 5 * (price - P10);
            println!("T7 B1 (i), {what} (the call at {} KAS a unit; due: {due}): W {} (the owner's accounts paid in {}), C1 {}, C2 {}; the owner's claims {:?}; the user gained {}, its claim of the second class {} is paid {}", kas(price), kas(p.held), kas(p.of[..p.of.len() - 1].iter().map(|x| x.0).sum::<i64>()), kas(p.c1), kas(p.c2), p.of[..p.of.len() - 1].iter().map(|x| (kas(x.1), kas(x.2), kas(x.3))).collect::<Vec<_>>(), kas(gained), kas(user.2), kas(user.3));
            table.push((both, bps, due, user, p.c2, p.held, gained));
        }
    }
    for (both, bps, _, user, c2, held, gained) in &table {
        // The user's claim is what it gained less the band of its leg (the tolerance twice of its worth at the call).
        assert!(user.1 == 0 && *gained - user.2 > 0 && *gained - user.2 < 80 * KAS1 / 100, "the user's claim: {user:?} of {gained}");
        if *both || *bps <= 500 {
            assert_eq!(user.3, user.2, "the user is whole (the owner's {}, {bps} bps)", if *both { "pair" } else { "long" });
        } else {
            // The owner's long alone, a move of 8 % or more while it is young: the second class is owed more than
            // the shard has: the user is paid its share.
            assert!(c2 > held && user.3 < user.2, "{bps} bps: {user:?}");
        }
    }
    // With the gate's requirement held no call of these books is due at 13; and none at any price while the owner
    // holds both sides.
    assert!(table.iter().all(|r| !r.2 || (!r.0 && r.1 != 0)), "a call is due with the row's requirement held and the price at 13, or with the row set off");
}

// ================================================================================================================
// B2: the class where a note is applied.

fn int_arg(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}

/// B2 and B6. `KpfTLeft.apply` and `KpfTLeftAll.apply` on notes made by hand, every combination of: the kind said (6
/// with nothing of the young; 9 with nothing, with 7 KAS, with a sompi below nothing; 6 with 7 KAS) x one leg or
/// three x the legs all old, all young, mixed (the edge: T before the call's time is old, a block later young) x
/// asked 50 KAS (more than is held), 5 KAS, nothing, 3 KAS paid in x nothing or 1 KAS paid to a claim before x
/// the shard called or not. The outcome is computed here from the rule as 17.8c writes it; the model
/// (`left_class`) says the same or refuses where the rule refuses; the engine takes that outcome and refuses: the
/// two amounts the other way round, a sompi more, no claim, the claim with the shard's count left as it was, a
/// claim where none is due; and the same transaction with the kind said the other (6 for 9, 9 for 6): the hash is
/// another. What the rule refuses the engine refuses with the outcome it would have had.
#[test]
fn t7_b2_the_class_where_a_note_is_applied_is_the_rules_and_no_other() {
    let w = world();
    let base = bare(&w);
    let ff = [fund(xk(&w.f), 36)];
    let key = xk(&w.w);
    let (old_t, young_t) = (T0 - 300, T0 - 299);
    let l = [fleg(&w, 0, 10, LONG, P10), fleg(&w, 1, 4, SHORT, P10), fleg(&w, 0, 3, SHORT, P10 * 11 / 10)];
    let (mut taken, mut refused, mut lies, mut first, mut second, mut both) = (0, 0, 0, 0, 0, 0);
    for legs_n in [1usize, 3] {
        for ages in 0..3 {
            if legs_n == 1 && ages == 2 {
                continue;
            }
            for asked in [50 * KAS1, 5 * KAS1, 0, -3 * KAS1] {
                for paid_before in [0, KAS1] {
                    for called in [true, false] {
                        if !called && paid_before != 0 {
                            continue;
                        }
                        for (tag, young) in [(6i64, 0i64), (9, 0), (9, 7 * KAS1), (6, 7 * KAS1), (9, -1)] {
                            let what = format!("{legs_n} legs, ages {ages}, asked {asked}, paid before {paid_before}, called {called}, tag {tag}, young {young}");
                            let legs: Vec<XLeg> = (0..legs_n).map(|i| leg_entered(&l[i], if ages == 0 || (ages == 2 && i == 0) { old_t } else { young_t })).collect();
                            let mut books = base.s;
                            for x in &l[..legs_n] {
                                books = books.booked(x).unwrap();
                            }
                            let s = if called { Shard { held: 20 * KAS1, called: T0, owed: 9 * KAS1, claims: Claims { paid1: paid_before, owed2: 2 * KAS1, paid2: 0 }, ..books } } else { Shard { held: 20 * KAS1, ..books } };
                            let shard = OLive { s, value: (s.held + SHARD_CARRIER) as u64, ..base.clone() };
                            let note = Left { sid: s.sid, legs: legs.clone(), key, asked };
                            let holds = NOTE_MIN + (-asked).max(0);
                            let live = OLive { s: note.clone(), value: holds as u64, id: base.id, op: op(75), daa: 6_050 };
                            // The rule, written here.
                            let is_young = |x: &XLeg| called && leg_time(x) > T0 - 300;
                            let (any, all) = (legs.iter().any(is_young), legs.iter().all(is_young));
                            assert_eq!((any, all), (called && ages != 0, called && ages == 1), "{what}");
                            let valid = (tag == 6 && young == 0) || (tag == 9 && called && young >= 0 && (young == 0 || any) && (asked <= 0 || !all));
                            let (rows_out, at, _, _) = s.left_class(&Left { asked: 0, ..note.clone() }, NOTE_LEFT, 0, NOTE_MIN).unwrap();
                            let mut next = rows_out;
                            next.held += (-asked).max(0);
                            let want = asked.max(0);
                            let (pay, a1, a2) = if !called {
                                let p = want.min(next.held);
                                (p, want - p, 0)
                            } else if paid_before > 0 || (tag != 9 && any) {
                                (0, 0, want + young.max(0))
                            } else {
                                (0, want, young.max(0))
                            };
                            next.held -= pay;
                            next.owed += a1;
                            next.claims.owed2 += a2;
                            let claim = (a1 + a2 > 0).then_some(Claim { sid: s.sid, key, amount1: a1, paid1: 0, amount2: a2, paid2: 0 });
                            let script = w.script(left_script(legs_n));
                            let build = |tag: i64, young: i64, next: Shard, pay: i64, claim: Option<Claim>| left_apply_as_c(&w.lib, &shard, &live, tag, young, &at, next, pay, claim, &script, w.payer(&w.f, &ff));
                            let took = |r: kpf_protocol::Result<(OBuilt, Shard, Option<Claim>)>| r.is_ok_and(|b| accepted(&b.0, &w.keys));
                            let model = s.left_class(&note, tag, young, holds);
                            assert_eq!(model.is_ok(), valid, "{what}: the model and the rule");
                            if !valid {
                                assert!(!took(build(tag, young, next, pay, claim)), "{what}: what the rule refuses is applied");
                                refused += 1;
                                continue;
                            }
                            let m = model.unwrap();
                            assert_eq!((m.0, m.1.clone(), m.2, m.3), (next, at.clone(), pay, claim), "{what}: the model's outcome");
                            let b = build(tag, young, next, pay, claim).unwrap().0;
                            assert!(accepted(&b, &w.keys), "{what}: the rule's outcome is not taken");
                            taken += 1;
                            match claim {
                                Some(c) if c.amount1 > 0 && c.amount2 > 0 => both += 1,
                                Some(c) if c.amount2 > 0 => second += 1,
                                Some(_) => first += 1,
                                None => {}
                            }
                            // The kind said the other, in the same transaction (the note's input is as it was).
                            let other = if tag == 6 { 9 } else { 6 };
                            let mut t = with_arg(&b, 11, int_arg(other));
                            assert!(matches!(&args_of(&mut t, 1)[12], Arg::V(ArtifactValue::Int(y)) if *y == young), "argument 12 is the young's amount");
                            args_of(&mut t, 1)[12] = int_arg(if other == 6 { 0 } else { young });
                            assert!(!accepted(&t, &w.keys), "{what}: the note applied as of the other kind");
                            lies += 1;
                            // Other outcomes.
                            let unc = Shard { owed: s.owed, claims: s.claims, ..next };
                            let mut wrong: Vec<(&str, Shard, Option<Claim>)> = vec![];
                            if let Some(c) = claim {
                                let sw = Claim { amount1: c.amount2, amount2: c.amount1, ..c };
                                wrong.push(("the two amounts the other way round", Shard { owed: s.owed + sw.amount1, claims: Claims { owed2: s.claims.owed2 + sw.amount2, ..s.claims }, ..next }, Some(sw)));
                                wrong.push(("a sompi more of the first class", Shard { owed: next.owed + 1, ..next }, Some(Claim { amount1: c.amount1 + 1, ..c })));
                                wrong.push(("a sompi more of the second class", Shard { claims: Claims { owed2: next.claims.owed2 + 1, ..next.claims }, ..next }, Some(Claim { amount2: c.amount2 + 1, ..c })));
                                wrong.push(("no claim written", next, None));
                                wrong.push(("the claim written and not counted", unc, Some(c)));
                                wrong.push(("counted and written all in the first class", Shard { owed: s.owed + c.amount1 + c.amount2, claims: s.claims, ..next }, Some(Claim { amount1: c.amount1 + c.amount2, amount2: 0, ..c })));
                                wrong.push(("the claim said paid in part", next, Some(Claim { paid1: c.amount1.min(1), paid2: c.amount2.min(1), ..c })));
                            } else {
                                wrong.push(("a claim of a sompi of the first class where none is due", Shard { owed: next.owed + 1, ..next }, Some(Claim { sid: s.sid, key, amount1: 1, paid1: 0, amount2: 0, paid2: 0 })));
                                wrong.push(("a claim of a sompi of the second class where none is due", Shard { claims: Claims { owed2: next.claims.owed2 + 1, ..next.claims }, ..next }, Some(Claim { sid: s.sid, key, amount1: 0, paid1: 0, amount2: 1, paid2: 0 })));
                            }
                            for (lie, n2, c2) in wrong {
                                if (n2, c2) == (next, claim) {
                                    continue;
                                }
                                assert!(!took(build(tag, young, n2, pay, c2)), "{what}: {lie}");
                                lies += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    println!("T7 B2: {taken} notes applied as the rule says at the engine (claims of the first class {first}, of the second {second}, of both {both}), {refused} that the rule refuses refused, {lies} other outcomes and kinds refused");
    assert!(taken > 100 && refused > 80 && first > 10 && second > 20 && both >= 2, "{taken} {refused} {first} {second} {both}");
}

// ================================================================================================================
// B4: a mark's note under a call.

/// B4 and B6. `KpfTMarkNote.apply`: the note of a mark of 10 units from 10 to 20 (it asks 100 KAS) applied to a
/// shard that holds 20: not called, 20 are paid and 80 owed (as before the change: the control); called, nothing is
/// paid and the 100 are of the first class where the leg is old and nothing was paid, of the second where the leg
/// is young or a claim was paid. Each other outcome is refused, and so are the 68 bytes of the books shown
/// otherwise (the sum of the gross, the rows' hash, each of the three numbers).
#[test]
fn t7_b4_a_marks_note_is_counted_in_its_class_and_the_books_are_shown_as_they_are() {
    use kpf_protocol::reserve::MarkNote;
    let w = world();
    let base = bare(&w);
    let ff = [fund(xk(&w.f), 36)];
    let key = xk(&w.w);
    let leg0 = fleg(&w, 0, 10, LONG, P10);
    let script = w.script("KpfTMarkNote");
    let asked = 100 * KAS1;
    for (what, time, paid_before, called, want) in [("an old leg", T0 - 300, 0, true, (0, asked, 0)), ("a young leg", T0 - 299, 0, true, (0, 0, asked)), ("an old leg after a claim was paid", T0 - 300, KAS1, true, (0, 0, asked)), ("a young leg after a claim was paid", T0 - 299, KAS1, true, (0, 0, asked)), ("a young leg, the shard not called", T0 - 299, 0, false, (20 * KAS1, 80 * KAS1, 0))] {
        let books = base.s.booked(&leg0).unwrap();
        let s = if called { Shard { held: 20 * KAS1, called: T0, owed: 9 * KAS1, claims: Claims { paid1: paid_before, owed2: 2 * KAS1, paid2: 0 }, ..books } } else { Shard { held: 20 * KAS1, ..books } };
        let shard = OLive { s, value: (s.held + SHARD_CARRIER) as u64, ..base.clone() };
        let note = OLive { s: MarkNote { sid: s.sid, leg: leg_entered(&leg0, time), entry: 2 * P10, key, asked }, value: NOTE_MIN as u64, id: base.id, op: op(61), daa: 6_300 };
        let (b, next, claim) = mark_apply_c(&w.lib, &shard, &note, &script, w.payer(&w.f, &ff)).unwrap();
        let sg = ok(&format!("B4: the mark's note of {what}"), &b, &w.keys);
        let (pay, a1, a2) = want;
        assert_eq!((s.held - next.held, next.owed - s.owed, next.claims.owed2 - s.claims.owed2, claim), (pay, a1, a2, Some(Claim { sid: s.sid, key, amount1: a1, paid1: 0, amount2: a2, paid2: 0 })), "{what}");
        assert_eq!(pays(&sg, &key, pay), pay > 0, "{what}");
        let c = claim.unwrap();
        let build = |n2: Shard, c2: Option<Claim>| mark_apply_as_c(&w.lib, &shard, &note, n2, pay, c2, &script, w.payer(&w.f, &ff));
        for (lie, n2, c2) in [
            ("the other class", Shard { owed: s.owed + a2, claims: Claims { owed2: s.claims.owed2 + a1, ..s.claims }, ..next }, Some(Claim { amount1: a2, amount2: a1, ..c })),
            ("a sompi more", Shard { owed: next.owed + 1, ..next }, Some(Claim { amount1: a1 + 1, ..c })),
            ("a sompi more of the second", Shard { claims: Claims { owed2: next.claims.owed2 + 1, ..next.claims }, ..next }, Some(Claim { amount2: a2 + 1, ..c })),
            ("no claim", next, None),
            ("written and not counted", Shard { owed: s.owed, claims: s.claims, ..next }, Some(c)),
        ] {
            let r = build(n2, c2);
            assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "{what}: {lie}");
        }
        // The books, 68 bytes: argument 4.
        let open = s.books_open();
        assert_eq!(open.len(), 68);
        assert!(matches!(&args_of(&mut b.clone(), 1)[4], Arg::V(ArtifactValue::Bytes(x)) if x[..] == open[..]), "argument 4 is the books opened");
        for (lie, at) in [("another sum of the gross", 1usize), ("another hash of the rows", 9), ("another amount paid to the first class", 42), ("another amount owed to the second", 51), ("another amount paid to the second", 60)] {
            let mut other = open.clone();
            other[at] ^= 1;
            assert!(!accepted(&with_arg(&b, 4, bytes_arg(&other)), &w.keys), "{what}: the books shown with {lie}");
        }
        assert!(!accepted(&with_arg(&b, 4, bytes_arg(&s.books_hash())), &w.keys), "{what}: the books shown as their hash, as before the change");
        println!("T7 B4, {what}: paid {}, of the first class {}, of the second {}; {} bytes", kas(pay), kas(a1), kas(a2), sg.mass.size);
    }
}

// ================================================================================================================
// B3: the band, at its corners.

/// B3, beside the 180 random accounts of `t1b_r54_a_step_under_a_call_is_the_models_on_random_accounts` (which
/// holds the young's amount of every step to arithmetic written out from the script). Here, at the engine:
///  - a young SHORT that gained: the band's base is the larger worth, which for a short that gained is its entry;
///  - a young gain smaller than its band says nothing (and asks nothing);
///  - two young legs: one rate on the SUM of their larger worths, rounded up once (a sompi more than a rate a leg);
///  - a young gain beside an old loss: the loss is paid whole, and the young's amount is said whole beside it;
///  - each as the step writes it, and a sompi off either way refused.
#[test]
fn t7_b3_the_band_at_its_corners() {
    let w = world();
    let base = bare(&w);
    let ff = [fund(xk(&w.f), 36)];
    let scale = w.markets[0].p.scale;
    let tiny = |dir: i64, entry: i64| XLeg { size: 1, ..fleg(&w, 0, 1, dir, entry) };
    let cases: Vec<(&str, Vec<XLeg>, i64, i64, i64, i64)> = vec![
        ("a young short of 10 units from 13 that gained 30 at 10: the band is of its entry's worth, 130", vec![leg_entered(&fleg(&w, 0, 10, SHORT, P10 * 13 / 10), T0 - 1)], P10, 0, 0, 30 * KAS1 - 13 * KAS1 / 10),
        ("a young long of 100 units from 10 that gained 5 at 10.05: less than its band of 10.05", vec![leg_entered(&fleg(&w, 0, 100, LONG, P10), T0 - 1)], P10 / 1_000 * 1_005, 0, 0, 0),
        ("two young longs of a hundred-millionth of a unit from 10, at 13: one rate on the sum of 26 sompi", vec![leg_entered(&tiny(LONG, P10), T0 - 1), leg_entered(&tiny(LONG, P10), T0 - 1)], P10 * 13 / 10, 0, 0, 5),
        ("a young long that gained 30 beside an old short that lost 30", vec![leg_entered(&fleg(&w, 0, 10, LONG, P10), T0 - 1), leg_entered(&fleg(&w, 0, 10, SHORT, P10), 0)], P10 * 13 / 10, -30 * KAS1, 30 * KAS1, 30 * KAS1 - 13 * KAS1 / 10),
    ];
    let _ = scale;
    for (what, legs, price, gain, pay, young) in cases {
        let shard = OLive { s: Shard { held: 20 * KAS1, ..base.s.booked(&fleg(&w, 0, 1, LONG, P10)).unwrap() }, ..base.clone() };
        let (_, call) = called_objs(&shard, [price, 0, 0, 0], 6_600);
        let a = cov(&w, &shard, &legs, 50 * KAS1);
        let (b, st) = call_step(&w.lib, &a, &a.s, None, &call, &w.script("KpfTCStep"), w.payer(&w.f, &ff)).unwrap();
        let sg = ok(what, &b, &w.keys);
        assert_eq!((st.gain, st.pay, st.note.asked, st.young), (gain, pay, -pay, young), "{what}");
        assert_eq!(&sg.tx.outputs[2].script_public_key, &hash_spk(&w.lib, &st.note.hash_step(young)), "{what}");
        for d in [1i64, -1] {
            let mut t = b.clone();
            t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &st.note.hash_step(young + d));
            assert!(!accepted(&t, &w.keys), "{what}: the young's amount {d} off");
        }
        // (What the account is paid is what it was paid before the change: its coins less what it paid, a note's
        // least and the fee. Nothing of the young's amount is paid to it.)
        assert!(pays(&sg, &xk(&w.w), 50 * KAS1 - pay - NOTE_MIN - st.fee), "{what}");
    }
}

// ================================================================================================================
// B5: the gate.

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo) as u64) as i64
    }
}

/// The gate written out from `KpfTEnterS.sil`, in wide numbers: (the row's requirement with the leg, without it).
fn gate_by_hand(s: &Shard, l: &XLeg, r: usize) -> (i128, i128) {
    let row = &s.rows[r];
    let (sc, e) = (l.scale as i128, l.entry as i128);
    let worth = |n: i128| (n / sc) * e + (n % sc) * (e / sc) + ((n % sc) * (e % sc)) / sc;
    let up = |w: i128, rate: i128| (w / 10_000) * rate + ((w % 10_000) * rate + 9_999) / 10_000;
    let (g, mmr) = (s.t.g_bps as i128, s.t.mmr_bps as i128);
    let (rn, rg) = (mmr + g, ((mmr - 3 * g) / 2).max(0) + g);
    let need = |n: i128, gr: i128| up(worth(n.abs()), rn) + up(worth(gr), rg);
    let units = if l.dir == LONG { -(l.size as i128) } else { l.size as i128 };
    (need(row.net as i128 + units, row.gross as i128 + l.size as i128), need(row.net as i128, row.gross as i128))
}

/// B5 and B6. The gate on random rows at the engine: a row that is new or has legs (its net of either sign, up to
/// its gross), a leg of either direction, from a hundred-millionth of a unit to 500 units, at an entry from a
/// sompi to 50 KAS a unit; the shard holding nothing, a sompi less than the row's requirement with the leg,
/// exactly it, a sompi more, or far more; owing nothing or 3 KAS. The engine, the model (`entered`) and the
/// arithmetic written out here agree on each, and both arms are met many times.
#[test]
fn t7_b5_the_gate_on_random_rows_is_the_models_and_the_arithmetics() {
    let w = world();
    let base = seeded(&w);
    let scale = w.markets[0].p.scale;
    let mut r = Rng(0x7_0005);
    let (mut by_first, mut by_second, mut refused) = (0, 0, 0);
    for case in 0..120 {
        let gross = if r.range(0, 4) == 0 { 0 } else { r.range(1, 3_000) * scale / 4 };
        let net = if gross == 0 { 0 } else { r.range(-gross, gross + 1) };
        let rows = if gross == 0 { [ShardRow::EMPTY; 4] } else { [ShardRow::of(w.head(0), net, gross), ShardRow::EMPTY, ShardRow::EMPTY, ShardRow::EMPTY] };
        let size = match r.range(0, 6) { 0 => 1, 1 => r.range(1, scale), _ => r.range(1, 2_000) * scale / 4 };
        let entry = match r.range(0, 6) { 0 => 1, 1 => r.range(1, 1_000), _ => r.range(1, 500) * P10 / 100 };
        let leg = XLeg { size, entry, ..fleg(&w, 0, 1, if r.range(0, 2) == 0 { LONG } else { SHORT }, P10) };
        let owed = if r.range(0, 3) == 0 { 3 * KAS1 } else { 0 };
        let books = Shard { rows, owed, ..base.s };
        let (with, without) = gate_by_hand(&books, &leg, 0);
        let with64 = with as i64;
        let held = match r.range(0, 6) { 0 => 0, 1 => with64 + owed - 1, 2 => with64 + owed, 3 => with64 + owed + 1, 4 => with64 / 2, _ => with64 * 3 + 50 * KAS1 }.max(0);
        let hand = (held - owed) as i128 >= with || with <= without;
        let taken = gate_takes(&w, &base, held, books, leg, 0);
        assert_eq!(taken, hand, "case {case}: net {net}, gross {gross}, the leg {} of {size} at {entry}, held {held}, owed {owed}: asked {with} with the leg and {without} without", if leg.dir == LONG { "long" } else { "short" });
        if !taken {
            refused += 1;
        } else if (held - owed) as i128 >= with {
            by_first += 1;
        } else {
            by_second += 1;
        }
    }
    println!("T7 B5: the gate on 120 random rows and legs: {by_first} entries taken by what the shard holds, {by_second} taken though it holds less (the row is asked no more), {refused} refused: the engine, the model and the arithmetic agree on each");
    assert!(by_first >= 25 && by_second >= 8 && refused >= 20, "{by_first} {by_second} {refused}");
}

/// B5, the leg's ENTRY as the gate's price. The gate reads the price the entry STATES, which nothing holds to the
/// market's where it is stated (the committee's price answers it only where someone disputes its point). At the
/// engine: an owner's long of 10,000 units "from a sompi a unit" enters a shard that holds 1 KAS (the row asks
/// 777 sompi). The books have the leg from that transaction: at the market's price of 13 the shard has "lost" 130
/// thousand KAS, and a call is due at once (the model). What holds it:
///  - the account under the entry is MARKED (its moving part a push longer): `KpfTCStep` steps no marked account
///    (run: the step built for the account as the entry makes it, with the marked object in its place, is
///    refused; the same step of the unmarked account is taken), so the silly leg makes no claim while its entry
///    is not settled;
///  - disputed, the entry's price point is answered wrong by the committee's price and the entry undone
///    (`KpfTEnterOff`; reviews T1 and T3 ran it): NOT run here;
///  - NOT disputed for a dispute period, the entry is applied and the leg stands at its sompi: under the call that
///    is due it is young, and its step says 130 thousand KAS less 1 % for the young (run: the step of the settled
///    account), which leaves every young user of the shard a ten-thousandth of what it gained.
/// So the gate is as strong as "every false statement on an account is disputed" (17.9's own rest), and no
/// stronger: finding S6.
#[test]
fn t7_b5_an_entry_stated_at_a_sompi_passes_the_gate() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let p13 = P10 * 13 / 10;
    let silly = fleg(&w, 0, 10_000, LONG, 1);
    let user_leg = fleg(&w, 0, 5, LONG, P10);
    let with_user = base.s.booked(&user_leg).unwrap();
    assert_eq!(gate_by_hand(&Shard { held: KAS1, ..with_user }, &silly, 0).0, row_need(10_005, 10_005, 1) as i128);
    assert_eq!(row_need(10_005, 10_005, 1), 777);
    assert!(gate_takes(&w, &base, KAS1, with_user, silly, 0), "a leg of 10,000 units stated at a sompi a unit enters a shard of 1 KAS");
    let after = Shard { held: 20 * KAS1, ..with_user }.entered(&silly, 0, T0).unwrap();
    assert!(after.call_due(&[p13, 0, 0, 0]) && after.equity(&[p13, 0, 0, 0]).unwrap() < -129_900 * KAS1);
    // The marked account is stepped by nothing.
    let shard = OLive { s: after, value: (after.held + SHARD_CARRIER) as u64, ..base.clone() };
    let (_, call) = called_objs(&shard, [p13, 0, 0, 0], 6_600);
    let acct = own_acct(&w, &shard, &[], 2_000 * KAS1, 81);
    let (_, _, marked, settled) = acct.s.c_enter(silly, 0, 0, &[], T0, acct.value as i64).unwrap();
    let stands = OLive { s: settled.clone(), ..acct.clone() };
    let (b, st) = call_step(&w.lib, &stands, &stands.s, None, &call, &w.script("KpfTCStep"), w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys), "(the control) the account the entry becomes, once settled, is stepped");
    let mut t = b.clone();
    object_made(&mut t, &w.lib, 0, &marked.hash());
    assert!(!accepted(&t, &w.keys), "a marked account is stepped as the account it would become");
    let mut t = b.clone();
    object_made(&mut t, &w.lib, 0, &marked.hash());
    args_of(&mut t, 1)[6] = bytes_arg(&marked.moving());
    assert!(!accepted(&t, &w.keys), "a marked account is stepped, its moving part of 180 bytes shown");
    // Settled undisputed: what its step says.
    let worth = 10_000 * p13;
    assert_eq!((st.gain, st.note.asked, st.young), (0, 0, worth - 10_000 - worth / 100));
    println!("T7 B5: an owner's long of 10,000 units stated at a sompi a unit passes the gate of a shard of 1 KAS (the row asks {} sompi); the call is due at once; marked, the account is not stepped; settled undisputed, its step says {} KAS for the young", row_need(10_005, 10_005, 1), kas(st.young));
}

// ================================================================================================================
// S2: the gate is of what is held, and of one row.

/// Finding S2. The entries at the engine (the gate), what they make due in the model (`call_due`, `requirement`).
///  (a) The shard of L4 holds 20 KAS, of which 15 are what its user gained: its equity at 13 is 5 KAS, and no call
///      is due. The gate reads what is HELD: the owner's long of 2 units more at 13 enters (the row asks 7.05 KAS),
///      and a call is due at once.
///  (b) The gate reads one ROW: a long of 25 units of another market at 10 (250 KAS; its row asks 19.375) enters
///      the same shard, and a call is due at once.
///  (c) Four rows, one gate each: a shard that holds what ONE row of 1,000 units at 10 asks (775 KAS) takes 1,000
///      units in each of its four rows; it is then asked four times what it holds, and a call is due at once.
#[test]
fn t7_s2_the_gate_is_of_what_is_held_and_of_one_row() {
    let w = world();
    let base = seeded(&w);
    let p13 = P10 * 13 / 10;
    let user_leg = fleg(&w, 0, 5, LONG, P10);
    let l4 = Shard { held: 20 * KAS1, ..base.s.booked(&user_leg).unwrap() };
    assert!(!l4.call_due(&[p13, 0, 0, 0]));
    assert_eq!(l4.equity(&[p13, 0, 0, 0]), Some(5 * KAS1));
    // (a)
    let two = fleg(&w, 0, 2, LONG, p13);
    assert_eq!(row_need(7, 7, p13), 70_525 * KAS1 / 10_000);
    assert!(gate_takes(&w, &base, 20 * KAS1, l4, two, 0), "2 units more enter the shard of L4");
    let a = l4.entered(&two, 0, T0).unwrap();
    assert!(a.call_due(&[p13, 0, 0, 0]), "(a) a call is due at once");
    // (b)
    let other = fleg(&w, 1, 25, LONG, P10);
    assert_eq!(row_need(25, 25, P10), 19_375 * KAS1 / 1_000);
    assert!(gate_takes(&w, &base, 20 * KAS1, l4, other, 1), "a leg of another row enters the shard of L4");
    let b = l4.entered(&other, 1, T0).unwrap();
    assert!(b.call_due(&[p13, P10, 0, 0]), "(b) a call is due at once");
    // (c)
    let need = row_need(1_000, 1_000, P10);
    assert_eq!(need, 775 * KAS1);
    let mut s = Shard { held: need, ..base.s };
    for i in 0..4 {
        let big = fleg(&w, i, 1_000, LONG, P10);
        assert!(gate_takes(&w, &base, need, Shard { expiry: 0, ..s }, big, i), "(c) row {i}");
        s = Shard { expiry: 0, ..Shard { expiry: 0, ..s }.entered(&big, i, T0).unwrap() };
    }
    assert_eq!((s.held, s.requirement(&[P10; 4])), (need, Some(4 * need)));
    assert!(s.call_due(&[P10; 4]), "(c) a call is due at once");
    println!("T7 S2: (a) 2 units more and (b) 250 KAS of another row enter the shard of L4 (20 KAS held, an equity of 5) and a call is due at once; (c) a shard that holds {} KAS takes 1,000 units at 10 in each of four rows and is then asked {} KAS", kas(need), kas(4 * need));
}
