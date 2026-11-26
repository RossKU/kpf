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
use kpf_protocol::reserve::{Draw, Head, Shard, ShardRow, NOTE_MIN, SHARD_CARRIER};
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
    assert_eq!(s.draw_wrong_point(&d, &st.claimer, &st.outs, shard.value as i64, T0, &PRICES), None);
    // Applied after the dispute period: the shard holds the amount less, the amount and the bond to the owner.
    let live = nth(&sg, 0, st, 6_510);
    let f = [fund(xk(&w.w), 31)];
    let b = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).unwrap();
    let r = signed("draw applied", &b, &w.keys);
    let n = r.tx.outputs.len();
    assert_eq!((r.tx.outputs[0].value, &r.tx.outputs[0].script_public_key), (shard.value - max as u64, &hash_spk(&w.lib, &s.drawn(max, T0).hash())));
    assert_eq!((r.tx.outputs[n - 1].value, &r.tx.outputs[n - 1].script_public_key), (max as u64, &p2pk_spk(&xk(&w.c))));
    // A sompi more is not stated by the tool; stated all the same, its end is answered wrong.
    assert!(shard_draw(&w.lib, &shard, &w.script("KpfTChange"), max + 1, PRICES, T0, w.payer(&w.c, &f)).is_err());
    let lies: Vec<(&str, Stated, Draw)> = vec![
        ("a sompi more", { let d = Draw { amount: max + 1, prices: PRICES }; Stated { arg: d.arg(), outs: Outs::of(&[Out { key: false, to: s.drawn(max + 1, T0).hash(), value: shard.value as i64 - max - 1 }, Out { key: true, to: xk(&w.c), value: max + 1 }]).unwrap(), ..st } }, Draw { amount: max + 1, prices: PRICES }),
        ("another key than the owner's", Stated { claimer: xk(&w.w), outs: Outs::of(&[Out { key: false, to: s.drawn(max, T0).hash(), value: shard.value as i64 - max }, Out { key: true, to: xk(&w.w), value: max }]).unwrap(), ..st }, d),
        ("rows of its own making in the shard that goes on", Stated { outs: Outs::of(&[Out { key: false, to: Shard { rows: [ShardRow::EMPTY; 4], ..s.drawn(max, T0) }.hash(), value: shard.value as i64 - max }, Out { key: true, to: xk(&w.c), value: max }]).unwrap(), ..st }, d),
        ("a market with no price", { let d = Draw { amount: KAS1, prices: [P10, 0, 0, 0] }; Stated { arg: d.arg(), outs: Outs::of(&[Out { key: false, to: s.drawn(KAS1, T0).hash(), value: shard.value as i64 - KAS1 }, Out { key: true, to: xk(&w.c), value: KAS1 }]).unwrap(), ..st } }, Draw { amount: KAS1, prices: [P10, 0, 0, 0] }),
    ];
    for (what, lie, d) in lies {
        assert!(!s.draw_end_right(&d, &lie.claimer, &lie.outs, shard.value as i64, T0), "{what}");
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
        let outs = Outs::of(&[Out { key: false, to: x.drawn(amount, T0).hash(), value: on.value as i64 - amount }, Out { key: true, to: xk(&w.c), value: amount }]).unwrap();
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
    assert_eq!(s.draw_wrong_point(&d, &st.claimer, &st.outs, shard.value as i64, T0, &PRICES), Some(1), "the end follows from the prices stated: the price of row 1 is the point");
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
    assert_eq!(s.draw_end_judged(&d, &xk(&w.c), T0), None, "the script cannot compute it");
    assert_eq!(s.draw_max(&PRICES), 0);
    assert!(s.draw(KAS1, PRICES, T0, shard.value as i64).is_err(), "the tool states no draw on it");
    // Stated all the same and disputed at its end: no verdict is accepted, so the dispute lapses and the draw ends.
    let outs = Outs::of(&[Out { key: false, to: s.drawn(KAS1, T0).hash(), value: shard.value as i64 - KAS1 }, Out { key: true, to: xk(&w.c), value: KAS1 }]).unwrap();
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
    let lie = Stated { claimer: xk(&w.w), outs: Outs::of(&[Out { key: false, to: s.drawn(KAS1, T0).hash(), value: shard.value as i64 - KAS1 }, Out { key: true, to: xk(&w.w), value: KAS1 }]).unwrap(), ..st };
    assert_eq!(s.draw_end_judged(&d, &lie.claimer, T0), Some(false));
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
        let outs = Outs::of(&[Out { key: false, to: s.drawn(amount, T0).hash(), value: held - amount }, Out { key: true, to: claimer, value: amount - off }]).filter(|_| amount - off > 0 && amount < held).unwrap_or_else(|| Outs::of(&[Out { key: false, to: s.hash(), value: held - 1 }, Out { key: true, to: claimer, value: 1 }]).unwrap());
        let d = Draw { amount, prices };
        let st = Stated { position: s.hash(), window: s.dispute_daa, late: s.t.late_daa, kind: KIND_DRAW, t0: T0, arg: d.arg(), outs, bond: s.bond(), claimer, held };
        let right = s.draw_end_right(&d, &claimer, &st.outs, held, T0);
        assert!(s.draw_end_judged(&d, &claimer, T0).is_some(), "case {case}: within what a script computes");
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
        AObj::Left(Left { sid: s.sid, legs: vec![leg, XLeg { dir: -LONG, ..leg }], key: xk(&w.w), asked: 3 * KAS1 }),
        AObj::Left(Left { sid: s.sid, legs: vec![leg], key: xk(&w.w), asked: -3 * KAS1 }),
        AObj::Rest(Rest { sid: s.sid, key: xk(&w.w), amount: 3 * KAS1 }),
        AObj::Rest(Rest { sid: s.sid, key: xk(&w.w), amount: 3 * KAS1 }),
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

/// A note that legs left, applied (whole, by the script for its number of legs): the shard after, what was paid,
/// the note of the rest, and the transaction.
fn left_all(w: &World, what: &str, shard: &OLive<Shard>, note: &OLive<Left>) -> (OLive<Shard>, i64, Option<Rest>, OSigned) {
    let f = [fund(xk(&w.w), 43)];
    let (b, next, rest) = left_apply(&w.lib, shard, note, &w.script(left_script(note.s.legs.len())), w.payer(&w.w, &f)).unwrap();
    let sg = signed(&format!("{what}: a note of {} applied", note.s.legs.len()), &b, &w.keys);
    let paid = (shard.s.held - next.held).max(0);
    (nth(&sg, 0, next, shard.daa + 1), paid, rest, sg)
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
    for order in [[0usize, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let mut shard = OLive { s, ..base.clone() };
        for (k, i) in order.iter().enumerate() {
            let note = left_note(&shard, &notes[*i], *i as u8);
            let (next, paid, rest, sg) = left_all(&w, &format!("legs left: note {i} in place {k}"), &shard, &note);
            assert_eq!((paid, rest), (notes[*i].asked, None));
            if paid > 0 {
                assert!(pays(&sg, &xk(&w.c), paid));
            }
            shard = next;
        }
        assert_eq!((shard.s.rows, shard.s.basis, shard.s.held, shard.value), ([ShardRow::EMPTY; 4], 0, 97 * KAS1, (97 * KAS1 + SHARD_CARRIER) as u64), "order {order:?}");
    }
    // A leg that leaves at its entry with nothing asked changes nothing of the equity at that price.
    let (after, at, paid, rest) = s.left(&Left { sid, legs: vec![legs[2]], key: xk(&w.w), asked: 0 }).unwrap();
    assert_eq!((at, paid, rest, after.equity(&[P10, P10, 2 * P10, 0])), (vec![1], 0, None, s.equity(&[P10, P10, 2 * P10, 0])));
    // A note is of six legs at most (an account has no more: section 14.2): of six it is applied in one
    // transaction, the books as they began; of seven by nothing.
    let seven: Vec<XLeg> = (0..7).map(|k| fleg(&w, k % 3, 2 + k as i64, if k % 2 == 0 { LONG } else { SHORT }, P10 + k as i64 * KAS1)).collect();
    let mut t = base.s;
    for l in &seven[..6] {
        t = t.booked(l).unwrap();
    }
    let note = Left { sid, legs: seven[..6].to_vec(), key: xk(&w.c), asked: 7 * KAS1 };
    let full = OLive { s: t, ..base.clone() };
    let (end, paid, rest, _) = left_all(&w, "six legs left", &full, &left_note(&full, &note, 9));
    assert_eq!((end.s.rows, end.s.basis, end.s.held, paid, rest), ([ShardRow::EMPTY; 4], 0, 93 * KAS1, 7 * KAS1, None));
    let more = OLive { s: t.booked(&seven[6]).unwrap(), ..base.clone() };
    assert!(more.s.left(&Left { sid, legs: seven, key: xk(&w.c), asked: 0 }).is_err(), "a note of seven legs");
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
    // 130 KAS are asked of a shard that holds 100: it pays the 100, and 30 go on: the note of the rest, counted in
    // what the shard owes when it is written (review 54, F94).
    let n = Left { sid: s.sid, legs: vec![leg], key: xk(&w.c), asked: 130 * KAS1 };
    let (b, next, rest) = left_apply(&w.lib, &shard, &left_note(&shard, &n, 0), &script, w.payer(&w.w, &f)).unwrap();
    let sg = signed("legs left: more asked than the shard holds", &b, &w.keys);
    let rest = rest.expect("the rest goes on");
    let k = sg.tx.outputs.len();
    assert_eq!((next.held, next.owed, next.rows, next.basis, rest, sg.tx.outputs[k - 1].value), (0, 30 * KAS1, [ShardRow::EMPTY; 4], 0, Rest { sid: s.sid, key: xk(&w.c), amount: 30 * KAS1 }, 100 * KAS1 as u64));
    assert_eq!(sg.tx.outputs[2].script_public_key, hash_spk(&w.lib, &rest.hash()));
    let at0 = s.left(&n).unwrap().1;
    for (what, owed) in [("the rest written and not counted", 0), ("counted twice", 60 * KAS1)] {
        let r = left_apply_as(&w.lib, &shard, &left_note(&shard, &n, 0), &at0, Shard { owed, ..next }, 100 * KAS1, Some(rest), &script, w.payer(&w.w, &f));
        assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "{what}");
    }
    // The shard holds 50 again: the rest is paid, is owed no more, and nothing goes on.
    let funded = OLive { s: Shard { held: 50 * KAS1, ..next }, value: (50 * KAS1 + SHARD_CARRIER) as u64, ..nth(&sg, 0, next, 6_300) };
    let rnote = nth(&sg, 2, rest, 6_300);
    let (b, after, more) = rest_apply(&w.lib, &funded, &rnote, &script, w.payer(&w.w, &f)).unwrap();
    signed("the rest paid", &b, &w.keys);
    assert_eq!((after.held, after.owed, more), (20 * KAS1, 0, None));
    assert!(rest_apply(&w.lib, &OLive { s: Shard { held: 0, ..next }, ..funded.clone() }, &rnote, &script, w.payer(&w.w, &f)).is_err(), "a shard that holds nothing pays nothing");
    // It holds 10: 10 are paid, and 20 go on, still owed.
    let part = OLive { s: Shard { held: 10 * KAS1, ..next }, value: (10 * KAS1 + SHARD_CARRIER) as u64, ..funded.clone() };
    let (b, pafter, pmore) = rest_apply(&w.lib, &part, &rnote, &script, w.payer(&w.w, &f)).unwrap();
    signed("the rest paid in part", &b, &w.keys);
    assert_eq!((pafter.held, pafter.owed, pmore), (0, 20 * KAS1, Some(Rest { amount: 20 * KAS1, ..rest })));
    // A note of the rest of more than its shard owes was never counted: it is spent, and nothing is paid (so that
    // it ends no draw for good), called or not.
    for called in [0, 6_400] {
        let never = OLive { s: Shard { owed: 29 * KAS1, called, ..funded.s }, ..funded.clone() };
        let (b, same, gone) = rest_apply(&w.lib, &never, &rnote, &script, w.payer(&w.w, &f)).unwrap();
        let sg = signed("a note of the rest that was never counted, spent", &b, &w.keys);
        assert_eq!((same, gone), (never.s, None));
        assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (never.value, &hash_spk(&w.lib, &never.s.hash())));
        assert!(!pays(&sg, &rnote.s.key, 30 * KAS1) && !pays(&sg, &rnote.s.key, 29 * KAS1));
    }

    // While the shard is called nothing is paid here: all that a note asks goes on as the note of the rest, counted
    // (the call's own script pays it its share: docs 14.6); one that asks nothing is applied (the leg is out of
    // the books).
    let called = OLive { s: Shard { called: 6_400, ..s }, ..shard.clone() };
    let (b, cnext, crest) = left_apply(&w.lib, &called, &left_note(&called, &n, 0), &script, w.payer(&w.w, &f)).unwrap();
    let sg = signed("a note that asks, applied to a called shard: all of it is the rest", &b, &w.keys);
    assert_eq!((cnext.held, cnext.owed, cnext.rows, cnext.basis, cnext.called), (s.held, n.asked, [ShardRow::EMPTY; 4], 0, 6_400), "nothing is paid, and what is asked is owed");
    assert_eq!(crest, Some(Rest { sid: s.sid, key: n.key, amount: n.asked }));
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (called.value, &hash_spk(&w.lib, &cnext.hash())));
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &crest.unwrap().hash())).1, NOTE_MIN as u64, "the note of the rest");
    assert!(!pays(&sg, &n.key, n.asked.min(s.held)), "a called shard pays no note");
    let (open_next, at, paid, open_rest) = s.left(&n).unwrap();
    let r = left_apply_as(&w.lib, &called, &left_note(&called, &n, 0), &at, Shard { called: 6_400, ..open_next }, paid, open_rest, &script, w.payer(&w.w, &f));
    assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "a called shard pays a note");
    let at_c = called.s.left(&n).unwrap().1;
    for (what, shard_after, note_after) in [
        ("the rest is not counted", Shard { owed: 0, ..cnext }, crest),
        ("counted twice", Shard { owed: 2 * n.asked, ..cnext }, crest),
        ("a note of a sompi more", cnext, Some(Rest { amount: n.asked + 1, ..crest.unwrap() })),
    ] {
        let r = left_apply_as(&w.lib, &called, &left_note(&called, &n, 0), &at_c, shard_after, 0, note_after, &script, w.payer(&w.w, &f));
        assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "{what}");
    }
    let quiet = Left { asked: 0, ..n.clone() };
    let (b, out, _) = left_apply(&w.lib, &called, &left_note(&called, &quiet, 1), &script, w.payer(&w.w, &f)).unwrap();
    signed("legs left a called shard's books, nothing asked", &b, &w.keys);
    assert_eq!((out.rows, out.basis, out.called), ([ShardRow::EMPTY; 4], 0, 6_400));
    // A note of the rest is not paid by this script while its shard is called.
    assert!(rest_apply(&w.lib, &OLive { s: Shard { called: 6_400, ..funded.s }, ..funded.clone() }, &rnote, &script, w.payer(&w.w, &f)).is_err(), "a called shard pays a note of the rest in full");

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
    // A note of two legs is applied by the script of two to six, in one transaction: both legs out and what is
    // asked paid. There is no state of the books between the two (review 48, F76): with one leg left in, with
    // the other's row as it was, or by the script of one leg, it is refused.
    let two = fleg(&w, 1, 3, SHORT, P10);
    let both = OLive { s: s.booked(&two).unwrap(), ..shard.clone() };
    let pair = Left { sid: s.sid, legs: vec![leg, two], key: xk(&w.c), asked: 4 * KAS1 };
    let pnote = left_note(&both, &pair, 5);
    let all = w.script("KpfTLeftAll");
    let (done, at, paid, rest) = both.s.left(&pair).unwrap();
    assert_eq!((done.rows, done.basis, done.held, paid, rest), ([ShardRow::EMPTY; 4], 0, 96 * KAS1, 4 * KAS1, None));
    let (b, _, _) = left_apply_as(&w.lib, &both, &pnote, &at, done, paid, None, &all, w.payer(&w.w, &f)).unwrap();
    signed("a note of two legs applied whole", &b, &w.keys);
    let half = Shard { rows: s.rows, basis: s.basis, ..done };
    let r = left_apply_as(&w.lib, &both, &pnote, &at, half, paid, None, &all, w.payer(&w.w, &f));
    assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "one leg left in the books");
    let r = left_apply_as(&w.lib, &both, &pnote, &[at[0], at[0]], done, paid, None, &all, w.payer(&w.w, &f));
    assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "the second leg read out of the first's row");
    assert!(left_apply(&w.lib, &both, &pnote, &script, w.payer(&w.w, &f)).is_err() || !accepted(&left_apply(&w.lib, &both, &pnote, &script, w.payer(&w.w, &f)).unwrap().0, &w.keys), "by the script of one leg");
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
    // (It owes 5 KAS by a note of the rest, which it counted when the note was written.)
    let s = Shard { owed: 5 * KAS1, ..base.s.booked(&leg).unwrap() };
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
    // One of more than the shard owes was never counted: it ends no draw (review 55, F96), and is spent for nothing.
    let never = OLive { s: Rest { amount: 5 * KAS1 + 1, ..rest.s }, ..rest.clone() };
    assert!(draw_note_rest(&w.lib, &live, &st, None, &s, &never, &script, xk(&w.w), w.payer(&w.w, &f)).is_err());
    let mut t = b.clone();
    for o in t.tx.outputs.iter_mut().filter(|o| o.script_public_key == hash_spk(&w.lib, &rest.s.hash())) {
        o.script_public_key = hash_spk(&w.lib, &never.s.hash());
    }
    assert!(!accepted(&t, &w.keys), "a note of the rest that was never counted ends a draw");
    // A note of other legs than the object shown is no such note: the transaction of the one, with the other's legs.
    let (mut t, other) = (b.clone(), Rest { amount: 6 * KAS1, ..rest.s });
    t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &other.hash());
    assert!(!accepted(&t, &w.keys), "the note is written on as another");
}

// ---------------------------------------------------------------- section 13, stage 3: a covered account's own statements

use kpf_protocol::covered::{CSaid, ALL, KIND_C_LEAVE, KIND_C_STEP, KIND_C_TAKE};

/// A covered account of the user's (`w`) under the shard: its coins, its legs, nothing of the clearing side.
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

/// The end of a statement, disputed and answered: the model's verdict is taken, the other refused.
fn end_answered(w: &World, what: &str, acct: &OLive<Account>, live: &OLive<Stated>, s: &CSaid, want: bool) {
    let f = [fund(xk(&w.w), 35)];
    let q = w.pointed(live, 0);
    let (b, right) = c_end(&w.lib, &q, &acct.s, s, &w.script(match live.s.kind { 12 => "KpfTCSplit", 13 => "KpfTCMark", 14 | 15 => "KpfTCTok", _ => "KpfTCEnd" }), xk(&w.f), w.payer(&w.w, &f), None).unwrap();
    assert_eq!(right, want, "{what}");
    signed(&format!("{what}: the end answered {}", if right { "right" } else { "wrong" }), &b, &w.keys);
    let (b, _) = c_end(&w.lib, &q, &acct.s, s, &w.script(match live.s.kind { 12 => "KpfTCSplit", 13 => "KpfTCMark", 14 | 15 => "KpfTCTok", _ => "KpfTCEnd" }), xk(&w.f), w.payer(&w.w, &f), Some(!want)).unwrap();
    assert!(!accepted(&b, &w.keys), "{what}: the other verdict is taken");
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

/// The two legs of the tests: long 10 units of market 0 from 10, short 5 of market 1 from 10.
/// Whether the transaction pays exactly `value` to the key (the payer's change goes to a key too).
fn pays(sg: &OSigned, key: &[u8; 32], value: i64) -> bool {
    sg.tx.outputs.iter().any(|o| o.script_public_key == p2pk_spk(key) && o.value == value as u64)
}

fn two_legs(w: &World) -> Vec<XLeg> {
    vec![fleg(w, 0, 10, LONG, P10), fleg(w, 1, 5, SHORT, P10)]
}

/// A take leaves the coins at the maintenance of what the legs are worth and at what the legs that lost have lost:
/// what a leg gained is no coin.
#[test]
fn a_take_from_a_covered_account_leaves_the_coins_at_the_maintenance_and_at_what_its_legs_lost() {
    let w = world();
    let shard = bare(&w);
    let acct = cov(&w, &shard, &two_legs(&w), 100 * KAS1);
    // At 12 and 11: the long gained 20, the short lost 5. Worth 120 and 55: 5.5 % of 175 is 9.625; and the 5.
    let a = [P10 * 12 / 10, P10 * 11 / 10];
    // And a note's least, which is no coin of the account's (review 47, F75).
    let keep = 9_625 * KAS1 / 1_000 + 5 * KAS1 + NOTE_MIN;
    let max = 100 * KAS1 - keep;
    let f = [fund(xk(&w.w), 33)];
    assert!(c_state(&w.lib, &acct, &w.script("KpfTChange"), KIND_C_TAKE, max + 1, 0, &a, T0, xk(&w.w), w.payer(&w.w, &f)).is_err(), "a sompi more");
    assert!(c_state(&w.lib, &acct, &w.script("KpfTChange"), KIND_C_TAKE, max, 0, &a, T0, xk(&w.c), w.payer(&w.c, &f)).is_err(), "by another key");
    let (live, s, o, sg) = c_applied(&w, "a take", &acct, KIND_C_TAKE, max, 0, &a, &w.w);
    let after = o.after.clone().unwrap();
    assert_eq!((after.a_margin, after.legs.clone(), sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (keep, acct.s.legs.clone(), keep as u64, &hash_spk(&w.lib, &after.hash())));
    assert!(pays(&sg, &xk(&w.w), max));
    end_answered(&w, "a take", &acct, &live, &s, true);
    // A sompi more, stated all the same: its end is wrong.
    let more = CSaid { amount: max + 1, ..s.clone() };
    let outs = Outs::of(&[Out { key: false, to: acct.s.c_hash(keep - 1, &acct.s.legs_root()), value: keep - 1 }, Out { key: true, to: xk(&w.w), value: max + 1 }]).unwrap();
    let lie = Stated { arg: more.arg(), outs, ..live.s };
    assert_eq!(acct.s.c_wrong_point(KIND_C_TAKE, &more, &lie.claimer, &lie.outs, acct.value as i64, T0, &a), Some(0));
    end_answered(&w, "a take of a sompi more", &acct, &acct_stated(&w, &acct, &lie, &more.said()), &more, false);
    // The rule's take paid to another key: wrong.
    let outs = Outs::of(&[live.s.outs.list()[0], Out { key: true, to: xk(&w.f), value: max }]).unwrap();
    let lie = Stated { outs, ..live.s };
    end_answered(&w, "a take paid to another", &acct, &acct_stated(&w, &acct, &lie, &s.said()), &s, false);
}

/// A leaving is of one leg or of all. A leg that gained leaves alone where the coins that stay are at the
/// maintenance of the legs that stay and at what they have lost; one that lost, where the coins pay what it lost.
/// Of all, the shard is asked the net, once. What a leaving writes is one note; applied, the books lose the legs.
#[test]
fn a_leaving_of_one_leg_or_of_all_writes_one_note_and_the_books_lose_the_legs() {
    let w = world();
    let legs = two_legs(&w);
    let base = bare(&w);
    let shard = OLive { s: base.s.booked(&legs[0]).unwrap().booked(&legs[1]).unwrap(), ..base.clone() };
    let a = [P10 * 12 / 10, P10 * 11 / 10];
    let f = [fund(xk(&w.w), 43)];
    let left_script = w.script("KpfTLeft");

    // (a) The long, which gained 20, leaves alone from an account of 100 KAS: asked 20 less 1 % of its 120.
    let acct = cov(&w, &shard, &legs, 100 * KAS1);
    let (live, s, o, sg) = c_applied(&w, "a leg that gained leaves alone", &acct, KIND_C_LEAVE, 0, 1, &a, &w.w);
    let (after, note) = (o.after.clone().unwrap(), o.note.clone().unwrap());
    assert_eq!((note.asked, note.legs.clone(), note.key, after.a_margin, after.legs.clone()), (188 * KAS1 / 10, vec![legs[0]], xk(&w.w), 100 * KAS1, vec![legs[1]]));
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (100 * KAS1 as u64, &hash_spk(&w.lib, &after.hash())));
    let (k, v) = out_of(&sg, &hash_spk(&w.lib, &note.hash()));
    assert_eq!(v, NOTE_MIN as u64);
    end_answered(&w, "a leg that gained leaves alone", &acct, &live, &s, true);
    let (b, next, rest) = left_apply(&w.lib, &shard, &nth(&sg, k, note.clone(), 6_600), &left_script, w.payer(&w.w, &f)).unwrap();
    let ap = signed("its note applied", &b, &w.keys);
    // (The short stays in the row it was booked in; the long's row is no market's again.)
    let alone = base.s.booked(&legs[1]).unwrap();
    assert_eq!((next.rows[0], next.rows[1], next.basis, next.held, rest), (ShardRow::EMPTY, alone.rows[0], alone.basis, 100 * KAS1 - note.asked, None));
    assert!(pays(&ap, &xk(&w.w), note.asked));

    // (b) From an account of 5 KAS it does not leave alone (the short that stays asks 3.025 and lost 5). All
    // leave: the net is 15, less 1 % of the 175 they are worth; the user has its coins.
    let poor = cov(&w, &shard, &legs, 5 * KAS1);
    assert!(c_state(&w.lib, &poor, &w.script("KpfTChange"), KIND_C_LEAVE, 0, 1, &a, T0, xk(&w.w), w.payer(&w.w, &f)).is_err());
    let (live, s, o, sg) = c_applied(&w, "every leg leaves", &poor, KIND_C_LEAVE, 0, ALL, &a, &w.w);
    let note = o.note.clone().unwrap();
    assert_eq!((o.after.clone(), o.paid, note.asked, note.legs.clone()), (None, 5 * KAS1, 1_325 * KAS1 / 100, legs.clone()));
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &note.hash())).1, NOTE_MIN as u64);
    end_answered(&w, "every leg leaves", &poor, &live, &s, true);
    let (k, _) = out_of(&sg, &hash_spk(&w.lib, &note.hash()));
    let (end, paid, rest, _) = left_all(&w, "the note of every leg", &shard, &nth(&sg, k, note.clone(), 6_600));
    assert_eq!((end.s.rows, end.s.basis, end.s.held, paid, rest), ([ShardRow::EMPTY; 4], 0, 100 * KAS1 - note.asked, note.asked, None));
    // The lie the second test is for: the long leaves alone from the poor account, and is asked for.
    let rows = poor.s.c_trace(&a, 1).unwrap();
    let lone = CSaid { amount: 0, skip: 1, lc: kpf_protocol::covered::chain_of_one(&legs[0]), rows };
    let asked = Left { sid: shard.s.sid, legs: vec![legs[0]], key: xk(&w.w), asked: 188 * KAS1 / 10 };
    let outs = Outs::with_notes(&[(Out { key: false, to: poor.s.c_hash(5 * KAS1, &lone.rows[1].h), value: 5 * KAS1 }, false), (Out { key: false, to: asked.hash(), value: NOTE_MIN }, true)]).unwrap();
    let lie = Stated { kind: KIND_C_LEAVE, arg: lone.arg(), outs, ..live.s };
    assert_eq!(poor.s.c_wrong_point(KIND_C_LEAVE, &lone, &lie.claimer, &lie.outs, poor.value as i64, T0, &a), Some(0));
    end_answered(&w, "a leg that gained leaves alone from coins that do not hold the other", &poor, &acct_stated(&w, &poor, &lie, &lone.said()), &lone, false);

    // (c) The short, which lost 5, leaves alone: the account pays the 5, and the note holds them.
    let (live, s, o, sg) = c_applied(&w, "a leg that lost leaves alone", &acct, KIND_C_LEAVE, 0, 2, &a, &w.w);
    let (after, note) = (o.after.clone().unwrap(), o.note.clone().unwrap());
    assert_eq!((note.asked, after.a_margin, after.legs.clone(), sg.tx.outputs[0].value), (-5 * KAS1, 95 * KAS1, vec![legs[0]], 95 * KAS1 as u64));
    let (k, v) = out_of(&sg, &hash_spk(&w.lib, &note.hash()));
    assert_eq!(v, (NOTE_MIN + 5 * KAS1) as u64);
    end_answered(&w, "a leg that lost leaves alone", &acct, &live, &s, true);
    let (b, next, rest) = left_apply(&w.lib, &shard, &nth(&sg, k, note.clone(), 6_600), &left_script, w.payer(&w.w, &f)).unwrap();
    let ap = signed("the note of a leg that lost applied", &b, &w.keys);
    assert_eq!((next.rows, next.basis, next.held, rest, ap.tx.outputs[0].value), (base.s.booked(&legs[0]).unwrap().rows, base.s.booked(&legs[0]).unwrap().basis, 105 * KAS1, None, (105 * KAS1 + SHARD_CARRIER) as u64));

    // (d) Every leg leaves at a net loss (9 and 11: 10 and 5 lost): the account pays 15, the user has 85.
    let down = [P10 * 9 / 10, P10 * 11 / 10];
    let (_, _, o, sg) = c_applied(&w, "every leg leaves at a loss", &acct, KIND_C_LEAVE, 0, ALL, &down, &w.w);
    let note = o.note.clone().unwrap();
    assert_eq!((o.paid, note.asked, out_of(&sg, &hash_spk(&w.lib, &note.hash())).1), (85 * KAS1, -15 * KAS1, (NOTE_MIN + 15 * KAS1) as u64));
    // (e) From 10 KAS: the coins pay as far as they go, a note's least staying with the user.
    let thin = cov(&w, &shard, &legs, 10 * KAS1);
    let (_, _, o, _) = c_applied(&w, "every leg leaves from coins that do not pay", &thin, KIND_C_LEAVE, 0, ALL, &down, &w.w);
    assert_eq!((o.paid, o.note.clone().unwrap().asked), (NOTE_MIN, -(10 * KAS1 - NOTE_MIN)));
    // A leg that lost more than the coins does not leave alone.
    assert!(c_state(&w.lib, &cov(&w, &shard, &legs, 4 * KAS1), &w.script("KpfTChange"), KIND_C_LEAVE, 0, 2, &a, T0, xk(&w.w), w.payer(&w.w, &f)).is_err());
    // Nobody but the user states a leaving.
    assert!(c_state(&w.lib, &acct, &w.script("KpfTChange"), KIND_C_LEAVE, 0, ALL, &a, T0, xk(&w.c), w.payer(&w.c, &f)).is_err());
}

