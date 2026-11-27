//! Review T17: the dated rows of a shard's books at the engine (docs/ARCHITECTURE-CLEARING.md 17.8j, steps A1 to A3):
//! the books' hash with a root, a row listed and taken out, the trace over dated rows. The reviewer's tests; its
//! report is docs/audit/REVIEW-T17-dated-rows.md. The world and its helpers are `thin_draw.rs`'s, copied.
//!
//! A test named `t17_vN_...` shows the finding VN as it stands: it passes while the finding is open. A test named
//! `t17_held_...` is of what was attacked and held.
#![allow(dead_code, unused_imports, clippy::too_many_arguments, clippy::type_complexity)]

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::books::{Books, Key, Row, OF_CALLS, OF_DAY, OF_PUTS};
use kpf_protocol::cite;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::cross::{XLeg, XTerms};
use kpf_protocol::oracle_build::{OBuilt, OLive, OPlan, OSigned};
use kpf_protocol::reserve::{trace_dated_point, DatedTrace, Draw, Priced, RowHead, RowOutBy, Shard, ShardRow, TraceRow, Traced, KIND_CALL, NOTE_MIN, SHARD_CARRIER};
use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::{Account, Out, Outs, Pointed, Stated, KIND_DRAW};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{Arg, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use silverscript_abi::ArtifactValue;
use std::cell::RefCell;

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;
const P10: i64 = 1_000_000_000;
const SEED: [u8; 32] = [0x5e; 32];
const BPS: i64 = 10_000;

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
    /// A shard of 100 KAS with no leg and no dated row, whose id is what an answer of a price holds it to.
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
    /// The statement `st` stated on the shard (its bytes `said`), landed at 6,510: `None` where `KpfTChange` refuses.
    fn states(&self, shard: &OLive<Shard>, st: &Stated, said: &[u8]) -> Option<OLive<Stated>> {
        let who = if st.claimer == xk(&self.c) { &self.c } else { &self.w };
        let f = [fund(xk(who), 30)];
        let s = &shard.s;
        let b = state_raw(&self.lib, shard, &s.fixed_hash(), &s.moving(), st, said, s.form(), &self.script("KpfTChange"), self.payer(who, &f)).ok()?;
        let sg = b.sign(&self.keys).ok()?;
        kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok().then(|| nth(&sg, 0, *st, 6_510))
    }
    fn stated(&self, shard: &OLive<Shard>, st: &Stated, said: &[u8]) -> OLive<Stated> {
        self.states(shard, st, said).expect("the statement is stated")
    }
    fn pointed(&self, st: &OLive<Stated>, point: i64) -> OLive<Pointed> {
        let f = [fund(xk(&self.w), 31)];
        let (b, q) = point_dispute(&self.lib, st, &self.script("KpfTPoint"), xk(&self.w), point, self.payer(&self.w, &f)).unwrap();
        let s = b.sign(&self.keys).expect("pointed");
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok(), "the point {point} is disputed");
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
    let mut markets: Vec<CMarket> = (0..4u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
    markets.sort_by_key(|m| Key::futures(m.id()).numbers());
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

const PRICES: [i64; 4] = [P10, P10, 0, 0];

// ================================================================================================================
// Review T17: the reviewer's own helpers.

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
fn i9(v: i64) -> Vec<u8> {
    [&[0x08][..], &sm8(v)].concat()
}
fn p33(x: &[u8; 32]) -> Vec<u8> {
    [&[0x20][..], x].concat()
}
fn num8(b: &[u8]) -> i64 {
    let mut x: [u8; 8] = b.try_into().expect("eight bytes");
    let below = x[7] & 0x80 != 0;
    x[7] &= 0x7f;
    let m = i64::from_le_bytes(x);
    if below { -m } else { m }
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
/// A leaf as a script is shown it, 138 bytes.
fn leaf138(r: &Row) -> Vec<u8> {
    r.bytes()[9..].to_vec()
}
fn beside_at(b: &Books, place: usize) -> Vec<u8> {
    b.beside(place).concat()
}
/// The shard `s` as an output stands on the chain, under the registry's id of `base`.
fn live(base: &OLive<Shard>, s: Shard) -> OLive<Shard> {
    OLive { s, value: (s.held + SHARD_CARRIER) as u64, ..base.clone() }
}

/// The shard of `a_dated_row_of_a_trace_is_answered_at_the_engine` (thin_draw.rs): ten futures short over forty in
/// market 0; of its market, 30 calls written at 10.3 and 20 puts held at 10.2 of one day, and a call of a later day
/// with no unit. Its books. (The units of the dated rows are written by hand: no leg enters one yet.)
fn dated_shard(w: &World) -> (OLive<Shard>, Books) {
    let h = w.head(0);
    let (scale, limit, market) = (h.scale, h.price_max, h.key.market);
    let base = w.bare();
    let (mut s, mut b) = (base.s, Books::opened());
    let keys = [Key { market, day: 900_000, of: OF_CALLS, strike: P10 / 100 * 103 }, Key { market, day: 900_000, of: OF_PUTS, strike: P10 / 100 * 102 }, Key { market, day: 1_800_000, of: OF_CALLS, strike: P10 }];
    for k in keys {
        let free: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_none()).collect();
        let stands = b.row(&Key::day_of(k.market, k.day)).is_some();
        (s, b) = s.row_in(&b, k, scale, limit, 0, free[0], (!stands).then_some(free[1])).unwrap();
    }
    for (k, net, gross) in [(keys[0], -30 * scale, 30 * scale), (keys[1], 20 * scale, 20 * scale)] {
        let at = b.place_of(&k).unwrap();
        b.leaves[at] = Some(Row { net, gross, ..b.leaves[at].unwrap() });
        s = Shard { root: b.root(), dgross: s.dgross + gross, ..s };
    }
    s.rows[0] = ShardRow { net: -10 * scale, gross: 40 * scale, ..s.rows[0] };
    s.basis = -100 * KAS1;
    (OLive { s, ..base }, b)
}

/// A draw of `amount` (or a call) of the shard at `prices`, as a statement and what it gives: the trace is the
/// caller's to say.
fn stated_of(w: &World, shard: &OLive<Shard>, call: bool, amount: i64, prices: [i64; 4]) -> (Stated, Draw) {
    let s = &shard.s;
    let (dst, dd) = s.draw(amount, prices, T0, shard.value as i64).unwrap();
    if call { (Stated { kind: KIND_CALL, bond: s.call_bond(), claimer: xk(&w.w), ..dst }, Draw { amount: 0, ..dd }) } else { (dst, dd) }
}

/// What the engine takes of the answer of the dated point `k` of the trace `t` by `KpfTShardRowD`: (right, wrong).
fn dated_taken(w: &World, live: &OLive<Stated>, s: &Shard, b: &Books, t: &Traced, k: i64) -> (bool, bool) {
    let f = [fund(xk(&w.w), 32)];
    let q = w.pointed(live, trace_dated_point(k as usize));
    let take = |v: bool| shard_row_d(&w.lib, &q, s, b, t, k, &w.script("KpfTShardRowD"), xk(&w.w), w.payer(&w.w, &f), Some(v)).is_ok_and(|(bb, _)| accepted(&bb, &w.keys));
    (take(true), take(false))
}

// ================================================================================================================
// V1. A dated point is a point "beyond the list" to `KpfTShardPrice`, which answers it right with nothing shown.

/// V1 (high). `KpfTShardPrice.answer` takes every point from 5 beyond the list's last price as "of nothing: right,
/// with nothing cited". The dated rows' points are 101 and above: they are such points. A dated row that is WRONG
/// (answered wrong at its own point by `KpfTShardRowD`, and right there by nobody) is answered RIGHT at the same
/// point by `KpfTShardPrice`, shown no leaf, no citation and no path: the statement stands again, the disputer has
/// lost its bond, and the answerer is paid of it. So of a draw's or a call's trace no dated row is held by its point
/// unless the honest answer lands first; and a trace whose dated rows ask nothing is right at its end by its own
/// sums.
#[test]
fn t17_v1_a_wrong_dated_row_is_answered_right_at_its_own_point_by_the_prices_script() {
    let w = world();
    let (shard, b) = dated_shard(&w);
    let s = &shard.s;
    let f = [fund(xk(&w.c), 33)];
    let price = w.script("KpfTShardPrice");
    let mut taken = 0;
    for call in [false, true] {
        let (st0, d) = stated_of(&w, &shard, call, KAS1, PRICES);
        let t = s.traced_with(&b, &d, call, T0).unwrap();
        let nd = t.dated.len() as i64;
        // What is said otherwise than the books: what the written calls are asked a sompi less; nothing asked of
        // any row; the held puts worth a KAS more; a row left out; and no dated row said at all.
        let mut less = t.clone();
        less.dated[1].ro -= 1;
        let mut none = t.clone();
        for r in none.dated.iter_mut() {
            r.ro = 0;
        }
        let mut worth = t.clone();
        for r in worth.dated.iter_mut().skip(2) {
            r.sn += KAS1;
        }
        let mut gap = t.clone();
        gap.dated.remove(2);
        let mut blind = t.clone();
        blind.dated.clear();
        for (what, lie, k) in [("asked a sompi less", &less, 2i64), ("nothing asked of any row", &none, 2), ("the held puts worth a KAS more", &worth, 3), ("a row left out", &gap, 3), ("no dated row said", &blind, 1)] {
            let st = Stated { arg: lie.arg(), ..st0 };
            let live = w.stated(&shard, &st, &lie.said());
            // (Control: the row is wrong at its point by the dated rows' own script, and right there by nobody.)
            assert_eq!(s.trace_dated_judged(&b, lie, k as usize, call, T0), Some(false), "{what}: the model");
            assert_eq!(dated_taken(&w, &live, s, &b, lie, k), (false, true), "{what}: KpfTShardRowD");
            // Closed: the prices' script answers no point from 101. Its builder refuses; and at the engine an answer
            // of the point made as the point 100's ("of nothing: right") is refused, with the place 100 and with
            // the place the point's.
            let point = trace_dated_point(k as usize);
            let q = w.pointed(&live, point);
            assert!(shard_price_t(&w.lib, &q, s, SEED, lie, point, &price, None, xk(&w.c), w.payer(&w.c, &f), None).is_err(), "V1 (closed): {what}: the builder");
            let (mut bb, _) = shard_price_t(&w.lib, &q, s, SEED, lie, 100, &price, None, xk(&w.c), w.payer(&w.c, &f), Some(true)).unwrap();
            assert!(!accepted(&bb, &w.keys), "V1 (closed): {what}: the dated point {k} answered as the point 100");
            let args = args_of(&mut bb, 1);
            let at = (1..args.len()).find(|i| matches!((&args[*i - 1], &args[*i]), (Arg::V(ArtifactValue::Int(x)), Arg::V(ArtifactValue::Int(100))) if *x == point)).expect("the point and the place answered");
            args[at] = int_arg(point);
            assert!(!accepted(&bb, &w.keys), "V1 (closed): {what}: the dated point {k} answered right by KpfTShardPrice");
            let _ = &st;
            taken += 1;
        }
        // Every dated point of the trace that asks nothing, and the point beyond the last, the same way.
        let st = Stated { arg: none.arg(), ..st0 };
        let live = w.stated(&shard, &st, &none.said());
        for k in 1..=nd + 1 {
            let q = w.pointed(&live, trace_dated_point(k as usize));
            assert!(shard_price_t(&w.lib, &q, s, SEED, &none, trace_dated_point(k as usize), &price, None, xk(&w.c), w.payer(&w.c, &f), None).is_err(), "V1 (closed): the dated point {k} of a trace that asks nothing");
            taken += 1;
        }
        // (A point from 9 to 100 is still of nothing, and the prices' script's: right, with nothing cited.)
        let q = w.pointed(&live, 100);
        let (bb, right) = shard_price_t(&w.lib, &q, s, SEED, &none, 100, &price, None, xk(&w.c), w.payer(&w.c, &f), None).unwrap();
        assert!(right && accepted(&bb, &w.keys), "the point 100 is of nothing");
        // And a point beyond the one after the last row is of nothing, and the dated rows' script's: right, by
        // the model and at the engine, so that no point is one nobody answers.
        let st = Stated { arg: t.arg(), ..st0 };
        let live = w.stated(&shard, &st, &t.said());
        for k in [nd + 2, nd + 40] {
            assert_eq!(s.trace_dated_judged(&b, &t, k as usize, call, T0), Some(true));
            assert_eq!(dated_taken(&w, &live, s, &b, &t, k), (true, false), "V1 (closed): the point beyond the last, {k}");
        }
    }
    // What it is worth: the draw that the four rows alone allow, said with dated rows that ask nothing, is right at
    // its end by its own sums (the end reads what the last row says), and every dated row of it is "right" above.
    let (st0, d) = stated_of(&w, &shard, false, KAS1, PRICES);
    let t = s.traced_with(&b, &d, false, T0).unwrap();
    let mut none = t.clone();
    for r in none.dated.iter_mut() {
        r.ro = 0;
    }
    let most = |t: &Traced| (1..=200i64).rev().map(|x| x * KAS1 / 2).find(|a| s.draw_trace_end_judged(&Traced { amount: *a, ..t.clone() }, &s.owner, T0) == Some(true)).unwrap_or(0);
    let (with, without) = (most(&t), most(&none));
    assert!(without > with && with > 0);
    let (st2, d2) = stated_of(&w, &shard, false, without, PRICES);
    let mut t2 = s.traced_with(&b, &d2, false, T0).unwrap();
    for r in t2.dated.iter_mut() {
        r.ro = 0;
    }
    let st = Stated { arg: t2.arg(), ..st2 };
    let live = w.stated(&shard, &st, &t2.said());
    let q = w.pointed(&live, 0);
    let (bb, right) = shard_end_t(&w.lib, &q, s, &t2, &w.script("KpfTShardEnd"), xk(&w.c), w.payer(&w.c, &f), None).unwrap();
    assert!(right && accepted(&bb, &w.keys), "the end of a draw whose dated rows ask nothing is right by its sums");
    // (Closed: and it is wrong at its dated rows, by the one script that answers them.)
    assert_eq!(dated_taken(&w, &live, s, &b, &t2, 2), (false, true), "V1 (closed): the row that asks nothing is answered wrong, and right by nobody");
    let _ = st0;
    println!("V1 (closed): {taken} wrong dated points that KpfTShardPrice answered right are answered by it no more; a draw of {without} sompi, where the dated rows allow {with}, is wrong at its rows");
}

// ================================================================================================================
// 1. `KpfTRowIn` and `KpfTRowOut` against the model, at the engine.

const SCALE_MAX: i64 = 1_000_000_000;
const MARGIN_MAX: i64 = i64::MAX / 8;

fn key_of(b: &[u8]) -> Key {
    Key { market: b[1..33].try_into().unwrap(), day: num8(&b[34..42]), of: num8(&b[43..51]), strike: num8(&b[52..60]) }
}
fn z27() -> Vec<u8> {
    [i9(0), i9(0), i9(0)].concat()
}

/// The root a script computes of a leaf shown at a place with the hashes beside its way: written here from the
/// documents (17.6), not taken of the model. `None` where a script refuses the path before it hashes.
fn script_root(leaf: &[u8], place: i64, beside: &[u8]) -> Option<[u8; 32]> {
    if !(0..=63).contains(&place) || beside.len() != 192 || (!leaf.is_empty() && leaf.len() != 138) {
        return None;
    }
    let mut h = if leaf.is_empty() { [0u8; 32] } else { blake2b(&[&i9(20)[..], leaf].concat()) };
    let mut p = place;
    for d in 0..6 {
        let s = &beside[32 * d..32 * d + 32];
        h = if p % 2 == 0 { blake2b(&[&i9(21)[..], &h, s].concat()) } else { blake2b(&[&i9(21)[..], s, &h].concat()) };
        p /= 2;
    }
    Some(h)
}

/// What `KpfTRowIn.put` is shown beside the shard.
#[derive(Clone, Debug)]
struct In {
    leafb: Vec<u8>,
    pb: i64,
    besideb: Vec<u8>,
    knext: Vec<u8>,
    day: i64,
    of: i64,
    strike: i64,
    market: Vec<u8>,
    scale: i64,
    limit: i64,
    r: i64,
    pe: i64,
    besidee: Vec<u8>,
    mode: i64,
    leafd: Vec<u8>,
    pd: i64,
    besided: Vec<u8>,
    signer: [u8; 32],
}

/// What an honest owner shows to list `key` (the leaf before it in the keys' order with the key that leaf names; the
/// market's row, or the first empty one; the day's leaf where it stands, else a second empty place), at the places
/// `pe` and `pd`. For a key that is in the books, or with no free place, it is what such an owner would try.
fn in_honest(s: &Shard, b: &Books, key: Key, scale: i64, limit: i64, pe: usize, pd: usize) -> In {
    let rows = b.in_order();
    let before = *rows.iter().rev().find(|r| r.key.below(&key)).unwrap_or(&rows[0]);
    let pb = b.place_of(&before.key).unwrap();
    let named = rows.iter().find(|r| r.key != Key::HEAD && r.key.hash() == before.next).map_or(Key::HEAD, |r| r.key);
    let mut b1 = b.clone();
    b1.leaves[pb] = Some(Row { next: key.hash(), ..before });
    let day = Key::day_of(key.market, key.day);
    let r = s.rows.iter().take(4).position(|x| x.key == Key::futures(key.market)).or_else(|| s.rows.iter().take(4).position(|x| *x == ShardRow::EMPTY)).unwrap_or(0);
    let (mode, leafd, pd, besided) = match b.shown(&day) {
        Some(sd) => (0, leaf138(&sd.row), sd.place, beside_at(b, sd.place)),
        None => {
            let mut b2 = b1.clone();
            b2.leaves[pe] = Some(Row { key, scale, price_max: limit, net: 0, gross: 0, entered: 0, next: day.hash() });
            (1, vec![0u8; 138], pd, beside_at(&b2, pd))
        }
    };
    In { leafb: leaf138(&before), pb: pb as i64, besideb: beside_at(b, pb), knext: named.bytes(), day: key.day, of: key.of, strike: key.strike, market: key.market.to_vec(), scale, limit, r: r as i64, pe: pe as i64, besidee: beside_at(&b1, pe), mode, leafd, pd: pd as i64, besided, signer: s.owner }
}

/// `KpfTRowIn.put` as the documents say it (17.8j, 2 to 4; "A2 as it is built"), on the shard `s` whose books are
/// `b`: said of the books, not of the script's hashes.
fn in_spec(s: &Shard, b: &Books, a: &In) -> bool {
    if s.called != 0 || a.signer != s.owner || a.market.len() != 32 || a.leafb.len() != 138 || a.knext.len() != 60 || a.leafd.len() != 138 {
        return false;
    }
    let m: [u8; 32] = a.market.clone().try_into().unwrap();
    let fk = Key::futures(m);
    if !fk.well_made() || a.day <= 0 || !(a.of == 1 || a.of == 2) || a.strike <= 0 || a.strike > a.limit || !(0..=3).contains(&a.r) {
        return false;
    }
    let row = s.rows[a.r as usize];
    let head = RowHead { key: fk, scale: a.scale, price_max: a.limit };
    if row.head() != head && (row != ShardRow::EMPTY || s.rows.iter().take(4).any(|x| x.key == fk) || a.scale <= 0 || a.scale > SCALE_MAX || a.limit <= 0 || a.limit > MARGIN_MAX) {
        return false;
    }
    if row.entered < 0 {
        return false;
    }
    let key = Key { market: m, day: a.day, of: a.of, strike: a.strike };
    // The leaf before: a leaf of the books at its place, below the key, naming the key said, which is above the
    // key or the key of nothing.
    if !(0..=63).contains(&a.pb) || !(0..=63).contains(&a.pe) || !(0..=63).contains(&a.pd) {
        return false;
    }
    let (pb, pe, pd) = (a.pb as usize, a.pe as usize, a.pd as usize);
    let Some(lb) = b.leaves[pb] else { return false };
    if a.leafb != leaf138(&lb) || a.besideb != beside_at(b, pb) || !lb.key.below(&key) || blake2b(&a.knext) != lb.next {
        return false;
    }
    let kn = key_of(&a.knext);
    if a.knext != Key::HEAD.bytes() && !key.below(&kn) {
        return false;
    }
    let mut b1 = b.clone();
    b1.leaves[pb] = Some(Row { next: key.hash(), ..lb });
    if b1.leaves[pe].is_some() || a.besidee != beside_at(&b1, pe) {
        return false;
    }
    let stands = (lb.key.market == m && lb.key.day == a.day) || (kn.market == m && kn.day == a.day);
    match a.mode {
        1 => {
            let mut b2 = b1.clone();
            b2.leaves[pe] = Some(Row { key, scale: a.scale, price_max: a.limit, net: 0, gross: 0, entered: 0, next: Key::day_of(m, a.day).hash() });
            !stands && b2.leaves[pd].is_none() && a.besided == beside_at(&b2, pd)
        }
        0 => {
            let Some(ld) = b.leaves[pd] else { return false };
            a.leafd == leaf138(&ld) && a.besided == beside_at(b, pd) && ld.key == Key::day_of(m, a.day) && (ld.scale, ld.price_max, ld.net, ld.gross, ld.entered) == (a.scale, a.limit, 0, 0, 0)
        }
        _ => false,
    }
}

/// The shard `KpfTRowIn.put` writes for what it is shown, whatever its checks say: the root of the last leaf it
/// puts in, and the row named counting one leaf more (two with the day's leaf).
fn in_after(s: &Shard, a: &In) -> Option<Shard> {
    let m: [u8; 32] = a.market.clone().try_into().ok()?;
    if a.leafb.len() != 138 || !(0..=3).contains(&a.r) {
        return None;
    }
    let key = Key { market: m, day: a.day, of: a.of, strike: a.strike };
    let blank = [i9(a.scale), i9(a.limit), z27()].concat();
    let root = if a.mode == 1 { script_root(&[Key::day_of(m, a.day).bytes(), blank, a.leafb[105..138].to_vec()].concat(), a.pd, &a.besided)? } else { script_root(&[key.bytes(), blank, a.leafb[105..138].to_vec()].concat(), a.pe, &a.besidee)? };
    let row = s.rows[a.r as usize];
    let head = RowHead { key: Key::futures(m), scale: a.scale, price_max: a.limit };
    let count = if row.head() == head { row.entered } else { 0 } + if a.mode == 1 { 2 } else { 1 };
    let mut n = *s;
    n.rows[a.r as usize] = ShardRow { entered: count, ..ShardRow::of(head, row.net, row.gross) };
    n.root = root;
    Some(n)
}

/// The transactions of these tests are one honest transaction of each script with the shard, what is shown and the
/// output written over: the shard always holds what the template's does.
struct Tmpl {
    put: OBuilt,
    out: OBuilt,
}

fn tmpl(w: &World) -> Tmpl {
    let h = w.head(0);
    let base = w.bare();
    let b0 = Books::opened();
    let f = [fund(xk(&w.c), 61)];
    let call = Key { market: h.key.market, day: 900_000, of: OF_CALLS, strike: P10 };
    let (put, s1, b1) = row_in(&w.lib, &base, &b0, &w.script("KpfTRowIn"), call, h.scale, h.price_max, 0, 1, Some(2), w.payer(&w.c, &f)).unwrap();
    assert!(accepted(&put, &w.keys), "the template of a row listed");
    let (out, _, _) = row_out(&w.lib, &live(&base, s1), &b1, &w.script("KpfTRowOut"), &call, RowOutBy::Owner, w.payer(&w.c, &f)).unwrap();
    assert!(accepted(&out, &w.keys), "the template of a row taken out");
    Tmpl { put, out }
}

fn in_tx(w: &World, t: &Tmpl, s: &Shard, a: &In, after: &[u8; 32]) -> OBuilt {
    let mut b = t.put.clone();
    object_made(&mut b, &w.lib, 0, &s.hash());
    let args = args_of(&mut b, 1);
    assert_eq!(args.len(), 25, "put: twenty-five arguments");
    let new = [
        bytes_arg(&s.terms()), bytes_arg(&s.moving()), bytes_arg(&s.rows_bytes()), bytes_arg(&s.books_head()), bytes_arg(&a.leafb), int_arg(a.pb), bytes_arg(&a.besideb), bytes_arg(&a.knext), int_arg(a.day), int_arg(a.of), int_arg(a.strike),
        bytes_arg(&a.market), int_arg(a.scale), int_arg(a.limit), int_arg(a.r), int_arg(a.pe), bytes_arg(&a.besidee), int_arg(a.mode), bytes_arg(&a.leafd), int_arg(a.pd), bytes_arg(&a.besided), Arg::Sig(a.signer),
    ];
    for (i, x) in new.into_iter().enumerate() {
        args[3 + i] = x;
    }
    b.tx.outputs[0].script_public_key = hash_spk(&w.lib, after);
    b
}
/// Whether the engine takes `put` shown `a`, the shard after written as the script computes it from what it is shown
/// (so that a refusal is a check's own).
fn in_taken(w: &World, t: &Tmpl, s: &Shard, a: &In) -> bool {
    let after = in_after(s, a).map_or([0xee; 32], |n| n.hash());
    accepted(&in_tx(w, t, s, a, &after), &w.keys)
}

/// What `KpfTRowOut.out` is shown beside the shard.
#[derive(Clone, Debug)]
struct Outw {
    leafr: Vec<u8>,
    pr: i64,
    besider: Vec<u8>,
    leafb: Vec<u8>,
    pb: i64,
    besideb: Vec<u8>,
    r: i64,
    mode: i64,
    leafd: Vec<u8>,
    pd: i64,
    besided: Vec<u8>,
    signer: Option<[u8; 32]>,
}

/// What is shown to take the leaf at `pr` out in the mode `mode` (0 signed by `signer`).
fn out_honest(s: &Shard, b: &Books, pr: usize, mode: i64, signer: Option<[u8; 32]>) -> Outw {
    let lr = b.leaves[pr].expect("a leaf");
    let pb = b.leaves.iter().position(|l| l.is_some_and(|x| x.next == lr.key.hash() && lr.key != Key::HEAD)).unwrap_or(0);
    let mut b1 = b.clone();
    b1.leaves[pr] = None;
    let r = s.rows.iter().take(4).position(|x| x.key == Key::futures(lr.key.market)).unwrap_or(0);
    let (leafd, pd, besided) = match b.shown(&Key::day_of(lr.key.market, lr.key.day)) {
        Some(sd) => (leaf138(&sd.row), sd.place as i64, beside_at(b, sd.place)),
        None => (vec![0u8; 138], 0, vec![0u8; 192]),
    };
    Outw { leafr: leaf138(&lr), pr: pr as i64, besider: beside_at(b, pr), leafb: b1.leaves[pb].map_or(vec![0u8; 138], |x| leaf138(&x)), pb: pb as i64, besideb: beside_at(&b1, pb), r: r as i64, mode, leafd, pd, besided, signer }
}

/// `KpfTRowOut.out` as the documents say it, said of the books.
fn out_spec(s: &Shard, b: &Books, a: &Outw) -> bool {
    if a.leafr.len() != 138 || a.leafb.len() != 138 || a.leafd.len() != 138 || !(0..=63).contains(&a.pr) || !(0..=63).contains(&a.pb) || !(0..=3).contains(&a.r) {
        return false;
    }
    let (pr, pb) = (a.pr as usize, a.pb as usize);
    let Some(lr) = b.leaves[pr] else { return false };
    if a.leafr != leaf138(&lr) || a.besider != beside_at(b, pr) || lr.key == Key::HEAD {
        return false;
    }
    let mut b1 = b.clone();
    b1.leaves[pr] = None;
    let Some(lb) = b1.leaves[pb] else { return false };
    if a.leafb != leaf138(&lb) || a.besideb != beside_at(&b1, pb) || lb.next != lr.key.hash() {
        return false;
    }
    if lr.key.of == OF_DAY {
        if lb.key.market == lr.key.market && lb.key.day == lr.key.day {
            return false;
        }
    } else {
        if !(lr.key.of == 1 || lr.key.of == 2) || (lr.net, lr.gross, lr.entered) != (0, 0, 0) {
            return false;
        }
        let may = match a.mode {
            0 => a.signer == Some(s.owner),
            1 => {
                (0..=63).contains(&a.pd) && b.leaves[a.pd as usize].is_some_and(|ld| a.leafd == leaf138(&ld) && a.besided == beside_at(b, a.pd as usize) && ld.key == Key::day_of(lr.key.market, lr.key.day) && (ld.net == 1 || ld.entered > 0))
            }
            2 => s.called != 0,
            _ => false,
        };
        if !may {
            return false;
        }
    }
    let row = s.rows[a.r as usize];
    row.key == Key::futures(lr.key.market) && row.entered >= 1
}

/// The shard `KpfTRowOut.out` writes for what it is shown, whatever its checks say.
fn out_after(s: &Shard, a: &Outw) -> Option<Shard> {
    if a.leafr.len() != 138 || a.leafb.len() != 138 || !(0..=3).contains(&a.r) {
        return None;
    }
    let root = script_root(&[&a.leafb[..105], &a.leafr[105..138]].concat(), a.pb, &a.besideb)?;
    let row = s.rows[a.r as usize];
    let mut n = *s;
    n.rows[a.r as usize] = if row.entered == 1 && row.net == 0 && row.gross == 0 { ShardRow::EMPTY } else { ShardRow { entered: row.entered - 1, ..row } };
    n.root = root;
    Some(n)
}

fn out_tx(w: &World, t: &Tmpl, s: &Shard, a: &Outw, after: &[u8; 32]) -> OBuilt {
    let mut b = t.out.clone();
    object_made(&mut b, &w.lib, 0, &s.hash());
    let args = args_of(&mut b, 1);
    assert_eq!(args.len(), 19, "out: nineteen arguments");
    let new = [
        bytes_arg(&s.terms()), bytes_arg(&s.moving()), bytes_arg(&s.rows_bytes()), bytes_arg(&s.books_head()), bytes_arg(&a.leafr), int_arg(a.pr), bytes_arg(&a.besider), bytes_arg(&a.leafb), int_arg(a.pb), bytes_arg(&a.besideb), int_arg(a.r), int_arg(a.mode),
        bytes_arg(&a.leafd), int_arg(a.pd), bytes_arg(&a.besided), a.signer.map_or(bytes_arg(&[0u8; 65]), Arg::Sig),
    ];
    for (i, x) in new.into_iter().enumerate() {
        args[3 + i] = x;
    }
    b.tx.outputs[0].script_public_key = hash_spk(&w.lib, after);
    b
}
fn out_taken(w: &World, t: &Tmpl, s: &Shard, a: &Outw) -> bool {
    let after = out_after(s, a).map_or([0xee; 32], |n| n.hash());
    accepted(&out_tx(w, t, s, a, &after), &w.keys)
}

/// Markets of every kind of number: the world's four, and made ones (numbers below zero, a first number of nothing,
/// a top bit in one number only).
fn pool(w: &World) -> Vec<[u8; 32]> {
    let mut v: Vec<[u8; 32]> = (0..4).map(|i| w.markets[i].id()).collect();
    v.extend([[0x01; 32], [0xff; 32], [0x7f; 32]]);
    for (i, hi) in [(7usize, 0x80u8), (15, 0x81), (23, 0xc0)] {
        let mut m = [0u8; 32];
        m[i - 7] = 3;
        m[i] = hi;
        m[30] = 1;
        v.push(m);
    }
    let mut m = [0u8; 32];
    m[31] = 0x01;
    v.push(m);
    v
}

const DAYS: [i64; 4] = [5_000, 900_000, 900_001, 1_800_000];

/// A shard and its books at random: up to four markets of the pool with a row among the four (some with futures
/// units, some heads of no unit), `fill` listings tried by the model's `row_in` at random places, then units written
/// by hand into some dated rows (a net, a gross, something entered) and some days' leaves written (a price, or
/// "none"). The markets it was made over (some have no row), each with its scale and limit.
fn random_books(g: &mut Rng, w: &World, fill: usize) -> (Shard, Books, Vec<([u8; 32], i64, i64)>) {
    let all = pool(w);
    let h = w.head(0);
    let mut ms: Vec<([u8; 32], i64, i64)> = vec![];
    while ms.len() < 5 {
        let m = all[g.below(all.len() as i64) as usize];
        if !ms.iter().any(|x| x.0 == m) {
            let (scale, limit) = match g.below(4) {
                0 => (1, MARGIN_MAX),
                1 => (SCALE_MAX, 1_000 * P10),
                _ => (h.scale, h.price_max),
            };
            ms.push((m, scale, limit));
        }
    }
    let mut s = w.bare().s;
    let mut b = Books::opened();
    // (Some markets have their futures row with units before anything is listed.)
    for (i, (m, scale, limit)) in ms.iter().enumerate().take(4) {
        if g.below(3) == 0 {
            let gross = g.range(1, 50) * scale;
            s.rows[i] = ShardRow::futures(*m, *scale, *limit, g.range(-1, 1) * gross, gross);
        }
    }
    for _ in 0..fill {
        let (m, scale, limit) = ms[g.below(ms.len() as i64) as usize];
        let key = Key { market: m, day: DAYS[g.below(4) as usize], of: 1 + g.below(2), strike: (1 + g.below(6)).min(limit) };
        let free: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_none()).collect();
        if free.len() < 2 {
            break;
        }
        let (pe, pd) = (free[g.below(free.len() as i64) as usize], free[g.below(free.len() as i64) as usize]);
        let Some(r) = s.rows.iter().take(4).position(|x| x.key == Key::futures(m)).or_else(|| s.rows.iter().take(4).position(|x| *x == ShardRow::EMPTY)) else { continue };
        let stands = b.row(&Key::day_of(m, key.day)).is_some();
        if let Ok((s2, b2)) = s.row_in(&b, key, scale, limit, r, pe, (!stands).then_some(pd)) {
            (s, b) = (s2, b2);
        }
    }
    // (Some rows are taken out again by the model: days' leaves stand whose day has no row.)
    for i in 0..64 {
        let Some(r) = b.leaves[i] else { continue };
        if r.key.is_dated() && g.below(5) == 0 {
            if let Ok((s2, b2)) = s.row_out(&b, &r.key, RowOutBy::Owner) {
                (s, b) = (s2, b2);
            }
        }
    }
    for i in 0..64 {
        let Some(r) = b.leaves[i] else { continue };
        if r.key.is_dated() && g.below(4) == 0 {
            let gross = g.range(0, 9) * r.scale;
            let (net, entered) = match g.below(4) {
                0 => (0, g.range(1, 9)),
                1 => (gross, 0),
                2 => (-gross, -g.range(0, 9)),
                _ => (0, 0),
            };
            b.leaves[i] = Some(Row { net, gross, entered, ..r });
            s.dgross += gross;
        } else if r.key.is_day() && g.below(4) == 0 {
            b.leaves[i] = Some(if g.below(2) == 0 { Row { entered: g.range(1, 9), ..r } } else { Row { net: 1, ..r } });
        }
    }
    s.root = b.root();
    (s, b, ms)
}

/// Whether the books and the four rows hold what 17.8j, 2 to 4 say of them: the leaves a list from the head in the
/// keys' order; a day's leaves next to one another, its day's leaf the last; every dated row with its day's leaf, at
/// its market's head's scale and limit; a market's futures row counts its leaves and stands exactly while it counts
/// any or has a unit; no market twice among the four; the root the shard's.
fn books_held(s: &Shard, b: &Books) -> bool {
    let rows = b.in_order();
    let mut ok = b.well_linked() && b.root() == s.root;
    for r in rows.iter().skip(1) {
        let head = s.rows.iter().take(4).find(|x| x.key == Key::futures(r.key.market));
        ok &= head.is_some_and(|h| (h.scale, h.price_max) == (r.scale, r.price_max));
        ok &= r.key.is_dated() || r.key.is_day();
        if r.key.is_dated() {
            ok &= b.row(&Key::day_of(r.key.market, r.key.day)).is_some();
        }
    }
    for (i, x) in s.rows.iter().take(4).enumerate() {
        if x.no_market() {
            ok &= *x == ShardRow::EMPTY;
            continue;
        }
        ok &= x.entered == b.count_of(&x.key.market) && (x.entered > 0 || x.gross != 0 || x.net != 0);
        ok &= !s.rows.iter().take(4).enumerate().any(|(j, y)| j != i && y.key == x.key);
    }
    ok
}

/// One thing changed in what `put` is shown. The name of the change.
fn key_in(a: &In) -> Key {
    Key { market: a.market.clone().try_into().expect("a market"), day: a.day, of: a.of, strike: a.strike }
}

fn in_changed(g: &mut Rng, s: &Shard, b: &Books, a: &In, which: i64, other: [u8; 32]) -> (In, &'static str) {
    let mut x = a.clone();
    let leaves: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_some()).collect();
    let free: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_none()).collect();
    let any_leaf = leaves[g.below(leaves.len() as i64) as usize];
    let any_free = free.get(g.below(free.len().max(1) as i64) as usize).copied().unwrap_or(any_leaf);
    let flip = |v: &mut Vec<u8>, g: &mut Rng| {
        let i = g.below(v.len() as i64) as usize;
        v[i] ^= 1 << g.below(8);
    };
    let what = match which {
        0 => {
            x.pb = [any_leaf as i64, any_free as i64, a.pb + 64, a.pb - 64, -1, 64][g.below(6) as usize];
            "the leaf before at another place"
        }
        1 => {
            flip(&mut x.besideb, g);
            "a hash beside the leaf before moved"
        }
        2 => {
            // (Another leaf of the books as the leaf before, with its own path and the key it truly names: a key
            // between is left out, or the list would be out of its order.)
            let l = b.leaves[any_leaf].unwrap();
            x.leafb = leaf138(&l);
            x.pb = any_leaf as i64;
            x.besideb = beside_at(b, any_leaf);
            x.knext = b.in_order().iter().find(|r| r.key != Key::HEAD && r.key.hash() == l.next).map_or(Key::HEAD.bytes(), |r| r.key.bytes());
            let mut b1 = b.clone();
            b1.leaves[any_leaf] = Some(Row { next: key_in(a).hash(), ..l });
            if (0..=63).contains(&a.pe) {
                x.besidee = beside_at(&b1, a.pe as usize);
            }
            "another leaf as the leaf before"
        }
        3 => {
            x.knext = [Key::HEAD.bytes(), key_in(a).bytes(), b.leaves[any_leaf].unwrap().key.bytes()][g.below(3) as usize].clone();
            "another key said to be named"
        }
        4 => {
            flip(&mut x.leafb, g);
            "a bit of the leaf before moved"
        }
        5 => {
            x.pe = [any_leaf as i64, a.pb, a.pe + 64, a.pe - 64, -1, 64, a.pd][g.below(7) as usize];
            "the row at a place that is not the empty one shown"
        }
        6 => {
            // (A place that is taken, with its own path in the books as the leaf before left them.)
            let mut b1 = b.clone();
            let lb = b.leaves[a.pb as usize].unwrap();
            b1.leaves[a.pb as usize] = Some(Row { next: key_in(a).hash(), ..lb });
            x.pe = any_leaf as i64;
            x.besidee = beside_at(&b1, any_leaf);
            "the row at a place that is taken, by its own path"
        }
        7 => {
            x.besidee = if (0..=63).contains(&a.pe) && g.below(2) == 0 { beside_at(b, a.pe as usize) } else { let mut v = a.besidee.clone(); flip(&mut v, g); v };
            "the empty place's path of other books"
        }
        8 => {
            // (The other mode: a second leaf of a day; or no leaf put in where none stands, shown any day's leaf.)
            if a.mode == 0 {
                x.mode = 1;
                x.pd = any_free as i64;
                let (key, mut b2) = (key_in(a), b.clone());
                let lb = b.leaves[a.pb as usize].unwrap();
                b2.leaves[a.pb as usize] = Some(Row { next: key.hash(), ..lb });
                b2.leaves[a.pe as usize] = Some(Row { key, scale: a.scale, price_max: a.limit, net: 0, gross: 0, entered: 0, next: Key::day_of(key.market, key.day).hash() });
                x.besided = beside_at(&b2, any_free);
                "the day's leaf put in where one stands"
            } else {
                x.mode = 0;
                let days: Vec<usize> = leaves.iter().copied().filter(|i| b.leaves[*i].unwrap().key.is_day()).collect();
                let at = days.get(g.below(days.len().max(1) as i64) as usize).copied().unwrap_or(any_leaf);
                x.leafd = leaf138(&b.leaves[at].unwrap());
                x.pd = at as i64;
                x.besided = beside_at(b, at);
                "no day's leaf put in where none stands"
            }
        }
        9 => {
            x.mode = [2, -1, 3][g.below(3) as usize];
            "a mode that is none"
        }
        10 => {
            if a.mode == 0 {
                // (A leaf that says "not written", as the script wants it, that is not the books'.)
                let m: [u8; 32] = a.market.clone().try_into().unwrap();
                x.leafd = [Key::day_of(m, a.day).bytes(), i9(a.scale), i9(a.limit), z27(), a.leafd[105..138].to_vec()].concat();
                if x.leafd == a.leafd {
                    x.pd = any_leaf as i64;
                }
                "a day's leaf shown as not written"
            } else {
                x.pd = [a.pe, a.pb, any_leaf as i64, a.pd + 64, -1][g.below(5) as usize];
                "the day's leaf at a place that is not empty"
            }
        }
        11 => {
            flip(&mut x.besided, g);
            "a hash beside the day's leaf moved"
        }
        12 => {
            x.scale = [a.scale + 1, a.scale - 1, 0, -a.scale, SCALE_MAX + 1, a.scale * 10][g.below(6) as usize];
            "another scale"
        }
        13 => {
            x.limit = [a.limit + 1, a.limit - 1, 0, -a.limit, MARGIN_MAX + 1, a.strike - 1][g.below(6) as usize];
            "another limit"
        }
        14 => {
            x.r = [(a.r + 1) % 4, (a.r + 2) % 4, (a.r + 3) % 4, 4, -1][g.below(5) as usize];
            "another row of the four"
        }
        15 => {
            let mut m = a.market.clone();
            match g.below(4) {
                0 => m = vec![0u8; 32],
                1 => {
                    let q = g.below(4) as usize;
                    m[8 * q..8 * q + 8].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x80]);
                }
                2 => flip(&mut m, g),
                _ => m = s.rows[g.below(4) as usize].key.market.to_vec(),
            }
            x.market = m;
            "another market"
        }
        16 => {
            x.strike = [0, -a.strike, a.limit + 1, a.strike + 1][g.below(4) as usize];
            "another strike"
        }
        17 => {
            x.day = [0, -a.day, a.day + 1][g.below(3) as usize];
            "another day"
        }
        18 => {
            x.of = [0, 3, 4, -1, 3 - a.of][g.below(5) as usize];
            "another kind"
        }
        _ => {
            x.signer = other;
            "another signer"
        }
    };
    (x, what)
}

