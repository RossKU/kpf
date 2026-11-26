//! A leg enters a covered account by a statement of its two parties (`KpfTEnter`, kind 6; `KpfTStepEnter`;
//! docs/ARCHITECTURE-CLEARING.md 11): the closure of review 37's F61. The entry of a leg is a price, and a price is
//! stated and answered for: the leg enters at the price its row states, of a committee the account's shard is of,
//! with side A at its maintenance; a disputed point of an entry that nobody answers costs its claimers their bond.
//! Every transaction is built, signed and run through `verify::verify`.
#![allow(unused_imports, dead_code)]

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cite;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::cross::{XLeg, XTerms, SIDE_A, SIDE_B};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::reserve::{futures_market, option_market, Head, SNote, Shard, ShardRow, NOTE_JOIN, NOTE_RELEASE, SHARD_CARRIER};
use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::{holder_of, option_dir, option_of, ATake, Account, Out, Outs, Pointed, Stated, CALL_HELD, CALL_WRITTEN, KIND_ACCOUNT_LEAVE, KIND_ACCOUNT_LIQ, KIND_ACCOUNT_TAKE, PUT_HELD, PUT_WRITTEN};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::cell::RefCell;

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;
const P10: i64 = 1_000_000_000;
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
    KeyUtxo { outpoint: op(n), amount: 1_000 * KAS1 as u64, daa: 0, key }
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
fn rate_up(w: i64, r: i64) -> i64 {
    (w / BPS) * r + ((w % BPS) * r + BPS - 1) / BPS
}