/// A step of the user's liquidation: below its maintenance on the net; the leg named lost and the coins pay what
/// it lost and the fee, and it leaves; else every leg leaves. Not of a user that stands; not naming a leg that
/// gained while another lost.
#[test]
fn a_step_of_a_covered_accounts_liquidation_takes_a_leg_that_lost_or_every_leg() {
    let w = world();
    let base = bare(&w);
    let f = [fund(xk(&w.f), 33)];
    let liq = w.terms().liq_fee_bps;
    // Long 10 of market 0 from 10, long 2 of market 1 from 10; 9.9 KAS. At 9.5 and 10: 5 lost; worth 95 and 20;
    // 4.5 % of 115 is 5.175, and the account stands at 4.9.
    let legs = vec![fleg(&w, 0, 10, LONG, P10), fleg(&w, 1, 2, LONG, P10)];
    let shard = OLive { s: base.s.booked(&legs[0]).unwrap().booked(&legs[1]).unwrap(), ..base.clone() };
    let a = [P10 * 95 / 100, P10];
    let acct = cov(&w, &shard, &legs, 99 * KAS1 / 10);
    let fee = 95 * KAS1 / 10_000 * liq;
    let (live, s, o, sg) = c_applied(&w, "a step: the leg that lost leaves", &acct, KIND_C_STEP, 0, 1, &a, &w.f);
    let (after, note) = (o.after.clone().unwrap(), o.note.clone().unwrap());
    assert_eq!((s.amount, o.fee, note.asked, after.a_margin, after.legs.clone()), (fee, fee, -5 * KAS1, 99 * KAS1 / 10 - 5 * KAS1 - fee, vec![legs[1]]));
    assert_eq!((sg.tx.outputs[0].value, out_of(&sg, &hash_spk(&w.lib, &note.hash())).1), ((99 * KAS1 / 10 - 5 * KAS1 - fee) as u64, (NOTE_MIN + 5 * KAS1) as u64));
    end_answered(&w, "a step", &acct, &live, &s, true);
    // The fee stated a sompi more: wrong.
    let more = CSaid { amount: fee + 1, ..s.clone() };
    let lie = Stated { arg: more.arg(), ..live.s };
    end_answered(&w, "a step with another fee", &acct, &acct_stated(&w, &acct, &lie, &more.said()), &more, false);
    // Naming the leg that did not lose, while the other lost: no step.
    assert!(c_state(&w.lib, &acct, &w.script("KpfTChange"), KIND_C_STEP, 0, 2, &a, T0, xk(&w.f), w.payer(&w.f, &f)).is_err());
    // A user that stands is not stepped (10 KAS more).
    assert!(c_state(&w.lib, &cov(&w, &shard, &legs, 199 * KAS1 / 10), &w.script("KpfTChange"), KIND_C_STEP, 0, 1, &a, T0, xk(&w.f), w.payer(&w.f, &f)).is_err());

    // The coins do not pay what the leg named lost and its fee (5.3 KAS; at 9.5 and 9: 5 and 2 lost): every leg
    // leaves. The coins pay 5.1 of the 7 (a note's least stays), and nothing is left for a fee.
    let low = [P10 * 95 / 100, P10 * 9 / 10];
    let small = cov(&w, &shard, &legs, 53 * KAS1 / 10);
    let (live, s, o, sg) = c_applied(&w, "a step: every leg leaves", &small, KIND_C_STEP, 0, 1, &low, &w.f);
    let note = o.note.clone().unwrap();
    assert_eq!((o.after.clone(), o.fee, o.paid, note.asked, note.legs.clone()), (None, 0, NOTE_MIN, -(53 * KAS1 / 10 - NOTE_MIN), legs.clone()));
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &note.hash())).1, (53 * KAS1 / 10) as u64);
    end_answered(&w, "a step in which every leg leaves", &small, &live, &s, true);

    // An account of one leg: the step takes it and leaves an account of no leg, with the coins that are left. 9
    // KAS, long 10 from 10 at 9.5: 5 lost, and the fee of 95.
    let one = vec![legs[0]];
    let single = cov(&w, &OLive { s: base.s.booked(&one[0]).unwrap(), ..base.clone() }, &one, 9 * KAS1);
    let (_, s, o, _) = c_applied(&w, "a step on an account of one leg", &single, KIND_C_STEP, 0, 1, &a[..1], &w.f);
    let after = o.after.clone().unwrap();
    assert_eq!((after.legs.len(), after.a_min_counter, after.a_margin, s.amount, o.note.clone().unwrap().asked), (0, 0, 9 * KAS1 - 5 * KAS1 - fee, fee, -5 * KAS1));
    assert_eq!(after.b_min_counter, 0, "a step writes no time");
}

/// The legs are counted and the account says a time (section 14.2). A take and a leaving write their own time and
/// twice the longest a statement lives, and a statement of the user's before the time the account says has a
/// wrong end; a step is held to no time. An account of no leg takes a take, stated with no row.
#[test]
fn a_covered_account_counts_its_legs_and_says_the_time_before_which_its_user_states_nothing() {
    let w = world();
    let base = bare(&w);
    let legs = two_legs(&w);
    let a = [P10, P10];
    let f = [fund(xk(&w.w), 33)];
    let acct = cov(&w, &base, &legs, 100 * KAS1);
    let life = acct.s.t.late_daa + 2 * acct.s.dispute_daa;
    // A take: the account it leaves says the time.
    let (_, _, o, _) = c_applied(&w, "a take that writes its time", &acct, KIND_C_TAKE, KAS1, 0, &a, &w.w);
    let after = o.after.clone().unwrap();
    assert_eq!((after.a_min_counter, after.b_min_counter), (2, T0 + 2 * life));
    // Before that time its user states nothing: not a take, not a leaving; a step is held to no time.
    let held = OLive { s: after.clone(), value: after.a_margin as u64, ..acct.clone() };
    for (kind, amount, skip) in [(KIND_C_TAKE, KAS1, 0), (KIND_C_LEAVE, 0, 1), (KIND_C_LEAVE, 0, ALL)] {
        assert!(c_state(&w.lib, &held, &w.script("KpfTChange"), kind, amount, skip, &a, T0 + 2 * life - 1, xk(&w.w), w.payer(&w.w, &f)).is_err(), "kind {kind} before the account's time");
        assert!(c_state(&w.lib, &held, &w.script("KpfTChange"), kind, amount, skip, &a, T0 + 2 * life, xk(&w.w), w.payer(&w.w, &f)).is_ok(), "kind {kind} at the account's time");
    }
    // Stated all the same before the time: its end is answered wrong.
    let early = Account { b_min_counter: T0 + 1, ..acct.s.clone() };
    let early = OLive { s: early, ..acct.clone() };
    let (st, s, _) = Account { b_min_counter: 0, ..early.s.clone() }.c_state(KIND_C_TAKE, KAS1, 0, &a, T0, early.value as i64, xk(&w.w)).unwrap();
    let lie = Stated { position: early.s.hash(), ..st };
    assert_eq!(early.s.c_end_judged(KIND_C_TAKE, &s, &lie.claimer, &lie.outs, early.value as i64, T0), Some(false));
    end_answered(&w, "a take before the account's time", &early, &acct_stated(&w, &early, &lie, &s.said()), &s, false);
    // A step of an account that says a time (its user below its maintenance) is the rule's.
    let low = [P10 * 9 / 10, P10 * 11 / 10];
    let poor = cov(&w, &base, &legs, 6 * KAS1);
    let poor = OLive { s: Account { b_min_counter: T0 + 1_000, ..poor.s.clone() }, ..poor };
    let (_, _, o, _) = c_applied(&w, "a step before the account's time", &poor, KIND_C_STEP, 0, 1, &low, &w.f);
    assert!(o.after.is_none(), "every leg left: the coins do not pay what both lost");
    // A leaving of one leg counts one less, and writes the time.
    let (_, _, o, _) = c_applied(&w, "a leaving that counts a leg less", &acct, KIND_C_LEAVE, 0, 2, &a, &w.w);
    let after = o.after.clone().unwrap();
    assert_eq!((after.legs.len(), after.a_min_counter, after.b_min_counter), (1, 1, T0 + 2 * life));
    // The account's only leg leaves alone: an account of no leg is left, and it takes a take stated with no row.
    let one = cov(&w, &base, &legs[..1], 50 * KAS1);
    let (_, _, o, _) = c_applied(&w, "the only leg leaves alone", &one, KIND_C_LEAVE, 0, 1, &a[..1], &w.w);
    let none = o.after.clone().unwrap();
    assert_eq!((none.legs.len(), none.a_min_counter, none.a_margin), (0, 0, 50 * KAS1));
    let empty = OLive { s: Account { b_min_counter: 0, ..none.clone() }, value: 50 * KAS1 as u64, ..one.clone() };
    let (live, s, o, sg) = c_applied(&w, "a take from an account of no leg", &empty, KIND_C_TAKE, 50 * KAS1 - NOTE_MIN, 0, &[], &w.w);
    assert_eq!((s.rows.len(), o.after.clone().unwrap().a_margin, sg.tx.outputs[0].value), (0, NOTE_MIN, NOTE_MIN as u64));
    end_answered(&w, "a take from an account of no leg", &empty, &live, &s, true);
    assert!(c_state(&w.lib, &empty, &w.script("KpfTChange"), KIND_C_TAKE, 50 * KAS1 - NOTE_MIN + 1, 0, &[], T0, xk(&w.w), w.payer(&w.w, &f)).is_err(), "a note's least stays");
    assert!(c_state(&w.lib, &empty, &w.script("KpfTChange"), KIND_C_LEAVE, 0, ALL, &[], T0, xk(&w.w), w.payer(&w.w, &f)).is_err(), "no leaving of no leg");
    // An account whose count is not its legs', or of seven legs, is none of this form.
    assert!(Account { a_min_counter: 1, ..acct.s.clone() }.c_check().is_err());
    let seven: Vec<XLeg> = (0..7).map(|_| legs[0]).collect();
    assert!(Account { legs: seven, a_min_counter: 7, ..acct.s.clone() }.c_check().is_err());
}

/// The rows and the prices of a statement on a covered account are answered a point at a time; and a point nobody
/// answers costs the claimer its bond.
#[test]
fn the_rows_and_prices_of_a_covered_accounts_statement_are_answered_and_a_point_nobody_answers_costs_the_claimer() {
    let w = world();
    let legs = two_legs(&w);
    let base = bare(&w);
    let shard = OLive { s: base.s.booked(&legs[0]).unwrap().booked(&legs[1]).unwrap(), ..base.clone() };
    let a = [P10 * 12 / 10, P10 * 11 / 10];
    let acct = cov(&w, &shard, &legs, 100 * KAS1);
    let f = [fund(xk(&w.w), 36)];
    let (live, s, _, _) = c_applied(&w, "a leaving", &acct, KIND_C_LEAVE, 0, 1, &a, &w.w);
    let row_script = w.script("KpfTCRow");
    let chains = [[0u8; 32], s.rows[0].g];
    // Each row, disputed, is answered right; the other verdict is refused.
    for i in 1..=2i64 {
        let q = w.pointed(&live, 2 * i - 1);
        let (b, right) = c_row(&w.lib, &q, &s, 2 * i - 1, &chains[i as usize - 1], &legs[i as usize - 1], &row_script, xk(&w.f), w.payer(&w.w, &f), None).unwrap();
        assert!(right);
        signed(&format!("row {i} answered right"), &b, &w.keys);
        let (b, _) = c_row(&w.lib, &q, &s, 2 * i - 1, &chains[i as usize - 1], &legs[i as usize - 1], &row_script, xk(&w.f), w.payer(&w.w, &f), Some(false)).unwrap();
        assert!(!accepted(&b, &w.keys), "row {i} answered wrong");
    }
    // Rows that are not the rule's: the leg that leaves counted as staying; the staying leg's loss left out; a
    // chain of staying legs that passes the leg by; the leg that leaves alone named by another chain.
    let good = &s.rows;
    let mut lies: Vec<(&str, CSaid, i64)> = vec![];
    let mut r = good.clone();
    r[0].wb = r[0].wa;
    lies.push(("the leg that leaves counted with those that stay", CSaid { rows: r, ..s.clone() }, 1));
    let mut r = good.clone();
    r[1].ea = 0;
    lies.push(("what the leg that stays lost left out", CSaid { rows: r, ..s.clone() }, 3));
    let mut r = good.clone();
    r[1].h = r[0].h;
    lies.push(("the leg that stays passed by", CSaid { rows: r, ..s.clone() }, 3));
    lies.push(("the leg that leaves named by another chain", CSaid { lc: kpf_protocol::covered::chain_of_one(&legs[1]), ..s.clone() }, 1));
    let mut r = good.clone();
    r[0].eb += 1;
    r[1].eb += 1;
    lies.push(("a sompi more gained by the leg that leaves", CSaid { rows: r, ..s.clone() }, 1));
    for (what, lie, point) in lies {
        let st = Stated { arg: lie.arg(), ..live.s };
        // (Whatever its end: the row is the point.)
        let wrong = acct.s.rows_wrong_point_by(&lie.rows, &a, &|i, l, before, row| kpf_protocol::covered::c_row_judged(&before.g, l, before, row, i as i64 + 1, &lie));
        assert_eq!(wrong, Some(point), "{what}");
        let q = w.pointed(&acct_stated(&w, &acct, &st, &lie.said()), point);
        let i = ((point + 1) / 2) as usize;
        let (b, right) = c_row(&w.lib, &q, &lie, point, &chains[i - 1], &legs[i - 1], &row_script, xk(&w.f), w.payer(&w.w, &f), None).unwrap();
        assert!(!right, "{what}");
        signed(&format!("{what}: answered wrong"), &b, &w.keys);
        let (b, _) = c_row(&w.lib, &q, &lie, point, &chains[i - 1], &legs[i - 1], &row_script, xk(&w.f), w.payer(&w.w, &f), Some(true)).unwrap();
        assert!(!accepted(&b, &w.keys), "{what}: answered right");
    }
    // A price: within the tolerance of the committee's it is right, beyond it wrong.
    let cite1 = w.script("KpfTCite");
    let q = w.pointed(&live, 2);
    let near = w.cited(0, a[0] * 1_004 / 1_000);
    let (b, right) = c_price(&w.lib, &q, &acct.s, &s, &w.script("KpfTStepPrice"), (&cite1, &near), xk(&w.f), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    signed("a price answered right", &b, &w.keys);
    let far = w.cited(0, a[0] * 102 / 100);
    let (b, right) = c_price(&w.lib, &q, &acct.s, &s, &w.script("KpfTStepPrice"), (&cite1, &far), xk(&w.f), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    signed("a price answered wrong", &b, &w.keys);
    // A point nobody answers: the account again, and the claimer's bond is lost, half to the disputer.
    let q = w.pointed(&live, 4);
    let b = point_lapse(&w.lib, &OLive { daa: 6_520, ..q.clone() }, &w.script("KpfTPoint"), w.payer(&w.w, &f)).unwrap();
    let sg = signed("a point of a leaving that nobody answered", &b, &w.keys);
    let bond = live.s.bond;
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (acct.value, &hash_spk(&w.lib, &acct.s.hash())));
    assert!(pays(&sg, &xk(&w.w), bond + bond / 2));
}

struct Xs(u64);
impl Xs {
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

/// On random accounts of one to four legs, at random prices, with random coins: whatever the model lets be stated
/// is answered right at the engine, and the same statement with another amount, wrong.
#[test]
fn on_random_covered_accounts_the_end_is_the_models() {
    let w = world();
    let base = bare(&w);
    let mut g = Xs(0x9e37_79b9_7f4a_7c15);
    let f = [fund(xk(&w.w), 33)];
    let (mut stated, mut refused) = ([0usize; 3], 0usize);
    for round in 0..70 {
        let n = g.range(1, 4) as usize;
        let legs: Vec<XLeg> = (0..n).map(|_| fleg(&w, g.range(0, 3) as usize, g.range(50, 150), if g.range(0, 1) == 0 { LONG } else { SHORT }, P10 * g.range(80, 120) / 100)).collect();
        let a: Vec<i64> = legs.iter().map(|l| l.entry * g.range(90, 110) / 100).collect();
        let worth: i64 = legs.iter().zip(&a).map(|(l, p)| l.size / w.markets[0].p.scale * p).sum();
        let kind = [KIND_C_TAKE, KIND_C_LEAVE, KIND_C_STEP][round % 3];
        // (Coins enough to take of, about what a leaving turns on, and too few to stand.)
        let pct = match kind {
            KIND_C_TAKE => g.range(20, 60),
            KIND_C_LEAVE => g.range(1, 25),
            _ => g.range(1, 6),
        };
        // (No fewer than 3 KAS: a bond of less is paid back in outputs the builders fill up to what a node relays.)
        let coins = (worth * pct / 100).max(3 * KAS1);
        let acct = cov(&w, &base, &legs, coins);
        let loser = (0..n).find(|i| (legs[*i].dir == LONG) == (a[*i] < legs[*i].entry) && a[*i] != legs[*i].entry).map(|i| i as i64 + 1);
        let skip = match kind {
            KIND_C_TAKE => 0,
            KIND_C_LEAVE if g.range(0, 3) == 0 => ALL,
            KIND_C_STEP => loser.unwrap_or(1),
            _ => g.range(1, n as i64),
        };
        let amount = if kind == KIND_C_TAKE { (coins * g.range(1, 40) / 100).max(1) } else { 0 };
        let who = if kind == KIND_C_STEP { &w.f } else { &w.w };
        let Ok((_, st, s, _)) = c_state(&w.lib, &acct, &w.script("KpfTChange"), kind, amount, skip, &a, T0, xk(who), w.payer(who, &f)) else {
            refused += 1;
            continue;
        };
        stated[(kind - KIND_C_TAKE) as usize] += 1;
        let what = format!("round {round}, kind {kind}, {n} legs, skip {skip}");
        let live = acct_stated(&w, &acct, &st, &s.said());
        end_answered(&w, &what, &acct, &live, &s, true);
        // The same with a sompi more stated as the amount: of a take, more than its outputs say; of the others,
        // not the fee.
        let more = CSaid { amount: s.amount + 1, ..s.clone() };
        let lie = Stated { arg: more.arg(), ..st };
        assert_eq!(acct.s.c_end_judged(kind, &more, &lie.claimer, &lie.outs, acct.value as i64, T0), Some(false), "{what}");
        end_answered(&w, &format!("{what}, a sompi more"), &acct, &acct_stated(&w, &acct, &lie, &more.said()), &more, false);
    }
    println!("random covered accounts: {} takes, {} leavings, {} steps stated and answered; {refused} the rule did not allow", stated[0], stated[1], stated[2]);
    assert!(stated.iter().all(|k| *k >= 5), "{stated:?}");
}


/// The life of a statement (section 14.1). A right answer is anyone's, and of the disputer's bond a quarter is the
/// claimer's and a quarter whoever answered's. The statement stands again and keeps its lateness: written at or
/// after the last time to dispute (its time, its lateness and a dispute period) it is applied at once. A dispute
/// that landed after that time is dismissed by anyone, and lapses nothing.
#[test]
fn a_statement_lives_no_longer_than_its_lateness_and_two_dispute_periods() {
    let w = world();
    let base = bare(&w);
    let legs = two_legs(&w);
    let a = [P10 * 12 / 10, P10 * 11 / 10];
    let acct = cov(&w, &OLive { s: base.s.booked(&legs[0]).unwrap().booked(&legs[1]).unwrap(), ..base.clone() }, &legs, 100 * KAS1);
    let f = [fund(xk(&w.f), 36)];
    let (b, st, s, o) = c_state(&w.lib, &acct, &w.script("KpfTChange"), KIND_C_LEAVE, 0, 1, &a, T0, xk(&w.w), w.payer(&w.w, &[fund(xk(&w.w), 33)])).unwrap();
    let live = nth(&signed("a leaving stated", &b, &w.keys), 0, st, 6_510);
    let last = st.last_dispute();
    assert_eq!((last, st.again().late, st.again().lateness(), st.again().last_dispute(), st.again().again()), (T0 + acct.s.t.late_daa + 50, kpf_protocol::thin::LATE_AGAIN + acct.s.t.late_daa, acct.s.t.late_daa, last, st.again()));
    // Its end is disputed in time, and answered right by a third party (the funder): a quarter of the disputer's
    // bond each to the claimer and to it, half to the copy.
    let q = w.pointed(&live, 0);
    assert!((q.daa as i64) <= last);
    let (b, right) = c_end(&w.lib, &q, &acct.s, &s, &w.script("KpfTCEnd"), xk(&w.f), w.payer(&w.f, &f), None).unwrap();
    assert!(right);
    let sg = signed("the end answered right by a third party", &b, &w.keys);
    let quarter = st.bond / 4;
    assert!(pays(&sg, &xk(&w.w), quarter) && pays(&sg, &xk(&w.f), quarter));
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key, sg.tx.outputs[1].value), (live.value, &hash_spk(&w.lib, &st.again().hash()), w.script("KpfTCEnd").value + (200_000 + st.bond - 2 * quarter) as u64));
    assert!(point_dismiss(&w.lib, &q, &w.script("KpfTPoint"), xk(&w.f), w.payer(&w.f, &f)).is_err(), "a dispute in time is not dismissed");
    // The statement that stands again, written at the last time to dispute: applied at once. Written before it:
    // after a dispute period.
    for (daa, age) in [(last as u64, 0u64), (last as u64 - 1, 50)] {
        let again = nth(&sg, 0, st.again(), daa);
        let b = stated_settle(&w.lib, &again, &w.script("KpfTChangeSettle"), w.payer(&w.f, &f)).unwrap();
        assert_eq!(b.tx.inputs[0].sequence, age, "written at {daa}");
        let ap = signed(&format!("the statement that stands again, written at {daa}, applied"), &b, &w.keys);
        assert_eq!(&ap.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &o.after.clone().unwrap().hash()));
    }
    // A dispute of the statement that stands again, landed after the last time: dismissed by anyone, a quarter of
    // its bond to the claimer and a quarter to who dismissed it; it does not lapse.
    // (Of the statement written before the last time: one written at or after it is disputed by nothing.)
    assert!(point_dispute(&w.lib, &nth(&sg, 0, st.again(), last as u64), &w.script("KpfTPoint"), xk(&w.c), 0, w.payer(&w.c, &[fund(xk(&w.c), 37)])).is_err());
    let again = nth(&sg, 0, st.again(), last as u64 - 1);
    let (b, late) = point_dispute(&w.lib, &again, &w.script("KpfTPoint"), xk(&w.c), 0, w.payer(&w.c, &[fund(xk(&w.c), 37)])).unwrap();
    let dsg = b.sign(&w.keys).expect("disputed");
    assert!(kpf_protocol::verify::verify(&dsg.tx, &dsg.entries).is_ok(), "a dispute cannot be held to a time when it is made");
    let lq = nth(&dsg, 0, late, last as u64 + 1);
    assert!(point_lapse(&w.lib, &lq, &w.script("KpfTPoint"), w.payer(&w.f, &f)).is_err(), "it lapses nothing");
    let b = point_dismiss(&w.lib, &lq, &w.script("KpfTPoint"), xk(&w.f), w.payer(&w.f, &f)).unwrap();
    let ds = signed("a dispute that landed after the last time, dismissed", &b, &w.keys);
    assert!(pays(&ds, &xk(&w.w), quarter) && pays(&ds, &xk(&w.f), quarter));
    assert_eq!((ds.tx.outputs[0].value, &ds.tx.outputs[0].script_public_key), (again.value, &hash_spk(&w.lib, &st.again().hash())), "the statement again, as it stood");
    // One that landed at the last time is in time: not dismissed; it lapses (at the claimer's cost, for this kind).
    let inq = nth(&dsg, 0, late, last as u64);
    assert!(point_dismiss(&w.lib, &inq, &w.script("KpfTPoint"), xk(&w.f), w.payer(&w.f, &f)).is_err());
    let b = point_lapse(&w.lib, &inq, &w.script("KpfTPoint"), w.payer(&w.f, &f)).unwrap();
    signed("a dispute that landed at the last time lapses", &b, &w.keys);
}

