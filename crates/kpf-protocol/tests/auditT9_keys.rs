//! Review T9 (docs/audit/REVIEW-T9-keys.md): stage 1 of the tree (docs/ARCHITECTURE-CLEARING.md 17.8d): a shard's
//! row named by its key. The reviewer's file: its world and helpers are `auditT7_second.rs`'s, copied (the
//! world's citation taken of any market: `cited_of`); its own begin at "Review T9" below.
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
        let s = Shard { t: self.terms(), rows: rows.into(), owner: xk(&self.c), held: 100 * KAS1, expiry: 0, dispute_daa: 50, bond_bps: 1_000, sid: [0x5d; 32], basis: -500 * KAS1, called: 0, owed: 0, claims: Default::default() };
        OLive { s, value: (s.held + SHARD_CARRIER) as u64, id: Some(Hash::from_bytes([0x73; 32])), op: op(12), daa: 6_100 }
    }
    fn cited(&self, i: usize, price: i64) -> Cited {
        self.cited_of(self.markets[i].id(), price)
    }
    /// (Review T9.) The committee's citation of a price of any 32 bytes for a market.
    fn cited_of(&self, market: [u8; 32], price: i64) -> Cited {
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
    OLive { s: Shard { rows: [ShardRow::EMPTY; 4].into(), basis: 0, ..x.s }, ..x }
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



// ================================================================================================================
// Review T9 (docs 17.8d, stage 1: a shard's row named by its key): the reviewer's own helpers and tests.

use kpf_protocol::books::{Key, OF_CALLS};
use kpf_protocol::reserve::{Draw, Priced, Traced, KIND_CALL};
use kpf_protocol::thin::KIND_DRAW;

fn sm8_t9(v: i64) -> [u8; 8] {
    let mut b = v.unsigned_abs().to_le_bytes();
    if v < 0 {
        b[7] |= 0x80;
    }
    b
}
fn i9_t9(v: i64) -> Vec<u8> {
    [&[0x08][..], &sm8_t9(v)].concat()
}
fn p33_t9(x: &[u8; 32]) -> Vec<u8> {
    [&[0x20][..], x].concat()
}
const E: ShardRow = ShardRow::EMPTY;

/// The shard `s` as an output stands on the chain, under the registry's id of `base`.
fn live(base: &OLive<Shard>, s: Shard) -> OLive<Shard> {
    OLive { s, value: (s.held + SHARD_CARRIER) as u64, ..base.clone() }
}

/// A futures leg of one unit of the market `market` (any 32 bytes), with the scale and limit of the world's.
fn mleg(w: &World, market: [u8; 32], units: i64, dir: i64) -> XLeg {
    XLeg { market, ..fleg(w, 0, units, dir, P10) }
}

/// The shard after an entry of `leg`, written by hand: row `r` is `row`; what the legs were entered at and the
/// shard's time move as the entry of a leg of that size, direction and price moves them (taken of the model's
/// entry of the same leg of an ordinary market into a shard of no row that holds enough).
fn after(s: &Shard, leg: &XLeg, r: usize, row: ShardRow) -> Shard {
    let twin = Shard { rows: [E; 4].into(), held: i64::MAX / 4, ..*s }.entered(&XLeg { market: [0x11; 32], ..*leg }, 0, T0).expect("the twin's entry");
    let mut n = Shard { basis: twin.basis, expiry: twin.expiry, ..*s };
    n.rows[r] = row;
    n
}
/// The row `row` with the leg's units in it (side B's: the size below zero where A is long), under `head`.
fn with_leg(head: RowHead, row: &ShardRow, leg: &XLeg) -> ShardRow {
    ShardRow::of(head, row.net + if leg.dir == LONG { -leg.size } else { leg.size }, row.gross + leg.size)
}

/// The row with the leg's units out of it, its head emptied where none is left (what a leaving writes).
fn without_leg(row: &ShardRow, leg: &XLeg) -> ShardRow {
    let (net, gross) = (row.net - if leg.dir == LONG { -leg.size } else { leg.size }, row.gross - leg.size);
    if net == 0 && gross == 0 { ShardRow { entered: row.entered, ..E } } else { ShardRow { net, gross, ..*row } }
}

/// Whether `KpfTEnterS` takes the entry of `leg` into row `r` of the shard, the shard after written as `next`
/// (shown the seed and the committee of these tests). `None`: the account's side has no such entry.
fn enters(w: &World, shard: &OLive<Shard>, leg: XLeg, r: usize, next: &Shard) -> Option<bool> {
    let acct = cov(w, shard, &[], 2_000 * KAS1);
    let f = [fund(xk(&w.w), 33)];
    let (st, said, _, _) = acct.s.c_enter(leg, 0, r as i64, &[], T0, acct.value as i64).ok()?;
    let b = c_enter_state_as(&w.lib, &acct, shard, &w.script("KpfTEnterS"), SEED, w.markets[0].committee_id, &st, &said, next, w.payer(&w.w, &f)).ok()?;
    Some(accepted(&b, &w.keys))
}

fn note_t9(w: &World, shard: &OLive<Shard>, legs: Vec<XLeg>) -> OLive<Left> {
    OLive { s: Left { sid: shard.s.sid, legs, key: xk(&w.w), asked: 0 }, value: NOTE_MIN as u64, id: shard.id, op: op(75), daa: 6_050 }
}
fn left_name(n: usize) -> &'static str {
    if n == 1 { "KpfTLeft" } else { "KpfTLeftAll" }
}
/// Whether the engine takes the note applied with its legs named in the rows `at`, the shard after written `next`.
fn leaves(w: &World, shard: &OLive<Shard>, note: &OLive<Left>, at: &[usize], next: Shard) -> bool {
    let ff = [fund(xk(&w.f), 36)];
    left_apply_as(&w.lib, shard, note, at, next, 0, None, &w.script(left_name(note.s.legs.len())), w.payer(&w.f, &ff)).is_ok_and(|b| accepted(&b.0, &w.keys))
}
/// Whether the engine takes the note spent as a note of a leg `k` that is no leg of the books.
fn voids(w: &World, shard: &OLive<Shard>, note: &OLive<Left>, k: usize) -> bool {
    let ff = [fund(xk(&w.f), 36)];
    left_void(&w.lib, shard, note, k, &w.script("KpfTLeftVoid"), w.payer(&w.f, &ff)).is_ok_and(|b| accepted(&b, &w.keys))
}