/// The control of the helpers above: what an honest owner shows is taken at the engine, the shard after is the
/// model's, and the builder's transaction is the hand-made one.
#[test]
fn t17_held_0_the_hand_made_transactions_are_the_builders() {
    let w = world();
    let t = tmpl(&w);
    let h = w.head(0);
    let base = w.bare();
    let b0 = Books::opened();
    let call = Key { market: h.key.market, day: 900_000, of: OF_CALLS, strike: P10 };
    let a = in_honest(&base.s, &b0, call, h.scale, h.price_max, 1, 2);
    assert!(in_spec(&base.s, &b0, &a) && in_taken(&w, &t, &base.s, &a));
    let (s1, b1) = base.s.row_in(&b0, call, h.scale, h.price_max, 0, 1, Some(2)).unwrap();
    assert_eq!(in_after(&base.s, &a).unwrap(), s1);
    assert_eq!(in_tx(&w, &t, &base.s, &a, &s1.hash()).sign(&w.keys).unwrap().tx.id(), t.put.sign(&w.keys).unwrap().tx.id(), "the hand-made transaction is the builder's");
    let o = out_honest(&s1, &b1, 1, 0, Some(s1.owner));
    assert!(out_spec(&s1, &b1, &o) && out_taken(&w, &t, &s1, &o));
    let (s2, _) = s1.row_out(&b1, &call, RowOutBy::Owner).unwrap();
    assert_eq!(out_after(&s1, &o).unwrap(), s2);
    assert_eq!(out_tx(&w, &t, &s1, &o, &s2.hash()).sign(&w.keys).unwrap().tx.id(), t.out.sign(&w.keys).unwrap().tx.id());
    println!("the world's markets: scale {}, limit {}; terms mmr {} g {}", h.scale, h.price_max, w.terms().mmr_bps, w.terms().g_bps);
}

