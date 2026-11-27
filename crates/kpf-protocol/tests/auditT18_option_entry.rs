//! Review T18: an option enters and leaves a shard's books, at the engine (docs/ARCHITECTURE-CLEARING.md 17.8j,
//! steps B1 to B3). The reviewer's tests; its report is docs/audit/REVIEW-T18-option-entry.md. The world and its
//! helpers are `thin_draw.rs`'s and `auditT17_dated_rows.rs`'s, copied.
//!
//! A test named `t18_uN_...` shows the finding UN as it stands: it passes while the finding is open. A test named
//! `t18_held_...` is of what was attacked and held.
#![allow(dead_code, unused_imports, unused_variables, clippy::too_many_arguments, clippy::type_complexity)]

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint, TransactionOutput};
use kaspa_consensus_core::Hash;
use kpf_protocol::books::{Books, Key, Row, OF_CALLS, OF_DAY, OF_PUTS};
use kpf_protocol::cite;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::covered::{leg_entered, leg_more, CEnterSaid, Marked, Unentered};
use kpf_protocol::cross::{XLeg, XTerms};
use kpf_protocol::oracle_build::{OBuilt, OLive, OPlan, OSigned};
use kpf_protocol::reserve::{Draw, Left, Rest, RowHead, RowOutBy, Shard, ShardRow, NOTE_LEFT, NOTE_MIN, SHARD_CARRIER};
use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::{Account, Out, Outs, Pointed, Stated};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{p2pk_spk, Arg, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use silverscript_abi::ArtifactValue;
use std::cell::RefCell;

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;
const P10: i64 = 1_000_000_000;
const SEED: [u8; 32] = [0x5e; 32];
const BPS: i64 = 10_000;
const DAY: i64 = T0 + 1_000_000;

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
        RowHead { key: Key::futures(m.id()), scale: m.p.scale, price_max: m.p.price_max }
    }
    /// A shard of 100 KAS with no leg and no dated row, whose id is what an entry holds it to.
    fn bare(&self) -> OLive<Shard> {
        let s = Shard { t: self.terms(), rows: [ShardRow::EMPTY; 4].into(), owner: xk(&self.c), held: 100 * KAS1, expiry: 0, dispute_daa: 50, bond_bps: 1_000, sid: [0x5d; 32], basis: 0, called: 0, owed: 0, claims: Default::default(), root: Shard::root_opened(), dgross: 0 }.seeded(&SEED, &self.markets[0].committee_id);
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
    fn pointed(&self, st: &OLive<Stated>, point: i64) -> OLive<Pointed> {
        let f = [fund(xk(&self.w), 31)];
        let (b, q) = point_dispute(&self.lib, st, &self.script("KpfTPoint"), xk(&self.w), point, self.payer(&self.w, &f)).unwrap();
        let s = b.sign(&self.keys).expect("pointed");
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok(), "the point {point} is disputed");
        nth(&s, 0, q, 6_520)
    }
}

/// The world of `thin_draw.rs`, with a library of `copies` of each script, each holding `value`.
fn world_of(copies: usize, value: u64) -> World {
    let (c, w, f) = (kp(2), kp(3), kp(9));
    let members = [kp(101), kp(102), kp(103)];
    let mut keys = KeySet::new();
    for k in [&c, &w, &f] {
        keys.add_keypair(*k);
    }
    let rid = Hash::from_bytes([0x52; 32]);
    let rp = CommitteeParams::testnet();
    let mut markets: Vec<CMarket> = (0..4u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
    markets.sort_by_key(|m| Key::futures(m.id()).numbers());
    let mut table = EMPTY_TABLE;
    for (i, wt) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: wt * KAS1 };
    }
    let table = OLive { s: EpochTable { members: table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(20), daa: 6_000 };
    let funds = vec![KeyUtxo { outpoint: op(200), amount: 100_000 * KAS1 as u64, daa: T0 as u64, key: xk(&f) }];
    let (g, lib) = lib_genesis(copies, value, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    World { c, w, f, members, keys, rid, markets, table, lib, scripts: lib_scripts(&gs.tx, 6_000), chain: RefCell::new(vec![]) }
}
fn world() -> World {
    world_of(1, 10 * KAS1 as u64)
}

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
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below(hi - lo + 1)
    }
}

fn sm8(v: i64) -> [u8; 8] {
    let mut b = v.unsigned_abs().to_le_bytes();
    if v < 0 {
        b[7] |= 0x80;
    }
    b
}
fn int_arg(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}
fn bytes_arg(b: &[u8]) -> Arg {
    Arg::V(ArtifactValue::Bytes(b.to_vec()))
}
/// The arguments of the contract input `i` (input 1 is the script's copy in every transaction of these tests).
fn args_of(b: &mut OBuilt, i: usize) -> &mut Vec<Arg> {
    let OPlan::Contract { args, .. } = &mut b.plans[i] else { panic!("input {i} is not a contract input") };
    args
}
/// Input `i` of a built transaction made the object whose state hashes to `h`.
fn object_made(b: &mut OBuilt, lib: &[u8; 32], i: usize, h: &[u8; 32]) {
    b.entries[i].script_public_key = hash_spk(lib, h);
    let OPlan::Contract { redeem, .. } = &mut b.plans[i] else { panic!("input {i} is not an object") };
    *redeem = kpf_protocol::thin_artifacts::kit().object(lib, h);
}
fn leaf138(r: &Row) -> Vec<u8> {
    r.bytes()[9..].to_vec()
}
fn live(base: &OLive<Shard>, s: Shard) -> OLive<Shard> {
    OLive { s, value: (s.held + SHARD_CARRIER) as u64, ..base.clone() }
}
fn pays(sg: &OSigned, key: &[u8; 32], value: i64) -> bool {
    sg.tx.outputs.iter().any(|o| o.script_public_key == p2pk_spk(key) && o.value == value as u64)
}
fn paid_to(sg: &OSigned, key: &[u8; 32]) -> u64 {
    sg.tx.outputs.iter().filter(|o| o.script_public_key == p2pk_spk(key)).map(|o| o.value).sum()
}

fn fleg(w: &World, i: usize, units: i64, dir: i64, entry: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir, entry }
}
/// An option leg of market `i` as an entry is given it: `way` 3 a call held, 4 a call written, 5 a put held, 6 a
/// put written.
fn oleg(w: &World, i: usize, way: i64, units: i64, strike: i64, entry: i64, day: i64, premium: i64) -> XLeg {
    XLeg { dir: way, committee: leg_more(6_000, day, strike, premium), ..fleg(w, i, units, LONG, entry) }
}
fn cov(w: &World, shard: &OLive<Shard>, legs: &[XLeg], coins: i64) -> OLive<Account> {
    let s = Account { t: w.terms(), legs: legs.to_vec(), a_key: xk(&w.w), b_key: xk(&w.c), a_margin: coins, b_margin: 0, expiry: 0, dispute_daa: 50, bond_bps: 1_000, a_min_counter: legs.len() as i64, b_min_counter: 0, shard: Some(shard.s.sid) };
    OLive { s, value: coins as u64, id: shard.id, op: op(80), daa: 6_100 }
}
fn left_note(shard: &OLive<Shard>, n: &Left, k: u8) -> OLive<Left> {
    OLive { s: n.clone(), value: NOTE_MIN as u64, id: shard.id, op: op(60 + k), daa: 6_200 }
}
fn key_of(w: &World, calls: bool, strike: i64, day: i64) -> Key {
    Key { market: w.head(0).key.market, day, of: if calls { OF_CALLS } else { OF_PUTS }, strike }
}
/// The series `k` listed in the books of the shard (by the model: `KpfTRowIn` is review T17's).
fn listed(w: &World, s: Shard, b: &Books, k: Key) -> (Shard, Books) {
    let h = w.head(0);
    let free: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_none()).collect();
    let stands = b.row(&Key::day_of(k.market, k.day)).is_some();
    s.row_in(b, k, h.scale, h.price_max, 0, free[0], (!stands).then_some(free[1])).expect("a series is listed")
}
/// The row of `k` written by hand (units of legs that entered before), the shard's root, sum and number with it.
fn row_set(s: Shard, b: &Books, k: &Key, net: i64, gross: i64, entered: i64) -> (Shard, Books) {
    let mut b = b.clone();
    let at = b.place_of(k).expect("the row");
    let r = b.leaves[at].expect("a row");
    b.leaves[at] = Some(Row { net, gross, entered, ..r });
    (Shard { root: b.root(), dgross: s.dgross - r.gross + gross, basis: s.basis - r.entered + entered, ..s }, b)
}
fn rate_up(w: i64, r: i64) -> i64 {
    (w / BPS) * r + ((w % BPS) * r + BPS - 1) / BPS
}
/// What a dated row is asked alone at the price `e`, at the upper rate, with the rate on its gross: the row's gate
/// (docs 17.8j, 7), written here from the paper (`Shard::dated_gate` is not public).
fn gate_of(s: &Shard, row: &Row, e: i64) -> i64 {
    let (_, wg, asked) = s.dated_adds(row, e, false).expect("the row's sums");
    asked + rate_up(wg, kpf_protocol::reserve::gross_rate(&s.t) + s.t.g_bps)
}
/// The shard and its books with the option leg `l` in its row, whatever the gate says: what `KpfTEnterSD` would
/// write were the gate not there.
fn as_if_entered(s: &Shard, books: &Books, l: &XLeg, t0: i64) -> (Shard, Books) {
    let (head, units, basis) = Shard::book_of_option(l).expect("an option leg");
    let nb = books.moved(&head.key, units, l.size, basis).expect("the row takes the leg");
    (Shard { root: nb.root(), dgross: s.dgross + l.size, basis: s.basis + basis, expiry: t0 + s.t.late_daa + 2 * s.dispute_daa, ..*s }, nb)
}
/// The entry `b` (built by `c_enter_state_d_as` of the statement `st`) saying the leg `leg` in place of its own:
/// what is said, the hash of what is said, and the marked account the statement stands on, all with that leg. So
/// a refusal is of the leg, and of nothing the test left out.
fn restated(w: &World, b: &OBuilt, st: &Stated, said: &CEnterSaid, marked: &Marked, leg: &XLeg) -> OBuilt {
    let mut t = b.clone();
    let said2 = CEnterSaid { leg: *leg, ..said.clone() };
    let mut m2 = marked.clone();
    let last = m2.0.legs.len() - 1;
    m2.0.legs[last] = *leg;
    let want = said.said();
    let at = args_of(&mut t, 1).iter().position(|a| matches!(a, Arg::V(ArtifactValue::Bytes(x)) if *x == want)).expect("what is said, among the arguments");
    args_of(&mut t, 1)[at] = bytes_arg(&said2.said());
    let st2 = Stated { position: m2.hash(), arg: said2.arg(), ..*st };
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &st2.hash());
    t
}

// ================================================================================================================
// U1: KpfTLeftVoid spends every note that has an option leg.

