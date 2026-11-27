//! Review T5 (docs/audit/REVIEW-T5-trace.md): a draw and a call as traces, on four rows (docs/ARCHITECTURE-CLEARING.md
//! 17.8b; the second of the four commits of 17.8a), at the script engine. The reviewer's file: its world and helpers
//! are those of `auditT3_legtime.rs` (after `thin_draw.rs`), and its own below them.
#![allow(dead_code)]

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cite;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::cross::{XLeg, XTerms};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::reserve::{Head, Shard, ShardRow, SHARD_CARRIER};
use kpf_protocol::state::LONG;
use kpf_protocol::thin::{Account, Out, Outs, Pointed, Stated};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{p2pk_spk, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::cell::RefCell;
use kpf_protocol::covered::{CSaid, KIND_C_LEAVE};
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
    // (Review T5: twelve markets, the first four those of the other files: a random shard takes its rows of them.)
    let markets = (0..12u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
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
// Review T5: the reviewer's own helpers.

use kpf_protocol::reserve::{Draw, Priced, Traced, KIND_CALL};
use kpf_protocol::thin::KIND_DRAW;

const BPS: i64 = 10_000;

/// A statement of the kind `kind` (5 a draw, 11 a call) that says the bytes `said` on the shard, with the outputs
/// given, written whatever the rule says.
fn stated_as(shard: &OLive<Shard>, kind: i64, said: &[u8], outs: Outs, claimer: [u8; 32], t0: i64) -> Stated {
    let s = &shard.s;
    Stated { position: s.hash(), window: s.dispute_daa, late: s.t.late_daa, kind, t0, arg: blake2b(said), outs, bond: if kind == KIND_CALL { s.call_bond() } else { s.bond() }, claimer, held: shard.value as i64 }
}
/// The outputs a draw of `amount` stated at `t0` writes: the shard holding the amount less, the amount to the claimer.
fn draw_outs(shard: &OLive<Shard>, amount: i64, t0: i64, claimer: [u8; 32]) -> Option<Outs> {
    Outs::of(&[Out { key: false, to: shard.s.drawn(amount, t0).hash(), value: shard.value as i64 - amount }, Out { key: true, to: claimer, value: amount }])
}
/// The outputs a call stated at `t0` writes, its object holding the bytes `list`: the shard called, the call, the fee.
fn call_outs_of(shard: &OLive<Shard>, list: &[u8], t0: i64, claimer: [u8; 32]) -> Option<Outs> {
    let s = &shard.s;
    let fee = s.t.keeper_fee_min.max(0);
    let pay = if s.held < fee { 0 } else { fee };
    let next = Shard { held: s.held - pay, called: t0, ..*s };
    let mut v = vec![(Out { key: false, to: next.hash(), value: shard.value as i64 - pay - CALL_HOLD }, false), (Out { key: false, to: Called::hash_of(&s.sid, t0, list), value: CALL_HOLD }, true)];
    if pay > 0 {
        v.push((Out { key: true, to: claimer, value: pay }, false));
    }
    Outs::with_notes(&v)
}
/// The list's bytes of what is said.
fn list_of(said: &[u8]) -> &[u8] {
    &said[16..16 + 56 * ((said.len() - 184) / 56)]
}

/// The statement `st` stated on the shard with the bytes `said`: taken by `KpfTChange` or not.
fn states(w: &World, shard: &OLive<Shard>, st: &Stated, said: &[u8]) -> Option<OLive<Stated>> {
    let who = if st.claimer == xk(&w.c) { &w.c } else { &w.w };
    let f = [fund(xk(who), 30)];
    let s = &shard.s;
    let b = state_raw(&w.lib, shard, &s.fixed_hash(), &s.moving(), st, said, s.form(), &w.script("KpfTChange"), w.payer(who, &f)).ok()?;
    let sg = b.sign(&w.keys).ok()?;
    kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok().then(|| nth(&sg, 0, *st, (st.t0 + 10) as u64))
}

/// The answer of the point `at` of the disputed statement `q`, whose bytes are `raw`, with the verdict given. The
/// builders take a trace and write its bytes; `raw` need not be what `Traced` writes back (a number of nothing
/// written below zero reads as nothing). So the answer is built of the trace read from `raw`, on the same statement
/// with that trace's hash, and the bytes said, the object and the statement that stands again are put in after.
/// For bytes that `Traced` writes back as they are, nothing is changed by it. `cite`: the citation of a price.
fn answer_raw(w: &World, q: &OLive<Pointed>, s: &Shard, raw: &[u8], at: i64, verdict: bool, cite: Option<&Cited>) -> Option<OBuilt> {
    answer_as(w, q, s, raw, &loose(raw)?, at, verdict, cite)
}

/// (Second round.) What is said with every number of nothing written below zero made eight zero bytes: the
/// numbers a script reads of it. `Traced::read` reads no trace of bytes it does not write back (since R1).
fn canon(raw: &[u8]) -> Vec<u8> {
    let mut v = raw.to_vec();
    let ne = (raw.len() - 184) / 56;
    let mut at: Vec<usize> = vec![0, 8, 16 + 56 * ne];
    for e in 0..ne {
        at.extend([16 + 56 * e + 32, 16 + 56 * e + 40, 16 + 56 * e + 48]);
    }
    at.extend((0..20).map(|i| 24 + 56 * ne + 8 * i));
    for a in at {
        if v[a..a + 8] == [0, 0, 0, 0, 0, 0, 0, 0x80] {
            v[a + 7] = 0;
        }
    }
    v
}
/// The trace whose numbers are those a script reads of `raw`.
fn loose(raw: &[u8]) -> Option<Traced> {
    if raw.len() < 184 || (raw.len() - 184) % 56 != 0 {
        return None;
    }
    Traced::read(&canon(raw))
}

/// As `answer_raw`, the builder given the trace `t` (what decides whether it writes a citation in: an entry in
/// its order and of no day is cited). The bytes shown to the script are `raw` all the same.
#[allow(clippy::too_many_arguments)]
fn answer_as(w: &World, q: &OLive<Pointed>, s: &Shard, raw: &[u8], t: &Traced, at: i64, verdict: bool, cite: Option<&Cited>) -> Option<OBuilt> {
    let qc = OLive { s: Pointed { st: Stated { arg: t.arg(), ..q.s.st }, ..q.s }, ..q.clone() };
    let f = [fund(xk(&w.f), 36)];
    let p = w.payer(&w.f, &f);
    let cs = w.script("KpfTCite");
    let (b, said_at) = if at == 0 {
        if q.s.st.kind == KIND_CALL {
            (call_end_t(&w.lib, &qc, s, t, &w.script("KpfTCallEnd"), xk(&w.f), p, Some(verdict)).ok()?.0, 7)
        } else {
            (shard_end_t(&w.lib, &qc, s, t, &w.script("KpfTShardEnd"), xk(&w.f), p, Some(verdict)).ok()?.0, 7)
        }
    } else if at <= 4 {
        (shard_row_t(&w.lib, &qc, s, t, at, &w.script("KpfTShardRow"), xk(&w.f), p, Some(verdict)).ok()?.0, 8)
    } else {
        (shard_price_t(&w.lib, &qc, s, t, at, &w.script("KpfTShardPrice"), cite.map(|c| (&cs, c)), xk(&w.f), p, Some(verdict)).ok()?.0, 8)
    };
    // (The place of what is said among the arguments: 7 of an end's, 8 of a row's and of a price's.)
    assert!(matches!(&args_of(&mut b.clone(), 1)[said_at], Arg::V(ArtifactValue::Bytes(x)) if x[..] == t.said()[..]), "the bytes said are argument {said_at}");
    let mut b = with_arg(&b, said_at, bytes_arg(raw));
    object_made(&mut b, &w.lib, 0, &q.s.hash());
    if verdict {
        b.tx.outputs[0].script_public_key = hash_spk(&w.lib, &q.s.st.again().hash());
    }
    Some(b)
}
/// What the engine takes of the answer of the point `at`: (right is taken, wrong is taken).
fn taken_as(w: &World, q: &OLive<Pointed>, s: &Shard, raw: &[u8], t: &Traced, at: i64, cite: Option<&Cited>) -> (bool, bool) {
    let take = |v: bool| answer_as(w, q, s, raw, t, at, v, cite).is_some_and(|b| accepted(&b, &w.keys));
    (take(true), take(false))
}
fn taken_raw(w: &World, q: &OLive<Pointed>, s: &Shard, raw: &[u8], at: i64, cite: Option<&Cited>) -> (bool, bool) {
    let take = |v: bool| answer_raw(w, q, s, raw, at, v, cite).is_some_and(|b| accepted(&b, &w.keys));
    (take(true), take(false))
}

// ================================================================================================================
// R1 (found in the first round, closed in the second), and the other numbers of nothing.

/// Eight bytes of nothing below zero: a script that reads a number reads nothing of them.
const BELOW: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0x80];

/// The books of these tests: a shard short 10 units of market 0 and 3 of market 1 from 10, holding `held`.
fn two_short(w: &World, held: i64) -> (OLive<Shard>, XLeg, XLeg) {
    let base = seeded(w);
    let (l0, l1) = (fleg(w, 0, 10, LONG, P10), fleg(w, 1, 3, LONG, P10));
    let books = base.s.booked(&l0).unwrap().booked(&l1).unwrap();
    (OLive { s: Shard { held, ..books }, value: (held + SHARD_CARRIER) as u64, ..base }, l0, l1)
}
/// The market of the world's that the entry `e` of a list is of.
fn market_of(w: &World, t: &Traced, e: usize) -> usize {
    (0..w.markets.len()).find(|i| w.head(*i).market == t.list[e].market).expect("a market of the world")
}

/// R1, closed (second round). `KpfTShardRow` and `KpfTShardPrice` now hold the day's eight bytes to eight bytes of
/// nothing, as `KpfTCStep` does, and `Traced::read` reads no trace of bytes it does not write back. The call of
/// the first round (a shard of 20 KAS short 10 units of market 0 and 3 of market 1, at 13; the last byte of the
/// day of market 0's entry `0x80`), and the same of a DRAW of a KAS on those books holding 500. At the engine, the
/// answers written past the builders (the trace read with that day as nothing; the bytes, the object and the
/// output put in after):
///  - it is stated (`KpfTChange` holds the length alone);
///  - the row of that market (the point 1) is answered WRONG and not right;
///  - the price of that entry (the point 5 + its place) is answered WRONG with nothing cited, and not right; with
///    its market's citation shown, no answer of it is taken, right or wrong;
///  - the end, the other rows, the other price and the points beyond are answered right and not wrong;
///  - under a dispute of the end, which is right, the row's wrong answer and the price's are each taken: the
///    shard again as it was, not called and nothing drawn. So the statement does not stand where anyone looks.
/// Fails if a point of it is taken another way.
#[test]
fn t5_r1_closed_a_day_of_nothing_below_zero_is_wrong_at_its_row_and_at_its_price_and_the_statement_does_not_stand() {
    let w = world();
    let p13 = P10 * 13 / 10;
    let up = [p13, p13, 0, 0];
    for call in [true, false] {
        let (shard, ..) = two_short(&w, if call { 20 * KAS1 } else { 500 * KAS1 });
        let s = &shard.s;
        let claimer = if call { xk(&w.w) } else { xk(&w.c) };
        let t = s.traced(&Draw { amount: if call { 0 } else { KAS1 }, prices: up }, call).unwrap();
        let e0 = t.rows[0].at as usize;
        let mut raw = t.said();
        raw[16 + 56 * e0 + 40..16 + 56 * e0 + 48].copy_from_slice(&BELOW);
        assert_eq!((Traced::read(&raw), loose(&raw)), (None, Some(t.clone())), "the model reads no trace of these bytes; their numbers are the rule's");
        // (Control: with eight zero bytes there the statement is the rule's.)
        let outs = |said: &[u8]| if call { call_outs_of(&shard, list_of(said), T0, claimer) } else { draw_outs(&shard, t.amount, T0, claimer) }.unwrap();
        let kind = if call { KIND_CALL } else { KIND_DRAW };
        assert_eq!(s.trace_wrong_point(&t, call, &claimer, &outs(&t.said()), shard.value as i64, T0, &up), None);
        let st = stated_as(&shard, kind, &raw, outs(&raw), claimer, T0);
        let live = states(&w, &shard, &st, &raw).expect("stated: the length is a trace's");
        // (What the builder is given for an answer with nothing cited of that entry: the same trace, of a day.)
        let mut of_a_day = t.clone();
        of_a_day.list[e0].day = 1;
        let price0 = 5 + e0 as i64;
        let mut got = vec![];
        for at in 0..=8i64 {
            let q = w.pointed(&live, at);
            let v = if at == price0 {
                let c = w.cited(market_of(&w, &t, e0), p13);
                assert_eq!(taken_raw(&w, &q, s, &raw, at, Some(&c)), (false, false), "call {call}: the price of the entry, its market's citation shown");
                taken_as(&w, &q, s, &raw, &of_a_day, at, None)
            } else {
                let cite = (5..=6).contains(&at).then(|| w.cited(market_of(&w, &t, (at - 5) as usize), p13));
                taken_raw(&w, &q, s, &raw, at, cite.as_ref())
            };
            let want = if at == 1 || at == price0 { (false, true) } else { (true, false) };
            assert_eq!(v, want, "R1 is open again: call {call}: the point {at} of a statement whose list writes a day of nothing below zero");
            got.push((at, v));
        }
        // Under a dispute of the end: the row's wrong answer, and the price's, each end the statement.
        let q0 = w.pointed(&live, 0);
        for (what, b) in [("the row", answer_raw(&w, &q0, s, &raw, 1, false, None)), ("the price", answer_as(&w, &q0, s, &raw, &of_a_day, price0, false, None))] {
            let b = b.expect("built");
            assert!(accepted(&b, &w.keys), "call {call}: {what} answered wrong under a dispute of the end");
            assert_eq!((&b.tx.outputs[0].script_public_key, b.tx.outputs[0].value, s.called), (&hash_spk(&w.lib, &s.hash()), shard.value, 0), "call {call}: the shard again, as it was");
        }
        println!("T5 R1 closed ({}): the point, (right taken, wrong taken): {got:?}", if call { "a call" } else { "a draw" });
    }
}

/// Second round: the other numbers of nothing, and the sign as the only difference. A number of eight bytes has
/// two forms only where it is nothing. In a statement of the rule's the numbers that are nothing are: an entry's
/// day (R1); the place of the price of the row that names the list's first entry; the place in the books of row
/// 1; the amount of a call; the sums up to a row before any worth; the count of an empty list. Each of the last
/// five is read by the answers alone, as a number, and by nothing that compares bytes: `KpfTCStep` is shown the
/// object, which is the list (bytes 16 on of what is said) and has none of them. Run, each written as nothing
/// below zero in a statement that is else the rule's: every point is answered right and none wrong, by the same
/// answers as with eight zero bytes; and for the call, applied, its object is the very object of the rule's call
/// and an account is stepped under it as the builder writes the step. One reader, or two that read alike.
/// The model reads none of the five (`Traced::read` is `None`): see R7 below.
/// And the sign as the only difference where a number is not nothing is another number, to every reader: an
/// entry's limit below nothing is wrong at its row (and the step would compare its bytes with the leg's); an
/// entry's price of nothing below zero is a price of nothing: wrong at its row and at its own point.
#[test]
fn t5_r1_the_other_numbers_of_nothing_have_one_way_of_being_read() {
    let w = world();
    let p13 = P10 * 13 / 10;
    let up = [p13, p13, 0, 0];
    let ff = [fund(xk(&w.f), 36)];
    let all_right = |what: &str, shard: &OLive<Shard>, call: bool, t: &Traced, raw: &[u8]| -> OLive<Stated> {
        assert_eq!((Traced::read(raw), loose(raw).as_ref(), raw != &t.said()[..]), (None, Some(t), true), "{what}");
        let s = &shard.s;
        let claimer = if call { xk(&w.w) } else { xk(&w.c) };
        let outs = if call { call_outs_of(shard, list_of(raw), T0, claimer) } else { draw_outs(shard, t.amount, T0, claimer) }.unwrap();
        let st = stated_as(shard, if call { KIND_CALL } else { KIND_DRAW }, raw, outs, claimer, T0);
        let live = states(&w, shard, &st, raw).unwrap_or_else(|| panic!("{what}: stated"));
        let ne = t.list.len() as i64;
        for at in 0..=5 + ne {
            let cite = (5..5 + ne).contains(&at).then(|| w.cited(market_of(&w, t, (at - 5) as usize), p13));
            assert_eq!(taken_raw(&w, &w.pointed(&live, at), s, raw, at, cite.as_ref()), (true, false), "{what}: the point {at}");
        }
        live
    };
    let below = |raw: &[u8], at: usize| {
        let mut x = raw.to_vec();
        assert_eq!(x[at..at + 8], [0u8; 8], "(a number of nothing)");
        x[at..at + 8].copy_from_slice(&BELOW);
        x
    };
    let (shard, l0, _) = two_short(&w, 20 * KAS1);
    let t = shard.s.traced(&Draw { amount: 0, prices: up }, true).unwrap();
    let ro = 24 + 56 * t.list.len();
    let first = (0..2).find(|r| t.rows[*r].at == 0).unwrap();
    let mut run = vec![];
    for (what, at) in [("the place of the price of the row that names the first entry", ro + 40 * first + 8), ("the place in the books of row 1", ro), ("the amount of a call", 0)] {
        let raw = below(&t.said(), at);
        let live = all_right(what, &shard, true, &t, &raw);
        // Applied: the object is the rule's call's, and an account of market 0 is stepped as the builder writes it.
        let ap = signed(&format!("{what}, nothing below zero: the call applied"), &stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.f, &ff)).unwrap(), &w.keys);
        let (next, call) = shard.s.called_at(&up, T0, shard.s.t.keeper_fee_min);
        assert_eq!((list_of(&raw), &ap.tx.outputs[0].script_public_key), (&call.body()[..], &hash_spk(&w.lib, &next.hash())));
        let called = nth(&ap, 0, next, 6_800);
        let call = nth(&ap, out_of(&ap, &hash_spk(&w.lib, &call.hash())).0, call, 6_800);
        let a = cov(&w, &called, &entered(&[l0], 0), 40 * KAS1);
        let (b, _) = call_step(&w.lib, &a, &a.s, None, &call, &w.script("KpfTCStep"), w.payer(&w.f, &ff)).unwrap();
        assert!(accepted(&b, &w.keys), "{what}: an account stepped under the call");
        run.push(what);
    }
    // The sums of a row before any worth: the same books with row 1 empty.
    let shifted = OLive { s: Shard { rows: [ShardRow::EMPTY, shard.s.rows[0], shard.s.rows[1], ShardRow::EMPTY], ..shard.s }, ..shard.clone() };
    let t2 = shifted.s.traced(&Draw { amount: 0, prices: [0, p13, p13, 0] }, true).unwrap();
    assert_eq!((t2.rows[0].wn, t2.rows[0].wg, t2.rows[0].sn), (0, 0, 0));
    let raw = below(&below(&below(&t2.said(), ro + 16), ro + 24), ro + 32);
    all_right("the three sums of an empty row 1", &shifted, true, &t2, &raw);
    run.push("the three sums of an empty first row");
    // The count of an empty list, and the last row's sums the end reads: a draw on a shard of no market.
    let none = bare(&w);
    let t3 = none.s.traced(&Draw { amount: KAS1, prices: [0; 4] }, false).unwrap();
    assert_eq!((t3.list.len(), t3.said().len()), (0, 184));
    let raw = below(&below(&below(&below(&t3.said(), 8), 160), 168), 176);
    all_right("the count of an empty list and the last row's sums", &none, false, &t3, &raw);
    run.push("the count of an empty list, and the sums the end reads");
    // The sign as the only difference, where the number is not nothing.
    let e0 = t.rows[0].at as usize;
    let mut limit = t.said();
    limit[16 + 56 * e0 + 39] |= 0x80;
    let mut price = t.said();
    price[16 + 56 * e0 + 48..16 + 56 * e0 + 56].copy_from_slice(&BELOW);
    for (what, raw, at_price) in [("an entry's limit below nothing", limit, false), ("an entry's price of nothing below zero", price, true)] {
        let st = stated_as(&shard, KIND_CALL, &raw, call_outs_of(&shard, list_of(&raw), T0, xk(&w.w)).unwrap(), xk(&w.w), T0);
        let live = states(&w, &shard, &st, &raw).unwrap_or_else(|| panic!("{what}: stated"));
        assert_eq!(taken_raw(&w, &w.pointed(&live, 1), &shard.s, &raw, 1, None), (false, true), "{what}: its row");
        if at_price {
            let c = w.cited(market_of(&w, &t, e0), p13);
            assert_eq!(taken_raw(&w, &w.pointed(&live, 5 + e0 as i64), &shard.s, &raw, 5 + e0 as i64, Some(&c)), (false, true), "{what}: its own point");
        }
    }
    println!("T5 second round: written as nothing below zero in a statement of the rule's, every point right and none wrong: {run:?}");
}

