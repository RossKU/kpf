//! The reserve, second part: the draw (`KpfTShardEnd`, `KpfTShardPrice`, `KpfTDrawNote`; kind 5 of `KpfTChange`;
//! docs/ARCHITECTURE-CLEARING.md 6a). A shard's owner states what it draws and a price a row; the shard keeps its
//! requirement at those prices; a dispute names the end or a row's price; and a note to the shard that is not
//! applied ends the draw. Every transaction is built, signed and run through `verify::verify`.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cite;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::cross::{XLeg, XTerms, SIDE_A};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::reserve::{Draw, Head, SNote, Shard, ShardRow, NOTE_JOIN, NOTE_MIN, SHARD_CARRIER};
use kpf_protocol::state::LONG;
use kpf_protocol::thin::{Account, Out, Outs, Pointed, Stated, KIND_ACCOUNT_TAKE, KIND_DRAW};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{p2pk_spk, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::cell::RefCell;

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
        let s = Shard { t: self.terms(), rows, owner: xk(&self.c), held: 100 * KAS1, expiry: 9_000_000, dispute_daa: 50, bond_bps: 1_000, sid: [0x5d; 32], basis: -500 * KAS1, called: 0 };
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

const PRICES: [i64; 4] = [P10, P10, 0, 0];

#[test]
fn the_owner_draws_what_is_above_the_requirement_and_no_more() {
    let w = world();
    let shard = w.shard();
    let s = &shard.s;
    // At 10: the net is worth 600 and 100 KAS, the gross 1,400 and 300. 5 % of 700 and 0.5 % of 1,700, and the
    // tolerance (0.5 %) of that more: 43.7175 KAS. It holds 50.
    let need = s.requirement(&PRICES).unwrap();
    // (5.5 % of the 700 KAS the net units are worth, and 2.25 % of the 1,700 the gross are: the upper level.)
    assert_eq!(need, 3_850_000_000 + 3_825_000_000);
    assert_eq!((s.equity(&PRICES), s.level(&PRICES, false)), (Some(100 * KAS1), Some(3_150_000_000 + 2_975_000_000)), "its equity is what it holds: its legs stand where they entered; the lower level is 4.5 % and 1.75 %");
    let max = s.draw_max(&PRICES);
    assert_eq!(max, 100 * KAS1 - need);
    let f = [fund(xk(&w.c), 30)];
    let (b, st, d) = shard_draw(&w.lib, &shard, &w.script("KpfTChange"), max, PRICES, T0, w.payer(&w.c, &f)).unwrap();
    let sg = signed("draw stated", &b, &w.keys);
    assert_eq!((st.kind, st.claimer, st.bond), (KIND_DRAW, xk(&w.c), 10 * KAS1));
    assert_eq!(s.draw_wrong_point(&d, &st.claimer, &st.outs, shard.value as i64, &PRICES), None);
    // Applied after the dispute period: the shard holds the amount less, the amount and the bond to the owner.
    let live = nth(&sg, 0, st, 6_510);
    let f = [fund(xk(&w.w), 31)];
    let b = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).unwrap();
    let r = signed("draw applied", &b, &w.keys);
    let n = r.tx.outputs.len();
    assert_eq!((r.tx.outputs[0].value, &r.tx.outputs[0].script_public_key), (shard.value - max as u64, &hash_spk(&w.lib, &s.drawn(max).hash())));
    assert_eq!((r.tx.outputs[n - 1].value, &r.tx.outputs[n - 1].script_public_key), (max as u64, &p2pk_spk(&xk(&w.c))));
    // A sompi more is not stated by the tool; stated all the same, its end is answered wrong.
    assert!(shard_draw(&w.lib, &shard, &w.script("KpfTChange"), max + 1, PRICES, T0, w.payer(&w.c, &f)).is_err());
    let lies: Vec<(&str, Stated, Draw)> = vec![
        ("a sompi more", { let d = Draw { amount: max + 1, prices: PRICES }; Stated { arg: d.arg(), outs: Outs::of(&[Out { key: false, to: s.drawn(max + 1).hash(), value: shard.value as i64 - max - 1 }, Out { key: true, to: xk(&w.c), value: max + 1 }]).unwrap(), ..st } }, Draw { amount: max + 1, prices: PRICES }),
        ("another key than the owner's", Stated { claimer: xk(&w.w), outs: Outs::of(&[Out { key: false, to: s.drawn(max).hash(), value: shard.value as i64 - max }, Out { key: true, to: xk(&w.w), value: max }]).unwrap(), ..st }, d),
        ("rows of its own making in the shard that goes on", Stated { outs: Outs::of(&[Out { key: false, to: Shard { rows: [ShardRow::EMPTY; 4], ..s.drawn(max) }.hash(), value: shard.value as i64 - max }, Out { key: true, to: xk(&w.c), value: max }]).unwrap(), ..st }, d),
        ("a market with no price", { let d = Draw { amount: KAS1, prices: [P10, 0, 0, 0] }; Stated { arg: d.arg(), outs: Outs::of(&[Out { key: false, to: s.drawn(KAS1).hash(), value: shard.value as i64 - KAS1 }, Out { key: true, to: xk(&w.c), value: KAS1 }]).unwrap(), ..st } }, Draw { amount: KAS1, prices: [P10, 0, 0, 0] }),
    ];
    for (what, lie, d) in lies {
        assert!(!s.draw_end_right(&d, &lie.claimer, &lie.outs, shard.value as i64), "{what}");
        let q = w.pointed(&w.stated(&shard, &lie, &d.said()), 0);
        let (b, right) = shard_end(&w.lib, &q, s, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
        assert!(!right, "{what}");
        let sg = signed(&format!("draw's end answered wrong: {what}"), &b, &w.keys);
        assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value, &hash_spk(&w.lib, &s.hash())), "{what}: the shard again");
        let forced = shard_end(&w.lib, &q, s, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), Some(true));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "{what}: answered right");
    }
    // The end of the draw that is the rule's is answered right, and not wrong.
    let q = w.pointed(&live, 0);
    let (b, right) = shard_end(&w.lib, &q, s, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    signed("draw's end answered right", &b, &w.keys);
    let forced = shard_end(&w.lib, &q, s, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), Some(false));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys));
}