/// A call held enters by `KpfTEnterSD` (at the engine). Its note asks its shard 3 KAS (the leg gained): the rule
/// (`left_d`) pays its user and takes the leg out of its row. `KpfTLeftVoid` spends the same note, by anyone: the
/// shard goes on as it is, the leg in its row, and nothing is paid. The same of the note that the leg did not enter,
/// and of a note of a futures leg and an option leg (the futures leg stays in its row too).
#[test]
fn t18_u1_a_note_with_an_option_leg_is_spent_by_anyone_and_its_legs_stay_in_the_books() {
    let w = world();
    let h = w.head(0);
    let base = w.bare();
    let committee = w.markets[0].committee_id;
    let k11 = key_of(&w, true, P10 / 10 * 11, DAY);
    let (s1, b1) = listed(&w, base.s, &Books::opened(), k11);
    let shard = live(&base, s1);
    let f = [fund(xk(&w.w), 33)];
    let ff = [fund(xk(&w.f), 36)];
    let (enter, void, left_d, all_d) = (w.script("KpfTEnterSD"), w.script("KpfTLeftVoid"), w.script("KpfTLeftD"), w.script("KpfTLeftAllD"));
    let acct = cov(&w, &shard, &[], 20 * KAS1);
    let held = oleg(&w, 0, 3, 10, P10 / 10 * 11, P10, DAY, KAS1 / 2);
    let (b, _, _, marked, _, next, b2) = c_enter_state_d(&w.lib, &acct, &shard, &b1, &enter, SEED, committee, held, 0, &[], T0, w.payer(&w.w, &f)).unwrap();
    signed("a held call enters", &b, &w.keys);
    let with = live(&base, next);
    let leg = leg_entered(&held, T0);
    assert_eq!(b2.row(&k11).unwrap().gross, 10 * h.scale);

    // The note that the leg left, asking 3 KAS for its user.
    let n = Left { sid: next.sid, legs: vec![leg], key: xk(&w.w), asked: 3 * KAS1 };
    let note = left_note(&with, &n, 1);
    let (ok, after, books, _) = left_apply_d(&w.lib, &with, &b2, &note, &left_d, w.payer(&w.f, &ff)).unwrap();
    let paid = signed("the rule: the note applied", &ok, &w.keys);
    assert!(pays(&paid, &xk(&w.w), 3 * KAS1) && books.row(&k11).unwrap().gross == 0 && after.held == next.held - 3 * KAS1, "the rule pays the user 3 KAS and takes the leg out");
    assert_eq!(with.s.gone_at(&n), Some(0), "U1, the model: an option leg is `no leg of these books`");
    let v = left_void(&w.lib, &with, &note, 0, &void, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("U1: the same note spent by KpfTLeftVoid", &v, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (with.value, &hash_spk(&w.lib, &with.s.hash())), "U1 is open: the shard goes on as it is, the leg in its row");
    assert_eq!(paid_to(&sg, &xk(&w.w)), 0, "U1 is open: the user is paid nothing, and no note of the rest is written");
    assert_eq!(sg.tx.outputs.iter().filter(|o| o.covenant.is_some_and(|c| Some(c.covenant_id) == with.id)).count(), 1, "U1 is open: the shard alone goes on under its id");

    // The note that the leg did not enter (its entry ended, undone by KpfTEnterOff).
    let m = OLive { s: marked.clone(), value: marked.0.a_margin as u64, id: acct.id, op: op(90), daa: 6_700 };
    let (bo, _, un) = enter_off(&w.lib, &m, &w.script("KpfTEnterOff"), w.payer(&w.f, &ff)).unwrap();
    let so = signed("the entry undone", &bo, &w.keys);
    let unote = nth(&so, 2, un, 6_710);
    assert!(unentered_apply_d(&w.lib, &with, &b2, &unote, &left_d, w.payer(&w.f, &ff)).is_ok_and(|(x, _, _)| accepted(&x, &w.keys)), "the rule: the leg is taken out");
    let v = unentered_void(&w.lib, &with, &unote, &void, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("U1: the note that an option did not enter spent by KpfTLeftVoid", &v, &w.keys);
    assert_eq!(&sg.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &with.s.hash()), "U1 is open: a leg that never entered stays in its row");

    // A note of a long and the call (an account taken whole), asking 7 KAS: voided by naming the option leg.
    let long = leg_entered(&fleg(&w, 0, 10, LONG, P10), T0);
    let sf = Shard { expiry: 0, ..next }.entered(&long, 0, T0).unwrap();
    let both_in = live(&base, sf);
    let n2 = Left { sid: sf.sid, legs: vec![long, leg], key: xk(&w.w), asked: 7 * KAS1 };
    let note2 = left_note(&both_in, &n2, 2);
    let (ok, after, _, _) = left_apply_d(&w.lib, &both_in, &b2, &note2, &all_d, w.payer(&w.f, &ff)).unwrap();
    let paid = signed("the rule: a note of two applied", &ok, &w.keys);
    assert!(pays(&paid, &xk(&w.w), 7 * KAS1) && after.rows[0].gross == 0 && after.dgross == 0);
    // (Named by its futures leg it is void as well: the units of the note's legs "of the named leg's row" count the
    // option leg's ten, which are of its market and scale and in no futures row, against the row's ten.)
    assert_eq!(both_in.s.gone_at(&n2), Some(0), "U1, the model: the futures leg of a note with an option of its market is `gone`");
    assert!(left_void(&w.lib, &both_in, &note2, 0, &void, w.payer(&w.f, &ff)).is_ok_and(|x| accepted(&x, &w.keys)), "U1 is open: named by its futures leg, which is in the books, the note is void too");
    let v = left_void(&w.lib, &both_in, &note2, 1, &void, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("U1: a note of a futures leg and an option leg spent by KpfTLeftVoid", &v, &w.keys);
    assert_eq!((&sg.tx.outputs[0].script_public_key, paid_to(&sg, &xk(&w.w))), (&hash_spk(&w.lib, &both_in.s.hash()), 0), "U1 is open: both legs stay in the books and the 7 KAS are paid to nobody");
}

// ================================================================================================================
// U2: KpfTEnterOff names the output that returns the premium freely; two of them are met by one output.

/// Two entries of a written call (20 KAS of premium each), into two accounts under two shards of one owner, ended.
/// Each undone alone returns 20 KAS to the owner. Undone in one transaction (a library of two copies of each
/// script), both scripts name one output: the owner has 20 KAS and whoever built the transaction the other 20.
#[test]
fn t18_u2_two_entries_undone_in_one_transaction_return_one_premium() {
    let w = world_of(2, 20 * KAS1 as u64);
    let offs: Vec<LibScript> = w.scripts.iter().filter(|s| s.s == "KpfTEnterOff").cloned().collect();
    assert_eq!(offs.len(), 2, "two copies of the script");
    let base = w.bare();
    let written = leg_entered(&oleg(&w, 0, 4, 10, P10 / 10 * 9, P10, DAY, 2 * KAS1), T0);
    let acct = cov(&w, &base, &[], 40 * KAS1);
    let m1 = acct.s.c_marked(&written, 20 * KAS1, T0).expect("a marked account");
    let m2 = Account { shard: Some([0x6d; 32]), ..acct.s.clone() }.c_marked(&written, 20 * KAS1, T0).expect("a marked account under another shard");
    let l1 = OLive { s: m1.clone(), value: m1.0.a_margin as u64, id: Some(Hash::from_bytes([0x73; 32])), op: op(90), daa: 6_700 };
    let l2 = OLive { s: m2.clone(), value: m2.0.a_margin as u64, id: Some(Hash::from_bytes([0x74; 32])), op: op(91), daa: 6_700 };
    let ff = [fund(xk(&w.f), 36)];
    let (a, _, _) = enter_off(&w.lib, &l1, &offs[0], w.payer(&w.f, &ff)).unwrap();
    let (b, _, _) = enter_off(&w.lib, &l2, &offs[1], w.payer(&w.f, &ff)).unwrap();
    let (sa, sb) = (signed("the first undone alone", &a, &w.keys), signed("the second undone alone", &b, &w.keys));
    assert_eq!((paid_to(&sa, &xk(&w.c)), paid_to(&sb, &xk(&w.c))), (20 * KAS1 as u64, 20 * KAS1 as u64), "alone, each returns its premium");
    assert_eq!((m1.undone_back().unwrap().3, m2.undone_back().unwrap().3), (20 * KAS1, 20 * KAS1), "the model: 40 KAS return");
    // Both in one transaction: inputs the first account, its script, the second account, its script, the funding;
    // outputs the two accounts and scripts at their inputs' places, the two notes, ONE output to the owner, the change.
    let bind = |o: &TransactionOutput, i: u16| {
        let mut o = o.clone();
        if let Some(c) = o.covenant.as_mut() {
            c.authorizing_input = i;
        }
        o
    };
    let mut m = a.clone();
    m.tx.inputs = vec![a.tx.inputs[0].clone(), a.tx.inputs[1].clone(), b.tx.inputs[0].clone(), b.tx.inputs[1].clone(), a.tx.inputs[2].clone()];
    m.entries = vec![a.entries[0].clone(), a.entries[1].clone(), b.entries[0].clone(), b.entries[1].clone(), a.entries[2].clone()];
    m.plans = vec![a.plans[0].clone(), a.plans[1].clone(), b.plans[0].clone(), b.plans[1].clone(), a.plans[2].clone()];
    m.input_labels = vec![a.input_labels[0].clone(), a.input_labels[1].clone(), b.input_labels[0].clone(), b.input_labels[1].clone(), a.input_labels[2].clone()];
    m.tx.outputs = vec![a.tx.outputs[0].clone(), a.tx.outputs[1].clone(), bind(&b.tx.outputs[0], 2), bind(&b.tx.outputs[1], 3), a.tx.outputs[2].clone(), bind(&b.tx.outputs[2], 2), a.tx.outputs[3].clone(), a.tx.outputs[4].clone()];
    m.output_labels = ["account", "script", "account 2", "script 2", "note", "note 2", "premium returned", "change"].iter().map(|s| s.to_string()).collect();
    m.created = vec![];
    m.change = Some(7);
    args_of(&mut m, 1)[7] = int_arg(6);
    let who2: Vec<u8> = [2i64, 2, 2].iter().flat_map(|x| sm8(*x)).collect();
    args_of(&mut m, 3)[0] = bytes_arg(&who2);
    args_of(&mut m, 3)[7] = int_arg(6);
    *args_of(&mut m, 2) = vec![int_arg(3), int_arg(0)];
    let sg = m.sign(&w.keys).expect("the two undone in one transaction");
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "U2 is closed: the transaction of the two is refused: {:?}", v.errors());
    assert_eq!(paid_to(&sg, &xk(&w.c)), 20 * KAS1 as u64, "U2 is open: two premiums of 20 KAS are returned by one output of 20");
    let change = sg.tx.outputs[7].value as i64;
    assert!(change > ff[0].amount as i64 + 19 * KAS1, "U2 is open: whoever built it has the other 20 KAS: its change is {change}");
    println!("U2: the owner has {} of 40 KAS; the builder's change is {} for a funding of {}", paid_to(&sg, &xk(&w.c)), change, ff[0].amount);
}

// ================================================================================================================
// B1: KpfTEnterSD against the model.

/// On random rows, shards and legs (the four ways; strikes in, at and out of the money; rows with units of both
/// signs; shards that hold nothing, a sompi either side of what the row is asked, and much): the gate written
/// here from the paper is `entered_d`'s verdict, and the engine's; what the engine takes is the model's shard;
/// one thing of the shard after changed is refused.
#[test]
fn t18_held_an_options_entry_is_the_models_on_random_rows_and_the_gate_is_to_the_sompi() {
    let w = world();
    let h = w.head(0);
    let (scale, limit) = (h.scale, h.price_max);
    let base = w.bare();
    let committee = w.markets[0].committee_id;
    let enter = w.script("KpfTEnterSD");
    let f = [fund(xk(&w.w), 33), fund(xk(&w.w), 34)];
    let mut r = Rng(0x243f6a8885a308d3);
    let (mut taken, mut refused, mut lowered, mut edge, mut lies) = (0, 0, 0, 0, 0);
    let mut by_way = [0; 4];
    for case in 0..600 {
        let calls = r.below(2) == 0;
        let strike = P10 / 100 * r.range(50, 150);
        let day = T0 + r.range(200_000, 3_000_000);
        let key = key_of(&w, calls, strike, day);
        let (s1, b1) = listed(&w, base.s, &Books::opened(), key);
        let (s1, b1) = listed(&w, s1, &b1, Key { strike: strike + 7, ..key });
        let gross_u = if r.below(4) == 0 { 0 } else { r.range(1, 300) };
        let net_u = if gross_u == 0 { 0 } else { r.range(-gross_u, gross_u) };
        let ent = if gross_u == 0 { 0 } else { r.range(-50, 50) * KAS1 + r.range(0, 99) };
        let (s2, b2) = row_set(s1, &b1, &key, net_u * scale, gross_u * scale, ent);
        let row = b2.row(&key).unwrap();
        let way = if calls { 3 + r.below(2) } else { 5 + r.below(2) };
        let size = r.range(1, 200) * scale + if r.below(3) == 0 { r.range(0, scale - 1) } else { 0 };
        let e = match r.below(5) {
            0 => strike,
            1 => strike + r.range(-3, 3),
            _ => P10 / 100 * r.range(60, 140) + r.range(0, 999),
        };
        let premium = match r.below(4) {
            0 => 0,
            1 => r.range(0, 99),
            _ => r.range(0, 3 * KAS1),
        };
        let leg = XLeg { dir: way, committee: leg_more(0, day, strike, premium), size, entry: e, ..fleg(&w, 0, 1, LONG, e) };
        let entered = leg_entered(&leg, T0);
        let (_, units, basis) = Shard::book_of_option(&entered).unwrap();
        assert_eq!(units, if way % 2 == 0 { size } else { -size }, "the clearing side's units");
        let with = Row { net: row.net + units, gross: row.gross + size, entered: row.entered + basis, ..row };
        let (need, before) = (gate_of(&s2, &with, e), gate_of(&s2, &row, e));
        let free = match r.below(7) {
            0 => 0,
            1 => need - 1,
            2 => need,
            3 => need + 1,
            4 => r.range(0, need.max(1)),
            5 => before,
            _ => 20_000 * KAS1,
        };
        let owes = if r.below(3) == 0 { r.range(1, 50) * KAS1 } else { 0 };
        let (holds, owes) = if free >= 0 { (free + owes, owes) } else { (0, -free) };
        let shard = live(&base, Shard { held: holds, owed: owes, ..s2 });
        let expect = free >= need || need <= before;
        let model = shard.s.entered_d(&b2, &entered, T0);
        assert_eq!(model.is_ok(), expect, "case {case}: the gate of the paper and the model's (way {way}, need {need}, before {before}, free {free})");
        let (as_if, books_after) = as_if_entered(&shard.s, &b2, &entered, T0);
        if let Ok((next, nb)) = &model {
            assert_eq!((*next, nb.root()), (as_if, books_after.root()), "case {case}: the model's shard after");
        }
        let acct = cov(&w, &shard, &[], 5_000 * KAS1);
        let add = if way % 2 == 0 { Marked::premium_added(&entered).unwrap() } else { r.range(0, 5) * KAS1 };
        let (st, said, _, _) = acct.s.c_enter(leg, add, 0, &[], T0, acct.value as i64).unwrap_or_else(|x| panic!("case {case}: the account's statement: {x}"));
        let b = c_enter_state_d_as(&w.lib, &acct, &shard, &b2, &enter, SEED, committee, &st, &said, &as_if, w.payer(&w.w, &f)).unwrap();
        assert_eq!(accepted(&b, &w.keys), expect, "case {case}: the engine and the model part (way {way}, size {size}, strike {strike}, entry {e}, row {} over {}, need {need}, before {before}, free {free})", row.net, row.gross);
        if !expect {
            refused += 1;
            continue;
        }
        taken += 1;
        by_way[(way - 3) as usize] += 1;
        if free < need {
            lowered += 1;
        }
        if free == need && need > before {
            edge += 1;
        }
        // One thing of the shard after changed: refused.
        let other_row = |x: Row| {
            let mut t = books_after.clone();
            let at = t.place_of(&key).unwrap();
            t.leaves[at] = Some(x);
            t.root()
        };
        let ra = books_after.row(&key).unwrap();
        let mut rows = as_if.rows;
        rows[0] = ShardRow { gross: rows[0].gross + 1, ..rows[0] };
        let wrong = [
            Shard { root: shard.s.root, ..as_if },
            Shard { dgross: as_if.dgross - 1, ..as_if },
            Shard { dgross: as_if.dgross + 1, ..as_if },
            Shard { basis: as_if.basis + 1, ..as_if },
            Shard { basis: as_if.basis - 1, ..as_if },
            Shard { root: other_row(Row { net: row.net - units, ..ra }), ..as_if },
            Shard { root: other_row(Row { net: ra.net + 1, ..ra }), ..as_if },
            Shard { root: other_row(Row { gross: ra.gross - 1, ..ra }), ..as_if },
            Shard { root: other_row(Row { entered: ra.entered + 1, ..ra }), ..as_if },
            Shard { root: other_row(Row { entered: ra.entered - 1, ..ra }), ..as_if },
            Shard { expiry: as_if.expiry - 1, ..as_if },
            Shard { expiry: as_if.expiry + 1, ..as_if },
            Shard { held: as_if.held + 1, ..as_if },
            Shard { owed: as_if.owed + 1, ..as_if },
            Shard { rows, ..as_if },
        ];
        for _ in 0..2 {
            let k = r.below(wrong.len() as i64) as usize;
            let mut t = b.clone();
            t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &wrong[k].hash());
            assert!(!accepted(&t, &w.keys), "case {case}: another shard after is taken (change {k})");
            lies += 1;
        }
    }
    println!("an option's entry on random rows: {taken} taken ({by_way:?} by way; {lowered} by a shard that holds less than the row is asked, the row asked no more; {edge} held to the sompi), {refused} refused by the gate; {lies} other shards after refused");
    assert!(taken >= 30 && refused >= 8 && lowered >= 3 && by_way.iter().all(|n| *n >= 3), "{taken} {refused} {lowered} {by_way:?}");
}