/// `a` is the user (side A), `b` the other party (side B: the clearing party where the account is covered), `w`
/// anyone.
struct World {
    a: Keypair,
    b: Keypair,
    w: Keypair,
    f: Keypair,
    members: [Keypair; 3],
    keys: KeySet,
    rid: Hash,
    markets: Vec<CMarket>,
    table: OLive<EpochTable>,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
    anchors: Vec<Anchor>,
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
    /// A futures leg of `units` of market `i`.
    fn fut(&self, i: usize, units: i64, dir: i64, entry: i64) -> XLeg {
        let m = &self.markets[i];
        XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir, entry }
    }
    /// An option on `units` of market `i`, struck at `strike`, last counted at `entry`.
    fn opt(&self, i: usize, units: i64, kind: i64, strike: i64, entry: i64) -> XLeg {
        self.fut(i, units, option_dir(kind, strike), entry)
    }
    /// An account of A against B that no shard covers.
    fn account(&self, legs: Vec<XLeg>, am: i64, bm: i64) -> OLive<Account> {
        let s = Account { t: self.terms(), legs, a_key: xk(&self.a), b_key: xk(&self.b), a_margin: am, b_margin: bm, expiry: 6_000 + self.markets[0].p.term, dispute_daa: 50, bond_bps: 100, a_min_counter: 0, b_min_counter: 0, shard: None };
        s.check().unwrap();
        OLive { value: (s.pot() + KAS1) as u64, s, id: Some(Hash::from_bytes([0x72; 32])), op: op(11), daa: 6_100 }
    }
    /// The committee's price of market `i` for T0 written as a note and accepted by a chain block: its citation.
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
    fn who(&self, key: &[u8; 32]) -> &Keypair {
        if *key == xk(&self.a) {
            &self.a
        } else if *key == xk(&self.b) {
            &self.b
        } else {
            &self.w
        }
    }
    /// The statement `st` made on the account, landed at 6,510.
    fn stated(&self, account: &OLive<Account>, st: &Stated, said: &[u8]) -> OLive<Stated> {
        let who = self.who(&st.claimer);
        let f = [fund(xk(who), 30)];
        let b = account_state(&self.lib, account, &self.script("KpfTChange"), st, said, self.payer(who, &f)).unwrap();
        let s = b.sign(&self.keys).expect("stated");
        let v = kpf_protocol::verify::verify(&s.tx, &s.entries);
        assert!(v.is_ok(), "stated: {:?}", v.errors());
        nth(&s, 0, *st, 6_510)
    }
    fn pointed(&self, st: &OLive<Stated>, point: i64) -> OLive<Pointed> {
        let f = [fund(xk(&self.w), 31)];
        let (b, q) = point_dispute(&self.lib, st, &self.script("KpfTPoint"), xk(&self.w), point, self.payer(&self.w, &f)).unwrap();
        let s = b.sign(&self.keys).expect("pointed");
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok());
        nth(&s, 0, q, 6_520)
    }
    /// The row `i` (from 1) of the stated trace answered by `KpfTStepRow` with the leg: the verdict the model
    /// gives is taken by the script, and the other is refused.
    fn row_answered(&self, what: &str, account: &OLive<Account>, st: &Stated, tk: &ATake, i: usize, leg: &XLeg) -> bool {
        let q = self.pointed(&self.stated(account, st, &tk.said()), 2 * i as i64 - 1);
        let f = [fund(xk(&self.w), 33)];
        let (b, right) = step_row(&self.lib, &q, tk, leg, &self.script("KpfTStepRow"), xk(&self.w), self.payer(&self.w, &f), None).unwrap();
        let s = signed(&format!("{what}: row {i} answered {}", if right { "right" } else { "wrong" }), &b, &self.keys);
        if !right {
            assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&self.lib, &account.s.hash())), "{what}: the account again");
        }
        let forced = step_row(&self.lib, &q, tk, leg, &self.script("KpfTStepRow"), xk(&self.w), self.payer(&self.w, &f), Some(!right));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &self.keys), "{what}: row {i} is answered the other way");
        right
    }
    /// The end of the statement answered by its kind's script (`KpfTStepEnd` for a take, `KpfTStepLiq` for a leg
    /// that leaves): the verdict the model gives is taken, and the other is refused.
    fn end_answered(&self, what: &str, account: &OLive<Account>, st: &Stated, tk: &ATake) -> bool {
        let q = self.pointed(&self.stated(account, st, &tk.said()), 0);
        let f = [fund(xk(&self.w), 33)];
        let script = self.script(if st.kind == KIND_ACCOUNT_TAKE { "KpfTStepEnd" } else { "KpfTStepLiq" });
        let (b, right) = step_end(&self.lib, &q, &account.s, tk, &script, xk(&self.w), self.payer(&self.w, &f), None).unwrap();
        signed(&format!("{what}: end answered {}", if right { "right" } else { "wrong" }), &b, &self.keys);
        let forced = step_end(&self.lib, &q, &account.s, tk, &script, xk(&self.w), self.payer(&self.w, &f), Some(!right));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &self.keys), "{what}: the end is answered the other way");
        right
    }
    /// The statement applied after its dispute period.
    fn settled(&self, what: &str, account: &OLive<Account>, st: &Stated, tk: &ATake) -> OSigned {
        let live = self.stated(account, st, &tk.said());
        let f = [fund(xk(&self.w), 45)];
        let b = stated_settle(&self.lib, &live, &self.script("KpfTChangeSettle"), self.payer(&self.w, &f)).unwrap();
        signed(&format!("{what}: applied"), &b, &self.keys)
    }
    /// The clearing party's shard, opened through the first anchor with 50 KAS.
    fn shard(&self) -> OLive<Shard> {
        let f = [fund(xk(&self.b), 40)];
        let (b, shard) = shard_open(&self.lib, &self.anchors[0], &self.script("KpfTShardOpen"), self.terms(), self.markets[0].committee_id, xk(&self.b), 50 * KAS1, 9_000_000, 50, 100, self.payer(&self.b, &f)).unwrap();
        let s = signed("shard opened", &b, &self.keys);
        nth(&s, 0, shard, 6_100)
    }
    /// A covered account of the user against the clearing party, with 20 and 2 KAS of margin and no leg.
    fn covered(&self, sid: [u8; 32]) -> OLive<Account> {
        let a = Account { t: self.terms(), legs: vec![], a_key: xk(&self.a), b_key: xk(&self.b), a_margin: 20 * KAS1, b_margin: 2 * KAS1, expiry: 9_000_000, dispute_daa: 50, bond_bps: 100, a_min_counter: 0, b_min_counter: 0, shard: Some(sid) };
        let f = [fund(xk(&self.a), 41)];
        let b = account_open_c(&self.lib, &self.anchors[1], &self.script("KpfTOpenC"), &a, shard_seed(&self.anchors[0]), self.markets[0].committee_id, self.payer(&self.a, &f)).unwrap();
        let s = signed("covered account opened", &b, &self.keys);
        nth(&s, 0, a, 6_100)
    }
    fn added(&self, account: &OLive<Account>, leg: XLeg) -> (OLive<Account>, OLive<SNote>) {
        let f = [fund(xk(&self.a), 42)];
        let (b, next, note) = leg_add(&self.lib, account, &self.script("KpfTLegAdd"), leg, 0, 0, self.payer(&self.a, &f)).unwrap();
        let s = signed("leg added", &b, &self.keys);
        (nth(&s, 0, next, 6_110), nth(&s, 2, note, 6_110))
    }
    fn applied(&self, shard: &OLive<Shard>, note: &OLive<SNote>, what: &str) -> OLive<Shard> {
        let f = [fund(xk(&self.w), 43)];
        let (b, next, _) = note_apply(&self.lib, shard, note, &self.script("KpfTNoteApply"), self.payer(&self.w, &f)).unwrap();
        let s = signed(what, &b, &self.keys);
        nth(&s, 0, next, shard.daa + 10)
    }
}