/// Section 13, stage 1. A shard knows what its side has gained or lost: its equity is what it holds and what its
/// net units are worth, less what its legs were entered at. A draw leaves that at the upper level, is of coins it
/// holds, and is not made while the shard is called.
#[test]
fn a_draw_is_of_the_equity_above_the_upper_level_of_coins_and_not_while_called() {
    let w = world();
    let shard = w.shard();
    let s = &shard.s;
    let held = shard.value as i64;
    let f = [fund(xk(&w.w), 31)];
    // The end of a draw of `amount` at `prices`, stated by the owner whatever the rule says, answered at the engine.
    let judged = |what: &str, on: &OLive<Shard>, amount: i64, prices: [i64; 4]| -> bool {
        let x = &on.s;
        let d = Draw { amount, prices };
        let outs = Outs::of(&[Out { key: false, to: x.drawn(amount).hash(), value: on.value as i64 - amount }, Out { key: true, to: xk(&w.c), value: amount }]).unwrap();
        let st = Stated { position: x.hash(), window: x.dispute_daa, late: x.t.late_daa, kind: KIND_DRAW, t0: T0, arg: d.arg(), outs, bond: x.bond(), claimer: xk(&w.c), held: on.value as i64 };
        let q = w.pointed(&w.stated(on, &st, &d.said()), 0);
        let (b, right) = shard_end(&w.lib, &q, x, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
        signed(&format!("{what}: the end of a draw answered {}", if right { "right" } else { "wrong" }), &b, &w.keys);
        let forced = shard_end(&w.lib, &q, x, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), Some(!right));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "{what}: answered the other way");
        right
    };
    let _ = held;
    // Market 0, where its side is 60 units short from 10, stands at 11: it has lost 60 KAS, and its equity is 40,
    // below what it is asked. It holds 100 KAS, and draws none of them.
    let up = [P10 * 11 / 10, P10, 0, 0];
    assert_eq!((s.equity(&up), s.draw_max(&up)), (Some(40 * KAS1), 0));
    assert!(!judged("a shard that lost draws a sompi", &shard, 1, up));
    // At 9 it has gained 60: its equity is 160. What it draws is of its equity above the upper level, and of the
    // coins it holds: the gain is in its users' margins until their legs leave.
    let down = [P10 * 9 / 10, P10, 0, 0];
    let need = s.requirement(&down).unwrap();
    assert_eq!((s.equity(&down), s.draw_max(&down)), (Some(160 * KAS1), 160 * KAS1 - need));
    assert!(judged("the most at 9", &shard, 160 * KAS1 - need, down));
    assert!(!judged("a sompi more at 9", &shard, 160 * KAS1 - need + 1, down));
    let far = [P10 / 2, P10, 0, 0];
    assert_eq!((s.equity(&far), s.draw_max(&far)), (Some(400 * KAS1), 100 * KAS1), "of coins: it holds 100");
    assert!(judged("all its coins at 5", &shard, 100 * KAS1, far));
    assert!(!judged("a sompi more than its coins", &shard, 100 * KAS1 + 1, far));
    // A shard that is called draws nothing.
    let called = OLive { s: Shard { called: 6_400, ..*s }, ..shard.clone() };
    assert_eq!(called.s.draw_max(&PRICES), 0);
    assert!(!judged("a called shard draws", &called, KAS1, PRICES));
    assert!(judged("the same shard, not called", &shard, KAS1, PRICES));
    // And a shard is opened with no leg and not called (the reserve's test opens one: thin_reserve.rs).
}