/// A statement of the kind `kind` that says the bytes `said` on the shard, written whatever the rule says.
fn stated_as(shard: &OLive<Shard>, kind: i64, said: &[u8], outs: Outs, claimer: [u8; 32], t0: i64) -> Stated {
    let s = &shard.s;
    Stated { position: s.hash(), window: s.dispute_daa, late: s.t.late_daa, kind, t0, arg: blake2b(said), outs, bond: if kind == KIND_CALL { s.call_bond() } else { s.bond() }, claimer, held: shard.value as i64 }
}
fn draw_outs(shard: &OLive<Shard>, amount: i64, t0: i64, claimer: [u8; 32]) -> Option<Outs> {
    Outs::of(&[Out { key: false, to: shard.s.drawn(amount, t0).hash(), value: shard.value as i64 - amount }, Out { key: true, to: claimer, value: amount }])
}
/// The statement stated on the shard with the bytes `said`: taken by `KpfTChange` or not.
fn states(w: &World, shard: &OLive<Shard>, st: &Stated, said: &[u8]) -> Option<OLive<Stated>> {
    let who = if st.claimer == xk(&w.c) { &w.c } else { &w.w };
    let f = [fund(xk(who), 30)];
    let s = &shard.s;
    let b = state_raw(&w.lib, shard, &s.fixed_hash(), &s.moving(), st, said, s.form(), &w.script("KpfTChange"), w.payer(who, &f)).ok()?;
    let sg = b.sign(&w.keys).ok()?;
    kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok().then(|| nth(&sg, 0, *st, (st.t0 + 10) as u64))
}
/// A draw of a KAS stated by the owner on the shard with the trace `t` as it is given.
fn draw_stated(w: &World, shard: &OLive<Shard>, t: &Traced) -> (OLive<Stated>, Stated) {
    let raw = t.said();
    let st = stated_as(shard, KIND_DRAW, &raw, draw_outs(shard, KAS1, T0, xk(&w.c)).unwrap(), xk(&w.c), T0);
    (states(w, shard, &st, &raw).expect("stated"), st)
}
/// What the engine takes of the answer of the price point `at` (5 and above) of the trace `t`: (right, wrong).
fn price_taken(w: &World, live: &OLive<Stated>, s: &Shard, t: &Traced, at: i64, cite: Option<&Cited>) -> (bool, bool) {
    let q = w.pointed(live, at);
    let cs = w.script("KpfTCite");
    let take = |v: bool| {
        let f = [fund(xk(&w.f), 36)];
        shard_price_t(&w.lib, &q, s, SEED, t, at, &w.script("KpfTShardPrice"), cite.map(|c| (&cs, c)), xk(&w.f), w.payer(&w.f, &f), Some(v)).is_ok_and(|b| accepted(&b.0, &w.keys))
    };
    (take(true), take(false))
}
/// The same of a row's point (1 to 4).
fn row_taken(w: &World, live: &OLive<Stated>, s: &Shard, t: &Traced, at: i64) -> (bool, bool) {
    let q = w.pointed(live, at);
    let take = |v: bool| {
        let f = [fund(xk(&w.f), 36)];
        shard_row_t(&w.lib, &q, s, t, at, &w.script("KpfTShardRow"), xk(&w.f), w.payer(&w.f, &f), Some(v)).is_ok_and(|b| accepted(&b.0, &w.keys))
    };
    (take(true), take(false))
}

/// The trace `t` with one entry more in its list, in the list's order, the rows naming their entries again.
fn with_entry(t: &Traced, en: Priced) -> Traced {
    let mut x = t.clone();
    let old = x.list.clone();
    x.list.push(en);
    x.list.sort_by_key(|p| p.numbers());
    x.e = x.list.len() as i64;
    for row in x.rows.iter_mut() {
        if let Some(o) = old.get(row.at as usize) {
            // (A row of no market says the place 0 and is read by nothing.)
            row.at = x.list.iter().position(|p| p.market == o.market).unwrap() as i64;
        }
    }
    x
}

// ----------------------------------------------------------------------------------------------------------------
// K1. Keys at an entry.

/// K1. `KpfTEnterS` names the leg's row by the leg's market and three numbers of nothing. On a shard whose id
/// its opening gives it:
///  - (control) a leg of M enters an empty row, which is then the 105 bytes of the futures key of M, the leg's
///    scale and limit, the units, and nine bytes of nothing;
///  - a row of M that has a day, a kind or a strike (made by hand) is not the leg's row: the leg enters it
///    neither keeping its key nor writing the futures key over it; it enters the EMPTY row beside it (the model
///    says the same): a market's futures row put in after a dated row of it, which the tree's rule will forbid;
///  - two rows of one key do not come to be: the leg into an empty row while another row has its key;
///  - a row of the key with another scale or limit: the leg enters neither it nor an empty row;
///  - the market of nothing enters no row (the rows of no market have its key).
#[test]
fn t9_k1_a_leg_enters_the_futures_row_of_its_market_and_no_other() {
    let w = world();
    let base = seeded(&w);
    let leg = fleg(&w, 0, 1, LONG, P10);
    let (m, sz) = (leg.market, leg.size);
    let head = RowHead::of(&leg);
    let n0 = base.s.entered(&leg, 0, T0).unwrap();
    assert_eq!(enters(&w, &base, leg, 0, &n0), Some(true), "(control) the leg enters an empty row");
    assert_eq!(n0.rows[0].bytes(), [p33_t9(&m), i9_t9(0), i9_t9(0), i9_t9(0), i9_t9(leg.scale), i9_t9(leg.price_max), i9_t9(-sz), i9_t9(sz), i9_t9(0)].concat());
    assert_eq!((n0.rows[0].bytes().len(), n0.rows_bytes().len()), (105, 420));
    assert_eq!(after(&base.s, &leg, 0, with_leg(head, &E, &leg)), n0, "(control) the shard after, written by hand, is the model's");
    // (Control of the hand: any empty row takes it, and the model says so.)
    for r in 1..4 {
        let n = after(&base.s, &leg, r, with_leg(head, &E, &leg));
        assert_eq!((enters(&w, &base, leg, r, &n), base.s.entered(&leg, r, T0).ok()), (Some(true), Some(n)), "(control) the empty row {r}");
    }
    // A row of M with a day, a kind, a strike.
    for (what, key) in [("a day", Key { day: 1, ..Key::futures(m) }), ("a kind", Key { of: OF_CALLS, ..Key::futures(m) }), ("a strike", Key { strike: 1, ..Key::futures(m) }), ("a day below nothing", Key { day: -1, ..Key::futures(m) })] {
        let odd_row = ShardRow { key, ..ShardRow::of(head, -5 * sz, 5 * sz) };
        let odd = live(&base, Shard { rows: [odd_row, E, E, E].into(), ..base.s });
        let keeps = after(&odd.s, &leg, 0, ShardRow { key, ..with_leg(head, &odd_row, &leg) });
        let over = after(&odd.s, &leg, 0, with_leg(head, &odd_row, &leg));
        let fresh = after(&odd.s, &leg, 0, with_leg(head, &E, &leg));
        assert_eq!((enters(&w, &odd, leg, 0, &keeps), enters(&w, &odd, leg, 0, &over), enters(&w, &odd, leg, 0, &fresh)), (Some(false), Some(false), Some(false)), "a leg enters a row of its market with {what}");
        assert!(odd.s.entered(&leg, 0, T0).is_err());
        let beside = after(&odd.s, &leg, 1, with_leg(head, &E, &leg));
        assert_eq!((enters(&w, &odd, leg, 1, &beside), odd.s.entered(&leg, 1, T0).ok()), (Some(true), Some(beside)), "the futures row beside a row with {what}: the engine and the model");
    }
    // Two rows of one key.
    let has = live(&base, Shard { expiry: 0, ..n0 });
    for r in 1..4 {
        let dup = after(&has.s, &leg, r, with_leg(head, &E, &leg));
        assert_eq!(enters(&w, &has, leg, r, &dup), Some(false), "two rows of one key");
        assert!(has.s.entered(&leg, r, T0).is_err());
    }
    let more = after(&has.s, &leg, 0, with_leg(head, &has.s.rows[0], &leg));
    assert_eq!((enters(&w, &has, leg, 0, &more), has.s.entered(&leg, 0, T0).ok()), (Some(true), Some(more)), "(control) a second leg into the row of its key");
    // The key with another scale, another limit.
    for (what, other) in [("another limit", XLeg { price_max: leg.price_max - 1, ..leg }), ("another scale", XLeg { scale: leg.scale * 10, size: leg.size * 10, ..leg })] {
        let oh = RowHead::of(&other);
        let into = after(&has.s, &other, 0, with_leg(oh, &has.s.rows[0], &other));
        let into_theirs = after(&has.s, &other, 0, with_leg(head, &has.s.rows[0], &other));
        let beside = after(&has.s, &other, 1, with_leg(oh, &E, &other));
        let got = (enters(&w, &has, other, 0, &into), enters(&w, &has, other, 0, &into_theirs), enters(&w, &has, other, 1, &beside));
        println!("T9 K1: a leg of {what} than its market's row: {got:?}");
        assert!(got.0 != Some(true) && got.1 != Some(true) && got.2 != Some(true), "a leg of {what} than its market's row enters");
        assert!(has.s.entered(&other, 0, T0).is_err() && has.s.entered(&other, 1, T0).is_err());
        // (Control: alone it enters, the row its own.)
        let alone = after(&base.s, &other, 0, with_leg(oh, &E, &other));
        assert_eq!(enters(&w, &base, other, 0, &alone), Some(true), "(control) a leg of {what} into a shard of no row");
    }
    // The market of nothing.
    let zero = mleg(&w, [0; 32], 1, LONG);
    for r in 0..4 {
        let z = after(&base.s, &zero, r, with_leg(RowHead::of(&zero), &E, &zero));
        let got = enters(&w, &base, zero, r, &z);
        assert!(got != Some(true) && base.s.entered(&zero, r, T0).is_err(), "the market of nothing enters row {r}: {got:?}");
        let got = enters(&w, &has, zero, r, &after(&has.s, &zero, r, with_leg(RowHead::of(&zero), &has.s.rows[r], &zero)));
        assert!(got != Some(true) && has.s.entered(&zero, r, T0).is_err(), "the market of nothing enters row {r} of a shard of a row: {got:?}");
    }
    println!("T9 K1: the market of nothing: the account's side: {:?}", enters(&w, &base, zero, 0, &base.s).map(|_| "an entry is stated on the account's side; the shard's refuses"));
}