fn world() -> World {
    let (a, b, w, f) = (kp(1), kp(2), kp(3), kp(9));
    let members = [kp(101), kp(102), kp(103)];
    let mut keys = KeySet::new();
    for k in [&a, &b, &w, &f] {
        keys.add_keypair(*k);
    }
    let rid = Hash::from_bytes([0x52; 32]);
    let rp = CommitteeParams::testnet();
    let markets = (0..8u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
    let mut table = EMPTY_TABLE;
    for (i, wt) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: wt * KAS1 };
    }
    let table = OLive { s: EpochTable { members: table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(20), daa: 6_000 };
    let funds = vec![fund(xk(&f), 200)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    let funds = vec![fund(xk(&f), 201)];
    let (r, _) = registry_genesis(&lib, 2, KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let rs = r.sign(&keys).expect("registry");
    let anchors = registry_anchors(&rs.tx, &lib, 6_000);
    World { a, b, w, f, members, keys, rid, markets, table, lib, scripts: lib_scripts(&gs.tx, 6_000), anchors, chain: RefCell::new(vec![]) }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
}


use kpf_protocol::thin::{row_after, AEnter, Row, KIND_ENTER};

/// The entry stated on the account by its two parties, landed at 6,510.
fn stated_entry(w: &World, account: &OLive<Account>, st: &Stated, en: &AEnter) -> OLive<Stated> {
    let f = [fund(xk(&w.a), 30)];
    let b = enter_state(&w.lib, account, st, &en.said(), &w.script("KpfTEnter"), w.payer(&w.a, &f)).unwrap();
    let s = b.sign(&w.keys).expect("stated");
    let v = kpf_protocol::verify::verify(&s.tx, &s.entries);
    assert!(v.is_ok(), "an entry is stated: {:?}", v.errors());
    nth(&s, 0, *st, 6_510)
}

/// A statement that `l` enters `x` at the price `a`, written by hand as an honest one is: for what the model does
/// not state.
fn by_hand(x: &Account, l: XLeg, a: i64, seed: [u8; 32], held: i64) -> (Stated, AEnter) {
    let rows = vec![row_after(&Row::ZERO, &l, a, false)];
    let en = AEnter { side: SIDE_A, amount: 0, skip: 1, leg: l, seed, rows };
    let after = Account { legs: vec![XLeg { entry: a, ..l }], ..x.clone() };
    let note = SNote { gross: l.size.abs(), ..SNote::leg(NOTE_JOIN, x.shard.unwrap(), &l, x.b_key) };
    let outs = Outs::with_notes(&[(Out { key: false, to: after.hash(), value: held }, false), (Out { key: false, to: note.hash(), value: note.value() }, true)]).unwrap();
    (Stated { position: x.hash(), window: x.dispute_daa, late: x.t.late_daa, kind: KIND_ENTER, t0: T0, arg: en.arg(), outs, bond: x.bond(), claimer: x.a_key, held }, en)
}

/// The end of the entry answered by `KpfTStepEnter`: the model's verdict is taken, and the other refused.
fn end_answered(w: &World, what: &str, account: &OLive<Account>, st: &Stated, en: &AEnter) -> bool {
    let q = w.pointed(&stated_entry(w, account, st, en), 0);
    let f = [fund(xk(&w.w), 33)];
    let script = w.script("KpfTStepEnter");
    let (b, right) = enter_end(&w.lib, &q, &account.s, en, &script, xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    let s = signed(&format!("{what}: the end of an entry answered {}", if right { "right" } else { "wrong" }), &b, &w.keys);
    if !right {
        assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &account.s.hash())), "{what}: the account again, without the leg");
    }
    let forced = enter_end(&w.lib, &q, &account.s, en, &script, xk(&w.w), w.payer(&w.w, &f), Some(!right));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "{what}: the end is answered the other way");
    right
}

/// Row `i` of the entry answered by `KpfTStepRow` with the leg.
fn row_answered(w: &World, what: &str, account: &OLive<Account>, st: &Stated, en: &AEnter, i: usize, prev: [u8; 32], leg: &XLeg) -> bool {
    let q = w.pointed(&stated_entry(w, account, st, en), 2 * i as i64 - 1);
    let f = [fund(xk(&w.w), 33)];
    let script = w.script("KpfTStepRow");
    let (b, right) = enter_row(&w.lib, &q, en, 2 * i as i64 - 1, &prev, leg, &script, xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    signed(&format!("{what}: row {i} of an entry answered {}", if right { "right" } else { "wrong" }), &b, &w.keys);
    let forced = enter_row(&w.lib, &q, en, 2 * i as i64 - 1, &prev, leg, &script, xk(&w.w), w.payer(&w.w, &f), Some(!right));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "{what}: row {i} is answered the other way");
    right
}