#[test]
fn a_price_that_understates_the_requirement_is_answered_by_the_committees() {
    let w = world();
    let shard = w.shard();
    let s = &shard.s;
    // The owner states market 0 at 8 (the committee's price is 10): the requirement looks smaller and more is drawn.
    let low = [P10 * 8 / 10, P10, 0, 0];
    let max = s.draw_max(&low);
    assert!(max > s.draw_max(&PRICES));
    let (st, d) = s.draw(max, low, T0, shard.value as i64).unwrap();
    assert_eq!(s.draw_wrong_point(&d, &st.claimer, &st.outs, shard.value as i64, &PRICES), Some(1), "the end follows from the prices stated: the price of row 1 is the point");
    let live = w.stated(&shard, &st, &d.said());
    let f = [fund(xk(&w.w), 32)];
    let cite1 = w.script("KpfTCite");
    let q = w.pointed(&live, 1);
    let c = w.cited(0, P10);
    let (b, right) = shard_price(&w.lib, &q, s, &d, 1, &w.script("KpfTShardPrice"), Some((&cite1, &c)), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    let sg = signed("row's price answered wrong", &b, &w.keys);
    assert_eq!(sg.tx.inputs.len(), 4, "the object, the script, one citation and a coin");
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value, &hash_spk(&w.lib, &s.hash())), "the shard again");
    let forced = shard_price(&w.lib, &q, s, &d, 1, &w.script("KpfTShardPrice"), Some((&cite1, &c)), xk(&w.w), w.payer(&w.w, &f), Some(true));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys));
    // The price of row 2 is the committee's: right; a row with no market and a point beyond the fourth are of
    // nothing: right, with no citation.
    let q = w.pointed(&live, 2);
    let c = w.cited(1, P10 + P10 / 400);
    let (b, right) = shard_price(&w.lib, &q, s, &d, 2, &w.script("KpfTShardPrice"), Some((&cite1, &c)), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    signed("row's price answered right", &b, &w.keys);
    for point in [3i64, 4, 5, 77] {
        let q = w.pointed(&live, point);
        let (b, right) = shard_price(&w.lib, &q, s, &d, point, &w.script("KpfTShardPrice"), None, xk(&w.w), w.payer(&w.w, &f), None).unwrap();
        assert!(right);
        signed(&format!("point {point} of nothing answered right"), &b, &w.keys);
        let forced = shard_price(&w.lib, &q, s, &d, point, &w.script("KpfTShardPrice"), None, xk(&w.w), w.payer(&w.w, &f), Some(false));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a point of nothing is answered wrong");
    }
    // Under a dispute of another point, the wrong price ends the draw all the same.
    let q = w.pointed(&live, 4);
    let c = w.cited(0, P10);
    let (b, right) = shard_price(&w.lib, &q, s, &d, 1, &w.script("KpfTShardPrice"), Some((&cite1, &c)), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    signed("row's price answered wrong under a dispute of another point", &b, &w.keys);
}

#[test]
fn a_note_that_is_not_applied_ends_a_draw() {
    let w = world();
    let shard = w.shard();
    let s = &shard.s;
    let (st, d) = s.draw(KAS1, PRICES, T0, shard.value as i64).unwrap();
    let live = w.stated(&shard, &st, &d.said());
    // A leg entered an account of the owner's before the draw was stated, and its note is not applied: the shard's
    // net is not what the draw counted.
    let leg = XLeg { market: w.markets[0].id(), committee: w.markets[0].committee_id, scale: w.markets[0].p.scale, price_max: w.markets[0].p.price_max, size: 500 * w.markets[0].p.scale, dir: LONG, entry: P10 };
    let join = SNote::leg(NOTE_JOIN, s.sid, &leg, xk(&w.c));
    let note = OLive { s: join, value: NOTE_MIN as u64, id: shard.id, op: op(13), daa: 6_400 };
    let f = [fund(xk(&w.w), 33)];
    let b = draw_note(&w.lib, &live, &st, None, s, &note, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &f)).unwrap();
    let sg = signed("draw ended by a note", &b, &w.keys);
    let n = sg.tx.outputs.len();
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value, &hash_spk(&w.lib, &s.hash())), "the shard again");
    assert_eq!((sg.tx.outputs[2].value, &sg.tx.outputs[2].script_public_key), (NOTE_MIN as u64, &hash_spk(&w.lib, &join.hash())), "the note goes on as it is");
    assert_eq!((sg.tx.outputs[n - 1].value, &sg.tx.outputs[n - 1].script_public_key), ((st.bond / 2) as u64, &p2pk_spk(&xk(&w.w))), "half of the owner's bond to who showed it");
    // A draw disputed at a point is ended the same way, and the disputer has its bond back.
    let q = w.pointed(&live, 1);
    let b = draw_note(&w.lib, &q, &st, Some((xk(&w.w), 1)), s, &note, &w.script("KpfTDrawNote"), xk(&w.f), w.payer(&w.w, &f)).unwrap();
    let sg = signed("disputed draw ended by a note", &b, &w.keys);
    let n = sg.tx.outputs.len();
    assert_eq!((sg.tx.outputs[n - 2].value, &sg.tx.outputs[n - 2].script_public_key), (st.bond as u64, &p2pk_spk(&xk(&w.w))));
    // A note that landed at or after the statement does not end it (the builder refuses; and the script, the same
    // transaction with the note's landing moved).
    let young = OLive { daa: 6_510, ..note.clone() };
    assert!(draw_note(&w.lib, &live, &st, None, s, &young, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &f)).is_err());
    let mut t = draw_note(&w.lib, &live, &st, None, s, &note, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &f)).unwrap();
    t.entries[2].block_daa_score = 6_510;
    assert!(!accepted(&t, &w.keys), "a note as young as the statement ends it");
    // A note of an account whose side B is not the owner is applied by no shard, and ends no draw: else anyone would
    // hold every draw of a clearing party with a note of its own making.
    let stranger = OLive { s: SNote::leg(NOTE_JOIN, s.sid, &leg, xk(&w.w)), ..note.clone() };
    assert!(draw_note(&w.lib, &live, &st, None, s, &stranger, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &f)).is_err());
    let mut t = draw_note(&w.lib, &live, &st, None, s, &note, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &f)).unwrap();
    {
        use kpf_protocol::oracle_build::OPlan;
        use kpf_protocol::tx::Arg;
        use silverscript_abi::ArtifactValue;
        // (The note shown as the stranger's: its key in the script's arguments, and the note's own script.)
        let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!("the script") };
        args[20] = Arg::V(ArtifactValue::Bytes(xk(&w.w).to_vec()));
        let stranger_script = kpf_protocol::thin_artifacts::kit().object(&w.lib, &stranger.s.hash());
        t.entries[2].script_public_key = kpf_protocol::tx::p2sh_spk(&stranger_script);
        if let OPlan::Contract { redeem, .. } = &mut t.plans[2] {
            *redeem = stranger_script;
        }
        t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &stranger.s.hash());
    }
    assert!(!accepted(&t, &w.keys), "a stranger's note ends a draw");
    // What a user is owed by an account of the owner's ends it; what an account of strangers owes does not; and a
    // deposit does not (it only adds to what the shard holds, and anyone writes one at any time: it would be a way
    // to take half of an owner's bond by landing one just before a draw).
    let swap = |n: &SNote| {
        use kpf_protocol::oracle_build::OPlan;
        use kpf_protocol::tx::{iv, Arg};
        use silverscript_abi::ArtifactValue;
        let mut t = draw_note(&w.lib, &live, &st, None, s, &note, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &f)).unwrap();
        let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!("the script") };
        let bytes = |b: Vec<u8>| Arg::V(ArtifactValue::Bytes(b));
        (args[16], args[17], args[18], args[19], args[20], args[21]) = (iv(n.kind), bytes(n.head.bytes()), iv(n.net), iv(n.gross), bytes(n.key.to_vec()), iv(n.amount));
        let script = kpf_protocol::thin_artifacts::kit().object(&w.lib, &n.hash());
        t.entries[2].script_public_key = kpf_protocol::tx::p2sh_spk(&script);
        if let OPlan::Contract { redeem, .. } = &mut t.plans[2] {
            *redeem = script;
        }
        t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &n.hash());
        t
    };
    assert!(accepted(&swap(&join), &w.keys), "the control: the swap of a note for itself");
    let owed = SNote::owed(s.sid, xk(&w.w), xk(&w.c), 3 * KAS1);
    let b = draw_note(&w.lib, &live, &st, None, s, &OLive { s: owed, ..note.clone() }, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &f)).unwrap();
    signed("draw ended by a note of what is owed", &b, &w.keys);
    assert!(accepted(&swap(&owed), &w.keys));
    for (what, n) in [("what strangers owe", SNote::owed(s.sid, xk(&w.w), xk(&w.f), 3 * KAS1)), ("a deposit", SNote::deposit(s.sid, xk(&w.c), 3 * KAS1)), ("a note to another shard", SNote { sid: [0x77; 32], ..join })] {
        assert!(draw_note(&w.lib, &live, &st, None, s, &OLive { s: n, ..note.clone() }, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &f)).is_err(), "{what}");
        assert!(!accepted(&swap(&n), &w.keys), "{what} ends a draw");
    }
}