// ---------------------------------------------------------------- section 14.3: an entry stated with the shard

use kpf_protocol::covered::{CEnterSaid, Marked, KIND_C_ENTER};

/// What the id of the shard of these tests is the hash of beside its committee, its terms and its periods.
const SEED: [u8; 32] = [0x5e; 32];

/// A shard of 100 KAS with no leg, whose id is what an entry holds it to: the hash of a seed, of the committee of
/// the markets, and of the terms and periods of the accounts under it.
fn seeded(w: &World) -> OLive<Shard> {
    let x = bare(w);
    let sid = kpf_protocol::reserve::shard_sid(&SEED, &w.markets[0].committee_id, &x.s.terms(), &x.s.moving()[93..120]);
    OLive { s: Shard { sid, ..x.s }, ..x }
}

/// An entry is stated in a transaction that spends the shard: its books have the leg from there. The statement
/// stands on the account marked; applied, the account has the leg, counts it, and says its time. It is disputed a
/// point at a time as a covered account's statements are.
#[test]
fn an_entry_is_stated_with_the_shard_and_the_books_have_the_leg_from_there() {
    let w = world();
    let shard = seeded(&w);
    let acct = cov(&w, &shard, &[], 20 * KAS1);
    let leg = fleg(&w, 0, 10, LONG, P10);
    let f = [fund(xk(&w.w), 33)];
    let enter = w.script("KpfTEnterS");
    let life = acct.s.t.late_daa + 2 * acct.s.dispute_daa;
    // An account of no leg and 20 KAS; a long of 10 units from 10 enters, with 5 KAS more.
    let (b, st, said, marked, after, next) = c_enter_state(&w.lib, &acct, &shard, &enter, SEED, leg, 5 * KAS1, &[], T0, w.payer(&w.w, &f)).unwrap();
    let sg = signed("an entry stated with the shard", &b, &w.keys);
    assert_eq!((st.kind, st.claimer, st.position, st.held, st.bond), (KIND_C_ENTER, xk(&w.w), marked.hash(), 25 * KAS1, 25 * KAS1 / 10));
    assert_eq!((marked.0.legs.clone(), marked.0.a_margin, marked.0.a_min_counter, marked.moving().len()), (vec![leg], 25 * KAS1, 1, 180));
    // (And the shard says its time: no draw before this entry is settled or ended.)
    assert_eq!((next, said.r), (Shard { expiry: T0 + life, ..shard.s.booked(&leg).unwrap() }, 0), "the books have the leg");
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), ((25 * KAS1 + st.bond) as u64, &hash_spk(&w.lib, &st.hash())));
    assert_eq!((sg.tx.outputs[2].value, &sg.tx.outputs[2].script_public_key), (shard.value, &hash_spk(&w.lib, &next.hash())));
    assert_eq!(Account::c_enter_wrong_point(&marked, &said, &st.claimer, &st.outs, st.held, T0, &[P10]), None);
    // Applied after its dispute period: the account with the leg, unmarked, counted, with its time.
    let live = nth(&sg, 0, st, 6_510);
    let b = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).unwrap();
    let ap = signed("the entry applied", &b, &w.keys);
    assert_eq!((after.legs.clone(), after.a_margin, after.a_min_counter, after.b_min_counter, after.moving().len()), (vec![leg], 25 * KAS1, 1, T0 + 2 * life, 171));
    assert_eq!((ap.tx.outputs[0].value, &ap.tx.outputs[0].script_public_key), (25 * KAS1 as u64, &hash_spk(&w.lib, &after.hash())));
    // The account it becomes takes a covered account's statements (after its time).
    let entered = nth(&ap, 0, after.clone(), 6_600);
    assert!(c_state(&w.lib, &entered, &w.script("KpfTChange"), KIND_C_LEAVE, 0, 1, &[P10], T0 + 2 * life, xk(&w.w), w.payer(&w.w, &f)).is_ok());
    assert!(c_state(&w.lib, &entered, &w.script("KpfTChange"), KIND_C_LEAVE, 0, 1, &[P10], T0 + 2 * life - 1, xk(&w.w), w.payer(&w.w, &f)).is_err());

    // Its points. The end and the row, right; the other verdict refused.
    let ff = [fund(xk(&w.f), 36)];
    let q = w.pointed(&live, 0);
    let (b, right) = c_enter_end(&w.lib, &q, &marked, &said, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(right);
    signed("the end of an entry answered right", &b, &w.keys);
    let (b, _) = c_enter_end(&w.lib, &q, &marked, &said, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.f, &ff), Some(false)).unwrap();
    assert!(!accepted(&b, &w.keys));
    let q = w.pointed(&live, 1);
    let (b, right) = c_enter_row(&w.lib, &q, &said, 1, &[0; 32], &leg, &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(right);
    signed("the row of the leg that enters answered right", &b, &w.keys);
    let (b, _) = c_enter_row(&w.lib, &q, &said, 1, &[0; 32], &leg, &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.f, &ff), Some(false)).unwrap();
    assert!(!accepted(&b, &w.keys));
    // A point beyond the trace is of nothing: right.
    let q = w.pointed(&live, 7);
    let (b, right) = c_enter_row(&w.lib, &q, &said, 7, &[0; 32], &leg, &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(right);
    signed("a point of an entry that is of nothing", &b, &w.keys);
    // The price of the leg, its own: within the tolerance of the committee price right, beyond it wrong.
    let cite1 = w.script("KpfTCite");
    let q = w.pointed(&live, 2);
    let near = w.cited(0, P10 * 1_004 / 1_000);
    let (b, right) = c_enter_price(&w.lib, &q, &marked, &said, &w.script("KpfTStepPrice"), (&cite1, &near), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(right);
    signed("the price of an entry answered right", &b, &w.keys);
    let far = w.cited(0, P10 * 102 / 100);
    let (b, right) = c_enter_price(&w.lib, &q, &marked, &said, &w.script("KpfTStepPrice"), (&cite1, &far), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(!right);
    let wrong = signed("the price of an entry answered wrong", &b, &w.keys);
    assert_eq!((wrong.tx.outputs[0].value, &wrong.tx.outputs[0].script_public_key), (25 * KAS1 as u64, &hash_spk(&w.lib, &marked.hash())), "the marked account again");
    // A price nobody answers ends the entry at the cost of nobody: the marked account again, both bonds back.
    let b = point_lapse(&w.lib, &q, &w.script("KpfTPoint"), w.payer(&w.f, &ff)).unwrap();
    let lapsed = signed("a price of an entry that nobody answered", &b, &w.keys);
    assert_eq!((lapsed.tx.outputs[0].value, &lapsed.tx.outputs[0].script_public_key), (25 * KAS1 as u64, &hash_spk(&w.lib, &marked.hash())));
    assert!(pays(&lapsed, &xk(&w.w), st.bond), "the bond of the claimer back, and of the disputer (the same key here)");

    // An entry that ended is undone by anyone, for the fee of a keeper out of the coins of the account: the
    // account without the leg, and the note that the leg did not enter; applied, the books are as they were.
    let m_live: OLive<Marked> = nth(&lapsed, 0, marked.clone(), 6_700);
    let (b, back, note) = enter_off(&w.lib, &m_live, &w.script("KpfTEnterOff"), w.payer(&w.f, &ff)).unwrap();
    let off = signed("an entry that ended, undone", &b, &w.keys);
    let fee = acct.s.t.keeper_fee_min;
    assert_eq!((back.legs.len(), back.a_min_counter, back.a_margin, back.b_min_counter, back.moving().len()), (0, 0, 25 * KAS1 - fee, 0, 171));
    assert_eq!((off.tx.outputs[0].value, &off.tx.outputs[0].script_public_key), ((25 * KAS1 - fee) as u64, &hash_spk(&w.lib, &back.hash())));
    assert_eq!((off.tx.outputs[2].value, &off.tx.outputs[2].script_public_key), (NOTE_MIN as u64, &hash_spk(&w.lib, &note.hash())));
    // (No output of the fee: what the transaction leaves is its funder's. Review 51, F83.)
    assert!(fee > 0 && !pays(&off, &xk(&w.f), fee));
    let booked = nth(&sg, 2, next, 6_511);
    let (b, books) = unentered_apply(&w.lib, &booked, &nth(&off, 2, note, 6_701), &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    signed("the note that the leg did not enter, applied", &b, &w.keys);
    assert_eq!((books.rows, books.basis, books.held, books.expiry), (shard.s.rows, shard.s.basis, shard.s.held, T0 + life));
}

/// What the stating of an entry refuses, and an entry of a user that does not stand; and a gain of a leg is margin
/// for the leg that enters.
#[test]
fn an_entry_is_refused_or_answered_wrong_and_a_gain_is_margin_for_it() {
    let w = world();
    let shard = seeded(&w);
    let leg = fleg(&w, 0, 10, LONG, P10);
    let f = [fund(xk(&w.w), 33)];
    let enter = w.script("KpfTEnterS");
    let life = w.terms().late_daa + 100;
    let other_committee = XLeg { committee: [0x99; 32], ..leg };
    // A statement made by hand: the marked account, the trace, the account it becomes.
    let by_hand = |acct: &OLive<Account>, m: Marked, prices: &[i64], shard: &OLive<Shard>, r: usize| {
        let l = *m.0.legs.last().unwrap();
        let rows = m.0.c_trace(prices, 0).unwrap();
        let said = CEnterSaid { add: m.0.a_margin - acct.s.a_margin, r: r as i64, leg: l, rows };
        let after = Account { b_min_counter: T0 + 2 * life, ..m.0.clone() };
        let held = acct.value as i64 + said.add;
        let st = Stated { position: m.hash(), window: 50, late: w.terms().late_daa, kind: KIND_C_ENTER, t0: T0, arg: said.arg(), outs: Outs::of(&[Out { key: false, to: after.hash(), value: held }]).unwrap(), bond: m.0.bond(), claimer: xk(&w.w), held };
        let mut next = shard.s;
        next.rows[r] = ShardRow { head: Head::of(&l), net: next.rows[r].net + if l.dir == LONG { -l.size } else { l.size }, gross: next.rows[r].gross + l.size };
        next.basis += if l.dir == LONG { -(l.size / l.scale * l.entry) } else { l.size / l.scale * l.entry };
        next.expiry = next.expiry.max(T0 + life);
        let b = c_enter_state_as(&w.lib, acct, shard, &enter, SEED, &st, &said, &next, w.payer(&w.w, &f));
        (b, st, said)
    };
    let mark = |a: &Account, l: XLeg| {
        let mut legs = a.legs.clone();
        legs.push(l);
        Marked(Account { legs, a_min_counter: a.a_min_counter + 1, ..a.clone() })
    };
    // The control: by hand, an entry the script takes.
    let acct = cov(&w, &shard, &[], 20 * KAS1);
    let (b, _, _) = by_hand(&acct, mark(&acct.s, leg), &[P10], &shard, 0);
    assert!(accepted(&b.unwrap(), &w.keys), "the control");
    // Refused where it is stated: a seventh leg; before the time of the account; a count below nothing; a called
    // shard; a row of another market, and a second row for a market that has one; a leg that is none.
    let six: Vec<XLeg> = (0..6).map(|_| leg).collect();
    let full = cov(&w, &shard, &six, 200 * KAS1);
    assert!(full.s.c_marked(&leg, 0, T0).is_none());
    let (b, _, _) = by_hand(&full, mark(&full.s, leg), &[P10; 7], &shard, 0);
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a seventh leg");
    let early = OLive { s: Account { b_min_counter: T0 + 1, ..acct.s.clone() }, ..acct.clone() };
    let (b, _, _) = by_hand(&early, mark(&early.s, leg), &[P10], &shard, 0);
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "before the time of the account");
    let below = OLive { s: Account { a_min_counter: -3, ..acct.s.clone() }, ..acct.clone() };
    let (b, _, _) = by_hand(&below, mark(&below.s, leg), &[P10], &shard, 0);
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a count below nothing");
    let called = OLive { s: Shard { called: 6_400, ..shard.s }, ..shard.clone() };
    let (b, _, _) = by_hand(&acct, mark(&acct.s, leg), &[P10], &called, 0);
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a called shard");
    let other = OLive { s: shard.s.booked(&fleg(&w, 1, 3, SHORT, P10)).unwrap(), ..shard.clone() };
    let (b, _, _) = by_hand(&acct, mark(&acct.s, leg), &[P10], &other, 0);
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a row of another market");
    let has = OLive { s: shard.s.booked(&leg).unwrap(), ..shard.clone() };
    let (b, _, _) = by_hand(&acct, mark(&acct.s, leg), &[P10], &has, 1);
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a second row for a market that has one");
    assert!(has.s.entered(&leg, 1, T0).is_err() && has.s.entered(&leg, 0, T0).is_ok() && other.s.entered(&leg, 0, T0).is_err() && other.s.entered(&leg, 1, T0).is_ok());
    // A leg of another committee than the shard's: the shard's id is the hash of its committee (review 51, F80).
    assert!(c_enter_state(&w.lib, &acct, &shard, &enter, SEED, other_committee, 0, &[], T0, w.payer(&w.w, &f)).is_err());
    let (b, _, _) = by_hand(&acct, mark(&acct.s, other_committee), &[P10], &shard, 0);
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a leg of another committee");
    // The shard written without its time, or with another: refused.
    let m0 = mark(&acct.s, leg);
    let (st0, said0, _, _) = acct.s.c_enter(leg, 0, 0, &[], T0, acct.value as i64).unwrap();
    assert_eq!(st0.position, m0.hash());
    for (what, time) in [("no time written", 0), ("a time before the entry is settled", T0 + life - 1), ("a later time than the entry asks", T0 + life + 1)] {
        let next = Shard { expiry: time, ..shard.s.entered(&leg, 0, T0).unwrap() };
        let b = c_enter_state_as(&w.lib, &acct, &shard, &enter, SEED, &st0, &said0, &next, w.payer(&w.w, &f));
        assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "{what}");
    }
    // An entry's time is a lateness after the last (review 57, F103): what it writes is a lateness or more after
    // what the shard says. A shard that says exactly that much less takes it; a block more, and it does not
    // (by the model, and at the engine with the shard put in).
    let late = shard.s.t.late_daa;
    let edge = OLive { s: Shard { expiry: T0 + life - late, ..shard.s }, ..shard.clone() };
    assert_eq!(edge.s.entered(&leg, 0, T0).unwrap().expiry, T0 + life);
    let b = c_enter_state_as(&w.lib, &acct, &edge, &enter, SEED, &st0, &said0, &edge.s.entered(&leg, 0, T0).unwrap(), w.payer(&w.w, &f)).unwrap();
    assert!(accepted(&b, &w.keys), "an entry a lateness after the last");
    let near = OLive { s: Shard { expiry: T0 + life - late + 1, ..shard.s }, ..shard.clone() };
    assert!(near.s.entered(&leg, 0, T0).is_err());
    let b = c_enter_state_as(&w.lib, &acct, &near, &enter, SEED, &st0, &said0, &Shard { expiry: T0 + life, ..edge.s.entered(&leg, 0, T0).unwrap() }, w.payer(&w.w, &f));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "an entry a block less than a lateness after the last");
    // (So a second entry at the same time is no entry: a lateness later it is.)
    let once = shard.s.entered(&leg, 0, T0).unwrap();
    assert!(once.entered(&leg, 0, T0).is_err() && once.entered(&leg, 0, T0 + late - 1).is_err() && once.entered(&leg, 0, T0 + late).is_ok());
    for (what, bad) in [("no direction", XLeg { dir: 5, ..leg }), ("no size", XLeg { size: 0, ..leg }), ("an entry beyond the limit of its market", XLeg { entry: leg.price_max + 1, ..leg })] {
        assert!(acct.s.c_marked(&bad, 0, T0).is_none(), "{what}");
        let m = mark(&acct.s, bad);
        let rows = vec![kpf_protocol::thin::Row { a: bad.entry, ..kpf_protocol::thin::Row::ZERO }];
        let said = CEnterSaid { add: 0, r: 0, leg: bad, rows };
        let st = Stated { position: m.hash(), window: 50, late: w.terms().late_daa, kind: KIND_C_ENTER, t0: T0, arg: said.arg(), outs: Outs::of(&[Out { key: false, to: m.0.hash(), value: acct.value as i64 }]).unwrap(), bond: m.0.bond(), claimer: xk(&w.w), held: acct.value as i64 };
        let b = c_enter_state_as(&w.lib, &acct, &shard, &enter, SEED, &st, &said, &shard.s, w.payer(&w.w, &f));
        assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "{what}");
    }

    // A user that does not stand with the leg (3 KAS for a leg worth 100): the model states no such entry; stated
    // all the same, its end is answered wrong, and the marked account is left to be undone.
    let poor = cov(&w, &shard, &[], 3 * KAS1);
    assert!(poor.s.c_enter(leg, 0, 0, &[], T0, poor.value as i64).is_err());
    let m = mark(&poor.s, leg);
    let (b, st, said) = by_hand(&poor, m.clone(), &[P10], &shard, 0);
    let sg = signed("an entry of a user that does not stand, stated", &b.unwrap(), &w.keys);
    assert_eq!(Account::c_enter_wrong_point(&m, &said, &st.claimer, &st.outs, st.held, T0, &[P10]), Some(0));
    let ff = [fund(xk(&w.f), 36)];
    let q = w.pointed(&nth(&sg, 0, st, 6_510), 0);
    let (b, right) = c_enter_end(&w.lib, &q, &m, &said, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(!right);
    let ended = signed("its end answered wrong", &b, &w.keys);
    assert_eq!(&ended.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &m.hash()));
    let (b, _) = c_enter_end(&w.lib, &q, &m, &said, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.f, &ff), Some(true)).unwrap();
    assert!(!accepted(&b, &w.keys));
    // The last row of another leg than the one said: its row is answered wrong.
    let acct2 = cov(&w, &shard, &[], 20 * KAS1);
    let m2 = mark(&acct2.s, leg);
    let (_, st2, said2) = by_hand(&acct2, m2.clone(), &[P10], &shard, 0);
    let twin = XLeg { size: leg.size + 1, ..leg };
    let lie = CEnterSaid { leg: twin, ..said2.clone() };
    let lst = Stated { arg: lie.arg(), ..st2 };
    assert_eq!(Account::c_enter_wrong_point(&m2, &lie, &lst.claimer, &lst.outs, lst.held, T0, &[P10]), Some(1));

    // A gain of a leg is margin for the leg that enters: 6 KAS and a long of 10 units from 10. At 10 a second leg
    // of 100 KAS of worth does not enter on those coins; at 12 (20 gained) it does.
    let held = cov(&w, &shard, &[leg], 6 * KAS1);
    let second = fleg(&w, 1, 10, SHORT, P10);
    assert!(held.s.c_enter(second, 0, 1, &[P10], T0, held.value as i64).is_err());
    let (st, said, m, _) = held.s.c_enter(second, 0, 1, &[P10 * 12 / 10], T0, held.value as i64).unwrap();
    let booked = OLive { s: shard.s.booked(&leg).unwrap(), ..shard.clone() };
    let b = c_enter_state_as(&w.lib, &held, &booked, &enter, SEED, &st, &said, &booked.s.entered(&second, 1, T0).unwrap(), w.payer(&w.w, &f)).unwrap();
    let sg = signed("a second leg enters on what the first gained", &b, &w.keys);
    let q = w.pointed(&nth(&sg, 0, st, 6_510), 0);
    let (b, right) = c_enter_end(&w.lib, &q, &m, &said, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(right);
    signed("its end answered right", &b, &w.keys);
}

// ---------------------------------------------------------------- sections 14.4 and 14.5: a shard is not held under draws, and a draw waits for entries

use kpf_protocol::thin_build::Pending;

