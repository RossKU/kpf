//! A stated change of a position and its answer (`KpfTChange`, `KpfTChangeSettle`, `KpfTChangeDispute`,
//! `KpfTChangeLate`, `KpfTTake`; docs/ARCHITECTURE-CLEARING.md 4): a party states that the position's entries move to
//! prices of its own and that it takes out what it then has above its floor. Nothing is computed when it is stated;
//! undisputed it is applied after the dispute period, disputed it is answered by the committee's prices, which the
//! answer reads in citations of price notes. Every transaction is built, signed and run through `verify::verify`.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cite;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::cross::{Cross, XLeg, XTerms, SIDE_A, SIDE_B};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::{take_outs, Change, Out, Outs, Position, Take, ORACLE_FEE};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{p2pk_spk, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use std::cell::RefCell;

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;
const P10: i64 = 1_000_000_000;
const P12: i64 = 1_200_000_000;

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
/// Built, signed, accepted by the engine, and with a sompi less at any output a script fixes refused.
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
    a: Keypair,
    b: Keypair,
    w: Keypair,
    f: Keypair,
    members: [Keypair; 3],
    keys: KeySet,
    rid: Hash,
    markets: [CMarket; 2],
    table: OLive<EpochTable>,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
    /// The chain blocks made so far, with their commitments.
    chain: RefCell<Vec<(Hash, Hash)>>,
}