#[test]
fn a_shard_takes_a_draw_and_no_other_statement_and_an_account_takes_no_draw() {
    let w = world();
    let shard = w.shard();
    let s = &shard.s;
    let (st, d) = s.draw(KAS1, PRICES, T0, shard.value as i64).unwrap();
    let f = [fund(xk(&w.c), 30)];
    // A take, a step of a liquidation, a leg's leaving stated on a shard: refused when stated (their end would
    // follow from a row of the claimer's own, and no row of it could be opened by anybody).
    for kind in [KIND_ACCOUNT_TAKE, 3, 4] {
        let said = vec![0u8; 116 + 88];
        let odd = Stated { kind, arg: blake2b(&said), ..st };
        let b = state_raw(&w.lib, &shard, &s.fixed_hash(), &s.moving(), &odd, &said, s.form(), &w.script("KpfTChange"), w.payer(&w.c, &f)).unwrap();
        assert!(!accepted(&b, &w.keys), "kind {kind} is stated on a shard");
    }
    // (The draw is: the control.)
    let b = state_raw(&w.lib, &shard, &s.fixed_hash(), &s.moving(), &st, &d.said(), s.form(), &w.script("KpfTChange"), w.payer(&w.c, &f)).unwrap();
    assert!(accepted(&b, &w.keys));
    // A draw of other bytes than an amount and four prices, or without the shard's form: refused.
    for (what, said, form) in [("39 bytes", d.said()[..39].to_vec(), s.form()), ("no form", d.said(), vec![]), ("another shard's form", d.said(), Shard { held: 1, rows: [ShardRow::EMPTY; 4], ..*s }.form())] {
        let odd = Stated { arg: blake2b(&said), ..st };
        let b = state_raw(&w.lib, &shard, &s.fixed_hash(), &s.moving(), &odd, &said, form, &w.script("KpfTChange"), w.payer(&w.c, &f)).unwrap();
        assert!(!accepted(&b, &w.keys), "a draw is stated with {what}");
    }
    // A draw stated on a covered account (an object of 171 bytes with a second key): refused.
    let account = Account { t: w.terms(), legs: vec![], a_key: xk(&w.c), b_key: xk(&w.w), a_margin: 50 * KAS1, b_margin: 0, expiry: 9_000_000, dispute_daa: 50, bond_bps: 1_000, a_min_counter: 0, b_min_counter: 0, shard: Some(s.sid) };
    let live = OLive { s: (), value: 50 * KAS1 as u64, id: shard.id, op: op(14), daa: 6_100 };
    let on = Stated { position: account.hash(), ..st };
    let b = state_raw(&w.lib, &live, &account.fixed_hash(), &account.moving(), &on, &d.said(), account.form(), &w.script("KpfTChange"), w.payer(&w.c, &f)).unwrap();
    assert!(!accepted(&b, &w.keys), "a draw is stated on an account");
    let _ = SIDE_A;
}