/// 1 (in). `KpfTRowIn.put` on books at random (several markets of every kind of number, three scales and limits,
/// trees from empty to full, rows with units and days written by hand, shards called): what an honest owner shows
/// for a key at random, and the same with one thing changed (twenty kinds of change), each at the engine against the
/// rule as the documents say it. The engine takes what the rule takes and nothing else; what it takes is what the
/// model's `Shard::row_in` writes; and the books after hold (`books_held`).
#[test]
fn t17_held_1_rows_listed_at_random_are_the_rules_and_the_models() {
    let w = world();
    let t = tmpl(&w);
    let mut g = Rng(0x17_0001);
    let (mut taken, mut refused, mut honest_taken) = (0, 0, 0);
    let mut by_change: std::collections::BTreeMap<&'static str, (i32, i32)> = Default::default();
    for round in 0..70 {
        let fill = [0, 2, 6, 14, 30, 45, 64][round % 7];
        let (mut s, b, ms) = random_books(&mut g, &w, fill);
        assert!(books_held(&s, &b), "round {round}: the books made hold");
        if round % 9 == 8 {
            s.called = 6_000;
        }
        let free: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_none()).collect();
        for _ in 0..40 {
            let (m, scale, limit) = ms[g.below(ms.len() as i64) as usize];
            let key = Key { market: m, day: DAYS[g.below(4) as usize], of: 1 + g.below(2), strike: (1 + g.below(7)).min(limit) };
            let pick = |g: &mut Rng| free.get(g.below(free.len().max(1) as i64) as usize).copied().unwrap_or(g.below(64) as usize);
            let (pe, mut pd) = (pick(&mut g), pick(&mut g));
            if pd == pe && free.len() >= 2 {
                pd = *free.iter().find(|x| **x != pe).unwrap();
            }
            // (One time in five the key, the scale or the limit is one the rule refuses or the market's head does
            // not have, and everything shown is what an honest owner would show for it: the refusal is that check's.)
            let (mut key, mut scale, mut limit, mut odd) = (key, scale, limit, "nothing changed");
            if g.below(5) == 0 {
                odd = "a key, a scale or a limit of its own, shown honestly";
                match g.below(12) {
                    0 => key.strike = 0,
                    1 => key.strike = -key.strike,
                    2 => key.strike = limit.saturating_add(1),
                    3 => key.day = 0,
                    4 => key.day = -key.day,
                    5 => key.of = [0, 3, 4, -1][g.below(4) as usize],
                    6 => key.market = [0; 32],
                    7 => {
                        let q = g.below(4) as usize;
                        key.market[8 * q..8 * q + 8].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x80]);
                    }
                    8 => scale = [0, -scale, SCALE_MAX + 1, scale + 1, 7][g.below(5) as usize],
                    9 => limit = [0, -limit, MARGIN_MAX + 1, limit.saturating_sub(1).max(key.strike), key.strike][g.below(5) as usize],
                    10 => (key.of, key.strike) = (3, 0),
                    _ => (key.day, key.of, key.strike) = (0, 0, 0),
                }
            }
            let honest = in_honest(&s, &b, key, scale, limit, pe, pd);
            let (a, what) = if g.below(4) == 0 || odd != "nothing changed" { (honest.clone(), odd) } else { { let which = g.below(20); in_changed(&mut g, &s, &b, &honest, which, xk(&w.w)) } };
            let (rule, engine) = (in_spec(&s, &b, &a), in_taken(&w, &t, &s, &a));
            assert_eq!(engine, rule, "round {round}, {what}: the engine {engine}, the rule {rule}: {a:?}");
            let e = by_change.entry(what).or_default();
            if engine {
                e.0 += 1;
                taken += 1;
                // What is taken is what the model writes, and the books hold.
                let k2 = Key { market: a.market.clone().try_into().unwrap(), day: a.day, of: a.of, strike: a.strike };
                let (s2, b2) = s.row_in(&b, k2, a.scale, a.limit, a.r as usize, a.pe as usize, (a.mode == 1).then_some(a.pd as usize)).unwrap_or_else(|e| panic!("round {round}, {what}: the engine takes what the model refuses: {e}: {a:?}"));
                assert_eq!(in_after(&s, &a).unwrap(), s2, "round {round}, {what}: the shard after is the model's");
                assert!(books_held(&s2, &b2), "round {round}, {what}: the books after hold");
                if what == "nothing changed" {
                    honest_taken += 1;
                    // (The output with one thing changed is refused: the count one more or less, the root of
                    // before, the sum of the gross moved.)
                    for wrong in [Shard { rows: { let mut r = s2.rows; r[a.r as usize].entered += 1; r }, ..s2 }, Shard { rows: { let mut r = s2.rows; r[a.r as usize].entered -= 1; r }, ..s2 }, Shard { root: s.root, ..s2 }, Shard { dgross: s2.dgross + 1, ..s2 }, Shard { held: s2.held + 1, ..s2 }] {
                        assert!(!accepted(&in_tx(&w, &t, &s, &a, &wrong.hash()), &w.keys), "round {round}: another shard written");
                    }
                }
            } else {
                e.1 += 1;
                refused += 1;
            }
        }
    }
    for (what, (yes, no)) in &by_change {
        println!("{what}: {yes} taken, {no} refused");
    }
    println!("rows listed: {taken} taken ({honest_taken} of them with nothing changed), {refused} refused; the engine and the rule agree on each");
    assert!(honest_taken > 100 && refused > 1_000);
}