/// K1, the nine bytes of what was entered. No script writes them other than nothing (`KpfTShardOpen` and
/// `KpfTEnterS` write nine bytes of nothing; `KpfTLeft` and `KpfTLeftAll` keep what is there). On a row made by
/// hand with 7 there: an entry writes nothing over it (and does not keep it); the last leg leaving leaves a row of
/// no market, no unit and 7 entered, which is NOT the empty row: no leg enters it (the model says the same), and
/// `KpfTCallPay` (`claim`, `off`) requires four empty rows. Unreachable in stage 1; a thing to hold when a row
/// with a day writes the number (stage 2 and the day): whoever empties a row's head empties these bytes.
#[test]
fn t9_k1_what_was_entered_is_written_by_nothing_and_a_row_made_with_it_is_stuck_when_emptied() {
    let w = world();
    let base = seeded(&w);
    let leg = fleg(&w, 0, 1, LONG, P10);
    let head = RowHead::of(&leg);
    let stale = ShardRow { entered: 7, ..with_leg(head, &E, &leg) };
    let odd = live(&base, Shard { rows: [stale, E, E, E].into(), ..base.s });
    let kept = after(&odd.s, &leg, 0, ShardRow { entered: 7, ..with_leg(head, &stale, &leg) });
    let zeroed = after(&odd.s, &leg, 0, with_leg(head, &stale, &leg));
    assert_eq!((enters(&w, &odd, leg, 0, &kept), enters(&w, &odd, leg, 0, &zeroed), odd.s.entered(&leg, 0, T0).ok()), (Some(false), Some(true), Some(zeroed)), "an entry writes nothing where 7 was entered");
    // The last leg leaves.
    let note = note_t9(&w, &odd, vec![leg_entered(&leg, 0)]);
    let (next, at, paid, rest) = odd.s.left(&note.s).unwrap();
    assert_eq!((at.clone(), paid, rest.is_none()), (vec![0], 0, true));
    assert_eq!(next.rows[0], ShardRow { entered: 7, ..E }, "the model: the head emptied, what was entered kept");
    assert!(leaves(&w, &odd, &note, &at, next), "the engine: the same");
    assert!(!leaves(&w, &odd, &note, &at, Shard { rows: [E; 4].into(), ..next }), "the engine does not write the empty row");
    let stuck = live(&base, next);
    let again = after(&stuck.s, &leg, 0, with_leg(head, &E, &leg));
    assert_eq!(enters(&w, &stuck, leg, 0, &again), Some(false), "a row of no market with 7 entered is entered");
    assert!(stuck.s.entered(&leg, 0, T0).is_err());
    let beside = after(&stuck.s, &leg, 1, with_leg(head, &E, &leg));
    assert_eq!(enters(&w, &stuck, leg, 1, &beside), Some(true), "(control) the empty row beside it");
}

// ----------------------------------------------------------------------------------------------------------------
// K1b. Bytes and numbers: nothing written below zero in a market's id.