/// A draw writes, in the shard it leaves, a time before which no draw is stated again: its own time and twice the
/// longest a statement lives. A draw stated before the time the shard says has a wrong end.
#[test]
fn a_draw_writes_a_time_before_which_no_draw_is_stated_again() {
    let w = world();
    let shard = w.shard();
    let s = shard.s;
    let life = s.t.late_daa + 2 * s.dispute_daa;
    let f = [fund(xk(&w.c), 30)];
    let (st, d) = s.draw(KAS1, PRICES, T0, shard.value as i64).unwrap();
    let after = s.drawn(KAS1, T0);
    assert_eq!((s.expiry, after.expiry, after.held), (0, T0 + 2 * life, s.held - KAS1));
    let live = w.stated(&shard, &st, &d.said());
    let b = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.c, &f)).unwrap();
    let sg = signed("a draw applied: the shard says its time", &b, &w.keys);
    assert_eq!(&sg.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &after.hash()));
    // The next draw: not before that time.
    let held = nth(&sg, 0, after, 6_600);
    assert!(after.draw(KAS1, PRICES, T0 + 2 * life - 1, held.value as i64).is_err());
    let (st2, d2) = after.draw(KAS1, PRICES, T0 + 2 * life, held.value as i64).unwrap();
    assert_eq!(after.draw_wrong_point(&d2, &st2.claimer, &st2.outs, held.value as i64, T0 + 2 * life, &PRICES), None);
    // Stated all the same before it (the same draw, said of the time T0 again): its end is answered wrong.
    let again = Shard { expiry: 0, ..after };
    let (st0, d0) = again.draw(KAS1, PRICES, T0, held.value as i64).unwrap();
    let lie = Stated { position: after.hash(), ..st0 };
    assert_eq!(after.draw_end_judged(&d0, &lie.claimer, T0), Some(false));
    let q = w.pointed(&w.stated(&held, &lie, &d0.said()), 0);
    let ff = [fund(xk(&w.f), 36)];
    let (b, right) = shard_end(&w.lib, &q, &after, &d0, &w.script("KpfTShardEnd"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(!right);
    signed("a draw before the time the shard says, answered wrong", &b, &w.keys);
    let (b, _) = shard_end(&w.lib, &q, &after, &d0, &w.script("KpfTShardEnd"), xk(&w.f), w.payer(&w.f, &ff), Some(true)).unwrap();
    assert!(!accepted(&b, &w.keys));
}

/// An entry is in the books of the shard from its statement and may yet not enter. It writes the time of the
/// shard: no draw is stated before the entry is settled or ended. An entry that ended and is not undone ends a
/// draw where it is shown: the marked account it left, or the note that the leg did not enter.
#[test]
fn no_draw_is_stated_before_an_entry_is_settled_and_one_that_ended_ends_a_draw() {
    let w = world();
    let base = seeded(&w);
    let acct = cov(&w, &base, &[], 20 * KAS1);
    let leg = fleg(&w, 0, 10, LONG, P10);
    let f = [fund(xk(&w.w), 33)];
    let ff = [fund(xk(&w.f), 36)];
    let life = acct.s.t.late_daa + 2 * acct.s.dispute_daa;
    let (b, st, _said, marked, _, next) = c_enter_state(&w.lib, &acct, &base, &w.script("KpfTEnterS"), SEED, leg, 0, &[], T0, w.payer(&w.w, &f)).unwrap();
    let sg = signed("an entry stated", &b, &w.keys);
    let entry = nth(&sg, 0, st, 6_510);
    let shard = nth(&sg, 2, next, 6_510);
    // At 9 the leg lost 10 KAS, which the books read as gained: a draw on that is of what is owed only if the leg
    // enters. Before the time the entry wrote, no draw is stated; at it, the entry is settled or ended.
    let at9 = [P10 * 9 / 10, 0, 0, 0];
    assert_eq!((next.equity(&at9), base.s.equity(&at9), next.expiry), (Some(110 * KAS1), Some(100 * KAS1), T0 + life));
    assert!(next.draw(KAS1, at9, T0 + life - 1, shard.value as i64).is_err());
    let t1 = T0 + life;
    let (dst, d) = next.draw(next.draw_max(&at9), at9, t1, shard.value as i64).unwrap();
    assert_eq!(d.amount, 100 * KAS1, "all it holds, on books that have the leg");
    // The entry ended (its price was answered by nobody) and is not undone: the marked account ends the draw.
    let q = w.pointed(&entry, 2);
    let b = point_lapse(&w.lib, &q, &w.script("KpfTPoint"), w.payer(&w.f, &ff)).unwrap();
    let lapsed = signed("the price of the entry lapses", &b, &w.keys);
    let m_live: OLive<Marked> = nth(&lapsed, 0, marked.clone(), 6_700);
    // (The draw, landed a block after its time.)
    let draw = OLive { daa: t1 as u64 + 1, ..w.stated(&shard, &dst, &d.said()) };
    let script = w.script("KpfTDrawEntry");
    let ended = |what: &str, pending: Pending, value: u64, state: [u8; 32]| {
        let b = draw_entry(&w.lib, &draw, &dst, None, &next, pending, &script, xk(&w.f), w.payer(&w.f, &ff)).unwrap();
        let s = signed(what, &b, &w.keys);
        assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (shard.value, &hash_spk(&w.lib, &next.hash())), "{what}: the shard again");
        assert_eq!((s.tx.outputs[2].value, &s.tx.outputs[2].script_public_key), (value, &hash_spk(&w.lib, &state)), "{what}: what is shown goes on");
        assert!(pays(&s, &xk(&w.f), dst.bond / 2), "{what}: half of the bond to who showed it");
    };
    ended("a draw ended by a marked account", Pending::Marked(&m_live), m_live.value, marked.hash());
    // Undone: the note that the leg did not enter ends it, whenever it landed.
    let (b, _, note) = enter_off(&w.lib, &m_live, &w.script("KpfTEnterOff"), w.payer(&w.f, &ff)).unwrap();
    let off = signed("the entry undone", &b, &w.keys);
    let n_live = nth(&off, 2, note, 6_800);
    ended("a draw ended by the note that a leg did not enter", Pending::Note(&n_live), NOTE_MIN as u64, note.hash());
    // A disputed draw is ended the same, its disputer the bond back.
    let dq = w.pointed(&draw, 1);
    let b = draw_entry(&w.lib, &dq, &dst, Some((dq.s.disputer, 1)), &next, Pending::Marked(&m_live), &script, xk(&w.f), w.payer(&w.f, &ff)).unwrap();
    let s = signed("a disputed draw ended by a marked account", &b, &w.keys);
    assert!(pays(&s, &dq.s.disputer, dst.bond) && pays(&s, &xk(&w.f), dst.bond / 2));
    // Nothing of another shard ends it.
    let elsewhere = Marked(Account { shard: Some([0x44; 32]), ..marked.0.clone() });
    let e_live = OLive { s: elsewhere.clone(), ..m_live.clone() };
    assert!(draw_entry(&w.lib, &draw, &dst, None, &next, Pending::Marked(&e_live), &script, xk(&w.f), w.payer(&w.f, &ff)).is_err());
    // The note applied: the books are as they were, and the draw of what the shard is good for is the rule.
    let (b, books) = unentered_apply(&w.lib, &shard, &n_live, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    signed("the note that the leg did not enter, applied", &b, &w.keys);
    assert_eq!((books.rows, books.basis, books.equity(&at9)), (base.s.rows, base.s.basis, Some(100 * KAS1)));
}

/// A note of a leg that is not in the books is applied by nothing: the scripts of legs write none, but a statement
/// nobody disputed could, and it would end every draw for good. Anyone spends it, the shard as it is; and no note
/// of legs that are in the books is spent so.
#[test]
fn a_note_of_a_leg_that_is_not_in_the_books_is_spent_and_changes_nothing() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let (leg, absent) = (fleg(&w, 0, 10, LONG, P10), fleg(&w, 1, 4, SHORT, P10));
    let shard = OLive { s: base.s.booked(&leg).unwrap(), ..base.clone() };
    let void = w.script("KpfTLeftVoid");
    // The note that a leg did not enter, of a leg no row has.
    let ghost = kpf_protocol::covered::Unentered { sid: shard.s.sid, leg: absent, key: xk(&w.w) };
    let g_live = OLive { s: ghost, value: NOTE_MIN as u64, id: shard.id, op: op(61), daa: 6_200 };
    assert!(unentered_apply(&w.lib, &shard, &g_live, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).is_err());
    let b = unentered_void(&w.lib, &shard, &g_live, &void, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("a note of a leg that is in no row, spent", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value, &hash_spk(&w.lib, &shard.s.hash())), "the shard as it is");
    // A note that two legs left, one of them in no row; and one of more units than the row has.
    for (what, legs) in [("one leg of two is in no row", vec![leg, absent]), ("more units than the row has", vec![leg, leg])] {
        let n = Left { sid: shard.s.sid, legs, key: xk(&w.w), asked: 3 * KAS1 };
        let live = left_note(&shard, &n, 7);
        assert!(shard.s.left(&n).is_err(), "{what}");
        let k = shard.s.gone_at(&n).expect(what);
        let b = left_void(&w.lib, &shard, &live, k, &void, w.payer(&w.f, &ff)).unwrap();
        let sg = signed(&format!("{what}: spent"), &b, &w.keys);
        assert_eq!(&sg.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &shard.s.hash()), "{what}: nothing is paid and the books are as they were");
    }
    // A note that holds what an account paid: the shard holds it from there (review 52, F84), and no less.
    let paid = Left { sid: shard.s.sid, legs: vec![absent], key: xk(&w.w), asked: -3 * KAS1 };
    let live = OLive { value: paid.value() as u64, ..left_note(&shard, &paid, 9) };
    let after = shard.s.voided(&paid).unwrap();
    assert_eq!((after.held, after.rows, after.basis), (shard.s.held + 3 * KAS1, shard.s.rows, shard.s.basis));
    let b = left_void(&w.lib, &shard, &live, 0, &void, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("a note that held what an account paid, spent", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value + 3 * KAS1 as u64, &hash_spk(&w.lib, &after.hash())), "the shard holds what the note held");
    for (what, value, state) in [("the shard as it was, and the coins whoever spends it's", shard.value, shard.s), ("the state says more and the output holds no more", shard.value, after), ("a sompi less", shard.value + 3 * KAS1 as u64 - 1, after)] {
        let mut bad = b.clone();
        bad.tx.outputs[0].value = value;
        bad.tx.outputs[0].script_public_key = hash_spk(&w.lib, &state.hash());
        assert!(!accepted(&bad, &w.keys), "{what}");
    }
    assert!(shard.s.voided(&Left { legs: vec![leg], ..paid.clone() }).is_err());
    // A note of legs that are in the books is not spent so, whichever leg is named; it is applied.
    let real = Left { sid: shard.s.sid, legs: vec![leg], key: xk(&w.w), asked: 3 * KAS1 };
    assert_eq!(shard.s.gone_at(&real), None);
    let b = left_void(&w.lib, &shard, &left_note(&shard, &real, 8), 0, &void, w.payer(&w.f, &ff)).unwrap();
    assert!(!accepted(&b, &w.keys), "a note of a leg that is in the books is spent as if it were not");
    let came = kpf_protocol::covered::Unentered { sid: shard.s.sid, leg, key: xk(&w.w) };
    let c_live = OLive { s: came, value: NOTE_MIN as u64, id: shard.id, op: op(62), daa: 6_200 };
    let b = unentered_void(&w.lib, &shard, &c_live, &void, w.payer(&w.f, &ff)).unwrap();
    assert!(!accepted(&b, &w.keys), "the note of a leg that is in the books");
    let (b, books) = unentered_apply(&w.lib, &shard, &c_live, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    signed("it is applied", &b, &w.keys);
    assert_eq!((books.rows, books.basis), (base.s.rows, base.s.basis));
}

/// A note is not taken at its word (review 53, F86): where it says it holds what an account paid, its shard takes
/// what the note's output holds above a note's least, and no more than the note says. A wrong statement nobody
/// disputed writes a note that says anything: it is cleared at nobody's cost, whatever it says.
#[test]
fn a_note_that_says_it_holds_more_than_it_does_gives_its_shard_what_it_holds() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let (leg, absent) = (fleg(&w, 0, 10, LONG, P10), fleg(&w, 1, 4, SHORT, P10));
    let shard = OLive { s: base.s.booked(&leg).unwrap(), ..base.clone() };
    let held = shard.s.held;
    // (What the note says it holds, what its output holds, what the shard takes.)
    let cases = [
        (-600 * KAS1, NOTE_MIN, 0),
        (-600 * KAS1, NOTE_MIN + 3 * KAS1, 3 * KAS1),
        (-3 * KAS1, NOTE_MIN + 3 * KAS1, 3 * KAS1),
        (-3 * KAS1, NOTE_MIN + 5 * KAS1, 3 * KAS1),
        (-3 * KAS1, NOTE_MIN - 1, 0),
        (-(i64::MAX - 5), NOTE_MIN + KAS1, KAS1),
        (-(i64::MAX - 5), NOTE_MIN, 0),
    ];
    for (k, (asked, holds, more)) in cases.into_iter().enumerate() {
        assert_eq!(kpf_protocol::reserve::held_for(asked, holds), more, "case {k}");
        // A leg of the books: applied.
        let n = Left { sid: shard.s.sid, legs: vec![leg], key: xk(&w.w), asked };
        let live = OLive { value: holds as u64, ..left_note(&shard, &n, 10 + k as u8) };
        let (b, next, rest) = left_apply(&w.lib, &shard, &live, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
        let sg = signed(&format!("case {k}: a note saying {asked} and holding {holds}, applied"), &b, &w.keys);
        assert_eq!((next.held, next.rows, next.basis, rest), (held + more, base.s.rows, base.s.basis, None), "case {k}");
        assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value + more as u64, &hash_spk(&w.lib, &next.hash())), "case {k}");
        // The shard written as holding a sompi more than the note held for it, or a sompi less: refused.
        for d in [1i64, -1] {
            if more + d < 0 {
                continue;
            }
            let mut bad = b.clone();
            bad.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Shard { held: held + more + d, ..next }.hash());
            bad.tx.outputs[0].value = (shard.value as i64 + more + d.max(0)) as u64;
            assert!(!accepted(&bad, &w.keys), "case {k}: the shard written holding {d} more");
        }
        // A leg of no row: spent, the shard holding the same more.
        let v = Left { legs: vec![absent], ..n.clone() };
        let live = OLive { value: holds as u64, ..left_note(&shard, &v, 30 + k as u8) };
        let after = shard.s.voided_holding(&v, holds).unwrap();
        assert_eq!((after.held, after.rows, after.basis), (held + more, shard.s.rows, shard.s.basis), "case {k}");
        let b = left_void(&w.lib, &shard, &live, 0, &w.script("KpfTLeftVoid"), w.payer(&w.f, &ff)).unwrap();
        let sg = signed(&format!("case {k}: the same note of a leg in no row, spent"), &b, &w.keys);
        assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value + more as u64, &hash_spk(&w.lib, &after.hash())), "case {k}");
    }
    // Two legs, by the script of several: the same.
    let two = OLive { s: shard.s.booked(&fleg(&w, 1, 4, SHORT, P10)).unwrap(), ..shard.clone() };
    let n = Left { sid: two.s.sid, legs: vec![leg, fleg(&w, 1, 4, SHORT, P10)], key: xk(&w.w), asked: -600 * KAS1 };
    let live = OLive { value: (NOTE_MIN + 2 * KAS1) as u64, ..left_note(&two, &n, 50) };
    let (b, next, _) = left_apply(&w.lib, &two, &live, &w.script("KpfTLeftAll"), w.payer(&w.f, &ff)).unwrap();
    signed("a note of two legs saying 600 KAS and holding 2, applied", &b, &w.keys);
    assert_eq!((next.held, next.rows, next.basis), (held + 2 * KAS1, base.s.rows, base.s.basis));
}

// ---------------------------------------------------------------- the call (docs/ARCHITECTURE-CLEARING.md 14.6)

use kpf_protocol::reserve::{Called, CALL_HOLD, KIND_CALL};

/// A call of `on` at `prices`, stated by the watcher whatever the rule says of it, with the outputs of a call that
/// is the rule's (the fee `fee` to the caller, the call at the prices `shown`).
fn call_said(w: &World, on: &OLive<Shard>, prices: [i64; 4], shown: [i64; 4], fee: i64, amount: i64) -> (Stated, Draw) {
    let x = &on.s;
    let d = Draw { amount, prices };
    let (next, call) = x.called_at(&shown, T0, fee);
    let mut v = vec![(Out { key: false, to: next.hash(), value: on.value as i64 - fee - CALL_HOLD }, false), (Out { key: false, to: call.hash(), value: CALL_HOLD }, true)];
    if fee > 0 {
        v.push((Out { key: true, to: xk(&w.w), value: fee }, false));
    }
    (Stated { position: x.hash(), window: x.dispute_daa, late: x.t.late_daa, kind: KIND_CALL, t0: T0, arg: d.arg(), outs: Outs::with_notes(&v).unwrap(), bond: x.call_bond(), claimer: xk(&w.w), held: on.value as i64 }, d)
}

/// A shard whose equity, each price moved the tolerance in its favour, is below its lower level is called by
/// anyone: the statement, applied, writes the shard called and the call, an object of its own, and pays the caller
/// the terms' least fee of a keeper. A shard at its level, or called, is not called.
#[test]
fn a_shard_below_its_lower_level_is_called_by_anyone_and_the_call_is_an_object_of_its_own() {
    let w = world();
    let shard = w.shard();
    let s = &shard.s;
    let fee = s.t.keeper_fee_min;
    // Market 0, where its side is 60 units short from 10, stands at 11. In its favour: 10.945 there and 10.05 in
    // market 1 (where it is 10 long). Its equity is 100 - 656.7 + 100.5 + 500 = 43.8 KAS; the lower level, 4.5 %
    // of the net's 757.2 and 1.75 % of the gross's 1,833.8, is 66.1655. At 10.5 it has 73.65 against 63.6.
    let up = [P10 * 11 / 10, P10, 0, 0];
    let near = [P10 * 105 / 100, P10, 0, 0];
    assert_eq!(s.call_end_judged(&Draw { amount: 0, prices: up }, T0, shard.value as i64), Some((true, fee)));
    assert!(s.call_due(&up) && !s.call_due(&near) && !s.call_due(&PRICES));
    let f = [fund(xk(&w.w), 30)];
    assert!(shard_call(&w.lib, &shard, &w.script("KpfTChange"), near, T0, xk(&w.w), w.payer(&w.w, &f)).is_err());
    let (b, st, d) = shard_call(&w.lib, &shard, &w.script("KpfTChange"), up, T0, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    let sg = signed("call stated", &b, &w.keys);
    // Its bond: the shard's (a tenth of the 100 KAS it holds), no more than a hundred fees, no less than a note's.
    assert_eq!((st.kind, st.claimer, st.bond), (KIND_CALL, xk(&w.w), (10 * KAS1).min(100 * fee).max(NOTE_MIN)));
    assert_eq!(sg.tx.outputs[0].value, shard.value + st.bond as u64);
    assert_eq!(s.call_wrong_point(&d, &st.claimer, &st.outs, shard.value as i64, T0, &up), None);
    assert_eq!(s.call_wrong_point(&d, &st.claimer, &st.outs, shard.value as i64, T0, &PRICES), Some(1), "at the committee's 10 the price of row 1 is the point");
    // Applied after the dispute period.
    let live = nth(&sg, 0, st, 6_510);
    let ff = [fund(xk(&w.f), 31)];
    let b = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.f, &ff)).unwrap();
    let r = signed("call applied", &b, &w.keys);
    let (next, call) = s.called_at(&up, T0, fee);
    assert_eq!((next.called, next.held, next.rows, next.basis), (T0, s.held - fee, s.rows, s.basis));
    assert_eq!((r.tx.outputs[0].value, &r.tx.outputs[0].script_public_key), (shard.value - fee as u64 - CALL_HOLD as u64, &hash_spk(&w.lib, &next.hash())), "the shard called");
    assert_eq!(out_of(&r, &hash_spk(&w.lib, &call.hash())).1, CALL_HOLD as u64, "the call");
    assert_eq!(call, Called { sid: s.sid, time: T0, heads: [w.head(0), w.head(1), Head::EMPTY, Head::EMPTY], prices: up });
    assert!(pays(&r, &xk(&w.w), fee) && pays(&r, &xk(&w.w), st.bond), "the fee and the bond to the caller");
    // A called shard is not called again, and nothing is drawn from it.
    assert!(!next.call_due(&up) && next.draw_max(&[P10 / 2, P10, 0, 0]) == 0);

    // The end of a call, stated whatever the rule says, answered at the engine.
    let judged = |what: &str, on: &OLive<Shard>, st: &Stated, d: &Draw| -> bool {
        let q = w.pointed(&w.stated(on, st, &d.said()), 0);
        let (b, right) = call_end(&w.lib, &q, &on.s, d, &w.script("KpfTCallEnd"), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
        assert_eq!(right, on.s.call_end_right(d, &st.claimer, &st.outs, on.value as i64, T0), "{what}");
        let sg = signed(&format!("{what}: the end of a call answered {}", if right { "right" } else { "wrong" }), &b, &w.keys);
        if !right {
            assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (on.value, &hash_spk(&w.lib, &on.s.hash())), "{what}: the shard again");
        }
        let forced = call_end(&w.lib, &q, &on.s, d, &w.script("KpfTCallEnd"), xk(&w.f), w.payer(&w.f, &ff), Some(!right));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "{what}: answered the other way");
        right
    };
    let (st, d) = call_said(&w, &shard, up, up, fee, 0);
    assert!(judged("the call that is the rule's", &shard, &st, &d));
    let (st, d) = call_said(&w, &shard, near, near, fee, 0);
    assert!(!judged("a shard at its lower level", &shard, &st, &d));
    let (st, d) = call_said(&w, &shard, PRICES, PRICES, fee, 0);
    assert!(!judged("a shard whose legs stand where they entered", &shard, &st, &d));
    let called = OLive { s: Shard { called: 6_400, ..*s }, ..shard.clone() };
    let (st, d) = call_said(&w, &called, up, up, fee, 0);
    assert!(!judged("a called shard", &called, &st, &d));
    let (st, d) = call_said(&w, &shard, up, up, fee, 1);
    assert!(!judged("something where a draw says its amount", &shard, &st, &d));
    let (st, d) = call_said(&w, &shard, up, up, fee + 1, 0);
    assert!(!judged("a sompi more than the fee", &shard, &st, &d));
    let (st, d) = call_said(&w, &shard, up, up, 0, 0);
    assert!(!judged("no fee, and the shard holding what the fee is", &shard, &st, &d));
    let (st, d) = call_said(&w, &shard, up, near, fee, 0);
    assert!(!judged("a call of other prices than those stated", &shard, &st, &d));
    let (st, d) = call_said(&w, &shard, [P10 * 11 / 10, 0, 0, 0], [P10 * 11 / 10, 0, 0, 0], fee, 0);
    assert!(!judged("a row with a market and no price", &shard, &st, &d));
    let (st, d) = call_said(&w, &shard, up, up, fee, 0);
    let not_called = { let (next, call) = s.called_at(&up, 0, fee); Outs::with_notes(&[(Out { key: false, to: next.hash(), value: shard.value as i64 - fee - CALL_HOLD }, false), (Out { key: false, to: call.hash(), value: CALL_HOLD }, true), (Out { key: true, to: xk(&w.w), value: fee }, false)]).unwrap() };
    assert!(!judged("the shard written not called", &shard, &Stated { outs: not_called, ..st }, &d));
    let other_rows = { let (next, call) = s.called_at(&up, T0, fee); Outs::with_notes(&[(Out { key: false, to: Shard { rows: [ShardRow::EMPTY; 4], ..next }.hash(), value: shard.value as i64 - fee - CALL_HOLD }, false), (Out { key: false, to: call.hash(), value: CALL_HOLD }, true), (Out { key: true, to: xk(&w.w), value: fee }, false)]).unwrap() };
    assert!(!judged("rows of the caller's making in the shard that goes on", &shard, &Stated { outs: other_rows, ..st }, &d));
    // A shard that holds less than the fee pays none, and is called all the same; its bond is a note's least.
    let poor = OLive { s: Shard { held: fee - 1, ..*s }, value: (fee - 1 + SHARD_CARRIER) as u64, ..shard.clone() };
    assert_eq!((poor.s.call_end_judged(&Draw { amount: 0, prices: PRICES }, T0, poor.value as i64), poor.s.call_bond()), (Some((true, 0)), NOTE_MIN));
    let (st, d) = call_said(&w, &poor, PRICES, PRICES, 0, 0);
    assert!(judged("a shard that holds less than the fee", &poor, &st, &d));
    let (b, st2, _) = shard_call(&w.lib, &poor, &w.script("KpfTChange"), PRICES, T0, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    signed("a shard that holds next to nothing is called", &b, &w.keys);
    assert_eq!(st2.bond, NOTE_MIN);
}

/// The prices of a call are the caller's to show: a row's price is answered with the committee's, and a point
/// nobody answers ends the call at the caller's cost.
#[test]
fn the_prices_of_a_call_are_the_callers_to_show() {
    let w = world();
    let shard = w.shard();
    let s = &shard.s;
    let up = [P10 * 11 / 10, P10, 0, 0];
    let f = [fund(xk(&w.w), 30)];
    let ff = [fund(xk(&w.f), 32)];
    let (b, st, d) = shard_call(&w.lib, &shard, &w.script("KpfTChange"), up, T0, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    let live = nth(&signed("call stated", &b, &w.keys), 0, st, 6_510);
    let cite1 = w.script("KpfTCite");
    // The committee's price of market 0 is 10: the call's 11 is wrong, and the shard is again.
    let q = w.pointed(&live, 1);
    let c = w.cited(0, P10);
    let (b, right) = shard_price(&w.lib, &q, s, &d, 1, &w.script("KpfTShardPrice"), Some((&cite1, &c)), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(!right);
    let sg = signed("a call's price answered wrong", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value, &hash_spk(&w.lib, &s.hash())), "the shard again");
    let forced = shard_price(&w.lib, &q, s, &d, 1, &w.script("KpfTShardPrice"), Some((&cite1, &c)), xk(&w.f), w.payer(&w.f, &ff), Some(true));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys));
    // It is 11: right, and the call stands again.
    let c = w.cited(0, P10 * 11 / 10);
    let (b, right) = shard_price(&w.lib, &q, s, &d, 1, &w.script("KpfTShardPrice"), Some((&cite1, &c)), xk(&w.f), w.payer(&w.f, &ff), None).unwrap();
    assert!(right);
    let sg = signed("a call's price answered right", &b, &w.keys);
    assert_eq!(&sg.tx.outputs[0].script_public_key, &hash_spk(&w.lib, &st.again().hash()), "the call stands again");
    // The script that answers a draw's end opens no call, and the call's none of a draw.
    let q0 = w.pointed(&live, 0);
    assert!(shard_end(&w.lib, &q0, s, &d, &w.script("KpfTShardEnd"), xk(&w.f), w.payer(&w.f, &ff), None).is_err());
    let max = s.draw_max(&PRICES);
    let (dst, dd) = s.draw(max, PRICES, T0, shard.value as i64).unwrap();
    let dq = w.pointed(&w.stated(&shard, &dst, &dd.said()), 0);
    assert!(call_end(&w.lib, &dq, s, &dd, &w.script("KpfTCallEnd"), xk(&w.f), w.payer(&w.f, &ff), None).is_err());
    // A point nobody answers: the shard again, and the caller's bond is lost, half to the disputer.
    let q = w.pointed(&live, 2);
    let b = point_lapse(&w.lib, &q, &w.script("KpfTPoint"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("a point of a call that nobody answered", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value, &hash_spk(&w.lib, &s.hash())), "the shard again");
    assert!(pays(&sg, &xk(&w.w), st.bond + st.bond / 2), "the disputer has its bond and half of the caller's");
}

/// On random shards and prices the end of a call takes its model's verdict, and no other.
#[test]
fn the_end_of_a_call_agrees_with_the_model_on_random_shards() {
    let w = world();
    let base = w.shard();
    let scale = w.markets[0].p.scale;
    let pm = w.markets[0].p.price_max;
    let fee = base.s.t.keeper_fee_min;
    let mut r = Rng(0x243f_6a88_85a3_08d3);
    let f = [fund(xk(&w.w), 36)];
    let (mut rights, mut wrongs, mut due_unpaid) = (0, 0, 0);
    for case in 0..80 {
        let mut shard = base.clone();
        for i in 0..4 {
            shard.s.rows[i] = if r.range(0, 3) == 0 {
                ShardRow::EMPTY
            } else {
                let gross = r.range(0, 60) * scale + r.range(0, scale - 1);
                ShardRow { head: w.head(i), net: r.range(-gross, gross), gross }
            };
        }
        // What it holds, and what its legs were entered at: so that its equity at 10 is near its lower level there.
        let at10 = [P10; 4];
        let level = shard.s.level(&at10, false).unwrap_or(0);
        shard.s.basis = shard.s.rows.iter().map(|x| x.net / scale * P10).sum::<i64>();
        shard.s.held = match r.range(0, 9) {
            0 => r.range(0, fee.max(1)),
            1 => level,
            _ => (level + r.range(-20, 20) * KAS1 / 4).max(0),
        };
        shard.s.called = if r.range(0, 19) == 0 { 6_000 } else { 0 };
        shard.value = (shard.s.held + SHARD_CARRIER) as u64;
        let s = &shard.s;
        let mut prices = [0i64; 4];
        for (i, p) in prices.iter_mut().enumerate() {
            *p = match r.range(0, 59) {
                0 => 0,
                1 => pm + 1,
                2 => pm,
                _ => P10 / 100 * r.range(95, 105) + r.range(0, 999),
            };
            if s.rows[i].head == Head::EMPTY && r.range(0, 1) == 0 {
                *p = 0;
            }
        }
        let held = shard.value as i64;
        let amount = if r.range(0, 19) == 0 { 1 } else { 0 };
        let judged = s.call_end_judged(&Draw { amount, prices }, T0, held);
        assert!(judged.is_some(), "case {case}: within what a script computes");
        let pay = judged.unwrap().1;
        let paid = match r.range(0, 11) {
            0 => pay + 1,
            1 if pay > 0 => 0,
            _ => pay,
        };
        let (st, d) = call_said(&w, &shard, prices, prices, paid, amount);
        let right = s.call_end_right(&d, &st.claimer, &st.outs, held, T0);
        assert_eq!(right, judged.unwrap().0 && paid == pay, "case {case}");
        if right {
            rights += 1;
            if pay == 0 { due_unpaid += 1 }
        } else {
            wrongs += 1;
        }
        let q = OLive { s: Pointed { st, disputer: xk(&w.c), point: 0 }, value: shard.value + 2 * st.bond as u64, id: shard.id, op: op(40), daa: 6_520 };
        let (b, said) = call_end(&w.lib, &q, s, &d, &w.script("KpfTCallEnd"), xk(&w.f), w.payer(&w.w, &f), None).unwrap();
        assert_eq!(said, right);
        assert!(accepted(&b, &w.keys), "case {case}: the model's verdict ({right}) is not the script's");
        let other = call_end(&w.lib, &q, s, &d, &w.script("KpfTCallEnd"), xk(&w.f), w.payer(&w.w, &f), Some(!right));
        assert!(other.is_err() || !accepted(&other.unwrap().0, &w.keys), "case {case}: the script takes the other verdict ({})", !right);
    }
    println!("ends of a call: {rights} right ({due_unpaid} with no fee), {wrongs} wrong");
    assert!(rights >= 10 && wrongs >= 10, "both verdicts are met: {rights} {wrongs}");
}