/// One thing changed in what `out` is shown.
fn out_changed(g: &mut Rng, s: &Shard, b: &Books, a: &Outw, which: i64, other: [u8; 32]) -> (Outw, &'static str) {
    let mut x = a.clone();
    let leaves: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_some()).collect();
    let free: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_none()).collect();
    let any_leaf = leaves[g.below(leaves.len() as i64) as usize];
    let any_free = free.get(g.below(free.len().max(1) as i64) as usize).copied().unwrap_or(any_leaf);
    let flip = |v: &mut Vec<u8>, g: &mut Rng| {
        let i = g.below(v.len() as i64) as usize;
        v[i] ^= 1 << g.below(8);
    };
    let mut b1 = b.clone();
    b1.leaves[a.pr as usize] = None;
    let what = match which {
        0 => {
            x.pr = [any_leaf as i64, any_free as i64, a.pr + 64, a.pr - 64, -1, 64][g.below(6) as usize];
            "the leaf at another place"
        }
        1 => {
            flip(&mut x.besider, g);
            "a hash beside the leaf moved"
        }
        2 => {
            flip(&mut x.leafr, g);
            "a bit of the leaf moved"
        }
        3 => {
            // (Another leaf as the one that names it, by its own path in the books as the leaving left them: the
            // head's, a row's of the same day, any.)
            let at = if g.below(3) == 0 { b.place_of(&Key::HEAD).unwrap() } else { any_leaf };
            if let Some(l) = b1.leaves[at] {
                x.leafb = leaf138(&l);
                x.pb = at as i64;
                x.besideb = beside_at(&b1, at);
            }
            "another leaf as the one that names it"
        }
        4 => {
            // (The leaf that names it made to name it: a leaf that is not the books'.)
            let at = any_leaf;
            if let Some(l) = b1.leaves[at] {
                x.leafb = [&leaf138(&l)[..105], &a.leafb[105..138]].concat();
                x.pb = at as i64;
                x.besideb = beside_at(&b1, at);
            }
            "a leaf made to name it"
        }
        5 => {
            x.pb = [any_leaf as i64, any_free as i64, a.pb + 64, a.pb - 64, -1, a.pr][g.below(6) as usize];
            "the leaf that names it at another place"
        }
        6 => {
            x.besideb = if g.below(2) == 0 { beside_at(b, a.pb as usize) } else { let mut v = a.besideb.clone(); flip(&mut v, g); v };
            "the path of the leaf that names it of other books"
        }
        7 => {
            x.r = [(a.r + 1) % 4, (a.r + 2) % 4, (a.r + 3) % 4, 4, -1][g.below(5) as usize];
            "another row of the four"
        }
        8 => {
            x.mode = [(a.mode + 1) % 3, (a.mode + 2) % 3, 3, -1][g.below(4) as usize];
            "another mode"
        }
        9 => {
            // (Another day's leaf shown, by its own path: a written one where there is one.)
            let days: Vec<usize> = leaves.iter().copied().filter(|i| b.leaves[*i].unwrap().key.is_day()).collect();
            let written: Vec<usize> = days.iter().copied().filter(|i| b.leaves[*i].unwrap().net == 1 || b.leaves[*i].unwrap().entered > 0).collect();
            let from = if written.is_empty() || g.below(3) == 0 { &days } else { &written };
            let at = from.get(g.below(from.len().max(1) as i64) as usize).copied().unwrap_or(any_leaf);
            x.leafd = leaf138(&b.leaves[at].unwrap());
            x.pd = at as i64;
            x.besided = beside_at(b, at);
            x.mode = 1;
            "another day's leaf shown, by its path"
        }
        10 => {
            // (Its day's leaf shown written, that is not written in the books: a price, or "none".)
            let mut l = a.leafd.clone();
            if g.below(2) == 0 { l[97] = 9 } else { l[79] = 1 }
            x.leafd = l;
            x.mode = 1;
            "its day's leaf said written"
        }
        11 => {
            flip(&mut x.besided, g);
            x.mode = 1;
            "a hash beside the day's leaf moved"
        }
        12 => {
            x.pd = [any_leaf as i64, a.pd + 64, -1, any_free as i64][g.below(4) as usize];
            x.mode = 1;
            "the day's leaf at another place"
        }
        _ => {
            x.signer = [Some(other), None][g.below(2) as usize];
            x.mode = 0;
            "another signer, or none"
        }
    };
    let _ = s;
    (x, what)
}

/// 1 (out). `KpfTRowOut.out` on the same books at random: every leaf of the books in every mode (the owner's
/// signature, its day's leaf shown, the shard called), and the same with one thing changed, each at the engine
/// against the rule. What it takes is what the model's `Shard::row_out` writes, and the books after hold. A row
/// with a unit, with a net, with something entered stays, in every mode; a day's leaf stays while a row of its day
/// stands; the head's leaf stays.
#[test]
fn t17_held_2_leaves_taken_out_at_random_are_the_rules_and_the_models() {
    let w = world();
    let t = tmpl(&w);
    let mut g = Rng(0x17_0002);
    let (mut taken, mut refused, mut units, mut days_held, mut by_anyone) = (0, 0, 0, 0, 0);
    let mut by_change: std::collections::BTreeMap<&'static str, (i32, i32)> = Default::default();
    for round in 0..60 {
        let fill = [2, 6, 14, 30, 45, 64][round % 6];
        let (mut s, b, _) = random_books(&mut g, &w, fill);
        if round % 4 == 3 {
            s.called = 6_000;
        }
        let leaves: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_some()).collect();
        for _ in 0..50 {
            let pr = leaves[g.below(leaves.len() as i64) as usize];
            let mode = g.below(3);
            let signer = if mode == 0 || g.below(4) == 0 { Some(s.owner) } else { None };
            let honest = out_honest(&s, &b, pr, mode, signer);
            let (a, what) = if g.below(3) == 0 { (honest.clone(), "nothing changed") } else { { let which = g.below(14); out_changed(&mut g, &s, &b, &honest, which, xk(&w.w)) } };
            let (rule, engine) = (out_spec(&s, &b, &a), out_taken(&w, &t, &s, &a));
            assert_eq!(engine, rule, "round {round}, {what}: the engine {engine}, the rule {rule}: {a:?}");
            let lr = b.leaves[pr].unwrap();
            let e = by_change.entry(what).or_default();
            if engine {
                e.0 += 1;
                taken += 1;
                let key = key_of(&a.leafr);
                let by = match a.mode {
                    _ if key.is_day() => RowOutBy::Owner,
                    0 => RowOutBy::Owner,
                    1 => RowOutBy::DayWritten,
                    _ => RowOutBy::Called,
                };
                if key.is_dated() && a.mode != 0 {
                    by_anyone += 1;
                }
                let (s2, b2) = s.row_out(&b, &key, by).unwrap_or_else(|e| panic!("round {round}, {what}: the engine takes what the model refuses: {e}: {a:?}"));
                assert_eq!(out_after(&s, &a).unwrap(), s2, "round {round}, {what}: the shard after is the model's");
                assert!(books_held(&s2, &b2), "round {round}, {what}: the books after hold");
                if what == "nothing changed" {
                    // (The output with one thing changed is refused: the count not lowered, lowered by two; the
                    // empty row written where the head still counts or has a unit; the head kept where it counts
                    // none and has no unit; the root of before.)
                    let r = a.r as usize;
                    let mut others = vec![Shard { rows: { let mut x = s2.rows; x[r] = s.rows[r]; x }, ..s2 }, Shard { root: s.root, ..s2 }, Shard { dgross: s2.dgross + 1, ..s2 }];
                    if s2.rows[r] == ShardRow::EMPTY {
                        others.push(Shard { rows: { let mut x = s2.rows; x[r] = ShardRow { entered: 0, ..s.rows[r] }; x }, ..s2 });
                    } else {
                        others.push(Shard { rows: { let mut x = s2.rows; x[r] = ShardRow::EMPTY; x }, ..s2 });
                        others.push(Shard { rows: { let mut x = s2.rows; x[r].entered -= 1; x }, ..s2 });
                    }
                    for wrong in others {
                        assert!(!accepted(&out_tx(&w, &t, &s, &a, &wrong.hash()), &w.keys), "round {round}: another shard written");
                    }
                }
            } else {
                e.1 += 1;
                refused += 1;
                if what == "nothing changed" && lr.key.is_dated() && (lr.net, lr.gross, lr.entered) != (0, 0, 0) {
                    units += 1;
                }
                if what == "nothing changed" && lr.key.is_day() {
                    days_held += 1;
                }
            }
        }
    }
    for (what, (yes, no)) in &by_change {
        println!("{what}: {yes} taken, {no} refused");
    }
    println!("leaves taken out: {taken} taken ({by_anyone} dated rows by anyone), {refused} refused ({units} rows with a unit, a net or something entered shown honestly; {days_held} days' leaves while a row of the day stands)");
    assert!(taken > 200 && refused > 1_000 && units > 20 && days_held > 20 && by_anyone > 20);
}