/// The gate, a sompi either side, for each of the four ways into an empty row: what the row is asked with the leg
/// is held, taken; a sompi less, refused, by the model and at the engine. And what the shard owes is not held.
#[test]
fn t18_held_the_rows_gate_a_sompi_either_side_for_each_way() {
    let w = world();
    let h = w.head(0);
    let base = w.bare();
    let committee = w.markets[0].committee_id;
    let enter = w.script("KpfTEnterSD");
    let f = [fund(xk(&w.w), 33)];
    for (way, strike, e) in [(3, P10 / 100 * 99, P10), (3, P10 / 100 * 130, P10), (4, P10 / 100 * 99, P10), (4, P10 / 100 * 104, P10), (5, P10 / 100 * 101, P10), (6, P10 / 100 * 101, P10), (6, P10 / 100 * 96, P10), (5, P10, P10), (4, P10, P10)] {
        let key = key_of(&w, way <= 4, strike, DAY);
        let (s1, b1) = listed(&w, base.s, &Books::opened(), key);
        let leg = oleg(&w, 0, way, 100, strike, e, DAY, KAS1 / 4);
        let entered = leg_entered(&leg, T0);
        let (_, units, basis) = Shard::book_of_option(&entered).unwrap();
        let row = b1.row(&key).unwrap();
        let need = gate_of(&s1, &Row { net: units, gross: entered.size, entered: basis, ..row }, e);
        assert!(need > 0, "way {way}: the row is asked something");
        for (owes, free, want) in [(0, need, true), (0, need - 1, false), (30 * KAS1, need, true), (30 * KAS1, need - 1, false), (0, 0, false)] {
            let shard = live(&base, Shard { held: free + owes, owed: owes, ..s1 });
            let model = shard.s.entered_d(&b1, &entered, T0);
            assert_eq!(model.is_ok(), want, "way {way} strike {strike}: the model, holding {free} above what it owes, asked {need}");
            let (as_if, _) = as_if_entered(&shard.s, &b1, &entered, T0);
            let acct = cov(&w, &shard, &[], 5_000 * KAS1);
            let add = if way % 2 == 0 { Marked::premium_added(&entered).unwrap() } else { 0 };
            let (st, said, _, _) = acct.s.c_enter(leg, add, 0, &[], T0, acct.value as i64).unwrap();
            let b = c_enter_state_d_as(&w.lib, &acct, &shard, &b1, &enter, SEED, committee, &st, &said, &as_if, w.payer(&w.w, &f)).unwrap();
            assert_eq!(accepted(&b, &w.keys), want, "way {way} strike {strike}: the engine, holding {free} above what it owes, asked {need}");
        }
        println!("way {way}, strike {strike}, entered at {e}, 100 units: the row is asked {need} sompi ({} KAS)", need / KAS1);
    }
}

/// An entry of the option leg `leg` (as it stands in the account) by hand: what is said and the statement made
/// from those of a valid entry (`tpl`), with the marked account of this leg; the shard after as it is given.
fn forced_entry(w: &World, acct: &OLive<Account>, shard: &OLive<Shard>, bk: &Books, tpl: (&Stated, &CEnterSaid), leg: &XLeg, add: i64, next: &Shard, f: &[KeyUtxo]) -> Option<OBuilt> {
    let said = CEnterSaid { add, leg: *leg, ..tpl.1.clone() };
    let marked = Marked(Account { legs: acct.s.legs.iter().copied().chain([*leg]).collect(), a_margin: acct.s.a_margin + add, a_min_counter: acct.s.a_min_counter + 1, ..acct.s.clone() });
    let st = Stated { position: marked.hash(), arg: said.arg(), bond: marked.0.c_bond(), held: acct.value as i64 + add, ..*tpl.0 };
    c_enter_state_d_as(&w.lib, acct, shard, bk, &w.script("KpfTEnterSD"), SEED, w.markets[0].committee_id, &st, &said, next, w.payer(&w.w, f)).ok()
}

/// A valid entry with one thing changed is refused by `KpfTEnterSD`: the leg's way (a futures leg, a token, 0, 8,
/// 9), its time, its premium below nothing or above the limit, its scale or limit; a signature; the input that
/// shows the time; the leaf, its place, its path; a called shard; a shard's time too near. And `KpfTEnterS`, the
/// futures twin, takes no option leg. Each change is made so that everything else agrees with it (what is said,
/// its hash, the marked account), and the unchanged transaction made the same way is taken.
#[test]
fn t18_held_an_options_entry_with_one_thing_changed_is_refused() {
    let w = world();
    let h = w.head(0);
    let base = w.bare();
    let committee = w.markets[0].committee_id;
    let enter = w.script("KpfTEnterSD");
    let f = [fund(xk(&w.w), 33), fund(xk(&w.w), 34)];
    let (kc, kq) = (key_of(&w, true, P10 / 100 * 99, DAY), key_of(&w, false, P10 / 100 * 101, DAY));
    let (s1, b1) = listed(&w, base.s, &Books::opened(), kc);
    let (s1, b1) = listed(&w, s1, &b1, kq);
    let (s1, bk) = listed(&w, s1, &b1, Key { strike: P10 / 100 * 98, ..kc });
    let shard = live(&base, Shard { held: 5_000 * KAS1, ..s1 });
    let acct = cov(&w, &shard, &[], 5_000 * KAS1);
    let entry = |way: i64| {
        let leg = oleg(&w, 0, way, 10, if way <= 4 { kc.strike } else { kq.strike }, P10, DAY, KAS1 / 2);
        let add = if way % 2 == 0 { 5 * KAS1 } else { 0 };
        c_enter_state_d(&w.lib, &acct, &shard, &bk, &enter, SEED, committee, leg, add, &[], T0, w.payer(&w.w, &f)).unwrap()
    };
    let n = std::cell::Cell::new(0);
    let no = |what: &str, t: &OBuilt| {
        assert!(!accepted(t, &w.keys), "taken: {what}");
        n.set(n.get() + 1);
    };
    // The leg's way.
    for (way, others) in [(4i64, vec![0i64, 1, 2, -4]), (6, vec![7, 8, 9]), (3, vec![]), (5, vec![])] {
        let (b, st, said, marked, _, _, _) = entry(way);
        signed(&format!("an entry of the way {way}"), &b, &w.keys);
        assert!(accepted(&restated(&w, &b, &st, &said, &marked, &said.leg), &w.keys), "the entry restated as it is");
        for d in others {
            no(&format!("the way {d} for {way}"), &restated(&w, &b, &st, &said, &marked, &XLeg { dir: d, ..said.leg }));
        }
        if way == 4 {
            // (A futures leg whole: long, its 32 bytes the time and three numbers of nothing.)
            no("a futures leg", &restated(&w, &b, &st, &said, &marked, &XLeg { dir: LONG, committee: leg_more(T0, 0, 0, 0), ..said.leg }));
        }
        let o = kpf_protocol::crow::option_of(&said.leg).unwrap();
        let more = |t: i64, p: i64| XLeg { committee: leg_more(t, o.day, o.strike, p), ..said.leg };
        no("the leg's time after the statement's", &restated(&w, &b, &st, &said, &marked, &more(T0 + 1, o.premium)));
        no("the leg's time before the statement's", &restated(&w, &b, &st, &said, &marked, &more(T0 - 1, o.premium)));
        no("a scale that is not the row's", &restated(&w, &b, &st, &said, &marked, &XLeg { scale: h.scale / 10, ..said.leg }));
        no("a limit that is not the row's", &restated(&w, &b, &st, &said, &marked, &XLeg { price_max: h.price_max + 1, ..said.leg }));
        no("a strike that is not the row's", &restated(&w, &b, &st, &said, &marked, &XLeg { committee: leg_more(T0, o.day, o.strike + 1, o.premium), ..said.leg }));
        no("a day that is not the row's", &restated(&w, &b, &st, &said, &marked, &XLeg { committee: leg_more(T0, o.day + 1, o.strike, o.premium), ..said.leg }));
        if way % 2 == 1 {
            no("a premium below nothing", &restated(&w, &b, &st, &said, &marked, &more(T0, -1)));
            no("a premium above the limit", &restated(&w, &b, &st, &said, &marked, &more(T0, h.price_max + 1)));
            assert!(accepted(&restated(&w, &b, &st, &said, &marked, &more(T0, h.price_max)), &w.keys), "a held option's premium at the limit is a premium");
            assert!(accepted(&restated(&w, &b, &st, &said, &marked, &more(T0, 0)), &w.keys), "a premium of nothing is a premium");
        } else {
            // (A written option's premium is what the entry adds: a sompi a unit more, and the entry adds too little.)
            no("a written option's premium another than what is added", &restated(&w, &b, &st, &said, &marked, &more(T0, o.premium + 1)));
            no("a written option's premium of nothing with coins added", &restated(&w, &b, &st, &said, &marked, &more(T0, 0)));
        }
        // The signatures, the input that shows the time, the leaf and its path.
        let sig_at: Vec<usize> = args_of(&mut b.clone(), 1).iter().enumerate().filter(|(_, a)| matches!(a, Arg::Sig(_))).map(|(i, _)| i).collect();
        assert_eq!(sig_at.len(), 2);
        let (mut t1, mut t2) = (b.clone(), b.clone());
        args_of(&mut t1, 1)[sig_at[1]] = Arg::Sig(xk(&w.w));
        args_of(&mut t2, 1)[sig_at[0]] = Arg::Sig(xk(&w.c));
        no("the user signs twice", &t1);
        no("the owner signs twice", &t2);
        let f_at = sig_at[0] - 1;
        for x in [0i64, 1, 2, -1, 5, 99] {
            let mut t = b.clone();
            args_of(&mut t, 1)[f_at] = int_arg(x);
            no(&format!("the input {x} shows the time"), &t);
        }
        let last = args_of(&mut b.clone(), 1).len();
        let key = if way <= 4 { kc } else { kq };
        let (own, other) = (bk.shown(&key).unwrap(), bk.shown(&Key { strike: P10 / 100 * 98, ..kc }).unwrap());
        let mut t = b.clone();
        args_of(&mut t, 1)[last - 3] = bytes_arg(&leaf138(&other.row));
        args_of(&mut t, 1)[last - 2] = int_arg(other.place as i64);
        args_of(&mut t, 1)[last - 1] = bytes_arg(&other.beside.concat());
        no("the leaf of another strike, with its own path", &t);
        let mut t = b.clone();
        args_of(&mut t, 1)[last - 2] = int_arg(other.place as i64);
        no("the leaf at another place", &t);
        for x in [-1i64, 64, 64 + own.place as i64] {
            let mut t = b.clone();
            args_of(&mut t, 1)[last - 2] = int_arg(x);
            no(&format!("the place {x}"), &t);
        }
        let mut path = own.beside.concat();
        path[100] ^= 1;
        let mut t = b.clone();
        args_of(&mut t, 1)[last - 1] = bytes_arg(&path);
        no("a path with a bit changed", &t);
        let mut t = b.clone();
        args_of(&mut t, 1)[last - 3] = bytes_arg(&leaf138(&Row { net: own.row.net + 1, ..own.row }));
        no("the leaf with a unit that is not there", &t);
        // A called shard; a shard's time a block too near.
        let (leg_in, add) = (said.leg, said.add);
        let called = live(&base, Shard { called: 6_000, ..shard.s });
        let (as_if, _) = as_if_entered(&called.s, &bk, &leg_in, T0);
        assert!(called.s.entered_d(&bk, &leg_in, T0).is_err());
        no("a called shard", &forced_entry(&w, &acct, &called, &bk, (&st, &said), &leg_in, add, &as_if, &f).expect("built"));
        for (time, want) in [(T0 + 2 * shard.s.dispute_daa, true), (T0 + 2 * shard.s.dispute_daa + 1, false)] {
            let timed = live(&base, Shard { expiry: time, ..shard.s });
            let (as_if, _) = as_if_entered(&timed.s, &bk, &leg_in, T0);
            assert_eq!(timed.s.entered_d(&bk, &leg_in, T0).is_ok(), want, "the model, a shard's time {time}");
            let t = forced_entry(&w, &acct, &timed, &bk, (&st, &said), &leg_in, add, &as_if, &f).expect("built");
            assert_eq!(accepted(&t, &w.keys), want, "the engine, a shard's time {time}");
            n.set(n.get() + 1);
        }
    }
    // A day at or before the statement's time; a day a block after it (the script that states takes it: its row is
    // what is answered wrong).
    let (_, st, said, _, _, _, _) = entry(3);
    for (day, want) in [(T0 - 1, false), (T0, false), (T0 + 1, true)] {
        let k = Key { day, ..kc };
        let free: Vec<usize> = (0..64).filter(|i| bk.leaves[*i].is_none()).collect();
        match shard.s.row_in(&bk, k, h.scale, h.price_max, 0, free[0], Some(free[1])) {
            Err(e) => println!("a series of the day {day} is listed by nothing ({e}): no leaf, no entry"),
            Ok((s9, b9)) => {
                let sh9 = live(&base, s9);
                let leg = XLeg { committee: leg_more(T0, day, kc.strike, KAS1 / 2), ..said.leg };
                let (as_if, _) = as_if_entered(&s9, &b9, &leg, T0);
                let t = forced_entry(&w, &acct, &sh9, &b9, (&st, &said), &leg, 0, &as_if, &f).expect("built");
                assert_eq!(accepted(&t, &w.keys), want, "an option of the day {day} stated at {T0}");
                n.set(n.get() + 1);
            }
        }
    }
    // The futures twin takes no option leg.
    let long = fleg(&w, 0, 10, LONG, P10);
    let (b, st, said, marked, _, _) = c_enter_state(&w.lib, &acct, &shard, &w.script("KpfTEnterS"), SEED, long, 0, &[], T0, w.payer(&w.w, &f)).unwrap();
    signed("a futures leg enters by KpfTEnterS", &b, &w.keys);
    assert!(accepted(&restated(&w, &b, &st, &said, &marked, &said.leg), &w.keys));
    for way in 3..=6 {
        no(&format!("KpfTEnterS: the way {way}"), &restated(&w, &b, &st, &said, &marked, &XLeg { dir: way, ..said.leg }));
        no(&format!("KpfTEnterS: an option leg whole, way {way}"), &restated(&w, &b, &st, &said, &marked, &XLeg { dir: way, committee: leg_more(T0, DAY, kc.strike, KAS1 / 2), ..said.leg }));
    }
    no("KpfTEnterS: a long with a day", &restated(&w, &b, &st, &said, &marked, &XLeg { committee: leg_more(T0, DAY, 0, 0), ..said.leg }));
    println!("an option's entry with one thing changed: {} refused or as the model has them", n.get());
}