impl World {
    fn script(&self, name: &str) -> LibScript {
        self.copy(name, 0)
    }
    fn copy(&self, name: &str, k: usize) -> LibScript {
        self.scripts.iter().filter(|s| s.s == name).nth(k).cloned().unwrap_or_else(|| panic!("{name}"))
    }
    fn payer<'a>(&self, who: &Keypair, funds: &'a [KeyUtxo]) -> Payer<'a> {
        Payer { funds, change_to: xk(who), rate: MIN_FEE_RATE }
    }
    /// A position of A against B: long 10 units of market 0 at 10, and with `two` short 5 units of market 1 at 10;
    /// A's margin 40 KAS, B's 60.
    fn position(&self, two: bool) -> OLive<Position> {
        let (m, n) = (&self.markets[0], &self.markets[1]);
        let leg = |m: &CMarket, size: i64, dir: i64| XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: size * m.p.scale, dir, entry: P10 };
        let p = &m.p;
        let t = XTerms { term: p.term, mmr_bps: p.mmr_bps, g_bps: p.g_bps, liq_fee_bps: p.liq_fee_bps, keeper_fee_min: p.keeper_fee_min, carrier: p.carrier, idle_daa: p.idle_daa, grace_daa: p.grace_daa, late_daa: p.late_daa, quiet_daa: p.quiet_daa };
        let x = Cross { t, legs: [leg(m, 10, LONG), if two { leg(n, 5, SHORT) } else { XLeg::none() }], a_key: xk(&self.a), b_key: xk(&self.b), a_margin: 40 * KAS1, b_margin: 60 * KAS1, expiry: 6_000 + p.term };
        let position = Position { x, dispute_daa: 50, bond_bps: 1_000, a_min_counter: 0, b_min_counter: 0 };
        position.check().unwrap();
        OLive { s: position, value: x.floor() as u64, id: Some(Hash::from_bytes([0x71; 32])), op: op(10), daa: 6_100 }
    }
    /// The committee's price of market `i` for T0 written as a note and accepted by a chain block: its citation.
    fn cited(&self, i: usize, price: i64) -> Cited {
        let market = self.markets[i].id();
        let signed_prices: Vec<SignedPrice> = self.members.iter().map(|k| sign_price(k, &self.rid.as_bytes(), &market, T0, price)).collect();
        let att = Attestation::collect(&self.table.s, &self.rid.as_bytes(), &market, T0, &signed_prices).expect("a quorum");
        let funds = vec![fund(xk(&self.f), 90 + i as u8)];
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
    /// The take of `side` stated on the position, landed at `landed`.
    fn stated(&self, position: &OLive<Position>, tk: Take, landed: u64) -> OLive<Change> {
        let who = if tk.side == SIDE_A { &self.a } else { &self.b };
        let f = [fund(xk(who), 30)];
        let (b, c) = take(&self.lib, position, &self.script("KpfTChange"), tk, T0, self.payer(who, &f)).unwrap();
        let s = signed("take stated", &b, &self.keys);
        nth(&s, 0, c, landed)
    }
    fn disputed(&self, c: &OLive<Change>) -> OLive<Change> {
        let f = [fund(xk(&self.w), 31)];
        let (b, d) = change_dispute(&self.lib, c, &self.script("KpfTChangeDispute"), xk(&self.w), self.payer(&self.w, &f)).unwrap();
        let s = signed("change disputed", &b, &self.keys);
        nth(&s, 0, d, c.daa + 10)
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
    let markets = [CMarket::testnet(rid.as_bytes(), [0xfe; 32]), CMarket::testnet(rid.as_bytes(), [0xfd; 32])];
    let mut table = EMPTY_TABLE;
    for (i, wt) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: wt * KAS1 };
    }
    let table = OLive { s: EpochTable { members: table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(20), daa: 6_000 };
    // (Two copies of each of fifty scripts at 10 KAS: more than one coin of the funder's.)
    let funds = vec![fund(xk(&f), 200), fund(xk(&f), 202)];
    // (At 10 KAS a copy the library's genesis is above a block's storage mass since it has 63 scripts: 11.)
    let (g, lib) = lib_genesis(2, lib_copy_least(2), Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    World { a, b, w, f, members, keys, rid, markets, table, lib, scripts: lib_scripts(&gs.tx, 6_000), chain: RefCell::new(vec![]) }
}

/// What A is owed of the position at the price `q` of market 0 (market 1 at 10).
fn owed_a(x: &Position, q: i64) -> i64 {
    x.x.owed_at(SIDE_A, [q, P10])
}

#[test]
fn a_take_is_stated_small_and_applied_after_the_dispute_period() {
    let w = world();
    let position = w.position(false);
    let x = position.s;
    // The price is 12: A gained 20 KAS on 10 units. It is owed 60 and keeps the maintenance, 5 % of 120.
    assert_eq!(x.take_max(SIDE_A, [P12, 0]), 54 * KAS1);
    let tk = Take { side: SIDE_A, a: [P12, 0], amount: 54 * KAS1 };
    let f = [fund(xk(&w.a), 30)];
    let (b, c) = take(&w.lib, &position, &w.script("KpfTChange"), tk, T0, w.payer(&w.a, &f)).unwrap();
    let s = signed("take stated", &b, &w.keys);
    assert_eq!(c.bond, 10 * KAS1, "a tenth of the pot");
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (position.value + c.bond as u64, &object_spk(&w.lib, &kpf_protocol::thin::TObj::Change(c))));
    // Stated by A alone: without its signature, or as another's statement, refused.
    let mut keys_b = KeySet::new();
    keys_b.add_keypair(w.b);
    keys_b.add_keypair(w.f);
    assert!(b.sign(&keys_b).is_err() || !accepted(&b, &keys_b), "a take stated without the claimer's signature");
    let live = nth(&s, 0, c, 6_510);

    // After the dispute period anyone applies it: the position as stated, the amount and the bond to A.
    let after = x.taken(&tk).unwrap();
    assert_eq!((after.x.legs[0].entry, after.x.a_margin, after.x.b_margin), (P12, 6 * KAS1, 40 * KAS1));
    assert!(c.becomes(&after));
    let f = [fund(xk(&w.w), 31)];
    let b = change_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).unwrap();
    let st = signed("change settled", &b, &w.keys);
    assert_eq!((st.tx.outputs[0].value, &st.tx.outputs[0].script_public_key), (position.value - 54 * KAS1 as u64, &object_spk(&w.lib, &kpf_protocol::thin::TObj::Position(after))));
    let n = st.tx.outputs.len();
    assert_eq!((st.tx.outputs[n - 1].value, &st.tx.outputs[n - 1].script_public_key), (54 * KAS1 as u64, &p2pk_spk(&xk(&w.a))), "the amount, at the last output");
    assert_eq!((st.tx.outputs[n - 2].value, &st.tx.outputs[n - 2].script_public_key), (c.bond as u64, &p2pk_spk(&xk(&w.a))), "the bond, before it");
    // Before the dispute period is over: refused (the object is asked its age).
    let mut t = b.clone();
    t.tx.inputs[0].sequence = 49;
    assert!(!accepted(&t, &w.keys), "a change is applied before its dispute period is over");
    // The amount to another key, or the position with another state: refused.
    let mut t = b.clone();
    t.tx.outputs[n - 1].script_public_key = p2pk_spk(&xk(&w.w));
    assert!(!accepted(&t, &w.keys), "the amount is paid to another key");
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = object_spk(&w.lib, &kpf_protocol::thin::TObj::Position(x));
    assert!(!accepted(&t, &w.keys), "the position goes on with another state than the one stated");

    // Nobody's worth changed at any price, but for what A took: at every price where A is still owed at least
    // that, A is owed the amount less and B the same.
    for q in [P10 * 6 / 10, P10, P12, P12 * 2, P12 * 5] {
        let (before, now) = (owed_a(&x, q), owed_a(&after, q));
        if before >= 54 * KAS1 {
            assert_eq!(now + 54 * KAS1, before, "A at {q}");
            assert_eq!(after.pot() - now, x.pot() - before, "B at {q}");
        } else {
            assert_eq!(now, 0, "below what it took, A is owed nothing more");
        }
    }
}