// ================================================================================================================
// 2. The trace over dated rows, at the engine.

struct TShard {
    shard: OLive<Shard>,
    b: Books,
    prices: [i64; 4],
}

fn up_t(x: i64, r: i64) -> i64 {
    ((x as i128 * r as i128 + 9_999) / 10_000) as i64
}

/// Strikes that matter at the price `p`: the price; each end of a move of the rate, of a draw's and of a call's
/// (the price moved the tolerance either way), and a sompi either side of each; far; the least and the limit.
fn strikes_near(p: i64, limit: i64, t: &XTerms) -> Vec<i64> {
    let (g, mmr) = (t.g_bps, t.mmr_bps);
    let mut v = vec![p, p - 1, p + 1, p / 2, p.saturating_mul(2), 1, limit];
    let down = ((p as i128 * (BPS - g) as i128) / BPS as i128) as i64;
    let up = up_t(p, BPS + g).min(limit);
    for (q, rate) in [(p, mmr + g), (down, mmr - g), (up, mmr - g)] {
        let d = up_t(q, rate);
        v.push(q);
        for e in [(q - d).max(0), (q + d).min(limit)] {
            v.extend([e - 1, e, e + 1]);
        }
    }
    v.into_iter().map(|x| x.clamp(1, limit)).collect()
}

/// A shard for a trace: the world's four markets, each at a scale, a price and a limit at random (the limit at the
/// price, a little above it, or far); each with a futures row of units, or none (a head of no unit where it has
/// leaves); about `leaves` leaves listed by the model at random places, strikes near and far; units written by hand
/// into two dated rows of three (the clearing side wrote, holds, both nothing with a gross, some of each). With
/// `ended`, rows of days at and before the statement's time among them.
fn trace_shard(g: &mut Rng, w: &World, leaves: usize, ended: bool) -> TShard {
    let base = w.bare();
    let mut s = base.s;
    let mut b = Books::opened();
    let t = s.t;
    let mut prices = [0i64; 4];
    let mut heads = vec![];
    for i in 0..4 {
        let scale = [100_000_000, 100_000_000, 1, 1_000_000_000][g.below(4) as usize];
        let p = [P10, 3 * P10 + 7, 123_456_789, 1_000_000_000_000, 10_007][g.below(5) as usize];
        let limit = [100_000_000_000_000, 100_000_000_000_000, p + p / 40, p][g.below(4) as usize];
        prices[i] = p;
        heads.push(RowHead { key: Key::futures(w.markets[i].id()), scale, price_max: limit });
        if g.below(2) == 0 {
            let gross = g.range(1, 40) * scale + if scale > 1 && g.below(2) == 0 { g.below(scale) } else { 0 };
            let net = [gross, -gross, 0, g.range(-gross, gross)][g.below(4) as usize];
            s.rows[i] = ShardRow::of(heads[i], net, gross);
        }
    }
    let days: &[i64] = if ended { &[900_000, 1_800_000, T0 + 1, T0, 3_000] } else { &[900_000, 1_800_000, T0 + 1] };
    let mut tries = 0;
    while b.in_order().len() - 1 < leaves && tries < 40 * leaves + 40 {
        tries += 1;
        let i = g.below(4) as usize;
        let h = heads[i];
        let near = strikes_near(prices[i], h.price_max, &t);
        let key = Key { market: h.key.market, day: days[g.below(days.len() as i64) as usize], of: 1 + g.below(2), strike: near[g.below(near.len() as i64) as usize] };
        let free: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_none()).collect();
        let stands = b.row(&Key::day_of(key.market, key.day)).is_some();
        if free.len() < if stands { 1 } else { 2 } {
            continue;
        }
        let pe = free[g.below(free.len() as i64) as usize];
        let pd = *free.iter().find(|x| **x != pe).unwrap_or(&pe);
        if let Ok((s2, b2)) = s.row_in(&b, key, h.scale, h.price_max, i, pe, (!stands).then_some(pd)) {
            (s, b) = (s2, b2);
        }
    }
    for i in 0..64 {
        let Some(r) = b.leaves[i] else { continue };
        if !r.key.is_dated() || g.below(3) == 0 {
            continue;
        }
        let gross = g.range(1, 40) * r.scale + if r.scale > 1 && g.below(2) == 0 { g.below(r.scale) } else { 0 };
        let net = [gross, -gross, 0, g.range(-gross, gross)][g.below(4) as usize];
        b.leaves[i] = Some(Row { net, gross, entered: g.range(-9, 9), ..r });
        s.dgross += gross;
    }
    s.root = b.root();
    s.held = g.range(2, 3_000) * KAS1;
    s.basis = g.range(-2_000, 2_000) * KAS1;
    TShard { shard: live(&base, s), b, prices }
}

/// The statement of a draw (of the trace's amount, by the owner) or of a call (by the watcher) that says the trace
/// `t` on the shard, with the outputs the rule gives it (a call that is not the rule's: any).
fn statement(w: &World, shard: &OLive<Shard>, call: bool, t: &Traced) -> Stated {
    let s = &shard.s;
    let held = shard.value as i64;
    let claimer = if call { xk(&w.w) } else { s.owner };
    let draw_outs = |amount: i64| Outs::of(&[Out { key: false, to: s.drawn(amount, T0).hash(), value: held - amount }, Out { key: true, to: s.owner, value: amount }]);
    let outs = if call { s.call_trace_outs(t, &claimer, T0, held).or_else(|| draw_outs(KAS1)) } else { draw_outs(t.amount).or_else(|| draw_outs(KAS1)) }.expect("outputs");
    Stated { position: s.hash(), window: s.dispute_daa, late: s.t.late_daa, kind: if call { KIND_CALL } else { KIND_DRAW }, t0: T0, arg: t.arg(), outs, bond: if call { s.call_bond() } else { s.bond() }, claimer, held }
}

fn verdicts(v: Option<bool>) -> (bool, bool) {
    (v == Some(true), v == Some(false))
}
/// The model's verdict of a row's point: 1 to 4 a futures row, 100 and k the dated row k.
fn point_judged(s: &Shard, b: &Books, t: &Traced, call: bool, point: i64) -> Option<bool> {
    if point > 100 { s.trace_dated_judged(b, t, (point - 100) as usize, call, T0) } else { s.trace_row_judged(t, point as usize, call) }
}
/// What the engine takes of the answer of a row's point by the row's script: (right, wrong).
fn point_taken(w: &World, live: &OLive<Stated>, s: &Shard, b: &Books, t: &Traced, point: i64) -> (bool, bool) {
    if point > 100 {
        return dated_taken(w, live, s, b, t, point - 100);
    }
    let f = [fund(xk(&w.w), 32)];
    let q = w.pointed(live, point);
    let take = |v: bool| shard_row_t(&w.lib, &q, s, t, point, &w.script("KpfTShardRow"), xk(&w.w), w.payer(&w.w, &f), Some(v)).is_ok_and(|(bb, _)| accepted(&bb, &w.keys));
    (take(true), take(false))
}
fn row_points(t: &Traced) -> Vec<i64> {
    (1..=4).chain(101..=101 + t.dated.len() as i64).collect()
}
/// The sums an end reads of a trace: the last row's, and what the dated rows are asked.
fn end_sums(t: &Traced) -> (i64, i64, i64, i64) {
    match t.dated.last() {
        Some(r) => (r.wn, r.wg, r.sn, r.ro),
        None => (t.rows[3].wn, t.rows[3].wg, t.rows[3].sn, 0),
    }
}

/// A trace said otherwise than `t`: what was changed.
fn lie_of(g: &mut Rng, t: &Traced) -> Option<(Traced, String)> {
    let mut x = t.clone();
    let n = t.dated.len();
    if n == 0 {
        return None;
    }
    let k = g.below(n as i64) as usize;
    let what = match g.below(9) {
        0 | 1 | 2 => {
            let f = g.below(6) as usize;
            let by = if g.below(2) == 0 { 1 } else { -1 };
            let r = &mut x.dated[k];
            *[&mut r.place, &mut r.at, &mut r.wn, &mut r.wg, &mut r.sn, &mut r.ro][f] += by;
            format!("the number {f} of the dated row {} moved by {by}", k + 1)
        }
        3 => {
            x.dated[k].nk[g.below(32) as usize] ^= 1 << g.below(8);
            format!("a bit of the hash of the dated row {} moved", k + 1)
        }
        4 => {
            let at = [0, k, n - 1][g.below(3) as usize];
            x.dated.remove(at);
            if x.dated.is_empty() {
                return None;
            }
            format!("the dated row {} of {n} left out", at + 1)
        }
        5 => {
            if n >= 64 {
                return None;
            }
            x.dated.insert(k, t.dated[k]);
            format!("the dated row {} said twice", k + 1)
        }
        6 => {
            if n < 2 {
                return None;
            }
            let at = k.min(n - 2);
            x.dated.swap(at, at + 1);
            format!("the dated rows {} and {} one for the other", at + 1, at + 2)
        }
        7 => {
            // (The hash of a key of another shard's tree: a row of no leaf of this one.)
            x.dated[k].nk = Key { market: [0x77; 32], day: 900_000, of: 1, strike: 5 }.hash();
            format!("the dated row {} naming a key of another tree", k + 1)
        }
        _ => {
            let f = g.below(3) as usize;
            let r = &mut x.rows[3];
            *[&mut r.wn, &mut r.wg, &mut r.sn][f] += 1;
            format!("the sum {f} of the fourth row moved")
        }
    };
    Some((x, what))
}