/// R7 (Low; second round). The model reads no trace of bytes it does not write back (the fix of R1), and the
/// scripts read every number of nothing in either form but an entry's day. So a statement that is NOT the rule's
/// and writes one such number below zero is judged by the scripts and read by no function of the model: `Traced::
/// read` and `Draw::read` are `None`, and every builder of an answer takes a `Traced`. Whoever watches with the
/// tool cannot name its wrong point nor answer it; it would have to dispute blind.
/// Here: a draw of a KAS on the books of 500 KAS, the worth of the net a sompi more from row 2 on (row 2 is wrong,
/// the end and the other rows right), the place in the books of row 1 written as nothing below zero. Stated. At
/// the engine, the answer written past the builders: row 2 is answered wrong and not right, row 1 right. Read as
/// the scripts read it (the reviewer's `loose`), the model names row 2.
/// Passes while the model reads nothing of it.
#[test]
fn t5_r7_a_wrong_statement_with_a_number_of_nothing_below_zero_is_judged_by_the_scripts_and_read_by_no_model() {
    let w = world();
    let p13 = P10 * 13 / 10;
    let up = [p13, p13, 0, 0];
    let (shard, ..) = two_short(&w, 500 * KAS1);
    let s = &shard.s;
    let owner = xk(&w.c);
    let t = s.traced(&Draw { amount: KAS1, prices: up }, false).unwrap();
    let mut lie = t.clone();
    lie.rows[1..].iter_mut().for_each(|r| r.wn += 1);
    let ro = 24 + 56 * t.list.len();
    let mut raw = lie.said();
    raw[ro..ro + 8].copy_from_slice(&BELOW);
    let st = stated_as(&shard, KIND_DRAW, &raw, draw_outs(&shard, KAS1, T0, owner).unwrap(), owner, T0);
    let live = states(&w, &shard, &st, &raw).expect("stated");
    assert_eq!(taken_raw(&w, &w.pointed(&live, 2), s, &raw, 2, None), (false, true), "row 2, by the scripts");
    assert_eq!(taken_raw(&w, &w.pointed(&live, 1), s, &raw, 1, None), (true, false), "row 1, its place written below zero");
    assert_eq!(taken_raw(&w, &w.pointed(&live, 0), s, &raw, 0, None), (true, false), "the end");
    assert_eq!(s.trace_wrong_point(&loose(&raw).unwrap(), false, &owner, &st.outs, shard.value as i64, T0, &up), Some(2), "read as the scripts read it, the model names row 2");
    assert!(Traced::read(&raw).is_none() && Draw::read(&raw).is_none(), "R7 is closed: the model reads what the scripts read");
}