#[test]
fn a_disputed_take_is_answered_by_cited_prices_and_never_both_ways() {
    let w = world();
    let position = w.position(false);
    let tk = Take { side: SIDE_A, a: [P12, 0], amount: 54 * KAS1 };
    let d = w.disputed(&w.stated(&position, tk, 6_510));
    let c1 = w.cited(0, P12);
    let cite1 = w.script("KpfTCite");
    let f = [fund(xk(&w.w), 32)];
    let (b, within, after) = take_answer(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &c1)], xk(&w.w), w.payer(&w.w, &f)).unwrap();
    assert!(within);
    let s = signed("take proved", &b, &w.keys);
    assert_eq!(s.tx.inputs.len(), 4, "the object, the script, the citation and a coin");
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (position.value - 54 * KAS1 as u64, &object_spk(&w.lib, &kpf_protocol::thin::TObj::Position(after))));
    // The claimer: its amount, its bond and half of the disputer's; the other half stays in the script's copy.
    let n = s.tx.outputs.len();
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), ((54 + 10 + 5) * KAS1 as u64, &p2pk_spk(&xk(&w.a))));
    assert_eq!(s.tx.outputs[1].value, w.script("KpfTTake").value + (ORACLE_FEE + 5 * KAS1) as u64);
    assert_eq!(s.tx.outputs[2].value, cite1.value + ORACLE_FEE as u64, "the citing copy goes on");
    // The same statement said not to be within the rule: refused by the script.
    let r = take_answer_with(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &c1)], xk(&w.w), w.payer(&w.w, &f), Some(false));
    assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "a take within the rule is refuted");

    // The committee's price is 11 (A stated 12, beyond the tolerance, and took what 12 gives): refuted.
    let c1 = w.cited(0, P10 * 11 / 10);
    let (b, within, after) = take_answer(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &c1)], xk(&w.w), w.payer(&w.w, &f)).unwrap();
    assert!(!within);
    assert_eq!(after, position.s);
    let s = signed("take refuted", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (position.value, &object_spk(&w.lib, &kpf_protocol::thin::TObj::Position(position.s))), "the position again");
    let n = s.tx.outputs.len();
    assert_eq!((s.tx.outputs[n - 2].value, &s.tx.outputs[n - 2].script_public_key), (10 * KAS1 as u64, &p2pk_spk(&xk(&w.w))), "the disputer's bond back");
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), (5 * KAS1 as u64, &p2pk_spk(&xk(&w.w))), "half of the bond lost, to who answered");
    assert_eq!(s.tx.outputs[1].value, w.script("KpfTTake").value + (ORACLE_FEE + 5 * KAS1) as u64);
    let r = take_answer_with(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &c1)], xk(&w.w), w.payer(&w.w, &f), Some(true));
    assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "a take not within the rule is proved");

    // A citation of another market, or of another time, answers nothing.
    let other = w.cited(1, P12);
    assert!(take_answer(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &other)], xk(&w.w), w.payer(&w.w, &f)).is_err());
    // An input that is not a copy of KpfTCite at the citation's place (the note's writer, whose signature script
    // begins otherwise, or no library script at all): refused by the script.
    let good = w.cited(0, P12);
    let (b, ..) = take_answer(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &good)], xk(&w.w), w.payer(&w.w, &f)).unwrap();
    assert!(accepted(&b, &w.keys));
    let mut t = b.clone();
    t.tx.inputs.swap(2, 3);
    t.entries.swap(2, 3);
    t.plans.swap(2, 3);
    assert!(!accepted(&t, &w.keys), "the place of the citation names a coin");
}