/// 2. On shards at random (`trace_shard`: one to sixty-three leaves; puts and calls; strikes at the price, at each
/// end of the move, a sompi either side, far; the clearing side wrote, holds, neither with a gross; markets with a
/// futures row of no unit), a draw and a call each:
///  - the trace the model makes is stated, and at EVERY row's point (1 to 4, 101 to one beyond the last) the
///    engine takes the model's verdict and not the other;
///  - at every price's point the engine takes the model's verdict of the committee's price, and not the other
///    (`KpfTShardPrice` over a statement with dated rows: not tested at the engine by its author);
///  - at the end the engine takes the model's verdict and not the other (`KpfTShardEnd`; `KpfTCallEnd` with dated
///    rows, with the outputs of a call that is the rule's: not tested at the engine by its author);
///  - traces said otherwise (a number of a dated row moved by one, a bit of its hash, a row left out, said twice,
///    two swapped, a key of another tree, the fourth row's sums): each is wrong at a point by the model, the engine
///    takes "wrong" there and not "right", and NO trace said otherwise is right at every point, but one that moves
///    the place of a price in a row that reads none (its sums are the true ones).
#[test]
fn t17_held_3_the_trace_over_dated_rows_at_random_is_the_models() {
    let w = world();
    let mut g = Rng(0x17_0003);
    let f = [fund(xk(&w.w), 32)];
    let cite = w.script("KpfTCite");
    let (mut points, mut lies, mut free, mut prices_right, mut prices_wrong, mut ends_right, mut ends_wrong, mut calls_right, mut no_verdict) = (0, 0, 0, 0, 0, 0, 0, 0, 0);
    let sizes = [1usize, 2, 3, 5, 9, 16, 30, 47, 63, 63, 4, 7, 12, 63];
    for (round, leaves) in sizes.iter().enumerate() {
        let ended = round % 5 == 4;
        let ts = trace_shard(&mut g, &w, *leaves, ended);
        let (shard, b) = (&ts.shard, &ts.b);
        let s = &shard.s;
        for call in [false, true] {
            // (Where rows of ended days have units no trace is the rule's: the one said counts them as rows of a
            // day to come. A draw is wrong at such a row; a call's has no answer here.)
            let Some(mut t) = s.traced_with(b, &Draw { amount: if call { 0 } else { KAS1 }, prices: ts.prices.into() }, call, if ended { 0 } else { T0 }) else {
                no_verdict += 1;
                continue;
            };
            if !call {
                // (The amount: the most the trace's sums allow, a sompi more, or a KAS.)
                let right = |a: i64| s.draw_trace_end_judged(&Traced { amount: a, ..t.clone() }, &s.owner, T0) == Some(true);
                let (mut lo, mut hi) = (0i64, s.held);
                while lo < hi {
                    let mid = lo + (hi - lo + 1) / 2;
                    if right(mid) { lo = mid } else { hi = mid - 1 }
                }
                t.amount = [lo.max(1), (lo + 1).min(s.held), KAS1][g.below(3) as usize];
            }
            let st = statement(&w, shard, call, &t);
            let live = w.stated(shard, &st, &t.said());
            for p in row_points(&t) {
                let judged = point_judged(s, b, &t, call, p);
                assert!(ended || judged == Some(true), "round {round}: the model's own trace is right at {p}");
                assert_eq!(point_taken(&w, &live, s, b, &t, p), verdicts(judged), "round {round} (call {call}), the point {p}: the engine and the model ({judged:?})");
                points += 1;
                if judged.is_none() {
                    no_verdict += 1;
                }
            }
            // The prices' points, by the committee's price: the one said, or one two hundredths above it.
            for e in 1..=t.list.len() {
                let i = (0..4).find(|i| w.markets[*i].id() == t.list[e - 1].market).unwrap();
                let c = if g.below(2) == 0 { t.list[e - 1].price } else { t.list[e - 1].price + t.list[e - 1].price / 50 + 1 };
                let cited = w.cited(i, c);
                let right = s.trace_price_right(&t, e, c);
                let q = w.pointed(&live, 4 + e as i64);
                let take = |v: bool| shard_price_t(&w.lib, &q, s, SEED, &t, 4 + e as i64, &w.script("KpfTShardPrice"), Some((&cite, &cited)), xk(&w.w), w.payer(&w.w, &f), Some(v)).is_ok_and(|(bb, _)| accepted(&bb, &w.keys));
                assert_eq!((take(true), take(false)), (right, !right), "round {round} (call {call}): the price {e} of a statement with {} dated rows, the committee's {c}", t.dated.len());
                if right { prices_right += 1 } else { prices_wrong += 1 }
            }
            // The end.
            if !ended {
                let q = w.pointed(&live, 0);
                let (right, take): (bool, Box<dyn Fn(bool) -> bool>) = if call {
                    (s.call_trace_end_right(&t, &st.claimer, &st.outs, st.held, T0), Box::new(|v: bool| call_end_t(&w.lib, &q, s, &t, &w.script("KpfTCallEnd"), xk(&w.w), w.payer(&w.w, &f), Some(v)).is_ok_and(|(bb, _)| accepted(&bb, &w.keys))))
                } else {
                    (s.draw_trace_end_right(&t, &st.claimer, &st.outs, st.held, T0), Box::new(|v: bool| shard_end_t(&w.lib, &q, s, &t, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), Some(v)).is_ok_and(|(bb, _)| accepted(&bb, &w.keys))))
                };
                assert_eq!((take(true), take(false)), (right, !right), "round {round} (call {call}): the end of a trace with {} dated rows", t.dated.len());
                if right { ends_right += 1 } else { ends_wrong += 1 }
                if right && call {
                    calls_right += 1;
                }
            }
            // Said otherwise.
            if ended {
                continue;
            }
            for _ in 0..14 {
                let Some((lie, what)) = lie_of(&mut g, &t) else { continue };
                let judged: Vec<(i64, Option<bool>)> = row_points(&lie).into_iter().map(|p| (p, point_judged(s, b, &lie, call, p))).collect();
                if judged.iter().all(|(_, v)| *v == Some(true)) {
                    assert!(what.starts_with("the number 1 "), "round {round}: {what}: a trace said otherwise is right at every point");
                    assert_eq!((end_sums(&lie), &lie.list, lie.amount), (end_sums(&t), &t.list, t.amount), "round {round}: {what}: right at every point with other sums");
                    free += 1;
                    continue;
                }
                let (p, _) = *judged.iter().find(|(_, v)| *v == Some(false)).unwrap_or_else(|| panic!("round {round}: {what}: wrong, and at no point by a verdict"));
                let lst = Stated { arg: lie.arg(), ..st };
                let live2 = w.states(shard, &lst, &lie.said()).unwrap_or_else(|| panic!("round {round}: {what}: not stated"));
                assert_eq!(point_taken(&w, &live2, s, b, &lie, p), (false, true), "round {round} (call {call}): {what}: at the point {p}");
                // (And at another point, whatever the model says of it there.)
                let (p2, v2) = judged[g.below(judged.len() as i64) as usize];
                assert_eq!(point_taken(&w, &live2, s, b, &lie, p2), verdicts(v2), "round {round} (call {call}): {what}: at the point {p2} ({v2:?})");
                lies += 1;
            }
        }
    }
    println!("(the lies' kinds are in `lie_of`)");
    println!("the trace over dated rows: {points} points of the model's traces ({no_verdict} with no verdict, taken neither way); {prices_right} prices right and {prices_wrong} wrong; {ends_right} ends right ({calls_right} of them calls) and {ends_wrong} wrong; {lies} traces said otherwise wrong at their point, {free} that move a place nothing reads");
    assert!(points > 500 && lies > 200 && prices_right > 5 && prices_wrong > 5 && ends_right > 3 && ends_wrong > 3);
}

/// 2 (the length). What `KpfTChange` takes as a draw's statement on a shard: the trace with its dated rows as it is
/// said; and not with its byte another, its count one more or less, a byte more or less, a row beyond the count,
/// the byte and a count of nothing with no row, sixty-five rows, the list's count written otherwise than the list
/// is, five prices in either form. Sixty-four rows, and four prices with a dated row, are stated.
#[test]
fn t17_held_4_a_statement_with_dated_rows_has_one_length_and_one_form() {
    let w = world();
    let (shard, b) = dated_shard(&w);
    let s = &shard.s;
    let t = s.traced_with(&b, &Draw { amount: KAS1, prices: PRICES.into() }, false, T0).unwrap();
    let st = statement(&w, &shard, false, &t);
    let said = t.said();
    let at = 184 + 56 * t.list.len();
    assert_eq!((t.list.len(), t.dated.len(), said.len(), said[at]), (1, 6, at + 9 + 480, 0x44));
    let with = |f: &dyn Fn(&mut Vec<u8>)| {
        let mut v = said.clone();
        f(&mut v);
        v
    };
    let rows = |n: usize| [&said[..at + 1], &sm8(n as i64)[..], &vec![0u8; 80 * n][..]].concat();
    let many = |e: usize, n: usize| [&sm8(KAS1)[..], &sm8(e as i64)[..], &vec![0u8; 56 * e][..], &sm8(4)[..], &[0u8; 160][..], &[0x44][..], &sm8(n as i64)[..], &vec![0u8; 80 * n][..]].concat();
    let cases: Vec<(&str, Vec<u8>, bool)> = vec![
        ("as it is said", said.clone(), true),
        ("the byte another", with(&|v| v[at] = 0x45), false),
        ("the byte nothing", with(&|v| v[at] = 0), false),
        ("the count one more", with(&|v| v[at + 1] += 1), false),
        ("the count one less", with(&|v| v[at + 1] -= 1), false),
        ("a byte more", with(&|v| v.push(0)), false),
        ("a byte less", with(&|v| { v.pop(); }), false),
        ("two bytes more", with(&|v| v.extend([0, 0])), false),
        ("a row beyond the count", with(&|v| v.extend([0u8; 80])), false),
        ("a row more, counted", with(&|v| { v.extend([0u8; 80]); v[at + 1] += 1 }), true),
        ("the byte and a count of nothing, no row", rows(0), false),
        ("sixty-four rows", rows(64), true),
        ("sixty-five rows", rows(65), false),
        ("the list's count written nothing", with(&|v| v[8] = 0), false),
        ("the list's count written two", with(&|v| v[8] = 2), false),
        ("the list's count written below nothing", with(&|v| v[15] = 0x80), false),
        ("without its dated rows (the form of before)", said[..at].to_vec(), true),
        ("four prices and a dated row", many(4, 1), true),
        ("five prices and a dated row", many(5, 1), false),
        ("no price and a dated row", many(0, 1), true),
        ("five prices in the form of before", many(5, 1)[..184 + 56 * 5].to_vec(), false),
    ];
    let mut n = 0;
    for (what, bytes, want) in &cases {
        let got = w.states(&shard, &Stated { arg: blake2b(bytes), ..st }, bytes).is_some();
        assert_eq!(got, *want, "{what} ({} bytes)", bytes.len());
        // (And what is stated reads back by the model exactly where it is one form's.)
        n += 1;
    }
    println!("the length of a statement with dated rows: {n} forms, each taken or refused as the rule says");
}

// ================================================================================================================
// 3. What a dated row adds, and the price a call counts it at.

/// What a dated row adds at the price `q` it is counted at, with the rate `rate`, written here from 17.8j, 6 and not
/// taken of the model: the clearing side's worth (below nothing where it wrote), the gross's worth, and the MOST it
/// loses at any price of the move tried (the ends, the strike and a sompi either side, the middle, and `more`).
fn row_at(row: &Row, q: i64, rate: i64, more: &[i64]) -> (i128, i128, i128) {
    let d = up_t(q, rate);
    let (l, u) = ((q - d).max(0), (q + d).min(row.price_max));
    let k = row.key.strike;
    let ex = |x: i64| if row.key.of == OF_CALLS { (x - k).max(0) } else { (k - x).max(0) };
    let wo = |x: i64| row.net.unsigned_abs() as i128 * ex(x) as i128 / row.scale as i128;
    let loss = |x: i64| if row.net < 0 { wo(x) - wo(q) } else if row.net > 0 { wo(q) - wo(x) } else { 0 };
    let asked = [l, u, q, k, k - 1, k + 1, l + (u - l) / 2].iter().chain(more).filter(|x| **x >= l && **x <= u).map(|x| loss(*x)).max().unwrap_or(0).max(0);
    (if row.net < 0 { -wo(q) } else { wo(q) }, row.gross as i128 * q as i128 / row.scale as i128, asked)
}
/// The price a call counts a dated row at by the code (up where the clearing side holds calls, or has puts and
/// holds none), and the price moved the other way.
fn call_prices(row: &Row, p: i64, g: i64) -> (i64, i64) {
    let down = ((p as i128 * (BPS - g) as i128) / BPS as i128) as i64;
    let up = up_t(p, BPS + g).min(row.price_max);
    // (After V2: up only where the row's worth to the shard rises with the price; with no net unit, down.)
    if (row.net > 0 && row.key.of == OF_CALLS) || (row.net < 0 && row.key.of != OF_CALLS) { (up, down) } else { (down, up) }
}

/// 3. `Shard::dated_adds` on 60,000 rows at random (strikes at the price, at each end of the move, a sompi either
/// side, far; the upper end cut at the limit; whole units and parts; scales 1, 10^8, 10^9), a draw's and a call's:
/// what it says the row adds is what 17.8j, 6 says, and what it says the row is asked is exactly the most the row
/// loses at any price of the move tried (never less at any price, and reached at an end).
#[test]
fn t17_held_5_a_dated_row_is_asked_exactly_the_most_it_loses_in_the_move() {
    let w = world();
    let s = w.bare().s;
    let t = s.t;
    let mut g = Rng(0x17_0005);
    let (mut n, mut nothing, mut cut, mut at_end) = (0, 0, 0, 0);
    for _ in 0..60_000 {
        let scale = [1, 100_000_000, SCALE_MAX][g.below(3) as usize];
        let p = [P10, 3 * P10 + 7, 123_456_789, 1_000_000_000_000, 10_007, g.range(1, 1_000_000)][g.below(6) as usize];
        let limit = [100_000_000_000_000, p + p / 40, p, p + 1][g.below(4) as usize];
        let near = strikes_near(p, limit, &t);
        let strike = near[g.below(near.len() as i64) as usize];
        let gross = g.range(1, 40) * scale + if scale > 1 { g.below(scale) } else { 0 };
        let net = [gross, -gross, 0, g.range(-gross, gross)][g.below(4) as usize];
        let row = Row { key: Key { market: [7; 32], day: 900_000, of: 1 + g.below(2), strike }, scale, price_max: limit, net, gross, entered: 0, next: [0; 32] };
        for call in [false, true] {
            let (q, rate) = if call { (call_prices(&row, p, t.g_bps).0, t.mmr_bps - t.g_bps) } else { (p, t.mmr_bps + t.g_bps) };
            let d = up_t(q, rate);
            let (l, u) = ((q - d).max(0), (q + d).min(limit));
            let more: Vec<i64> = (0..6).map(|_| g.range(l, u)).collect();
            let (sn, wg, asked) = row_at(&row, q, rate, &more);
            assert_eq!(s.dated_adds(&row, p, call).map(|(a, b, c)| (a as i128, b as i128, c as i128)), Some((sn, wg, asked)), "{row:?} at {p} (call {call}): counted at {q}, the move {l} to {u}");
            n += 1;
            if asked == 0 { nothing += 1 }
            if q + d > limit { cut += 1 }
            if strike == l || strike == u { at_end += 1 }
        }
    }
    println!("a dated row asked alone: {n} rows, {nothing} asked nothing, {cut} with the upper end cut at the limit, {at_end} with the strike at an end exactly: each asked the most it loses in the move");
    assert!(nothing > 1_000 && cut > 1_000 && at_end > 1_000);
}