/// K1b (finding Q1). The paper decided for the tree: "an id of which a number of 8 bytes is nothing written below
/// zero is no market's (two such ids are one key by their numbers)". Stage 1 puts the plain market in the list
/// of a draw's and a call's prices, ordered by the market's 32 bytes as four NUMBERS, and `KpfTEnterS` compares
/// keys as BYTES and holds no market to that decision. Two ids that differ in one chunk written `00..00` and
/// `00..80`: both enter, two rows; a draw (and a call: the same script answers its prices) has two entries of
/// which neither is below the other, so whichever stands second is answered wrong with nothing cited, and is
/// answered right by nobody. Every statement over such books has a wrong point.
#[test]
fn t9_k1b_the_second_of_two_markets_that_are_one_by_their_numbers_makes_no_row_and_books_of_both_have_no_right_trace() {
    let w = world();
    let base = seeded(&w);
    let mut ma = [0x21u8; 32];
    ma[8..16].copy_from_slice(&[0; 8]);
    let mut mb = ma;
    mb[15] = 0x80;
    let (la, lb) = (mleg(&w, ma, 1, LONG), mleg(&w, mb, 1, LONG));
    let pa = Priced { market: ma, limit: la.price_max, day: 0, price: P10 };
    let pb = Priced { market: mb, ..pa };
    assert!(pa.numbers() == pb.numbers() && !pa.below(&pb) && !pb.below(&pa) && ma != mb);
    let s1 = base.s.entered(&la, 0, T0).unwrap();
    assert_eq!(enters(&w, &base, la, 0, &s1), Some(true));
    let one = live(&base, Shard { expiry: 0, ..s1 });
    let model = one.s.entered(&lb, 1, T0);
    let s2 = after(&one.s, &lb, 1, with_leg(RowHead::of(&lb), &E, &lb));
    let engine = enters(&w, &one, lb, 1, &s2);
    println!("T9 K1b: the second of two ids that are one by their numbers enters a row of its own: the engine {engine:?}, the model {:?}", model.as_ref().map(|_| "takes it"));
    // (Q1 closed, 17.8e: `KpfTEnterS` makes no row for an id of which a number is nothing written below zero, and
    // the model's `entered` refuses it. What follows is of books made by hand with a row of each: why.)
    assert_eq!((engine, model.is_err()), (Some(false), true), "Q1 is open again: the second of two ids that are one by their numbers enters a row of its own");
    // A draw over the two rows.
    let two = live(&base, Shard { expiry: 0, ..s2 });
    let s = &two.s;
    let prices = [P10, P10, 0, 0];
    let t = s.traced(&Draw { amount: KAS1, prices: prices.into() }, false).expect("the model has a trace");
    assert_eq!(t.list.len(), 2);
    let (lv, st) = draw_stated(&w, &two, &t);
    let second = t.list[1].market;
    let got = (row_taken(&w, &lv, s, &t, 1), row_taken(&w, &lv, s, &t, 2), price_taken(&w, &lv, s, &t, 5, Some(&w.cited_of(t.list[0].market, P10))), price_taken(&w, &lv, s, &t, 6, None), price_taken(&w, &lv, s, &t, 6, Some(&w.cited_of(second, P10))));
    let wrong = s.trace_wrong_point(&t, false, &xk(&w.c), &st.outs, two.value as i64, T0, &prices);
    println!("T9 K1b: the draw over them: rows {:?} {:?}; the first price {:?}; the second price, nothing cited {:?}, cited {:?}; the model's first wrong point: {wrong:?}", got.0, got.1, got.2, got.3, got.4);
    assert_eq!((got.0, got.1, got.2), ((true, false), (true, false), (true, false)));
    assert_eq!((got.3, got.4 .0), ((false, true), false), "Q1 is closed: the second of two entries that are one by their numbers is answered right");
    // The other order of the list is no better.
    let mut other = t.clone();
    other.list.swap(0, 1);
    for row in other.rows.iter_mut().take(2) {
        row.at = 1 - row.at;
    }
    let (lv2, _) = draw_stated(&w, &two, &other);
    assert_eq!(price_taken(&w, &lv2, s, &other, 6, None), (false, true), "the other order");
    assert_eq!(wrong, Some(6), "the model names the same point");
}

// ----------------------------------------------------------------------------------------------------------------
// K2. Keys at a leaving.

/// K2. `KpfTLeft`, `KpfTLeftAll` and `KpfTLeftVoid` name the leg's row by the leg's market and three numbers of
/// nothing. A row of the leg's market with a day, a kind or a strike (made by hand) is not the leg's row: the note
/// is not applied to it (named at that row or at any other; one leg, and two legs), and it is spent as a note of
/// a leg of no row. (Controls: on the futures row it is applied and not voided.) A leg of the market of nothing
/// leaves no row, an empty one neither (whose key is its key), and its note is voided.
#[test]
fn t9_k2_a_leg_leaves_the_futures_row_of_its_market_and_no_other() {
    let w = world();
    let base = seeded(&w);
    let (l0, l1) = (fleg(&w, 0, 1, LONG, P10), fleg(&w, 1, 2, SHORT, P10));
    let booked = base.s.booked(&l0).unwrap().booked(&l1).unwrap();
    let good = live(&base, booked);
    let (e0, e1) = (leg_entered(&l0, 0), leg_entered(&l1, 0));
    let n1 = note_t9(&w, &good, vec![e0]);
    let n2 = note_t9(&w, &good, vec![e0, e1]);
    let (next1, at1, ..) = good.s.left(&n1.s).unwrap();
    let (next2, at2, ..) = good.s.left(&n2.s).unwrap();
    assert_eq!((at1.clone(), at2.clone(), next1.rows[0], next2.rows), (vec![0], vec![0, 1], E, [E; 4].into()));
    assert!(leaves(&w, &good, &n1, &at1, next1) && leaves(&w, &good, &n2, &at2, next2), "(control) the notes are applied to the futures rows");
    assert!(!voids(&w, &good, &n1, 0) && !voids(&w, &good, &n2, 0) && !voids(&w, &good, &n2, 1), "(control) a note of legs of the books is spent");
    for (what, key) in [("a day", Key { day: 1, ..Key::futures(l0.market) }), ("a kind", Key { of: OF_CALLS, ..Key::futures(l0.market) }), ("a strike", Key { strike: 1, ..Key::futures(l0.market) })] {
        let mut odd = booked;
        odd.rows[0].key = key;
        let odd = live(&base, odd);
        assert!(odd.s.left(&n1.s).is_err() && odd.s.left(&n2.s).is_err() && odd.s.gone_at(&n1.s) == Some(0) && odd.s.gone_at(&n2.s) == Some(0), "the model: a row with {what}");
        // The shard after as if the dated row were the leg's (its head emptied), and with the row kept.
        let mut kept1 = next1;
        kept1.rows[0] = ShardRow { net: 0, gross: 0, ..odd.s.rows[0] };
        for r in 0..4 {
            assert!(!leaves(&w, &odd, &n1, &[r], next1) && !leaves(&w, &odd, &n1, &[r], kept1), "one leg leaves a row of its market with {what} (named at {r})");
        }
        let mut kept2 = next2;
        kept2.rows[0] = kept1.rows[0];
        assert!(!leaves(&w, &odd, &n2, &[0, 1], next2) && !leaves(&w, &odd, &n2, &[0, 1], kept2), "two legs, one of a row with {what}");
        assert!(voids(&w, &odd, &n1, 0) && voids(&w, &odd, &n2, 0) && !voids(&w, &odd, &n2, 1), "the note of a leg whose market has only a row with {what} is spent as of no row; not by its other leg");
    }
    // The market of nothing.
    let zero = leg_entered(&mleg(&w, [0; 32], 1, LONG), 0);
    let nz = note_t9(&w, &good, vec![zero]);
    assert!(good.s.left(&nz.s).is_err() && good.s.gone_at(&nz.s) == Some(0));
    for r in 0..4 {
        assert!(!leaves(&w, &good, &nz, &[r], good.s), "a leg of the market of nothing leaves row {r}");
    }
    assert!(voids(&w, &good, &nz, 0), "the note of a leg of the market of nothing is spent as of no row");
    // (And with the scale and limit of nothing, which an empty row has: no script divides by it.)
    let zz = XLeg { scale: 0, price_max: 0, ..zero };
    let nzz = note_t9(&w, &good, vec![zz]);
    for r in 0..4 {
        assert!(!leaves(&w, &good, &nzz, &[r], good.s), "a leg of nothing at all leaves row {r}");
    }
    println!("T9 K2: a leg of the market, scale and limit of nothing: voided {} (the model: gone at {:?})", voids(&w, &good, &nzz, 0), good.s.gone_at(&nzz.s));
}

// ----------------------------------------------------------------------------------------------------------------
// K3. A price answered: the committee by the seed.