// ================================================================================================================
// B1. The model against the scripts, on random shards.

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: i64) -> i64 {
        (self.next() % n as u64) as i64
    }
}

/// A random shard of one to four markets of the world's twelve, in rows taken at random (an empty row may stand
/// between two that have a market); nets of both signs, of nothing, and of the whole gross; sizes that are not
/// whole units; a price a row. `call`: its equity is far below its lower level at those prices; else far above
/// its upper level. And the market of each row.
fn random_books(w: &World, r: &mut Rng, call: bool) -> (OLive<Shard>, [i64; 4], [Option<usize>; 4]) {
    let scale = w.markets[0].p.scale;
    let mut rows = [ShardRow::EMPTY; 4];
    let mut prices = [0i64; 4];
    let mut of = [None; 4];
    let k = 1 + r.below(4) as usize;
    let mut places: Vec<usize> = vec![0, 1, 2, 3];
    let mut markets: Vec<usize> = (0..w.markets.len()).collect();
    for _ in 0..k {
        let row = places.remove(r.below(places.len() as i64) as usize);
        let m = markets.remove(r.below(markets.len() as i64) as usize);
        let gross = (1 + r.below(300)) * scale + r.below(scale);
        let net = match r.below(5) {
            0 => 0,
            1 => gross,
            2 => -gross,
            _ => r.below(2 * gross + 1) - gross,
        };
        rows[row] = ShardRow { head: w.head(m), net, gross };
        prices[row] = P10 / 2 + r.below(3 * P10);
        of[row] = Some(m);
    }
    let mut s = Shard { t: w.terms(), rows, owner: xk(&w.c), held: 0, expiry: 0, dispute_daa: 50, bond_bps: 1_000, sid: [0x5d; 32], basis: 0, called: 0, owed: 0, claims: Default::default() };
    let sn = s.equity(&prices).unwrap();
    s.held = (1 + r.below(900)) * KAS1 + r.below(KAS1);
    s.basis = if call { sn + s.held + 10_000 * KAS1 } else { sn - 10_000 * KAS1 };
    (OLive { s, value: (s.held + SHARD_CARRIER) as u64, id: Some(Hash::from_bytes([0x73; 32])), op: op(12), daa: 6_100 }, prices, of)
}

/// Whether the entry `e` (from 1) of a trace's list is one a citation is shown for: in its order, of no day.
fn cited_entry(t: &Traced, e: usize) -> bool {
    t.list.get(e.wrapping_sub(1)).is_some_and(|en| (e < 2 || t.list[e - 2].below(en)) && en.day == 0)
}

/// The citation the answer of the point `at` (5 and above) of the trace `t` shows: the committee's price `c` (one a
/// row, moved `bps`) of the market of the row whose name the entry has; none where the entry is out of its order,
/// of a day, beyond the list, or of no row.
fn price_cite(w: &World, s: &Shard, of: &[Option<usize>; 4], c: &[i64; 4], t: &Traced, at: i64, bps: i64) -> Option<Cited> {
    let e = usize::try_from(at - 4).ok()?;
    if !cited_entry(t, e) {
        return None;
    }
    let r = s.rows.iter().position(|x| x.head.market == t.list[e - 1].market && x.head.market != [0; 32])?;
    Some(w.cited(of[r]?, c[r] * (BPS + bps) / BPS))
}

