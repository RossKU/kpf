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
        let s = Shard { t: self.terms(), rows: rows.into(), owner: xk(&self.c), held: 100 * KAS1, expiry: 0, dispute_daa: 50, bond_bps: 1_000, sid: [0x5d; 32], basis: -500 * KAS1, called: 0, owed: 0, claims: Default::default(), root: Shard::root_opened() };
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
    let (b, next, claim) = left_apply_c(&w.lib, shard, &note, NOTE_STEP, step.young, &w.script(left_script_on(&(shard).s, (&note).s.legs.len())), p).unwrap();
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
        // (Review T7, docs 17.8c: the steps' notes are of tag 9 and are applied as such. What the owner's YOUNG leg
        // gained is not nothing any more: it is said beside what is asked, 3,000 KAS less the band of 130 (the
        // tolerance twice of the leg's worth at the call, 13,000), and is a claim of the second class. It is
        // behind the honest claim, which is of the first: paid of what the shard has beyond what the first class
        // is owed.)
        let band = (1_000 * (P10 * 13 / 10) / 10_000) * 100;
        let young_o = if asks == 0 { 3_000 * KAS1 - band } else { 0 };
        assert_eq!((s_h.young, s_o.young, band), (0, young_o, 130 * KAS1), "{what}: what the young legs gained, less their band");
        let (b, sh1, c_h) = step_apply_t7(&w, &called, &sg_h, &s_h, 6_610, w.payer(&w.f, &ff));
        let sg = signed("the honest note applied", &b, &w.keys);
        let c_h = c_h.expect("a claim");
        assert_eq!((c_h.amount1, c_h.amount2), (15 * KAS1, 0), "the honest claim is of the first class");
        let (sh1, c_h) = (nth(&sg, 0, sh1, 6_620), nth(&sg, 2, kpf_protocol::reserve::Rest { sid: c_h.sid, key: c_h.key, amount: c_h.amount1 }, 6_620));
        let (b, sh2, c_o) = step_apply_t7(&w, &sh1, &sg_o, &s_o, 6_610, w.payer(&w.f, &ff));
        let sg = signed("the owner's note applied", &b, &w.keys);
        let sh2 = nth(&sg, 0, sh2, 6_630);
        assert_eq!((sh2.s.rows, sh2.s.owed, sh2.s.claims.owed2, sh2.s.held, c_o.map(|c| (c.amount1, c.amount2))), ([ShardRow::EMPTY; 4].into(), 15 * KAS1 + asks, young_o, 20 * KAS1 - fee, Some((asks, young_o))), "{what}: the books are empty, and what is owed by claims of each class");
        let c_o = c_o.map(|c| nth(&sg, 2, c, 6_630));
        // The honest claim paid. (Review T6, docs 17.8c: from the time of the claims, which a coin that landed
        // then shows; and its part of what the shard ever held, where that is at least a note's least.)
        assert!(claim_pay(&w.lib, &sh2, &c_h, &pay_s, w.payer(&w.f, &ff)).is_err(), "a claim is paid before the time of the claims");
        let fl = [coin(xk(&w.f), 36, sh2.s.claims_time().unwrap())];
        match claim_pay(&w.lib, &sh2, &c_h, &pay_s, w.payer(&w.f, &fl)) {
            Ok((b, _, paid)) => {
                let sg = signed(&format!("F116, the owner's leg {what}: the honest claim paid"), &b, &w.keys);
                // (Review T6, second round, K1: paid whole, its key has the part and what the claim's note held,
                // in one output.)
                assert!(paid == 15 * KAS1 && pays(&sg, &xk(&w.w), paid + c_h.value as i64) && !pays(&sg, &xk(&w.w), paid));
                paid_honest.push(paid);
                // (Review T7: the owner's claim of the second class, shown FIRST, is paid what the shard has
                // beyond what the first class is owed and no more: 4.9 of its 2,870 KAS; the honest claim finds
                // its 15 KAS whatever the order.)
                let c_o = c_o.clone().expect("the owner's claim of the second class");
                let (b, after, got, left) = claim_pay_c(&w.lib, &sh2, &c_o, &pay_s, w.payer(&w.f, &fl)).unwrap();
                signed(&format!("F116, the owner's leg {what}: the owner's claim of the second class, paid first"), &b, &w.keys);
                assert_eq!((got, after.held, left.map(|c| c.paid2)), (5 * KAS1 - fee, 15 * KAS1, Some(5 * KAS1 - fee)), "{what}: the second class has what is beyond the first");
                assert_eq!(after.claim_paid_at(&c_h.s.claim(), i64::MAX).map(|x| x.1).ok(), Some(15 * KAS1));
            }
            Err(_) => {
                // (What F116 was, as it is now: the honest claim's part of 19.9 KAS over 3,015 owed is 0.099 KAS,
                // below a note's least: it is not paid and not spent (it was paid the 0.099 and spent). It waits
                // for coins, as the owner's claim of 3,000 does not: that is paid its 19.8 KAS at once.)
                let (h, o) = (sh2.s.held / 1_000, (sh2.s.owed - 1) / 1_000 + 1);
                let part = (15 * KAS1 / o) * h + (15 * KAS1 % o) * h / o;
                assert!(asks > 0 && part < NOTE_MIN, "{what}: a part of {part}");
                let c = OLive { s: c_h.s.claim(), value: c_h.value, id: c_h.id, op: c_h.op, daa: c_h.daa };
                let next = Shard { held: sh2.s.held - part, claims: Claims { paid1: part, owed2: 0, paid2: 0 }, ..sh2.s };
                for left in [Some(Claim { paid1: part, ..c.s }), None] {
                    let b = claim_pay_as_c(&w.lib, &sh2, &c, next, part, left, &pay_s, w.payer(&w.f, &fl));
                    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "{what}: a part below a note's least is paid");
                }
                let c_o = c_o.clone().expect("the owner's claim");
                let c_o = OLive { s: kpf_protocol::reserve::Rest { sid: c_o.s.sid, key: c_o.s.key, amount: c_o.s.amount1 }, value: c_o.value, id: c_o.id, op: c_o.op, daa: c_o.daa };
                let (b, after, got) = claim_pay(&w.lib, &sh2, &c_o, &pay_s, w.payer(&w.f, &fl)).unwrap();
                signed(&format!("F116, the owner's leg {what}: the owner's claim paid its part"), &b, &w.keys);
                assert!(got > 198 * KAS1 / 10 && after.held < KAS1 / 10 && after.owed == sh2.s.owed, "{what}: {got}");
                paid_honest.push(part);
            }
        }
    }
    println!("T3 F116 at the engine: an honest claim of 15 KAS on a called shard of 19.9 KAS, beside the owner's long of 1,000 units that gained 3,000 KAS: paid {} sompi where the owner's leg is young, {} where it is old", paid_honest[0], paid_honest[1]);
    assert_eq!(paid_honest[0], 15 * KAS1, "F116 is open: the honest claim is not paid whole beside the owner's young leg");
    assert!(paid_honest[1] < KAS1 / 10 && paid_honest[1] > 9 * KAS1 / 100, "(what F116 was: the part; it waits now, below a note's least) {}", paid_honest[1]);

    // Review T7, the gate (docs 17.8c, piece (c)). The books above are written by hand: `KpfTEnterS` does not let
    // them come to be. Both, at the engine:
    //  - into the shard of 20 KAS the owner's long of 1,000 units from 10 does not enter (the row with it asks
    //    778.875 KAS: 7.75 % of 10,050): the play fails at its entry;
    //  - it enters where the shard holds that much, to the sompi. In that shard, paid up by 758.875 KAS of the
    //    owner's, the OLD leg's play (what F116 was for every leg: a price that went from 10 to 13 with nobody
    //    calling for T) pays the honest claim of 15 its part of 778.775 over 3,015: 3.87 KAS where it was 0.099.
    //    The young leg's play pays it whole, as above.
    let with_honest = base.s.booked(&honest).unwrap();
    let need = row_need(1_005, 1_005, P10);
    assert_eq!(need, 778_875 * KAS1 / 1_000);
    assert!(!gate_takes(&w, &base, 20 * KAS1, with_honest, big, 0), "the owner's 1,000 units enter a shard of 20 KAS");
    assert!(gate_takes(&w, &base, need, with_honest, big, 0) && !gate_takes(&w, &base, need - 1, with_honest, big, 0), "the gate, to the sompi");
    let paid_up = OLive { s: Shard { held: need, ..books }, value: (need + SHARD_CARRIER) as u64, ..base.clone() };
    let (called_up, _) = called_objs(&paid_up, p13, 6_600);
    let old_h = Left { sid: base.s.sid, legs: entered(&[honest], T0 - 2 * t), key: xk(&w.w), asked: 15 * KAS1 };
    let old_o = Left { sid: base.s.sid, legs: entered(&[big], T0 - t), key: xk(&w.c), asked: 3_000 * KAS1 };
    let (s1, _, _, ch) = called_up.s.left_class(&old_h, NOTE_STEP, 0, NOTE_MIN).unwrap();
    let (s2, _, _, _) = s1.left_class(&old_o, NOTE_STEP, 0, NOTE_MIN).unwrap();
    let live = OLive { s: s2, ..called_up.clone() };
    let fl = [coin(xk(&w.f), 36, s2.claims_time().unwrap())];
    let c = OLive { s: ch.unwrap(), value: NOTE_MIN as u64, id: base.id, op: op(77), daa: 6_640 };
    let (b, _, got, left) = claim_pay_c(&w.lib, &live, &c, &pay_s, w.payer(&w.f, &fl)).unwrap();
    signed("F116 with the gate: the honest claim beside the owner's old leg, the shard paid up", &b, &w.keys);
    println!("T7, F116 with the gate: into a shard of 20 KAS the owner's 1,000 units do not enter; the shard paid up to {need} sompi, the honest claim of 15 KAS beside the owner's old leg that gained 3,000 is paid {got} sompi (0.099 KAS before the gate)");
    assert!(got > 387 * KAS1 / 100 && got < 388 * KAS1 / 100 && left.is_some(), "{got}");
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
    let (b, right) = c_row(&w.lib, &q, &said, 1, &[0; 32], None, &l_m, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
    assert!(right);
    signed("the row of a mark answered right", &b, &w.keys);
    // (The same mark, its chain of the leg at its new entry with another time: the row is wrong.)
    for other in [T0 - t - 1_000, T0, 0] {
        let mut rows = said.rows.clone();
        rows[0].h = chain_of_one(&XLeg { entry: ne, ..leg_entered(&raw, other) });
        let lie = CSaid { rows, ..said.clone() };
        let st = Stated { arg: lie.arg(), ..live.s };
        let q = w.pointed(&acct_stated(&w, &m_acct, &st, &lie.said()), 1);
        let (b, right) = c_row(&w.lib, &q, &lie, 1, &[0; 32], None, &l_m, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
        assert!(!right && accepted(&b, &w.keys), "a mark that rewrites the leg's time to {other}: its row is answered wrong");
        let (b, _) = c_row(&w.lib, &q, &lie, 1, &[0; 32], None, &l_m, &row_script, xk(&w.f), w.payer(&w.w, &fw), Some(true)).unwrap();
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
    let (b, right) = c_row(&w.lib, &q, &said, 3, &said.rows[0].g, Some(&l_m), &l_s, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
    assert!(right);
    signed("the row of the leg that leaves answered right", &b, &w.keys);
    let lie = CSaid { lc: chain_of_one(&leg_entered(&l_s, 0)), ..said.clone() };
    let st = Stated { arg: lie.arg(), ..live.s };
    let q = w.pointed(&acct_stated(&w, &s_acct, &st, &lie.said()), 3);
    let (b, right) = c_row(&w.lib, &q, &lie, 3, &said.rows[0].g, Some(&l_m), &l_s, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
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
    // (Review T7, docs 17.8c: the note is of tag 9 and says what the young legs gained less their band: 60 KAS
    // less the tolerance twice of 260, the two legs' worth at the call. Applied, the legs are out of the books and
    // it is a claim of the second class: "makes no claim" was the hole L4.)
    let young2 = 60 * KAS1 - 26 * KAS1 / 10;
    assert_eq!(st.young, young2);
    assert!(pays(&sg, &xk(&w.w), st.user) && out_of(&sg, &hash_spk(&w.lib, &st.note_hash())).1 == NOTE_MIN as u64);
    let (b, next, claim) = step_apply_t7(&w, &called, &sg, &st, 6_610, w.payer(&w.f, &ff));
    signed("the note of young legs that gained, applied to the called shard", &b, &w.keys);
    assert_eq!((next.rows, next.owed, next.claims.owed2, next.held, claim.map(|c| (c.amount1, c.amount2))), ([ShardRow::EMPTY; 4].into(), 0, young2, called.s.held, Some((0, young2))));
    // A young loss larger than the coins.
    let (st, sg) = stepped("a young short that lost more than its account has", &cov(&w, &shard, &[leg_entered(&short, T0 - 1)], 5 * KAS1));
    assert_eq!((st.gain, st.pay, st.note.asked, st.user), (-30 * KAS1, 5 * KAS1 - NOTE_MIN, -(5 * KAS1 - NOTE_MIN), 0));
    assert_eq!((out_of(&sg, &hash_spk(&w.lib, &st.note_hash())).1, st.young), ((5 * KAS1) as u64, 0));

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
    // (Review T7: and the young's amount is said whole beside it, 30 KAS less the band of 1.3: the account still
    // owes its 6 KAS, and the claim of the second class is not the less for them.)
    assert_eq!(st.young, 30 * KAS1 - 13 * KAS1 / 10);
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
        c_enter_state_as(&w.lib, a, &seeded, &enter, SEED, long.committee, &st, &said, &next, w.payer(&w.w, &f))
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
    // (Review T7, docs 17.8c: L1 AS FOUND IS CLOSED. The note of an account's own leaving is counted in its class
    // where it is applied: its leg is young at the call, so the 48.5 KAS are a claim of the second class, whole,
    // and of the first class nothing is owed for it. Counted in the first class, the transaction is refused. This
    // test asserted the hole: `claim.amount == next.owed == 48.5 KAS`, at one share with every honest claim.)
    let live_note = nth(&ap, k, note.clone(), quiet as u64 + 50);
    assert!(left_apply(&w.lib, &called, &live_note, &w.script(left_script_on(&(&called).s, (&live_note).s.legs.len())), w.payer(&w.f, &ff)).is_err(), "L1: the young leg's leaving is a claim of the first class");
    let (nb, next, claim) = left_apply_c(&w.lib, &called, &live_note, NOTE_LEFT, 0, &w.script(left_script_on(&(&called).s, (&live_note).s.legs.len())), w.payer(&w.f, &ff)).unwrap();
    signed("L1: its note applied to the called shard", &nb, &w.keys);
    let claim = claim.expect("a claim");
    let asked = 4_850_000_000;
    let first = left_apply_as_c(&w.lib, &called, &live_note, NOTE_LEFT, 0, &[0], Shard { owed: asked, claims: called.s.claims, ..next }, 0, Some(Claim { amount1: asked, amount2: 0, ..claim }), &w.script(left_script_on(&(&called).s, (&live_note).s.legs.len())), w.payer(&w.f, &ff));
    assert!(first.is_err() || !accepted(&first.unwrap().0, &w.keys), "L1 is open: the leaving of a young leg is counted in the first class");
    assert_eq!((note.asked, claim.amount1, claim.amount2, next.owed, next.claims.owed2, next.rows), (asked, 0, asked, 0, asked, [ShardRow::EMPTY; 4].into()), "L1 is open: the leaving of a young leg is a claim of the first class");
    // What it does to an honest claim (the model of `KpfTCallPay`, which review T6 ran against the engine): the
    // shard holds 9.9 KAS; beside an old user's claim of 9 KAS of the first class the old claim is paid whole and
    // the young leaving 0.9 KAS; at one share (before) the old claim had 1.55 KAS of its 9.
    let beside = Shard { owed: 9 * KAS1, ..next };
    let due = beside.claims_time().unwrap();
    let honest = Claim { sid: beside.sid, key: xk(&w.f), amount1: 9 * KAS1, paid1: 0, amount2: 0, paid2: 0 };
    let (after, got_young, _) = beside.claim_paid_at(&claim, due).unwrap();
    let (_, got_old, _) = after.claim_paid_at(&honest, due).unwrap();
    assert_eq!((got_old, got_young), (9 * KAS1, beside.held - 9 * KAS1));
    // What is left of it (finding S3 of review T7): the leaving is at its user's own later price. The step of the
    // same leg says 18.8 KAS for the young (20 less the band of 1.2); the leaving 101 blocks later says 48.5.
    // Within the second class the leg that was not stepped has 2.6 times its step's share (F95, in the second
    // class).
    let young_at_call = at_call.young;
    assert_eq!(young_at_call, 20 * KAS1 - 12 * KAS1 / 10);
    println!("T3 L1, with the second class: a leg that is young at the call: its step asks {} sompi and says {young_at_call} for the young; its user's leaving, stated 101 blocks after the call's time and applied 50 later, asks {}: a claim of the second class of {} (owed by claims of the first class {}, of the second {}); beside an old claim of 9 KAS on 9.9 held, the old is paid {got_old} and the leaving {got_young}", at_call.note.asked, note.asked, claim.amount2, next.owed, next.claims.owed2);
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
    // (Review T9, docs 17.8d: a row says NO committee, so "a row whose head says another committee" is an object
    // that cannot be written any more, by hand or otherwise: the 420 bytes of the rows hold the committee nowhere,
    // and what this half gave up in review T3 is given back. What it holds now: the leg's row is the row of the
    // leg's KEY (the market plain, a day, a kind and a strike of nothing), scale and limit: found, and the units
    // taken out; and a row of the leg's market that has a day (made by hand: no script writes one) is NOT the
    // leg's row: the model has no row for the note. At the engine: `auditT9_keys.rs`.)
    let odd = booked;
    assert!(odd.rows_bytes().len() == 420 && !odd.rows_bytes().windows(32).any(|x| x == &w.markets[0].committee_id[..]), "a row says no committee");
    assert_eq!(odd.rows[0].head(), RowHead::of(&raw));
    let mut dated = booked;
    dated.rows[0].key.day = 1;
    let dated = OLive { s: dated, ..base.clone() };
    let dn = OLive { s: Left { sid: dated.s.sid, legs: vec![leg], key: xk(&w.w), asked: 0 }, value: NOTE_MIN as u64, id: dated.id, op: op(75), daa: 6_050 };
    assert!(left_apply(&w.lib, &dated, &dn, &w.script(left_script_on(&(&dated).s, (&dn).s.legs.len())), w.payer(&w.f, &ff)).is_err() && dated.s.gone_at(&dn.s) == Some(0), "a row of the leg's market with a day is not the leg's row");
    let odd = OLive { s: odd, ..base.clone() };
    let note = Left { sid: odd.s.sid, legs: vec![leg], key: xk(&w.w), asked: 0 };
    let note_live = OLive { s: note.clone(), value: NOTE_MIN as u64, id: odd.id, op: op(75), daa: 6_050 };
    let (b, next, rest) = left_apply(&w.lib, &odd, &note_live, &w.script(left_script_on(&(&odd).s, (&note_live).s.legs.len())), w.payer(&w.f, &ff)).unwrap();
    signed("a note of a leg applied to a row whose head says another committee", &b, &w.keys);
    assert_eq!((next.rows, next.basis, rest), ([ShardRow::EMPTY; 4].into(), 0, None));
    // (And it is not spent as a note of no leg of the books.)
    let void = left_void(&w.lib, &odd, &note_live, 0, &w.script("KpfTLeftVoid"), w.payer(&w.f, &ff));
    assert!(void.is_err() || !accepted(&void.unwrap(), &w.keys), "a note of a leg of the books is spent");

    // Two rows of one market in a call's body.
    let shard = OLive { s: Shard { held: 20 * KAS1, ..booked }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let (_, call) = called_objs(&shard, [P10 * 13 / 10, 0, 0, 0], 6_600);
    let two = OLive { s: Called { heads: [w.head(0), w.head(0), RowHead::EMPTY, RowHead::EMPTY].into(), prices: [P10 * 13 / 10, P10 * 2, 0, 0].into(), ..call.s }, ..call.clone() };
    let a = cov(&w, &shard, &[leg], 40 * KAS1);
    let step_s = w.script("KpfTCStep");
    let (b, first) = call_step(&w.lib, &a, &a.s, None, &two, &step_s, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys));
    assert_eq!((first.at.clone(), first.note.asked), (vec![0], 30 * KAS1));
    let at20 = a.s.call_step(&Called { prices: [P10 * 2, P10 * 2, 0, 0].into(), ..two.s }, a.value as i64).unwrap();
    let second = CallStep { at: vec![1], ..at20 };
    assert_eq!(second.note.asked, 100 * KAS1);
    // (Review T9, docs 17.8d: ONE entry a market. The call's object is the list of its prices, and `Called::list`
    // gives a market of two rows one entry, the first row's: the object of `two` IS the call at 13 alone, and the
    // second row's price of 20 is nowhere in it. It was: "the step names either, and the note asks 30 KAS or 100
    // KAS as whoever steps chooses". Now whoever steps has one entry to show: the step at 20 is refused, named
    // at the first place and at the second. What review T3 found the change gave up is closed for a call whose
    // body is the model's; a list written by hand with one market twice is `auditT9_keys.rs`'s.)
    assert_eq!(two.s.list().iter().map(|e| (e.market, e.price)).collect::<Vec<_>>(), vec![(w.markets[0].id(), P10 * 13 / 10)]);
    assert_eq!(two.s.hash(), Called { heads: [w.head(0), RowHead::EMPTY, RowHead::EMPTY, RowHead::EMPTY].into(), prices: [P10 * 13 / 10, 0, 0, 0].into(), ..two.s }.hash(), "the second row of the market is not in the call's object");
    for at in [vec![1], vec![0]] {
        let b = call_step_as(&w.lib, &a, &a.s, None, &two, &CallStep { at, ..second.clone() }, &step_s, w.payer(&w.f, &ff));
        assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "the step at the second row's price of a market of one entry");
    }
    let b = call_step_as(&w.lib, &a, &a.s, None, &two, &CallStep { at: vec![1], ..first.clone() }, &step_s, w.payer(&w.f, &ff));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "the right step naming a place the list has not");
    let second = first.clone();
    // (Review T5, docs 17.8b: the call's object is the list of prices: a market's name, its limit, a day, the
    // price. It has NO scale: a call whose row says another scale is the same object, and the step is taken as
    // before. It was: "a row of the market's name and another scale or limit is no row of the leg: the step is
    // refused". An entry of another LIMIT is still no entry of the leg. What holds a leg's scale to its row is
    // now `KpfTEnterS` alone (one row a name, the leg's 18 bytes of scale and limit written in the head), and
    // the note's application (`KpfTLeft` finds the row by name, scale and limit): REVIEW-T5, R4.)
    let mut other_scale = two.clone();
    other_scale.s.heads[1].scale += 1;
    assert_eq!(other_scale.s.hash(), two.s.hash(), "the call's object does not say a scale");
    let b = call_step_as(&w.lib, &a, &a.s, None, &other_scale, &second, &step_s, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys), "the object is the same: the step is the one above");
    // (Review T9: the limit the object says for the market is its FIRST row's.)
    let mut other_limit = two.clone();
    other_limit.s.heads[1].price_max += 1;
    assert_eq!(other_limit.s.hash(), two.s.hash(), "the second row of a market is not in the call's object, whatever its limit");
    other_limit.s.heads[0].price_max += 1;
    assert_ne!(other_limit.s.hash(), two.s.hash());
    let b = call_step_as(&w.lib, &a, &a.s, None, &other_limit, &second, &step_s, w.payer(&w.f, &ff));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "an entry of another limit");
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
    // (Review T5, docs 17.8b: KpfTCStep 5,056 from 4,829: it reads a leg's price from the list of 56 bytes an
    // entry, and still hashes the leg's market to the name its row has. KpfTShardEnd 1,747 from 3,234 and
    // KpfTCallEnd 1,853 from 3,695: no loop over the rows. KpfTShardPrice 2,700 (2,697 before R1 of review T5 was closed) from 1,810: the list's order, and
    // the committee found in the row of the entry's name.)
    // (Review T7, docs 17.8c (b) and (c): KpfTCStep 5,439 from 5,056; KpfTLeft 2,916 from 2,506; KpfTLeftAll 5,093 from 4,310; KpfTLeftVoid 2,088 from 2,030; KpfTMarkNote 1,419 from 1,190; KpfTEnterS 2,982 from 2,484.)
    assert_eq!(sizes, vec![("KpfTCStep", 5319), ("KpfTEnterS", 3039), ("KpfTLeft", 1705), ("KpfTLeftAll", 4476), ("KpfTLeftVoid", 2110), ("KpfTStepPrice", 1838)]);
    let books: Vec<(&str, usize)> = ["KpfTShardOpen", "KpfTCallPay", "KpfTShardEnd", "KpfTShardPrice", "KpfTCallEnd"].iter().map(|n| (*n, kit.script(n).unwrap().len())).collect();
    assert_eq!(books, vec![("KpfTShardOpen", 789), ("KpfTCallPay", 2461), ("KpfTShardEnd", 1748), ("KpfTShardPrice", 2326), ("KpfTCallEnd", 1854)]);
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
    // (Review T9: the six legs are of two markets, and the books have two rows and the call two prices. Before
    // stage 1 a row's head held the 32 bytes where a covered leg says its time, and `booked` made the old legs
    // and the young ones of one market two rows: these books had FOUR rows and the call four prices, two markets
    // twice, an object no entry at the engine could make. The steps below were measured with those 224 bytes.)
    assert_eq!((shard.s.rows.iter().filter(|r| !r.no_market()).count(), call.s.list().len(), call.s.body().len()), (2, 2, 112));
    for (what, legs, coins) in [("one leg, a note's least", vec![legs6[0]], NOTE_MIN), ("one leg, 40 KAS", vec![legs6[0]], 40 * KAS1), ("six legs, three of them young, 40 KAS", legs6.clone(), 40 * KAS1)] {
        let a = cov(&w, &shard, &legs, coins);
        let (b, _) = call_step(&w.lib, &a, &a.s, None, &call, &w.script("KpfTCStep"), w.payer(&w.f, &ff)).unwrap();
        let sg = signed(&format!("a step under a call: {what}"), &b, &w.keys);
        steps.push((what, sg.mass.size, sg.mass.compute, sg.mass.storage));
    }
    println!("T3 B5: the scripts {sizes:?}; an entry stated: {} bytes (compute {}, storage {}); steps (what, bytes, compute, storage): {steps:?}", entry.mass.size, entry.mass.compute, entry.mass.storage);
    // (Review T5, docs 17.8b: the steps were 6,890, 6,942 and 7,538: the script is 227 bytes longer and the call's
    // object, of two prices here, 112 bytes where it was 368 and one push longer. 17.8b estimated 6,520 to 6,750.)
    // (Review T6, docs 17.8c: an entry stated was 5,040: `KpfTEnterS` is 21 bytes longer (it hashes the three
    // numbers of the claims) and is shown 5 bytes fewer beside the rows.)
    // (Review T7, docs 17.8c (b) and (c): an entry stated was 5,056: `KpfTEnterS` is 498 bytes longer (the gate).
    // The steps were 6,972, 7,024 and 7,620: `KpfTCStep` is 383 bytes longer (the band, the note of tag 9).)
    // (Review T9, docs 17.8d: an entry stated was 5,554: `KpfTEnterS` is 16 bytes less and is shown 12 bytes more
    // of rows. The steps were 7,355, 7,407 and 8,003: `KpfTCStep` is 120 bytes less (no hash a leg), and the
    // call's list is of two prices where the old measure had four (above): 112 bytes less that are the test's,
    // not the stage's.)
    // (An entry is 49 bytes more: `KpfTEnterS` makes no row for a market of which a number is nothing written below
    // zero. Review T9, Q1; docs 17.8e.)
    // (Docs 17.8h: an entry stated was 5,599: its statement says a head and a place, 40 bytes, and its row is 232
    // bytes where it was 120; `KpfTEnterS` is 2 bytes more. Docs 17.8i: 5,754 with a row of 232 bytes; of 256, 24 more.)
    assert_eq!((entry.mass.size, steps[0].1, steps[1].1, steps[2].1), (5_833, 7_123, 7_175, 7_771)); // (17.8j, A1: the root, 33 bytes, and 22 of script.)
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
        // (Review T7, docs 17.8c: L4 AS FOUND IS CLOSED. The steps' notes are of tag 9. The young user's says, beside
        // nothing asked, the 15 KAS less the band of 0.65 (the tolerance twice of 65): applied, a claim of the
        // second class of 14.35 KAS. The call does not end over it; it is paid whole from the time of the claims;
        // the shard is then open with 20 less the fee and the 14.35. This test asserted the hole: both notes
        // applied as an account's own of nothing asked, no claim, the call ended at once with 19.9 KAS held.)
        let young_u = if asks == 0 { 15 * KAS1 - 65 * KAS1 / 100 } else { 0 };
        assert_eq!((s_u.young, s_o.young), (young_u, 0), "{what}");
        let (b, sh1, c_u) = step_apply_t7(&w, &called, &sg_u, &s_u, 6_610, w.payer(&w.f, &ff));
        let sg1 = signed("L4: the user's note applied", &b, &w.keys);
        let sh1 = nth(&sg1, 0, sh1, 6_620);
        let (b, sh2, c_o) = step_apply_t7(&w, &sh1, &sg_o, &s_o, 6_610, w.payer(&w.f, &ff));
        let sh2 = nth(&signed("L4: the owner's note applied", &b, &w.keys), 0, sh2, 6_630);
        assert_eq!((c_o, sh2.s.rows, sh2.s.owed, sh2.s.claims.owed2, sh2.s.held), (None, [ShardRow::EMPTY; 4].into(), asks, young_u, 20 * KAS1 - fee), "{what}");
        let c_u = c_u.expect("the user's claim, of the first class or of the second");
        assert_eq!((c_u.amount1, c_u.amount2), (asks, young_u), "{what}");
        if asks == 0 {
            let fl = [coin(xk(&w.f), 36, sh2.s.claims_time().unwrap())];
            assert!(call_off(&w.lib, &sh2, &call, &pay_s, w.payer(&w.f, &fl)).is_err(), "L4 is open: the call ends over the young user's claim");
            let b = call_off_as(&w.lib, &sh2, &call, Shard { called: 0, owed: 0, claims: Claims::default(), ..sh2.s }, &pay_s, w.payer(&w.f, &fl));
            assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "L4 is open: the call ends over the young user's claim, at the engine");
            let (b, sh3, got, left) = claim_pay_c(&w.lib, &sh2, &nth(&sg1, 2, c_u, 6_620), &pay_s, w.payer(&w.f, &fl)).unwrap();
            let sg = signed("L4: the young user's claim paid", &b, &w.keys);
            assert!(got == young_u && left.is_none() && pays(&sg, &xk(&w.w), got + NOTE_MIN), "{what}: {got}");
            let sh3 = nth(&sg, 0, sh3, sh2.s.claims_time().unwrap() as u64 + 10);
            let (b, open) = call_off(&w.lib, &sh3, &call, &pay_s, w.payer(&w.f, &fl)).unwrap();
            signed("L4: the call ends, the young user paid", &b, &w.keys);
            println!("T3 L4, with the second class: a user's young leg gained 15 KAS (with no call its leaving is paid {paid} sompi); under the call the owner brought about it is paid {got} sompi, the same; the shard is then open and holds {}", open.held);
            assert_eq!((open.called, open.owed, open.claims, open.held, open.rows), (0, 0, Claims::default(), 20 * KAS1 - fee - young_u, [ShardRow::EMPTY; 4].into()), "L4 is open: the owner has its shard and the young gains after the call");
            assert_eq!(got, paid, "the user has under the call what its own leaving would have paid it");
            continue;
        }
        let c_u = Some(kpf_protocol::reserve::Rest { sid: c_u.sid, key: c_u.key, amount: c_u.amount1 });
        if let Some(claim) = c_u {
            // (Before the change: the user's claim, paid whole.)
            // (Review T6, docs 17.8c: from the time of the claims, which a coin that landed then shows.)
            let fl = [coin(xk(&w.f), 36, sh2.s.claims_time().unwrap())];
            let (b, _, got) = claim_pay(&w.lib, &sh2, &nth(&sg1, 2, claim, 6_620), &pay_s, w.payer(&w.f, &fl)).unwrap();
            let sg = signed("L4: the old leg's claim paid", &b, &w.keys);
            // (Review T6, second round, K1: with what the claim's note held, in one output.)
            assert!(got == 15 * KAS1 && pays(&sg, &xk(&w.w), got + NOTE_MIN), "{what}");
            continue;
        }
        unreachable!("each of the two plays ends above");
    }
    // Review T7, the gate (docs 17.8c, piece (c)). The books above are written by hand. At the engine the owner's
    // long of 1,000 units from 13 does not enter the shard of 20 KAS (the row with it asks 1,012.5375 KAS: 7.75 %
    // of 13,065): the play fails at its entry. Paid up to that, to the sompi, the leg enters, and the shard is
    // then NOT due for a call at 13 (its equity is above its lower level): the play does not
    // bring the call about. (How a call is then brought about, and what the user has: `auditT7_second.rs`.)
    let need = row_need(1_005, 1_005, p13[0]);
    assert_eq!(need, 10_125_375 * KAS1 / 10_000);
    assert!(!gate_takes(&w, &base, 20 * KAS1, alone, big, 0), "L4: the owner's 1,000 units enter a shard of 20 KAS");
    assert!(gate_takes(&w, &base, need, alone, big, 0) && !gate_takes(&w, &base, need - 1, alone, big, 0), "the gate, to the sompi");
    assert!(!Shard { held: need, ..with_own }.call_due(&p13), "paid up, the shard is due for a call with the owner's leg in");
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
    let b = c_enter_state_as(&w.lib, &acct, &shard, &enter, SEED, w.markets[0].committee_id, &lie, &said, &next, w.payer(&w.w, &f)).unwrap();
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
/// The 36 bytes a script that shows the rows is shown beside them: the sum of the gross (a push of eight bytes) and
/// the claims' three numbers, plain, 27 bytes (review T6, docs 17.8c: it was 41 bytes, the last 32 the hash of
/// the three numbers; the hash is now made by each script, of the 27 bytes it is shown).
fn head_of(gross: i64, claims: &[u8]) -> Vec<u8> {
    assert_eq!(claims.len(), 27);
    [&[0x08u8][..], &sm8(gross)[..], claims, &p33b(&Shard::root_opened())[..]].concat()
}
/// The claims' three numbers of nothing, 27 bytes.
fn no_claims() -> Vec<u8> {
    kpf_protocol::reserve::Claims::default().bytes()
}
/// The state's hash of the shard `s` with the 41 bytes `head` where its books' own would be, written out as the
/// scripts hash it (the reviewer's own: `Shard::hash` is asserted against it).
fn shard_hash_with(s: &Shard, head: &[u8]) -> [u8; 32] {
    // (The root of the dated rows' tree, its push, after the claims' numbers: docs 17.8j, 1.)
    let books = blake2b(&[&head[..9], &p33b(&blake2b(&s.rows_bytes()))[..], &head[36..], &p33b(&blake2b(&head[9..36]))[..]].concat());
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
    let (b, ..) = left_apply(&w.lib, &booked, &one, &w.script(left_script_on(&(&booked).s, (&one).s.legs.len())), w.payer(&w.f, &ff)).unwrap();
    v.push(("KpfTLeft.apply", b, 5, 0, booked.s, same()));
    let two = note_of(&booked, Left { sid: booked.s.sid, legs: entered(&[l0, l1, l0], 7), key: xk(&w.w), asked: 0 }, NOTE_MIN, 75);
    let (b, ..) = left_apply(&w.lib, &booked, &two, &w.script(left_script_on(&(&booked).s, (&two).s.legs.len())), w.payer(&w.f, &ff)).unwrap();
    v.push(("KpfTLeftAll.apply", b, 5, 0, booked.s, same()));
    // KpfTLeftVoid.void: a note of a leg of a market that has no row.
    let ghost = note_of(&booked, Left { sid: booked.s.sid, legs: entered(&[fleg(w, 2, 3, SHORT, P10)], 0), key: xk(&w.w), asked: 0 }, NOTE_MIN, 75);
    let b = left_void(&w.lib, &booked, &ghost, 0, &w.script("KpfTLeftVoid"), w.payer(&w.f, &ff)).unwrap();
    v.push(("KpfTLeftVoid.void", b, 5, 0, booked.s, same()));
    // KpfTLeft.rest: a note of the rest, of a shard that owes it and is not called.
    let owing = OLive { s: Shard { owed: 20 * KAS1, ..booked.s }, ..booked.clone() };
    let rest = Rest { sid: owing.s.sid, key: xk(&w.w), amount: 20 * KAS1 };
    let (b, ..) = rest_apply(&w.lib, &owing, &note_of(&owing, rest, rest.value(), 96), &w.script("KpfTRest"), w.payer(&w.f, &ff)).unwrap();
    v.push(("KpfTLeft.rest", b, 5, 0, owing.s, same()));
    // KpfTCallPay.claim and .off: a called shard of no row.
    let called = OLive { s: Shard { owed: 20 * KAS1, called: T0, ..base.s }, ..base.clone() };
    // (Review T6, docs 17.8c: a claim is paid from the time of the claims: a coin that landed then shows it.)
    let fl = [coin(xk(&w.f), 36, called.s.claims_time().unwrap())];
    let (b, ..) = claim_pay(&w.lib, &called, &note_of(&called, Rest { sid: called.s.sid, ..rest }, rest.value(), 96), &w.script("KpfTCallPay"), w.payer(&w.f, &fl)).unwrap();
    v.push(("KpfTCallPay.claim", b, 5, 0, called.s, same()));
    // (Review T6: and one of a shard whose three numbers are not nothing: 5 KAS of 20 paid to the first class, 8
    // owed to the second and 1 of them paid; a claim of 4 of the first class and 2 of the second, none paid.)
    let part = OLive { s: Shard { held: 50 * KAS1, owed: 20 * KAS1, called: T0, claims: kpf_protocol::reserve::Claims { paid1: 5 * KAS1, owed2: 8 * KAS1, paid2: KAS1 }, ..base.s }, ..base.clone() };
    let both = kpf_protocol::reserve::Claim { sid: part.s.sid, key: xk(&w.w), amount1: 4 * KAS1, paid1: 0, amount2: 2 * KAS1, paid2: 0 };
    let (b, _, paid, left) = claim_pay_c(&w.lib, &part, &note_of(&part, both, NOTE_MIN, 96), &w.script("KpfTCallPay"), w.payer(&w.f, &fl)).unwrap();
    assert_eq!((paid, left), (6 * KAS1, None));
    v.push(("KpfTCallPay.claim (numbers that are not nothing)", b, 5, 0, part.s, same()));
    let done = OLive { s: Shard { owed: 0, ..called.s }, ..called.clone() };
    let call = OLive { s: Called { sid: done.s.sid, time: T0, heads: [RowHead::EMPTY; 4].into(), prices: [0; 4].into() }, value: CALL_HOLD as u64, id: done.id, op: op(91), daa: 6_600 };
    let (b, _) = call_off(&w.lib, &done, &call, &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).unwrap();
    v.push(("KpfTCallPay.off", b, 5, 0, done.s, same()));
    // The answers of a row of a draw's trace and of a price of its list, and of a row of a call's: the object is
    // the disputed point of a statement on the shard. (Review T5, docs 17.8b: the two ENDS read no row and show
    // none: they are shown the books' hash, 32 bytes, and are no longer of this list: `ends_t5` below. The rows
    // are shown to the new `KpfTShardRow.answer`, a point 1 to 4, and to `KpfTShardPrice.answer`, a point from 5.)
    // (Review T9, docs 17.8d: a price's answer opens the shard's id by the seed of its opening and its committee:
    // the shard made by hand has the id that opening gives it.)
    let shard = w.shard();
    let shard = OLive { s: shard.s.seeded(&SEED, &w.markets[0].committee_id), ..shard };
    let s = shard.s;
    let prices = [P10, P10, 0, 0];
    let (_, st, d) = shard_draw(&w.lib, &shard, &w.script("KpfTChange"), KAS1, prices, T0, w.payer(&w.c, &fc)).unwrap();
    let live = w.stated(&shard, &st, &s.traced(&d, false).unwrap().said());
    let pointed_of = |st: Stated, point: i64| -> Box<dyn Fn([u8; 32]) -> [u8; 32]> { Box::new(move |h| Pointed { st: Stated { position: h, ..st }, disputer: xk(&kp(3)), point }.hash()) };
    let q = w.pointed(&live, 1);
    assert_eq!(pointed_of(st, 1)(s.hash()), q.s.hash());
    let (b, right) = shard_row(&w.lib, &q, &s, &d, 1, &w.script("KpfTShardRow"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    v.push(("KpfTShardRow.answer (a draw)", b, 5, 0, s, pointed_of(st, 1)));
    // (The price of row 1: the point 4 and its place in the list, from 1.)
    let p1 = 5 + s.traced(&d, false).unwrap().rows[0].at;
    let q = w.pointed(&live, p1);
    let c = w.cited(0, P10);
    let (b, right) = shard_price(&w.lib, &q, &s, SEED, &d, p1, &w.script("KpfTShardPrice"), Some((&w.script("KpfTCite"), &c)), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    v.push(("KpfTShardPrice.answer", b, 5, 0, s, pointed_of(st, p1)));
    let up = [P10 * 11 / 10, P10, 0, 0];
    let (_, st, d) = shard_call(&w.lib, &shard, &w.script("KpfTChange"), up, T0, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    let q = w.pointed(&w.stated(&shard, &st, &s.traced(&d, true).unwrap().said()), 2);
    let (b, right) = shard_row(&w.lib, &q, &s, &d, 2, &w.script("KpfTShardRow"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(right);
    v.push(("KpfTShardRow.answer (a call)", b, 5, 0, s, pointed_of(st, 2)));
    v
}

/// Review T5 (docs 17.8b): the two ends, which show no row: (what, the transaction, the shard, the object's hash
/// for a shard of a given state's hash). Argument 4 is the books' hash, 32 bytes.
#[allow(clippy::type_complexity)]
fn ends_t5(w: &World) -> Vec<(&'static str, OBuilt, Shard, Box<dyn Fn([u8; 32]) -> [u8; 32]>)> {
    let (f, ff, fc) = ([fund(xk(&w.w), 33)], [fund(xk(&w.f), 36)], [fund(xk(&w.c), 30)]);
    let shard = w.shard();
    let s = shard.s;
    let pointed_of = |st: Stated, point: i64| -> Box<dyn Fn([u8; 32]) -> [u8; 32]> { Box::new(move |h| Pointed { st: Stated { position: h, ..st }, disputer: xk(&kp(3)), point }.hash()) };
    let mut v: Vec<(&'static str, OBuilt, Shard, Box<dyn Fn([u8; 32]) -> [u8; 32]>)> = vec![];
    let (_, st, d) = shard_draw(&w.lib, &shard, &w.script("KpfTChange"), KAS1, [P10, P10, 0, 0], T0, w.payer(&w.c, &fc)).unwrap();
    let q = w.pointed(&w.stated(&shard, &st, &s.traced(&d, false).unwrap().said()), 0);
    let (b, right) = shard_end(&w.lib, &q, &s, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    v.push(("KpfTShardEnd.answer", b, s, pointed_of(st, 0)));
    let up = [P10 * 11 / 10, P10, 0, 0];
    let (_, st, d) = shard_call(&w.lib, &shard, &w.script("KpfTChange"), up, T0, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    let q = w.pointed(&w.stated(&shard, &st, &s.traced(&d, true).unwrap().said()), 0);
    let (b, right) = call_end(&w.lib, &q, &s, &d, &w.script("KpfTCallEnd"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(right);
    v.push(("KpfTCallEnd.answer", b, s, pointed_of(st, 0)));
    v
}

/// Review T5. The two ends carry the books' hash unopened (argument 4, 32 bytes, where they were shown the rows
/// and the 41 bytes): shown other 32 bytes (the hash of the rows alone, a bit, nothing, another shard's books),
/// refused: the statement's position is of the shard's own. The object made the disputed point of the same
/// statement on a shard of the OLD form (terms and the hash of the rows alone): refused with the books' hash
/// shown; with the hash of the rows shown where the books' hash is, the object opens (as for every script that
/// carries the 32 bytes unopened: the test below) and the same answer is then refused only because the outputs
/// stated are of the shard of this form (printed). No script of this library writes a shard of the old form
/// (`KpfTShardOpen`); it is said because the two ends were of the scripts that could not open one.
#[test]
fn t5_the_two_ends_hold_the_32_bytes_they_are_shown_to_the_statements_shard() {
    let w = world();
    let cases = ends_t5(&w);
    assert_eq!(cases.len(), 2);
    for (what, b, s, obj_of) in &cases {
        assert!(accepted(b, &w.keys), "{what}: the control");
        assert!(matches!(&args_of(&mut b.clone(), 1)[4], Arg::V(ArtifactValue::Bytes(x)) if x[..] == s.books_hash()[..]) && matches!(&args_of(&mut b.clone(), 1)[3], Arg::V(ArtifactValue::Bytes(x)) if x[..] == s.terms()[..]) && matches!(&args_of(&mut b.clone(), 1)[5], Arg::V(ArtifactValue::Bytes(x)) if x[..] == s.moving()[..]), "{what}: the books' hash is argument 4, between the terms and the moving part");
        let rows_alone = blake2b(&s.rows_bytes());
        let mut bit = s.books_hash();
        bit[0] ^= 1;
        let other = Shard { rows: [ShardRow::of(w.head(3), 0, 77), ShardRow::EMPTY, ShardRow::EMPTY, ShardRow::EMPTY].into(), ..*s }.books_hash();
        for h in [rows_alone, bit, [0u8; 32], other] {
            assert!(!accepted(&with_arg(b, 4, bytes_arg(&h)), &w.keys), "{what}: shown other 32 bytes than its shard's books' hash");
        }
        // The shard of the old form in the shard's place: refused with the books' hash; with the rows' hash shown
        // where the books' is, the object is opened: the end is then judged of the old form's shard.
        let mut t = b.clone();
        object_made(&mut t, &w.lib, 0, &obj_of(shard_hash_old(s)));
        assert!(!accepted(&t, &w.keys), "{what}: the shard of the form of before, the books' hash shown");
        let t = with_arg(&t, 4, bytes_arg(&rows_alone));
        let right_taken = accepted(&t, &w.keys);
        println!("T5: {what}: a statement on a shard of the old form, the end shown the rows' hash for the books': answered right, taken: {right_taken}");
        let mut t = b.clone();
        object_made(&mut t, &w.lib, 0, &obj_of(s.hash()));
        assert!(accepted(&t, &w.keys), "{what}: (control) the shard put in its own place");
    }
}

/// Second round, B (1) and (3). Each of the nine entries of the seven scripts that show a shard's rows (review T5,
/// docs 17.8b: it was ten of eight: the two ends show no row now, `KpfTShardRow.answer` is new and is here twice,
/// of a draw and of a call), in a
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
    assert_eq!(cases.len(), 11);
    let stray_account = cov(&w, &seeded(&w), &[], 20 * KAS1).s.hash();
    let stray_note = Rest { sid: [0x5d; 32], key: xk(&w.w), amount: KAS1 }.hash();
    assert_eq!(no_claims().len(), 27);
    let (mut refused, mut old) = (0, 0);
    for (what, b, at, obj, s, obj_of) in &cases {
        assert!(accepted(b, &w.keys), "{what}: the control");
        let head = s.books_head();
        // (Review T6, docs 17.8c: 36 bytes, the claims' three numbers plain, where it was 41 with their hash.)
        assert_eq!((head.len(), shard_hash_with(s, &head), units(s)), (69, s.hash(), s.gross_sum()), "{what}");
        assert!(matches!(&args_of(&mut b.clone(), 1)[*at], Arg::V(ArtifactValue::Bytes(x)) if x[..] == head[..]) && matches!(&args_of(&mut b.clone(), 1)[*at - 1], Arg::V(ArtifactValue::Bytes(x)) if x[..] == s.rows_bytes()[..]), "{what}: the 41 bytes are argument {at}, after the rows");
        let (g, claims) = (s.gross_sum(), s.claims.bytes());
        // (Review T6: where a hash of the claims was shown with a bit flipped or as nothing, the three numbers
        // are shown with a bit flipped, and each a sompi more; then a push's length changed, the 41 bytes of
        // the form of before, a byte fewer.)
        let mut flipped = claims.clone();
        flipped[26] ^= 1;
        let with = |d: [i64; 3]| -> Vec<u8> { kpf_protocol::reserve::Claims { paid1: s.claims.paid1 + d[0], owed2: s.claims.owed2 + d[1], paid2: s.claims.paid2 + d[2] }.bytes() };
        let mut tag = head.clone();
        tag[0] = 0x09;
        let mut heads = vec![head_of(g + 1, &claims), head_of(g - 1, &claims), head_of(i64::MAX, &claims), head_of(-g - 1, &claims), head_of(g, &flipped), head_of(g, &with([1, 0, 0])), tag, Shard { rows: [ShardRow::of(w.head(3), 0, 77), ShardRow::EMPTY, ShardRow::EMPTY, ShardRow::EMPTY].into(), ..*s }.books_head()];
        heads.push(head_of(g, &with([0, 1, 0])));
        heads.push(head_of(g, &with([0, 0, 1])));
        heads.push(head_of(g, &with([1, 1, 1])));
        for at in [9usize, 18, 27] {
            let mut h = head.clone();
            h[at] = 0x09;
            heads.push(h);
        }
        heads.push([&head[..9], &s.claims_hash()[..]].concat());
        heads.push(head[..35].to_vec());
        heads.push([&head[..], &[0u8][..]].concat());
        if g != 0 {
            heads.push(head_of(0, &claims));
        }
        for h in &heads {
            assert!(!accepted(&with_arg(b, *at, bytes_arg(h)), &w.keys), "{what}: shown other bytes than its shard's 36 ({:?})", &h[..9]);
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
    println!("T3 second round: nine entries that show the rows (ten transactions): {refused} transactions with other 41 bytes refused; {old} objects that are no shard of this form refused in the shard's place, each with six other 41 bytes too");
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
            t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &shard_hash_with(&next, &head_of(other, &next.claims.bytes())));
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
        // (Review T7: the step's note, of tag 9, as the step wrote it; its legs are old: a claim of the first class.)
        assert_eq!(left_script(a.s.legs.len()), script);
        let (b, next, claim) = step_apply_t7(&w, &called, &sg, &step, 6_810, w.payer(&w.f, &ff));
        let claim = claim.map(|c| { assert_eq!((c.amount2, c.paid1), (0, 0)); kpf_protocol::reserve::Rest { sid: c.sid, key: c.key, amount: c.amount1 } });
        let sg = signed(&format!("the note of {what} applied"), &b, &w.keys);
        assert_eq!((said(&next), units(&next)), (after * scale, after * scale), "{what}");
        for other in [after * scale + 1, after * scale - 1, 17 * scale] {
            let mut t = b.clone();
            t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_hash_with(&next, &head_of(other, &next.claims.bytes())));
            assert!(!accepted(&t, &w.keys), "{what}: the shard written with a sum of {other}");
        }
        if let Some(c) = claim {
            claims.push(nth(&sg, 2, c, 6_820));
        }
        called = nth(&sg, 0, next, 6_820);
    }
    assert_eq!((called.s.rows, said(&called.s), claims.len(), called.s.owed), ([ShardRow::EMPTY; 4].into(), 0, 1, 120 * KAS1));
    // (Review T6, docs 17.8c: a claim is paid from the time of the claims; the shard holds 99.9 KAS of the 120 it
    // owes: the claim is paid its part (all that is held, to the unit of a thousand sompi), what is owed stands,
    // and the call does NOT end (it ended here before, 20.1 KAS never paid). Paid up by its owner, the claim is
    // paid whole and the call ends: the sum is nothing all along.)
    let due = called.s.claims_time().unwrap();
    assert!(claim_pay(&w.lib, &called, &claims[0], &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).is_err(), "a claim is paid before the time of the claims");
    let fl = [coin(xk(&w.f), 36, due)];
    let c0 = OLive { s: claims[0].s.claim(), value: claims[0].value, id: claims[0].id, op: claims[0].op, daa: claims[0].daa };
    let (b, next, paid, left) = claim_pay_c(&w.lib, &called, &c0, &w.script("KpfTCallPay"), w.payer(&w.f, &fl)).unwrap();
    let sg = signed("the claim paid its part", &b, &w.keys);
    assert_eq!((paid, next.held, next.owed, next.claims.paid1, said(&next)), (100 * KAS1 - fee, 0, 120 * KAS1, 100 * KAS1 - fee, 0));
    let (called, c0) = (nth(&sg, 0, next, due as u64 + 10), nth(&sg, 2, left.expect("the claim again"), due as u64 + 10));
    assert!(call_off(&w.lib, &called, &call, &w.script("KpfTCallPay"), w.payer(&w.f, &fl)).is_err(), "the call ends while the claim is not whole");
    let fc = [fund(xk(&w.c), 38)];
    let (b, next) = shard_fund(&w.lib, &called, &w.script("KpfTAddC"), 20 * KAS1 + fee, w.payer(&w.c, &fc)).unwrap();
    let sg = signed("the owner pays its called shard up", &b, &w.keys);
    assert_eq!(said(&next), 0);
    let called = nth(&sg, 0, next, due as u64 + 20);
    let (b, next, paid, left) = claim_pay_c(&w.lib, &called, &c0, &w.script("KpfTCallPay"), w.payer(&w.f, &fl)).unwrap();
    let sg = signed("the claim paid whole", &b, &w.keys);
    assert_eq!((paid, left, next.held, next.owed, next.claims.paid1, said(&next)), (20 * KAS1 + fee, None, 0, 120 * KAS1, 120 * KAS1, 0));
    let (b, open) = call_off(&w.lib, &nth(&sg, 0, next, due as u64 + 30), &call, &w.script("KpfTCallPay"), w.payer(&w.f, &fl)).unwrap();
    signed("the call ends", &b, &w.keys);
    assert_eq!((open.called, open.owed, open.claims, said(&open), open.rows), (0, 0, kpf_protocol::reserve::Claims::default(), 0, [ShardRow::EMPTY; 4].into()));
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
    // (Review T7, docs 17.8c, the gate: `KpfTEnterS` computes the worth of the row's gross units at the leg's own
    // entry. A row of the largest gross at an entry of 10 KAS a unit is worth more than 64 bits hold: such a row
    // takes no leg at that entry any more, by the model and at the engine (run below; finding S5 of review T7,
    // low). The last unit of this test enters at a sompi a unit, where the row's worth is counted: the test is of
    // the SUM of the gross at its end, not of a price.)
    let last = fleg(&w, 3, 1, LONG, 1);
    let at10 = fleg(&w, 3, 1, LONG, P10);
    let full = |i: usize, gross: i64| ShardRow::of(w.head(i), 0, gross);
    let top = OLive { s: Shard { rows: [full(0, MARGIN_MAX), full(1, MARGIN_MAX), full(2, MARGIN_MAX), full(3, MARGIN_MAX - scale)].into(), ..base.s }, ..base.clone() };
    assert_eq!((top.s.gross_sum(), units(&top.s)), (4 * MARGIN_MAX - scale, 4 * MARGIN_MAX - scale));
    assert!(4 * (MARGIN_MAX as i128) < i64::MAX as i128);
    let acct = cov(&w, &top, &[], 20 * KAS1);
    let (b, _, _, _, _, next) = c_enter_state(&w.lib, &acct, &top, &w.script("KpfTEnterS"), SEED, last, 0, &[], T0, w.payer(&w.w, &f)).unwrap();
    signed("the last unit four full rows take", &b, &w.keys);
    assert_eq!((next.gross_sum(), next.rows[3].gross), (4 * MARGIN_MAX, MARGIN_MAX));
    // (The same unit at 10 KAS: refused by the gate's arithmetic alone, by the model and at the engine, whatever
    // the shard holds. Before the gate it was taken: this test entered it.)
    {
        let rich = OLive { s: Shard { held: i64::MAX / 4, ..top.s }, ..top.clone() };
        let e = rich.s.entered(&at10, 3, T0).unwrap_err().to_string();
        assert!(e.contains("its row's requirement is beyond what is counted"), "{e}");
        let (st, said, _, _) = acct.s.c_enter(at10, 0, 3, &[], T0, acct.value as i64).unwrap();
        let mut would = next;
        would.basis = top.s.basis - P10;
        let b = c_enter_state_as(&w.lib, &acct, &rich, &w.script("KpfTEnterS"), SEED, w.markets[0].committee_id, &st, &said, &Shard { held: rich.s.held, ..would }, w.payer(&w.w, &f)).unwrap();
        assert!(!accepted(&b, &w.keys), "a row whose gross is worth more than 64 bits at the leg's entry takes the leg");
    }
    let brim = OLive { s: next, ..top.clone() };
    for i in 0..4 {
        // (At a sompi a unit, so that what refuses is the bound of the gross and not the gate's arithmetic.)
        let e = c_enter_state(&w.lib, &acct, &brim, &w.script("KpfTEnterS"), SEED, fleg(&w, i, 1, LONG, 1), 0, &[], T0 + 100, w.payer(&w.w, &[coin(xk(&w.w), 33, T0 + 100)]));
        assert!(e.is_err_and(|e| e.to_string().contains("its books are beyond what is counted")), "a unit more into row {i}");
    }
    let n = note_of(&brim, Left { sid: brim.s.sid, legs: entered(&[last], 0), key: xk(&w.w), asked: 0 }, NOTE_MIN, 75);
    let (b, after, _) = left_apply(&w.lib, &brim, &n, &w.script(left_script_on(&(&brim).s, (&n).s.legs.len())), w.payer(&w.f, &ff)).unwrap();
    signed("a leg out of the fullest books", &b, &w.keys);
    assert_eq!(after.gross_sum(), 4 * MARGIN_MAX - scale);

    // Objects no script writes: books that say another sum than their rows have.
    // (a) A called shard of no row, its books saying 5: its call is ended, and it goes on saying 5.
    let done = OLive { s: Shard { owed: 0, called: T0, ..base.s }, ..base.clone() };
    let call = OLive { s: Called { sid: done.s.sid, time: T0, heads: [RowHead::EMPTY; 4].into(), prices: [0; 4].into() }, value: CALL_HOLD as u64, id: done.id, op: op(91), daa: 6_600 };
    let (b, open) = call_off(&w.lib, &done, &call, &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys));
    let five = head_of(5, &done.s.claims.bytes());
    let mut t = with_arg(&b, 5, bytes_arg(&five));
    object_made(&mut t, &w.lib, 0, &shard_hash_with(&done.s, &five));
    assert!(!accepted(&t, &w.keys), "(the output is still the model's)");
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_hash_with(&open, &five));
    let off_with_a_sum = accepted(&t, &w.keys);
    // (b) A row of 10 units, the books saying 3: the note of the 10 is applied, and the books say 7 below nothing.
    let l = fleg(&w, 0, 10, LONG, P10);
    let ten = OLive { s: base.s.booked(&l).unwrap(), ..base.clone() };
    let n = note_of(&ten, Left { sid: ten.s.sid, legs: entered(&[l], 0), key: xk(&w.w), asked: 0 }, NOTE_MIN, 75);
    let (b, after, _) = left_apply(&w.lib, &ten, &n, &w.script(left_script_on(&(&ten).s, (&n).s.legs.len())), w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys));
    let three = head_of(3 * scale, &ten.s.claims.bytes());
    let mut t = with_arg(&b, 5, bytes_arg(&three));
    object_made(&mut t, &w.lib, 0, &shard_hash_with(&ten.s, &three));
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_hash_with(&after, &head_of(-7 * scale, &ten.s.claims.bytes())));
    let below_nothing = accepted(&t, &w.keys);
    println!("T3 second round: of objects no script writes: a call ended on books of no row that say a sum of 5: {off_with_a_sum}; a note of 10 units applied to books that say 3, which then say 7 below nothing: {below_nothing}");
    // (Review T6, docs 17.8c and 17.8a, 3: `KpfTCallPay.off` and `.claim` read the sum beside the four rows now: a
    // call is NOT ended on books of no row that say a sum of 5 (it was). `KpfTLeft.apply` compares nothing still.)
    assert!(!off_with_a_sum && below_nothing, "the call's end reads the sum; no other script compares the sum it is shown with the rows (then say so in 17.8a)");
    // (And the same for a claim: a called shard of no row that says a sum of 5 pays no claim.)
    {
        let owing = OLive { s: Shard { owed: 20 * KAS1, called: T0, ..base.s }, ..base.clone() };
        let fl = [coin(xk(&w.f), 36, owing.s.claims_time().unwrap())];
        let rest = Rest { sid: owing.s.sid, key: xk(&w.w), amount: 20 * KAS1 };
        let (b, next, _) = claim_pay(&w.lib, &owing, &note_of(&owing, rest, NOTE_MIN, 96), &w.script("KpfTCallPay"), w.payer(&w.f, &fl)).unwrap();
        assert!(accepted(&b, &w.keys));
        let five = head_of(5, &owing.s.claims.bytes());
        let mut t = with_arg(&b, 5, bytes_arg(&five));
        object_made(&mut t, &w.lib, 0, &shard_hash_with(&owing.s, &five));
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_hash_with(&next, &head_of(5, &next.claims.bytes())));
        assert!(!accepted(&t, &w.keys), "a claim is paid by a shard of no row whose books say a sum of 5");
        // (The control of the way it is written: the same with the sum of nothing is the builder's own.)
        let zero = head_of(0, &owing.s.claims.bytes());
        let mut t = with_arg(&b, 5, bytes_arg(&zero));
        object_made(&mut t, &w.lib, 0, &shard_hash_with(&owing.s, &zero));
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_hash_with(&next, &head_of(0, &next.claims.bytes())));
        assert!(accepted(&t, &w.keys));
    }
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
            // (Whoever hashes the rows hashes the 36 bytes with them, every time. Review T6, docs 17.8c: the last
            // 27 hashed by the script; `KpfTCallPay` writes the shard with the 36 bytes it made, `ng`.)
            assert_eq!(code.matches("blake2b(rw)").count(), code.matches("gb.slice(0, 9) + p33(blake2b(rw)) + gb.slice(36, 69) + p33(blake2b(gb.slice(9, 36)))").count() + code.matches("ng.slice(0, 9) + p33(blake2b(rw)) + ng.slice(36, 69) + p33(blake2b(ng.slice(9, 36)))").count() + code.matches("gb.slice(0, 9) + p33(blake2b(rw)) + p33(byte[32](r1.slice(0, 32))) + p33(blake2b(gb.slice(9, 36)))").count(), "{}", p.display()); // (17.8j, A2: `KpfTRowIn` and `KpfTRowOut` open the root by a path and hash it where the others are shown its push.)
            assert_eq!((code.matches("gb.slice(9, 41)").count(), code.matches("byte[41] gbook").count()), (0, 0), "{}", p.display());
            assert_eq!(code.matches("ng.slice(0, 9)").count() > 0, p.file_stem().unwrap() == "KpfTCallPay", "{}", p.display());
            assert_eq!(code.matches("blake2b(rows)").count(), 0, "{}", p.display());
        }
    }
    opens.sort();
    // (Review T5, docs 17.8b: `KpfTShardEnd` and `KpfTCallEnd` read no row and left this list; `KpfTShardRow` is new.)
    // (17.8e: `KpfTRest`, which was `KpfTLeft.rest`, is a script of its own.)
    assert_eq!(opens, vec!["KpfTCallPay", "KpfTEnterS", "KpfTLeft", "KpfTLeftAll", "KpfTLeftAllC", "KpfTLeftC", "KpfTLeftVoid", "KpfTRest", "KpfTRowIn", "KpfTRowOut", "KpfTShardPrice", "KpfTShardRow"]);
}