#[test]
fn what_is_stated_against_the_rule_is_refuted() {
    let w = world();
    let position = w.position(false);
    let x = position.s;
    let held = position.value as i64;
    let c12 = w.cited(0, P12);
    let cite1 = w.script("KpfTCite");
    let good = Take { side: SIDE_A, a: [P12, 0], amount: 54 * KAS1 };
    let honest = x.take(good, T0, held).unwrap();
    let after = x.taken(&good).unwrap();
    let evil = |outs: &[Out]| Outs::of(outs).unwrap();
    let cases: Vec<(&str, Take, Change)> = vec![
        // A sompi more than the floor leaves.
        ("a sompi too much", Take { amount: 54 * KAS1 + 1, ..good }, x.take(Take { amount: 54 * KAS1 + 1, ..good }, T0, held).unwrap()),
        // The amount stated is right, the outputs pay A more and leave the position less.
        ("outputs that pay more than the amount", good, Change { outs: evil(&[Out { key: false, to: after.hash(), value: held - 60 * KAS1 }, Out { key: true, to: xk(&w.a), value: 60 * KAS1 }]), ..honest }),
        // The position goes on with a state of the claimer's own making (B's margin its own).
        ("another state than the take's", good, Change { outs: evil(&[Out { key: false, to: Position { x: Cross { a_margin: 46 * KAS1, b_margin: 0, ..after.x }, ..after }.hash(), value: held - 54 * KAS1 }, Out { key: true, to: xk(&w.a), value: 54 * KAS1 }]), ..honest }),
        // The position ends and everything is paid to A.
        ("the pot taken whole", good, Change { outs: evil(&[Out { key: true, to: xk(&w.a), value: held }]), ..honest }),
        // B's take, stated by A as its own.
        ("the other side's take", Take { side: SIDE_B, a: [P12, 0], amount: KAS1 }, Change { claimer: xk(&w.a), ..x.take(Take { side: SIDE_B, a: [P12, 0], amount: KAS1 }, T0, held).unwrap() }),
        // A price a tenth away from the committee's with nothing taken: the entries are not moved from the truth.
        ("entries moved away", Take { side: SIDE_A, a: [P10 * 13 / 10, 0], amount: 0 }, x.take(Take { side: SIDE_A, a: [P10 * 13 / 10, 0], amount: 0 }, T0, held).unwrap()),
    ];
    for (i, (what, tk, c)) in cases.into_iter().enumerate() {
        let who = if c.claimer == xk(&w.a) { &w.a } else { &w.b };
        let f = [fund(xk(who), 30)];
        let b = change_state(&w.lib, &position, &w.script("KpfTChange"), &c, w.payer(who, &f)).unwrap();
        let s = signed(&format!("stated: {what}"), &b, &w.keys);
        let live = nth(&s, 0, c, 6_510);
        assert!(!x.take_within(&tk, &c.claimer, &c.outs, held, [P12, 0]), "{what}");
        let f = [fund(xk(&w.w), 33)];
        // Answered as it stands, by whoever holds the citation: no dispute is needed (odd cases), or after one.
        let live = if i % 2 == 0 { w.disputed(&live) } else { live };
        let (b, within, _) = take_answer(&w.lib, &live, &w.script("KpfTTake"), &[(&cite1, &c12)], xk(&w.w), w.payer(&w.w, &f)).unwrap();
        assert!(!within, "{what}");
        let r = signed(&format!("refuted: {what}"), &b, &w.keys);
        assert_eq!((r.tx.outputs[0].value, &r.tx.outputs[0].script_public_key), (position.value, &object_spk(&w.lib, &kpf_protocol::thin::TObj::Position(x))), "{what}: the position again");
        let forced = take_answer_with(&w.lib, &live, &w.script("KpfTTake"), &[(&cite1, &c12)], xk(&w.w), w.payer(&w.w, &f), Some(true));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "{what}: proved");
    }
    let _ = take_outs;
}