/// Traces that are not the rule's, made of the rule's `t`: one thing changed in each.
fn lies_of(t: &Traced, s: &Shard, r: &mut Rng) -> Vec<(String, Traced)> {
    let mut v: Vec<(String, Traced)> = vec![];
    let ne = t.list.len();
    let i = r.below(4) as usize;
    let k = r.below(ne as i64) as usize;
    // (A row that has a market, and its place in the trace.)
    let m = (0..4).map(|j| (i + j) % 4).find(|j| s.rows[*j].head.market != [0; 32]).unwrap();
    {
        let mut put = |what: &str, f: &dyn Fn(&mut Traced)| {
            let mut x = t.clone();
            f(&mut x);
            v.push((what.to_string(), x));
        };
        put("the amount a sompi more", &|x| x.amount += 1);
        put("the list's count as written, one more", &|x| x.e += 1);
        put("the list's count as written, one less", &|x| x.e -= 1);
        put("the rows' count as written, five", &|x| x.n = 5);
        put("the rows' count as written, three", &|x| x.n = 3);
        put("a row's place in the books, one more", &|x| x.rows[i].place += 1);
        put("a row's place in the books, below nothing", &|x| x.rows[i].place = -1);
        put("a row's place in the books, the largest number", &|x| x.rows[i].place = i64::MAX);
        put("the place of a row's price, one more", &|x| x.rows[m].at += 1);
        put("the place of a row's price, below nothing", &|x| x.rows[m].at = -1);
        put("the place of a row's price, the largest number", &|x| x.rows[m].at = i64::MAX);
        put("the place of a row's price, 2^62", &|x| x.rows[m].at = 1 << 62);
        put("a row's worth of the net, a sompi more", &|x| x.rows[i].wn += 1);
        put("a row's worth of the gross, a sompi less", &|x| x.rows[i].wg -= 1);
        put("a row's signed worth, a sompi less", &|x| x.rows[i].sn -= 1);
        put("the worth of the net seven more from a row on", &|x| x.rows[i..].iter_mut().for_each(|y| y.wn += 7));
        put("the signed worth below nothing in the last row", &|x| x.rows[3].sn = -x.rows[3].sn - 1);
        put("a row that has a market written as one that has none", &|x| {
            let p = if m == 0 { (0, 0, 0) } else { (x.rows[m - 1].wn, x.rows[m - 1].wg, x.rows[m - 1].sn) };
            (x.rows[m].wn, x.rows[m].wg, x.rows[m].sn) = p;
        });
        put("an entry's limit one more", &|x| x.list[k].limit += 1);
        put("an entry of a day", &|x| x.list[k].day = 1);
        put("an entry's price a sompi more, the sums as they were", &|x| x.list[k].price += 1);
        put("an entry's market, a bit of its last byte", &|x| x.list[k].market[31] ^= 1);
        put("an entry's market, the sign of its first number", &|x| x.list[k].market[7] ^= 0x80);
        put("no list", &|x| {
            x.list.clear();
            x.e = 0;
            x.rows.iter_mut().for_each(|y| (y.at, y.wn, y.wg, y.sn) = (0, 0, 0, 0));
        });
        if ne >= 2 {
            put("the first two entries the other way, the rows naming them as they stand", &|x| {
                x.list.swap(0, 1);
                x.rows.iter_mut().for_each(|y| y.at = [1, 0, 2, 3][y.at as usize]);
            });
            put("every row's price the next entry's", &|x| x.rows.iter_mut().for_each(|y| y.at = (y.at + 1) % ne as i64));
        }
        if ne < 4 {
            put("the first entry twice, the second at the end", &|x| {
                x.list.push(x.list[0]);
                x.e += 1;
            });
            put("an entry of a market no row has, at the end in its order", &|x| {
                let mut market = [0xff; 32];
                market[7] = 0x7f;
                x.list.push(Priced { market, limit: x.list[0].limit, day: 0, price: P10 });
                x.e += 1;
            });
        }
    }
    v
}

/// B1. The model against the scripts, at the engine, on 14 random shards, a draw and a call on each.
/// The rule's trace: `trace_wrong_point` names none; every point (the end, the four rows, each price of the list
/// with the committee's citation, the two after the list, 99, 2^62 and the largest number) is answered right and
/// not wrong; one price of each, cited 3 % away, is answered wrong and not right.
/// Then about thirty lies a statement (one number of the trace moved, the counts, the list reordered, an entry
/// twice, an entry of no row, no list, a row's price another entry's, a row written as empty; and a false price
/// with the trace the rule makes of it): each is stated (`KpfTChange` holds the length alone), the model names its
/// first wrong point, and at that point the engine takes the wrong answer and not the right; under a dispute of
/// another point the same wrong answer is taken, and no right one. An entry in its order that no row is of has no
/// citation: nobody answers its point (the builder refuses; the engine is run on it in B3).
/// And: the lists are in the order of the names' NUMBERS, which in these shards is not the order of their bytes
/// in at least one list, and some first numbers are below nothing.
#[test]
fn t5_b1_the_trace_of_a_draw_and_of_a_call_on_random_shards_the_model_and_the_engine() {
    let w = world();
    let mut r = Rng(0x17_08b5);
    let (mut honest, mut points, mut lies, mut still_right, mut orphans, mut by_point) = (0, 0, 0, 0, 0, std::collections::BTreeMap::<i64, usize>::new());
    let (mut not_byte_order, mut below_nothing, mut empty_between) = (0, 0, 0);
    for case in 0..14 {
        for call in [false, true] {
            let (shard, prices, of) = random_books(&w, &mut r, call);
            let s = &shard.s;
            let held = shard.value as i64;
            let (st, d) = if call { s.call(prices, T0, held, xk(&w.w)).unwrap() } else { s.draw(1 + r.below(s.held), prices, T0, held).unwrap() };
            let kind = st.kind;
            let t = s.traced(&d, call).unwrap();
            let raw = t.said();
            let ne = t.list.len() as i64;
            assert_eq!((kind, raw.len() as i64, blake2b(&raw)), (if call { KIND_CALL } else { KIND_DRAW }, 184 + 56 * ne, st.arg));
            let by_bytes = { let mut x: Vec<[u8; 32]> = t.list.iter().map(|e| e.market).collect(); x.sort(); x };
            not_byte_order += (by_bytes != t.list.iter().map(|e| e.market).collect::<Vec<_>>()) as usize;
            below_nothing += t.list.iter().filter(|e| e.numbers()[0] < 0).count();
            empty_between += (0..3).any(|j| s.rows[j].head.market == [0; 32] && s.rows[j + 1..].iter().any(|x| x.head.market != [0; 32])) as usize;
            let live = states(&w, &shard, &st, &raw).unwrap_or_else(|| panic!("case {case}: the rule's statement is stated"));
            assert_eq!(s.trace_wrong_point(&t, call, &st.claimer, &st.outs, held, T0, &prices), None, "case {case}");
            // Every point of the rule's trace, and beyond.
            for at in (0..=4 + ne).chain([5 + ne, 6 + ne, 99, 1 << 62, i64::MAX]) {
                let q = w.pointed(&live, at);
                let cite = price_cite(&w, s, &of, &prices, &t, at, 0);
                assert_eq!(cite.is_some(), (5..=4 + ne).contains(&at));
                assert_eq!(taken_raw(&w, &q, s, &raw, at, cite.as_ref()), (true, false), "case {case}, call {call}: the point {at} of the rule's trace");
                points += 1;
            }
            // One price, the committee's 3 % away: wrong, and not right.
            let at = 5 + r.below(ne);
            let far = price_cite(&w, s, &of, &prices, &t, at, if r.below(2) == 0 { 300 } else { -300 });
            assert_eq!(taken_raw(&w, &w.pointed(&live, at), s, &raw, at, far.as_ref()), (false, true), "case {case}: a price 3 % from the committee's");
            honest += 1;
            // The lies.
            let mut all = lies_of(&t, s, &mut r);
            let m = (0..4).find(|j| s.rows[*j].head.market != [0; 32]).unwrap();
            let mut false_prices = prices;
            false_prices[m] = prices[m] * 105 / 100;
            if let Some(x) = s.traced(&Draw { amount: d.amount, prices: false_prices }, call) {
                all.push(("a price 5 % from the committee's, the trace the rule makes of it".into(), x));
            }
            for (what, lie) in all {
                let raw = lie.said();
                let outs = if call { call_outs_of(&shard, list_of(&raw), T0, st.claimer) } else { draw_outs(&shard, lie.amount, T0, st.claimer) };
                let Some(outs) = outs else { continue };
                let lst = stated_as(&shard, kind, &raw, outs, st.claimer, T0);
                let live = states(&w, &shard, &lst, &raw).unwrap_or_else(|| panic!("case {case}: {what}: not stated"));
                let Some(p) = s.trace_wrong_point(&lie, call, &lst.claimer, &lst.outs, held, T0, &prices) else {
                    // (The place of the price of a row that has no market is read by nothing; an amount a sompi
                    // more may still be the rule's.)
                    still_right += 1;
                    continue;
                };
                let cite = price_cite(&w, s, &of, &prices, &lie, p, 0);
                let q = w.pointed(&live, p);
                let v = taken_raw(&w, &q, s, &raw, p, cite.as_ref());
                if p >= 5 && cite.is_none() && cited_entry(&lie, (p - 4) as usize) {
                    assert_eq!(v, (false, false), "case {case}: {what}: an entry of no row");
                    orphans += 1;
                    continue;
                }
                assert_eq!(v, (false, true), "case {case}, call {call}: {what}: the point {p} the model names");
                let other = w.pointed(&live, if p == 0 { 3 } else { 0 });
                let under = |verdict: bool| answer_raw(&w, &other, s, &raw, p, verdict, cite.as_ref()).is_some_and(|b| accepted(&b, &w.keys));
                assert_eq!((under(true), under(false)), (false, true), "case {case}: {what}: the point {p}, under a dispute of another point");
                *by_point.entry(p.min(5)).or_default() += 1;
                lies += 1;
            }
        }
    }
    println!("T5 B1: {honest} statements of the rule's (a draw and a call on each of 14 random shards): {points} points answered right and not wrong, and a price 3 % away wrong on each; {lies} lies each answered wrong at the point the model names, and under a dispute of another point (by point, 5 for a price: {by_point:?}); {still_right} changes that left the trace the rule's; {orphans} entries of no row, which nobody answers. Lists not in the order of their bytes: {not_byte_order}; first numbers below nothing: {below_nothing}; shards with an empty row before a row that has a market: {empty_between}");
    assert!(lies >= 600 && orphans >= 10 && by_point.len() == 6 && not_byte_order >= 1 && below_nothing >= 5 && empty_between >= 3, "{lies} {orphans} {by_point:?} {not_byte_order} {below_nothing} {empty_between}");
}

// ================================================================================================================
// B2, B3. What the end does not look at; sums at the end of 64 bits; an entry of no row; the lapse.

/// A shard of 100 KAS over two rows: market 0, its side long 10 units; market 1, its side short 60 units; every leg
/// from 10. (A row that is short AFTER a row: what `KpfTShardRow` adds before it looks at the sign.)
fn long_then_short(w: &World) -> OLive<Shard> {
    let x = w.shard();
    let scale = w.markets[0].p.scale;
    let rows = [ShardRow { head: w.head(0), net: 10 * scale, gross: 30 * scale }, ShardRow { head: w.head(1), net: -60 * scale, gross: 140 * scale }, ShardRow::EMPTY, ShardRow::EMPTY];
    OLive { s: Shard { rows, ..x.s }, ..x }
}