/// The largest size a leg has at its market's limit, and a row whose gross is at the limit: the engine and the
/// model (`c_marked` and `entered_d`) say the same.
#[test]
fn t18_held_the_largest_sizes_are_the_models() {
    let w = world();
    let h = w.head(0);
    let base = w.bare();
    let f = [fund(xk(&w.w), 33), fund(xk(&w.w), 34)];
    let max = kpf_protocol::cross::MARGIN_MAX;
    let kc = key_of(&w, true, P10, DAY);
    let (s1, bk) = listed(&w, base.s, &Books::opened(), kc);
    let rich = live(&base, Shard { held: max, ..s1 });
    let acct = cov(&w, &rich, &[], 5_000 * KAS1);
    let (_, st, said, _, _, _, _) = c_enter_state_d(&w.lib, &acct, &rich, &bk, &w.script("KpfTEnterSD"), SEED, w.markets[0].committee_id, oleg(&w, 0, 3, 10, P10, P10, DAY, 0), 0, &[], T0, w.payer(&w.w, &f)).unwrap();
    let largest = (max / h.price_max - 1) * h.scale + h.scale - 1;
    let mut seen = vec![];
    for way in [3i64, 4] {
        for (what, size, e) in [("the largest size", largest, P10), ("the largest size at the limit's price", largest, h.price_max), ("a unit of size more", largest + 1, P10), ("the row's limit of units", max, P10)] {
            let leg = XLeg { dir: way, size, entry: e, committee: leg_more(T0, DAY, P10, 0), ..said.leg };
            let model = acct.s.c_marked(&leg, 0, T0).is_some() && rich.s.entered_d(&bk, &leg, T0).is_ok();
            let engine = match Shard::book_of_option(&leg).ok().and_then(|(hd, u, x)| bk.moved(&hd.key, u, leg.size, x).ok()) {
                None => false,
                Some(_) => {
                    let (as_if, _) = as_if_entered(&rich.s, &bk, &leg, T0);
                    forced_entry(&w, &acct, &rich, &bk, (&st, &said), &leg, 0, &as_if, &f).is_some_and(|t| accepted(&t, &w.keys))
                }
            };
            assert_eq!(engine, model, "way {way}, {what}: the engine takes it {engine}, the model {model}");
            seen.push((way, what, model));
        }
        // A row that has units to the limit less the leg's.
        for gross in [max - 10 * h.scale, max - 10 * h.scale + 1] {
            let (s2, b2) = row_set(s1, &bk, &kc, 0, gross, 0);
            let sh = live(&base, Shard { held: max, ..s2 });
            let leg = XLeg { dir: way, ..said.leg };
            let model = sh.s.entered_d(&b2, &leg, T0);
            let engine = b2.moved(&kc, if way == 4 { leg.size } else { -leg.size }, leg.size, 0).ok().is_some_and(|_| {
                let (as_if, _) = as_if_entered(&sh.s, &b2, &leg, T0);
                forced_entry(&w, &acct, &sh, &b2, (&st, &said), &leg, 0, &as_if, &f).is_some_and(|t| accepted(&t, &w.keys))
            });
            assert_eq!(engine, model.is_ok(), "way {way}, a row of {gross} units: the engine {engine}, the model {:?}", model.as_ref().err());
            seen.push((way, "a row at its limit", model.is_ok()));
        }
    }
    println!("the largest sizes: {seen:?}");
}

// ================================================================================================================
// B2: KpfTEnterOff.

/// For a futures leg and each way of an option: what an undone entry pays and to whom, the model's, at the engine;
/// a sompi less to the owner or in the account, the user's key, or another output named, refused. A premium of a
/// note's least returns and a sompi less stays; a premium larger than the coins less a note's least returns as
/// far as the coins go.
#[test]
fn t18_held_what_an_undone_entry_pays_and_to_whom() {
    let w = world();
    let base = w.bare();
    let ff = [fund(xk(&w.f), 36)];
    let off = w.script("KpfTEnterOff");
    let keeper = w.terms().keeper_fee_min;
    let mut seen = vec![];
    for (way, units, prem, coins0) in [
        (1i64, 10, 0, 40 * KAS1),
        (3, 10, 2 * KAS1, 40 * KAS1),
        (5, 10, 2 * KAS1, 40 * KAS1),
        (4, 10, 2 * KAS1, 40 * KAS1),
        (6, 10, 2 * KAS1, 40 * KAS1),
        (4, 1, NOTE_MIN, 40 * KAS1),
        (6, 1, NOTE_MIN - 1, 40 * KAS1),
        (4, 1, NOTE_MIN + 1, NOTE_MIN + keeper / 2),
        (4, 10, 2 * KAS1, NOTE_MIN / 2),
        (6, 10, 2 * KAS1, NOTE_MIN),
    ] {
        let leg = if way <= 2 { leg_entered(&fleg(&w, 0, units, way, P10), T0) } else { leg_entered(&oleg(&w, 0, way, units, P10 / 10 * 9, P10, DAY, prem), T0) };
        let written = way == 4 || way == 6;
        let worth = units * prem;
        let add = if written { worth } else { 7 * KAS1 };
        let acct = cov(&w, &base, &[], coins0);
        let marked = acct.s.c_marked(&leg, add, T0).expect("a marked account");
        let coins = coins0 + add;
        let mut back = if written { worth.min(coins - NOTE_MIN) } else { 0 };
        if back < NOTE_MIN {
            back = 0;
        }
        let fee = keeper.min(coins - back - NOTE_MIN).max(0);
        let (after, fee_m, _, back_m) = marked.undone_back().expect("undone");
        assert_eq!((back_m, fee_m, after.a_margin), (back, fee, coins - back - fee), "way {way}: the model");
        let m = OLive { s: marked.clone(), value: coins as u64, id: acct.id, op: op(90), daa: 6_700 };
        let (b, _, note) = enter_off(&w.lib, &m, &off, w.payer(&w.f, &ff)).unwrap();
        let sg = signed(&format!("way {way}, premium {worth}: undone"), &b, &w.keys);
        assert_eq!((paid_to(&sg, &xk(&w.c)), sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key, sg.tx.outputs[2].value), (back as u64, (coins - back - fee) as u64, &hash_spk(&w.lib, &after.hash()), NOTE_MIN as u64), "way {way}: the engine");
        assert_eq!(paid_to(&sg, &xk(&w.w)), 0, "nothing to the user's key");
        let mut t = b.clone();
        t.tx.outputs[0].value -= 1;
        assert!(!accepted(&t, &w.keys), "way {way}: a sompi less in the account");
        let mut t = b.clone();
        t.tx.outputs[2].value -= 1;
        assert!(!accepted(&t, &w.keys), "way {way}: a sompi less in the note");
        let last = args_of(&mut b.clone(), 1).len() - 1;
        if back > 0 {
            let mut t = b.clone();
            t.tx.outputs[3].value -= 1;
            assert!(!accepted(&t, &w.keys), "way {way}: a sompi less to the owner");
            let mut t = b.clone();
            t.tx.outputs[3].script_public_key = p2pk_spk(&xk(&w.w));
            assert!(!accepted(&t, &w.keys), "way {way}: the premium to the user");
            for po in [0i64, 1, 2, 4, 5, -1, 1_000] {
                let mut t = b.clone();
                args_of(&mut t, 1)[last] = int_arg(po);
                assert!(!accepted(&t, &w.keys), "way {way}: the output {po} named for the premium");
            }
            // (An account that says a sompi more premium than its leg: the leg is in the account's hash.)
            let mut t = b.clone();
            t.tx.outputs[0].value += 1;
            t.tx.outputs[3].value -= 1;
            assert!(!accepted(&t, &w.keys), "way {way}: a sompi of the premium kept in the account");
        } else {
            let mut t = b.clone();
            args_of(&mut t, 1)[last] = int_arg(1_000);
            assert!(accepted(&t, &w.keys), "way {way}: nothing returns, and the output named is not read");
        }
        seen.push((way, worth, coins0, back, fee));
    }
    println!("undone entries (way, premium, coins before, returned, fee): {seen:?}");
}