// ---------------------------------------------------------------- under a call (docs 14.6)

use kpf_protocol::covered::CallStep;

/// The shard called and its call, as objects (what the settlement of a call writes: the tests above).
fn called_objs(shard: &OLive<Shard>, prices: [i64; 4], daa: u64) -> (OLive<Shard>, OLive<Called>) {
    let fee = shard.s.t.keeper_fee_min.min(shard.s.held);
    let (next, call) = shard.s.called_at(&prices, T0, fee);
    (OLive { s: next, value: shard.value - fee as u64 - CALL_HOLD as u64, id: shard.id, op: op(90), daa }, OLive { s: call, value: CALL_HOLD as u64, id: shard.id, op: op(91), daa })
}

/// A called shard, whole: its accounts are stepped by anyone at the call's prices (as they stand, or under a
/// statement that landed at or after the call's time), each writing its note; what the notes ask are claims, paid
/// at one share once no row has a unit; and the call ends where nothing is owed.
#[test]
fn a_called_shards_accounts_are_stepped_its_claims_paid_at_one_share_and_its_call_ends() {
    let w = world();
    let base = seeded(&w);
    let (ff, fw) = ([fund(xk(&w.f), 36)], [fund(xk(&w.w), 37)]);
    let fee = base.s.t.keeper_fee_min;
    // Three accounts in market 0, each of one leg from 10: long 10 (the watcher's, 20 KAS), short 4 (the funder's,
    // 5 KAS), long 5 (the watcher's again, 20 KAS). The shard is 11 units short over 19, and holds 20 KAS.
    let (l1, l2, l3) = (fleg(&w, 0, 10, LONG, P10), fleg(&w, 0, 4, SHORT, P10), fleg(&w, 0, 5, LONG, P10));
    let books = base.s.booked(&l1).unwrap().booked(&l2).unwrap().booked(&l3).unwrap();
    let shard = OLive { s: Shard { held: 20 * KAS1, ..books }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let a1 = cov(&w, &shard, &[l1], 20 * KAS1);
    let a2 = { let x = cov(&w, &shard, &[l2], 5 * KAS1); OLive { s: Account { a_key: xk(&w.f), ..x.s.clone() }, op: op(81), ..x } };
    let a3 = OLive { op: op(82), ..cov(&w, &shard, &[l3], 20 * KAS1) };
    // At 13 the shard has lost 33 KAS: it is called.
    let p13 = [P10 * 13 / 10, 0, 0, 0];
    assert!(shard.s.call_due(&p13));
    let (called, call) = called_objs(&shard, p13, 6_600);
    let step_s = w.script("KpfTCStep");

    // The first account, as it stands: its leg gained 30 KAS. Its coins go back to its user less the fee; the note
    // asks the 30.
    let (b, s1) = call_step(&w.lib, &a1, &a1.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("an account stepped under a call: its leg gained", &b, &w.keys);
    // (The note's least is the account's own: whoever steps adds nothing. Review 54, F93.)
    assert_eq!(s1, CallStep { at: vec![0], gain: 30 * KAS1, pay: 0, fee, user: 20 * KAS1 - NOTE_MIN - fee, claimer_bond: 0, disputer_bond: 0, note: Left { sid: shard.s.sid, legs: vec![l1], key: xk(&w.w), asked: 30 * KAS1 }, after: None });
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (CALL_HOLD as u64, &hash_spk(&w.lib, &call.s.hash())), "the call again, as it is");
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &s1.note.hash())).1, NOTE_MIN as u64, "the note");
    // (The fee is no output: what the transaction leaves is its funder's.)
    assert!(pays(&sg, &xk(&w.w), s1.user) && !pays(&sg, &xk(&w.f), fee));
    let n1 = nth(&sg, 2, s1.note.clone(), 6_610);
    // What the step does not make: a note that asks more, the user paid less, no fee.
    for (what, lie) in [
        ("a note that asks a sompi more", CallStep { note: Left { asked: s1.note.asked + 1, ..s1.note.clone() }, ..s1.clone() }),
        ("a note that asks nothing", CallStep { note: Left { asked: 0, ..s1.note.clone() }, ..s1.clone() }),
        ("the user paid a sompi less", CallStep { user: s1.user - 1, ..s1.clone() }),
        ("a note of another key", CallStep { note: Left { key: xk(&w.f), ..s1.note.clone() }, ..s1.clone() }),
    ] {
        let b = call_step_as(&w.lib, &a1, &a1.s, None, &call, &lie, &step_s, w.payer(&w.f, &ff));
        assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "{what}");
    }
    // The call of another shard steps nothing of this one; and an account of no leg is not stepped.
    let other = OLive { s: Called { sid: [0x44; 32], ..call.s }, ..call.clone() };
    assert!(call_step(&w.lib, &a1, &a1.s, None, &other, &step_s, w.payer(&w.f, &ff)).is_err());
    let b = call_step_as(&w.lib, &a1, &a1.s, None, &other, &s1, &step_s, w.payer(&w.f, &ff));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "the call of another shard");
    let none = cov(&w, &shard, &[], 20 * KAS1);
    assert!(call_step(&w.lib, &none, &none.s, None, &call, &step_s, w.payer(&w.f, &ff)).is_err());

    // The second: its leg lost 12 KAS and it has 5: it pays 4.8 (a note's least staying), which the note holds.
    // What is left for its user after the fee is under a note's least: no output of it (it is whoever steps's).
    let (b, s2) = call_step(&w.lib, &a2, &a2.s, None, &call, &step_s, w.payer(&w.w, &fw)).unwrap();
    let sg = signed("an account stepped under a call: its leg lost more than it has", &b, &w.keys);
    assert_eq!((s2.gain, s2.pay, s2.note.asked, s2.fee, s2.user), (-12 * KAS1, 5 * KAS1 - NOTE_MIN, -(5 * KAS1 - NOTE_MIN), 0, 0));
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &s2.note.hash())).1, (NOTE_MIN + s2.pay) as u64, "the note holds what the account paid");
    let n2 = nth(&sg, 2, s2.note.clone(), 6_610);
    let b = call_step_as(&w.lib, &a2, &a2.s, None, &call, &CallStep { pay: s2.pay - 1, ..s2.clone() }, &step_s, w.payer(&w.w, &fw));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "the note holds a sompi less than the account paid");

    // The third carries a take its user stated after the call (it landed at 6,610, the call's time is 6,500): the
    // statement is ended, its bond back, and the account stepped. Disputed, both bonds go back.
    let (st, said, _) = a3.s.c_state(KIND_C_TAKE, KAS1, 0, &[P10 * 13 / 10], 6_605, a3.value as i64, xk(&w.w)).unwrap();
    let live = OLive { daa: 6_610, ..acct_stated(&w, &a3, &st, &said.said()) };
    let (b, s3) = call_step(&w.lib, &live, &a3.s, Some((&st, None)), &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("an account stepped under a call, over a statement that landed after it", &b, &w.keys);
    // (The bond of a statement the user stated is paid with its coins, in one output.)
    assert_eq!((s3.gain, s3.note.asked, s3.user, s3.claimer_bond, s3.disputer_bond), (15 * KAS1, 15 * KAS1, 20 * KAS1 - NOTE_MIN - fee + st.bond, 0, 0));
    assert!(pays(&sg, &xk(&w.w), s3.user) && !pays(&sg, &xk(&w.w), st.bond));
    let n3 = nth(&sg, 2, s3.note.clone(), 6_620);
    let q = OLive { daa: 6_620, ..w.pointed(&live, 0) };
    let (b, s3d) = call_step(&w.lib, &q, &a3.s, Some((&st, Some((xk(&w.w), 0)))), &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    assert_eq!(s3d, CallStep { disputer_bond: st.bond, ..s3.clone() });
    let sg = signed("the same, the statement disputed", &b, &w.keys);
    assert!(pays(&sg, &xk(&w.w), s3.user) && pays(&sg, &xk(&w.w), st.bond), "the disputer's bond back");
    // A statement of before the call's time (stated at 6,400, landed at 6,410) stands: the step does not end it,
    // nor where it is disputed after the call (the object spent is then the dispute's: review 54, F90).
    let (st0, said0, _) = a3.s.c_state(KIND_C_TAKE, KAS1, 0, &[P10 * 13 / 10], 6_400, a3.value as i64, xk(&w.w)).unwrap();
    let early = OLive { daa: 6_410, ..acct_stated(&w, &a3, &st0, &said0.said()) };
    let over0 = a3.s.call_step_over(&call.s, a3.value as i64, Some((st0.bond, st0.claimer, false))).unwrap();
    assert!(call_step(&w.lib, &early, &a3.s, Some((&st0, None)), &call, &step_s, w.payer(&w.f, &ff)).is_err());
    let b = call_step_as(&w.lib, &early, &a3.s, Some((&st0, None)), &call, &over0, &step_s, w.payer(&w.f, &ff));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a statement that landed before the call's time is ended by a step");
    let q0 = OLive { daa: 6_600, ..w.pointed(&early, 0) };
    let over0d = a3.s.call_step_over(&call.s, a3.value as i64, Some((st0.bond, st0.claimer, true))).unwrap();
    assert!(call_step(&w.lib, &q0, &a3.s, Some((&st0, Some((xk(&w.w), 0)))), &call, &step_s, w.payer(&w.f, &ff)).is_err());
    let b = call_step_as(&w.lib, &q0, &a3.s, Some((&st0, Some((xk(&w.w), 0)))), &call, &over0d, &step_s, w.payer(&w.f, &ff));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a statement of before the call, disputed after it, is ended by a step");
    // (Answered right after the call it stands again, written anew: it is held to its own time still.)
    let again = OLive { s: st0.again(), daa: 6_650, ..early.clone() };
    assert!(call_step(&w.lib, &again, &a3.s, Some((&st0.again(), None)), &call, &step_s, w.payer(&w.f, &ff)).is_err());
    let b = call_step_as(&w.lib, &again, &a3.s, Some((&st0.again(), None)), &call, &over0, &step_s, w.payer(&w.f, &ff));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a statement of before the call that stands again is ended by a step");

    // The notes applied to the called shard: the legs out of the books; what they ask, claims; what one holds, the
    // shard's.
    let pay_s = w.script("KpfTCallPay");
    let (b, sh1, c1) = left_apply(&w.lib, &called, &n1, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("the first note, to the called shard", &b, &w.keys);
    let (sh1, c1) = (nth(&sg, 0, sh1, 6_630), nth(&sg, 2, c1.unwrap(), 6_630));
    // (A claim is not paid while a row has a unit.)
    assert!(claim_pay(&w.lib, &sh1, &c1, &pay_s, w.payer(&w.f, &ff)).is_err());
    let b = claim_pay_as(&w.lib, &sh1, &c1, Shard { owed: 0, ..sh1.s }, 0, &pay_s, w.payer(&w.f, &ff));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a claim paid while a row has a unit");
    let (b, sh2, none) = left_apply(&w.lib, &sh1, &n2, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    let sh2 = nth(&signed("the second note: what it holds is the shard's", &b, &w.keys), 0, sh2, 6_640);
    assert_eq!((none, sh2.s.held, sh2.s.owed), (None, called.s.held + s2.pay, 30 * KAS1));
    let (b, sh3, c3) = left_apply(&w.lib, &sh2, &n3, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("the third note", &b, &w.keys);
    let (sh3, c3) = (nth(&sg, 0, sh3, 6_650), nth(&sg, 2, c3.unwrap(), 6_650));
    assert_eq!((sh3.s.rows, sh3.s.basis, sh3.s.owed, sh3.s.held), ([ShardRow::EMPTY; 4], 0, 45 * KAS1, 20 * KAS1 - fee + s2.pay));
    // (The call does not end while something is owed.)
    assert!(call_off(&w.lib, &sh3, &call, &pay_s, w.payer(&w.f, &ff)).is_err());
    let b = call_off_as(&w.lib, &sh3, &call, Shard { called: 0, ..sh3.s }, &pay_s, w.payer(&w.f, &ff));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "the call ended while claims are owed");

    // The claims, at one share: 24.7 KAS held against 45 owed. The first (30) is counted in a unit of a thousand
    // sompi (45 KAS is above 3,000,000,000 sompi): it has at most 7 parts in ten million less than its exact share.
    let held = sh3.s.held;
    let (b, sh4, p1) = claim_pay(&w.lib, &sh3, &c1, &pay_s, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("a claim paid its share", &b, &w.keys);
    let exact = (30 * KAS1 as i128 * held as i128 / (45 * KAS1) as i128) as i64;
    assert!(p1 <= exact && exact - p1 <= exact / 1_000_000, "{p1} against {exact}");
    assert_eq!((sh4.held, sh4.owed), (held - p1, 15 * KAS1));
    assert!(pays(&sg, &xk(&w.w), p1));
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (sh3.value - p1 as u64, &hash_spk(&w.lib, &sh4.hash())));
    for (what, next, paid) in [("a sompi more", Shard { held: held - p1 - 1, ..sh4 }, p1 + 1), ("all of it", Shard { held: held - 30 * KAS1.min(held), ..sh4 }, (30 * KAS1).min(held)), ("the claim paid and still owed", Shard { owed: 45 * KAS1, ..sh4 }, p1), ("the shard no poorer", Shard { held, ..sh4 }, p1)] {
        let b = claim_pay_as(&w.lib, &sh3, &c1, next, paid, &pay_s, w.payer(&w.f, &ff));
        assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "{what}");
    }
    // The last claim has all that is left: the same share but for what rounding left.
    let sh4 = nth(&sg, 0, sh4, 6_660);
    let (b, sh5, p3) = claim_pay(&w.lib, &sh4, &c3, &pay_s, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("the last claim paid", &b, &w.keys);
    assert_eq!((p3, sh5.held, sh5.owed), (held - p1, 0, 0));
    assert!((p3 as i128 * 30 - p1 as i128 * 15).abs() <= 30 * 15 * 1_000, "one share: {p1} of 30 and {p3} of 15");
    // The call ends: the shard is not called, and what the call held is its output's again.
    let sh5 = nth(&sg, 0, sh5, 6_670);
    let (b, open) = call_off(&w.lib, &sh5, &call, &pay_s, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("the call ends", &b, &w.keys);
    assert_eq!((open.called, open.owed, open.held, open.rows), (0, 0, 0, [ShardRow::EMPTY; 4]));
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (sh5.value + CALL_HOLD as u64, &hash_spk(&w.lib, &open.hash())));
    let b = call_off_as(&w.lib, &sh5, &call, sh5.s, &pay_s, w.payer(&w.f, &ff));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "the call spent and the shard still called");
}

/// A shard that holds more than it owes by claims pays each in full; and one that holds nothing pays nothing, and
/// its claims are spent all the same, so that its call ends.
#[test]
fn a_claim_is_paid_no_more_than_it_asks_and_a_shard_of_nothing_pays_nothing() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let pay_s = w.script("KpfTCallPay");
    let claim = |amount: i64, k: u8| OLive { s: Rest { sid: base.s.sid, key: xk(&w.w), amount }, value: NOTE_MIN as u64, id: base.id, op: op(70 + k), daa: 6_600 };
    for (what, held, owed, amount, paid) in [
        ("more held than owed", 50 * KAS1, 30 * KAS1, 30 * KAS1, 30 * KAS1),
        ("as much held as owed", 30 * KAS1, 30 * KAS1, 12 * KAS1, 12 * KAS1),
        ("nothing held", 0, 30 * KAS1, 12 * KAS1, 0),
        ("a sompi held", 1, 30 * KAS1, 30 * KAS1, 1),
        ("a sompi held, of a part", 1, 30 * KAS1, 29 * KAS1, 0),
        ("half, exact below 30 KAS owed", 10 * KAS1 + 1, 20 * KAS1 + 2, 7 * KAS1, 7 * KAS1 / 2),
        ("a third of 3,000 KAS owed, in thousands of sompi", 1_000 * KAS1, 3_000 * KAS1, 300 * KAS1, 100 * KAS1 - 100 * KAS1 / 300_000_000_000 * 0),
    ] {
        let s = Shard { held, owed, called: 6_400, ..base.s };
        let live = OLive { s, value: (held + SHARD_CARRIER) as u64, ..base.clone() };
        let c = claim(amount, 1);
        let (next, p) = s.claim_paid(&c.s).unwrap();
        let exact = (amount as i128 * held as i128 / owed as i128).min(amount as i128) as i64;
        assert!(p <= exact && exact - p <= exact / 1_000_000 + 1, "{what}: {p} against {exact}");
        if owed < 3_000_000_000 || held >= owed {
            assert_eq!(p, exact, "{what}");
        }
        let _ = paid;
        let (b, n2, p2) = claim_pay(&w.lib, &live, &c, &pay_s, w.payer(&w.f, &ff)).unwrap();
        assert_eq!((n2, p2), (next, p));
        signed(&format!("{what}: a claim of {amount} paid {p}"), &b, &w.keys);
        let b = claim_pay_as(&w.lib, &live, &c, Shard { held: held - p - 1, ..next }, p + 1, &pay_s, w.payer(&w.f, &ff));
        assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "{what}: a sompi more");
    }
    // A claim of more than is owed, of nothing, to another shard, or on a shard that is not called: not paid.
    let s = Shard { held: 50 * KAS1, owed: 30 * KAS1, called: 6_400, ..base.s };
    assert!(s.claim_paid(&claim(30 * KAS1 + 1, 2).s).is_err() && s.claim_paid(&claim(0, 2).s).is_err() && s.claim_paid(&Rest { sid: [0x44; 32], ..claim(KAS1, 2).s }).is_err());
    assert!(Shard { called: 0, ..s }.claim_paid(&claim(KAS1, 2).s).is_err());
    let live = OLive { s: Shard { called: 0, ..s }, value: (50 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    let b = claim_pay_as(&w.lib, &live, &claim(KAS1, 2), Shard { held: 49 * KAS1, owed: 29 * KAS1, called: 0, ..s }, KAS1, &pay_s, w.payer(&w.f, &ff));
    assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "a claim paid by a shard that is not called");
}

/// On random accounts and calls, a step is the model's and no other.
#[test]
fn on_random_accounts_a_step_under_a_call_is_the_models() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let step_s = w.script("KpfTCStep");
    let mut r = Rng(0x13198a2e03707344);
    let (mut gained, mut lost, mut short_of, mut dust) = (0, 0, 0, 0);
    for case in 0..60 {
        // A call of four rows, one a market, at prices near 10.
        let mut books = base.s;
        for i in 0..4 {
            books = books.booked(&fleg(&w, i, 1, LONG, P10)).unwrap();
        }
        let mut prices = [0i64; 4];
        for p in prices.iter_mut() {
            *p = P10 / 100 * r.range(70, 130) + r.range(0, 999);
        }
        let shard = OLive { s: books, ..base.clone() };
        let (_, call) = called_objs(&shard, prices, 6_600);
        let n = r.range(1, 6) as usize;
        let legs: Vec<XLeg> = (0..n).map(|_| fleg(&w, r.range(0, 3) as usize, r.range(1, 40), if r.range(0, 1) == 0 { LONG } else { SHORT }, P10 / 100 * r.range(80, 120) + r.range(0, 999))).collect();
        let coins = match r.range(0, 5) {
            0 => r.range(0, NOTE_MIN + 10_000_000),
            1 => NOTE_MIN + r.range(0, 30_000_000),
            _ => r.range(1, 600) * KAS1 + r.range(0, KAS1 - 1),
        };
        let acct = cov(&w, &shard, &legs, coins);
        let step = acct.s.call_step(&call.s, coins).expect("a step");
        if step.gain >= 0 { gained += 1 } else if step.pay < -step.gain { short_of += 1 } else { lost += 1 }
        if step.user == 0 { dust += 1 }
        let least = NOTE_MIN.min(coins - step.pay);
        assert!(step.pay + least + step.fee + step.user == coins || (step.user == 0 && coins - step.pay - least - step.fee < NOTE_MIN), "case {case}: the coins are the note's, the fee's and the user's");
        let (b, said) = call_step(&w.lib, &acct, &acct.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
        assert_eq!(said, step);
        assert!(accepted(&b, &w.keys), "case {case}: the model's step is not the script's: {step:?}");
        let mut lies = vec![CallStep { note: Left { asked: step.note.asked + 1, ..step.note.clone() }, ..step.clone() }, CallStep { note: Left { asked: step.note.asked - 1, ..step.note.clone() }, ..step.clone() }];
        if step.user > 0 {
            lies.push(CallStep { user: step.user - 1, ..step.clone() });
        }
        if step.pay > 0 {
            lies.push(CallStep { pay: step.pay - 1, ..step.clone() });
        }
        for (k, lie) in lies.iter().enumerate() {
            let b = call_step_as(&w.lib, &acct, &acct.s, None, &call, lie, &step_s, w.payer(&w.f, &ff));
            assert!(b.is_err() || !accepted(&b.unwrap(), &w.keys), "case {case}: lie {k} is taken: {lie:?}");
        }
    }
    println!("steps under a call: {gained} gained, {lost} lost and paid, {short_of} lost more than they had, {dust} with nothing left for the user");
    assert!(gained >= 8 && lost >= 8 && short_of >= 3 && dust >= 2, "{gained} {lost} {short_of} {dust}");
}

/// An entry stated at or after the call's time is ended by anyone who shows the call: the marked account again,
/// the bonds back. One that landed before stands.
#[test]
fn an_entry_that_landed_after_the_call_is_ended_by_showing_the_call() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let leg = fleg(&w, 0, 10, LONG, P10);
    let acct = cov(&w, &base, &[], 20 * KAS1);
    let (st, _said, marked, _after) = acct.s.c_enter(leg, 5 * KAS1, 0, &[], T0 + 100, acct.value as i64 + 5 * KAS1).unwrap();
    let shard = OLive { s: base.s.booked(&leg).unwrap(), ..base.clone() };
    let (_, call) = called_objs(&shard, [P10, 0, 0, 0], 6_600);
    let script = w.script("KpfTCallEnter");
    let live = OLive { s: st, value: (st.held + st.bond) as u64, id: base.id, op: op(85), daa: 6_610 };
    let b = call_enter_end(&w.lib, &live, &st, None, &marked, &call, &script, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("an entry ended by the call", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (st.held as u64, &hash_spk(&w.lib, &marked.hash())), "the marked account again");
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &call.s.hash())).1, CALL_HOLD as u64, "the call again");
    assert!(pays(&sg, &st.claimer, st.bond));
    // Disputed: both bonds back.
    let q = OLive { s: Pointed { st, disputer: xk(&w.c), point: 2 }, value: (st.held + 2 * st.bond) as u64, id: live.id, op: live.op, daa: live.daa };
    let b = call_enter_end(&w.lib, &q, &st, Some((xk(&w.c), 2)), &marked, &call, &script, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("an entry disputed, ended by the call", &b, &w.keys);
    assert!(pays(&sg, &st.claimer, st.bond) && pays(&sg, &xk(&w.c), st.bond));
    // One of before the call's time (stated at 6,600, landed at 6,610, under a call of 6,611) stands, disputed
    // after the call or not; and the call of another shard ends nothing.
    let later = OLive { s: Called { time: 6_611, ..call.s }, ..call.clone() };
    let q_after = OLive { s: q.s, value: q.value, id: q.id, op: q.op, daa: 6_700 };
    assert!(call_enter_end(&w.lib, &q_after, &st, Some((xk(&w.c), 2)), &marked, &later, &script, w.payer(&w.f, &ff)).is_err());
    assert!(call_enter_end(&w.lib, &live, &st, None, &marked, &later, &script, w.payer(&w.f, &ff)).is_err());
    let b = call_enter_end(&w.lib, &OLive { daa: 6_700, ..live.clone() }, &st, None, &marked, &later, &script, w.payer(&w.f, &ff)).unwrap();
    let mut t = b.clone();
    assert!(accepted(&t, &w.keys), "(landed at 6,700, after a call of 6,611: ended)");
    // (The same transaction, its object's place in the chain before the call's time.)
    for e in t.entries.iter_mut().take(1) {
        e.block_daa_score = 6_610;
    }
    assert!(!accepted(&t, &w.keys), "an entry that landed before the call's time is ended");
    let other = OLive { s: Called { sid: [0x44; 32], ..call.s }, ..call.clone() };
    assert!(call_enter_end(&w.lib, &live, &st, None, &marked, &other, &script, w.payer(&w.f, &ff)).is_err());
}