#[test]
fn a_leg_enters_a_covered_account_by_a_statement_of_its_two_parties() {
    let w = world();
    let shard = w.shard();
    let account = w.covered(shard.s.sid);
    let seed = shard_seed(&w.anchors[0]);
    assert_eq!(shard.s.sid, shard_id(&w.anchors[0], &w.markets[0].committee_id, &shard.s), "a shard is of one committee and of its terms: its id says which");
    let x = &account.s;
    let held = account.value as i64;
    // The user goes long 10 units of market 0 at 10: the two state it, and the leg's entry is the price stated.
    let f = [fund(xk(&w.a), 30)];
    let (b, st, en, after) = account_enter(&w.lib, &account, &w.script("KpfTEnter"), w.fut(0, 10, LONG, 0), &[P10], T0, seed, w.payer(&w.a, &f)).unwrap();
    let s = signed("a leg enters: stated", &b, &w.keys);
    assert_eq!((st.kind, st.claimer, en.rows.len(), en.rows[0].gain), (KIND_ENTER, xk(&w.a), 1, 0), "it gains nothing as it enters");
    assert_eq!((after.legs.len(), after.legs[0].entry, after.a_margin, after.b_margin), (1, P10, 20 * KAS1, 2 * KAS1));
    assert_eq!(x.enter_wrong_point(&en, &st.claimer, &st.outs, held, &[P10]), None, "the entry is the rule's");
    // Either party alone states no entry.
    for out in [&w.a, &w.b] {
        let mut some = KeySet::new();
        for k in [&w.a, &w.b, &w.w, &w.f] {
            if xk(k) != xk(out) {
                some.add_keypair(*k);
            }
        }
        assert!(b.sign(&some).is_err() || !accepted(&b, &some), "a leg enters by one party's statement");
    }
    // Nobody disputes: it is applied after the dispute period. The account has the leg, and the note that it
    // entered is written, as KpfTLegAdd wrote it.
    let live = nth(&s, 0, st, 6_510);
    let f2 = [fund(xk(&w.w), 45)];
    let b = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f2)).unwrap();
    let r = signed("a leg enters: applied", &b, &w.keys);
    let n = r.tx.outputs.len();
    let join = SNote::leg(NOTE_JOIN, shard.s.sid, &after.legs[0], xk(&w.b));
    assert_eq!((r.tx.outputs[0].value, &r.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &after.hash())));
    let at = r.tx.outputs.iter().position(|o| o.script_public_key == hash_spk(&w.lib, &join.hash())).expect("the note that the leg entered");
    let scale = w.markets[0].p.scale;
    let shard = w.applied(&shard, &nth(&r, at, join, 6_570), "the join of a leg that entered by a statement, applied");
    assert_eq!((shard.s.rows[0].head.market, shard.s.rows[0].net, shard.s.rows[0].gross), (futures_market(&w.markets[0].id()), -10 * scale, 10 * scale));
    let _ = n;

    // A second leg enters the account that has one: the trace counts both, and the old leg's entry moves to its
    // price as in any statement. A call struck at its entry, 13, enters; one struck at 10 does not: it would be
    // counted at the 30 it is worth exercised, which its holder is paid none of and must hold whole (it has 20).
    let account = nth(&r, 0, after.clone(), 6_570);
    let x = &account.s;
    let a = [P10, P10 * 13 / 10];
    assert!(x.enter(w.opt(1, 10, CALL_HELD, P10, 0), &a, T0, account.value as i64, seed).is_err(), "a call far in the money enters for nothing");
    let (b, st, en, after) = account_enter(&w.lib, &account, &w.script("KpfTEnter"), w.opt(1, 10, CALL_HELD, P10 * 13 / 10, 0), &a, T0, seed, w.payer(&w.a, &f)).unwrap();
    signed("a second leg enters: stated", &b, &w.keys);
    assert_eq!((en.rows.len(), en.rows[1].gain, en.rows[1].ea, after.legs[1].entry), (2, 0, 0, a[1]));
    assert_eq!(x.enter_wrong_point(&en, &st.claimer, &st.outs, account.value as i64, &a), None);
    // Rows 1 and 2 of it are answered right with their legs.
    assert!(row_answered(&w, "the old leg's row", &account, &st, &en, 1, [0; 32], &x.legs[0]));
    assert!(row_answered(&w, "the entering leg's row", &account, &st, &en, 2, en.rows[0].g, &after.legs[1]));
    assert!(end_answered(&w, "a second leg", &account, &st, &en));
}