#[test]
fn a_statement_is_answered_at_once_and_one_out_of_its_time_only_ends() {
    let w = world();
    let position = w.position(false);
    let tk = Take { side: SIDE_A, a: [P12, 0], amount: 54 * KAS1 };
    let c12 = w.cited(0, P12);
    let cite1 = w.script("KpfTCite");
    // Stated and answered by A itself with the committee's price: applied without waiting, the bond back.
    let live = w.stated(&position, tk, 6_510);
    let f = [fund(xk(&w.a), 34)];
    let (b, within, after) = take_answer(&w.lib, &live, &w.script("KpfTTake"), &[(&cite1, &c12)], xk(&w.a), w.payer(&w.a, &f)).unwrap();
    assert!(within);
    let s = signed("take answered at once", &b, &w.keys);
    let n = s.tx.outputs.len();
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), ((54 + 10) * KAS1 as u64, &p2pk_spk(&xk(&w.a))));
    assert_eq!(s.tx.outputs[1].value, w.script("KpfTTake").value + ORACLE_FEE as u64, "nothing is lost: only the oracle fee stays");
    assert_eq!(s.tx.outputs[0].script_public_key, object_spk(&w.lib, &kpf_protocol::thin::TObj::Position(after)));

    // A statement for T0 that landed 101 DAA after it (the lateness is 100): the price of T0 is not the price of
    // now. It is not settled, not disputed, not answered; anyone ends it and has its bond.
    let stale = w.stated(&position, tk, (T0 + 101) as u64);
    let f = [fund(xk(&w.w), 35)];
    assert!(change_settle(&w.lib, &stale, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).is_err());
    assert!(change_dispute(&w.lib, &stale, &w.script("KpfTChangeDispute"), xk(&w.w), w.payer(&w.w, &f)).is_err());
    assert!(take_answer(&w.lib, &stale, &w.script("KpfTTake"), &[(&cite1, &c12)], xk(&w.a), w.payer(&w.w, &f)).is_err());
    // (The scripts refuse them too: the same transactions built for a statement in time, spent on the stale one.)
    let in_time = OLive { daa: 6_510, ..stale.clone() };
    for (what, b) in [
        ("settled", change_settle(&w.lib, &in_time, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).unwrap()),
        ("disputed", change_dispute(&w.lib, &in_time, &w.script("KpfTChangeDispute"), xk(&w.w), w.payer(&w.w, &f)).unwrap().0),
        ("answered", take_answer(&w.lib, &in_time, &w.script("KpfTTake"), &[(&cite1, &c12)], xk(&w.a), w.payer(&w.w, &f)).unwrap().0),
    ] {
        assert!(accepted(&b, &w.keys), "{what}, in time");
        let mut t = b.clone();
        t.entries[0].block_daa_score = (T0 + 101) as u64;
        assert!(!accepted(&t, &w.keys), "a statement out of its time is {what}");
    }
    let (b, again) = change_late(&w.lib, &stale, &w.script("KpfTChangeLate"), xk(&w.w), w.payer(&w.w, &f)).unwrap();
    assert_eq!(again, position.s);
    let s = signed("statement ended as late", &b, &w.keys);
    let n = s.tx.outputs.len();
    // (Half of the bond of 10 KAS to whoever ends it; the other half stays in the script's copy: review 58, F105.)
    assert_eq!((s.tx.outputs[0].value, s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), (position.value, 5 * KAS1 as u64, &p2pk_spk(&xk(&w.w))));
    // One in time is not ended as late.
    assert!(change_late(&w.lib, &in_time, &w.script("KpfTChangeLate"), xk(&w.w), w.payer(&w.w, &f)).is_err());
    let mut t = b.clone();
    t.entries[0].block_daa_score = 6_510;
    assert!(!accepted(&t, &w.keys), "a statement in time is ended as late");
}