/// V2 (low). "In a call a dated row is counted at the price moved the tolerance in the shard's favour": the code
/// (model and script alike) moves it UP where the clearing side holds calls, wrote puts, or has puts and no net
/// unit, and down otherwise. Where the row's worth to the clearing side does not move with the price (no net unit;
/// held calls and written puts out of the money beyond the move), the move up changes nothing but the gross's
/// worth, which it raises: the price is moved AGAINST the shard, and a call is right for a shard that holds its
/// lower requirement at the price said (and more than it at the price moved down). The amount is the rate on the
/// gross of a hundredth of the row's gross worth (twice the tolerance).
#[test]
fn t17_v2_a_call_counts_some_dated_rows_at_the_price_moved_against_the_shard() {
    let w = world();
    let base = w.bare();
    let t = base.s.t;
    let rg = ((t.mmr_bps - 3 * t.g_bps) / 2).max(0);
    // In the model, by the kind of row: how often the price the code counts at leaves the shard worse than the
    // price moved the other way (what the row adds to the equity, less what it adds to the requirement).
    let mut g = Rng(0x17_0022);
    let mut by: std::collections::BTreeMap<&'static str, (i64, i64, i128)> = Default::default();
    for _ in 0..40_000 {
        let scale = 100_000_000;
        let p = [P10, 3 * P10 + 7, 123_456_789][g.below(3) as usize];
        let limit = 100_000_000_000_000;
        let near = strikes_near(p, limit, &t);
        let strike = near[g.below(near.len() as i64) as usize];
        let gross = g.range(1, 40) * scale;
        let (net, of) = ([gross, -gross, 0][g.below(3) as usize], 1 + g.below(2));
        let row = Row { key: Key { market: [7; 32], day: 900_000, of, strike }, scale, price_max: limit, net, gross, entered: 0, next: [0; 32] };
        let (fav, other) = call_prices(&row, p, t.g_bps);
        let standing = |q: i64| {
            let (sn, wg, asked) = row_at(&row, q, t.mmr_bps - t.g_bps, &[]);
            sn - (wg * rg as i128 + 9_999) / 10_000 - asked
        };
        let what = match (net.signum(), of) {
            (1, 1) => "holds calls (counted up)",
            (-1, 1) => "wrote calls (counted down)",
            (1, _) => "holds puts (counted down)",
            (-1, _) => "wrote puts (counted up)",
            (_, 1) => "calls, no net unit (counted down)",
            _ => "puts, no net unit (counted down)",
        };
        let e = by.entry(what).or_default();
        e.0 += 1;
        if standing(fav) < standing(other) {
            e.1 += 1;
            e.2 = e.2.max(standing(other) - standing(fav));
        }
    }
    for (what, (n, against, most)) in &by {
        println!("{what}: {n} rows, {against} counted at the price that leaves the shard worse (by {most} sompi at the most)");
    }
    assert_eq!((by["wrote calls (counted down)"].1, by["holds puts (counted down)"].1, by["calls, no net unit (counted down)"].1, by["puts, no net unit (counted down)"].1), (0, 0, 0, 0), "a row counted down is counted in the shard's favour; V2 (closed for a row of no net unit)");
    assert!(by["holds calls (counted up)"].1 > 0 && by["wrote puts (counted up)"].1 > 0, "V2 stands in part (the last phase): held calls and written puts out of the money are counted against the shard");

    // At the engine: a shard of 175 KAS whose one row is a thousand units of a series at the price 10. The rate on
    // the gross (1.75 %) of its gross worth is 175 KAS at 10, 174.125 at 9.95, 175.875 at 10.05.
    let h = w.head(0);
    let f = [fund(xk(&w.w), 32)];
    for (what, of, strike, sign) in [("puts of no net unit", OF_PUTS, P10 / 2, 0), ("calls held, out of the money beyond the move", OF_CALLS, 2 * P10, 1), ("puts written, out of the money beyond the move", OF_PUTS, P10 / 2, -1)] {
        let key = Key { market: h.key.market, day: 900_000, of, strike };
        let (mut s, mut b) = base.s.row_in(&Books::opened(), key, h.scale, h.price_max, 0, 1, Some(2)).unwrap();
        let gross = 1_000 * h.scale;
        b.leaves[1] = Some(Row { net: sign * gross, gross, ..b.leaves[1].unwrap() });
        (s.root, s.dgross, s.held, s.basis) = (b.root(), gross, 175 * KAS1, 0);
        let shard = live(&base, s);
        let tr = s.traced_with(&b, &Draw { amount: 0, prices: [P10, 0, 0, 0].into() }, true, T0).unwrap();
        if sign == 0 {
            // (Closed for a row of no net unit: counted at the price moved down, and the shard is not called.)
            assert_eq!(end_sums(&tr), (0, 9_950 * KAS1, 0, 0), "{what}: the gross's worth at 9.95, nothing else");
            assert_eq!(s.call_trace_end_judged(&tr, T0, shard.value as i64).map(|x| x.0), Some(false), "V2 (closed): {what}: no call");
            continue;
        }
        assert_eq!(end_sums(&tr), (0, 10_050 * KAS1, 0, 0), "{what}: the gross's worth at 10.05, nothing else");
        assert_eq!((up_t(10_000 * KAS1, rg), up_t(9_950 * KAS1, rg), up_t(10_050 * KAS1, rg)), (s.held, s.held - 875 * KAS1 / 1_000, s.held + 875 * KAS1 / 1_000));
        let st = statement(&w, &shard, true, &tr);
        assert!(s.call_trace_end_right(&tr, &st.claimer, &st.outs, st.held, T0), "V2 is open ({what}): the call is the rule's");
        let live_st = w.stated(&shard, &st, &tr.said());
        for p in row_points(&tr) {
            assert_eq!(point_taken(&w, &live_st, &s, &b, &tr, p), (true, false), "{what}: the point {p}");
        }
        let q = w.pointed(&live_st, 0);
        let take = |v: bool| call_end_t(&w.lib, &q, &s, &tr, &w.script("KpfTCallEnd"), xk(&w.w), w.payer(&w.w, &f), Some(v)).is_ok_and(|(bb, _)| accepted(&bb, &w.keys));
        assert_eq!((take(true), take(false)), (true, false), "V2 is open ({what}): the engine takes the call of a shard that holds its lower requirement at the price said");
        // (Counted at the price said, or moved down, the same shard is not called.)
        for wg in [10_000 * KAS1, 9_950 * KAS1] {
            let mut other = tr.clone();
            for r in other.dated.iter_mut().skip(1) {
                r.wg = wg;
            }
            assert_eq!(s.call_trace_end_judged(&other, T0, st.held).map(|x| x.0), Some(false), "{what}: by {wg} of gross worth it is not called");
        }
    }
}

/// 3 (sizes). A row of the largest sizes at the largest limits and prices (units to the largest number, a price at
/// the limit of 2^60, prices either side of the largest a call's move multiplies): where the model computes what
/// the row adds, the engine takes "right" for it and "wrong" for a sompi more asked, and not the other; where the
/// model computes none (a number beyond 64 bits), the engine takes neither verdict.
#[test]
fn t17_held_6_a_row_of_the_largest_sizes_has_the_models_verdict_or_none() {
    let w = world();
    let base = w.bare();
    let market = w.head(0).key.market;
    let mut g = Rng(0x17_0006);
    let (mut some, mut none) = (0, 0);
    for round in 0..90 {
        let scale = [1, 100_000_000, SCALE_MAX][g.below(3) as usize];
        let limit = [MARGIN_MAX, 100_000_000_000_000, 917_000_000_000_000, 930_000_000_000_000][g.below(4) as usize];
        let p = [limit, limit / 2, limit - 1, 1, 917_702_000_000_000i64.min(limit), 926_000_000_000_000i64.min(limit), 918_000_000_000_000i64.min(limit)][g.below(7) as usize];
        let strike = [1, limit, p, p / 2 + 1][g.below(4) as usize].clamp(1, limit);
        let gross = [1, scale, MARGIN_MAX, i64::MAX / 2, i64::MAX, 4_000_000_000_000_000_000, 1_000_000 * scale.min(1_000_000), 9_000_000_000][g.below(8) as usize];
        let net = [gross, -gross, 0][g.below(3) as usize];
        let key = Key { market, day: 900_000, of: 1 + g.below(2), strike };
        let (mut s, mut b) = base.s.row_in(&Books::opened(), key, scale, limit, 0, 5, Some(9)).unwrap();
        b.leaves[5] = Some(Row { net, gross, ..b.leaves[5].unwrap() });
        (s.root, s.dgross) = (b.root(), gross);
        let shard = live(&base, s);
        let (row, day, head) = (b.leaves[5].unwrap(), b.leaves[9].unwrap(), b.row(&Key::HEAD).unwrap());
        for call in [false, true] {
            let adds = s.dated_adds(&row, p, call);
            let (sn, wg, ro) = adds.unwrap_or((1, 1, 1));
            let make = |ro: i64| Traced {
                amount: if call { 0 } else { KAS1 },
                e: 1,
                list: vec![Priced { market, limit, day: 0, price: p }],
                n: 4,
                rows: (0..4).map(|i| TraceRow { place: i, at: 0, wn: 0, wg: 0, sn: 0 }).collect(),
                dated: vec![DatedTrace { place: 0, at: 0, wn: 0, wg: 0, sn: 0, ro: 0, nk: head.next }, DatedTrace { place: 5, at: 0, wn: 0, wg, sn, ro, nk: row.next }, DatedTrace { place: 9, at: 0, wn: 0, wg, sn, ro, nk: day.next }],
            };
            let t = make(ro);
            let judged = s.trace_dated_judged(&b, &t, 2, call, T0);
            assert_eq!(judged, adds.map(|_| true), "round {round}: the model's verdict of what the model adds");
            let live_st = w.stated(&shard, &statement(&w, &shard, call, &t), &t.said());
            assert_eq!(dated_taken(&w, &live_st, &s, &b, &t, 2), verdicts(judged), "round {round} (call {call}): {row:?} at {p}: the model says {adds:?}");
            if adds.is_some() {
                some += 1;
                if ro < i64::MAX {
                    let t2 = make(ro + 1);
                    let live2 = w.stated(&shard, &statement(&w, &shard, call, &t2), &t2.said());
                    assert_eq!(dated_taken(&w, &live2, &s, &b, &t2, 2), (false, true), "round {round} (call {call}): a sompi more asked");
                }
            } else {
                none += 1;
            }
        }
    }
    println!("rows of the largest sizes: {some} with a verdict, the engine's the model's; {none} with none, taken neither way");
    assert!(some > 30 && none > 30);
}

// ================================================================================================================
// 4. A1 at the engine: the root in the books' hash, a futures row as its market's head, a called shard's claims.

use kpf_protocol::reserve::{Called, Claim, Claims, Left, Rest, CALL_HOLD};

fn fleg(w: &World, i: usize, units: i64, dir: i64, entry: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir, entry }
}
fn cov(w: &World, shard: &OLive<Shard>, legs: &[XLeg], coins: i64) -> OLive<Account> {
    let s = Account { t: w.terms(), legs: legs.to_vec(), a_key: xk(&w.w), b_key: xk(&w.c), a_margin: coins, b_margin: 0, expiry: 0, dispute_daa: 50, bond_bps: 1_000, a_min_counter: legs.len() as i64, b_min_counter: 0, shard: Some(shard.s.sid) };
    OLive { s, value: coins as u64, id: shard.id, op: op(80), daa: 6_100 }
}
/// A shard of the world whose market 0 has a call listed and its day's leaf: its futures row at the place 0 is the
/// market's head, of no unit, counting two; the root is not the opened one.
fn listed_shard(w: &World) -> (Shard, Books, Key) {
    let h = w.head(0);
    let call = Key { market: h.key.market, day: 900_000, of: OF_CALLS, strike: P10 };
    let (s, b) = w.bare().s.row_in(&Books::opened(), call, h.scale, h.price_max, 0, 1, Some(2)).unwrap();
    assert!(s.root != Shard::root_opened() && s.rows[0] == ShardRow { entered: 2, ..ShardRow::of(h, 0, 0) });
    (s, b, call)
}
/// Whether the engine takes the transaction with its output `k` written as the shard `wrong`.
fn with_shard_out(w: &World, b: &OBuilt, k: usize, wrong: &Shard) -> bool {
    let mut t = b.clone();
    t.tx.outputs[k].script_public_key = hash_spk(&w.lib, &wrong.hash());
    accepted(&t, &w.keys)
}

/// 4. A futures leg enters a market's head of no unit (`KpfTEnterS`): the count and the root stay, and no other
/// shard is written (the count nothing, the opened root). The last futures leg leaves a head that counts leaves
/// (`KpfTLeft`, and two legs by `KpfTLeftAll`; of a called shard by their twins): the head stands with no unit, and
/// the empty row is refused; with a count of nothing the empty row is written, and the head kept is refused.
#[test]
fn t17_held_7_a_futures_leg_enters_and_leaves_a_head_and_the_count_and_the_root_stay() {
    let w = world();
    let base = w.bare();
    let h = w.head(0);
    let (s1, _, _) = listed_shard(&w);
    let shard = live(&base, s1);
    // An entry.
    let acct = cov(&w, &shard, &[], 20 * KAS1);
    let leg = fleg(&w, 0, 10, LONG, P10);
    let f = [fund(xk(&w.w), 33)];
    let (b, _, _, _, _, next) = c_enter_state(&w.lib, &acct, &shard, &w.script("KpfTEnterS"), SEED, leg, 5 * KAS1, &[], T0, w.payer(&w.w, &f)).unwrap();
    let sg = signed("a futures leg enters a head of no unit", &b, &w.keys);
    assert_eq!((next.rows[0].entered, next.rows[0].gross, next.root, next.gross_sum()), (2, leg.size, s1.root, leg.size));
    let k = sg.tx.outputs.iter().position(|o| o.script_public_key == hash_spk(&w.lib, &next.hash())).expect("the shard's output");
    for (what, wrong) in [
        ("the count nothing", Shard { rows: { let mut r = next.rows; r[0].entered = 0; r }, ..next }),
        ("the count one more", Shard { rows: { let mut r = next.rows; r[0].entered = 3; r }, ..next }),
        ("the opened root", Shard { root: Shard::root_opened(), ..next }),
        ("the sum of the gross without the leg", Shard { dgross: -leg.size, ..next }),
    ] {
        assert!(!with_shard_out(&w, &b, k, &wrong), "an entry writes {what}");
    }
    // (A leg of another scale than the head's does not enter the head, nor an empty row beside it.)
    let other = XLeg { scale: leg.scale / 10, ..leg };
    for r in 0..2 {
        let got = c_enter_state(&w.lib, &acct, &shard, &w.script("KpfTEnterS"), SEED, other, 5 * KAS1, &[], T0, w.payer(&w.w, &f));
        assert!(got.is_err(), "the model takes a leg of another scale into the market's head (row {r})");
    }
    // A leaving: of one leg, and of two.
    let fw = [fund(xk(&w.w), 43)];
    let mut twins = 0;
    for called in [0, 6_000] {
        for legs in [vec![leg], vec![leg, fleg(&w, 0, 4, SHORT, P10 * 11 / 10)]] {
            for counted in [true, false] {
                let mut s = if counted { s1 } else { base.s };
                for l in &legs {
                    s = s.booked(l).unwrap();
                }
                s.called = called;
                assert_eq!(s.rows[0].entered, if counted { 2 } else { 0 }, "a leg booked keeps the count");
                let on = live(&base, s);
                let note = OLive { s: Left { sid: s.sid, legs: legs.clone(), key: xk(&w.w), asked: 0 }, value: NOTE_MIN as u64, id: on.id, op: op(61), daa: 6_200 };
                let name = left_script_on(&s, legs.len());
                let built = if called == 0 { left_apply(&w.lib, &on, &note, &w.script(name), w.payer(&w.w, &fw)).map(|(b, n, _)| (b, n)) } else { left_apply_c(&w.lib, &on, &note, 6, 0, &w.script(name), w.payer(&w.w, &fw)).map(|(b, n, _)| (b, n)) };
                let (b, next) = match built {
                    Ok(x) => x,
                    Err(e) => {
                        assert!(called != 0, "{name}: {e}");
                        println!("NOT REACHED: {name} on a called shard: {e}");
                        continue;
                    }
                };
                let sg = signed(&format!("{name}: the last legs leave a row that counts {} leaves", s.rows[0].entered), &b, &w.keys);
                if called != 0 {
                    twins += 1;
                }
                let k = sg.tx.outputs.iter().position(|o| o.script_public_key == hash_spk(&w.lib, &next.hash())).expect("the shard's output");
                let (head, empty) = (ShardRow { entered: s.rows[0].entered, ..ShardRow::of(h, 0, 0) }, ShardRow::EMPTY);
                assert_eq!((next.rows[0], next.root), (if counted { head } else { empty }, s.root), "{name}");
                let other = Shard { rows: { let mut r = next.rows; r[0] = if counted { empty } else { head }; r }, ..next };
                assert!(!with_shard_out(&w, &b, k, &other), "{name}: {} is written", if counted { "the empty row, while the head counts leaves" } else { "a head of no unit that counts none" });
                assert!(!with_shard_out(&w, &b, k, &Shard { root: Shard::root_opened(), ..next }) || !counted, "{name}: the opened root is written");
                if counted {
                    assert!(!with_shard_out(&w, &b, k, &Shard { rows: { let mut r = next.rows; r[0].entered = 0; r }, ..next }), "{name}: the count is written nothing");
                }
            }
        }
    }
    println!("a head entered and left: the count and the root stay ({twins} of the leavings of a called shard)");
}