/// Review 54, F89 and F88. An account whose output holds less than it must pay is stepped all the same, for what
/// the output holds (its leg leaves the books, and the claims of its shard are paid). And a statement on a shard
/// has a bond of a note's least at the least, whatever the shard holds.
#[test]
fn an_account_that_holds_less_than_it_says_is_stepped_and_a_small_shards_draw_has_a_bond() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let leg = fleg(&w, 0, 10, SHORT, P10);
    let shard = OLive { s: base.s.booked(&leg).unwrap(), ..base.clone() };
    let (_, call) = called_objs(&shard, [P10 * 14 / 10, 0, 0, 0], 6_600);
    // The account says 1,000 KAS and its output holds 30; its leg lost 40 at the call's price.
    let x = cov(&w, &shard, &[leg], 1_000 * KAS1);
    let thin = OLive { value: 30 * KAS1 as u64, ..x };
    let (b, step) = call_step(&w.lib, &thin, &thin.s, None, &call, &w.script("KpfTCStep"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("an account whose output holds less than it lost, stepped", &b, &w.keys);
    assert_eq!((step.gain, step.pay, step.note.asked, step.fee, step.user), (-40 * KAS1, 30 * KAS1, -30 * KAS1, 0, 0), "it pays what its output holds");
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &step.note.hash())).1, (NOTE_MIN + 30 * KAS1) as u64);
    // (The note then gives the shard what it holds above its least: the 30.)
    let note = nth(&sg, 2, step.note.clone(), 6_610);
    let (called, _) = called_objs(&shard, [P10 * 14 / 10, 0, 0, 0], 6_600);
    let (b, after, _) = left_apply(&w.lib, &called, &note, &w.script("KpfTLeft"), w.payer(&w.f, &ff)).unwrap();
    signed("its note applied", &b, &w.keys);
    assert_eq!((after.held, after.rows), (called.s.held + 30 * KAS1, [ShardRow::EMPTY; 4]));

    // A shard of 1 KAS at a tenth: its bond would be 0.1 KAS; a draw's is a KAS at the least (review 56, F101).
    let small = OLive { s: Shard { held: KAS1, ..base.s }, value: (KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    assert_eq!(small.s.bond(), KAS1);
    let fc = [fund(xk(&w.c), 30)];
    let (b, st, d) = shard_draw(&w.lib, &small, &w.script("KpfTChange"), KAS1 / 2, [0; 4], T0, w.payer(&w.c, &fc)).unwrap();
    let sg = signed("a small shard's draw stated", &b, &w.keys);
    assert_eq!((st.bond, sg.tx.outputs[0].value), (KAS1, small.value + KAS1 as u64));
    let cheap = Stated { bond: KAS1 / 10, ..st };
    let b = state_raw(&w.lib, &small, &small.s.fixed_hash(), &small.s.moving(), &cheap, &d.said(), small.s.form(), &w.script("KpfTChange"), w.payer(&w.c, &fc)).unwrap();
    assert!(!accepted(&b, &w.keys), "a draw stated at the shard's rate of what it holds, below a KAS");
    // And a covered account's statement has a bond of a note's least at the least (F100): an account of 1 KAS.
    let leg1 = fleg(&w, 1, 1, LONG, P10);
    let tiny = cov(&w, &shard, &[leg1], KAS1);
    assert_eq!((tiny.s.bond(), tiny.s.c_bond()), (KAS1 / 10, NOTE_MIN));
    let (st, said, _) = tiny.s.c_state(KIND_C_LEAVE, 0, ALL, &[P10], T0, tiny.value as i64, xk(&w.w)).unwrap();
    assert_eq!(st.bond, NOTE_MIN);
    acct_stated(&w, &tiny, &st, &said.said());
    assert!(account_state(&w.lib, &tiny, &w.script("KpfTChange"), &Stated { bond: KAS1 / 10, ..st }, &said.said(), w.payer(&w.w, &[fund(xk(&w.w), 34)])).is_err());
    let b = state_raw(&w.lib, &tiny, &tiny.s.fixed_hash(), &tiny.s.moving(), &Stated { bond: KAS1 / 10, ..st }, &said.said(), tiny.s.form(), &w.script("KpfTChange"), w.payer(&w.w, &[fund(xk(&w.w), 34)])).unwrap();
    assert!(!accepted(&b, &w.keys), "a leaving stated at the account's rate of its coins, below a note's least");
}

// ---------------------------------------------------------------- an account changes hands (docs 15)

/// A covered account is handed to another key by its user and whoever takes it: the account again under the other
/// key, nothing else of it changed, the shard not shown; what the taker pays the leaver is in the transaction. And
/// two accounts of one user under one shard become one: the legs and the coins of both, the later time; the shard
/// shown, as it is, and not called.
#[test]
fn an_account_changes_hands_and_two_of_one_user_become_one() {
    let w = world();
    let base = seeded(&w);
    let (l1, l2) = (fleg(&w, 0, 10, LONG, P10), fleg(&w, 1, 5, SHORT, P10));
    let shard = OLive { s: base.s.booked(&l1).unwrap().booked(&l2).unwrap(), ..base.clone() };
    let a1 = cov(&w, &shard, &[l1], 20 * KAS1);
    // The watcher's account goes to the funder, who pays 3 KAS for it.
    let ff = [fund(xk(&w.f), 36)];
    let (b, mine) = account_pass(&w.lib, &a1, &w.script("KpfTPass"), xk(&w.f), 0, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("an account handed over", &b, &w.keys);
    // (The account says the time before which nothing is done on it at will again: when its output landed, and a
    // lateness. Review 57, F104.)
    let late = a1.s.t.late_daa;
    assert_eq!(mine, Account { a_key: xk(&w.f), expiry: a1.daa as i64 + late, ..a1.s.clone() });
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (a1.value, &hash_spk(&w.lib, &mine.hash())));
    // (What the taker pays the leaver is in the same transaction, and no script's: the two signatures hold it.)
    let (paid, _) = account_pass(&w.lib, &a1, &w.script("KpfTPass"), xk(&w.f), 3 * KAS1, w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&paid, &w.keys));
    assert!(pays(&paid.sign(&w.keys).unwrap(), &xk(&w.w), 3 * KAS1), "the leaver is paid in the same transaction");
    assert_eq!(sg.tx.inputs.len(), 3, "the account, the script and the taker's coin: no shard");
    // Not: the account written with other coins, another leg's entry, another time, or holding less.
    for (what, other) in [
        ("a coin more in it", Account { a_margin: mine.a_margin + 1, ..mine.clone() }),
        ("no time written", Account { expiry: 0, ..mine.clone() }),
        ("a time a block sooner", Account { expiry: mine.expiry - 1, ..mine.clone() }),
        ("a leg at another entry", Account { legs: vec![XLeg { entry: P10 / 2, ..l1 }], ..mine.clone() }),
        ("no time before which its user states nothing", Account { b_min_counter: mine.b_min_counter + 1, ..mine.clone() }),
        ("another shard", Account { shard: Some([0x44; 32]), ..mine.clone() }),
        ("the clearing party's key changed with it", Account { b_key: xk(&w.f), ..mine.clone() }),
    ] {
        let mut t = b.clone();
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &other.hash());
        assert!(!accepted(&t, &w.keys), "{what}");
    }
    let mut t = b.clone();
    t.tx.outputs[0].value -= 1;
    assert!(!accepted(&t, &w.keys), "the account holds a sompi less");
    // Whoever takes it signs: the same transaction signed without the taker's key on the account is refused.
    let mut only = KeySet::new();
    only.add_keypair(w.w);
    assert!(b.sign(&only).is_err() || !accepted(&b, &only), "handed over without the taker's signature");
    // Handed on at once it is not: the taker's coin landed before the time the account now says. With a coin that
    // landed at that time, it is; and an addition waits the same way.
    let handed = OLive { s: mine.clone(), daa: 6_150, ..a1.clone() };
    let old_coin = [KeyUtxo { daa: 0, ..fund(xk(&w.w), 38) }];
    assert!(account_pass(&w.lib, &handed, &w.script("KpfTPass"), xk(&w.w), 0, w.payer(&w.w, &old_coin)).is_err());
    assert!(account_add(&w.lib, &handed, &w.script("KpfTAddC"), SIDE_A, KAS1, w.payer(&w.f, &old_coin)).is_err());
    let new_coin = [KeyUtxo { daa: mine.expiry as u64, ..fund(xk(&w.w), 39) }];
    let (b, back) = account_pass(&w.lib, &handed, &w.script("KpfTPass"), xk(&w.w), 0, w.payer(&w.w, &new_coin)).unwrap();
    signed("handed on, a lateness later", &b, &w.keys);
    assert_eq!((back.a_key, back.expiry), (xk(&w.w), mine.expiry.max(6_150 + late)));
    // (The same transaction with the coin a block older: refused at the engine.)
    let mut t = b.clone();
    for e in t.entries.iter_mut().skip(2).take(1) {
        e.block_daa_score = mine.expiry as u64 - 1;
    }
    assert!(!accepted(&t, &w.keys), "handed on before the time the account says");
    let new_f = [KeyUtxo { daa: mine.expiry as u64, ..fund(xk(&w.f), 40) }];
    let (b, more) = account_add(&w.lib, &handed, &w.script("KpfTAddC"), SIDE_A, KAS1, w.payer(&w.f, &new_f)).unwrap();
    signed("added to, a lateness later", &b, &w.keys);
    assert_eq!((more.a_margin, more.expiry), (mine.a_margin + KAS1, mine.expiry.max(6_150 + late)));
    let mut t = b.clone();
    for e in t.entries.iter_mut().skip(2).take(1) {
        e.block_daa_score = mine.expiry as u64 - 1;
    }
    assert!(!accepted(&t, &w.keys), "added to before the time the account says");
    // The account is the taker's from there: a statement of its own, and the old user states none.
    let held = a1.value as i64;
    assert!(mine.c_state(KIND_C_TAKE, KAS1, 0, &[P10], T0, held, xk(&w.f)).is_ok() && mine.c_state(KIND_C_TAKE, KAS1, 0, &[P10], T0, held, xk(&w.w)).is_err());

    // Two accounts of one user: a long of 10 with 20 KAS, a short of 5 with 8 KAS and a time; joined.
    let a2 = { let x = cov(&w, &shard, &[l2], 8 * KAS1); OLive { s: Account { b_min_counter: 7_000, ..x.s.clone() }, op: op(81), ..x } };
    let fw = [fund(xk(&w.w), 37)];
    let join = w.script("KpfTJoinC");
    let (b, one) = account_join(&w.lib, &a1, &a2, &shard, &join, w.payer(&w.w, &fw)).unwrap();
    let sg = signed("two accounts joined", &b, &w.keys);
    assert_eq!((one.legs.clone(), one.a_margin, one.a_min_counter, one.b_min_counter), (vec![l1, l2], 28 * KAS1, 2, 7_000));
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (a1.value + a2.value, &hash_spk(&w.lib, &one.hash())));
    assert_eq!((sg.tx.outputs[2].value, &sg.tx.outputs[2].script_public_key), (shard.value, &hash_spk(&w.lib, &shard.s.hash())), "the shard as it is");
    // (One account of the two: a gain of the one's leg stands for the other's in its user's standing.)
    assert!(one.c_check().is_ok());
    for (what, other) in [
        ("a coin more than the two had", Account { a_margin: one.a_margin + 1, ..one.clone() }),
        ("the earlier of the two times", Account { b_min_counter: 0, ..one.clone() }),
        ("one leg of the two", Account { legs: vec![l1], a_min_counter: 1, ..one.clone() }),
        ("the legs the other way round", Account { legs: vec![l2, l1], ..one.clone() }),
    ] {
        let mut t = b.clone();
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &other.hash());
        assert!(!accepted(&t, &w.keys), "{what}");
    }
    let mut t = b.clone();
    t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &Shard { held: shard.s.held + 1, ..shard.s }.hash());
    assert!(!accepted(&t, &w.keys), "the shard written otherwise");
    // Not joined: under a called shard; another user's; of another shard; more than six legs between them.
    let called = OLive { s: Shard { called: 6_400, ..shard.s }, ..shard.clone() };
    assert!(account_join(&w.lib, &a1, &a2, &called, &join, w.payer(&w.w, &fw)).is_err());
    let theirs = OLive { s: mine.clone(), ..a1.clone() };
    assert!(account_join(&w.lib, &theirs, &a2, &shard, &join, w.payer(&w.w, &fw)).is_err());
    let elsewhere = OLive { s: Account { shard: Some([0x44; 32]), ..a2.s.clone() }, ..a2.clone() };
    assert!(account_join(&w.lib, &a1, &elsewhere, &shard, &join, w.payer(&w.w, &fw)).is_err());
    let four = cov(&w, &shard, &[l1, l1, l1, l1], 20 * KAS1);
    let three = OLive { op: op(82), ..cov(&w, &shard, &[l2, l2, l2], 20 * KAS1) };
    assert!(account_join(&w.lib, &four, &three, &shard, &join, w.payer(&w.w, &fw)).is_err());
    let two = OLive { op: op(83), ..cov(&w, &shard, &[l2, l2], 20 * KAS1) };
    let (b, six) = account_join(&w.lib, &four, &two, &shard, &join, w.payer(&w.w, &fw)).unwrap();
    signed("four legs and two joined: six", &b, &w.keys);
    assert_eq!((six.legs.len(), six.a_min_counter), (6, 6));
}

/// Review 58, F105. A statement out of its time is ended by anyone for half its bond, and the other half stays in
/// the script's copy: its own claimer has no round for two fees. And an entry's time is not ahead of the chain:
/// the transaction shows an input that landed at or after it.
#[test]
fn a_statement_out_of_its_time_costs_half_its_bond_and_an_entry_is_not_of_a_time_ahead() {
    let w = world();
    let base = seeded(&w);
    let leg = fleg(&w, 0, 10, LONG, P10);
    let shard = OLive { s: base.s.booked(&leg).unwrap(), ..base.clone() };
    let acct = cov(&w, &shard, &[leg], 20 * KAS1);
    let ff = [fund(xk(&w.f), 36)];
    // A take of a time ahead (7,500; it lands at 6,510): nothing applies or disputes it; anyone ends it.
    let (st, said, _) = acct.s.c_state(KIND_C_TAKE, KAS1, 0, &[P10], 7_500, acct.value as i64, xk(&w.w)).unwrap();
    let live = acct_stated(&w, &acct, &st, &said.said());
    assert!(!st.in_time(live.daa));
    assert!(stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.f, &ff)).is_err() || !accepted(&stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.f, &ff)).unwrap(), &w.keys));
    let late = w.script("KpfTChangeLate");
    let b = stated_late(&w.lib, &live, &late, xk(&w.f), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("a statement of a time ahead, ended", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (acct.value, &hash_spk(&w.lib, &acct.s.hash())), "the account again");
    assert!(pays(&sg, &xk(&w.f), st.bond / 2) && !pays(&sg, &xk(&w.f), st.bond), "half the bond to whoever ends it");
    // (The whole bond to whoever ends it, the copy keeping none of it: refused.)
    let mut t = b.clone();
    t.tx.outputs[1].value -= (st.bond - st.bond / 2) as u64;
    let n = t.tx.outputs.len();
    t.tx.outputs[n - 1].value += (st.bond - st.bond / 2) as u64;
    assert!(!accepted(&t, &w.keys), "the whole bond to whoever ends a statement out of its time");
    // One that landed in time is not ended so.
    let (st2, said2, _) = acct.s.c_state(KIND_C_TAKE, KAS1, 0, &[P10], T0, acct.value as i64, xk(&w.w)).unwrap();
    let live2 = acct_stated(&w, &acct, &st2, &said2.said());
    assert!(stated_late(&w.lib, &live2, &late, xk(&w.f), w.payer(&w.f, &ff)).is_err());

    // An entry of a time ahead of every input of its transaction is not stated.
    let user = cov(&w, &base, &[], 20 * KAS1);
    let fw = [fund(xk(&w.w), 37)];
    let enter = w.script("KpfTEnterS");
    assert!(c_enter_state(&w.lib, &user, &base, &enter, SEED, leg, 5 * KAS1, &[], T0 + 1, w.payer(&w.w, &fw)).is_err(), "the funder's coin landed a block before the entry's time");
    let (b, ..) = c_enter_state(&w.lib, &user, &base, &enter, SEED, leg, 5 * KAS1, &[], T0, w.payer(&w.w, &fw)).unwrap();
    assert!(accepted(&b, &w.keys));
    // (The same transaction, the coin shown a block older than the entry's time: refused at the engine.)
    let mut t = b.clone();
    for e in t.entries.iter_mut().skip(3).take(1) {
        e.block_daa_score = T0 as u64 - 1;
    }
    assert!(!accepted(&t, &w.keys), "an entry of a time ahead of the input it shows");
}

// ---------------------------------------------------------------- one account made two (docs 16.2)

/// A leg leaves a covered account for an account of its own, by its user's statement: each of the two stands by
/// itself at the prices stated, no note is written and the shard is not shown. The new account is then joined to
/// another, or handed over: a leg moves between accounts, and a part of an account is sold.
#[test]
fn a_leg_leaves_an_account_for_one_of_its_own_and_each_of_the_two_stands() {
    use kpf_protocol::covered::KIND_C_SPLIT;
    let w = world();
    let base = seeded(&w);
    let legs = two_legs(&w);
    let shard = OLive { s: base.s.booked(&legs[0]).unwrap().booked(&legs[1]).unwrap(), ..base.clone() };
    let acct = cov(&w, &shard, &legs, 40 * KAS1);
    let a = [P10, P10];
    // The short of 5 units of market 1 (worth 50 KAS) leaves with 10 KAS: 30 stay with the long of 10 (worth 100).
    let (live, said, o, sg) = c_applied(&w, "a leg leaves for an account of its own", &acct, KIND_C_SPLIT, 10 * KAS1, 2, &a, &w.w);
    let (stays, new) = (o.after.clone().unwrap(), o.split.clone().unwrap());
    assert_eq!((o.note.clone(), o.paid, o.fee), (None, 0, 0), "no note: the books are as they were");
    assert_eq!((stays.legs.clone(), stays.a_margin, stays.a_min_counter), (vec![legs[0]], 30 * KAS1, 1));
    assert_eq!((new.legs.clone(), new.a_margin, new.a_min_counter, new.a_key, new.shard), (vec![legs[1]], 10 * KAS1, 1, acct.s.a_key, acct.s.shard));
    assert_eq!((stays.b_min_counter, new.b_min_counter), (live.s.t0 + 2 * (acct.s.t.late_daa + 2 * acct.s.dispute_daa), stays.b_min_counter), "both say the time a statement of the user's writes");
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (30 * KAS1 as u64, &hash_spk(&w.lib, &stays.hash())));
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &new.hash())).1, 10 * KAS1 as u64, "the new account, under the same id");
    assert!(stays.c_check().is_ok() && new.c_check().is_ok());
    // Its end is answered right; and the ends that are not the rule's, wrong.
    end_answered(&w, "a split", &acct, &live, &said, true);
    let up = acct.s.t.mmr_bps + acct.s.t.g_bps;
    let need = |worth: i64| NOTE_MIN + (worth * up + 9_999) / 10_000;
    let lie = |what: &str, amount: i64, skip: i64, claimer: [u8; 32], t0: i64, want: bool| {
        let rows = acct.s.c_trace(&a, skip.max(1)).unwrap();
        let lc = kpf_protocol::covered::chain_of_one(&acct.s.legs[(skip.max(1) - 1) as usize]);
        let s = CSaid { amount, skip, lc, rows };
        let judged = acct.s.c_end(KIND_C_SPLIT, &s, &claimer, acct.value as i64, t0).unwrap();
        assert_eq!(judged.is_some(), want, "{what}: the model");
        // (Stated with the outputs of the rule where it has any; else with those of the split above.)
        let outs = judged.map(|o| o.outs).unwrap_or(o.outs);
        let st = Stated { position: acct.s.hash(), window: acct.s.dispute_daa, late: acct.s.t.late_daa, kind: KIND_C_SPLIT, t0, arg: s.arg(), outs, bond: acct.s.c_bond(), claimer, held: acct.value as i64 };
        let live = acct_stated(&w, &acct, &st, &s.said());
        end_answered(&w, what, &acct, &live, &s, want);
    };
    lie("the least the new account stands by", need(50 * KAS1), 2, xk(&w.w), T0, true);
    lie("a sompi less than the new account stands by", need(50 * KAS1) - 1, 2, xk(&w.w), T0, false);
    lie("the most that leaves the account that stays standing", 40 * KAS1 - need(100 * KAS1), 2, xk(&w.w), T0, true);
    lie("a sompi more than leaves the account that stays standing", 40 * KAS1 - need(100 * KAS1) + 1, 2, xk(&w.w), T0, false);
    lie("by another key than the user's", 10 * KAS1, 2, xk(&w.f), T0, false);
    lie("no leg named", 10 * KAS1, 0, xk(&w.w), T0, false);
    lie("nothing with the leg", 0, 2, xk(&w.w), T0, false);
    // An account of one leg is not made two.
    let one = cov(&w, &shard, &legs[..1], 40 * KAS1);
    assert!(one.s.c_state(KIND_C_SPLIT, 10 * KAS1, 1, &[P10], T0, one.value as i64, xk(&w.w)).is_err());
    // A leg that lost carries what it lost: at 12 the short of 5 has lost 10 KAS, and stands alone with 13.5 more.
    let up12 = [P10, P10 * 12 / 10];
    let least = NOTE_MIN + (60 * KAS1 * up + 9_999) / 10_000 + 10 * KAS1;
    assert!(acct.s.c_state(KIND_C_SPLIT, least, 2, &up12, T0, acct.value as i64, xk(&w.w)).is_ok());
    assert!(acct.s.c_state(KIND_C_SPLIT, least - 1, 2, &up12, T0, acct.value as i64, xk(&w.w)).is_err());

    // The new account is joined to another of its user's (a leg moved), or handed over (a part sold).
    let new_live = OLive { s: new.clone(), value: 10 * KAS1 as u64, id: acct.id, op: op(86), daa: 6_700 };
    let other = OLive { op: op(87), ..cov(&w, &shard, &[fleg(&w, 2, 3, LONG, P10)], 20 * KAS1) };
    let shard2 = OLive { s: shard.s.booked(&fleg(&w, 2, 3, LONG, P10)).unwrap(), ..shard.clone() };
    let fw = [fund(xk(&w.w), 37)];
    let (b, moved) = account_join(&w.lib, &other, &new_live, &shard2, &w.script("KpfTJoinC"), w.payer(&w.w, &fw)).unwrap();
    signed("the leg joined to another account: it moved", &b, &w.keys);
    assert_eq!((moved.legs.clone(), moved.a_margin, moved.b_min_counter), (vec![fleg(&w, 2, 3, LONG, P10), legs[1]], 30 * KAS1, new.b_min_counter));
    let ff = [fund(xk(&w.f), 36)];
    let (b, sold) = account_pass(&w.lib, &new_live, &w.script("KpfTPass"), xk(&w.f), 0, w.payer(&w.f, &ff)).unwrap();
    signed("the leg's account handed over: a part sold", &b, &w.keys);
    assert_eq!((sold.a_key, sold.legs.clone()), (xk(&w.f), vec![legs[1]]));
}

// ---------------------------------------------------------------- a gain taken out of a leg that stays (docs 16.3)

use kpf_protocol::covered::KIND_C_MARK;
use kpf_protocol::reserve::MarkNote;

fn mark_note(sg: &OSigned, w: &World, n: &MarkNote, daa: u64) -> OLive<MarkNote> {
    let (k, _) = out_of(sg, &hash_spk(&w.lib, &n.hash()));
    nth(sg, k, *n, daa)
}