/// B2 and B3: the end reads no row, and R3 (Low): the model of a row is not the script where a sum said is at the
/// end of 64 bits.
/// A draw of a KAS on a shard long 10 units of market 0 and short 60 of market 1, both at 10. Its trace is written
/// with the signed worth of row 1 the largest number, and the rows after it following from that (row 2, short,
/// 600 KAS less). At the engine:
///  - the END is answered right and not wrong: it reads the last row's sums, which make the equity as large as a
///    number goes. No row is read: whoever watches disputes the rows, not the end;
///  - row 1 is answered wrong and not right (its signed worth is not the books');
///  - row 2 is answered by NOBODY: the script adds the row before's signed worth and this row's worth before it
///    looks whether the row is short, and that sum does not fit. `trace_row_judged` says it is RIGHT (it subtracts
///    at once): the tool would build an answer the engine refuses. The same where the worth of the gross said for
///    row 1 is the largest number and row 2's worth of the net is a sompi off: the model says wrong (it stops at
///    the first sum that differs), the script adds the gross first and fails.
/// Nothing is gained by it: the statement is wrong at row 1, which is answered, whatever point was disputed; and
/// `trace_wrong_point` names row 1. It is a difference between the model and the script, as F71 was.
#[test]
fn t5_b2_the_end_reads_no_row_and_a_sum_at_the_end_of_64_bits_is_judged_by_the_model_where_the_script_fails() {
    let w = world();
    let shard = long_then_short(&w);
    let s = &shard.s;
    let prices = [P10, P10, 0, 0];
    let d = Draw { amount: KAS1, prices };
    let t = s.traced(&d, false).unwrap();
    assert_eq!((t.rows[0].sn, t.rows[1].sn), (100 * KAS1, -500 * KAS1));
    let owner = xk(&w.c);
    let run = |what: &str, lie: &Traced| -> [(bool, bool); 5] {
        let raw = lie.said();
        let st = stated_as(&shard, KIND_DRAW, &raw, draw_outs(&shard, lie.amount, T0, owner).unwrap(), owner, T0);
        let live = states(&w, &shard, &st, &raw).unwrap_or_else(|| panic!("{what}: stated"));
        [0, 1, 2, 3, 4].map(|at| taken_raw(&w, &w.pointed(&live, at), s, &raw, at, None))
    };
    // (Control: the rule's trace.)
    assert_eq!(run("the rule's", &t), [(true, false); 5]);
    // The signed worth of row 1 the largest number; the rows after follow from it.
    let mut lie = t.clone();
    lie.rows[0].sn = i64::MAX;
    for j in 1..4 {
        lie.rows[j].sn = i64::MAX - 600 * KAS1;
    }
    let got = run("the signed worth of row 1 the largest number", &lie);
    let model: Vec<Option<bool>> = (1..=4).map(|at| s.trace_row_judged(&lie, at, false)).collect();
    println!("T5 B2: the signed worth of row 1 the largest number: the engine, a point at a time: {got:?}; the model of the rows: {model:?}; of the end: {:?}", s.draw_trace_end_judged(&lie, &owner, T0));
    assert_eq!(got[0], (true, false), "the end, of sums no row's answer takes");
    assert_eq!(s.draw_trace_end_judged(&lie, &owner, T0), Some(true));
    assert_eq!((got[1], model[0]), ((false, true), Some(false)), "row 1");
    assert_eq!((got[3], got[4], model[2], model[3]), ((true, false), (true, false), Some(true), Some(true)), "the rows that have no market follow");
    assert_eq!(got[2], (false, false), "R3 is closed: the script answers row 2");
    assert_eq!(model[1], Some(true), "R3 is closed: the model of row 2 is the script's");
    assert_eq!(s.trace_wrong_point(&lie, false, &owner, &draw_outs(&shard, KAS1, T0, owner).unwrap(), shard.value as i64, T0, &prices), Some(1));
    // The worth of the gross of row 1 the largest number, and row 2's worth of the net a sompi off.
    let mut lie = t.clone();
    lie.rows[0].wg = i64::MAX;
    lie.rows[1].wn += 1;
    let got = run("the worth of the gross of row 1 the largest number", &lie);
    println!("T5 B2: the worth of the gross of row 1 the largest number, row 2's net a sompi off: the engine: {got:?}; the model of row 2: {:?}", s.trace_row_judged(&lie, 2, false));
    assert_eq!((got[1], got[2], s.trace_row_judged(&lie, 2, false)), ((false, true), (false, false), Some(false)), "R3 is closed");
}

/// B3 (and T21, T24 of the paper's review): an entry that no row is of, and the lapse of a call's point.
/// A call that is the rule's but for one entry more in its list, in its order, of a market no row has. Its end and
/// its rows are answered right (the builder; B1). The point of that entry: the engine takes no answer of it:
/// written with the citation of another entry's market, right or wrong; written with nothing cited, right or
/// wrong (`KpfTShardPrice` requires a row of the entry's name, and a citation of an entry in its order). Disputed,
/// it lapses after a dispute period: the shard again, not called; the disputer has its bond and half of the
/// caller's; the script's copy keeps the other half; the caller has nothing (`KpfTPoint.lapse`, kinds 5 and up).
/// So it costs whoever wrote it, as 17.8b says, and the call ends: an entry more is not a way to hold a call.
/// And a point below nothing is disputed by nobody (`KpfTPoint.dispute`), where the point 7 is (the control).
#[test]
fn t5_b3_an_entry_no_row_is_of_is_answered_by_nobody_and_its_dispute_lapses_at_the_callers_cost() {
    let w = world();
    let base = seeded(&w);
    let l0 = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: Shard { held: 20 * KAS1, ..base.s.booked(&l0).unwrap() }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let s = &shard.s;
    let p13 = P10 * 13 / 10;
    let prices = [p13, 0, 0, 0];
    let caller = xk(&w.c);
    let t = s.traced(&Draw { amount: 0, prices }, true).unwrap();
    let mut lie = t.clone();
    let mut market = [0xff; 32];
    market[7] = 0x7f;
    lie.list.push(Priced { market, limit: t.list[0].limit, day: 0, price: P10 });
    lie.e = 2;
    assert!(lie.list[0].below(&lie.list[1]));
    let raw = lie.said();
    let st = stated_as(&shard, KIND_CALL, &raw, call_outs_of(&shard, list_of(&raw), T0, caller).unwrap(), caller, T0);
    let live = states(&w, &shard, &st, &raw).expect("stated");
    for at in 0..=5i64 {
        let cite = (at == 5).then(|| w.cited(0, p13));
        assert_eq!(taken_raw(&w, &w.pointed(&live, at), s, &raw, at, cite.as_ref()), (true, false), "the point {at}");
    }
    assert_eq!(s.trace_wrong_point(&lie, true, &caller, &st.outs, shard.value as i64, T0, &prices), Some(6), "the model names the entry of no row");
    // The point 6, at the engine. The builder writes the answer of the point 5 (with its citation) and of a point
    // beyond the list (with none), for the same statement disputed at those points; the point answered (argument
    // 17 of `KpfTShardPrice.answer`) and the object are then made the point 6's.
    let q6 = w.pointed(&live, 6);
    let ff = [fund(xk(&w.f), 36)];
    let cs = w.script("KpfTCite");
    let c = w.cited(0, p13);
    let mut tried = 0;
    for (as_point, cite) in [(5i64, Some(&c)), (9, None)] {
        for verdict in [true, false] {
            let q = OLive { s: Pointed { point: as_point, ..q6.s }, ..q6.clone() };
            let (b, _) = shard_price_t(&w.lib, &q, s, &lie, as_point, &w.script("KpfTShardPrice"), cite.map(|c| (&cs, c)), xk(&w.f), w.payer(&w.f, &ff), Some(verdict)).unwrap();
            // (Control: as the builder wrote it, of its own point, on its own object, it is taken where it is right.)
            assert_eq!(accepted(&b, &w.keys), verdict, "(control) the point {as_point} answered {verdict}");
            assert!(matches!(&args_of(&mut b.clone(), 1)[17], Arg::V(ArtifactValue::Int(x)) if *x == as_point) && matches!(&args_of(&mut b.clone(), 1)[16], Arg::V(ArtifactValue::Int(x)) if *x == as_point), "arguments 16 and 17: the point disputed and the point answered");
            let mut x = with_arg(&with_arg(&b, 17, Arg::V(ArtifactValue::Int(6))), 16, Arg::V(ArtifactValue::Int(6)));
            object_made(&mut x, &w.lib, 0, &q6.s.hash());
            assert!(!accepted(&x, &w.keys), "the entry of no row is answered {verdict} (written as the point {as_point} is)");
            tried += 1;
        }
    }
    // The lapse.
    let lapse = signed("B3: the dispute of an entry of no row lapsed", &point_lapse(&w.lib, &q6, &w.script("KpfTPoint"), w.payer(&w.f, &ff)).unwrap(), &w.keys);
    let k = lapse.tx.outputs.len();
    let half = st.bond / 2;
    assert_eq!(&lapse.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &s.hash()), "the shard again, not called");
    assert_eq!((lapse.tx.outputs[k - 1].value, &lapse.tx.outputs[k - 1].script_public_key), ((st.bond + half) as u64, &p2pk_spk(&xk(&w.w))), "the disputer: its bond and half of the caller's");
    assert!(!lapse.tx.outputs.iter().any(|o| o.script_public_key == p2pk_spk(&caller)), "nothing to the caller");
    assert_eq!(lapse.tx.outputs[1].value - lapse.entries[1].amount, (200_000 + st.bond - half) as u64, "the copy: the other half beside its fee");
    // A point below nothing.
    let fw = [fund(xk(&w.w), 31)];
    let (b, _) = point_dispute(&w.lib, &live, &w.script("KpfTPoint"), xk(&w.w), 0, w.payer(&w.w, &fw)).unwrap();
    assert!(accepted(&b, &w.keys));
    let n = args_of(&mut b.clone(), 1).len();
    assert!(matches!(&args_of(&mut b.clone(), 1)[n - 1], Arg::V(ArtifactValue::Int(0))), "the last argument of `KpfTPoint.dispute` is the point");
    let as_point = |p: i64| {
        let mut x = with_arg(&b, n - 1, Arg::V(ArtifactValue::Int(p)));
        x.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Pointed { st, disputer: xk(&w.w), point: p }.hash());
        accepted(&x, &w.keys)
    };
    assert!(as_point(7) && as_point(i64::MAX), "(control) a point far beyond the trace is disputed");
    assert!(!as_point(-1) && !as_point(-i64::MAX), "a point below nothing is disputed");
    println!("T5 B3: an entry of no row: the points 0 to 5 answered right; the point 6: {tried} answers refused (cited and not, right and wrong); lapsed: the disputer has {} sompi (its bond of {} and half of the caller's), the caller nothing, the shard as it was", st.bond + half, st.bond);
}

