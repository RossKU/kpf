//! Options as legs of an account (docs/ARCHITECTURE-CLEARING.md 4b, 6a, 10). An option is a leg whose direction's
//! place says its kind and its strike, and whose entry is the price it was last counted at: its row adds to A's gain
//! what it is worth if exercised at the price stated less what it was worth at its entry. So a take moves an
//! option's gain as it moves a futures leg's. Its underlying's worth is asked the maintenance of whoever wrote it
//! alone; its holder is asked none, and keeps whole what the option was counted at. And it leaves by its holder's
//! side alone, and a shard counts it in a row of its own. Every transaction is built, signed and run through
//! `verify::verify`.

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
        let (b, shard) = shard_open(&self.lib, &self.anchors[0], &self.script("KpfTShardOpen"), self.terms(), xk(&self.b), 50 * KAS1, 9_000_000, 50, 100, self.payer(&self.b, &f)).unwrap();
        let s = signed("shard opened", &b, &self.keys);
        nth(&s, 0, shard, 6_100)
    }
    /// A covered account of the user against the clearing party, with 20 and 2 KAS of margin and no leg.
    fn covered(&self, sid: [u8; 32]) -> OLive<Account> {
        let a = Account { t: self.terms(), legs: vec![], a_key: xk(&self.a), b_key: xk(&self.b), a_margin: 20 * KAS1, b_margin: 2 * KAS1, expiry: 9_000_000, dispute_daa: 50, bond_bps: 100, a_min_counter: 0, b_min_counter: 0, shard: Some(sid) };
        let f = [fund(xk(&self.a), 41)];
        let b = account_open_c(&self.lib, &self.anchors[1], &self.script("KpfTOpenC"), &a, self.payer(&self.a, &f)).unwrap();
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

const KINDS: [i64; 4] = [CALL_HELD, CALL_WRITTEN, PUT_HELD, PUT_WRITTEN];

/// An account of one to five legs, futures and options of every kind, and a price a leg.
fn random_account(w: &World, r: &mut Rng, margins: (i64, i64)) -> (Account, Vec<i64>) {
    let n = r.range(1, 5) as usize;
    let scale = w.markets[0].p.scale;
    let tenth = |r: &mut Rng| P10 / 10 * r.range(5, 15);
    let mut legs = vec![];
    let mut p = vec![];
    for i in 0..n {
        let entry = tenth(r);
        let mut l = match r.range(0, 5) {
            0 => w.fut(i, 1, LONG, entry),
            1 => w.fut(i, 1, SHORT, entry),
            k => w.opt(i, 1, KINDS[k as usize - 2], tenth(r), entry),
        };
        l.size = r.range(1, 50) * scale + r.range(0, scale - 1);
        legs.push(l);
        p.push(tenth(r) + r.range(0, 999));
    }
    let x = Account { t: w.terms(), legs, a_key: xk(&w.a), b_key: xk(&w.b), a_margin: r.range(margins.0, margins.1) * KAS1, b_margin: r.range(margins.0, margins.1) * KAS1, expiry: 9_000_000, dispute_daa: 50, bond_bps: 100, a_min_counter: 0, b_min_counter: 0, shard: None };
    (x, p)
}

/// The numbers alone. An option counts from its entry as a futures leg does, with a floor at its strike; what was
/// counted is not counted again; a loss on one leg is paid by a gain on another in the same sum.
#[test]
fn an_option_s_row_adds_what_it_is_worth_exercised_since_it_was_last_counted() {
    let w = world();
    // (A's gain; then, in KAS: the worth A and B are asked the maintenance of, and what A and B keep whole.)
    let gain = |kind: i64, entry: i64, at: i64| -> (i64, [i64; 4]) {
        let x = w.account(vec![w.opt(0, 10, kind, P10, entry)], 100 * KAS1, 100 * KAS1).s;
        let row = *x.trace(&[at]).last().unwrap();
        (row.gain, [row.wa / KAS1, row.wb / KAS1, row.ea / KAS1, row.eb / KAS1])
    };
    let p = |tenths: i64| P10 / 10 * tenths;
    // Ten units struck at 10, last counted at 10.
    assert_eq!(gain(CALL_HELD, p(10), p(13)), (30 * KAS1, [0, 130, 30, 0]), "a call A holds: above its strike it is worth the rise; B, who wrote it, is asked its underlying's maintenance; A keeps the 30 whole");
    assert_eq!(gain(CALL_HELD, p(10), p(9)), (0, [0, 90, 0, 0]), "and nothing below it");
    assert_eq!(gain(CALL_WRITTEN, p(10), p(13)), (-30 * KAS1, [130, 0, 0, 30]), "a call A wrote: the other way");
    assert_eq!(gain(PUT_HELD, p(10), p(7)), (30 * KAS1, [0, 70, 30, 0]));
    assert_eq!(gain(PUT_HELD, p(10), p(13)), (0, [0, 130, 0, 0]));
    assert_eq!(gain(PUT_WRITTEN, p(10), p(7)), (-30 * KAS1, [70, 0, 0, 30]));
    // Last counted at 13 (30 were moved to its holder then): what was counted is not counted again, and comes back
    // where the price falls.
    assert_eq!(gain(CALL_HELD, p(13), p(13)).0, 0);
    assert_eq!(gain(CALL_HELD, p(13), p(15)).0, 20 * KAS1);
    assert_eq!(gain(CALL_HELD, p(13), p(11)).0, -20 * KAS1);
    assert_eq!(gain(CALL_HELD, p(13), p(9)).0, -30 * KAS1, "no more than was counted");
    for (l, holder) in [(w.opt(0, 1, CALL_HELD, P10, P10), SIDE_A), (w.opt(0, 1, CALL_WRITTEN, P10, P10), SIDE_B), (w.opt(0, 1, PUT_HELD, P10, P10), SIDE_A), (w.opt(0, 1, PUT_WRITTEN, P10, P10), SIDE_B)] {
        assert_eq!((holder_of(&l), option_of(&l).map(|o| o.1)), (Some(holder), Some(P10)));
    }
    assert_eq!((option_of(&w.fut(0, 1, LONG, P10)), option_of(&w.fut(0, 1, SHORT, P10))), (None, None));

    // A's futures leg lost 20 and its call gained 30, with 5 of margin: one sum, 10 to A. (Counted apart, the 15 of
    // the loss beyond the margin would be forgiven while the option's 30 stood against the other party.)
    let x = w.account(vec![w.fut(0, 10, LONG, P10), w.opt(1, 10, CALL_HELD, P10, P10)], 5 * KAS1, 100 * KAS1).s;
    let rows = x.trace(&[p(8), p(13)]);
    assert_eq!(rows.iter().map(|r| r.gain).collect::<Vec<_>>(), vec![-20 * KAS1, 10 * KAS1]);
    let after = x.taken(SIDE_A, 0, &rows).unwrap();
    assert_eq!((after.a_margin, after.b_margin), (15 * KAS1, 90 * KAS1));

    // On random accounts of futures and options: a take moves value between the margins and out and no more; after
    // the most is taken, nothing is at the same prices; and the entries stand at the prices.
    let mut r = Rng(0x1234_5678_9abc_def1);
    let mut taken = 0;
    for case in 0..3_000 {
        let (x, p) = random_account(&w, &mut r, (0, 400));
        for side in [SIDE_A, SIDE_B] {
            let max = x.take_max(side, &p);
            let Some(next) = x.taken(side, max, &x.trace(&p)) else { continue };
            taken += 1;
            assert_eq!(next.pot() + max, x.pot(), "case {case}");
            assert_eq!(next.trace(&p).last().unwrap().gain, 0, "case {case}: every leg stands at its price");
            assert_eq!(next.take_max(side, &p), 0, "case {case}: more is taken at the same prices");
            assert!(next.legs.iter().zip(&x.legs).all(|(n, o)| n.dir == o.dir && n.size == o.size), "case {case}: an option's kind and strike stay");
        }
    }
    assert!(taken > 1_000, "{taken}");

    // A side that stands at its maintenance and keeps what the options it holds were counted at is not below zero
    // after every price moved by less than the rate, each its own way: an option moves by no more than its
    // underlying against whoever wrote it, and its holder loses by it no more than it was counted at.
    let (mut stood_a, mut stood_b) = (0, 0);
    for case in 0..4_000 {
        let (x, p) = random_account(&w, &mut r, (0, 60));
        let row = *x.trace(&p).last().unwrap();
        let mmr = x.t.mmr_bps;
        let q: Vec<i64> = p.iter().map(|p| p + p / BPS * r.range(-mmr * 9 / 10, mmr * 9 / 10)).collect();
        let after = *x.trace(&q).last().unwrap();
        let (floor, floor_b) = (rate_up(row.wa, mmr) + row.ea, rate_up(row.wb, mmr) + row.eb);
        if x.a_margin + row.gain >= floor {
            stood_a += 1;
            assert!(x.a_margin + after.gain >= 0, "case {case}: side A stood at its maintenance and is below zero after a move within the rate");
        }
        if x.b_margin - row.gain >= floor_b {
            stood_b += 1;
            assert!(x.b_margin - after.gain >= 0, "case {case}: side B stood at its maintenance and is below zero after a move within the rate");
        }
    }
    assert!(stood_a > 300 && stood_b > 300, "{stood_a} {stood_b}");
}

/// A's futures leg, a call A holds and a put A wrote, ten units each, from 10.
fn three_legs(w: &World) -> Vec<XLeg> {
    vec![w.fut(0, 10, LONG, P10), w.opt(1, 10, CALL_HELD, P10, P10), w.opt(2, 10, PUT_WRITTEN, P10, P10)]
}

#[test]
fn a_take_moves_an_option_s_gain_once_and_its_row_is_answered_with_its_leg_alone() {
    let w = world();
    let account = w.account(three_legs(&w), 100 * KAS1, 100 * KAS1);
    let x = &account.s;
    let held = account.value as i64;
    // At 10.5, 13 and 9: the futures leg gained 5, the call 30, and the put A wrote lost it 10.
    let a = [P10 * 105 / 100, P10 * 13 / 10, P10 * 9 / 10];
    let rows = x.trace(&a);
    assert_eq!(rows.iter().map(|r| r.gain).collect::<Vec<_>>(), vec![5 * KAS1, 35 * KAS1, 25 * KAS1]);
    // A is asked the maintenance of its futures leg and of the put it wrote (105 and 90), and keeps the 30 its call
    // was counted at; B that of the futures leg and of the call it wrote, and keeps the 10 of its put.
    assert_eq!((rows[2].wa, rows[2].wb, rows[2].ea, rows[2].eb), (195 * KAS1, 235 * KAS1, 30 * KAS1, 10 * KAS1));
    let max = x.take_max(SIDE_A, &a);
    assert_eq!(max, 125 * KAS1 - rate_up(195 * KAS1, x.t.mmr_bps + x.t.g_bps) - 30 * KAS1);
    let f = [fund(xk(&w.a), 30)];
    let (b, st, tk) = account_take(&w.lib, &account, &w.script("KpfTChange"), SIDE_A, max, &a, T0, w.payer(&w.a, &f)).unwrap();
    signed("take stated on an account with options", &b, &w.keys);
    assert_eq!(x.wrong_point(&tk, &st.claimer, &st.outs, held, &a), None, "the statement is the rule's");
    assert!(w.end_answered("the most A takes", &account, &st, &tk));
    let (more, tk_more) = x.take(SIDE_A, max + 1, &a, T0, held).unwrap();
    assert!(!w.end_answered("a sompi of what A's call was counted at", &account, &more, &tk_more));
    let r = w.settled("take on an account with options", &account, &st, &tk);
    let after = x.taken(SIDE_A, max, &tk.rows).unwrap();
    assert_eq!((r.tx.outputs[0].value, &r.tx.outputs[0].script_public_key), (account.value - max as u64, &hash_spk(&w.lib, &after.hash())));
    assert_eq!(after.legs.iter().map(|l| (l.entry, l.dir)).collect::<Vec<_>>(), a.iter().zip(&x.legs).map(|(p, l)| (*p, l.dir)).collect::<Vec<_>>(), "every leg is counted at its price; an option's kind and strike stay");
    assert_eq!((after.trace(&a).last().unwrap().gain, after.take_max(SIDE_A, &a)), (0, 0), "nothing is taken twice");
    // The call falls back below its strike: the 30 that were counted come back.
    assert_eq!(after.trace(&[a[0], P10 * 9 / 10, a[2]]).last().unwrap().gain, -30 * KAS1);

    // The rows of the two options are answered right with their legs.
    for i in [2usize, 3] {
        assert!(w.row_answered("an option's row", &account, &st, &tk, i, &x.legs[i - 1]));
    }
    // A trace that counts the call as a futures leg would be (at 9 it lost 10: there is no floor at the strike),
    // and one that leaves out what the put A wrote lost it: the row is the point that is wrong.
    let low = [a[0], P10 * 9 / 10, a[2]];
    let (st, tk) = x.take(SIDE_A, KAS1, &low, T0, held).unwrap();
    assert_eq!(tk.rows.iter().map(|r| r.gain).collect::<Vec<_>>(), vec![5 * KAS1, 5 * KAS1, -5 * KAS1]);
    for (i, delta) in [(2usize, -10 * KAS1), (3, 10 * KAS1)] {
        let mut rows = tk.rows.clone();
        for r in rows[i - 1..].iter_mut() {
            r.gain += delta;
        }
        let lie_tk = ATake { rows, ..tk.clone() };
        let after = x.taken(SIDE_A, KAS1, &lie_tk.rows).unwrap();
        let lie = Stated { arg: lie_tk.arg(), outs: Outs::of(&[Out { key: false, to: after.hash(), value: held - KAS1 }, Out { key: true, to: xk(&w.a), value: KAS1 }]).unwrap(), ..st };
        assert!(x.end_right(&lie_tk, &lie.claimer, &lie.outs, held), "the end follows from the last row as it stands");
        assert_eq!(x.wrong_point(&lie_tk, &lie.claimer, &lie.outs, held, &low), Some(2 * i as i64 - 1));
        assert!(!w.row_answered("an option's row miscounted", &account, &lie, &lie_tk, i, &x.legs[i - 1]));
    }
}

#[test]
fn an_option_leaves_by_its_holder_s_side_alone() {
    let w = world();
    let account = w.account(three_legs(&w), 100 * KAS1, 100 * KAS1);
    let x = &account.s;
    let held = account.value as i64;
    let a = [P10 * 105 / 100, P10 * 13 / 10, P10 * 9 / 10];
    // A holds the call: it leaves by A's statement, for what it is worth exercised (the 30 are in A's margin with
    // the other legs' gain, and the leg is no more).
    let f = [fund(xk(&w.a), 30)];
    let (b, st, tk, stepped) = account_leave(&w.lib, &account, &w.script("KpfTChange"), SIDE_A, 2, &a, T0, w.payer(&w.a, &f)).unwrap();
    signed("an option leaves by its holder: stated", &b, &w.keys);
    assert_eq!((stepped.fee, stepped.owed, stepped.after.a_margin, stepped.after.b_margin), (0, 0, 125 * KAS1, 75 * KAS1));
    assert_eq!(stepped.after.legs.iter().map(|l| (l.dir, l.entry)).collect::<Vec<_>>(), vec![(LONG, a[0]), (option_dir(PUT_WRITTEN, P10), a[2])]);
    assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LEAVE, &tk, &st.claimer, &st.outs, held, &a), None);
    let r = w.settled("an option leaves by its holder", &account, &st, &tk);
    assert_eq!((r.tx.outputs[0].value, &r.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &stepped.after.hash())));
    assert!(w.row_answered("the row of the option that leaves", &account, &st, &tk, 2, &x.legs[1]));

    // B wrote it: B does not end it (far from its strike it is worth nothing exercised, and something to its
    // holder). The tool refuses; stated all the same, its end follows from its rows and the row of the leg is wrong.
    let fb = [fund(xk(&w.b), 30)];
    assert!(account_leave(&w.lib, &account, &w.script("KpfTChange"), SIDE_B, 2, &a, T0, w.payer(&w.b, &fb)).is_err());
    let (lie, lie_tk, _) = x.step_any(KIND_ACCOUNT_LEAVE, SIDE_B, 2, &a, T0, held, xk(&w.b)).unwrap();
    assert!(x.step_end_right(KIND_ACCOUNT_LEAVE, &lie_tk, &lie.claimer, &lie.outs, held));
    assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LEAVE, &lie_tk, &lie.claimer, &lie.outs, held, &a), Some(3));
    assert!(!w.row_answered("an option ended by who wrote it", &account, &lie, &lie_tk, 2, &x.legs[1]));
    // B holds the put A wrote: it leaves by B's statement, and not by A's.
    let (st, tk, _) = x.step(KIND_ACCOUNT_LEAVE, SIDE_B, 3, &a, T0, held, xk(&w.b)).unwrap();
    assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LEAVE, &tk, &st.claimer, &st.outs, held, &a), None);
    assert!(w.row_answered("a put that leaves by its holder", &account, &st, &tk, 3, &x.legs[2]));
    assert!(x.step(KIND_ACCOUNT_LEAVE, SIDE_A, 3, &a, T0, held, xk(&w.a)).is_err());
    // A futures leg leaves by either party, as before.
    assert!(x.step(KIND_ACCOUNT_LEAVE, SIDE_B, 1, &a, T0, held, xk(&w.b)).is_ok());

    // A step of a liquidation takes a futures leg or an option the side holds, and not one it wrote. A has 12 of
    // margin and lost 10 on its futures leg: it stands below its maintenance.
    let thin = w.account(three_legs(&w), 12 * KAS1, 100 * KAS1);
    let y = &thin.s;
    let held = thin.value as i64;
    let low = [P10 * 9 / 10, P10, P10];
    let fw = [fund(xk(&w.w), 30)];
    assert!(account_liq(&w.lib, &thin, &w.script("KpfTChange"), SIDE_A, 3, &low, T0, xk(&w.w), w.payer(&w.w, &fw)).is_err());
    let (lie, lie_tk, _) = y.step_any(KIND_ACCOUNT_LIQ, SIDE_A, 3, &low, T0, held, xk(&w.w)).unwrap();
    assert_eq!(y.wrong_point_of(KIND_ACCOUNT_LIQ, &lie_tk, &lie.claimer, &lie.outs, held, &low), Some(5));
    assert!(!w.row_answered("a liquidation that ends an option the side wrote", &thin, &lie, &lie_tk, 3, &y.legs[2]));
    let (b, st, tk) = account_liq(&w.lib, &thin, &w.script("KpfTChange"), SIDE_A, 2, &low, T0, xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    signed("a liquidation takes an option the side holds: stated", &b, &w.keys);
    assert_eq!(y.wrong_point_of(KIND_ACCOUNT_LIQ, &tk, &st.claimer, &st.outs, held, &low), None);
    assert_eq!(tk.amount, rate_up(100 * KAS1, y.t.liq_fee_bps).min(2 * KAS1), "the fee of the leg's underlying, of the 2 A has");
    assert!(w.row_answered("the row of the option a liquidation takes", &thin, &st, &tk, 2, &y.legs[1]));
    assert!(w.end_answered("a liquidation takes an option the side holds", &thin, &st, &tk));
    w.settled("a liquidation takes an option the side holds", &thin, &st, &tk);

    // What the options a side holds were counted at is not there for its futures legs. A holds a call struck at 8
    // that was counted at 10 (20 of its 25 are that), and its futures leg lost 5: it has 20, all of it the call's,
    // and nothing for the maintenance of the futures leg. The leg is taken by a step.
    let z = w.account(vec![w.fut(0, 10, LONG, P10), w.opt(1, 10, CALL_HELD, P10 * 8 / 10, P10)], 25 * KAS1, 100 * KAS1);
    let held = z.value as i64;
    let p = [P10 * 95 / 100, P10];
    let row = *z.s.trace(&p).last().unwrap();
    assert_eq!((row.gain, row.wa, row.ea), (-5 * KAS1, 95 * KAS1, 20 * KAS1));
    let (st, tk, stepped) = z.s.step(KIND_ACCOUNT_LIQ, SIDE_A, 1, &p, T0, held, xk(&w.w)).unwrap();
    assert_eq!((stepped.fee, stepped.after.legs.len()), (47_500_000, 1));
    assert!(w.end_answered("a liquidation where what the call was counted at decides", &z, &st, &tk));
    // The fee came out of what the call was counted at: A has 19.525 against the 20 counted, which it could lose
    // whole. So the next step exercises the call for it (a holder that paid no fee out of it is below nothing: the
    // next test).
    let (last, fee) = stepped.after.liq_stepped(SIDE_A, 1, &stepped.after.trace(&[P10])).unwrap();
    assert_eq!((last.legs.len(), fee, last.a_margin), (0, 50_000_000, 20 * KAS1 - 47_500_000 - 50_000_000));
}