#[test]
fn a_dispute_nobody_answers_lapses_and_both_bonds_return() {
    let w = world();
    let position = w.position(false);
    let tk = Take { side: SIDE_A, a: [P12, 0], amount: 54 * KAS1 };
    let d = w.disputed(&w.stated(&position, tk, 6_510));
    let f = [fund(xk(&w.w), 36)];
    // A disputed change is not settled.
    assert!(change_settle(&w.lib, &d, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).is_err());
    let (b, again) = change_lapse(&w.lib, &d, &w.script("KpfTChangeDispute"), w.payer(&w.w, &f)).unwrap();
    assert_eq!(again, position.s);
    let s = signed("dispute lapsed", &b, &w.keys);
    let n = s.tx.outputs.len();
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (position.value, &object_spk(&w.lib, &kpf_protocol::thin::TObj::Position(position.s))));
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), (10 * KAS1 as u64, &p2pk_spk(&xk(&w.w))));
    assert_eq!((s.tx.outputs[n - 2].value, &s.tx.outputs[n - 2].script_public_key), (10 * KAS1 as u64, &p2pk_spk(&xk(&w.a))));
    let mut t = b.clone();
    t.tx.inputs[0].sequence = 49;
    assert!(!accepted(&t, &w.keys), "a dispute lapses before its period is over");
}

#[test]
fn two_legs_take_two_citations_and_b_takes_as_a_does() {
    let w = world();
    let position = w.position(true);
    let x = position.s;
    // Market 0 fell to 9 and market 1 (which A is short) rose to 11: B gained 10 + 5.
    let c = [P10 * 9 / 10, P10 * 11 / 10];
    let max = x.take_max(SIDE_B, c);
    assert!(max > 0);
    let tk = Take { side: SIDE_B, a: c, amount: max };
    let d = w.disputed(&w.stated(&position, tk, 6_510));
    let (c1, c2) = (w.cited(0, c[0]), w.cited(1, c[1]));
    let (cite1, cite2) = (w.copy("KpfTCite", 0), w.copy("KpfTCite", 1));
    let f = [fund(xk(&w.w), 37)];
    let (b, within, after) = take_answer(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &c1), (&cite2, &c2)], xk(&w.w), w.payer(&w.w, &f)).unwrap();
    assert!(within);
    assert_eq!((after.x.legs[0].entry, after.x.legs[1].entry), (c[0], c[1]));
    let s = signed("two-leg take proved", &b, &w.keys);
    assert_eq!(s.tx.inputs.len(), 5, "the object, the script, two citations and a coin");
    // One citation alone, the same citation twice, or the two the wrong way round: refused.
    assert!(take_answer(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &c1)], xk(&w.w), w.payer(&w.w, &f)).is_err());
    assert!(take_answer(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &c2), (&cite2, &c1)], xk(&w.w), w.payer(&w.w, &f)).is_err());
    // A sompi more: refuted with the same two citations.
    let more = Take { amount: max + 1, ..tk };
    let d = w.disputed(&w.stated(&position, more, 6_510));
    let (b, within, _) = take_answer(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &c1), (&cite2, &c2)], xk(&w.w), w.payer(&w.w, &f)).unwrap();
    assert!(!within);
    signed("two-leg take refuted", &b, &w.keys);
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