/// A shard short 10 units of market 0 and 3 of market 1 from 10, holding 100 KAS, of the id its opening gives it.
fn two_rows(w: &World) -> OLive<Shard> {
    let base = seeded(w);
    let books = base.s.booked(&fleg(w, 0, 10, LONG, P10)).unwrap().booked(&fleg(w, 1, 3, LONG, P10)).unwrap();
    live(&base, books)
}
/// The world's market that the entry `e` of the list is of.
fn market_of(w: &World, t: &Traced, e: usize) -> usize {
    (0..w.markets.len()).find(|i| w.markets[*i].id() == t.list[e].market).expect("a market of the world")
}
/// The answer `b` of a price (right, on the shard it was built for) written for the same point of the same
/// statement on another shard `s2` that differs from it in its id alone (`q2`: that statement disputed), shown
/// the seed and the committee given.
fn retarget(w: &World, b: &OBuilt, s2: &Shard, q2: &OLive<Pointed>, seed: [u8; 32], cmt: [u8; 32]) -> OBuilt {
    // (The statement on the other shard writes that shard after: argument 9 is the hash of its outputs.)
    assert!(matches!(&args_of(&mut b.clone(), 1)[6], Arg::V(ArtifactValue::Bytes(x)) if x.len() == 171) && matches!(&args_of(&mut b.clone(), 1)[9], Arg::V(ArtifactValue::Bytes(x)) if x.len() == 32));
    let mut x = with_arg(&with_arg(&with_arg(&with_arg(b, 6, bytes_arg(&s2.moving())), 9, bytes_arg(&q2.s.st.outs.hash())), 20, bytes_arg(&seed)), 21, bytes_arg(&cmt));
    object_made(&mut x, &w.lib, 0, &q2.s.hash());
    x.tx.outputs[0].script_public_key = hash_spk(&w.lib, &q2.s.st.again().hash());
    x
}

/// K3. `KpfTShardPrice` reads no row: it is shown the seed of the shard's opening and a committee, holds their
/// hash with the shard's terms and periods to the shard's id, and the citation to that committee and to the
/// entry's market, plain. At the engine, on a draw of a shard of two rows:
///  - (control) each price is answered right with the citation of its market, shown the seed and the committee;
///  - another seed, another committee, the two the other way round, each of the two the shard's id: refused;
///  - the citation of the OTHER market of the same committee at this point: refused;
///  - a shard of another seed and the same committee: the answer written for it with its own seed is taken (the
///    control of the hand), with this shard's seed not;
///  - a shard whose id is opened for ANOTHER committee: the citation of this world's committee answers no price
///    of it, whichever committee is shown;
///  - a shard whose id is of chosen bytes (no script writes one): no price of it is answered, whatever is shown;
///    the dispute lapses at the cost of whoever stated (the lapse: `t1b_fixed_f69...`, `t5_b3...`).
#[test]
fn t9_k3_a_price_is_answered_for_the_committee_the_shards_id_is_opened_by() {
    let w = world();
    let shard = two_rows(&w);
    let s = shard.s;
    let committee = w.markets[0].committee_id;
    assert!(s.is_of(&SEED, &committee));
    let prices = [P10, P10, 0, 0];
    let d = Draw { amount: KAS1, prices: prices.into() };
    let t = s.traced(&d, false).unwrap();
    let (lv, _) = draw_stated(&w, &shard, &t);
    let cs = w.script("KpfTCite");
    let ps = w.script("KpfTShardPrice");
    let ff = [fund(xk(&w.f), 36)];
    let cites = [w.cited(market_of(&w, &t, 0), P10), w.cited(market_of(&w, &t, 1), P10)];
    let answer = |q: &OLive<Pointed>, at: i64, c: &Cited| shard_price_t(&w.lib, q, &s, SEED, &t, at, &ps, Some((&cs, c)), xk(&w.f), w.payer(&w.f, &ff), Some(true)).unwrap().0;
    let (q5, q6) = (w.pointed(&lv, 5), w.pointed(&lv, 6));
    let (b5, b6) = (answer(&q5, 5, &cites[0]), answer(&q6, 6, &cites[1]));
    assert!(accepted(&b5, &w.keys) && accepted(&b6, &w.keys), "(control) the two prices answered right");
    assert!(matches!(&args_of(&mut b5.clone(), 1)[20], Arg::V(ArtifactValue::Bytes(x)) if x[..] == SEED[..]) && matches!(&args_of(&mut b5.clone(), 1)[21], Arg::V(ArtifactValue::Bytes(x)) if x[..] == committee[..]) && matches!(&args_of(&mut b5.clone(), 1)[22], Arg::V(ArtifactValue::Int(5))) && args_of(&mut b5.clone(), 1).len() == 23, "arguments 20 to 22: the seed, the committee, the statement's kind");
    // The seed and the committee shown.
    let mut flipped = SEED;
    flipped[31] ^= 1;
    for other in [[0u8; 32], flipped, committee, s.sid, s.owner] {
        assert!(!accepted(&with_arg(&b5, 20, bytes_arg(&other)), &w.keys), "a price is answered with another seed shown");
    }
    let mut cflip = committee;
    cflip[0] ^= 0x80;
    for other in [[0u8; 32], cflip, SEED, s.sid, [0x99; 32]] {
        assert!(!accepted(&with_arg(&b5, 21, bytes_arg(&other)), &w.keys), "a price is answered with another committee shown");
    }
    assert!(!accepted(&with_arg(&with_arg(&b5, 20, bytes_arg(&committee)), 21, bytes_arg(&SEED)), &w.keys), "the seed and the committee the other way round");
    assert!(!accepted(&with_arg(&b5, 22, Arg::V(ArtifactValue::Int(11))), &w.keys), "a draw's price answered as a call's");
    // The citation of the other market at this point: the answer of the point 6 (with its citation) made the
    // point 5's, and the other way.
    for (b, q, at) in [(&b6, &q5, 5i64), (&b5, &q6, 6)] {
        let mut x = with_arg(&with_arg(b, 17, Arg::V(ArtifactValue::Int(at))), 16, Arg::V(ArtifactValue::Int(at)));
        object_made(&mut x, &w.lib, 0, &q.s.hash());
        assert!(!accepted(&x, &w.keys), "the price {at} is answered with the citation of the other market");
        // (Control of the hand: the same writing with nothing changed is the answer as it was.)
    }
    assert!(shard_price_t(&w.lib, &q5, &s, SEED, &t, 5, &ps, Some((&cs, &cites[1])), xk(&w.f), w.payer(&w.f, &ff), Some(true)).is_err(), "the builder: the citation of another market");
    // Other shards: the same books and statement, another id.
    let stated_on = |s2: Shard| {
        let l2 = live(&shard, s2);
        let (lv2, _) = draw_stated(&w, &l2, &t);
        w.pointed(&lv2, 5)
    };
    let seed2 = [0x5f; 32];
    let sister = s.seeded(&seed2, &committee);
    let q_sis = stated_on(sister);
    assert!(accepted(&retarget(&w, &b5, &sister, &q_sis, seed2, committee), &w.keys), "(control of the hand) a shard of another seed, answered with its seed");
    assert!(!accepted(&retarget(&w, &b5, &sister, &q_sis, SEED, committee), &w.keys), "a shard of another seed answered with this shard's");
    let theirs = s.seeded(&SEED, &[0x77; 32]);
    let q_th = stated_on(theirs);
    for (seed, cmt) in [(SEED, [0x77u8; 32]), (SEED, committee), ([0x77; 32], SEED), (seed2, committee)] {
        assert!(!accepted(&retarget(&w, &b5, &theirs, &q_th, seed, cmt), &w.keys), "a citation of this committee answers a price of a shard of another");
    }
    assert!(shard_price_t(&w.lib, &q_th, &theirs, SEED, &t, 5, &ps, Some((&cs, &cites[0])), xk(&w.f), w.payer(&w.f, &ff), Some(true)).is_err(), "the builder: a shard of another committee");
    let chosen = Shard { sid: [0x5d; 32], ..s };
    let q_ch = stated_on(chosen);
    for (seed, cmt) in [(SEED, committee), ([0x5d; 32], committee), (SEED, [0x5d; 32]), ([0; 32], [0; 32])] {
        assert!(!accepted(&retarget(&w, &b5, &chosen, &q_ch, seed, cmt), &w.keys), "a price of a shard whose id no seed opens is answered");
    }
    // (Its rows are answered as any other's: it is its prices nobody answers.)
    let l_ch = live(&shard, chosen);
    let (lv_ch, st_ch) = draw_stated(&w, &l_ch, &t);
    assert_eq!(row_taken(&w, &lv_ch, &chosen, &t, 1), (true, false));
    let q = w.pointed(&lv_ch, 5);
    let lapse = signed("T9 K3: the dispute of a price of a shard whose id no seed opens lapsed", &point_lapse(&w.lib, &q, &w.script("KpfTPoint"), w.payer(&w.f, &ff)).unwrap(), &w.keys);
    let k = lapse.tx.outputs.len();
    assert_eq!((lapse.tx.outputs[k - 1].value, &lapse.tx.outputs[k - 1].script_public_key), ((st_ch.bond + st_ch.bond / 2) as u64, &p2pk_spk(&xk(&w.w))), "the disputer: its bond and half of the claimer's");
}