#[test]
fn the_holder_of_an_option_is_asked_no_maintenance_for_it_and_keeps_what_it_was_counted_at() {
    let w = world();
    // The user paid for a call on 10 units struck at 10 and keeps nothing in the account; the other party wrote it
    // and holds 100.
    let account = w.account(vec![w.opt(0, 10, CALL_HELD, P10, P10)], 0, 100 * KAS1);
    let x = &account.s;
    let held = account.value as i64;
    for tenths in [5, 10, 13, 20] {
        let a = [P10 / 10 * tenths];
        assert!(x.step_any(KIND_ACCOUNT_LIQ, SIDE_A, 1, &a, T0, held, xk(&w.w)).is_err(), "at {tenths}: the holder is below no maintenance");
        assert_eq!(x.take_max(SIDE_A, &a), 0, "at {tenths}: what the option is counted at is not taken out before it is exercised");
    }
    // At 13 the call is counted at 30: B is asked the maintenance of the underlying's 130, and the 30 are A's.
    let a = [P10 * 13 / 10];
    assert_eq!(x.take_max(SIDE_B, &a), 70 * KAS1 - rate_up(130 * KAS1, x.t.mmr_bps + x.t.g_bps));
    let (st, tk) = x.take(SIDE_A, 0, &a, T0, held).unwrap();
    assert!(w.end_answered("a take of nothing that counts the option", &account, &st, &tk));
    let (lie, lie_tk) = x.take(SIDE_A, 1, &a, T0, held).unwrap();
    assert!(!w.end_answered("the holder takes a sompi of what its option was counted at", &account, &lie, &lie_tk));
    let counted = x.taken(SIDE_A, 0, &tk.rows).unwrap();
    assert_eq!((counted.a_margin, counted.b_margin, counted.legs[0].entry), (30 * KAS1, 70 * KAS1, a[0]));
    // The price falls back to 9: the 30 come back, and A has what it had: nothing, and nothing below zero.
    let back = counted.trace(&[P10 * 9 / 10]);
    let fell = counted.taken(SIDE_B, 0, &back).unwrap();
    assert_eq!((back[0].gain, fell.a_margin, fell.b_margin), (-30 * KAS1, 0, 100 * KAS1));
    assert!(fell.step_any(KIND_ACCOUNT_LIQ, SIDE_A, 1, &[P10 * 9 / 10], T0, held, xk(&w.w)).is_err());
    // Nor is a holder below for what the pot cannot pay it (review 34, F57). A keeps 5 and the writer 20; at 14 the
    // call is counted at 40, more than the pot's 25: nobody liquidates A, at this price or beyond, and A exercises
    // for all the pot holds.
    let short = w.account(vec![w.opt(0, 10, CALL_HELD, P10, P10)], 5 * KAS1, 20 * KAS1);
    for tenths in [11, 14, 30, 100] {
        assert!(short.s.step_any(KIND_ACCOUNT_LIQ, SIDE_A, 1, &[P10 / 10 * tenths], T0, short.value as i64, xk(&w.w)).is_err(), "at {tenths}: the holder is below nothing");
    }
    let (st, tk, out) = short.s.step(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &[P10 * 14 / 10], T0, short.value as i64, xk(&w.a)).unwrap();
    assert_eq!((out.after.a_margin, out.after.b_margin, out.owed), (25 * KAS1, 0, 20 * KAS1));
    assert!(w.end_answered("the holder exercises for all the pot holds", &short, &st, &tk));
    // A exercises at 13: the leg leaves, and the 30 are A's to take.
    let live = OLive { s: counted.clone(), ..account.clone() };
    let (st, tk, stepped) = counted.step(KIND_ACCOUNT_LEAVE, SIDE_A, 1, &a, T0, held, xk(&w.a)).unwrap();
    assert_eq!((stepped.after.a_margin, stepped.after.b_margin, stepped.after.legs.len()), (30 * KAS1, 70 * KAS1, 0));
    assert!(w.end_answered("the holder exercises", &live, &st, &tk));
}