// ================================================================================================================
// B3: KpfTLeftD and KpfTLeftAllD against the model.

/// On random books (three series and two futures rows, with legs of other accounts in them) a note of one to six
/// legs, futures and options mixed in any order, several of one row, asking above and below nothing of a shard
/// that holds more and less: `left_d`'s shard is what the engine takes, and no other; the books lose exactly the
/// note's legs.
#[test]
fn t18_held_notes_of_option_legs_are_the_models_on_random_books() {
    let w = world();
    let h = w.head(0);
    let scale = h.scale;
    let base = w.bare();
    let ff = [fund(xk(&w.f), 36)];
    let (left_d, all_d) = (w.script("KpfTLeftD"), w.script("KpfTLeftAllD"));
    let keys = [key_of(&w, true, P10 / 100 * 104, DAY), key_of(&w, true, P10 / 100 * 96, DAY), key_of(&w, false, P10 / 100 * 101, DAY + 500)];
    let (mut s0, mut b0) = (Shard { held: 1_000_000 * KAS1, ..base.s }, Books::opened());
    for k in keys {
        (s0, b0) = listed(&w, s0, &b0, k);
    }
    let mut r = Rng(0x13198a2e03707344);
    let (mut ones, mut alls, mut owed_n, mut paid_n, mut took_n, mut lies, mut same_row) = (0, 0, 0, 0, 0, 0, 0);
    for case in 0..400 {
        let leg_of = |r: &mut Rng| -> XLeg {
            let size = r.range(1, 60) * scale + if r.below(3) == 0 { r.range(1, scale - 1) } else { 0 };
            let e = P10 / 100 * r.range(85, 115) + r.range(0, 999);
            if r.below(3) == 0 {
                leg_entered(&XLeg { size, ..fleg(&w, r.below(2) as usize, 1, if r.below(2) == 0 { LONG } else { SHORT }, e) }, T0)
            } else {
                let k = keys[r.below(3) as usize];
                let way = if k.of == OF_CALLS { 3 + r.below(2) } else { 5 + r.below(2) };
                leg_entered(&XLeg { dir: way, committee: leg_more(0, k.day, k.strike, r.range(0, KAS1)), size, ..fleg(&w, 0, 1, LONG, e) }, T0)
            }
        };
        let n_legs = r.range(1, 6) as usize;
        let mut legs: Vec<XLeg> = (0..n_legs).map(|_| leg_of(&mut r)).collect();
        if n_legs == 1 && kpf_protocol::crow::option_of(&legs[0]).is_none() {
            legs[0] = leg_entered(&oleg(&w, 0, 3, r.range(1, 50), keys[0].strike, P10, DAY, KAS1 / 3), T0);
        }
        let others: Vec<XLeg> = (0..r.range(0, 4)).map(|_| leg_of(&mut r)).collect();
        // Every leg entered, by the model (the engine's entry is the tests' above).
        let (mut s, mut b) = (s0, b0.clone());
        for l in legs.iter().chain(&others) {
            s = Shard { expiry: 0, ..s };
            if kpf_protocol::crow::option_of(l).is_some() {
                (s, b) = s.entered_d(&b, l, T0).unwrap_or_else(|x| panic!("case {case}: {x}"));
            } else {
                let row = if l.market == w.markets[0].id() { 0 } else { 1 };
                s = s.entered(l, row, T0).unwrap_or_else(|x| panic!("case {case}: {x}"));
            }
        }
        let held = match r.below(4) {
            0 => 0,
            1 => r.range(0, 5 * KAS1),
            _ => r.range(50, 500) * KAS1,
        };
        let shard = live(&base, Shard { held, ..s });
        let asked = match r.below(5) {
            0 => 0,
            1 | 2 => -r.range(1, 40 * KAS1),
            _ => r.range(1, 20 * KAS1),
        };
        let n = Left { sid: s.sid, legs: legs.clone(), key: xk(&w.w), asked };
        let holds = if asked < 0 { NOTE_MIN - if r.below(4) == 0 { asked / 2 } else { asked } } else { NOTE_MIN };
        let note = OLive { value: holds as u64, ..left_note(&shard, &n, 1) };
        let script = if n_legs == 1 { &left_d } else { &all_d };
        let (tx, next, nb, rest) = left_apply_d(&w.lib, &shard, &b, &note, script, w.payer(&w.f, &ff)).unwrap_or_else(|x| panic!("case {case}: the model applies no such note: {x}"));
        let sg = b_signed(&tx, &w.keys).unwrap_or_else(|x| panic!("case {case}: the engine refuses the model's shard: {x} ({n_legs} legs, asked {asked}, held {held})"));
        // The books without the note's legs: the others alone, entered in their order.
        let (mut sx, mut bx) = (s0, b0.clone());
        for l in &others {
            sx = Shard { expiry: 0, ..sx };
            if kpf_protocol::crow::option_of(l).is_some() {
                (sx, bx) = sx.entered_d(&bx, l, T0).unwrap();
            } else {
                sx = sx.entered(l, if l.market == w.markets[0].id() { 0 } else { 1 }, T0).unwrap();
            }
        }
        let took = if asked < 0 { (holds - NOTE_MIN).min(-asked) } else { 0 };
        let pay = asked.max(0).min(held);
        let owes = asked.max(0) - pay;
        assert_eq!((nb.root(), next.root, next.dgross, next.basis), (bx.root(), bx.root(), sx.dgross, sx.basis), "case {case}: the tree, the sum and the number without the note's legs");
        for i in 0..4 {
            assert_eq!((next.rows[i].net, next.rows[i].gross), (sx.rows[i].net, sx.rows[i].gross), "case {case}: the futures row {i}");
        }
        assert_eq!((next.held, next.owed, rest.map(|x| x.amount)), (held + took - pay, owes, (owes > 0).then_some(owes)), "case {case}: what is held, owed and written of the rest");
        assert_eq!(sg.tx.outputs[0].value as i64, shard.value as i64 + took - pay, "case {case}: the shard's coins");
        if pay > 0 {
            assert!(paid_to(&sg, &xk(&w.w)) >= pay as u64, "case {case}: the user is paid");
            paid_n += 1;
        }
        if owes > 0 {
            owed_n += 1;
        }
        if took > 0 {
            took_n += 1;
        }
        if n_legs == 1 { ones += 1 } else { alls += 1 }
        let opts: Vec<Key> = legs.iter().filter(|l| kpf_protocol::crow::option_of(l).is_some()).map(|l| Shard::book_of_option(l).unwrap().0.key).collect();
        if opts.iter().enumerate().any(|(i, k)| opts[..i].contains(k)) {
            same_row += 1;
        }
        // Another shard after: refused.
        let mut rows = next.rows;
        rows[0] = ShardRow { gross: rows[0].gross + 1, ..rows[0] };
        let wrong = [
            Shard { root: shard.s.root, ..next },
            Shard { dgross: next.dgross + 1, ..next },
            Shard { dgross: next.dgross - 1, ..next },
            Shard { basis: next.basis + 1, ..next },
            Shard { basis: next.basis - 1, ..next },
            Shard { held: next.held + 1, ..next },
            Shard { owed: next.owed + 1, ..next },
            Shard { rows, ..next },
            Shard { rows: shard.s.rows, root: next.root, ..next },
        ];
        for _ in 0..2 {
            let k = r.below(wrong.len() as i64) as usize;
            if wrong[k] == next {
                continue;
            }
            let mut t = tx.clone();
            t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &wrong[k].hash());
            assert!(!accepted(&t, &w.keys), "case {case}: another shard after is taken (change {k})");
            lies += 1;
        }
    }
    println!("notes on random books: {ones} of one option leg, {alls} of several ({same_row} with two option legs of one row); {paid_n} paid, {owed_n} with a note of the rest, {took_n} that held what an account paid; {lies} other shards after refused");
    assert!(ones >= 8 && alls >= 50 && same_row >= 5 && paid_n >= 15 && owed_n >= 5 && took_n >= 15, "{ones} {alls} {same_row} {paid_n} {owed_n} {took_n}");
}

fn b_signed(b: &OBuilt, keys: &KeySet) -> Result<OSigned, String> {
    let sg = b.sign(keys).map_err(|e| e.to_string())?;
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    if v.is_ok() { Ok(sg) } else { Err(format!("{:?}", v.errors())) }
}