/// K3 (finding Q2). "An entry no row is of is priced as any other" (docs 17.8d; it was "cited by nobody: it costs
/// whoever wrote it"). A draw that is the rule's but for one entry more in its list, in its order, of a market no
/// row has and the shard's committee signs (the world's market 2):
///  - priced within the tolerance of the committee's price: every point is answered RIGHT and none wrong (the
///    end, the rows, the three prices): the statement stands. It was unanswerable and cost its writer a bond;
///  - priced outside it: that point is answered wrong with the citation (it costs its writer, by an answer and
///    no longer by a lapse);
///  - the model's `trace_wrong_point` still NAMES the point of the entry of no row as wrong, whatever its price:
///    a watcher who disputes by the model where the entry is priced right is answered right and loses its bond.
/// (An entry of a market the committee signs no price of is answered by nobody, as before: `t5_b3...`.)
#[test]
fn t9_k3_an_entry_no_row_is_of_is_priced_as_any_other_and_the_model_judges_only_its_order() {
    let w = world();
    let shard = two_rows(&w);
    let s = shard.s;
    let prices = [P10, P10, 0, 0];
    let t = s.traced(&Draw { amount: KAS1, prices: prices.into() }, false).unwrap();
    let m2 = w.markets[2].id();
    for (what, price, want) in [("priced right", P10, (true, false)), ("priced at twice the committee's", 2 * P10, (false, true))] {
        let more = with_entry(&t, Priced { market: m2, limit: t.list[0].limit, day: 0, price });
        let e2 = more.list.iter().position(|p| p.market == m2).unwrap();
        assert!(more.list.len() == 3 && more.list.windows(2).all(|x| x[0].below(&x[1])) && s.row(&m2).is_none());
        let (lv, st) = draw_stated(&w, &shard, &more);
        let f = [fund(xk(&w.f), 36)];
        let end = |v: bool| shard_end_t(&w.lib, &w.pointed(&lv, 0), &s, &more, &w.script("KpfTShardEnd"), xk(&w.f), w.payer(&w.f, &f), Some(v)).is_ok_and(|b| accepted(&b.0, &w.keys));
        assert_eq!((end(true), end(false)), (true, false), "{what}: the end");
        for at in 1..=4 {
            assert_eq!(row_taken(&w, &lv, &s, &more, at), (true, false), "{what}: the row {at}");
        }
        for e in 0..3usize {
            let c = w.cited(market_of(&w, &more, e), P10);
            let got = price_taken(&w, &lv, &s, &more, 5 + e as i64, Some(&c));
            assert_eq!(got, if e == e2 { want } else { (true, false) }, "{what}: the price {e}");
        }
        assert_eq!(price_taken(&w, &lv, &s, &more, 5 + e2 as i64, None), (false, false), "{what}: the entry of no row answered with nothing cited");
        assert_eq!(price_taken(&w, &lv, &s, &more, 8, None), (true, false), "{what}: the point beyond the list");
        let named = s.trace_wrong_point(&more, false, &xk(&w.c), &st.outs, shard.value as i64, T0, &prices);
        println!("T9 K3: an entry more of a market the committee signs, of no row, {what}: the engine answers it {want:?} (right, wrong); the model's first wrong point: {named:?}");
        // (Q2 closed, 17.8e: the model is given no price of a market no row is of, and names no point for it.)
        assert_eq!(named, None, "Q2 is open again: the model names the point of an entry no row is of");
    }
}

/// K3, the list's order where a market's chunks have the top bit set. The order is of the market's 32 bytes as
/// four NUMBERS (sign and magnitude): `ff..ff` is below `01..01`. A shard with a row of each (made by hand, the
/// id its opening gives it; the committee signs both): the model's list has `ff..ff` first; both prices are
/// answered right. Written in the order of the bytes, the second is answered wrong with nothing cited.
#[test]
fn t9_k3_the_lists_order_is_of_numbers_where_a_markets_top_bits_are_set() {
    let w = world();
    let base = seeded(&w);
    let (hi, lo) = ([0xffu8; 32], [0x01u8; 32]);
    let books = base.s.booked(&mleg(&w, lo, 2, LONG)).unwrap().booked(&mleg(&w, hi, 3, SHORT)).unwrap();
    let shard = live(&base, books);
    let s = shard.s;
    let t = s.traced(&Draw { amount: KAS1, prices: [P10, P10, 0, 0].into() }, false).unwrap();
    assert_eq!((t.list[0].market, t.list[1].market, t.rows[0].at, t.rows[1].at), (hi, lo, 1, 0), "the model's order");
    let (lv, _) = draw_stated(&w, &shard, &t);
    assert_eq!((row_taken(&w, &lv, &s, &t, 1), row_taken(&w, &lv, &s, &t, 2)), ((true, false), (true, false)));
    assert_eq!((price_taken(&w, &lv, &s, &t, 5, Some(&w.cited_of(hi, P10))), price_taken(&w, &lv, &s, &t, 6, Some(&w.cited_of(lo, P10)))), ((true, false), (true, false)), "the order of the numbers");
    let mut bytes = t.clone();
    bytes.list.swap(0, 1);
    for row in bytes.rows.iter_mut().take(2) {
        row.at = 1 - row.at;
    }
    let (lv2, _) = draw_stated(&w, &shard, &bytes);
    assert_eq!(price_taken(&w, &lv2, &s, &bytes, 6, None), (false, true), "the order of the bytes: the second entry");
    assert!(!price_taken(&w, &lv2, &s, &bytes, 6, Some(&w.cited_of(hi, P10))).0);
}