#[test]
fn an_entry_at_a_price_of_ones_own_choosing_is_answered_wrong() {
    let w = world();
    let shard = w.shard();
    let account = w.covered(shard.s.sid);
    let seed = shard_seed(&w.anchors[0]);
    let x = &account.s;
    let held = account.value as i64;
    let leg = w.fut(0, 10, LONG, 0);
    let cite1 = w.script("KpfTCite");
    let f = [fund(xk(&w.w), 34)];
    // Review 37's entry: at 0.1 where the committee says 10. The statement's end and row follow from its price;
    // the price is the point that is wrong, answered with one citation: the account again, without the leg.
    let (lie, len, lafter) = x.enter(leg, &[P10 / 100], T0, held, seed).unwrap();
    assert_eq!(x.enter_wrong_point(&len, &lie.claimer, &lie.outs, held, &[P10]), Some(2));
    let q = w.pointed(&stated_entry(&w, &account, &lie, &len), 2);
    let c = w.cited(0, P10);
    let (b, right) = enter_price(&w.lib, &q, x, &lafter.legs, &len, 2, &w.script("KpfTStepPrice"), (&cite1, &c), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    let s = signed("the price of an entry answered wrong", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())));
    let n = s.tx.outputs.len();
    assert_eq!(s.tx.outputs[n - 1].value, (lie.bond / 2) as u64, "half of the claimers' bond to who answered");
    let forced = enter_price(&w.lib, &q, x, &lafter.legs, &len, 2, &w.script("KpfTStepPrice"), (&cite1, &c), xk(&w.w), w.payer(&w.w, &f), Some(true));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys));
    // An entry at the committee's price is answered right at each of its points.
    let (st, en, after) = x.enter(leg, &[P10], T0, held, seed).unwrap();
    let q = w.pointed(&stated_entry(&w, &account, &st, &en), 2);
    let c = w.cited(0, P10 + P10 / 400);
    let (b, right) = enter_price(&w.lib, &q, x, &after.legs, &en, 2, &w.script("KpfTStepPrice"), (&cite1, &c), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    signed("the price of an entry answered right", &b, &w.keys);
    assert!(row_answered(&w, "an entry", &account, &st, &en, 1, [0; 32], &after.legs[0]));
    assert!(end_answered(&w, "an entry", &account, &st, &en));

    // What the leg's row refuses: an entry that is not the row's price (the leg would gain as it enters), a leg
    // beyond what is computed, a kind that is none, an option with no strike, and a leg shown by the statement that
    // is not the one the row is of (whoever watches is shown the leg that enters, whole: review 39, F63).
    assert_eq!(&en.said()[24..135], &kpf_protocol::thin::leg_bytes(&after.legs[0])[..], "the statement shows the leg");
    assert_eq!(AEnter::read(&en.said()).as_ref(), Some(&en));
    let l = XLeg { entry: P10, ..leg };
    let most = kpf_protocol::cross::MARGIN_MAX / l.price_max - 1;
    let odd: Vec<(&str, XLeg, i64)> = vec![
        ("an entry below the row's price", l, P10 * 11 / 10),
        ("a leg a unit beyond what is computed", XLeg { size: (most + 1) * l.scale, ..l }, P10),
        ("a direction that is none", XLeg { dir: 0, ..l }, P10),
        ("an option with no strike", XLeg { dir: CALL_HELD, ..l }, P10),
        ("a strike beyond the market's limit", XLeg { dir: option_dir(CALL_HELD, l.price_max + 1), ..l }, P10),
    ];
    for (what, bad, a) in odd {
        let (lie, len) = by_hand(x, bad, a, seed, held);
        let lie = Stated { ..lie };
        assert!(!row_answered(&w, what, &account, &lie, &len, 1, [0; 32], &bad), "{what}");
    }
    let (honest, en1) = by_hand(x, l, P10, seed, held);
    assert!(row_answered(&w, "the same by hand, as it should be", &account, &honest, &en1, 1, [0; 32], &l));
    for (what, len) in [
        ("another leg shown than the row's: a unit larger", AEnter { leg: XLeg { size: l.size + 1, ..l }, ..en1.clone() }),
        ("another leg shown than the row's: an option", AEnter { leg: XLeg { dir: option_dir(CALL_HELD, P10), ..l }, ..en1.clone() }),
    ] {
        let lie = Stated { arg: len.arg(), ..honest };
        assert!(!row_answered(&w, what, &account, &lie, &len, 1, [0; 32], &l), "{what}");
    }
}