#[test]
fn an_option_enters_a_covered_account_and_a_shard_counts_it_in_a_row_of_its_own() {
    let w = world();
    let shard = w.shard();
    let account = w.covered(shard.s.sid);
    let m = &w.markets[0];
    let scale = m.p.scale;
    let ohead = Head { market: option_market(&m.id()), committee: m.committee_id, scale, price_max: m.p.price_max };
    // The user buys a call on 10 units: the note names the options of the market, with the size.
    let call = w.opt(0, 10, CALL_HELD, P10, P10);
    let (account, join) = w.added(&account, call);
    assert_eq!(join.s, SNote { sid: shard.s.sid, kind: NOTE_JOIN, head: ohead, net: 10 * scale, gross: 10 * scale, key: xk(&w.b), amount: 0 });
    let shard = w.applied(&shard, &join, "an option's join applied");
    assert_eq!(shard.s.rows[0], ShardRow { head: ohead, net: 10 * scale, gross: 10 * scale });
    // A futures leg of the same market, the user long: the clearing party is short it in the market's own row. It
    // is not set against the option.
    let (account, join) = w.added(&account, w.fut(0, 10, LONG, P10));
    let shard = w.applied(&shard, &join, "a futures leg's join applied");
    assert_eq!((shard.s.rows[0].net, shard.s.rows[1].head.market, shard.s.rows[1].net), (10 * scale, futures_market(&m.id()), -10 * scale));
    // A put the user wrote counts as the call does: options are not set against one another.
    let (account, join) = w.added(&account, w.opt(0, 4, PUT_WRITTEN, P10, P10));
    let shard = w.applied(&shard, &join, "a written put's join applied");
    assert_eq!(shard.s.rows[0], ShardRow { head: ohead, net: 14 * scale, gross: 14 * scale });
    // What the shard is asked at 10: the rates of 140 and 100 KAS, as if no leg were set against another.
    let t = &shard.s.t;
    let need = rate_up(240 * KAS1, t.mmr_bps) + rate_up(240 * KAS1, t.liq_fee_bps);
    assert_eq!(shard.s.requirement(&[P10, P10, 0, 0]), Some(need + rate_up(need, t.g_bps)));

    // What is no leg does not enter: an option with no strike, one struck beyond its market's limit, a kind that is
    // none.
    let f = [fund(xk(&w.a), 42)];
    for (what, dir) in [("an option with no strike", CALL_HELD), ("a strike beyond the market's limit", option_dir(CALL_HELD, m.p.price_max + 1)), ("a kind above the four", option_dir(7, P10)), ("a kind below the four", option_dir(1, P10)), ("no kind", 0)] {
        let leg = XLeg { dir, ..call };
        assert!(leg_add(&w.lib, &account, &w.script("KpfTLegAdd"), leg, 0, 0, w.payer(&w.a, &f)).is_err(), "{what}");
        let mut next = account.s.clone();
        next.legs.push(leg);
        let note = SNote::leg(NOTE_JOIN, shard.s.sid, &leg, xk(&w.b));
        let r = leg_add_as(&w.lib, &account, &w.script("KpfTLegAdd"), leg, 0, 0, next, note, w.payer(&w.a, &f));
        assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "{what} enters a covered account");
    }
    // Nor does a leg whose worth at its market's limit is beyond what the scripts compute (review 33, F56: such a
    // leg could be stated on, and answered, by nobody). The largest that is computed enters.
    let most = kpf_protocol::cross::MARGIN_MAX / m.p.price_max - 1;
    for (what, size, enters) in [("a leg a unit beyond what is computed", (most + 1) * scale, false), ("the largest leg that is computed", most * scale + scale - 1, true)] {
        for leg in [XLeg { size, ..w.fut(0, 1, LONG, P10) }, XLeg { size, ..call }] {
            assert_eq!(leg_add(&w.lib, &account, &w.script("KpfTLegAdd"), leg, 0, 0, w.payer(&w.a, &f)).is_ok(), enters, "{what}");
            let mut next = account.s.clone();
            next.legs.push(leg);
            let note = SNote::leg(NOTE_JOIN, shard.s.sid, &leg, xk(&w.b));
            let r = leg_add_as(&w.lib, &account, &w.script("KpfTLegAdd"), leg, 0, 0, next, note, w.payer(&w.a, &f));
            assert_eq!(r.is_ok_and(|b| accepted(&b.0, &w.keys)), enters, "{what}");
        }
    }
    // A futures leg of a market that is called as the options' row is (review 34, F59) is not counted there: a
    // row's name is the hash of a kind and a market, and its kind is futures.
    let named = XLeg { market: option_market(&m.id()), ..w.fut(0, 10, LONG, P10) };
    let (b, next, note) = leg_add(&w.lib, &account, &w.script("KpfTLegAdd"), named, 0, 0, w.payer(&w.a, &f)).unwrap();
    assert!(accepted(&b, &w.keys));
    assert!(note.head.market != ohead.market && note.head.market == futures_market(&option_market(&m.id())));
    let forged = SNote { head: ohead, ..note };
    let r = leg_add_as(&w.lib, &account, &w.script("KpfTLegAdd"), named, 0, 0, next, forged, w.payer(&w.a, &f));
    assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "a futures leg is noted in the options' row");
    // And an option is not noted under its market's futures row (where it would be set against futures legs).
    let leg = w.opt(0, 3, CALL_WRITTEN, P10, P10);
    let (b, next, note) = leg_add(&w.lib, &account, &w.script("KpfTLegAdd"), leg, 0, 0, w.payer(&w.a, &f)).unwrap();
    assert!(accepted(&b, &w.keys));
    let plain = SNote { head: Head { market: futures_market(&m.id()), ..ohead }, net: -3 * scale, ..note };
    let r = leg_add_as(&w.lib, &account, &w.script("KpfTLegAdd"), leg, 0, 0, next, plain, w.payer(&w.a, &f));
    assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "an option's note names its market's futures row");
}