/// What the scripts of a note refuse: an option leg's note through the futures scripts, a futures leg's through
/// `KpfTLeftD`; the second leg of a row shown the row as it was; a leg shown the slot of another; slots too short;
/// more units than the row has; a futures leg's row misnamed; a called shard by each.
#[test]
fn t18_held_a_note_of_option_legs_with_one_thing_changed_is_refused() {
    let w = world();
    let h = w.head(0);
    let scale = h.scale;
    let base = w.bare();
    let ff = [fund(xk(&w.f), 36)];
    let (left_d, all_d) = (w.script("KpfTLeftD"), w.script("KpfTLeftAllD"));
    let (k1, k2) = (key_of(&w, true, P10 / 10 * 11, DAY), key_of(&w, false, P10 / 10 * 9, DAY));
    let (s0, b0) = listed(&w, base.s, &Books::opened(), k1);
    let (s0, b0) = listed(&w, s0, &b0, k2);
    let long = leg_entered(&fleg(&w, 0, 10, LONG, P10), T0);
    let (c4, c6, p5) = (leg_entered(&oleg(&w, 0, 3, 4, k1.strike, P10, DAY, KAS1 / 2), T0), leg_entered(&oleg(&w, 0, 3, 6, k1.strike, P10, DAY, KAS1 / 2), T0), leg_entered(&oleg(&w, 0, 6, 5, k2.strike, P10, DAY, KAS1 / 2), T0));
    let mut s = s0.entered(&long, 0, T0).unwrap();
    let mut b = b0.clone();
    for l in [&c4, &c6, &p5] {
        (s, b) = Shard { expiry: 0, ..s }.entered_d(&b, l, T0).unwrap();
    }
    let shard = live(&base, s);
    let mut n = 0;
    let mut no = |what: &str, t: &OBuilt| {
        assert!(!accepted(t, &w.keys), "taken: {what}");
        n += 1;
    };
    let note_of = |legs: Vec<XLeg>, k: u8| left_note(&shard, &Left { sid: s.sid, legs, key: xk(&w.w), asked: 0 }, k);
    // One option leg: by KpfTLeftD; by KpfTLeft, the futures script, with the shard the rule writes: refused.
    let one = note_of(vec![c4], 1);
    let (tx, next, _, _) = left_apply_d(&w.lib, &shard, &b, &one, &left_d, w.payer(&w.f, &ff)).unwrap();
    signed("a note of one option leg", &tx, &w.keys);
    for r in 0..4 {
        if let Ok((t, _, _)) = left_apply_as(&w.lib, &shard, &one, &[r], next, 0, None, &w.script("KpfTLeft"), w.payer(&w.f, &ff)) {
            no("an option leg's note by KpfTLeft", &t);
        }
        if let Ok((t, _, _)) = left_apply_as(&w.lib, &shard, &one, &[r], shard.s, 0, None, &w.script("KpfTLeft"), w.payer(&w.f, &ff)) {
            no("an option leg's note by KpfTLeft, the shard as it is", &t);
        }
    }
    // (A futures leg's note through KpfTLeftD: the leg and the note changed in the transaction above.)
    let fnote = Left { sid: s.sid, legs: vec![long], key: xk(&w.w), asked: 0 };
    let mut t = tx.clone();
    let leg_at = args_of(&mut t, 1).iter().position(|a| matches!(a, Arg::V(ArtifactValue::Bytes(x)) if *x == kpf_protocol::thin::leg_bytes(&c4))).expect("the leg, among the arguments");
    args_of(&mut t, 1)[leg_at] = bytes_arg(&kpf_protocol::thin::leg_bytes(&long));
    object_made(&mut t, &w.lib, 2, &fnote.hash());
    no("a futures leg's note by KpfTLeftD", &t);
    let (fs, _, _, _) = shard.s.left(&fnote).unwrap();
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &fs.hash());
    no("a futures leg's note by KpfTLeftD, the shard as the rule writes it", &t);
    // (More units than the row has; a leg of the row's key and another scale.)
    let big = Left { sid: s.sid, legs: vec![leg_entered(&oleg(&w, 0, 3, 11, k1.strike, P10, DAY, KAS1 / 2), T0)], key: xk(&w.w), asked: 0 };
    assert!(shard.s.left_d(&b, &big, NOTE_MIN).is_err());
    let nb = b.moved(&k1, 10 * scale, -10 * scale, 0).unwrap();
    let as_if = Shard { root: nb.root(), dgross: s.dgross - 11 * scale, ..s };
    if let Ok(t) = left_apply_d_of(&w.lib, &shard, &b, &left_note(&shard, &big, 2), &big, big.hash(), NOTE_LEFT, as_if, 0, None, &left_d, w.payer(&w.f, &ff)) {
        no("more units than the row has", &t);
    }
    // Several legs.
    let both = note_of(vec![c4, long, p5, c6], 3);
    let (tx, next, nb, _) = left_apply_d(&w.lib, &shard, &b, &both, &all_d, w.payer(&w.f, &ff)).unwrap();
    signed("a note of four legs, three of them options", &tx, &w.keys);
    assert_eq!((nb.root(), next.dgross, next.rows[0].gross), (b0.root(), 0, 0), "every leg out: the tree as it was listed");
    let last = args_of(&mut tx.clone(), 1).len();
    let bytes_at = |t: &mut OBuilt, i: usize| -> Vec<u8> {
        match &args_of(t, 1)[i] {
            Arg::V(ArtifactValue::Bytes(x)) => x.clone(),
            _ => panic!("bytes"),
        }
    };
    let (leaves, places, besides) = (bytes_at(&mut tx.clone(), last - 3), bytes_at(&mut tx.clone(), last - 2), bytes_at(&mut tx.clone(), last - 1));
    assert_eq!((leaves.len(), places.len(), besides.len()), (4 * 138, 4 * 8, 4 * 192));
    // (The fourth leg, the second of its row, shown the row as the first was shown it.)
    let mut t = tx.clone();
    let (mut lv, mut bs) = (leaves.clone(), besides.clone());
    lv.splice(3 * 138..4 * 138, leaves[..138].to_vec());
    bs.splice(3 * 192..4 * 192, besides[..192].to_vec());
    args_of(&mut t, 1)[last - 3] = bytes_arg(&lv);
    args_of(&mut t, 1)[last - 1] = bytes_arg(&bs);
    no("the second leg of a row shown the row as it was", &t);
    // (The first and the third leg, of two rows, each shown the other's slot.)
    let mut t = tx.clone();
    let (mut lv, mut pl, mut bs) = (leaves.clone(), places.clone(), besides.clone());
    lv.splice(..138, leaves[2 * 138..3 * 138].to_vec());
    lv.splice(2 * 138..3 * 138, leaves[..138].to_vec());
    pl.splice(..8, places[16..24].to_vec());
    pl.splice(16..24, places[..8].to_vec());
    bs.splice(..192, besides[2 * 192..3 * 192].to_vec());
    bs.splice(2 * 192..3 * 192, besides[..192].to_vec());
    args_of(&mut t, 1)[last - 3] = bytes_arg(&lv);
    args_of(&mut t, 1)[last - 2] = bytes_arg(&pl);
    args_of(&mut t, 1)[last - 1] = bytes_arg(&bs);
    no("two legs each shown the other's slot", &t);
    // (Slots too short: the last option leg's cut off.)
    for (i, len) in [(last - 3, 4 * 138 - 1), (last - 2, 4 * 8 - 1), (last - 1, 4 * 192 - 1), (last - 3, 3 * 138), (last - 3, 138)] {
        let mut t = tx.clone();
        let x = bytes_at(&mut t, i);
        args_of(&mut t, 1)[i] = bytes_arg(&x[..len]);
        no("a slot cut short", &t);
    }
    // (A place of another leaf; the futures leg's row misnamed.)
    let mut t = tx.clone();
    let mut pl = places.clone();
    pl.splice(..8, places[16..24].to_vec());
    args_of(&mut t, 1)[last - 2] = bytes_arg(&pl);
    no("a leaf at another place", &t);
    let rows_at = args_of(&mut tx.clone(), 1).iter().position(|a| matches!(a, Arg::V(ArtifactValue::Bytes(x)) if x.len() == 32 && x[8..16] == sm8(0) && *x != xk(&w.w).to_vec())).expect("the rows named");
    for r in 1..4i64 {
        let mut t = tx.clone();
        let mut at = bytes_at(&mut t, rows_at);
        at.splice(8..16, sm8(r));
        args_of(&mut t, 1)[rows_at] = bytes_arg(&at);
        no("the futures leg's row misnamed", &t);
    }
    // (The same note by KpfTLeftAll, the futures script; a note of futures legs alone by KpfTLeftAllD is the twin's.)
    if let Ok((t, _, _)) = left_apply_as(&w.lib, &shard, &both, &[0, 0, 0, 0], next, 0, None, &w.script("KpfTLeftAll"), w.payer(&w.f, &ff)) {
        no("a note with option legs by KpfTLeftAll", &t);
    }
    // A called shard, by each.
    let called = live(&base, Shard { called: 6_000, ..s });
    // (And by the futures scripts of a called shard, KpfTLeftC and KpfTLeftAllC, with the shard as the rule of a
    // shard that is not called would write it, called; and as it is.)
    for r in 0..4 {
        let (n1, _, _, _, _) = shard.s.left_d(&b, &one.s, NOTE_MIN).unwrap();
        for after in [Shard { called: 6_000, ..n1 }, called.s] {
            if let Ok((t, _, _)) = left_apply_as_c(&w.lib, &called, &left_note(&called, &one.s, 6), NOTE_LEFT, 0, &[r], after, 0, None, &w.script("KpfTLeftC"), w.payer(&w.f, &ff)) {
                no("an option leg's note by KpfTLeftC", &t);
            }
        }
    }
    if let Ok((t, _, _)) = left_apply_as_c(&w.lib, &called, &left_note(&called, &both.s, 7), NOTE_LEFT, 0, &[0, 0, 0, 0], Shard { called: 6_000, ..next }, 0, None, &w.script("KpfTLeftAllC"), w.payer(&w.f, &ff)) {
        no("a note with option legs by KpfTLeftAllC", &t);
    }
    let (n1, _, _, _, _) = shard.s.left_d(&b, &one.s, NOTE_MIN).unwrap();
    if let Ok(t) = left_apply_d_of(&w.lib, &called, &b, &left_note(&called, &one.s, 4), &one.s, one.s.hash(), NOTE_LEFT, Shard { called: 6_000, ..n1 }, 0, None, &left_d, w.payer(&w.f, &ff)) {
        no("a called shard, a note of one option leg", &t);
    }
    let (n4, _, at, _, _) = shard.s.left_d(&b, &both.s, NOTE_MIN).unwrap();
    if let Ok(t) = left_apply_all_d_of(&w.lib, &called, &b, &left_note(&called, &both.s, 5), &at, Shard { called: 6_000, ..n4 }, 0, None, &all_d, w.payer(&w.f, &ff)) {
        no("a called shard, a note of several legs", &t);
    }
    println!("a note of option legs with one thing changed: {n} refused");
    assert!(n >= 20, "{n}");
}

// ================================================================================================================
// U3: an option that entered has no way out past its day or under a call (C and D are not built), and B1 is in the
// library.

/// A held call enters (the engine's entry is the tests' above). From its day on, by the model: its account states
/// nothing (no take, no leaving, no step: the account's other legs and coins stand with it), and no trace of a
/// draw or of a call of its shard is the rule's (a call's row of an ended day has no answer: review T17). Under a
/// call stated before the day: the account is stepped by nothing, its note is applied by nothing, and the sum of
/// the gross, which every payment of a claim asks to be nothing, has its units.
#[test]
fn t18_u3_an_option_that_entered_stops_its_account_and_its_shard_at_its_day_and_under_a_call() {
    use kpf_protocol::covered::{ALL, KIND_C_LEAVE, KIND_C_STEP, KIND_C_TAKE};
    let w = world();
    let base = w.bare();
    let k1 = key_of(&w, true, P10 / 10 * 11, DAY);
    let (s0, b0) = listed(&w, base.s, &Books::opened(), k1);
    let c1 = leg_entered(&oleg(&w, 0, 3, 1, k1.strike, P10, DAY, KAS1 / 2), T0);
    let long = leg_entered(&fleg(&w, 1, 10, LONG, P10), T0);
    let (s, b) = Shard { expiry: 0, ..s0.entered(&long, 1, T0).unwrap() }.entered_d(&b0, &c1, T0).unwrap();
    let shard = live(&base, s);
    let acct = cov(&w, &shard, &[long, c1], 50 * KAS1);
    let states = |t: i64| -> usize {
        [(KIND_C_TAKE, KAS1, 0i64), (KIND_C_LEAVE, 0, 1), (KIND_C_LEAVE, 0, 2), (KIND_C_LEAVE, 0, ALL), (KIND_C_STEP, 0, 1), (KIND_C_STEP, 0, ALL)].iter().filter(|(kind, amount, skip)| acct.s.c_state(*kind, *amount, *skip, &[P10, P10], t, acct.value as i64, xk(&w.w)).is_ok()).count()
    };
    let traced = |t: i64, call: bool| shard.s.traced_with(&b, &Draw { amount: 0, prices: [P10, P10, 0, 0].into() }, call, t).is_some();
    assert!(states(DAY - 1) >= 2, "before the day the account takes coins out and its futures leg leaves");
    assert!(traced(DAY - 1, false) && traced(DAY - 1, true), "before the day a draw and a call are traced");
    for t in [DAY, DAY + 1, DAY + 10_000_000] {
        assert_eq!(states(t), 0, "U3 is open: from its day on an account with an option leg states nothing (at {t})");
        assert!(!traced(t, false) && !traced(t, true), "U3 is open: from the day on no draw and no call of the shard is traced (at {t})");
    }
    // Under a call.
    let (cs, call) = shard.s.called_at(&[P10, P10, 0, 0], T0 + 1_000, 0);
    assert!(cs.called != 0 && cs.dgross == c1.size);
    assert!(acct.s.call_step(&call, 50 * KAS1).is_none(), "U3 is open: the call's step takes no account that has an option leg (KpfTCStep asks a way of 1 or 2)");
    let n = Left { sid: s.sid, legs: vec![c1], key: xk(&w.w), asked: KAS1 };
    assert!(cs.left_d(&b, &n, NOTE_MIN).is_err() && cs.left_class(&n, NOTE_LEFT, 0, NOTE_MIN).is_err(), "U3 is open: a note of an option leg is applied to a called shard by nothing");
    assert_eq!(cs.gone_at(&n), Some(0), "and it is spent by KpfTLeftVoid (U1)");
    // An entry of an option whose day is the block after the statement's: `KpfTEnterSD` takes it (run at the engine
    // in `t18_held_an_options_entry_with_one_thing_changed_is_refused`; the account's row is what is answered
    // wrong), and the books have the leg from the statement: from the next block no call of the shard is traced.
    let kn = Key { day: T0 + 1, ..k1 };
    let (sn, bn) = s0.row_in(&b0, kn, w.head(0).scale, w.head(0).price_max, 0, 5, Some(6)).unwrap();
    let near = leg_entered(&oleg(&w, 0, 3, 1, k1.strike, P10, T0 + 1, 0), T0);
    let (sn, bn) = sn.entered_d(&bn, &near, T0).expect("the shard takes an option of the next block");
    let d = Draw { amount: 0, prices: [P10, P10, 0, 0].into() };
    assert!(sn.traced_with(&bn, &d, true, T0).is_some() && sn.traced_with(&bn, &d, true, T0 + 1).is_none(), "U3 is open: one transaction of the owner's, and from the next block no call of its shard is traced");
}

// ================================================================================================================
// U4: what the gate bounds.

/// The gate is of the leg's row alone, against all the shard holds: a shard of 100 KAS takes, a row at a time, legs
/// whose rows are asked 100 KAS each. Eight rows: 800 KAS asked of 100.
#[test]
fn t18_u4_the_gate_holds_each_row_to_all_the_shard_holds() {
    let w = world();
    let base = w.bare();
    let (mut s, mut b) = (base.s, Books::opened());
    let mut asked = 0;
    for i in 0..8i64 {
        let k = key_of(&w, true, P10 / 100 * (130 + i), DAY);
        (s, b) = listed(&w, s, &b, k);
        // (Calls far out of the money, held by users: the row is asked the rate on its gross alone, 2.25 %.)
        let leg = leg_entered(&oleg(&w, 0, 3, 444, k.strike, P10, DAY, KAS1 / 100), T0);
        (s, b) = Shard { expiry: 0, ..s }.entered_d(&b, &leg, T0).unwrap_or_else(|e| panic!("row {i}: {e}"));
        asked += gate_of(&s, &b.row(&k).unwrap(), P10);
    }
    println!("U4: a shard that holds {} KAS has eight rows asked {} KAS together, each within the gate", s.held / KAS1, asked / KAS1);
    assert!(s.held == 100 * KAS1 && asked > 7 * s.held, "U4 is open: the rows are asked {asked} together of a shard that holds {}", s.held);
    let one_more = leg_entered(&oleg(&w, 0, 3, 2, P10 / 100 * 130, P10, DAY, 0), T0);
    assert!(Shard { expiry: 0, ..s }.entered_d(&b, &one_more, T0).is_err(), "and a row at what the shard holds takes no unit more");
}