#[test]
fn the_end_of_an_entry_holds_it_to_the_shards_committee_and_to_side_as_maintenance() {
    let w = world();
    let shard = w.shard();
    let account = w.covered(shard.s.sid);
    let seed = shard_seed(&w.anchors[0]);
    let x = &account.s;
    let held = account.value as i64;
    let l = w.fut(0, 10, LONG, P10);
    // A leg of a committee of one's own: the model states none; stated by hand, its row is right (it is a leg) and
    // its end is wrong: the shard's id is not of that committee.
    let own = XLeg { committee: [0x77; 32], ..l };
    assert!(x.enter(own, &[P10], T0, held, seed).is_err());
    let (lie, len) = by_hand(x, own, P10, seed, held);
    assert!(!x.enter_end_right(&len, &lie.claimer, &lie.outs, held));
    assert!(!end_answered(&w, "a leg of a committee of one's own", &account, &lie, &len));
    assert!(row_answered(&w, "a leg of a committee of one's own", &account, &lie, &len, 1, [0; 32], &own));
    // What the id is said to be the hash of is not it.
    let (lie, len) = by_hand(x, l, P10, [0x11; 32], held);
    assert!(!end_answered(&w, "another seed", &account, &lie, &len));
    // An account whose side A holds nothing: the leg does not enter (two accounts of one's own keys, long and
    // short with no margin, were owed by the shard whichever way the price moved).
    let bare = OLive { s: Account { a_margin: 0, b_margin: 22 * KAS1, ..x.clone() }, ..account.clone() };
    assert!(bare.s.enter(l, &[P10], T0, held, seed).is_err());
    let (lie, len) = by_hand(&bare.s, l, P10, seed, held);
    assert!(!end_answered(&w, "a side A with no margin", &bare, &lie, &len));
    // With exactly its maintenance it enters: 5.5 % of 100 KAS.
    let floor = rate_up(100 * KAS1, x.t.mmr_bps + x.t.g_bps);
    let just = OLive { s: Account { a_margin: floor, b_margin: 22 * KAS1 - floor, ..x.clone() }, ..account.clone() };
    let (st, en, _) = just.s.enter(l, &[P10], T0, held, seed).unwrap();
    assert!(end_answered(&w, "a side A at its maintenance", &just, &st, &en));
    let under = OLive { s: Account { a_margin: floor - 1, b_margin: 22 * KAS1 - floor + 1, ..x.clone() }, ..account.clone() };
    assert!(under.s.enter(l, &[P10], T0, held, seed).is_err());
    // Outputs that are not the entry's: more margin for A, no note, a note of other units.
    let (st, en, after) = x.enter(l, &[P10], T0, held, seed).unwrap();
    let join = SNote::leg(NOTE_JOIN, shard.s.sid, &after.legs[0], xk(&w.b));
    let out = |to: [u8; 32], note: Option<SNote>| {
        let mut v = vec![(Out { key: false, to, value: held }, false)];
        if let Some(n) = note {
            v.push((Out { key: false, to: n.hash(), value: n.value() }, true));
        }
        Outs::with_notes(&v).unwrap()
    };
    for (what, outs) in [
        ("margin of side B's made side A's", out(Account { a_margin: 22 * KAS1, b_margin: 0, ..after.clone() }.hash(), Some(join))),
        ("no note", out(after.hash(), None)),
        ("a note of fewer units", out(after.hash(), Some(SNote { net: join.net / 2, gross: join.gross / 2, ..join }))),
    ] {
        let lie = Stated { outs, ..st };
        assert!(!end_answered(&w, what, &account, &lie, &en), "{what}");
    }
}

#[test]
fn a_disputed_point_of_an_entry_that_nobody_answers_costs_its_claimers() {
    let w = world();
    let shard = w.shard();
    let account = w.covered(shard.s.sid);
    let seed = shard_seed(&w.anchors[0]);
    let x = &account.s;
    let held = account.value as i64;
    // A leg on a market nobody prices enters at any price as far as a citation goes: nobody can answer its price,
    // right or wrong. The prices of an entry are its claimers' to show: the dispute that nobody answers ends the
    // entry, and the disputer has its bond and half of theirs.
    let unpriced = XLeg { market: [0x4d; 32], ..w.fut(0, 10, LONG, 0) };
    let (st, en, _) = x.enter(unpriced, &[P10 / 100], T0, held, seed).unwrap();
    let live = stated_entry(&w, &account, &st, &en);
    let q = w.pointed(&live, 2);
    let f = [fund(xk(&w.w), 35)];
    let b = point_lapse(&w.lib, &q, &w.script("KpfTPoint"), w.payer(&w.w, &f)).unwrap();
    let s = signed("a point of an entry lapses", &b, &w.keys);
    let n = s.tx.outputs.len();
    let half = st.bond / 2;
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())), "the account again, without the leg");
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), ((st.bond + half) as u64, &kpf_protocol::tx::p2pk_spk(&xk(&w.w))), "the disputer's bond and half of the claimers'");
    assert_eq!(s.tx.outputs[1].value, w.script("KpfTPoint").value + (kpf_protocol::thin::ORACLE_FEE + st.bond - half) as u64, "the other half in the script's copy");
    assert!(!s.tx.outputs.iter().any(|o| o.script_public_key == kpf_protocol::tx::p2pk_spk(&xk(&w.a)) && o.value == st.bond as u64), "the claimers' bond does not come back");
}

#[test]
fn what_an_account_of_ones_own_keys_took_from_a_shard_it_takes_no_more() {
    let w = world();
    let shard = w.shard();
    let account = w.covered(shard.s.sid);
    let seed = shard_seed(&w.anchors[0]);
    let x = &account.s;
    let held = account.value as i64;
    // Entered at the committee's price and left at it, a leg is owed nothing: what is owed comes from two prices
    // that were answered for.
    let (_, _, after) = x.enter(w.fut(0, 10, LONG, 0), &[P10], T0, held, seed).unwrap();
    let (_, _, out) = after.step(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &[P10], T0, held, xk(&w.a)).unwrap();
    assert_eq!((out.owed, out.after.a_margin, out.after.b_margin), (0, 20 * KAS1, 2 * KAS1));
    // A call struck at 0.1 where the price is 10, entered for nothing: it does not enter unless its holder holds
    // the 99 KAS it is counted at; and then it is paid none of them, takes none out, and leaves with what it had.
    let deep = w.opt(1, 10, CALL_HELD, P10 / 100, 0);
    assert!(x.enter(deep, &[P10], T0, held, seed).is_err());
    let rich = Account { a_margin: 120 * KAS1, ..x.clone() };
    let (_, en, after) = rich.enter(deep, &[P10], T0, held, seed).unwrap();
    assert_eq!((en.rows[0].gain, en.rows[0].ea, after.a_margin, after.b_margin), (0, 99 * KAS1, 120 * KAS1, 2 * KAS1));
    assert_eq!(after.take_max(SIDE_A, &[P10]), 21 * KAS1, "all but what the call was counted at");
    let (_, _, out) = after.step(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &[P10], T0, held, xk(&w.a)).unwrap();
    assert_eq!((out.owed, out.after.a_margin, out.after.b_margin), (0, 120 * KAS1, 2 * KAS1), "exercised where it entered, it paid nothing");
}