#[test]
fn an_option_leaves_a_covered_account_with_the_note_of_its_row_and_what_is_owed() {
    let w = world();
    let shard = w.shard();
    let account = w.covered(shard.s.sid);
    let m = &w.markets[0];
    let scale = m.p.scale;
    let ohead = Head { market: option_market(&m.id()), committee: m.committee_id, scale, price_max: m.p.price_max };
    let (account, join) = w.added(&account, w.opt(0, 10, CALL_HELD, P10, P10));
    let shard = w.applied(&shard, &join, "an option's join applied");
    let x = &account.s;
    let held = account.value as i64;
    // At 13 the call is worth 30 exercised; the clearing party has 2 in the account. The user exercises it.
    let a = [P10 * 13 / 10];
    let f = [fund(xk(&w.a), 30)];
    let (b, st, tk, stepped) = account_leave(&w.lib, &account, &w.script("KpfTChange"), SIDE_A, 1, &a, T0, w.payer(&w.a, &f)).unwrap();
    signed("an option leaves a covered account: stated", &b, &w.keys);
    assert_eq!((stepped.owed, stepped.after.a_margin, stepped.after.b_margin, stepped.after.legs.len()), (28 * KAS1, 22 * KAS1, 0, 0));
    assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LEAVE, &tk, &st.claimer, &st.outs, held, &a), None);
    let release = SNote { sid: shard.s.sid, kind: NOTE_RELEASE, head: ohead, net: 10 * scale, gross: 10 * scale, key: xk(&w.b), amount: 0 };
    let owed = SNote::owed(shard.s.sid, xk(&w.a), xk(&w.b), 28 * KAS1);
    assert_eq!(st.outs.list().iter().map(|o| o.to).collect::<Vec<_>>(), vec![stepped.after.hash(), release.hash(), owed.hash()]);
    let s = w.settled("an option leaves a covered account", &account, &st, &tk);
    let n = s.tx.outputs.len();
    assert_eq!(s.tx.outputs[n - 1].script_public_key, hash_spk(&w.lib, &release.hash()));
    // The release empties the options' row; what is owed is paid by the shard.
    let shard = w.applied(&shard, &nth(&s, n - 1, release, 6_570), "an option's release applied");
    assert_eq!(shard.s.rows[0], ShardRow::EMPTY);
    let shard = w.applied(&shard, &nth(&s, n - 2, owed, 6_570), "what the option was worth beyond the pot, paid");
    assert_eq!(shard.s.held, 22 * KAS1);
    // The clearing party does not exercise the user's call for it (the tool refuses: the leg is not its to end).
    assert!(x.step(KIND_ACCOUNT_LEAVE, SIDE_B, 1, &a, T0, held, xk(&w.b)).is_err());
    // And a statement that names the market's futures row for the option that leaves is wrong at that row.
    let plain = ATake { leave: Some((Head { market: futures_market(&m.id()), ..ohead }, 10 * scale)), ..tk.clone() };
    let rel = SNote { head: Head { market: futures_market(&m.id()), ..ohead }, ..release };
    let outs = Outs::with_notes(&[(Out { key: false, to: stepped.after.hash(), value: held }, false), (Out { key: false, to: rel.hash(), value: rel.value() }, true), (Out { key: false, to: owed.hash(), value: owed.value() }, true)]).unwrap();
    let lie = Stated { arg: plain.arg(), outs, ..st };
    assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LEAVE, &plain, &lie.claimer, &lie.outs, held, &a), Some(1));
    assert!(!w.row_answered("an option's leaving noted under its market's futures row", &account, &lie, &plain, 1, &x.legs[0]));
}