/// K3b. One entry a market, where two rows of one market have two limits (made by hand: a futures row and a row
/// with a day; the tree's rule gives a dated row its futures row's limit). The model has no trace. At the
/// engine no statement over such books is right: said with one entry, the row of the other limit is answered
/// wrong; said with two entries of the one market, the second is not above the first and is answered wrong with
/// nothing cited. (Control: with one limit, one entry, every point right.)
#[test]
fn t9_k3b_two_rows_of_one_market_with_two_limits_have_no_right_trace() {
    let w = world();
    let base = seeded(&w);
    let (scale, pm, m) = (w.markets[0].p.scale, w.markets[0].p.price_max, w.markets[0].id());
    let fut = ShardRow::futures(m, scale, pm, -10 * scale, 10 * scale);
    let dated = |limit: i64| ShardRow { key: Key { market: m, day: 9_000, of: OF_CALLS, strike: 1 }, ..ShardRow::futures(m, scale, limit, 5 * scale, 5 * scale) };
    let same = live(&base, Shard { rows: [fut, dated(pm), E, E].into(), basis: -50 * KAS1, ..base.s });
    let odd = live(&base, Shard { rows: [fut, dated(pm - 1), E, E].into(), basis: -50 * KAS1, ..base.s });
    let d = Draw { amount: KAS1, prices: [P10, P10, 0, 0].into() };
    let t = same.s.traced(&d, false).unwrap();
    assert_eq!((t.list.len(), t.rows[0].at, t.rows[1].at), (1, 0, 0));
    assert!(odd.s.traced(&d, false).is_none(), "the model: two limits, no trace");
    let c = w.cited(0, P10);
    let (lv, _) = draw_stated(&w, &same, &t);
    assert_eq!((row_taken(&w, &lv, &same.s, &t, 1), row_taken(&w, &lv, &same.s, &t, 2), price_taken(&w, &lv, &same.s, &t, 5, Some(&c))), ((true, false), (true, false), (true, false)), "(control) one limit");
    let (lv, _) = draw_stated(&w, &odd, &t);
    assert_eq!((row_taken(&w, &lv, &odd.s, &t, 1), row_taken(&w, &lv, &odd.s, &t, 2)), ((true, false), (false, true)), "one entry: the row of the other limit");
    let mut two = t.clone();
    two.list.push(Priced { limit: pm - 1, ..t.list[0] });
    two.e = 2;
    two.rows[1].at = 1;
    let (lv, _) = draw_stated(&w, &odd, &two);
    assert_eq!(price_taken(&w, &lv, &odd.s, &two, 6, None), (false, true), "two entries of the one market: the second");
    assert!(!price_taken(&w, &lv, &odd.s, &two, 6, Some(&c)).0);
    println!("T9 K3b: two entries of one market, the row of the second limit naming the second: its row's point {:?}", row_taken(&w, &lv, &odd.s, &two, 2));
}

// ----------------------------------------------------------------------------------------------------------------
// K4. A call's step: the plain market.

/// The step `b` under the call written for a call's object whose list is the bytes `body`.
fn recall(w: &World, b: &OBuilt, call: &OLive<Called>, body: &[u8]) -> OBuilt {
    let h = Called::hash_of(&call.s.sid, call.s.time, body);
    let mut x = with_arg(b, 8, bytes_arg(body));
    object_made(&mut x, &w.lib, 2, &h);
    x.tx.outputs[0].script_public_key = hash_spk(&w.lib, &h);
    x
}

/// K4. `KpfTCStep` holds the list's entry to the leg's market as bytes. A leg of market M (its top bit set or
/// not) under a call whose list has M: found, wherever M stands in the list. A list whose entry is the old form's
/// name (the hash of the futures' tag and M), or the options' name: refused. An entry of M with another limit, or
/// of a day: refused. A list written by hand with M twice (no trace that is right has one; `KpfTCStep` reads no
/// order): the step is taken at either place.
#[test]
fn t9_k4_a_calls_step_finds_the_legs_market_plain_in_the_list() {
    let w = world();
    let base = seeded(&w);
    let hi = [0xffu8; 32];
    for (what, raw) in [("a market of the world", fleg(&w, 0, 10, LONG, P10)), ("a market whose top bits are set", mleg(&w, hi, 10, LONG))] {
        let leg = leg_entered(&raw, 0);
        let other = fleg(&w, 1, 3, SHORT, P10);
        let books = base.s.booked(&raw).unwrap().booked(&other).unwrap();
        let shard = live(&base, Shard { held: 20 * KAS1, ..books });
        let p13 = P10 * 13 / 10;
        let (_, call) = called_objs(&shard, [p13, P10, 0, 0], 6_600);
        let list = call.s.list();
        let r = list.iter().position(|e| e.market == raw.market).unwrap();
        assert!(list.len() == 2 && list[0].below(&list[1]) && (raw.market != hi || r == 0));
        let a = cov(&w, &shard, &[leg], 40 * KAS1);
        let ff = [fund(xk(&w.f), 36)];
        let (b, step) = call_step(&w.lib, &a, &a.s, None, &call, &w.script("KpfTCStep"), w.payer(&w.f, &ff)).unwrap();
        assert!(accepted(&b, &w.keys) && step.at.len() == 1 && step.at[0] as i64 == r as i64 && step.note.asked == 30 * KAS1, "(control) {what}: the step, the leg's price found at {r}");
        assert!(matches!(&args_of(&mut b.clone(), 1)[8], Arg::V(ArtifactValue::Bytes(x)) if x[..] == call.s.body()[..]) && matches!(&args_of(&mut b.clone(), 1)[5], Arg::V(ArtifactValue::Bytes(x)) if x[..] == sm8_t9(r as i64)[..]), "arguments 5 and 8: the places and the list");
        let body = call.s.body();
        assert!(accepted(&recall(&w, &b, &call, &body), &w.keys), "(control of the hand) the same list");
        let en = &body[56 * r..56 * r + 56];
        let with_name = |name: [u8; 32]| [&body[..56 * r], &name[..], &en[32..], &body[56 * r + 56..]].concat();
        assert!(!accepted(&recall(&w, &b, &call, &with_name(kpf_protocol::reserve::futures_market(&raw.market))), &w.keys), "{what}: a list of the old form's names (futures)");
        assert!(!accepted(&recall(&w, &b, &call, &with_name(kpf_protocol::reserve::option_market(&raw.market))), &w.keys), "{what}: a list of the old form's names (options)");
        let mut twin = raw.market;
        twin[31] ^= 0x80;
        assert!(!accepted(&recall(&w, &b, &call, &with_name(twin)), &w.keys), "{what}: a market of another top bit");
        let mut limit = body.clone();
        limit[56 * r + 32] ^= 1;
        let mut day = body.clone();
        day[56 * r + 40] = 1;
        let mut below = body.clone();
        below[56 * r + 47] = 0x80;
        assert!(!accepted(&recall(&w, &b, &call, &limit), &w.keys) && !accepted(&recall(&w, &b, &call, &day), &w.keys) && !accepted(&recall(&w, &b, &call, &below), &w.keys), "{what}: an entry of another limit, of a day, of a day of nothing written below zero");
        // The leg's entry at another place of a longer list; and twice.
        let first = Priced { market: [0x7e; 32], limit: 1, day: 0, price: 1 }.bytes();
        let longer = [&first[..], &body[..]].concat();
        let moved = with_arg(&recall(&w, &b, &call, &longer), 5, bytes_arg(&sm8_t9(r as i64 + 1)));
        assert!(accepted(&moved, &w.keys) && !accepted(&recall(&w, &b, &call, &longer), &w.keys), "{what}: the entry a place later, named there and not where it was");
        let twice = [&body[..56 * r + 56], en, &body[56 * r + 56..]].concat();
        let at_second = with_arg(&recall(&w, &b, &call, &twice), 5, bytes_arg(&sm8_t9(r as i64 + 1)));
        assert!(accepted(&recall(&w, &b, &call, &twice), &w.keys) && accepted(&at_second, &w.keys), "{what}: (a list no right trace has) the market twice: either place");
    }
}