/// The user of a covered account takes a gain out of a leg that stays: the leg is from now entered at a price
/// between its entry and the price stated, and what it gained up to there is asked of the shard. What it keeps
/// unrealized is the band a leaving takes, so that the mark and the leaving together ask what the leaving alone
/// would have. No unit moves, and the shard's equity is what it was at any price.
#[test]
fn a_gain_is_taken_out_of_a_leg_that_stays_and_the_band_is_taken_once() {
    use kpf_protocol::aobj::{outcome, AObj, Host};
    let w = world();
    let base = seeded(&w);
    let legs = two_legs(&w);
    let shard = OLive { s: base.s.booked(&legs[0]).unwrap().booked(&legs[1]).unwrap(), ..base.clone() };
    let acct = cov(&w, &shard, &legs, 40 * KAS1);
    let fw = [fund(xk(&w.w), 44)];
    let note_script = w.script("KpfTMarkNote");
    let (g2, up) = (2 * acct.s.t.g_bps, acct.s.t.mmr_bps + acct.s.t.g_bps);
    let band = |worth: i64| (worth * g2 + 9_999) / 10_000;

    // The long of 10 units from 10, at 12: its new entry is the nearest to 12 that leaves the band of 120 KAS
    // unrealized, and what it gained up to there is asked of the shard. The coins stay.
    let up12 = [P10 * 12 / 10, P10];
    let ne = acct.s.mark_entry(&legs[0], up12[0]).unwrap();
    let asked = 10 * ne - 100 * KAS1;
    assert!(ne < up12[0] && asked > 0 && 10 * (up12[0] - ne) >= band(120 * KAS1) && 10 * (up12[0] - ne - 1) < band(120 * KAS1));
    let (live1, said1, o1, sg) = c_applied(&w, "a gain taken out of a leg that stays", &acct, KIND_C_MARK, 0, 1, &up12, &w.w);
    let (after, note) = (o1.after.clone().unwrap(), o1.mark.unwrap());
    assert_eq!((said1.amount, o1.note.clone(), o1.split.clone(), o1.paid, o1.fee), (ne, None, None, 0, 0));
    assert_eq!((note, note.value()), (MarkNote { sid: shard.s.sid, leg: legs[0], entry: ne, key: xk(&w.w), asked }, NOTE_MIN));
    let marked = XLeg { entry: ne, ..legs[0] };
    assert_eq!((after.legs.clone(), after.a_margin, after.a_min_counter), (vec![marked, legs[1]], 40 * KAS1, 2));
    assert_eq!(after.b_min_counter, live1.s.t0 + 2 * (acct.s.t.late_daa + 2 * acct.s.dispute_daa), "it writes the account's time");
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (40 * KAS1 as u64, &hash_spk(&w.lib, &after.hash())));
    assert!(after.c_check().is_ok());
    end_answered(&w, "a mark", &acct, &live1, &said1, true);
    // (What is kept of it reads back, and the statement's outcome is the account and the note.)
    assert_eq!(AObj::decode(&AObj::Mark(note).encode()).unwrap(), AObj::Mark(note));
    assert_eq!(outcome(&Host::Account(acct.s.clone()), &live1.s, &said1.said()), vec![AObj::Account(after.clone()), AObj::Mark(note)]);

    // Its note is applied: the shard pays, what its legs were entered at takes the new entry, and its equity is
    // what it was at any price. No row moves.
    let note1 = mark_note(&sg, &w, &note, 6_700);
    assert_eq!(note1.value, NOTE_MIN as u64);
    let (b, next, rest) = mark_apply(&w.lib, &shard, &note1, &note_script, w.payer(&w.w, &fw)).unwrap();
    let sg = signed("the note that a leg was marked, applied", &b, &w.keys);
    assert!(pays(&sg, &xk(&w.w), asked) && rest.is_none());
    assert_eq!((next.held, next.rows, next.owed), (shard.s.held - asked, shard.s.rows, 0));
    assert_eq!(next.basis, base.s.booked(&marked).unwrap().booked(&legs[1]).unwrap().basis, "the books say the new entry");
    for p in [[P10 * 12 / 10, P10, 0, 0], [P10 / 2, P10 * 3, 0, 0], [P10 * 5, P10 / 3, 0, 0]] {
        assert_eq!(next.equity(&p), shard.s.equity(&p));
    }
    // (The shard written with what its legs were entered at as it was, or paying a sompi less: refused.)
    let b = mark_apply_as(&w.lib, &shard, &note1, Shard { basis: shard.s.basis, ..next }, asked, None, &note_script, w.payer(&w.w, &fw)).unwrap().0;
    assert!(!accepted(&b, &w.keys), "the books as they were");
    let b = mark_apply_as(&w.lib, &shard, &note1, Shard { held: next.held + 1, ..next }, asked - 1, None, &note_script, w.payer(&w.w, &fw)).unwrap().0;
    assert!(!accepted(&b, &w.keys), "a sompi less paid");
    // The band is taken once: the leg then leaves at the same price, and the two together ask what the leaving
    // alone asks. Its two notes end the books the same whichever is applied first.
    let (_, _, o) = after.c_state(KIND_C_LEAVE, 0, 1, &up12, after.b_min_counter, 40 * KAS1, xk(&w.w)).unwrap();
    let gone = o.note.unwrap();
    let alone = acct.s.c_state(KIND_C_LEAVE, 0, 1, &up12, T0, 40 * KAS1, xk(&w.w)).unwrap().2.note.unwrap();
    assert_eq!((asked + gone.asked, alone.asked, gone.legs.clone()), (20 * KAS1 - band(120 * KAS1), 20 * KAS1 - band(120 * KAS1), vec![marked]));
    let first = next.left(&gone).unwrap().0;
    let other = shard.s.left(&gone).unwrap().0.marked(&note).unwrap().0;
    assert_eq!((first, first.basis), (other, base.s.booked(&legs[1]).unwrap().basis), "whatever the order");

    // A shard that holds less than is asked pays what it holds and owes the rest; a called one pays nothing.
    let small = OLive { s: Shard { held: 5 * KAS1, ..shard.s }, value: (5 * KAS1 + SHARD_CARRIER) as u64, ..shard.clone() };
    let (b, next, rest) = mark_apply(&w.lib, &small, &note1, &note_script, w.payer(&w.w, &fw)).unwrap();
    let sg = signed("a mark's note to a shard that holds less", &b, &w.keys);
    assert!(pays(&sg, &xk(&w.w), 5 * KAS1));
    assert_eq!((next.held, next.owed, rest), (0, asked - 5 * KAS1, Some(Rest { sid: shard.s.sid, key: xk(&w.w), amount: asked - 5 * KAS1 })));
    let called = OLive { s: Shard { called: 6_400, ..shard.s }, ..shard.clone() };
    let (b, next, rest) = mark_apply(&w.lib, &called, &note1, &note_script, w.payer(&w.w, &fw)).unwrap();
    signed("a mark's note to a called shard", &b, &w.keys);
    assert_eq!((next.held, next.owed, rest.map(|r| r.amount)), (shard.s.held, asked, Some(asked)));
    // It is applied whatever the rows hold: to a shard with no unit of the leg's market.
    let (b, next, _) = mark_apply(&w.lib, &base, &note1, &note_script, w.payer(&w.w, &fw)).unwrap();
    signed("a mark's note to a shard whose rows have nothing of the leg", &b, &w.keys);
    assert_eq!((next.rows, next.basis), (base.s.rows, base.s.basis - asked));
    // To another shard it is not applied; and a note that asks other than the leg gained between its two entries
    // (a statement nobody disputed wrote it) is applied by nothing.
    let elsewhere = OLive { s: Shard { sid: [0x77; 32], ..shard.s }, ..shard.clone() };
    assert!(mark_apply(&w.lib, &elsewhere, &note1, &note_script, w.payer(&w.w, &fw)).is_err());
    for more in [1, -1, 50 * KAS1] {
        let lie = OLive { s: MarkNote { asked: asked + more, ..note }, ..note1.clone() };
        assert!(shard.s.marked(&lie.s).is_err());
        let said = Shard { held: shard.s.held - asked - more, basis: next_basis(&shard.s, &note), ..shard.s };
        let b = mark_apply_as(&w.lib, &shard, &lie, said, asked + more, None, &note_script, w.payer(&w.w, &fw)).unwrap().0;
        assert!(!accepted(&b, &w.keys), "a note that asks {more} more than the leg gained");
    }

    // A short that gained: 5 units from 10, at 8. Its new entry is above 8.
    let dn8 = [P10, P10 * 8 / 10];
    let ne2 = acct.s.mark_entry(&legs[1], dn8[1]).unwrap();
    let asked2 = 50 * KAS1 - 5 * ne2;
    assert!(ne2 > dn8[1] && asked2 > 0 && 5 * (ne2 - dn8[1]) >= band(40 * KAS1));
    let (live2, said2, o2, sg) = c_applied(&w, "a gain taken out of a short", &acct, KIND_C_MARK, 0, 2, &dn8, &w.w);
    let (after2, short_note) = (o2.after.clone().unwrap(), o2.mark.unwrap());
    assert_eq!((short_note.asked, short_note.entry, after2.legs[1].entry, after2.legs[0]), (asked2, ne2, ne2, legs[0]));
    end_answered(&w, "a mark of a short", &acct, &live2, &said2, true);
    let (b, next, _) = mark_apply(&w.lib, &shard, &mark_note(&sg, &w, &short_note, 6_700), &note_script, w.payer(&w.w, &fw)).unwrap();
    signed("the note of a marked short, applied", &b, &w.keys);
    for p in [[P10, P10 * 8 / 10, 0, 0], [P10 * 4, P10 / 4, 0, 0]] {
        assert_eq!((next.equity(&p), next.held), (shard.s.equity(&p), shard.s.held - asked2));
    }

    // The ends that are not the rule's.
    let lie = |what: &str, x: &OLive<Account>, a: &[i64; 2], ne: i64, skip: i64, claimer: [u8; 32], want: bool| {
        let rows = x.s.c_trace_marked(a, skip.max(1), ne).unwrap();
        let lc = kpf_protocol::covered::chain_of_one(&x.s.legs[(skip.max(1) - 1) as usize]);
        let s = CSaid { amount: ne, skip, lc, rows };
        let judged = x.s.c_end(KIND_C_MARK, &s, &claimer, x.value as i64, T0).unwrap();
        assert_eq!(judged.is_some(), want, "{what}: the model");
        // (Stated with the outputs of the rule where it has any; else with those of the first mark above.)
        let outs = judged.map(|o| o.outs).unwrap_or(o1.outs);
        let st = Stated { position: x.s.hash(), window: x.s.dispute_daa, late: x.s.t.late_daa, kind: KIND_C_MARK, t0: T0, arg: s.arg(), outs, bond: x.s.c_bond(), claimer, held: x.value as i64 };
        let live = acct_stated(&w, x, &st, &s.said());
        end_answered(&w, what, x, &live, &s, want);
    };
    lie("the nearest entry the band allows", &acct, &up12, ne, 1, xk(&w.w), true);
    lie("an entry a step nearer the price than the band allows", &acct, &up12, ne + 1, 1, xk(&w.w), false);
    lie("an entry at the price: no band", &acct, &up12, up12[0], 1, xk(&w.w), false);
    lie("an entry beyond the price", &acct, &up12, up12[0] * 2, 1, xk(&w.w), false);
    lie("an entry further from the price: less taken", &acct, &up12, P10 * 11 / 10, 1, xk(&w.w), true);
    lie("the leg's own entry: nothing gained up to it", &acct, &up12, P10, 1, xk(&w.w), false);
    lie("an entry below the leg's own: a loss is no mark", &acct, &up12, P10 * 9 / 10, 1, xk(&w.w), false);
    lie("no entry", &acct, &up12, 0, 1, xk(&w.w), false);
    lie("by another key than the user's", &acct, &up12, ne, 1, xk(&w.f), false);
    lie("no leg named", &acct, &up12, ne, 0, xk(&w.w), false);
    lie("every leg named", &acct, &up12, ne, ALL, xk(&w.w), false);
    lie("a leg that gained nothing beyond the band", &acct, &[P10 * 101 / 100, P10], P10 * 1_005 / 1_000, 1, xk(&w.w), false);
    let waits = OLive { s: Account { b_min_counter: T0 + 1, ..acct.s.clone() }, ..acct.clone() };
    lie("before the account's time", &waits, &up12, ne, 1, xk(&w.w), false);
    // The account stands as after a take: a note's least and the maintenance and tolerance of 170 KAS.
    let need = NOTE_MIN + (170 * KAS1 * up + 9_999) / 10_000;
    lie("the least coins that stand", &cov(&w, &shard, &legs, need), &up12, ne, 1, xk(&w.w), true);
    lie("a sompi less than stands", &cov(&w, &shard, &legs, need - 1), &up12, ne, 1, xk(&w.w), false);
    // (What another leg lost is counted alone: at 12 the short has lost 10 KAS.)
    let both = [P10 * 12 / 10, P10 * 12 / 10];
    let need = NOTE_MIN + (180 * KAS1 * up + 9_999) / 10_000 + 10 * KAS1;
    lie("the least coins that stand beside a leg that lost", &cov(&w, &shard, &legs, need), &both, ne, 1, xk(&w.w), true);
    lie("a sompi less, beside a leg that lost", &cov(&w, &shard, &legs, need - 1), &both, ne, 1, xk(&w.w), false);

    // The rows: right as a mark's; wrong as a leaving's, and wrong where the marked leg is said to keep more
    // than it does, or the other leg adds to that sum.
    let row_script = w.script("KpfTCRow");
    for (at, prev, leg) in [(1i64, [0u8; 32], legs[0]), (3, said1.rows[0].g, legs[1])] {
        let q = w.pointed(&live1, at);
        let (b, right) = c_row(&w.lib, &q, &said1, at, &prev, &leg, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
        assert!(right);
        signed(&format!("a mark's row at the point {at} answered right"), &b, &w.keys);
        let (b, _) = c_row(&w.lib, &q, &said1, at, &prev, &leg, &row_script, xk(&w.f), w.payer(&w.w, &fw), Some(false)).unwrap();
        assert!(!accepted(&b, &w.keys), "a mark's row at the point {at} answered wrong");
    }
    let mut kept_more = said1.rows.clone();
    kept_more[0].gain += KAS1;
    kept_more[1].gain += KAS1;
    let mut other_adds = said1.rows.clone();
    other_adds[1].gain += KAS1;
    let mut passed = said1.rows.clone();
    passed[0].h = [0; 32];
    for (what, rows, at, prev, leg) in [
        ("a leaving's rows", acct.s.c_trace(&up12, 1).unwrap(), 1i64, [0u8; 32], legs[0]),
        ("the marked leg said to keep more", kept_more, 1, [0u8; 32], legs[0]),
        ("the other leg adding to what the marked leg keeps", other_adds, 3, said1.rows[0].g, legs[1]),
        ("the marked leg passed by in the chain of the legs", passed, 1, [0u8; 32], legs[0]),
    ] {
        let bent = CSaid { rows, ..said1.clone() };
        let st = Stated { arg: bent.arg(), ..live1.s };
        let live = acct_stated(&w, &acct, &st, &bent.said());
        let q = w.pointed(&live, at);
        let (b, right) = c_row(&w.lib, &q, &bent, at, &prev, &leg, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
        assert!(!right, "{what}");
        signed(&format!("a mark with {what}: the row answered wrong"), &b, &w.keys);
        let (b, _) = c_row(&w.lib, &q, &bent, at, &prev, &leg, &row_script, xk(&w.f), w.payer(&w.w, &fw), Some(true)).unwrap();
        assert!(!accepted(&b, &w.keys), "a mark with {what}, answered right");
    }
}

/// What a shard's legs were entered at once a note of a mark is applied, whatever it asks.
fn next_basis(s: &Shard, n: &MarkNote) -> i64 {
    s.marked(&MarkNote { asked: 10 * n.entry - 100 * KAS1, ..*n }).unwrap().0.basis
}

/// Review 60, F108. A note that a leg was marked, not applied, ends a draw as a note that legs left does: the
/// shard holds coins that are asked of it. One the shard would not apply ends none.
#[test]
fn a_note_of_a_mark_that_is_not_applied_ends_a_draw() {
    let w = world();
    let base = bare(&w);
    // A user is short 10 units from 10: at 9 it marked the leg to 9.5 and is owed 5 KAS, which the shard holds.
    let leg = fleg(&w, 0, 10, SHORT, P10);
    let s = base.s.booked(&leg).unwrap();
    let shard = OLive { s, ..base.clone() };
    let at9 = [P10 * 9 / 10, 0, 0, 0];
    let (st, d) = s.draw(s.draw_max(&at9), at9, T0, shard.value as i64).unwrap();
    let live = w.stated(&shard, &st, &d.said());
    let f = [fund(xk(&w.w), 37)];
    let script = w.script("KpfTDrawNote");
    let n = MarkNote { sid: s.sid, leg, entry: P10 * 95 / 100, key: xk(&w.w), asked: 5 * KAS1 };
    assert!(s.marked(&n).is_ok());
    let note = OLive { s: n, value: NOTE_MIN as u64, id: shard.id, op: op(72), daa: 6_400 };
    let b = draw_note_mark(&w.lib, &live, &st, None, &s, &note, &script, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    let sg = signed("a draw ended by a note that a leg was marked", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (shard.value, &hash_spk(&w.lib, &s.hash())), "the shard again");
    assert_eq!(sg.tx.outputs[2].script_public_key, hash_spk(&w.lib, &n.hash()), "the note goes on as it is");
    // One that landed after the statement ends nothing.
    assert!(draw_note_mark(&w.lib, &live, &st, None, &s, &OLive { daa: 6_520, ..note.clone() }, &script, xk(&w.w), w.payer(&w.w, &f)).is_err());
    // One the shard would not apply ends none: it asks a sompi more or less than the leg gained, or is of a leg
    // that lost between its entries, or is to another shard.
    for lie in [MarkNote { asked: 5 * KAS1 + 1, ..n }, MarkNote { asked: 5 * KAS1 - 1, ..n }, MarkNote { entry: P10 * 105 / 100, ..n }, MarkNote { sid: [0x44; 32], ..n }] {
        let other = OLive { s: lie, ..note.clone() };
        assert!(draw_note_mark(&w.lib, &live, &st, None, &s, &other, &script, xk(&w.w), w.payer(&w.w, &f)).is_err());
        let b = draw_note_mark_as(&w.lib, &live, &st, None, &s, &other, &script, xk(&w.w), w.payer(&w.w, &f)).unwrap();
        assert!(!accepted(&b, &w.keys), "a note the shard would not apply ends a draw: {lie:?}");
    }
}

// ---------------------------------------------------------------- a token leg in a covered account (docs 16.12)

use kpf_protocol::covered::DIR_SPOT;

/// A token leg: `units` of a token priced by market `i`, held by the vault `[0x99; 32]`, at a discount of 20 %.
fn tleg(w: &World, i: usize, units: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: [0x99; 32], scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir: DIR_SPOT, entry: 2_000 }
}

/// What a token leg counts at a price: its worth less the discount, rounded down, and a sompi.
fn counted(units: i64, price: i64) -> i64 {
    units * price / 10_000 * 8_000 + 1
}

/// A token leg counts as coins, at its worth less its discount, for what leaves an account as coins and for its
/// user's standing; and nothing for the legs' worth. A statement of these kinds names no token leg, and "every
/// leg" is not while one is held.
#[test]
fn a_token_leg_counts_as_coins_at_a_discount_and_is_named_by_no_statement_of_these_kinds() {
    use kpf_protocol::covered::{c_row_judged_of, KIND_C_SPLIT};
    let w = world();
    let base = seeded(&w);
    // A long of 1,000 units from 10 (worth 10,000 KAS: 600 of maintenance and tolerance at these terms' rates,
    // whatever they are: `up` below) and 100 tokens priced by market 1, at 10: they count 800 KAS and a sompi.
    let long = fleg(&w, 0, 1_000, LONG, P10);
    let tok = tleg(&w, 1, 100);
    let shard = OLive { s: base.s.booked(&long).unwrap(), ..base.clone() };
    let acct = cov(&w, &shard, &[long, tok], 10 * KAS1);
    assert!(acct.s.c_check().is_ok());
    let up = acct.s.t.mmr_bps + acct.s.t.g_bps;
    let need = (10_000 * KAS1 * up + 9_999) / 10_000;
    let m = 10 * KAS1 - NOTE_MIN;
    let a = [P10, P10];
    let rows = acct.s.c_trace(&a, 0).unwrap();
    assert_eq!((rows[1].tk, rows[1].wa, rows[1].gain, rows[1].ea), (counted(100, P10), 10_000 * KAS1, 0, 0), "the tokens count in the sixth sum and in no other");
    assert!(counted(100, P10) + m > need, "the tokens carry the long");

    // A take: what the tokens count carries the legs, and the user takes its coins out; no more than the coins.
    let (live, said, o, sg) = c_applied(&w, "a take from an account that holds tokens", &acct, KIND_C_TAKE, m, 0, &a, &w.w);
    assert_eq!((o.after.clone().unwrap().a_margin, sg.tx.outputs[0].value), (NOTE_MIN, NOTE_MIN as u64));
    end_answered(&w, "a take the tokens carry", &acct, &live, &said, true);
    assert!(acct.s.c_state(KIND_C_TAKE, m + 1, 0, &a, T0, acct.value as i64, xk(&w.w)).is_err(), "a sompi more than the coins");
    // (Without the token leg the same account takes nothing.)
    let bare_acct = cov(&w, &shard, &[long], 10 * KAS1);
    assert!(bare_acct.s.c_state(KIND_C_TAKE, 1, 0, &a[..1], T0, bare_acct.value as i64, xk(&w.w)).is_err());
    // Where the tokens are worth less the test binds: at a price where they count what is asked less half the
    // coins, the user takes half its coins (and what the price's rounding leaves), no more.
    let p_low = (need - m / 2) / 80 / 100 * 100;
    let low = [P10, p_low];
    let most = m + counted(100, p_low) - need;
    assert!(most > 0 && most < m);
    let lie = |what: &str, amount: i64, want: bool| {
        let s = CSaid { amount, skip: 0, lc: [0; 32], rows: acct.s.c_trace(&low, 0).unwrap() };
        let judged = acct.s.c_end(KIND_C_TAKE, &s, &xk(&w.w), acct.value as i64, T0).unwrap();
        assert_eq!(judged.is_some(), want, "{what}: the model");
        let outs = judged.map(|o| o.outs).unwrap_or(o.outs);
        let st = Stated { position: acct.s.hash(), window: acct.s.dispute_daa, late: acct.s.t.late_daa, kind: KIND_C_TAKE, t0: T0, arg: s.arg(), outs, bond: acct.s.c_bond(), claimer: xk(&w.w), held: acct.value as i64 };
        let live = acct_stated(&w, &acct, &st, &s.said());
        end_answered(&w, what, &acct, &live, &s, want);
    };
    lie("the most the tokens leave", most, true);
    lie("a sompi more than the tokens leave", most + 1, false);

    // The rows: the token leg's row is right as it counts, and wrong where it counts nothing, or its worth whole,
    // or adds to the legs' worth.
    let fw = [fund(xk(&w.w), 44)];
    let row_script = w.script("KpfTCRow");
    for (at, prev, leg) in [(1i64, [0u8; 32], long), (3, said.rows[0].g, tok)] {
        let q = w.pointed(&live, at);
        let (b, right) = c_row(&w.lib, &q, &said, at, &prev, &leg, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
        assert!(right);
        signed(&format!("a row of an account that holds tokens, at the point {at}, answered right"), &b, &w.keys);
        let (b, _) = c_row(&w.lib, &q, &said, at, &prev, &leg, &row_script, xk(&w.f), w.payer(&w.w, &fw), Some(false)).unwrap();
        assert!(!accepted(&b, &w.keys), "the row at the point {at} answered wrong");
    }
    let good = &said.rows;
    let mut nothing = good.clone();
    nothing[1].tk = 0;
    let mut whole = good.clone();
    whole[1].tk = 1_000 * KAS1 + 1;
    let mut worth_too = good.clone();
    worth_too[1].wa += 1_000 * KAS1;
    let mut futures_adds = good.clone();
    futures_adds[0].tk = KAS1;
    futures_adds[1].tk += KAS1;
    for (what, rows, at, prev, leg) in [
        ("the tokens counting nothing", nothing, 3i64, good[0].g, tok),
        ("the tokens counted at their worth whole", whole, 3, good[0].g, tok),
        ("the tokens among the legs' worth", worth_too, 3, good[0].g, tok),
        ("a futures leg adding to what the tokens count", futures_adds, 1, [0u8; 32], long),
    ] {
        let bent = CSaid { rows, ..said.clone() };
        let st = Stated { arg: bent.arg(), ..live.s };
        let lv = acct_stated(&w, &acct, &st, &bent.said());
        let q = w.pointed(&lv, at);
        let (b, right) = c_row(&w.lib, &q, &bent, at, &prev, &leg, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
        assert!(!right, "{what}");
        signed(&format!("{what}: the row answered wrong"), &b, &w.keys);
        let (b, _) = c_row(&w.lib, &q, &bent, at, &prev, &leg, &row_script, xk(&w.f), w.payer(&w.w, &fw), Some(true)).unwrap();
        assert!(!accepted(&b, &w.keys), "{what}, answered right");
    }
    // A token leg is named by no leaving, step, split or mark: its row is wrong there, whatever it sums.
    for kind in [KIND_C_LEAVE, KIND_C_STEP, KIND_C_SPLIT, KIND_C_MARK] {
        let named = CSaid { amount: 0, skip: 2, lc: kpf_protocol::covered::chain_of_one(&tok), rows: good.clone() };
        assert_eq!(c_row_judged_of(kind, &good[0].g, &tok, &good[0], &good[1], 2, &named), Some(false));
    }
    let named = CSaid { amount: 0, skip: 2, lc: kpf_protocol::covered::chain_of_one(&tok), rows: good.clone() };
    let st = Stated { kind: KIND_C_LEAVE, arg: named.arg(), ..live.s };
    let lv = acct_stated(&w, &acct, &st, &named.said());
    let q = w.pointed(&lv, 3);
    let (b, right) = c_row(&w.lib, &q, &named, 3, &good[0].g, &tok, &row_script, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
    assert!(!right);
    signed("a leaving that names a token leg: its row answered wrong", &b, &w.keys);
    // (And a discount beyond the whole is no token leg's.)
    let odd = XLeg { entry: 10_001, ..tok };
    assert!(cov(&w, &shard, &[long, odd], 10 * KAS1).s.c_check().is_err());

    // The standing: at 9.8 the long has lost 200 KAS. With 10 KAS alone the user is a step's; with the tokens,
    // which count 800, it is not. At 9.5 (500 lost) it is, with the tokens too.
    let mild = [P10 * 98 / 100, P10];
    assert!(bare_acct.s.c_state(KIND_C_STEP, 0, 1, &mild[..1], T0, bare_acct.value as i64, xk(&w.f)).is_ok(), "a step's without the tokens");
    assert!(acct.s.c_state(KIND_C_STEP, 0, 1, &mild, T0, acct.value as i64, xk(&w.f)).is_err(), "not a step's with them");
    let down = [P10 * 95 / 100, P10];
    assert!(acct.s.c_state(KIND_C_STEP, 0, 1, &down, T0, acct.value as i64, xk(&w.f)).is_ok(), "a step's at 9.5, with the tokens too");
    // (Its coins do not pay the leg, and it holds tokens: the account is not taken whole. The leg leaves, the coins
    // pay what they can, and the account owes the rest: docs 16.18. A step of every leg is not stated of it.)
    let few = cov(&w, &shard, &[long, tleg(&w, 1, 10)], 10 * KAS1);
    assert!(few.s.c_trace(&down, 1).unwrap()[1].tk == counted(10, P10));
    assert!(few.s.c_state(KIND_C_STEP, 0, ALL, &down, T0, few.value as i64, xk(&w.f)).is_err(), "every leg, while tokens are held");
    let (live, said, o, sg) = c_applied(&w, "a step of a leg the coins do not pay, on an account that holds tokens", &few, KIND_C_STEP, 0, 1, &down, &w.f);
    let owing = o.after.clone().unwrap();
    let paid = 10 * KAS1 - NOTE_MIN;
    assert_eq!((owing.legs.clone(), owing.a_margin, owing.b_margin, o.fee, said.amount), (vec![tleg(&w, 1, 10)], NOTE_MIN, 500 * KAS1 - paid, 0, 0));
    assert_eq!(owing.b_min_counter, T0 + few.s.t.late_daa + few.s.dispute_daa, "it writes the account's time: a sale is stated at or after it");
    assert_eq!((o.note.clone().unwrap().asked, sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (-paid, NOTE_MIN as u64, &hash_spk(&w.lib, &owing.hash())));
    end_answered(&w, "a step that makes an account owe", &few, &live, &said, true);
    // (The same step said with a fee the coins do not pay, or of an account that then owes nothing: wrong.)
    assert_eq!(few.s.c_end(KIND_C_STEP, &CSaid { amount: KAS1, ..said.clone() }, &xk(&w.f), few.value as i64, T0), Some(None));
    // An account that owes is stepped again, and does nothing else of these kinds.
    let two_long = cov(&w, &shard, &[long, fleg(&w, 0, 100, LONG, P10), tleg(&w, 1, 10)], 10 * KAS1);
    let a3d = [P10 * 95 / 100, P10 * 95 / 100, P10];
    let (_, _, o1) = two_long.s.c_state(KIND_C_STEP, 0, 1, &a3d, T0, two_long.value as i64, xk(&w.f)).unwrap();
    let mid = o1.after.unwrap();
    assert!(mid.b_margin > 0 && mid.legs.len() == 2 && mid.c_check().is_ok());
    let (_, _, o2) = mid.c_state(KIND_C_STEP, 0, 1, &a3d[1..], T0, NOTE_MIN, xk(&w.f)).unwrap();
    assert_eq!(o2.after.unwrap().b_margin, mid.b_margin + 50 * KAS1, "what the second leg lost is owed too");
    assert!(mid.c_state(KIND_C_TAKE, 1, 0, &a3d[1..], T0 + 1_000, NOTE_MIN, xk(&w.w)).is_err() && mid.passed(xk(&w.f), 7_000).is_err() && mid.owes_nothing().is_err());

    // A split and a mark take what the tokens count as a take does.
    let short = fleg(&w, 1, 5, SHORT, P10);
    let two = cov(&w, &shard, &[long, short, tok], 10 * KAS1);
    let a3 = [P10, P10, P10];
    let need_short = NOTE_MIN + (50 * KAS1 * up + 9_999) / 10_000;
    let (live, said, o, _) = c_applied(&w, "a split of an account that holds tokens", &two, KIND_C_SPLIT, need_short, 2, &a3, &w.w);
    assert_eq!((o.after.clone().unwrap().legs, o.split.clone().unwrap().legs), (vec![long, tok], vec![short]), "the tokens stay with the account that stays");
    end_answered(&w, "a split the tokens carry", &two, &live, &said, true);
    assert!(cov(&w, &shard, &[long, short], 10 * KAS1).s.c_state(KIND_C_SPLIT, need_short, 2, &a3[..2], T0, 10 * KAS1, xk(&w.w)).is_err(), "not without them");
    let up12 = [P10 * 12 / 10, P10];
    let (live, said, o, _) = c_applied(&w, "a mark in an account that holds tokens", &acct, KIND_C_MARK, 0, 1, &up12, &w.w);
    assert!(o.mark.unwrap().asked > 0 && o.after.clone().unwrap().legs[1] == tok);
    end_answered(&w, "a mark the tokens carry", &acct, &live, &said, true);
    assert!(bare_acct.s.c_state(KIND_C_MARK, 0, 1, &up12[..1], T0, bare_acct.value as i64, xk(&w.w)).is_err(), "not without them");
}

// ---------------------------------------------------------------- tokens leave, or are sold in a step (docs 16.12)

use kpf_protocol::covered::{Ticket, KIND_C_TOK_OUT, KIND_C_TOK_SOLD};

/// A token leg leaves its account by the user's statement where the account stands without it, and is sold in a
/// step where a step is due and no step on a futures leg is: either writes the account without the leg and a
/// ticket, to the user or to the claimer, who pays the account the leg's worth less the liquidation fee's rate.
#[test]
fn tokens_leave_by_their_users_statement_or_are_sold_in_a_step_and_either_writes_a_ticket() {
    use kpf_protocol::aobj::{outcome, AObj, Host};
    use kpf_protocol::covered::{c_row_judged_of, chain_of_one};
    let w = world();
    let base = seeded(&w);
    let long = fleg(&w, 0, 1_000, LONG, P10);
    let tok = tleg(&w, 1, 100);
    let shard = OLive { s: base.s.booked(&long).unwrap(), ..base.clone() };
    let t = w.terms();
    let up = t.mmr_bps + t.g_bps;
    let need = (10_000 * KAS1 * up + 9_999) / 10_000;
    let a = [P10, P10];
    let fw = [fund(xk(&w.w), 44)];

    // While the tokens carry the long they do not leave. With coins that carry it alone, they do: the account goes
    // on without the leg, at the time a statement of its user's writes, and a ticket is the user's.
    let poor = cov(&w, &shard, &[long, tok], 10 * KAS1);
    assert!(poor.s.c_state(KIND_C_TOK_OUT, 0, 2, &a, T0, poor.value as i64, xk(&w.w)).is_err());
    let rich = cov(&w, &shard, &[long, tok], NOTE_MIN + need);
    let (live, said, o, sg) = c_applied(&w, "tokens leave", &rich, KIND_C_TOK_OUT, 0, 2, &a, &w.w);
    let (after, ticket) = (o.after.clone().unwrap(), o.ticket.unwrap());
    assert_eq!(ticket, Ticket { chain: chain_of_one(&tok), key: xk(&w.w) });
    assert_eq!((after.legs.clone(), after.a_margin, after.a_min_counter), (vec![long], NOTE_MIN + need, 1));
    assert_eq!(after.b_min_counter, live.s.t0 + 2 * (t.late_daa + 2 * rich.s.dispute_daa));
    assert_eq!((said.rows[1].eb, said.rows[1].tk, said.rows[1].h, said.rows[1].wa), (1_000 * KAS1, 0, said.rows[0].h, 10_000 * KAS1), "the leg's worth whole, counted for nothing, out of the chain");
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (rich.value, &hash_spk(&w.lib, &after.hash())));
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &ticket.hash())).1, NOTE_MIN as u64);
    end_answered(&w, "tokens that leave", &rich, &live, &said, true);
    assert_eq!(AObj::decode(&AObj::Ticket(ticket).encode()).unwrap(), AObj::Ticket(ticket));
    assert_eq!(outcome(&Host::Account(rich.s.clone()), &live.s, &said.said()), vec![AObj::Account(after.clone()), AObj::Ticket(ticket)]);

    // The price of the token leg's row is the shard's committee's: the citation is of the leg's market, and the
    // script opens the shard's id for the committee. The futures leg's is answered as ever.
    let (cite1, price) = (w.script("KpfTCite"), w.script("KpfTStepPrice"));
    for (point, i, c, want) in [(4i64, 1usize, P10 * 1_004 / 1_000, true), (4, 1, P10 * 102 / 100, false), (2, 0, P10 * 1_004 / 1_000, true)] {
        let q = w.pointed(&live, point);
        let cited = w.cited(i, c);
        let (b, right) = c_price_seeded(&w.lib, &q, &rich.s, &said, &price, (&cite1, &cited), SEED, xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
        assert_eq!(right, want);
        signed(&format!("the price of the row {} of an account that holds tokens, answered {}", i + 1, if want { "right" } else { "wrong" }), &b, &w.keys);
        let (b, _) = c_price_seeded(&w.lib, &q, &rich.s, &said, &price, (&cite1, &cited), SEED, xk(&w.f), w.payer(&w.w, &fw), Some(!want)).unwrap();
        assert!(!accepted(&b, &w.keys), "the other verdict on the price of the row {}", i + 1);
    }
    // (With another seed the shard's id is not opened: no citation answers the token's price.)
    assert!(c_price_seeded(&w.lib, &w.pointed(&live, 4), &rich.s, &said, &price, (&cite1, &w.cited(1, P10)), [0x11; 32], xk(&w.f), w.payer(&w.w, &fw), None).is_err());

    // The ends that are not the rule's; and the rows.
    let lie = |what: &str, x: &OLive<Account>, kind: i64, prices: &[i64; 2], amount: i64, skip: i64, claimer: [u8; 32], want: bool| {
        // (The token leg is the second of every account here: the trace names it, whatever the statement says.)
        let named = 2i64;
        let s = CSaid { amount, skip, lc: chain_of_one(&x.s.legs[named as usize - 1]), rows: x.s.c_trace_tok(kind, prices, named).unwrap() };
        let judged = x.s.c_end(kind, &s, &claimer, x.value as i64, T0).unwrap();
        assert_eq!(judged.is_some(), want, "{what}: the model");
        let outs = judged.map(|o| o.outs).unwrap_or(o.outs);
        let st = Stated { position: x.s.hash(), window: x.s.dispute_daa, late: x.s.t.late_daa, kind, t0: T0, arg: s.arg(), outs, bond: x.s.c_bond(), claimer, held: x.value as i64 };
        let live = acct_stated(&w, x, &st, &s.said());
        end_answered(&w, what, x, &live, &s, want);
    };
    lie("a sompi less than stands without the tokens", &cov(&w, &shard, &[long, tok], NOTE_MIN + need - 1), KIND_C_TOK_OUT, &a, 0, 2, xk(&w.w), false);
    lie("tokens taken out by another key", &rich, KIND_C_TOK_OUT, &a, 0, 2, xk(&w.f), false);
    lie("something where a take has its amount", &rich, KIND_C_TOK_OUT, &a, 1, 2, xk(&w.w), false);
    lie("no leg named", &rich, KIND_C_TOK_OUT, &a, 0, 0, xk(&w.w), false);
    lie("every leg named", &rich, KIND_C_TOK_OUT, &a, 0, ALL, xk(&w.w), false);
    let waits = OLive { s: Account { b_min_counter: T0 + 1, ..rich.s.clone() }, ..rich.clone() };
    lie("before the account's time", &waits, KIND_C_TOK_OUT, &a, 0, 2, xk(&w.w), false);
    // A futures leg named by these kinds: the model writes no such trace; its row is wrong, and so is the end,
    // which is shown the leg named and asks a token leg (review 62, F109).
    assert!(rich.s.c_trace_tok(KIND_C_TOK_OUT, &a, 1).is_none() && rich.s.c_state(KIND_C_TOK_OUT, 0, 1, &a, T0, rich.value as i64, xk(&w.w)).is_err());
    for kind in [KIND_C_TOK_OUT, KIND_C_TOK_SOLD] {
        let s = CSaid { amount: 0, skip: 1, lc: chain_of_one(&long), rows: rich.s.c_trace(&a, 0).unwrap() };
        assert_eq!(c_row_judged_of(kind, &[0; 32], &long, &kpf_protocol::thin::Row::ZERO, &s.rows[0], 1, &s), Some(false));
        let every = CSaid { skip: ALL, ..s.clone() };
        assert_eq!(c_row_judged_of(kind, &s.rows[0].g, &tok, &s.rows[0], &s.rows[1], 2, &every), Some(false));
    }
    let s = CSaid { amount: 0, skip: 1, lc: chain_of_one(&long), rows: rich.s.c_trace(&a, 0).unwrap() };
    assert_eq!(rich.s.c_end(KIND_C_TOK_OUT, &s, &xk(&w.w), rich.value as i64, T0), Some(None), "the leg named is no token leg");
    let lv = acct_stated(&w, &rich, &Stated { arg: s.arg(), ..live.s }, &s.said());
    end_answered(&w, "tokens leave, a futures leg named", &rich, &lv, &s, false);
    let (b, right) = c_row(&w.lib, &w.pointed(&lv, 1), &s, 1, &[0u8; 32], &long, &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
    assert!(!right);
    signed("tokens leave, a futures leg named: its row answered wrong", &b, &w.keys);
    let q = w.pointed(&live, 3);
    let (b, right) = c_row(&w.lib, &q, &said, 3, &said.rows[0].g, &tok, &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.w, &fw), None).unwrap();
    assert!(right);
    signed("tokens leave: the token leg's row answered right", &b, &w.keys);

    // An account that owes nothing is sold from by nobody; and one that owes takes no tokens out.
    assert!(rich.s.c_state(KIND_C_TOK_SOLD, 0, 2, &a, T0, rich.value as i64, xk(&w.f)).is_err());
}

/// An account that owes its shard (docs 16.18): anyone sells a token leg of it, the shard paid first; anyone pays
/// what it owes; and it does nothing else until it owes nothing.
#[test]
fn an_account_that_owes_is_sold_from_or_paid_for_and_does_nothing_else() {
    use kpf_protocol::covered::chain_of_one;
    use kpf_protocol::reserve::Owed;
    let w = world();
    let base = seeded(&w);
    let shard = base.clone();
    let tok = tleg(&w, 1, 10);
    let t = w.terms();
    // An account of 10 tokens (worth 100 KAS at 10) and a note's least of coins, that owes 30 KAS.
    let whole = cov(&w, &shard, &[tok], NOTE_MIN);
    let owes = 30 * KAS1;
    let ower = OLive { s: Account { b_margin: owes, ..whole.s.clone() }, ..whole.clone() };
    assert!(ower.s.c_check().is_ok() && ower.s.owes_nothing().is_err());
    let a = [P10];
    let pay = ower.s.tok_price(&tok, P10).unwrap();
    assert_eq!(pay, 100 * KAS1 - 100 * KAS1 / 10_000 * t.liq_fee_bps);
    // It takes nothing out, is handed to nobody and is entered by no leg.
    assert!(ower.s.c_state(KIND_C_TOK_OUT, 0, 1, &a, T0, ower.value as i64, xk(&w.w)).is_err());
    assert!(ower.s.passed(xk(&w.f), 7_000).is_err());
    let ff = [fund(xk(&w.f), 36)];
    assert!(account_pass(&w.lib, &ower, &w.script("KpfTPass"), xk(&w.f), 0, w.payer(&w.f, &ff)).is_err());

    // The sale: the claimer pays the leg's worth less the fee's rate; 30 KAS of it to the shard, by a note that
    // holds them; the rest is the account's coins. The account owes nothing after, and holds no leg.
    let (live, said, o, sg) = c_applied(&w, "a sale of a token leg of an account that owes", &ower, KIND_C_TOK_SOLD, 0, 1, &a, &w.f);
    let (after, ticket, owed) = (o.after.clone().unwrap(), o.ticket.unwrap(), o.owed.unwrap());
    assert_eq!((said.amount, ticket, owed), (pay, Ticket { chain: chain_of_one(&tok), key: xk(&w.f) }, Owed { sid: shard.s.sid, amount: owes }));
    assert_eq!((after.legs.len(), after.a_margin, after.b_margin, after.a_min_counter), (0, NOTE_MIN + pay - owes, 0, 0));
    assert_eq!((live.s.bond, live.value), (pay / 100, ower.value + (pay / 100) as u64), "a sale's bond is a hundredth of what it pays; the statement holds nothing of the payment");
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), ((NOTE_MIN + pay - owes) as u64, &hash_spk(&w.lib, &after.hash())));
    assert_eq!((out_of(&sg, &hash_spk(&w.lib, &ticket.hash())).1, out_of(&sg, &hash_spk(&w.lib, &owed.hash())).1), (NOTE_MIN as u64, (NOTE_MIN + owes) as u64));
    end_answered(&w, "a sale", &ower, &live, &said, true);
    let sale_lie = |what: &str, x: &OLive<Account>, amount: i64, claimer: [u8; 32], want: bool| {
        let s = CSaid { amount, skip: 1, lc: chain_of_one(&tok), rows: x.s.c_trace_tok(KIND_C_TOK_SOLD, &a, 1).unwrap() };
        let judged = x.s.c_end(KIND_C_TOK_SOLD, &s, &claimer, x.value as i64, T0).unwrap();
        assert_eq!(judged.is_some(), want, "{what}: the model");
        let outs = judged.map(|o| o.outs).unwrap_or(o.outs);
        let st = Stated { position: x.s.hash(), window: x.s.dispute_daa, late: x.s.t.late_daa, kind: KIND_C_TOK_SOLD, t0: T0, arg: s.arg(), outs, bond: x.s.c_bond_for(KIND_C_TOK_SOLD, amount), claimer, held: x.value as i64 };
        let live = acct_stated(&w, x, &st, &s.said());
        end_answered(&w, what, x, &live, &s, want);
    };
    sale_lie("a sompi less paid for the tokens", &ower, pay - 1, xk(&w.f), false);
    sale_lie("more paid for the tokens: the rest to the coins", &ower, pay + KAS1, xk(&w.f), true);
    sale_lie("a sale of an account that owes nothing", &whole, pay, xk(&w.f), false);
    sale_lie("a sale before the account's time", &OLive { s: Account { b_min_counter: T0 + 1, ..ower.s.clone() }, ..ower.clone() }, pay, xk(&w.f), false);
    // (Where more is owed than is paid, all of it is the shard's, and the account still owes.)
    let deep = OLive { s: Account { b_margin: 500 * KAS1, ..whole.s.clone() }, ..whole.clone() };
    let (_, _, od) = deep.s.c_state(KIND_C_TOK_SOLD, 0, 1, &a, T0, deep.value as i64, xk(&w.f)).unwrap();
    assert_eq!((od.after.clone().unwrap().b_margin, od.after.unwrap().a_margin, od.owed.unwrap().amount), (500 * KAS1 - pay, NOTE_MIN, pay));

    // The note of what was paid is applied to the shard: it holds that much more. One whose output holds less
    // gives what it holds.
    let fw = [fund(xk(&w.w), 44)];
    let owed_script = w.script("KpfTOwed");
    let (k, _) = out_of(&sg, &hash_spk(&w.lib, &owed.hash()));
    let note = nth(&sg, k, owed, 6_700);
    let (b, next) = owed_apply(&w.lib, &shard, &note, &owed_script, w.payer(&w.w, &fw)).unwrap();
    signed("the note of what an account paid, applied", &b, &w.keys);
    assert_eq!((next.held, next.rows, next.basis, next.owed), (shard.s.held + owes, shard.s.rows, shard.s.basis, shard.s.owed));
    let b = owed_apply_as(&w.lib, &shard, &note, Shard { held: next.held + 1, ..next }, &owed_script, w.payer(&w.w, &fw)).unwrap();
    assert!(!accepted(&b, &w.keys), "a sompi more than the note holds");
    let thin = OLive { value: (NOTE_MIN + 5 * KAS1) as u64, ..note.clone() };
    let (b, next) = owed_apply(&w.lib, &shard, &thin, &owed_script, w.payer(&w.w, &fw)).unwrap();
    signed("a note of a payment that holds less than it says", &b, &w.keys);
    assert_eq!(next.held, shard.s.held + 5 * KAS1);
    assert!(owed_apply(&w.lib, &OLive { s: Shard { sid: [0x77; 32], ..shard.s }, ..shard.clone() }, &note, &owed_script, w.payer(&w.w, &fw)).is_err());
    let called = OLive { s: Shard { called: 6_400, ..shard.s }, ..shard.clone() };
    let (b, next) = owed_apply(&w.lib, &called, &note, &owed_script, w.payer(&w.w, &fw)).unwrap();
    signed("a note of a payment applied to a called shard", &b, &w.keys);
    assert_eq!(next.held, shard.s.held + owes);

    // Anyone pays what an account owes: all of it, or a KAS or more.
    let (b, paid_up, n) = owed_pay(&w.lib, &ower, &owed_script, owes, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("what an account owes, paid", &b, &w.keys);
    assert_eq!((paid_up.b_margin, n, sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (0, Owed { sid: shard.s.sid, amount: owes }, ower.value, &hash_spk(&w.lib, &paid_up.hash())));
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &n.hash())).1, (NOTE_MIN + owes) as u64);
    assert!(paid_up.owes_nothing().is_ok() && paid_up.c_state(KIND_C_TOK_OUT, 0, 1, &a, T0, ower.value as i64, xk(&w.w)).is_ok(), "then its tokens leave by its user's statement");
    let (b, part, _) = owed_pay(&w.lib, &ower, &owed_script, KAS1, w.payer(&w.f, &ff)).unwrap();
    signed("a KAS of what an account owes, paid", &b, &w.keys);
    assert_eq!(part.b_margin, owes - KAS1);
    for amount in [KAS1 - 1, owes + 1, 0] {
        assert!(owed_pay(&w.lib, &ower, &owed_script, amount, w.payer(&w.f, &ff)).is_err());
        let said = Account { b_margin: owes - amount, ..ower.s.clone() };
        let b = owed_pay_as(&w.lib, &ower, &owed_script, amount, &said, &Owed { sid: shard.s.sid, amount }, w.payer(&w.f, &ff)).unwrap();
        assert!(!accepted(&b, &w.keys), "{amount} paid of {owes} owed");
    }
    assert!(owed_pay(&w.lib, &whole, &owed_script, KAS1, w.payer(&w.f, &ff)).is_err(), "an account that owes nothing is paid for by nobody");

    // A sale nobody applies: as old as a lateness and a dispute period it is ended by anyone, for half its bond.
    let fc = [fund(xk(&w.c), 46)];
    let life = live.s.sale_life() as u64;
    assert_eq!(life as i64, t.late_daa + ower.s.dispute_daa);
    let b = tok_lapse(&w.lib, &live, &w.script("KpfTCTok"), xk(&w.c), life, w.payer(&w.c, &fc)).unwrap();
    let lp = signed("a sale nobody applied, ended", &b, &w.keys);
    assert_eq!((lp.tx.outputs[0].value, &lp.tx.outputs[0].script_public_key, b.tx.inputs[0].sequence), (ower.value, &hash_spk(&w.lib, &ower.s.hash()), life));
    assert!(pays(&lp, &xk(&w.c), live.s.bond / 2));
    let b = tok_lapse(&w.lib, &live, &w.script("KpfTCTok"), xk(&w.c), life - 1, w.payer(&w.c, &fc)).unwrap();
    assert!(!accepted(&b, &w.keys), "a sale ended a block before its life is over");
}