/// Review 39, F62. An account's terms and periods are its two keys' to choose when it is opened; a shard's id is of
/// the terms and periods the accounts under it have, and no leg enters an account of others.
#[test]
fn an_account_of_other_terms_than_its_shards_takes_no_leg() {
    let w = world();
    let shard = w.shard();
    let account = w.covered(shard.s.sid);
    let seed = shard_seed(&w.anchors[0]);
    let x = &account.s;
    let held = account.value as i64;
    let l = w.fut(0, 10, LONG, P10 / 2);
    // The account of review 39: a tolerance of 50 % and no maintenance rate. A leg at 5, where the committee says
    // 10, has a price within that tolerance; its end is wrong, since the shard is not of those terms.
    let mut t = x.t;
    t.g_bps = 5_000;
    t.mmr_bps = 0;
    let loose = OLive { s: Account { t, ..x.clone() }, ..account.clone() };
    assert!(loose.s.enter(l, &[P10 / 2], T0, held, seed).is_err());
    let (lie, len) = by_hand(&loose.s, l, P10 / 2, seed, held);
    assert_eq!(loose.s.enter_wrong_point(&len, &lie.claimer, &lie.outs, held, &[P10]), Some(0));
    assert!(!end_answered(&w, "an account of looser terms", &loose, &lie, &len));
    // The periods too: a dispute period or a bond rate of the account's own.
    for (what, other) in [("a dispute period of its own", Account { dispute_daa: 1, ..x.clone() }), ("a bond rate of its own", Account { bond_bps: 500, ..x.clone() })] {
        let other = OLive { s: other, ..account.clone() };
        let leg = XLeg { entry: P10, ..l };
        assert!(other.s.enter(leg, &[P10], T0, held, seed).is_err(), "{what}");
        let (lie, len) = by_hand(&other.s, leg, P10, seed, held);
        assert!(!end_answered(&w, what, &other, &lie, &len), "{what}");
    }
    // And a leg that entered an account of the shard's terms does not move into one of others.
    let (_, _, after) = x.enter(l, &[P10], T0, held, seed).unwrap();
    assert!(after.leg_moved(1, &Account { a_key: xk(&w.w), ..x.clone() }, [0; 4]).is_ok());
    assert!(after.leg_moved(1, &Account { t, a_key: xk(&w.w), ..x.clone() }, [0; 4]).is_err());
    assert!(after.leg_moved(1, &Account { dispute_daa: 1, a_key: xk(&w.w), ..x.clone() }, [0; 4]).is_err());
}