// ================================================================================================================
// B4. The call's step, and the list.

/// B4. `KpfTCStep` and the list.
///  - A call's object of five entries, 280 bytes (no script writes one: `KpfTChange` holds a call's bytes to 408):
///    no step is taken under it. Of four entries, the leg's first and three of markets made up after it, out of
///    their order: the step is taken (the control of how the object is put in; and the step does not look at the
///    order: it is held where a price's point is disputed, 17.8b, T25).
///  - A call whose list says a price of nothing for the leg's market (a false call nobody disputed): the step is
///    taken at nothing, as 17.8b keeps it. A long of 10 units from 10 with 40 KAS: it lost 100 KAS, and pays all
///    it holds but the note's least.
///  - A shard of no market: no call is due (the model), and a call stated on it all the same, its list empty
///    (184 bytes), is answered wrong at its end.
#[test]
fn t5_b4_a_step_under_a_list_of_five_of_a_price_of_nothing_and_a_call_of_no_market() {
    let w = world();
    let base = seeded(&w);
    let l0 = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: Shard { held: 20 * KAS1, ..base.s.booked(&l0).unwrap() }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let ff = [fund(xk(&w.f), 36)];
    let step_s = w.script("KpfTCStep");
    let p13 = P10 * 13 / 10;
    let (called, call) = called_objs(&shard, [p13, 0, 0, 0], 6_600);
    let a = cov(&w, &called, &entered(&[l0], 0), 40 * KAS1);
    let (b, step) = call_step(&w.lib, &a, &a.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys) && step.at == vec![0] && step.note.asked == 30 * KAS1);
    let body = call.s.body();
    assert_eq!(body.len(), 56);
    let made = |n: u8| Priced { market: [n; 32], limit: P10, day: 0, price: P10 }.bytes();
    let under = |list: &[u8]| {
        let real = Called::hash_of(&shard.s.sid, T0, list);
        let mut t = with_arg(&b, 8, bytes_arg(list));
        object_made(&mut t, &w.lib, 2, &real);
        let o = t.tx.outputs.iter().position(|o| o.script_public_key == hash_spk(&w.lib, &call.s.hash())).expect("the call again");
        t.tx.outputs[o].script_public_key = hash_spk(&w.lib, &real);
        accepted(&t, &w.keys)
    };
    let four = [body.clone(), made(9), made(3), made(3)].concat();
    let five = [four.clone(), made(1)].concat();
    assert!(under(&body), "(control) the object put in as it is");
    assert!(under(&four), "(control) a list of four, the three after the leg's out of their order and one twice");
    assert!(!under(&five), "a step under a list of five entries");
    assert!(!under(&five[..279]) && !under(&four[..223]) && !under(&[]), "a step under a list that is not of whole entries, or of none");
    // A price of nothing.
    let (called0, call0) = called_objs(&shard, [0, 0, 0, 0], 6_600);
    assert_eq!(call0.s.list()[0].price, 0);
    let a0 = cov(&w, &called0, &entered(&[l0], 0), 40 * KAS1);
    let (b0, step0) = call_step(&w.lib, &a0, &a0.s, None, &call0, &step_s, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("B4: a step under a call that says a price of nothing", &b0, &w.keys);
    println!("T5 B4: a long of 10 units from 10 with 40 KAS stepped at a price of nothing: it lost {} sompi, pays {}, its note asks {}, the user has {}; {} bytes", -step0.gain, step0.pay, step0.note.asked, step0.user, sg.mass.size);
    assert_eq!((step0.gain, step0.note.asked < 0, step0.user), (-100 * KAS1, true, 0));
    // A shard of no market.
    let none = bare(&w);
    assert!(!none.s.call_due(&[0; 4]) && none.s.call([0; 4], T0, none.value as i64, xk(&w.w)).is_err(), "the model calls a shard of no market");
    let t = none.s.traced(&Draw { amount: 0, prices: [0; 4] }, true).unwrap();
    let raw = t.said();
    assert_eq!((raw.len(), t.list.len()), (184, 0));
    let st = stated_as(&none, KIND_CALL, &raw, call_outs_of(&none, &[], T0, xk(&w.w)).unwrap(), xk(&w.w), T0);
    let live = states(&w, &none, &st, &raw).expect("a call of 184 bytes is stated");
    assert_eq!(taken_raw(&w, &w.pointed(&live, 0), &none.s, &raw, 0, None), (false, true), "the end of a call of a shard of no market");
    for at in 1..=6i64 {
        assert_eq!(taken_raw(&w, &w.pointed(&live, at), &none.s, &raw, at, None), (true, false), "the point {at} of a trace of no market");
    }
}