#[test]
fn a_shard_too_large_to_compute_is_drawn_from_by_no_answer() {
    let w = world();
    let mut shard = w.shard();
    // The owner signed for legs of 40 thousand million units in a market: at a price of 10 their worth is beyond 64
    // bits, and the engine's integers are checked.
    let scale = w.markets[0].p.scale;
    shard.s.rows[0] = ShardRow { head: w.head(0), net: -40_000_000_000 * scale, gross: 40_000_000_000 * scale };
    let s = &shard.s;
    let d = Draw { amount: KAS1, prices: PRICES };
    assert_eq!(s.requirement(&PRICES), None);
    assert_eq!(s.draw_end_judged(&d, &xk(&w.c)), None, "the script cannot compute it");
    assert_eq!(s.draw_max(&PRICES), 0);
    assert!(s.draw(KAS1, PRICES, T0, shard.value as i64).is_err(), "the tool states no draw on it");
    // Stated all the same and disputed at its end: no verdict is accepted, so the dispute lapses and the draw ends.
    let outs = Outs::of(&[Out { key: false, to: s.drawn(KAS1).hash(), value: shard.value as i64 - KAS1 }, Out { key: true, to: xk(&w.c), value: KAS1 }]).unwrap();
    let st = Stated { position: s.hash(), window: s.dispute_daa, late: s.t.late_daa, kind: KIND_DRAW, t0: T0, arg: d.arg(), outs, bond: s.bond(), claimer: xk(&w.c), held: shard.value as i64 };
    let q = w.pointed(&w.stated(&shard, &st, &d.said()), 0);
    let f = [fund(xk(&w.w), 34)];
    assert!(shard_end(&w.lib, &q, s, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), None).is_err());
    for verdict in [true, false] {
        let forced = shard_end(&w.lib, &q, s, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), Some(verdict));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "the end is answered {verdict}");
    }
    let b = point_lapse(&w.lib, &q, &w.script("KpfTPoint"), w.payer(&w.w, &f)).unwrap();
    let sg = signed("draw nobody can answer lapses", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value, &hash_spk(&w.lib, &s.hash())), "the shard again");
    // A draw that is wrong before any worth is computed (another key than the owner's) is answered wrong all the same.
    let lie = Stated { claimer: xk(&w.w), outs: Outs::of(&[Out { key: false, to: s.drawn(KAS1).hash(), value: shard.value as i64 - KAS1 }, Out { key: true, to: xk(&w.w), value: KAS1 }]).unwrap(), ..st };
    assert_eq!(s.draw_end_judged(&d, &lie.claimer), Some(false));
    let q = w.pointed(&w.stated(&shard, &lie, &d.said()), 0);
    let (b, right) = shard_end(&w.lib, &q, s, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    signed("draw of a shard too large, by another key, answered wrong", &b, &w.keys);
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

/// Review 32 did not reach a random comparison of the reserve's arithmetic between model and script: here it is for
/// the draw. Random shards (rows of odd units, empty rows, what they hold), random draws around the most that is
/// drawn (a sompi more, a sompi less, nothing, by another key, a price of nothing or beyond the limit, outputs a
/// sompi off): `KpfTShardEnd` accepts the model's verdict and not the other; and for a row's price against a
/// committee's near it, `KpfTShardPrice` the same.
#[test]
fn the_draws_answers_agree_with_the_model_on_random_shards() {
    let w = world();
    let base = w.shard();
    let scale = w.markets[0].p.scale;
    let pm = w.markets[0].p.price_max;
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let f = [fund(xk(&w.w), 36)];
    let (mut rights, mut wrongs, mut prices_right, mut prices_wrong) = (0, 0, 0, 0);
    for case in 0..60 {
        let mut shard = base.clone();
        for i in 0..4 {
            shard.s.rows[i] = if r.range(0, 3) == 0 {
                ShardRow::EMPTY
            } else {
                let gross = r.range(0, 60) * scale + r.range(0, scale - 1);
                ShardRow { head: w.head(i), net: r.range(-gross, gross), gross }
            };
        }
        shard.s.held = r.range(20, 200) * KAS1 + r.range(0, KAS1 - 1);
        // What its legs were entered at: near what its net units are worth at 10, so that its equity is near what
        // it holds; and now and then it is called.
        shard.s.basis = shard.s.rows.iter().map(|x| x.net / scale * P10).sum::<i64>() + r.range(-60, 60) * KAS1;
        shard.s.called = if r.range(0, 19) == 0 { 6_000 } else { 0 };
        shard.value = (shard.s.held + SHARD_CARRIER) as u64;
        let s = &shard.s;
        let mut prices = [0i64; 4];
        for (i, p) in prices.iter_mut().enumerate() {
            *p = match r.range(0, 59) {
                0 => 0,
                1 => pm + 1,
                2 => pm,
                _ => P10 / 10 * r.range(5, 15) + r.range(0, 999),
            };
            if s.rows[i].head == Head::EMPTY && r.range(0, 1) == 0 {
                *p = 0;
            }
        }
        let max = s.draw_max(&prices);
        let amount = match r.range(0, 11) {
            0 | 1 => max + 1,
            2 => 0,
            3 => -1,
            4 => s.held + 1,
            5 => r.range(1, s.held),
            6 | 7 => max - 1,
            8 => r.range(1, max.max(1)),
            _ => max,
        };
        let claimer = if r.range(0, 14) == 0 { xk(&w.w) } else { xk(&w.c) };
        let held = shard.value as i64;
        let off = if r.range(0, 14) == 0 { 1 } else { 0 };
        let outs = Outs::of(&[Out { key: false, to: s.drawn(amount).hash(), value: held - amount }, Out { key: true, to: claimer, value: amount - off }]).filter(|_| amount - off > 0 && amount < held).unwrap_or_else(|| Outs::of(&[Out { key: false, to: s.hash(), value: held - 1 }, Out { key: true, to: claimer, value: 1 }]).unwrap());
        let d = Draw { amount, prices };
        let st = Stated { position: s.hash(), window: s.dispute_daa, late: s.t.late_daa, kind: KIND_DRAW, t0: T0, arg: d.arg(), outs, bond: s.bond(), claimer, held };
        let right = s.draw_end_right(&d, &claimer, &st.outs, held);
        assert!(s.draw_end_judged(&d, &claimer).is_some(), "case {case}: within what a script computes");
        if right {
            assert!(amount <= max && amount > 0 && off == 0 && claimer == s.owner, "case {case}: right only within the most that is drawn");
            rights += 1;
        } else {
            wrongs += 1;
        }
        // The statement, disputed at its end (the object as the dispute leaves it: the shard's value and two bonds).
        let q = OLive { s: Pointed { st, disputer: xk(&w.w), point: 0 }, value: shard.value + 2 * st.bond as u64, id: shard.id, op: op(40), daa: 6_520 };
        let (b, said) = shard_end(&w.lib, &q, s, &d, &w.script("KpfTShardEnd"), xk(&w.f), w.payer(&w.w, &f), None).unwrap();
        assert_eq!(said, right);
        assert!(accepted(&b, &w.keys), "case {case}: the model's verdict ({right}) is not the script's");
        let other = shard_end(&w.lib, &q, s, &d, &w.script("KpfTShardEnd"), xk(&w.f), w.payer(&w.w, &f), Some(!right));
        assert!(other.is_err() || !accepted(&other.unwrap().0, &w.keys), "case {case}: the script takes the other verdict ({})", !right);
        // A row's price against a committee's near it (a case in three: a citation is a large transaction).
        if case % 3 == 0 {
            let at = r.range(1, 4);
            let row = s.rows[at as usize - 1];
            if row.head != Head::EMPTY {
                let stated = prices[at as usize - 1];
                let g = s.t.g_bps;
                let c = match r.range(0, 5) {
                    0 => stated * 10_000 / (10_000 - g) + r.range(-2, 2),
                    1 => stated * 10_000 / (10_000 + g) + r.range(-2, 2),
                    2 => stated,
                    _ => (stated + r.range(-stated / 50, stated / 50)).max(1),
                }
                .clamp(1, pm);
                let q = OLive { s: Pointed { st, disputer: xk(&w.w), point: at }, ..q.clone() };
                let cited = w.cited(at as usize - 1, c);
                let cite1 = w.script("KpfTCite");
                let right = s.draw_price_right(&d, at as usize, c);
                let (b, said) = shard_price(&w.lib, &q, s, &d, at, &w.script("KpfTShardPrice"), Some((&cite1, &cited)), xk(&w.f), w.payer(&w.w, &f), None).unwrap();
                assert_eq!(said, right);
                assert!(accepted(&b, &w.keys), "case {case}: row {at} at {stated} against {c}: the model's verdict ({right}) is not the script's");
                let other = shard_price(&w.lib, &q, s, &d, at, &w.script("KpfTShardPrice"), Some((&cite1, &cited)), xk(&w.f), w.payer(&w.w, &f), Some(!right));
                assert!(other.is_err() || !accepted(&other.unwrap().0, &w.keys), "case {case}: row {at}: the script takes the other verdict");
                if right { prices_right += 1 } else { prices_wrong += 1 }
            }
        }
    }
    println!("ends: {rights} right, {wrongs} wrong; prices: {prices_right} right, {prices_wrong} wrong");
    assert!(rights >= 8 && wrongs >= 8 && prices_right >= 2 && prices_wrong >= 2, "both verdicts are met: {rights} {wrongs} {prices_right} {prices_wrong}");
}

#[test]
fn what_is_kept_of_a_shard_a_note_and_a_draw_reads_back_as_it_was_written() {
    use kpf_protocol::aobj::{AObj, Host};
    let w = world();
    let shard = w.shard();
    let s = &shard.s;
    let (st, d) = s.draw(KAS1, PRICES, T0, shard.value as i64).unwrap();
    let leg = XLeg { market: w.markets[0].id(), committee: w.markets[0].committee_id, scale: w.markets[0].p.scale, price_max: w.markets[0].p.price_max, size: 500 * w.markets[0].p.scale, dir: LONG, entry: P10 };
    let account = Account { t: w.terms(), legs: vec![leg, XLeg { dir: -LONG, ..leg }], a_key: xk(&w.w), b_key: xk(&w.c), a_margin: 50 * KAS1, b_margin: 3, expiry: 9_000_000, dispute_daa: 50, bond_bps: 1_000, a_min_counter: 7, b_min_counter: 9, shard: Some(s.sid) };
    let plain = Account { shard: None, legs: vec![], ..account.clone() };
    let all = vec![
        AObj::Shard(*s),
        AObj::Account(account.clone()),
        AObj::Account(plain),
        AObj::Note(SNote::leg(NOTE_JOIN, s.sid, &leg, xk(&w.c))),
        AObj::Note(SNote::owed(s.sid, xk(&w.w), xk(&w.c), 3 * KAS1)),
        AObj::Stated { on: Host::Shard(*s), st, said: d.said() },
        AObj::Pointed { on: Host::Shard(*s), q: Pointed { st, disputer: xk(&w.w), point: 3 }, said: d.said() },
    ];
    for o in &all {
        let b = o.encode();
        let back = AObj::decode(&b).unwrap_or_else(|e| panic!("{}: {e}", o.kind()));
        assert_eq!((&back, back.hash()), (o, o.hash()), "{}", o.kind());
        // A byte more, a byte less, and another kind: refused.
        assert!(AObj::decode(&[&b[..], &[0]].concat()).is_err() && AObj::decode(&b[..b.len() - 1]).is_err(), "{}", o.kind());
        let mut other = b.clone();
        other[0] = if b[0] == 20 { 21 } else { 20 };
        assert!(AObj::decode(&other).is_err(), "{} read as another kind", o.kind());
    }
    // A statement whose bytes are not of its kind (a draw's 40 bytes on an account) is not one.
    let odd = AObj::Stated { on: Host::Account(account), st: Stated { kind: KIND_ACCOUNT_TAKE, ..st }, said: d.said() };
    assert!(AObj::decode(&odd.encode()).is_err());
    assert_eq!(all[0].least_value(), s.held + SHARD_CARRIER);
}

// ---------------------------------------------------------------- section 13, stage 2: the notes of a leaving

use kpf_protocol::reserve::{Left, Rest};
use kpf_protocol::state::SHORT;

fn fleg(w: &World, i: usize, units: i64, dir: i64, entry: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir, entry }
}