/// Under a call an account that holds token legs is stepped as any, whatever its tokens are worth: its futures
/// legs leave by a note that names them alone, its coins pay what they can, and it goes on with its token legs,
/// owing the rest (docs 16.18). The step is never refused for the tokens: a call reaches its end.
#[test]
fn the_calls_step_takes_the_futures_legs_and_leaves_the_account_with_its_tokens_owing_the_rest() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let fee = base.s.t.keeper_fee_min;
    let (long, tok) = (fleg(&w, 0, 10, LONG, P10), tleg(&w, 1, 10));
    let books = base.s.booked(&long).unwrap();
    let shard = OLive { s: Shard { held: 20 * KAS1, ..books }, value: (20 * KAS1 + SHARD_CARRIER) as u64, ..base.clone() };
    // The call's price is 8: the long of 10 units from 10 has lost 20 KAS.
    let (called, call) = called_objs(&shard, [P10 * 8 / 10, 0, 0, 0], 6_600);
    let step_s = w.script("KpfTCStep");

    // An account of the long, 10 tokens and 5 KAS: the coins pay 4.8 (a note's least is the note's), and the
    // account goes on with its tokens, owing 15.2. It says the time of its landing, a lateness and a dispute period.
    let poor = cov(&w, &shard, &[long, tok], 5 * KAS1);
    let (b, st) = call_step(&w.lib, &poor, &poor.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("the call's step on an account that holds tokens and whose coins do not pay", &b, &w.keys);
    let next = st.after.clone().unwrap();
    let paid = 5 * KAS1 - NOTE_MIN;
    assert_eq!((st.gain, st.pay, st.user, st.note.legs.clone(), st.note.asked), (-20 * KAS1, paid, 0, vec![long], -paid));
    assert_eq!((next.legs.clone(), next.a_margin, next.b_margin, next.a_min_counter), (vec![tok], 0, 20 * KAS1 - paid, 1));
    assert_eq!(next.b_min_counter, poor.daa as i64 + poor.s.t.late_daa + poor.s.dispute_daa);
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &next.hash())).1, NOTE_MIN as u64, "the account that goes on holds a note's least, put in by whoever steps");
    assert_eq!(out_of(&sg, &hash_spk(&w.lib, &st.note.hash())).1, (NOTE_MIN + paid) as u64);
    assert!(next.c_check().is_ok() && next.owes_nothing().is_err());
    // (Its note is of the futures leg alone, and the called shard applies it.)
    let fw = [fund(xk(&w.w), 37)];
    let (k, v) = out_of(&sg, &hash_spk(&w.lib, &st.note.hash()));
    let note = OLive { s: st.note.clone(), value: v, id: shard.id, op: kaspa_consensus_core::tx::TransactionOutpoint { transaction_id: sg.tx.id(), index: k as u32 }, daa: 6_700 };
    let (b, after_note, rest) = left_apply(&w.lib, &called, &note, &w.script(left_script(1)), w.payer(&w.w, &fw)).unwrap();
    signed("the note of the call's step on such an account, applied", &b, &w.keys);
    assert_eq!((after_note.held, after_note.rows[0].gross, rest), (called.s.held + paid, 0, None));
    // What is written otherwise is refused: the account owing a sompi less, with a sompi of coins, not going on
    // at all, or the note naming the token leg too.
    for (what, lie) in [
        ("owing a sompi less", kpf_protocol::covered::CallStep { after: Some(Account { b_margin: next.b_margin - 1, ..next.clone() }), ..st.clone() }),
        ("said to hold a sompi of coins", kpf_protocol::covered::CallStep { after: Some(Account { a_margin: 1, ..next.clone() }), ..st.clone() }),
        ("at the time it had", kpf_protocol::covered::CallStep { after: Some(Account { b_min_counter: poor.s.b_min_counter, ..next.clone() }), ..st.clone() }),
        ("not going on", kpf_protocol::covered::CallStep { after: None, ..st.clone() }),
        ("its note naming the token leg", kpf_protocol::covered::CallStep { note: Left { legs: vec![long, tok], ..st.note.clone() }, ..st.clone() }),
    ] {
        let b = call_step_as(&w.lib, &poor, &poor.s, None, &call, &lie, &step_s, w.payer(&w.f, &ff)).unwrap();
        assert!(!accepted(&b, &w.keys), "the account {what}");
    }

    // One whose coins pay: it owes nothing and goes on with its tokens and what is left; its time is as it was.
    let rich = cov(&w, &shard, &[long, tok], 30 * KAS1);
    let (b, st) = call_step(&w.lib, &rich, &rich.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    signed("the call's step on an account that holds tokens and whose coins pay", &b, &w.keys);
    let next = st.after.clone().unwrap();
    assert_eq!((st.pay, st.user, next.a_margin, next.b_margin, next.b_min_counter, next.legs.clone()), (20 * KAS1, 0, 30 * KAS1 - 20 * KAS1 - NOTE_MIN - fee, 0, rich.s.b_min_counter, vec![tok]));
    assert!(next.owes_nothing().is_ok());
    // An account of token legs alone has nothing in the call's rows: no step of it.
    let only = cov(&w, &shard, &[tok], 5 * KAS1);
    assert!(call_step(&w.lib, &only, &only.s, None, &call, &step_s, w.payer(&w.f, &ff)).is_err());
    // An account that owed before owes that and what this step adds.
    let owed_before = OLive { s: Account { b_margin: 7 * KAS1, ..poor.s.clone() }, ..poor.clone() };
    let (b, st) = call_step(&w.lib, &owed_before, &owed_before.s, None, &call, &step_s, w.payer(&w.f, &ff)).unwrap();
    signed("the call's step on an account that owed already", &b, &w.keys);
    assert_eq!(st.after.unwrap().b_margin, 7 * KAS1 + 20 * KAS1 - paid);
}