/// B4, the call's whole way with the new object, the list in another order than the rows. Two markets of the
/// world's twelve whose names stand the other way in the list than their rows in the books. A long of 10 units
/// and a short of 4 in the first row, a long of 3 in the second, all from 10; the call at 30 and 10. Stated,
/// applied (the object holds the list: 112 bytes); the account of the two legs of the first row is stepped naming
/// the place 1 twice, the account of the second row's leg naming the place 0; the notes applied (`KpfTLeftAll`,
/// `KpfTLeft`); the claim paid; the call ended. A step naming the row's place instead of the list's is refused.
#[test]
fn t5_b4_the_whole_way_of_a_call_whose_list_is_not_in_the_order_of_its_rows() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let other_way = |i: usize, j: usize| !Priced { market: w.head(i).market, limit: 0, day: 0, price: 0 }.below(&Priced { market: w.head(j).market, limit: 0, day: 0, price: 0 });
    let (i, j) = (0..12).flat_map(|i| (0..12).map(move |j| (i, j))).find(|(i, j)| i != j && other_way(*i, *j)).expect("two markets");
    let legs = [fleg(&w, i, 10, LONG, P10), fleg(&w, i, 4, kpf_protocol::state::SHORT, P10), fleg(&w, j, 3, LONG, P10)];
    let mut books = base.s;
    for l in &legs {
        books = books.booked(l).unwrap();
    }
    let shard = OLive { s: books, ..base.clone() };
    let scale = w.markets[0].p.scale;
    assert_eq!((shard.s.rows[0].head, shard.s.rows[1].head, shard.s.rows[0].gross, shard.s.rows[1].gross), (w.head(i), w.head(j), 14 * scale, 3 * scale));
    let up = [P10 * 3, P10, 0, 0];
    let fw = [fund(xk(&w.w), 30)];
    let (b, st, d) = shard_call(&w.lib, &shard, &w.script("KpfTChange"), up, T0, xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    let t = shard.s.traced(&d, true).unwrap();
    assert_eq!((t.rows[0].at, t.rows[1].at, t.list[0].price, t.list[1].price), (1, 0, P10, P10 * 3), "the list is in the order of the names, the other way than the rows");
    let live = nth(&signed("B4: the call stated", &b, &w.keys), 0, st, 6_510);
    let ap = signed("B4: the call applied", &stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.f, &ff)).unwrap(), &w.keys);
    let fee = shard.s.t.keeper_fee_min;
    let (next, call) = shard.s.called_at(&up, T0, fee);
    assert_eq!((call.body(), call.hash()), (t.list_bytes(), Called::hash_of(&shard.s.sid, T0, &t.list_bytes())));
    assert_eq!(&ap.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &next.hash()));
    let mut called = nth(&ap, 0, next, 6_800);
    let call = nth(&ap, out_of(&ap, &hash_spk(&w.lib, &call.hash())).0, call, 6_800);
    let step_s = w.script("KpfTCStep");
    let a_two = cov(&w, &called, &entered(&legs[..2], 0), 50 * KAS1);
    let a_one = OLive { op: op(84), ..cov(&w, &called, &entered(&legs[2..], 0), 20 * KAS1) };
    let mut claims = vec![];
    for (what, a, script, places, wrong) in [("two legs of the first row", &a_two, "KpfTLeftAll", vec![1usize, 1], vec![0usize, 0]), ("the leg of the second row", &a_one, "KpfTLeft", vec![0], vec![1])] {
        let (b, step) = call_step(&w.lib, a, &a.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
        assert_eq!(step.at, places, "{what}: the places are the list's");
        let sg = signed(&format!("B4: the account of {what} stepped"), &b, &w.keys);
        let by_row = call_step_as(&w.lib, a, &a.s, None, &call, &CallStep { at: wrong, ..step.clone() }, &step_s, w.payer(&w.f, &ff)).unwrap();
        assert!(!accepted(&by_row, &w.keys), "{what}: stepped naming the rows' places");
        let (b, next, claim) = left_apply(&w.lib, &called, &nth(&sg, 2, step.note.clone(), 6_810), &w.script(script), w.payer(&w.f, &ff)).unwrap();
        let sg = signed(&format!("B4: the note of {what} applied"), &b, &w.keys);
        if let Some(c) = claim {
            claims.push(nth(&sg, 2, c, 6_820));
        }
        called = nth(&sg, 0, next, 6_820);
    }
    assert_eq!((called.s.rows, claims.len(), called.s.owed), ([ShardRow::EMPTY; 4], 1, 120 * KAS1), "the long of 10 and the short of 4 gained 120 KAS at 30");
    // (Review T6, docs 17.8c: the claim is paid its part of what the shard ever held, 99.9 KAS of 120: what is
    // owed stands, the claim goes on and the call does NOT end (it ended here before, 20.1 KAS never paid). The
    // claim and the shard landed after the time of the claims (6,750): the claim's own input shows it. Paid up by
    // its owner, the claim is paid whole and the call ends.)
    assert!(called.s.claims_time().unwrap() <= 6_820);
    let c0 = OLive { s: claims[0].s.claim(), value: claims[0].value, id: claims[0].id, op: claims[0].op, daa: claims[0].daa };
    let (b, next, paid, left) = claim_pay_c(&w.lib, &called, &c0, &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("B4: the claim paid its part", &b, &w.keys);
    assert_eq!((paid, next.held, next.owed, next.claims.paid1), (100 * KAS1 - fee, 0, 120 * KAS1, 100 * KAS1 - fee));
    let (called, c0) = (nth(&sg, 0, next, 6_830), nth(&sg, 2, left.expect("the claim again"), 6_830));
    assert!(call_off(&w.lib, &called, &call, &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).is_err(), "the call ends while the claim is not whole");
    let fc = [fund(xk(&w.c), 38)];
    let (b, next) = shard_fund(&w.lib, &called, &w.script("KpfTAddC"), 20 * KAS1 + fee, w.payer(&w.c, &fc)).unwrap();
    let called = nth(&signed("B4: the owner pays its called shard up", &b, &w.keys), 0, next, 6_840);
    let (b, next, paid, left) = claim_pay_c(&w.lib, &called, &c0, &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("B4: the claim paid whole", &b, &w.keys);
    assert_eq!((paid, left, next.held, next.owed, next.claims.paid1), (20 * KAS1 + fee, None, 0, 120 * KAS1, 120 * KAS1));
    let (b, open) = call_off(&w.lib, &nth(&sg, 0, next, 6_850), &call, &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).unwrap();
    signed("B4: the call ends", &b, &w.keys);
    assert_eq!((open.called, open.owed, open.claims, open.rows), (0, 0, kpf_protocol::reserve::Claims::default(), [ShardRow::EMPTY; 4]));
}

// ================================================================================================================
// B5. KpfTChange.

/// B5. What `KpfTChange` holds of a draw's and a call's bytes: a length of 184 to 408 by steps of 56, and nothing
/// else (the counts written are the end's to hold). A draw and a call of the rule's, their bytes cut or filled
/// with zero bytes to each length: stated at 184, 240, 296, 352 and 408; refused at 0, 40 (the length of before),
/// 183, 185, 239, 241, 407, 409, 464 and 520. And two statements of an account as controls (the script is in
/// each): a take and a leaving of a covered account are stated as before, and with a byte more said, refused.
#[test]
fn t5_b5_the_length_of_what_a_draw_and_a_call_say_and_two_statements_of_an_account() {
    let w = world();
    let mut seen = vec![];
    for call in [false, true] {
        let shard = if call { OLive { s: Shard { held: 5 * KAS1, basis: -500 * KAS1 + 200 * KAS1, ..w.shard().s }, value: (5 * KAS1 + SHARD_CARRIER) as u64, ..w.shard() } } else { w.shard() };
        let s = &shard.s;
        let prices = [P10, P10, 0, 0];
        let (st, d) = if call { s.call(prices, T0, shard.value as i64, xk(&w.w)).unwrap() } else { s.draw(KAS1, prices, T0, shard.value as i64).unwrap() };
        let honest = s.traced(&d, call).unwrap().said();
        assert_eq!(honest.len(), 296);
        for len in [0usize, 40, 183, 184, 185, 239, 240, 241, 296, 352, 407, 408, 409, 464, 520] {
            let mut said = honest.clone();
            said.resize(len, 0);
            let stated = states(&w, &shard, &Stated { arg: blake2b(&said), ..st }, &said).is_some();
            assert_eq!(stated, (184..=408).contains(&len) && (len - 184) % 56 == 0, "a {} of {len} bytes", if call { "call" } else { "draw" });
            seen.push((call, len, stated));
        }
    }
    // The controls: a covered account's take (kind 7) and its leg's leaving (kind 8).
    let base = seeded(&w);
    let leg = fleg(&w, 0, 10, LONG, P10);
    let books = OLive { s: base.s.booked(&leg).unwrap(), ..base.clone() };
    let acct = cov(&w, &books, &entered(&[leg], 0), 50 * KAS1);
    let f = [fund(xk(&w.w), 33)];
    let mut controls = 0;
    for (kind, amount, skip) in [(kpf_protocol::covered::KIND_C_TAKE, KAS1, 0), (KIND_C_LEAVE, 0, 1)] {
        let (b, st, said, _) = c_state(&w.lib, &acct, &w.script("KpfTChange"), kind, amount, skip, &[P10], T0, xk(&w.w), w.payer(&w.w, &f)).unwrap();
        assert!(accepted(&b, &w.keys), "(control) a statement of kind {kind} on a covered account");
        let more = [said.said(), vec![0u8]].concat();
        let longer = account_state(&w.lib, &acct, &w.script("KpfTChange"), &Stated { arg: blake2b(&more), ..st }, &more, w.payer(&w.w, &f));
        assert!(longer.is_err() || !accepted(&longer.unwrap(), &w.keys), "a statement of kind {kind} that says a byte more");
        let draws = account_state(&w.lib, &acct, &w.script("KpfTChange"), &Stated { kind: KIND_DRAW, ..st }, &said.said(), w.payer(&w.w, &f));
        assert!(draws.is_err() || !accepted(&draws.unwrap(), &w.keys), "a draw stated on an account");
        controls += 1;
    }
    println!("T5 B5: KpfTChange is {} bytes (1,728 before); the lengths (a call, the length, stated): {seen:?}; {controls} statements of a covered account stated as before", kpf_protocol::thin_artifacts::kit().script("KpfTChange").unwrap().len());
}

// ================================================================================================================
// B6. The name of a row in the list; sizes.

/// B6. The list holds the name a row has (the hash of its kind and its market), not the plain market. No script
/// of this library writes a row of options (`KpfTEnterS` writes the futures' name, and holds an entry's leg to
/// no day, strike or premium): a shard with a futures row and an options row of ONE market is made by hand here.
/// A draw on it states two prices for the one underlying, 0.4 % below and 0.4 % above the committee's: two
/// entries of two names; each row is answered right, and each price is answered right with the SAME citation.
/// It was so before the change (two rows, a price a row, each within the tolerance of the one citation): the
/// list by names changes nothing there, and with the plain market (17.8a, 4) the two rows have one entry.
#[test]
fn t5_b6_a_futures_row_and_an_options_row_of_one_market_are_two_entries_and_two_prices() {
    let w = world();
    let x = w.shard();
    let scale = w.markets[0].p.scale;
    let fut = w.head(0);
    let opt = Head { market: kpf_protocol::reserve::option_market(&w.markets[0].id()), ..fut };
    let rows = [ShardRow { head: fut, net: -10 * scale, gross: 10 * scale }, ShardRow { head: opt, net: 5 * scale, gross: 5 * scale }, ShardRow::EMPTY, ShardRow::EMPTY];
    let shard = OLive { s: Shard { rows, basis: -50 * KAS1, ..x.s }, ..x };
    let s = &shard.s;
    let prices = [P10 * 996 / 1_000, P10 * 1_004 / 1_000, 0, 0];
    let (st, d) = s.draw(KAS1, prices, T0, shard.value as i64).unwrap();
    let t = s.traced(&d, false).unwrap();
    assert_eq!(t.list.len(), 2);
    assert_ne!(t.list[0].price, t.list[1].price);
    let raw = t.said();
    let live = states(&w, &shard, &st, &raw).expect("stated");
    for at in 0..=6i64 {
        let cite = (at >= 5).then(|| w.cited(0, P10));
        assert_eq!(taken_raw(&w, &w.pointed(&live, at), s, &raw, at, cite.as_ref()), (true, false), "the point {at}");
    }
    // (One entry for both rows: the second row's name is not the entry's: wrong at that row.)
    let mut one = t.clone();
    one.rows[1].at = one.rows[0].at;
    let raw1 = one.said();
    let st1 = stated_as(&shard, KIND_DRAW, &raw1, draw_outs(&shard, KAS1, T0, xk(&w.c)).unwrap(), xk(&w.c), T0);
    let live1 = states(&w, &shard, &st1, &raw1).expect("stated");
    assert_eq!(taken_raw(&w, &w.pointed(&live1, 2), s, &raw1, 2, None), (false, true), "two rows naming one entry");
}

/// The sizes of the transactions, on the shard of two markets of `thin_draw.rs` (296 bytes said) and on one of four
/// (408 bytes said).
#[test]
fn t5_the_sizes_of_the_scripts_and_of_the_transactions_of_a_trace() {
    let w = world();
    let kit = kpf_protocol::thin_artifacts::kit();
    let sizes: Vec<(&str, usize)> = ["KpfTShardEnd", "KpfTCallEnd", "KpfTShardPrice", "KpfTCStep", "KpfTChange", "KpfTShardRow"].iter().map(|n| (*n, kit.script(n).unwrap().len())).collect();
    // (Before, from the artifacts of 5281d25: KpfTShardEnd 3,234; KpfTCallEnd 3,695; KpfTShardPrice 1,810; KpfTCStep
    // 4,829; KpfTChange 1,728; KpfTShardRow is new. Second round: the day held as eight bytes (R1) is three bytes
    // in each of KpfTShardPrice (2,700 from 2,697) and KpfTShardRow (2,098 from 2,095); the others as they were.)
    assert_eq!(sizes, vec![("KpfTShardEnd", 1747), ("KpfTCallEnd", 1853), ("KpfTShardPrice", 2707), ("KpfTCStep", 5056), ("KpfTChange", 1760), ("KpfTShardRow", 2105)]);
    let scale = w.markets[0].p.scale;
    let two = w.shard();
    let four = OLive { s: Shard { rows: [two.s.rows[0], two.s.rows[1], ShardRow { head: w.head(2), net: 5 * scale, gross: 5 * scale }, ShardRow { head: w.head(3), net: -5 * scale, gross: 5 * scale }], ..two.s }, ..two.clone() };
    let mut out = vec![];
    for (what, shard) in [("two markets", &two), ("four markets", &four)] {
        let prices = if what == "two markets" { [P10, P10, 0, 0] } else { [P10; 4] };
        let (fc, fw, ff) = ([fund(xk(&w.c), 30)], [fund(xk(&w.w), 30)], [fund(xk(&w.f), 36)]);
        let s = &shard.s;
        // A draw: stated; its end, a row and a price answered right.
        let (b, st, d) = shard_draw(&w.lib, shard, &w.script("KpfTChange"), KAS1, prices, T0, w.payer(&w.c, &fc)).unwrap();
        let sg = signed(&format!("sizes, {what}: a draw stated"), &b, &w.keys);
        let live = nth(&sg, 0, st, 6_510);
        let (b, _) = shard_end(&w.lib, &w.pointed(&live, 0), s, &d, &w.script("KpfTShardEnd"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
        let end = signed(&format!("sizes, {what}: a draw's end answered"), &b, &w.keys);
        let (b, _) = shard_row(&w.lib, &w.pointed(&live, 1), s, &d, 1, &w.script("KpfTShardRow"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
        let row = signed(&format!("sizes, {what}: a row answered"), &b, &w.keys);
        let p1 = 5 + s.traced(&d, false).unwrap().rows[0].at;
        let c = w.cited(0, P10);
        let (b, _) = shard_price(&w.lib, &w.pointed(&live, p1), s, &d, p1, &w.script("KpfTShardPrice"), Some((&w.script("KpfTCite"), &c)), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
        let price = signed(&format!("sizes, {what}: a price answered"), &b, &w.keys);
        // A call, on the same books holding 5 KAS and having lost.
        let low = OLive { s: Shard { held: 5 * KAS1, basis: s.basis + 300 * KAS1, ..*s }, value: (5 * KAS1 + SHARD_CARRIER) as u64, ..shard.clone() };
        let (b, cst, cd) = shard_call(&w.lib, &low, &w.script("KpfTChange"), prices, T0, xk(&w.w), w.payer(&w.w, &fw)).unwrap();
        let csg = signed(&format!("sizes, {what}: a call stated"), &b, &w.keys);
        let clive = nth(&csg, 0, cst, 6_510);
        let (b, _) = call_end(&w.lib, &w.pointed(&clive, 0), &low.s, &cd, &w.script("KpfTCallEnd"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
        let cend = signed(&format!("sizes, {what}: a call's end answered"), &b, &w.keys);
        let ap = signed(&format!("sizes, {what}: a call applied"), &stated_settle(&w.lib, &clive, &w.script("KpfTChangeSettle"), w.payer(&w.f, &ff)).unwrap(), &w.keys);
        // A step of one leg and of six under the call's object.
        let (called, call) = called_objs(&low, prices, 6_600);
        let legs6: Vec<XLeg> = (0..6).map(|k| fleg(&w, k % 2, 1, if k % 2 == 0 { LONG } else { kpf_protocol::state::SHORT }, P10)).collect();
        let mut steps = vec![];
        for legs in [&legs6[..1], &legs6[..]] {
            let a = cov(&w, &called, &entered(legs, 0), 40 * KAS1);
            let (b, _) = call_step(&w.lib, &a, &a.s, None, &call, &w.script("KpfTCStep"), w.payer(&w.f, &ff)).unwrap();
            steps.push(signed(&format!("sizes, {what}: a step of {} legs", legs.len()), &b, &w.keys).mass.size);
        }
        out.push((what, s.traced(&d, false).unwrap().said().len(), sg.mass.size, csg.mass.size, end.mass.size, cend.mass.size, row.mass.size, price.mass.size, ap.mass.size, steps[0], steps[1]));
    }
    println!("T5 sizes: the scripts {sizes:?}; (the shard, bytes said, a draw stated, a call stated, a draw's end answered, a call's end answered, a row answered, a price answered, a call applied, a step of one leg, of six): {out:?}");
}

/// B3, the order of events. A draw whose END is right and whose row 1 is wrong (the worth of the gross said a
/// sompi less from row 1 on: the requirement of the end is the same or a sompi less). `KpfTPoint` remembers no
/// point: the owner's other key disputes the end, it is answered right, the draw stands again; the end is
/// disputed AGAIN, and answered right again; each round costs the disputer its bond. What that buys:
///  - while the end is disputed, the wrong row is answered wrong all the same (`at == point || verdict == 0`), and
///    the draw ends: a dispute of a right point shields nothing;
///  - standing again, row 1 is disputed and answered wrong;
///  - the draw standing again is not applied before it stood a dispute period (the builder refuses it; at the
///    engine, the settlement written with no age on its input is refused), unless it was written at or after its last time
///    to dispute (its time, its lateness and a period: 6,650 here), where it is applied at once and no dispute of
///    it is taken. Until 6,650 the draw is, in every block, standing (anyone disputes) or disputed (anyone answers
///    its wrong row): the rounds leave whoever watches the same blocks, five points or nine.
#[test]
fn t5_b3_the_end_answered_right_again_and_again_shields_no_wrong_row() {
    let w = world();
    let shard = w.shard();
    let s = &shard.s;
    let owner = xk(&w.c);
    let prices = [P10, P10, 0, 0];
    let t = s.traced(&Draw { amount: KAS1, prices }, false).unwrap();
    let mut lie = t.clone();
    lie.rows.iter_mut().for_each(|r| r.wg -= 1);
    let raw = lie.said();
    let st = stated_as(&shard, KIND_DRAW, &raw, draw_outs(&shard, KAS1, T0, owner).unwrap(), owner, T0);
    assert_eq!(s.trace_wrong_point(&lie, false, &owner, &st.outs, shard.value as i64, T0, &prices), Some(1));
    assert_eq!((s.draw_trace_end_right(&lie, &owner, &st.outs, shard.value as i64, T0), s.trace_row_judged(&lie, 1, false), s.trace_row_judged(&lie, 2, false)), (true, Some(false), Some(true)));
    let mut live = states(&w, &shard, &st, &raw).expect("stated");
    assert_eq!(st.last_dispute(), 6_650);
    let mut rounds = 0;
    for landed in [6_530u64, 6_560, 6_600] {
        // The end disputed: answered right (the statement stands again), and not wrong; and under that dispute
        // the wrong row is answered wrong.
        let q = w.pointed(&live, 0);
        assert_eq!(taken_raw(&w, &q, s, &raw, 0, None), (true, false), "the end");
        let wrong = answer_raw(&w, &q, s, &raw, 1, false, None).unwrap();
        assert!(accepted(&wrong, &w.keys), "row 1 answered wrong under a dispute of the end");
        assert_eq!(&wrong.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &s.hash()), "the shard again");
        let right = answer_raw(&w, &q, s, &raw, 0, true, None).unwrap();
        let sg = right.sign(&w.keys).expect("signed");
        assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok());
        live = nth(&sg, 0, live.s.again(), landed);
        assert_eq!(live.s.last_dispute(), 6_650, "the last time to dispute does not move");
        // Standing again: row 1 disputed, answered wrong and not right.
        assert_eq!(taken_raw(&w, &w.pointed(&live, 1), s, &raw, 1, None), (false, true), "row 1, the draw standing again");
        // Not applied before it stood a dispute period.
        let ff = [fund(xk(&w.f), 36)];
        let early = OLive { daa: landed, ..live.clone() };
        let b = stated_settle(&w.lib, &early, &w.script("KpfTChangeSettle"), w.payer(&w.f, &ff)).unwrap();
        assert!(accepted(&b, &w.keys), "(control) applied once it stood a dispute period");
        let mut at_once = b.clone();
        at_once.tx.inputs[0].sequence = 0;
        assert!(!accepted(&at_once, &w.keys), "the draw standing again applied with no age on its input, before its last time to dispute");
        rounds += 1;
    }
    // Written at its last time to dispute: applied at once, and no dispute of it is built.
    let last = OLive { daa: 6_650, ..live.clone() };
    let ff = [fund(xk(&w.f), 36)];
    let fw = [fund(xk(&w.w), 31)];
    let b = stated_settle(&w.lib, &last, &w.script("KpfTChangeSettle"), w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys) && b.tx.inputs[0].sequence == 0, "written at the last time to dispute, it is applied at once");
    assert!(point_dispute(&w.lib, &last, &w.script("KpfTPoint"), xk(&w.w), 1, w.payer(&w.w, &fw)).is_err());
    println!("T5 B3: {rounds} rounds of the end disputed and answered right: in each the wrong row is answered wrong under the dispute of the end, and after it; the last time to dispute stays 6,650");
}