// ----------------------------------------------------------------------------------------------------------------
// K5. The model and the engine on random books.

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

/// K5. Random books of rows named by keys (futures rows of six markets, two of them made, one with its top bits
/// set; one row in seven with a day; a row in nine with another limit than the world's), a random leg (a market
/// of the six, or of nothing; sometimes another limit), a random row named:
///  - the entry: the engine takes it where the model does, the shard after the model's; where the model refuses,
///    the shard after is written by hand (the leg's units added to the row named, under the leg's head) and the
///    engine refuses;
///  - the leaving of that leg (a note of one leg; and of two, the second of another market): applied where the
///    model applies it, at the model's rows to the model's shard; where the model refuses the note of one leg, it
///    is applied at no row named (by hand: the row's units less, its head emptied where none is left);
///  - the void: taken for the leg the model says is gone, and for no leg before it.
#[test]
fn t9_k5_an_entry_a_leaving_and_a_void_on_random_books_the_model_and_the_engine() {
    let w = world();
    let base = seeded(&w);
    let scale = w.markets[0].p.scale;
    let pm = w.markets[0].p.price_max;
    let pool: Vec<[u8; 32]> = (0..4).map(|i| w.markets[i].id()).chain([[0x01; 32], [0xff; 32]]).collect();
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut entered, mut refused, mut applied, mut unapplied, mut voided, mut two_applied, mut dated_met) = (0, 0, 0, 0, 0, 0, 0);
    for case in 0..60 {
        let mut rows = [E; 4];
        let mut left: Vec<[u8; 32]> = pool.clone();
        for row in rows.iter_mut() {
            if r.below(5) < 2 {
                continue;
            }
            let m = left.remove(r.below(left.len() as i64) as usize);
            let gross = (1 + r.below(5)) * scale;
            let net = (r.below(2 * (gross / scale) + 1) - gross / scale) * scale;
            let mut x = ShardRow::futures(m, scale, if r.below(9) == 0 { pm - 1 } else { pm }, net, gross);
            if r.below(7) == 0 {
                x.key.day = 5;
            }
            *row = x;
        }
        let shard = live(&base, Shard { rows: rows.into(), ..base.s });
        let s = &shard.s;
        let market = match r.below(12) {
            0 => [0u8; 32],
            _ => pool[r.below(6) as usize],
        };
        let leg = XLeg { price_max: if r.below(9) == 0 { pm - 1 } else { pm }, ..mleg(&w, market, 1, if r.below(2) == 0 { LONG } else { SHORT }) };
        let head = RowHead::of(&leg);
        let at = if r.below(3) == 0 { r.below(4) as usize } else { s.row_for(&head).unwrap_or(r.below(4) as usize) };
        dated_met += s.rows.iter().any(|x| x.key.market == market && x.key.day != 0) as usize;
        // The entry.
        let model = s.entered(&leg, at, T0);
        let next = match &model {
            Ok(n) => *n,
            Err(_) => after(s, &leg, at, with_leg(head, &s.rows[at], &leg)),
        };
        let engine = enters(&w, &shard, leg, at, &next);
        assert!(engine == Some(model.is_ok()) || (engine.is_none() && model.is_err()), "case {case}: the entry of {leg:?} into row {at} of {rows:?}: the engine {engine:?}, the model {:?}", model.as_ref().err());
        if model.is_ok() { entered += 1 } else { refused += 1 }
        // The leaving of one leg.
        let e = leg_entered(&leg, 0);
        let n1 = note_t9(&w, &shard, vec![e]);
        let gone = s.gone_at(&n1.s);
        match s.left(&n1.s) {
            Ok((next, rows_at, paid, rest)) => {
                assert!(paid == 0 && rest.is_none() && gone.is_none());
                assert!(leaves(&w, &shard, &n1, &rows_at, next), "case {case}: the note of {leg:?} is not applied to {rows:?}");
                applied += 1;
            }
            Err(_) => {
                for q in 0..4 {
                    let mut by_hand = *s;
                    by_hand.rows[q] = without_leg(&by_hand.rows[q], &leg);
                    assert!(!leaves(&w, &shard, &n1, &[q], by_hand) && !leaves(&w, &shard, &n1, &[q], *s), "case {case}: the note of {leg:?} is applied at row {q} of {rows:?}");
                }
                unapplied += 1;
            }
        }
        let void = voids(&w, &shard, &n1, 0);
        assert_eq!(void, gone == Some(0), "case {case}: the void of the note of {leg:?} on {rows:?}");
        voided += void as usize;
        // Two legs.
        let m2 = pool[r.below(6) as usize];
        if m2 != market {
            let l2 = leg_entered(&mleg(&w, m2, 1, if r.below(2) == 0 { LONG } else { SHORT }), 0);
            let n2 = note_t9(&w, &shard, vec![e, l2]);
            let gone2 = s.gone_at(&n2.s);
            if let Ok((next, rows_at, _, _)) = s.left(&n2.s) {
                assert!(gone2.is_none() && leaves(&w, &shard, &n2, &rows_at, next), "case {case}: the note of two legs is not applied");
                two_applied += 1;
            }
            for k in 0..2 {
                let v = voids(&w, &shard, &n2, k);
                assert!(gone2.is_some() || !v, "case {case}: a note of two legs of the books is spent by its leg {k}");
                if gone2 == Some(k) {
                    assert!(v, "case {case}: the note of two legs is not spent by the leg {k} the model says is gone");
                }
                if gone2.is_some_and(|g| k < g) {
                    assert!(!v, "case {case}: the note of two legs is spent by a leg before the one that is gone");
                }
            }
        }
    }
    println!("T9 K5: 60 random books: entries taken {entered}, refused {refused}; notes of one leg applied {applied}, not applied {unapplied}, voided {voided}; notes of two legs applied {two_applied}; the leg's market had a row with a day in {dated_met}");
    assert!(entered >= 10 && refused >= 5 && applied >= 5 && unapplied >= 10 && voided >= 10 && two_applied >= 1 && dated_met >= 1, "the cases are of every kind");
}