// ================================================================================================================
// U5: the model's `c_marked` takes what the script refuses.

/// `c_marked` says what `KpfTEnterS` (and its twin) make of an account, `None` where the script refuses. Of a
/// written option whose entry adds another amount than its premium it says a marked account; `KpfTEnterSD`
/// refuses (run in `t18_held_an_options_entry_with_one_thing_changed_is_refused`); only `c_enter` has the rule.
#[test]
fn t18_u5_the_models_marked_account_of_a_written_option_that_adds_another_amount() {
    let w = world();
    let base = w.bare();
    let acct = cov(&w, &base, &[], 40 * KAS1);
    let written = oleg(&w, 0, 4, 10, P10, P10, DAY, 2 * KAS1);
    for add in [0, 20 * KAS1 - 1, 20 * KAS1 + 1] {
        assert!(acct.s.c_marked(&leg_entered(&written, T0), add, T0).is_some(), "U5 is open: c_marked takes a written option adding {add}");
        assert!(acct.s.c_enter(written, add, 0, &[], T0, acct.value as i64).is_err(), "c_enter states none");
    }
}

// ================================================================================================================
// The account's side of an option's entry.

/// An option enters an empty account and accounts with futures legs (of its market next to it, and apart) and
/// another option: the model finds no wrong point; at the engine the end and every row are answered right and the
/// other verdict refused; the price of the leg that enters is right within the tolerance of the committee's and
/// wrong beyond it. Then the account a sompi short of what it is asked: the end is wrong, at the engine.
#[test]
fn t18_held_the_accounts_side_of_an_options_entry() {
    let w = world();
    let h = w.head(0);
    let base = w.bare();
    let committee = w.markets[0].committee_id;
    let enter = w.script("KpfTEnterSD");
    let f = [fund(xk(&w.w), 33), fund(xk(&w.w), 34)];
    let ff = [fund(xk(&w.f), 36)];
    let (kc, kq) = (key_of(&w, true, P10 / 100 * 104, DAY), key_of(&w, false, P10 / 100 * 97, DAY));
    let (s1, b1) = listed(&w, base.s, &Books::opened(), kc);
    let (s1, bk) = listed(&w, s1, &b1, kq);
    let shard = live(&base, Shard { held: 50_000 * KAS1, ..s1 });
    let was = |l: XLeg| leg_entered(&l, 6_000);
    let other_opt = was(oleg(&w, 0, 5, 3, kq.strike, P10, DAY, KAS1 / 4));
    let (mut points, mut shorts) = (0, 0);
    for (what, legs, prices) in [
        ("an empty account", vec![], vec![]),
        ("beside a long of its market", vec![was(fleg(&w, 0, 20, LONG, P10 / 100 * 98))], vec![P10]),
        ("beside a short of another market", vec![was(fleg(&w, 1, 20, SHORT, P10 / 100 * 98))], vec![P10 / 100 * 99]),
        ("after a leg of its market and one apart", vec![was(fleg(&w, 1, 5, LONG, P10)), was(fleg(&w, 0, 7, SHORT, P10 / 100 * 101))], vec![P10 / 100 * 102, P10]),
        ("beside another option of its market", vec![other_opt], vec![P10]),
        ("beside a long and an option of its market", vec![was(fleg(&w, 0, 9, LONG, P10)), other_opt], vec![P10, P10]),
    ] {
        for way in 3..=6i64 {
            let key = if way <= 4 { kc } else { kq };
            let leg = oleg(&w, 0, way, 10, key.strike, P10, DAY, KAS1 / 2);
            let add = if way % 2 == 0 { 5 * KAS1 } else { 0 };
            // The least coins the account enters with, to the sompi (the model's end), and the entry at them.
            let stands = |coins: i64| cov(&w, &shard, &legs, coins).s.c_enter(leg, add, 0, &prices, T0, coins).is_ok();
            let (mut lo, mut hi) = (0i64, 4_000 * KAS1);
            assert!(stands(hi), "{what}, way {way}: an account of 4,000 KAS enters");
            while lo + 1 < hi {
                let mid = (lo + hi) / 2;
                if stands(mid) { hi = mid } else { lo = mid }
            }
            let least = hi;
            let acct = cov(&w, &shard, &legs, least);
            let (b, st, said, marked, after, _, _) = c_enter_state_d(&w.lib, &acct, &shard, &bk, &enter, SEED, committee, leg, add, &prices, T0, w.payer(&w.w, &f)).unwrap_or_else(|x| panic!("{what}, way {way}: {x}"));
            let sg = signed(&format!("{what}, way {way}: stated"), &b, &w.keys);
            let all: Vec<i64> = prices.iter().copied().chain([P10]).collect();
            assert_eq!(Account::c_enter_wrong_point(&marked, &said, &xk(&w.w), &st.outs, st.held, T0, &all), None, "{what}, way {way}: the model finds a wrong point");
            if !(legs.len() == 2 && way != 3) {
                let live_st = nth(&sg, 0, st, 6_510);
                let q = w.pointed(&live_st, 0);
                let (bb, right) = c_enter_end(&w.lib, &q, &marked, &said, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
                assert!(right && accepted(&bb, &w.keys), "{what}, way {way}: the end, right");
                let (bb, _) = c_enter_end(&w.lib, &q, &marked, &said, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.f, &ff), Some(false)).unwrap();
                assert!(!accepted(&bb, &w.keys), "{what}, way {way}: the end answered wrong is taken");
                points += 1;
                for i in 1..=marked.0.legs.len() {
                    let q = w.pointed(&live_st, 2 * i as i64 - 1);
                    let prev = if i >= 2 { said.rows[i - 2].g } else { [0; 32] };
                    let before = (i >= 2).then(|| &marked.0.legs[i - 2]);
                    let (bb, right) = c_enter_row(&w.lib, &q, &said, 2 * i as i64 - 1, &prev, before, &marked.0.legs[i - 1], &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
                    assert!(right && accepted(&bb, &w.keys), "{what}, way {way}: the row {i}, right");
                    let (bb, _) = c_enter_row(&w.lib, &q, &said, 2 * i as i64 - 1, &prev, before, &marked.0.legs[i - 1], &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.f, &ff), Some(false)).unwrap();
                    assert!(!accepted(&bb, &w.keys), "{what}, way {way}: the row {i} answered wrong is taken");
                    points += 1;
                }
                // Applied: the account with the leg.
                let bb = stated_settle(&w.lib, &live_st, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).unwrap();
                let ap = signed(&format!("{what}, way {way}: applied"), &bb, &w.keys);
                assert_eq!((&ap.tx.outputs[0].script_public_key, ap.tx.outputs[0].value as i64, after.legs.last().copied(), after.a_margin), (&hash_spk(&w.lib, &after.hash()), least + add, Some(leg_entered(&leg, T0)), least + add), "{what}, way {way}: the account with the leg and the coins added");
            }
            // A sompi short: stated by nobody; stated past the model, its end is wrong.
            let short = cov(&w, &shard, &legs, least - 1);
            assert!(short.s.c_enter(leg, add, 0, &prices, T0, least - 1).is_err());
            if legs.len() <= 1 {
                let (as_if, _) = as_if_entered(&shard.s, &bk, &leg_entered(&leg, T0), T0);
                let t = forced_entry(&w, &short, &shard, &bk, (&st, &said), &leg_entered(&leg, T0), add, &as_if, &f).expect("built");
                let sg = signed(&format!("{what}, way {way}: stated a sompi short"), &t, &w.keys);
                let m2 = short.s.c_marked(&leg_entered(&leg, T0), add, T0).unwrap();
                let said2 = CEnterSaid { add, leg: leg_entered(&leg, T0), ..said.clone() };
                let st2 = Stated { position: m2.hash(), arg: said2.arg(), bond: m2.0.c_bond(), held: short.value as i64 + add, ..st };
                let q = w.pointed(&nth(&sg, 0, st2, 6_510), 0);
                let (bb, right) = c_enter_end(&w.lib, &q, &m2, &said2, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
                assert!(!right && accepted(&bb, &w.keys), "{what}, way {way}: a sompi short, the end is wrong");
                let (bb, _) = c_enter_end(&w.lib, &q, &m2, &said2, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.f, &ff), Some(true)).unwrap();
                assert!(!accepted(&bb, &w.keys), "{what}, way {way}: a sompi short, the end answered right is taken");
                shorts += 1;
            }
            if legs.is_empty() {
                println!("an option alone, way {way} (10 units, strike {}, at 10, premium 0.5 a unit): the account enters with {least} sompi at the least ({} added by the entry)", key.strike, add);
            }
        }
    }
    println!("the account's side: {points} points answered right and not wrong; {shorts} entries a sompi short answered wrong at their end");
    // The price of the leg that enters: the committee's, within the tolerance.
    let acct = cov(&w, &shard, &[], 100 * KAS1);
    let (b, st, said, marked, _, _, _) = c_enter_state_d(&w.lib, &acct, &shard, &bk, &enter, SEED, committee, oleg(&w, 0, 3, 10, kc.strike, P10, DAY, KAS1 / 2), 0, &[], T0, w.payer(&w.w, &f)).unwrap();
    let live_st = nth(&signed("an entry for its price", &b, &w.keys), 0, st, 6_510);
    let q = w.pointed(&live_st, 2);
    let cite1 = w.script("KpfTCite");
    for (price, want) in [(P10 * 1_004 / 1_000, true), (P10 * 996 / 1_000, true), (P10 * 102 / 100, false), (P10 * 98 / 100, false)] {
        let c = w.cited(0, price);
        let (bb, right) = c_enter_price(&w.lib, &q, &marked, &said, &w.script("KpfTStepPrice"), (&cite1, &c), SEED, xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
        assert_eq!(right, want, "the price of an option's entry against the committee's {price}");
        assert!(accepted(&bb, &w.keys), "the price's answer, {price}");
        let other = c_enter_price(&w.lib, &q, &marked, &said, &w.script("KpfTStepPrice"), (&cite1, &c), SEED, xk(&w.f), w.payer(&w.f, &ff), Some(!want));
        assert!(other.is_err() || !accepted(&other.unwrap().0, &w.keys), "the other verdict of the price, {price}");
    }
    // The last row of an option exactly T after the statement's time is wrong whatever it sums; a block more, right.
    let t = said.head.t;
    for (day, want) in [(T0 + t + 1, true), (T0 + t, false), (T0 + 1, false)] {
        let free: Vec<usize> = (0..64).filter(|i| bk.leaves[*i].is_none()).collect();
        let (s9, b9) = shard.s.row_in(&bk, Key { day, ..kc }, h.scale, h.price_max, 0, free[0], Some(free[1])).unwrap();
        let sh9 = live(&base, s9);
        let leg = leg_entered(&oleg(&w, 0, 3, 10, kc.strike, P10, day, KAS1 / 2), T0);
        let (as_if, _) = as_if_entered(&s9, &b9, &leg, T0);
        // (Within T the rule states none: what a valid entry says, with this leg and its chain in the row.)
        let mut said9 = if want { acct.s.c_enter(oleg(&w, 0, 3, 10, kc.strike, P10, day, KAS1 / 2), 0, 0, &[], T0, acct.value as i64).expect("an option a block more than T before its day is stated").1 } else { CEnterSaid { leg, ..said.clone() } };
        said9.rows[0].g = kpf_protocol::covered::chain_of_one(&leg);
        assert_eq!(kpf_protocol::covered::c_enter_row_judged_at(T0, &[0; 32], None, &leg, &kpf_protocol::crow::CRow::ZERO, &said9.rows[0], 1, &said9), Some(want), "the model, a day {day}");
        let tx = forced_entry(&w, &acct, &sh9, &b9, (&st, &said9), &leg, 0, &as_if, &f).expect("built");
        let sg = signed(&format!("an option of the day {day} stated"), &tx, &w.keys);
        let m9 = acct.s.c_marked(&leg, 0, T0).unwrap();
        let st9 = Stated { position: m9.hash(), arg: said9.arg(), bond: m9.0.c_bond(), held: acct.value as i64, ..st };
        let q = w.pointed(&nth(&sg, 0, st9, 6_510), 1);
        let (bb, right) = c_enter_row(&w.lib, &q, &said9, 1, &[0; 32], None, &leg, &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
        assert_eq!(right, want, "the builder, a day {day}");
        assert!(accepted(&bb, &w.keys), "the row of an option of the day {day}, answered {want}");
        let (bb, _) = c_enter_row(&w.lib, &q, &said9, 1, &[0; 32], None, &leg, &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.f, &ff), Some(!want)).unwrap();
        assert!(!accepted(&bb, &w.keys), "the row of an option of the day {day}, the other verdict");
    }
}

// ================================================================================================================
// A whole life at the engine.

/// A series is listed (`KpfTRowIn`); one user holds it and another writes it (`KpfTEnterSD`, a lateness apart); the
/// two legs leave by their notes (`KpfTLeftD`); the row has no unit and nothing entered; the owner takes it out and
/// its day's leaf (`KpfTRowOut`). The shard is as it was opened, but for its time and the coins the notes moved.
#[test]
fn t18_held_a_series_whole_life_leaves_the_shard_as_it_was_opened() {
    let w = world();
    let h = w.head(0);
    let base = w.bare();
    let committee = w.markets[0].committee_id;
    let fc = [fund(xk(&w.c), 37)];
    let ff = [fund(xk(&w.f), 36)];
    let at = |sg: &OSigned, s: &Shard| sg.tx.outputs.iter().position(|o| o.script_public_key == hash_spk(&w.lib, &s.hash())).expect("the shard's output");
    let k = key_of(&w, true, P10 / 100 * 97, DAY);
    let (tx, s1, b1) = row_in(&w.lib, &base, &Books::opened(), &w.script("KpfTRowIn"), k, h.scale, h.price_max, 0, 1, Some(2), w.payer(&w.c, &fc)).unwrap();
    let sg = signed("the series listed", &tx, &w.keys);
    let sh1 = nth(&sg, at(&sg, &s1), s1, 6_400);
    // (Sizes that are no whole unit, in the money at the entry: what was entered is rounded, the same both ways.)
    let size = 7 * h.scale + h.scale / 3;
    let e = P10 + 333;
    let (held, written) = (XLeg { size, ..oleg(&w, 0, 3, 1, k.strike, e, DAY, KAS1 / 2) }, XLeg { size, ..oleg(&w, 0, 4, 1, k.strike, e, DAY, KAS1 / 2) });
    let acct_a = cov(&w, &sh1, &[], 200 * KAS1);
    let f1 = [fund(xk(&w.w), 33)];
    let (tx, _, _, _, _, s2, b2) = c_enter_state_d(&w.lib, &acct_a, &sh1, &b1, &w.script("KpfTEnterSD"), SEED, committee, held, 0, &[], T0, w.payer(&w.w, &f1)).unwrap();
    let sg = signed("the first user holds it", &tx, &w.keys);
    let sh2 = nth(&sg, at(&sg, &s2), s2, T0 as u64);
    let t1 = T0 + s2.t.late_daa;
    let f2 = [KeyUtxo { outpoint: op(38), amount: 1_000 * KAS1 as u64, daa: t1 as u64, key: xk(&w.w) }];
    let acct_b = OLive { op: op(81), ..cov(&w, &sh2, &[], 200 * KAS1) };
    let add = Marked::premium_added(&leg_entered(&written, t1)).unwrap();
    assert!(c_enter_state_d(&w.lib, &acct_b, &sh2, &b2, &w.script("KpfTEnterSD"), SEED, committee, written, add, &[], t1 - 1, w.payer(&w.w, &f2)).is_err(), "a second entry a block less than a lateness after the first");
    let (tx, _, _, _, _, s3, b3) = c_enter_state_d(&w.lib, &acct_b, &sh2, &b2, &w.script("KpfTEnterSD"), SEED, committee, written, add, &[], t1, w.payer(&w.w, &f2)).unwrap();
    let sg = signed("the second user writes it", &tx, &w.keys);
    let sh3 = nth(&sg, at(&sg, &s3), s3, t1 as u64);
    let row = b3.row(&k).unwrap();
    assert_eq!((row.net, row.gross, row.entered, s3.basis, s3.dgross), (0, 2 * size, 0, 0, 2 * size), "one holds and one writes: no net unit, nothing entered on balance");
    // The two leave by their notes: the first paid its premium, the second is paid 2 KAS.
    let na = Left { sid: s3.sid, legs: vec![leg_entered(&held, T0)], key: xk(&w.w), asked: -3 * KAS1 };
    let (tx, s4, b4, _) = left_apply_d(&w.lib, &sh3, &b3, &OLive { value: (NOTE_MIN + 3 * KAS1) as u64, ..left_note(&sh3, &na, 1) }, &w.script("KpfTLeftD"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("the first leg leaves", &tx, &w.keys);
    let sh4 = nth(&sg, at(&sg, &s4), s4, t1 as u64 + 10);
    let nb = Left { sid: s3.sid, legs: vec![leg_entered(&written, t1)], key: xk(&w.w), asked: 2 * KAS1 };
    let (tx, s5, b5, rest) = left_apply_d(&w.lib, &sh4, &b4, &left_note(&sh4, &nb, 2), &w.script("KpfTLeftD"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("the second leg leaves", &tx, &w.keys);
    assert!(rest.is_none() && pays(&sg, &xk(&w.w), 2 * KAS1));
    let sh5 = nth(&sg, at(&sg, &s5), s5, t1 as u64 + 20);
    let row = b5.row(&k).unwrap();
    assert_eq!((row.net, row.gross, row.entered, s5.basis, s5.dgross, b5.root()), (0, 0, 0, 0, 0, b1.root()), "the row has no unit and nothing entered: the tree as it was listed");
    // The owner takes the row out, and the day's leaf.
    let (tx, s6, b6) = row_out(&w.lib, &sh5, &b5, &w.script("KpfTRowOut"), &k, RowOutBy::Owner, w.payer(&w.c, &fc)).unwrap();
    let sg = signed("the row taken out", &tx, &w.keys);
    let sh6 = nth(&sg, at(&sg, &s6), s6, t1 as u64 + 30);
    let (tx, s7, b7) = row_out(&w.lib, &sh6, &b6, &w.script("KpfTRowOut"), &Key::day_of(k.market, k.day), RowOutBy::Owner, w.payer(&w.c, &fc)).unwrap();
    let sg = signed("the day's leaf taken out", &tx, &w.keys);
    assert_eq!((Shard { held: base.s.held, expiry: base.s.expiry, ..s7 }, b7.root(), s7.held, sg.tx.outputs[at(&sg, &s7)].value), (base.s, Books::opened().root(), base.s.held + KAS1, base.value + KAS1 as u64), "the shard as it was opened, with the 3 KAS it was paid less the 2 it paid");
}

// ================================================================================================================
// What the model says of an account with an option leg at several times, of a hand-over, a join and a split, and
// the sizes: printed (what the findings rest on is asserted in their tests).

#[test]
fn t18_held_the_models_words_and_the_sizes_printed() {
    let w = world();
    let base = w.bare();
    let k1 = key_of(&w, true, P10 / 10 * 11, DAY);
    let (s0, b0) = listed(&w, base.s, &Books::opened(), k1);
    let c4 = leg_entered(&oleg(&w, 0, 3, 4, k1.strike, P10, DAY, KAS1 / 2), T0);
    let (s, b) = s0.entered_d(&b0, &c4, T0).unwrap();
    let shard = live(&base, s);
    let acct = cov(&w, &shard, &[c4], 50 * KAS1);
    for t in [T0 + 1_000, DAY - 1, DAY, DAY + 1, DAY + 100_000] {
        for (kind, amount, skip) in [(kpf_protocol::covered::KIND_C_TAKE, KAS1, 0i64), (kpf_protocol::covered::KIND_C_LEAVE, 0, 1), (kpf_protocol::covered::KIND_C_LEAVE, 0, kpf_protocol::covered::ALL), (kpf_protocol::covered::KIND_C_STEP, 0, 1), (kpf_protocol::covered::KIND_C_STEP, 0, kpf_protocol::covered::ALL)] {
            let x = acct.s.c_state(kind, amount, skip, &[P10], t, acct.value as i64, xk(&w.w));
            println!("t {t} kind {kind} skip {skip}: {}", match &x { Ok(_) => "stated".to_string(), Err(e) => e.to_string() });
        }
        let d = shard.s.draw(KAS1, [P10, 0, 0, 0], t, shard.value as i64);
        println!("t {t} a draw: {}", match &d { Ok((_, dd)) => format!("stated; traced {}", shard.s.traced_with(&b, dd, false, t).is_some()), Err(e) => e.to_string() });
    }
    let fee = shard.s.t.keeper_fee_min.min(shard.s.held);
    let (cs, call) = shard.s.called_at(&[P10, 0, 0, 0], T0 + 1_000, fee);
    println!("called: dgross {} ; the account's step under the call: {:?}", cs.dgross, acct.s.call_step(&call, 50 * KAS1).map(|x| x.note.asked));
    let n = Left { sid: s.sid, legs: vec![c4], key: xk(&w.w), asked: KAS1 };
    println!("left_d on the called shard: {:?}; left_class: {:?}", cs.left_d(&b, &n, NOTE_MIN).err().map(|e| e.to_string()), cs.left_class(&n, NOTE_LEFT, 0, NOTE_MIN).err().map(|e| e.to_string()));
    println!("c_marked of a written option adding nothing: {}", acct.s.c_marked(&leg_entered(&oleg(&w, 0, 4, 1, k1.strike, P10, DAY, KAS1), T0), 0, T0).is_some());
    // A hand-over, a join and a split of an account with an option leg (the model).
    let t = T0 + 5_000;
    println!("passed: {:?}", acct.s.passed(xk(&w.f), t).map(|a| a.legs.len()).map_err(|e| e.to_string()));
    let other = Account { legs: vec![leg_entered(&fleg(&w, 1, 3, LONG, P10), T0)], a_min_counter: 1, ..acct.s.clone() };
    println!("joined: {:?}", acct.s.joined(&other, &shard.s).map(|a| a.legs.len()).map_err(|e| e.to_string()));
    let two = Account { legs: vec![leg_entered(&fleg(&w, 1, 3, LONG, P10), T0), c4], a_min_counter: 2, ..acct.s.clone() };
    for skip in [1i64, 2] {
        println!("split of the leg {skip}: {:?}", two.c_state(kpf_protocol::covered::KIND_C_SPLIT, 10 * KAS1, skip, &[P10, P10], t, two.a_margin, xk(&w.w)).map(|_| "stated").map_err(|e| e.to_string()));
    }
    // The sizes.
    let ff = [fund(xk(&w.f), 36)];
    let f = [fund(xk(&w.w), 33)];
    let empty = cov(&w, &live(&base, s0), &[], 50 * KAS1);
    let held = oleg(&w, 0, 3, 4, k1.strike, P10, DAY, KAS1 / 2);
    let (tx, _, _, _, _, s1, b1) = c_enter_state_d(&w.lib, &empty, &live(&base, s0), &b0, &w.script("KpfTEnterSD"), SEED, w.markets[0].committee_id, held, 0, &[], T0, w.payer(&w.w, &f)).unwrap();
    println!("size, an option's entry stated: {}", signed("stated", &tx, &w.keys).mass.size);
    let wr = empty.s.c_marked(&leg_entered(&oleg(&w, 0, 4, 10, k1.strike, P10, DAY, 2 * KAS1), T0), 20 * KAS1, T0).unwrap();
    let (tx, _, _) = enter_off(&w.lib, &OLive { s: wr.clone(), value: wr.0.a_margin as u64, id: empty.id, op: op(90), daa: 6_700 }, &w.script("KpfTEnterOff"), w.payer(&w.f, &ff)).unwrap();
    println!("size, an entry undone with its premium returned: {}", signed("undone", &tx, &w.keys).mass.size);
    let sh1 = live(&base, s1);
    let n1 = Left { sid: s1.sid, legs: vec![leg_entered(&held, T0)], key: xk(&w.w), asked: 0 };
    let (tx, _, _, _) = left_apply_d(&w.lib, &sh1, &b1, &left_note(&sh1, &n1, 1), &w.script("KpfTLeftD"), w.payer(&w.f, &ff)).unwrap();
    println!("size, a note of an option leg applied: {}", signed("left", &tx, &w.keys).mass.size);
    let long = leg_entered(&fleg(&w, 0, 10, LONG, P10), T0);
    let s2 = Shard { expiry: 0, ..s1 }.entered(&long, 0, T0).unwrap();
    let sh2 = live(&base, s2);
    let n2 = Left { sid: s1.sid, legs: vec![long, leg_entered(&held, T0)], key: xk(&w.w), asked: 0 };
    let (tx, _, _, _) = left_apply_d(&w.lib, &sh2, &b1, &left_note(&sh2, &n2, 2), &w.script("KpfTLeftAllD"), w.payer(&w.f, &ff)).unwrap();
    println!("size, a note of a futures leg and an option leg applied: {}", signed("left all", &tx, &w.keys).mass.size);
    let v = left_void(&w.lib, &sh1, &left_note(&sh1, &n1, 1), 0, &w.script("KpfTLeftVoid"), w.payer(&w.f, &ff)).unwrap();
    println!("size, a note of an option leg spent by KpfTLeftVoid: {}", signed("void", &v, &w.keys).mass.size);
}