#[test]
fn the_price_of_a_row_of_options_is_answered_with_its_underlying_s() {
    let w = world();
    let m = &w.markets[1];
    let scale = m.p.scale;
    let ohead = Head { market: option_market(&m.id()), committee: m.committee_id, scale, price_max: m.p.price_max };
    // A shard that counts options on 20 units of market 1, and holds 50 KAS.
    let s = Shard { t: w.terms(), rows: [ShardRow { head: ohead, net: 20 * scale, gross: 20 * scale }, ShardRow::EMPTY, ShardRow::EMPTY, ShardRow::EMPTY], owner: xk(&w.b), held: 50 * KAS1, expiry: 9_000_000, dispute_daa: 50, bond_bps: 1_000, sid: [0x5d; 32] };
    let shard = OLive { s, value: (s.held + SHARD_CARRIER) as u64, id: Some(Hash::from_bytes([0x73; 32])), op: op(12), daa: 6_100 };
    let stated = |st: &Stated, said: &[u8]| -> OLive<Stated> {
        let f = [fund(xk(&w.b), 30)];
        let b = state_raw(&w.lib, &shard, &s.fixed_hash(), &s.moving(), st, said, s.form(), &w.script("KpfTChange"), w.payer(&w.b, &f)).unwrap();
        let sg = b.sign(&w.keys).expect("stated");
        assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "the draw is stated");
        nth(&sg, 0, *st, 6_510)
    };
    let honest = [P10, 0, 0, 0];
    let low = [P10 / 2, 0, 0, 0];
    let held = shard.value as i64;
    // The owner states the underlying at 5 (the committee's price is 10): more is drawn.
    let max = s.draw_max(&low);
    assert!(max > s.draw_max(&honest));
    let (st, d) = s.draw(max, low, T0, held).unwrap();
    assert_eq!(s.draw_wrong_point(&d, &st.claimer, &st.outs, held, &honest), Some(1));
    let f = [fund(xk(&w.w), 32)];
    let cite1 = w.script("KpfTCite");
    let q = w.pointed(&stated(&st, &d.said()), 1);
    let c = w.cited(1, P10);
    let (b, right) = shard_price(&w.lib, &q, &s, &d, 1, &w.script("KpfTShardPrice"), Some((&cite1, &c)), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    let sg = signed("the price of a row of options answered wrong, with its underlying's citation", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value, &hash_spk(&w.lib, &s.hash())), "the shard again");
    let forced = shard_price(&w.lib, &q, &s, &d, 1, &w.script("KpfTShardPrice"), Some((&cite1, &c)), xk(&w.w), w.payer(&w.w, &f), Some(true));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys));
    // The citation of another market is not the row's.
    let other = w.cited(0, P10 / 2);
    assert!(shard_price(&w.lib, &q, &s, &d, 1, &w.script("KpfTShardPrice"), Some((&cite1, &other)), xk(&w.w), w.payer(&w.w, &f), None).is_err());
    // A draw at the committee's price is answered right.
    let (st, d) = s.draw(s.draw_max(&honest), honest, T0, held).unwrap();
    let q = w.pointed(&stated(&st, &d.said()), 1);
    let c = w.cited(1, P10 + P10 / 400);
    let (b, right) = shard_price(&w.lib, &q, &s, &d, 1, &w.script("KpfTShardPrice"), Some((&cite1, &c)), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    signed("the price of a row of options answered right", &b, &w.keys);
    let forced = shard_price(&w.lib, &q, &s, &d, 1, &w.script("KpfTShardPrice"), Some((&cite1, &c)), xk(&w.w), w.payer(&w.w, &f), Some(false));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys));
}