/// Review 40, F64. The terms and periods an entry is held to are checked in an answer, which nobody gives where
/// the account's own dispute period is a block. An account is opened under a shard only with the shard's.
#[test]
fn a_covered_account_is_opened_only_with_its_shards_terms_and_periods() {
    use kpf_protocol::oracle_build::OPlan;
    use kpf_protocol::tx::Arg;
    use silverscript_abi::ArtifactValue;
    let w = world();
    let shard = w.shard();
    let (seed, committee) = (shard_seed(&w.anchors[0]), w.markets[0].committee_id);
    let base = Account { t: w.terms(), legs: vec![], a_key: xk(&w.a), b_key: xk(&w.b), a_margin: 20 * KAS1, b_margin: 2 * KAS1, expiry: 9_000_000, dispute_daa: 50, bond_bps: 100, a_min_counter: 0, b_min_counter: 0, shard: Some(shard.s.sid) };
    let f = [fund(xk(&w.a), 41)];
    let open = |a: &Account| account_open_c(&w.lib, &w.anchors[1], &w.script("KpfTOpenC"), a, seed, committee, w.payer(&w.a, &f));
    let honest = open(&base).unwrap();
    assert!(accepted(&honest, &w.keys));
    // Its lease is its own. Where a position has its asks a covered account counts its legs and keeps the time
    // before which its user states nothing again: none and none at its opening (review 49, R6).
    assert!(open(&Account { expiry: 77, ..base.clone() }).is_ok_and(|b| accepted(&b, &w.keys)));
    for (count, time) in [(5 * KAS1, 0), (0, KAS1), (-58, 0), (6, 0)] {
        assert!(!open(&Account { a_min_counter: count, b_min_counter: time, ..base.clone() }).is_ok_and(|b| accepted(&b, &w.keys)), "opened with {count} legs counted and the time {time}");
    }
    let mut loose = w.terms();
    loose.g_bps = 5_000;
    let mut bare = w.terms();
    bare.mmr_bps = 0;
    for (what, other) in [
        ("a dispute period of a block", Account { dispute_daa: 1, ..base.clone() }),
        ("a bond rate of its own", Account { bond_bps: 1, ..base.clone() }),
        ("a tolerance of its own", Account { t: loose, ..base.clone() }),
        ("no maintenance rate", Account { t: bare, ..base.clone() }),
    ] {
        assert!(open(&other).is_err(), "{what}: the tool opens it");
        // Built past the tool: the opening's transaction, with the other account's terms, moving part and state.
        let mut t = honest.clone();
        let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!("the script") };
        args[3] = Arg::V(ArtifactValue::Bytes(other.terms()));
        args[4] = Arg::V(ArtifactValue::Bytes(other.moving()));
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &other.hash());
        assert!(!accepted(&t, &w.keys), "{what}: the script opens it");
    }
    // Another seed or another committee than the id is of: refused.
    assert!(account_open_c(&w.lib, &w.anchors[1], &w.script("KpfTOpenC"), &base, [0x11; 32], committee, w.payer(&w.a, &f)).is_err());
    assert!(account_open_c(&w.lib, &w.anchors[1], &w.script("KpfTOpenC"), &base, seed, [0x77; 32], w.payer(&w.a, &f)).is_err());
}

/// Review 40, F65. The prices of a statement are only near the committee's: a leg entered at the one end of the
/// tolerance and left at the other gains twice the tolerance of its worth from prices alone, each of them answered
/// right. The shard is not asked that: it is asked what is owed at any price within the tolerance.
#[test]
fn what_prices_within_the_tolerance_make_is_not_asked_of_the_shard() {
    let w = world();
    let shard = w.shard();
    let account = w.covered(shard.s.sid);
    let seed = shard_seed(&w.anchors[0]);
    // Review 40's round: 140 units enter at 9.95 and leave at 10.05 where the committee says 10. Side A holds 80.
    let x = Account { a_margin: 80 * KAS1, ..account.s.clone() };
    let live = OLive { s: x.clone(), value: (82 * KAS1) as u64, ..account.clone() };
    let held = live.value as i64;
    let (st, en, after) = x.enter(w.fut(0, 140, LONG, 0), &[P10 * 995 / 1000], T0, held, seed).unwrap();
    assert_eq!(x.enter_wrong_point(&en, &st.claimer, &st.outs, held, &[P10]), None, "an entry within the tolerance");
    let entered = OLive { s: after.clone(), ..live.clone() };
    let (st, tk, out) = after.step(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &[P10 * 1005 / 1000], T0, held, xk(&w.a)).unwrap();
    assert_eq!(after.wrong_point_of(KIND_ACCOUNT_LEAVE, &tk, &st.claimer, &st.outs, held, &[P10]), None, "a leaving within the tolerance");
    // It gained 14 KAS of prices alone; the pot pays the 2 it has; of the 12 it could not, nothing is asked of the
    // shard: twice the tolerance of the leg's worth (1,407 KAS, in both sides' worth) is 28.14 KAS.
    assert_eq!((out.owed, out.after.a_margin, out.after.b_margin), (0, 82 * KAS1, 0));
    assert!(st.outs.list().len() == 2, "the account and the note that the leg left: no note of what is owed");
    assert!(w.end_answered("a leg that left at the other end of the tolerance", &entered, &st, &tk));
    // A gain beyond the tolerance is asked, less that band: at 11 the leg gained 147, the pot pays 2, and of the
    // 145 it could not the shard is asked 114.2 (twice the tolerance of 1,540 KAS in both sides' worth: 30.8).
    let (st, tk, out) = after.step(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &[P10 * 11 / 10], T0, held, xk(&w.a)).unwrap();
    assert_eq!(out.owed, 1_142 * KAS1 / 10);
    assert!(w.end_answered("a leg that left beyond the tolerance", &entered, &st, &tk));
    // A statement that asks the shard for all the pot could not pay has a wrong end.
    let whole = SNote::owed(shard.s.sid, xk(&w.a), xk(&w.b), 145 * KAS1);
    let mut list: Vec<(Out, bool)> = st.outs.list().iter().enumerate().map(|(i, o)| (*o, st.outs.is_note(i))).collect();
    let last = list.len() - 1;
    list[last] = (Out { key: false, to: whole.hash(), value: whole.value() }, true);
    let lie = Stated { outs: Outs::with_notes(&list).unwrap(), ..st };
    assert!(!w.end_answered("the shard asked for what prices alone could make", &entered, &lie, &tk));
}