/// A shard of 100 KAS with no leg, to book legs in by hand (the statement of an entry does it: stage 4).
fn bare(w: &World) -> OLive<Shard> {
    let x = w.shard();
    OLive { s: Shard { rows: [ShardRow::EMPTY; 4], basis: 0, ..x.s }, ..x }
}

fn left_note(shard: &OLive<Shard>, n: &Left, k: u8) -> OLive<Left> {
    OLive { s: n.clone(), value: NOTE_MIN as u64, id: shard.id, op: op(60 + k), daa: 6_200 }
}

/// A leg is in the books from its entry; the note that legs left takes each out of its row and out of what the
/// legs were entered at. Whatever the order the notes are applied in, the books end as they began.
#[test]
fn the_note_that_legs_left_takes_them_out_of_the_books_whatever_the_order() {
    let w = world();
    let base = bare(&w);
    let scale = w.markets[0].p.scale;
    // Six legs of three users in three markets.
    let legs = [fleg(&w, 0, 10, LONG, P10), fleg(&w, 0, 4, SHORT, P10 * 11 / 10), fleg(&w, 1, 7, LONG, P10), fleg(&w, 1, 7, SHORT, P10 * 9 / 10), fleg(&w, 2, 3, LONG, 2 * P10), fleg(&w, 0, 5, LONG, P10 * 12 / 10)];
    let mut s = base.s;
    for l in &legs {
        s = s.booked(l).unwrap();
    }
    assert_eq!([s.rows[0].net, s.rows[0].gross, s.rows[1].net, s.rows[1].gross, s.rows[2].net, s.rows[2].gross].map(|x| x / scale), [-11, 19, 0, 14, -3, 3]);
    assert_eq!(s.basis, -183 * KAS1, "what the legs were entered at: side B's units times their entries");
    // At 10, 10 and 20 its users have lost 13 KAS between them: its equity is what it holds and those.
    assert_eq!(s.equity(&[P10, P10, 2 * P10, 0]), Some(113 * KAS1));
    // Three notes: a user's two legs of market 0; one leg, for which 3 KAS are asked; three legs of two markets.
    let sid = s.sid;
    let notes = [
        Left { sid, legs: vec![legs[0], legs[5]], key: xk(&w.w), asked: 0 },
        Left { sid, legs: vec![legs[1]], key: xk(&w.c), asked: 3 * KAS1 },
        Left { sid, legs: vec![legs[2], legs[3], legs[4]], key: xk(&w.w), asked: 0 },
    ];
    let f = [fund(xk(&w.w), 43)];
    for order in [[0usize, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let mut shard = OLive { s, ..base.clone() };
        for (k, i) in order.iter().enumerate() {
            let note = left_note(&shard, &notes[*i], *i as u8);
            let (b, next, rest) = left_apply(&w.lib, &shard, &note, &w.script("KpfTLeft"), w.payer(&w.w, &f)).unwrap();
            let sg = signed(&format!("legs left: note {i} applied in place {k}"), &b, &w.keys);
            assert!(rest.is_none());
            if notes[*i].asked > 0 {
                let n = sg.tx.outputs.len();
                assert_eq!((sg.tx.outputs[n - 1].value, &sg.tx.outputs[n - 1].script_public_key), (notes[*i].asked as u64, &p2pk_spk(&xk(&w.c))));
            }
            shard = nth(&sg, 0, next, 6_300 + k as u64);
        }
        assert_eq!((shard.s.rows, shard.s.basis, shard.s.held, shard.value), ([ShardRow::EMPTY; 4], 0, 97 * KAS1, (97 * KAS1 + SHARD_CARRIER) as u64), "order {order:?}");
    }
    // A leg that leaves at its entry with nothing asked changes nothing of the equity at that price.
    let (after, at, paid, rest) = s.left(&Left { sid, legs: vec![legs[2]], key: xk(&w.w), asked: 0 }).unwrap();
    assert_eq!((at, paid, rest, after.equity(&[P10, P10, 2 * P10, 0])), (vec![1], 0, None, s.equity(&[P10, P10, 2 * P10, 0])));
}

/// What a shard does not hold goes on as a note of the rest; nothing is paid while it is called; and what is no
/// note of legs in its books is not applied.
#[test]
fn what_a_shard_does_not_hold_goes_on_as_a_note_of_the_rest_and_nothing_is_paid_while_it_is_called() {
    let w = world();
    let base = bare(&w);
    let leg = fleg(&w, 0, 10, LONG, P10);
    let s = base.s.booked(&leg).unwrap();
    let shard = OLive { s, ..base.clone() };
    let f = [fund(xk(&w.w), 43)];
    let script = w.script("KpfTLeft");
    // 130 KAS are asked of a shard that holds 100: it pays the 100, and 30 go on.
    let n = Left { sid: s.sid, legs: vec![leg], key: xk(&w.c), asked: 130 * KAS1 };
    let (b, next, rest) = left_apply(&w.lib, &shard, &left_note(&shard, &n, 0), &script, w.payer(&w.w, &f)).unwrap();
    let sg = signed("legs left: more asked than the shard holds", &b, &w.keys);
    let rest = rest.expect("the rest goes on");
    let k = sg.tx.outputs.len();
    assert_eq!((next.held, next.rows, next.basis, rest, sg.tx.outputs[k - 1].value), (0, [ShardRow::EMPTY; 4], 0, Rest { sid: s.sid, key: xk(&w.c), amount: 30 * KAS1 }, 100 * KAS1 as u64));
    assert_eq!(sg.tx.outputs[2].script_public_key, hash_spk(&w.lib, &rest.hash()));
    // The shard holds 50 again: the rest is paid, and nothing goes on.
    let funded = OLive { s: Shard { held: 50 * KAS1, ..next }, value: (50 * KAS1 + SHARD_CARRIER) as u64, ..nth(&sg, 0, next, 6_300) };
    let rnote = nth(&sg, 2, rest, 6_300);
    let (b, after, more) = rest_apply(&w.lib, &funded, &rnote, &script, w.payer(&w.w, &f)).unwrap();
    signed("the rest paid", &b, &w.keys);
    assert_eq!((after.held, more), (20 * KAS1, None));
    assert!(rest_apply(&w.lib, &OLive { s: Shard { held: 0, ..next }, ..funded.clone() }, &rnote, &script, w.payer(&w.w, &f)).is_err(), "a shard that holds nothing pays nothing");

    // While the shard is called nothing is paid here: a note that asks something waits; one that asks nothing is
    // applied (the leg is out of the books).
    let called = OLive { s: Shard { called: 6_400, ..s }, ..shard.clone() };
    assert!(left_apply(&w.lib, &called, &left_note(&called, &n, 0), &script, w.payer(&w.w, &f)).is_err());
    let (open_next, at, paid, open_rest) = s.left(&n).unwrap();
    let r = left_apply_as(&w.lib, &called, &left_note(&called, &n, 0), &at, Shard { called: 6_400, ..open_next }, paid, open_rest, &script, w.payer(&w.w, &f));
    assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "a called shard pays a note");
    let quiet = Left { asked: 0, ..n.clone() };
    let (b, out, _) = left_apply(&w.lib, &called, &left_note(&called, &quiet, 1), &script, w.payer(&w.w, &f)).unwrap();
    signed("legs left a called shard's books, nothing asked", &b, &w.keys);
    assert_eq!((out.rows, out.basis, out.called), ([ShardRow::EMPTY; 4], 0, 6_400));
    let paid_rest = rest_apply(&w.lib, &OLive { s: Shard { called: 6_400, ..funded.s }, ..funded.clone() }, &rnote, &script, w.payer(&w.w, &f));
    assert!(paid_rest.is_err(), "a called shard pays a note of the rest");

    // What is not applied: a leg that is in no row; a leg larger than its row has; a leg read out of another row;
    // the books left with the leg in them; what the legs were entered at left as it was.
    let other = Left { legs: vec![fleg(&w, 1, 10, LONG, P10)], ..quiet.clone() };
    assert!(left_apply(&w.lib, &shard, &left_note(&shard, &other, 2), &script, w.payer(&w.w, &f)).is_err());
    let big = Left { legs: vec![fleg(&w, 0, 11, LONG, P10)], ..quiet.clone() };
    assert!(left_apply(&w.lib, &shard, &left_note(&shard, &big, 3), &script, w.payer(&w.w, &f)).is_err());
    let (good, at, _, _) = s.left(&quiet).unwrap();
    let lies: Vec<(&str, Vec<usize>, Shard)> = vec![
        ("the leg read out of another row", vec![1], good),
        ("the leg left in its row", at.clone(), Shard { rows: s.rows, ..good }),
        ("what the legs were entered at left as it was", at.clone(), Shard { basis: s.basis, ..good }),
        ("more held than there was", at.clone(), Shard { held: good.held + 1, ..good }),
    ];
    for (what, at, next) in lies {
        let r = left_apply_as(&w.lib, &shard, &left_note(&shard, &quiet, 4), &at, next, 0, None, &script, w.payer(&w.w, &f));
        assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "{what}");
    }
    let (b, _, _) = left_apply_as(&w.lib, &shard, &left_note(&shard, &quiet, 4), &at, good, 0, None, &script, w.payer(&w.w, &f)).unwrap();
    assert!(accepted(&b, &w.keys), "the control");
}