#[test]
fn the_answer_agrees_with_the_model_on_random_takes() {
    let w = world();
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let (cite1, cite2) = (w.copy("KpfTCite", 0), w.copy("KpfTCite", 1));
    let (mut yes, mut no) = (0, 0);
    for k in 0..36 {
        let two = k % 3 == 0;
        let mut position = w.position(two);
        position.s.x.a_margin = r.range(5, 80) * KAS1;
        position.s.x.b_margin = r.range(5, 80) * KAS1;
        position.s.a_min_counter = r.range(0, 20) * KAS1;
        position.s.b_min_counter = r.range(0, 20) * KAS1;
        if position.s.check().is_err() {
            continue;
        }
        position.value = position.s.x.floor() as u64;
        let x = position.s;
        // The committee's prices, and the claimer's own within or beyond the tolerance.
        let c = [P10 * r.range(70, 130) / 100, if two { P10 * r.range(70, 130) / 100 } else { 0 }];
        let off = |r: &mut Rng, p: i64| if p == 0 { 0 } else { p + p * r.range(-70, 70) / 10_000 };
        let a = [off(&mut r, c[0]), off(&mut r, c[1])];
        let side = if r.next() % 2 == 0 { SIDE_A } else { SIDE_B };
        let max = x.take_max(side, c);
        let amount = match r.next() % 4 {
            0 => max,
            1 => max + 1 + r.range(0, 3) * KAS1,
            2 => max / 2,
            _ => 0,
        };
        let tk = Take { side, a, amount };
        let Ok(ch) = x.take(tk, T0, position.value as i64) else { continue };
        let who = if side == SIDE_A { &w.a } else { &w.b };
        let f = [fund(xk(who), 30)];
        let b = change_state(&w.lib, &position, &w.script("KpfTChange"), &ch, w.payer(who, &f)).unwrap();
        let s = b.sign(&w.keys).expect("stated");
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok());
        let live = nth(&s, 0, ch, 6_510);
        let (c1, c2) = (w.cited(0, c[0]), w.cited(1, c[1].max(1)));
        let cites: Vec<(&LibScript, &Cited)> = if two { vec![(&cite1, &c1), (&cite2, &c2)] } else { vec![(&cite1, &c1)] };
        let f = [fund(xk(&w.w), 38)];
        let model = x.take_within(&tk, &ch.claimer, &ch.outs, position.value as i64, c);
        for verdict in [true, false] {
            let built = take_answer_with(&w.lib, &live, &w.script("KpfTTake"), &cites, xk(&w.w), w.payer(&w.w, &f), Some(verdict));
            let took = built.is_ok_and(|b| accepted(&b.0, &w.keys));
            assert_eq!(took, verdict == model, "case {k}: side {side}, prices {c:?}, stated {a:?}, amount {amount} of {max}: the script says {took} to the verdict {verdict}, the model {model}");
        }
        if model { yes += 1 } else { no += 1 }
    }
    println!("random takes: {yes} within the rule, {no} not");
    assert!(yes >= 5 && no >= 5);
}