/// On random accounts of futures and options: every row of an honest trace is answered right by the script and not
/// wrong, and a row whose gain is a sompi off is answered wrong and not right.
#[test]
fn the_row_s_answer_agrees_with_the_model_on_random_accounts_with_options() {
    let w = world();
    let mut r = Rng(0x0bad_cafe_1234_5678);
    let mut options = 0;
    for case in 0..10 {
        let (x, p) = random_account(&w, &mut r, (3_000, 4_000));
        x.check().unwrap();
        let account = OLive { value: (x.pot() + KAS1) as u64, s: x.clone(), id: Some(Hash::from_bytes([0x72; 32])), op: op(11), daa: 6_100 };
        let held = account.value as i64;
        let (st, tk) = x.take(SIDE_A, 0, &p, T0, held).unwrap();
        let i = r.range(1, x.legs.len() as i64) as usize;
        options += option_of(&x.legs[i - 1]).is_some() as usize;
        assert!(w.row_answered(&format!("case {case}"), &account, &st, &tk, i, &x.legs[i - 1]), "case {case}");
        let mut rows = tk.rows.clone();
        rows[i - 1].gain += if case % 2 == 0 { 1 } else { -1 };
        let lie_tk = ATake { rows, ..tk.clone() };
        let lie = Stated { arg: lie_tk.arg(), ..st };
        assert!(!w.row_answered(&format!("case {case}, a sompi off"), &account, &lie, &lie_tk, i, &x.legs[i - 1]), "case {case}");
    }
    assert!(options >= 3, "{options}");
}