/// 4 (the root). A shard written with a root that is not its books' (no script writes one): nothing opens it. No
/// row is listed or taken out, and no dated point of a trace is answered either way by the rows' script.
#[test]
fn t17_held_8_a_root_that_is_not_the_books_is_opened_by_nothing() {
    let w = world();
    let t = tmpl(&w);
    let (shard, b) = dated_shard(&w);
    let h = w.head(0);
    let odd = Shard { root: [0x99; 32], ..shard.s };
    let key = Key { market: h.key.market, day: 900_000, of: OF_CALLS, strike: 7 };
    let a = in_honest(&shard.s, &b, key, h.scale, h.price_max, 40, 41);
    assert!(in_taken(&w, &t, &shard.s, &a) && !in_taken(&w, &t, &odd, &a), "a row listed into books of another root");
    let free = b.place_of(&Key { market: h.key.market, day: 1_800_000, of: OF_CALLS, strike: P10 }).unwrap();
    let o = out_honest(&shard.s, &b, free, 0, Some(shard.s.owner));
    assert!(out_taken(&w, &t, &shard.s, &o) && !out_taken(&w, &t, &odd, &o), "a row taken out of books of another root");
    let on = live(&shard, odd);
    for call in [false, true] {
        let tr = shard.s.traced_with(&b, &Draw { amount: if call { 0 } else { KAS1 }, prices: PRICES.into() }, call, T0).unwrap();
        let st = statement(&w, &on, call, &tr);
        let live_st = w.stated(&on, &st, &tr.said());
        for k in 1..=tr.dated.len() as i64 {
            let q = w.pointed(&live_st, 100 + k);
            let f = [fund(xk(&w.w), 32)];
            // (The builder is the true books'; the shard shown is the one with the other root.)
            let take = |v: bool| shard_row_d(&w.lib, &q, &Shard { root: b.root(), ..odd }, &b, &tr, k, &w.script("KpfTShardRowD"), xk(&w.w), w.payer(&w.w, &f), Some(v)).is_ok_and(|(bb, _)| accepted(&bb, &w.keys));
            assert_eq!((take(true), take(false)), (false, false), "the dated point {k} of a shard of another root");
        }
    }
}

/// 4 (the root carried). The scripts that show the rows and write none, on a shard whose root is not the opened one
/// (a head of no unit counting two leaves): a note of the rest paid (`KpfTRest`), a note of a leg that is not in
/// the books spent (`KpfTLeftVoid`: the head has fewer units than the note's leg), a note of a mark applied under a
/// call (`KpfTMarkNoteC`, which is shown the books in 101 bytes). Each is taken, and with the opened root written
/// in its place refused.
#[test]
fn t17_held_9_the_scripts_that_write_no_row_carry_the_root() {
    use kpf_protocol::reserve::MarkNote;
    let w = world();
    let base = w.bare();
    let (s1, _, _) = listed_shard(&w);
    let f = [fund(xk(&w.f), 44)];
    let leg = fleg(&w, 0, 10, LONG, P10);
    let mut reached = vec![];
    let mut check = |what: &'static str, b: &OBuilt, next: &Shard| {
        let sg = signed(what, b, &w.keys);
        let k = sg.tx.outputs.iter().position(|o| o.script_public_key == hash_spk(&w.lib, &next.hash())).unwrap_or_else(|| panic!("{what}: the shard's output"));
        assert_eq!(next.root, s1.root, "{what}: the root stays");
        assert!(!with_shard_out(&w, b, k, &Shard { root: Shard::root_opened(), ..*next }), "{what}: the opened root is written");
        reached.push(what);
    };
    // A note of the rest.
    let s = Shard { owed: 3 * KAS1, ..s1 };
    let rest = OLive { s: Rest { sid: s.sid, key: xk(&w.w), amount: 3 * KAS1 }, value: NOTE_MIN as u64, id: base.id, op: op(62), daa: 6_200 };
    match rest_apply(&w.lib, &live(&base, s), &rest, &w.script("KpfTRest"), w.payer(&w.f, &f)) {
        Ok((b, next, _)) => check("KpfTRest", &b, &next),
        Err(e) => println!("NOT REACHED: KpfTRest: {e}"),
    }
    // A note of a leg that is not in the books.
    let note = OLive { s: Left { sid: s1.sid, legs: vec![leg], key: xk(&w.w), asked: 0 }, value: NOTE_MIN as u64, id: base.id, op: op(61), daa: 6_200 };
    match left_void(&w.lib, &live(&base, s1), &note, 0, &w.script("KpfTLeftVoid"), w.payer(&w.f, &f)) {
        Ok(b) => check("KpfTLeftVoid", &b, &s1),
        Err(e) => println!("NOT REACHED: KpfTLeftVoid: {e}"),
    }
    // A note of a mark, under a call.
    let sc = Shard { called: 6_000, ..s1.booked(&leg).unwrap() };
    let mark = OLive { s: MarkNote { sid: sc.sid, leg, entry: P10 * 11 / 10, key: xk(&w.w), asked: 10 * KAS1 }, value: NOTE_MIN as u64, id: base.id, op: op(63), daa: 6_200 };
    match mark_apply_c(&w.lib, &live(&base, sc), &mark, &w.script(mark_script_on(&sc)), w.payer(&w.f, &f)) {
        Ok((b, next, _)) => check(mark_script_on(&sc), &b, &next),
        Err(e) => println!("NOT REACHED: {}: {e}", mark_script_on(&sc)),
    }
    println!("the root carried by: {reached:?}");
    assert!(reached.len() >= 2);
}

/// V3 (medium). `KpfTCallPay` reads the sum of the gross and no row (A1; docs 17.8j, 2; review T16, Z3): a called
/// shard whose futures rows are heads of no unit that count leaves pays its claims and ends its call, at the
/// engine. THE MODEL DOES NOT: `Shard::claim_paid_at` and `Shard::call_off` still ask that the four rows are the
/// empty row, so the builders `claim_pay_c`, `claim_pay` and `call_off` refuse to build what the script takes. And
/// with a unit in a dated row nothing is paid and the call does not end, by the sum.
#[test]
fn t17_v3_the_model_refuses_the_claims_of_a_called_shard_whose_heads_count_leaves() {
    let w = world();
    let base = w.bare();
    let (s1, b1, call_key) = listed_shard(&w);
    let pay_s = w.script("KpfTCallPay");
    let s = Shard { held: 50 * KAS1, owed: 30 * KAS1, called: 6_400, ..s1 };
    let twin = Shard { rows: [ShardRow::EMPTY; 4].into(), root: Shard::root_opened(), ..s };
    let on = live(&base, s);
    let c = OLive { s: Rest { sid: s.sid, key: xk(&w.w), amount: 30 * KAS1 }.claim(), value: NOTE_MIN as u64, id: base.id, op: op(71), daa: 6_600 };
    let fl = [KeyUtxo { daa: s.claims_time().unwrap() as u64, ..fund(xk(&w.f), 36) }];
    // The model: refused with the head, taken of the same shard with four empty rows.
    let (next_t, paid, left) = twin.claim_paid_at(&c.s, i64::MAX).expect("the twin of four empty rows pays");
    assert_eq!((paid, left), (30 * KAS1, None));
    // (Closed: the model pays as the script does, the head and the root as they are; and the builder builds it.)
    assert_eq!(s.claim_paid_at(&c.s, i64::MAX).ok(), Some((Shard { rows: s.rows, root: s.root, ..next_t }, paid, left)), "V3 (closed): the model pays the claim while a head stands");
    assert!(claim_pay_c(&w.lib, &on, &c, &pay_s, w.payer(&w.f, &fl)).is_ok(), "V3 (closed): the builder builds it");
    // The engine: the claim is paid, the head and the root as they are.
    let next = Shard { rows: s.rows, root: s.root, ..next_t };
    let b = claim_pay_as_c(&w.lib, &on, &c, next, paid, left, &pay_s, w.payer(&w.f, &fl)).unwrap();
    let sg = signed("V3: a claim paid by a called shard whose head counts two leaves", &b, &w.keys);
    let k = sg.tx.outputs.iter().position(|o| o.script_public_key == hash_spk(&w.lib, &next.hash())).expect("the shard's output");
    assert!(!with_shard_out(&w, &b, k, &Shard { root: Shard::root_opened(), ..next }) && !with_shard_out(&w, &b, k, &Shard { rows: twin.rows, ..next }), "the claim's payment writes another root, or empties the head");
    // The call ends: the model refuses, the engine takes it.
    let after = live(&base, next);
    let call = OLive { s: Called { sid: s.sid, time: s.called, heads: Default::default(), prices: Default::default() }, value: CALL_HOLD as u64, id: base.id, op: op(91), daa: 6_500 };
    let ended_t = Shard { rows: twin.rows, root: twin.root, ..next }.call_off(&call.s).expect("the twin's call ends");
    let ended = Shard { rows: s.rows, root: s.root, ..ended_t };
    assert!(next.call_off(&call.s).ok() == Some(ended) && call_off(&w.lib, &after, &call, &pay_s, w.payer(&w.f, &fl)).is_ok(), "V3 (closed): the model ends the call while a head stands");
    let b = call_off_as(&w.lib, &after, &call, ended, &pay_s, w.payer(&w.f, &fl)).unwrap();
    signed("V3: the call of a shard whose head counts two leaves ended", &b, &w.keys);
    assert_eq!((ended.called, ended.rows[0].entered, ended.root), (0, 2, s.root));
    // With a unit in a dated row: nothing is paid and the call does not end, by the sum of the gross.
    let mut bu = b1.clone();
    let at = bu.place_of(&call_key).unwrap();
    bu.leaves[at] = Some(Row { net: -5, gross: 5, ..bu.leaves[at].unwrap() });
    let su = Shard { root: bu.root(), dgross: 5, ..s };
    let b = claim_pay_as_c(&w.lib, &live(&base, su), &c, Shard { root: su.root, dgross: 5, ..next }, paid, left, &pay_s, w.payer(&w.f, &fl));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a claim is paid while a dated row has a unit");
    let b = call_off_as(&w.lib, &live(&base, Shard { root: su.root, dgross: 5, ..next }), &call, Shard { root: su.root, dgross: 5, ..ended }, &pay_s, w.payer(&w.f, &fl));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a call ends while a dated row has a unit");
    // (The same sum with no unit anywhere is a shard no script writes: the scripts hold the sum, not the rows.)
}

/// V4 (low). The call's object is written from the list the call's statement says (`KpfTCallEnd`), and the model's
/// trace lists no market whose futures row has no unit (17.8j, 6). The model's own call (`Shard::called_at`,
/// `Called::list`, by which `Shard::call_outs` and the builder `shard_call` write a call's outputs) lists EVERY
/// row's market: for a shard with a head of no unit it is another object than the one the script writes.
#[test]
fn t17_v4_the_models_call_lists_a_head_of_no_unit_that_the_trace_does_not() {
    let w = world();
    let (s1, b1, _) = listed_shard(&w);
    // (Units in market 1; market 0 a head of no unit that counts two leaves.)
    let h1 = w.head(1);
    let mut s = s1;
    s.rows[1] = ShardRow::of(h1, -60 * h1.scale, 140 * h1.scale);
    let prices = [P10, P10, 0, 0];
    let t = s.traced_with(&b1, &Draw { amount: 0, prices: prices.into() }, true, T0).unwrap();
    let (_, call) = s.called_at(&prices, T0, 0);
    assert_eq!((t.list.len(), call.list().len()), (1, 1), "V4 (closed): the trace lists one market, and the model's call the same");
    assert_eq!(Called::hash_of(&s.sid, T0, &t.list_bytes()), call.hash(), "V4 (closed): the call the script writes is the model's");
    // (And the builders of a draw and a call state the trace of the four rows alone, whatever the tree: wrong at
    // the point 101 for this shard, by the model.)
    let four = s.traced(&Draw { amount: 0, prices: prices.into() }, true).unwrap();
    assert_eq!(s.trace_dated_judged(&b1, &four, 1, true, T0), Some(false));
}