#[test]
fn review_30_a_sompi_more_changes_no_answer_and_an_unknown_flag_is_not_stated() {
    use kpf_protocol::oracle_build::OPlan;
    use kpf_protocol::tx::Arg;
    use silverscript_abi::ArtifactValue;
    let w = world();
    let position = w.position(false);
    let tk = Take { side: SIDE_A, a: [P12, 0], amount: 54 * KAS1 };
    let c12 = w.cited(0, P12);
    let cite1 = w.script("KpfTCite");
    // F51. The disputer writes the disputed object with a sompi more than the statement and its bond: the take is
    // within the rule all the same, and is paid as it was stated.
    let mut d = w.disputed(&w.stated(&position, tk, 6_510));
    d.value += 1;
    let f = [fund(xk(&w.w), 32)];
    let (b, within, _) = take_answer(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &c12)], xk(&w.w), w.payer(&w.w, &f)).unwrap();
    assert!(within);
    let s = signed("take proved with a sompi more in the object", &b, &w.keys);
    let n = s.tx.outputs.len();
    assert_eq!(s.tx.outputs[n - 1].value, (54 + 10 + 5) * KAS1 as u64);
    let forced = take_answer_with(&w.lib, &d, &w.script("KpfTTake"), &[(&cite1, &c12)], xk(&w.w), w.payer(&w.w, &f), Some(false));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a sompi more refutes a take within the rule");

    // F54. The outputs' flags are held when they are stated: the first 0 or 1, the others 1. (The settlement took
    // any other first flag for a payment, and no reader knew it.) The same statement with its output hash made for
    // the bytes it shows is refused for the flag alone.
    let x = position.s;
    let c = x.take(tk, T0, position.value as i64).unwrap();
    let fa = [fund(xk(&w.a), 30)];
    let b = change_state(&w.lib, &position, &w.script("KpfTChange"), &c, w.payer(&w.a, &fa)).unwrap();
    assert!(accepted(&b, &w.keys));
    let with = |raw: Vec<u8>| -> bool {
        let mut t = b.clone();
        let OPlan::Contract { args, .. } = &mut t.plans[1] else { panic!("the script") };
        args[8] = Arg::V(ArtifactValue::Bytes(raw.clone()));
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &c.as_stated().hash_with_outs(&blake2b(&raw)));
        accepted(&t, &w.keys)
    };
    let raw = c.outs.bytes();
    assert!(with(raw.clone()), "the control: the statement rebuilt from its own bytes");
    for flag in [2u8, 3, 0x7f, 0x80, 0xff] {
        let mut first = raw.clone();
        first[0] = flag;
        assert!(!with(first), "a first output of flag {flag} is stated");
    }
    for flag in [0u8, 3, 0xff] {
        let mut second = raw.clone();
        second[41] = flag;
        assert!(!with(second), "a second output of flag {flag} is stated");
    }
    // (A second output of flag 2 is a note to a shard, which a covered account's statements carry. On a position
    // it is stated and read, and is within the rule at no price: whoever watches disputes it.)
    let mut note = raw.clone();
    note[41] = 2;
    assert!(with(note.clone()));
    let read = kpf_protocol::thin::Outs::read(&note).expect("read");
    assert!(read.is_note(1) && !x.take_within(&tk, &c.claimer, &read, position.value as i64, [P12, 0]));
    // F55. A library of one copy of each script has two of the citing script, and is no library with one.
    let lib1 = lib_genesis(1, 10 * KAS1 as u64, w.payer(&w.f, &[fund(xk(&w.f), 201)])).unwrap().0;
    assert_eq!(lib1.tx.outputs.iter().filter(|o| o.covenant.is_some() && o.script_public_key == script_spk("KpfTCite").unwrap()).count(), 2);
}