/// Until the note that legs left is applied the books have legs that are gone: what they lost reads as the shard's
/// equity. A draw stated meanwhile is ended by that note, whatever it asks; and by a note of the rest, which is
/// what the shard owes and its books do not say.
#[test]
fn a_note_that_legs_left_or_a_note_of_the_rest_that_is_not_applied_ends_a_draw() {
    let w = world();
    let base = bare(&w);
    // A user is short 10 units from 10: the shard's side is long them.
    let leg = fleg(&w, 0, 10, SHORT, P10);
    let s = base.s.booked(&leg).unwrap();
    let shard = OLive { s, ..base.clone() };
    // The leg left at 12; its note is not applied. At 12 the books read 20 KAS gained, and the owner draws on them.
    let at12 = [P10 * 12 / 10, 0, 0, 0];
    assert_eq!(s.equity(&at12), Some(120 * KAS1));
    let (st, d) = s.draw(s.draw_max(&at12), at12, T0, shard.value as i64).unwrap();
    let live = w.stated(&shard, &st, &d.said());
    let f = [fund(xk(&w.w), 37)];
    let script = w.script("KpfTDrawNote");
    let note = OLive { s: Left { sid: s.sid, legs: vec![leg], key: xk(&w.w), asked: 0 }, value: NOTE_MIN as u64, id: shard.id, op: op(70), daa: 6_400 };
    let b = draw_note_left(&w.lib, &live, &st, None, &s, &note, &script, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    let sg = signed("a draw ended by a note that legs left", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value, &hash_spk(&w.lib, &s.hash())), "the shard again");
    assert_eq!(sg.tx.outputs[2].script_public_key, hash_spk(&w.lib, &note.s.hash()), "the note goes on as it is");
    // One that landed after the statement ends nothing, nor one to another shard.
    assert!(draw_note_left(&w.lib, &live, &st, None, &s, &OLive { daa: 6_520, ..note.clone() }, &script, xk(&w.w), w.payer(&w.w, &f)).is_err());
    let elsewhere = OLive { s: Left { sid: [0x44; 32], ..note.s.clone() }, ..note.clone() };
    assert!(draw_note_left(&w.lib, &live, &st, None, &s, &elsewhere, &script, xk(&w.w), w.payer(&w.w, &f)).is_err());
    // A note of the rest that is not paid ends a draw too.
    let rest = OLive { s: Rest { sid: s.sid, key: xk(&w.w), amount: 5 * KAS1 }, value: NOTE_MIN as u64, id: shard.id, op: op(71), daa: 6_400 };
    let b = draw_note_rest(&w.lib, &live, &st, None, &s, &rest, &script, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    signed("a draw ended by a note of the rest", &b, &w.keys);
    // A note of other legs than the object shown is no such note: the transaction of the one, with the other's legs.
    let (mut t, other) = (b.clone(), Rest { amount: 6 * KAS1, ..rest.s });
    t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &other.hash());
    assert!(!accepted(&t, &w.keys), "the note is written on as another");
}
