//! An account of any number of legs (`KpfTPoint`, `KpfTStepRow`, `KpfTStepPrice`, `KpfTStepEnd`;
//! docs/ARCHITECTURE-CLEARING.md 4b). Its state is a position's with a chain of legs in place of two, so the scripts
//! that state, apply, dispute and end a change serve it as they are. A take on it is stated with a trace, a dispute
//! names one point of the trace, and the answer is of that point alone: one leg and at most one citation, whatever
//! the number of legs. Every transaction is built, signed and run through `verify::verify`.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cite;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::cross::{XLeg, XTerms, SIDE_A, SIDE_B};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::{row_right, ATake, Account, Out, Outs, Pointed, Row, Stated, KIND_ACCOUNT_LIQ, ORACLE_FEE};
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
    chain: RefCell<Vec<(Hash, Hash)>>,
}

impl World {
    fn script(&self, name: &str) -> LibScript {
        self.scripts.iter().find(|s| s.s == name).cloned().unwrap_or_else(|| panic!("{name}"))
    }
    fn payer<'a>(&self, who: &Keypair, funds: &'a [KeyUtxo]) -> Payer<'a> {
        Payer { funds, change_to: xk(who), rate: MIN_FEE_RATE }
    }
    /// An account of A against B with `n` legs, one a market, long and short in turn, each of `3 + i` units from 10;
    /// A's margin 40 KAS a leg, B's 60.
    fn account(&self, n: usize) -> OLive<Account> {
        let p = &self.markets[0].p;
        let t = XTerms { term: p.term, mmr_bps: p.mmr_bps, g_bps: p.g_bps, liq_fee_bps: p.liq_fee_bps, keeper_fee_min: p.keeper_fee_min, carrier: p.carrier, idle_daa: p.idle_daa, grace_daa: p.grace_daa, late_daa: p.late_daa, quiet_daa: p.quiet_daa };
        let legs = (0..n).map(|i| { let m = &self.markets[i]; XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: (3 + i as i64) * m.p.scale, dir: if i % 2 == 0 { LONG } else { SHORT }, entry: P10 } }).collect();
        let s = Account { t, legs, a_key: xk(&self.a), b_key: xk(&self.b), a_margin: 40 * n as i64 * KAS1, b_margin: 60 * n as i64 * KAS1, expiry: 6_000 + p.term, dispute_daa: 50, bond_bps: 100, a_min_counter: 0, b_min_counter: 0, shard: None };
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
    /// The statement `st` made on the account, landed at 6,510.
    fn stated(&self, account: &OLive<Account>, st: &Stated, said: &[u8]) -> OLive<Stated> {
        let who = if st.claimer == xk(&self.a) { &self.a } else if st.claimer == xk(&self.b) { &self.b } else { &self.w };
        let f = [fund(xk(who), 30)];
        let b = account_state(&self.lib, account, &self.script("KpfTChange"), st, said, self.payer(who, &f)).unwrap();
        let s = b.sign(&self.keys).expect("stated");
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok());
        nth(&s, 0, *st, 6_510)
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
    let (a, b, w, f) = (kp(1), kp(2), kp(3), kp(9));
    let members = [kp(101), kp(102), kp(103)];
    let mut keys = KeySet::new();
    for k in [&a, &b, &w, &f] {
        keys.add_keypair(*k);
    }
    let rid = Hash::from_bytes([0x52; 32]);
    let rp = CommitteeParams::testnet();
    let markets = (0..16u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
    let mut table = EMPTY_TABLE;
    for (i, wt) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: wt * KAS1 };
    }
    let table = OLive { s: EpochTable { members: table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(20), daa: 6_000 };
    let funds = vec![fund(xk(&f), 200)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    World { a, b, w, f, members, keys, rid, markets, table, lib, scripts: lib_scripts(&gs.tx, 6_000), chain: RefCell::new(vec![]) }
}

/// The prices of `n` markets: every one a twentieth above 10 where A is long and below where it is short (A gains
/// on every leg).
fn prices(n: usize) -> Vec<i64> {
    (0..n).map(|i| if i % 2 == 0 { P10 * 105 / 100 } else { P10 * 95 / 100 }).collect()
}

#[test]
fn a_take_on_an_account_of_five_legs_is_stated_and_applied_by_the_scripts_of_a_position() {
    let w = world();
    let account = w.account(5);
    let x = &account.s;
    let a = prices(5);
    let max = x.take_max(SIDE_A, &a);
    assert!(max > 0);
    let f = [fund(xk(&w.a), 30)];
    let (b, st, tk) = account_take(&w.lib, &account, &w.script("KpfTChange"), SIDE_A, max, &a, T0, w.payer(&w.a, &f)).unwrap();
    let s = signed("account take stated (5 legs)", &b, &w.keys);
    assert_eq!(tk.rows.len(), 5);
    assert_eq!(x.wrong_point(&tk, &st.claimer, &st.outs, account.value as i64, &a), None, "the statement is the rule's");
    let live = nth(&s, 0, st, 6_510);
    let f = [fund(xk(&w.w), 31)];
    let b = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).unwrap();
    let r = signed("account take applied", &b, &w.keys);
    let after = x.taken(SIDE_A, max, &tk.rows).unwrap();
    assert_eq!((r.tx.outputs[0].value, &r.tx.outputs[0].script_public_key), (account.value - max as u64, &hash_spk(&w.lib, &after.hash())));
    assert_eq!(after.legs.iter().map(|l| l.entry).collect::<Vec<_>>(), a);
    let n = r.tx.outputs.len();
    assert_eq!((r.tx.outputs[n - 1].value, &r.tx.outputs[n - 1].script_public_key), (max as u64, &p2pk_spk(&xk(&w.a))));
    // A sompi more is not the rule's, and the point that is wrong is the end.
    let (lie, tk2) = x.take(SIDE_A, max + 1, &a, T0, account.value as i64).unwrap();
    assert_eq!(x.wrong_point(&tk2, &lie.claimer, &lie.outs, account.value as i64, &a), Some(0));
}

#[test]
fn the_end_of_a_take_is_answered_right_or_wrong_and_never_both() {
    let w = world();
    let account = w.account(5);
    let x = &account.s;
    let a = prices(5);
    let max = x.take_max(SIDE_B, &a.iter().map(|p| 2 * P10 - p).collect::<Vec<_>>());
    assert!(max > 0, "at the mirrored prices B gained");
    let b_prices: Vec<i64> = a.iter().map(|p| 2 * P10 - p).collect();
    let (st, tk) = x.take(SIDE_B, max, &b_prices, T0, account.value as i64).unwrap();
    let live = w.stated(&account, &st, &tk.said());
    let q = w.pointed(&live, 0);
    let f = [fund(xk(&w.w), 32)];
    // Right: the statement stands again (its dispute period begins again), half of the disputer's bond to the
    // claimer, the other half in the script's copy.
    let (b, right) = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepEnd"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    let s = signed("end answered: right", &b, &w.keys);
    // (Of the disputer's bond a quarter is the claimer's, a quarter whoever answered's, half the copy's: docs 14.1.)
    let quarter = st.bond / 4;
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (live.value, &hash_spk(&w.lib, &st.again().hash())), "the statement again");
    assert_eq!((st.again().lateness(), st.again().again()), (st.late, st.again()), "it keeps its lateness");
    let n = s.tx.outputs.len();
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), (quarter as u64, &p2pk_spk(&xk(&w.b))));
    assert_eq!((s.tx.outputs[n - 2].value, &s.tx.outputs[n - 2].script_public_key), (quarter as u64, &p2pk_spk(&xk(&w.w))), "who answered");
    assert_eq!(s.tx.outputs[1].value, w.script("KpfTStepEnd").value + (ORACLE_FEE + st.bond - 2 * quarter) as u64);
    let forced = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepEnd"), xk(&w.w), w.payer(&w.w, &f), Some(false));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a right end is answered wrong");

    // Wrong ends: a sompi too much; outputs that pay more; the pot taken whole; another's take; a trace cut short.
    let (more, tk_more) = x.take(SIDE_B, max + 1, &b_prices, T0, account.value as i64).unwrap();
    let held = account.value as i64;
    let after = x.taken(SIDE_B, max, &tk.rows).unwrap();
    let lies: Vec<(&str, Stated, ATake)> = vec![
        ("a sompi too much", more, tk_more),
        ("outputs that pay more", Stated { outs: Outs::of(&[Out { key: false, to: after.hash(), value: held - max - KAS1 }, Out { key: true, to: xk(&w.b), value: max + KAS1 }]).unwrap(), ..st }, tk.clone()),
        ("the pot taken whole", Stated { outs: Outs::of(&[Out { key: true, to: xk(&w.b), value: held }]).unwrap(), ..st }, tk.clone()),
        ("another's take", Stated { claimer: xk(&w.w), ..st }, tk.clone()),
        ("a trace cut short", { let t = ATake { rows: tk.rows[..4].to_vec(), ..tk.clone() }; Stated { arg: t.arg(), ..st } }, ATake { rows: tk.rows[..4].to_vec(), ..tk.clone() }),
    ];
    for (what, lie, tk) in lies {
        assert!(!x.end_right(&tk, &lie.claimer, &lie.outs, held), "{what}");
        let q = w.pointed(&w.stated(&account, &lie, &tk.said()), 0);
        let (b, right) = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepEnd"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
        assert!(!right, "{what}");
        let s = signed(&format!("end answered: wrong ({what})"), &b, &w.keys);
        assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())), "{what}: the account again");
        let n = s.tx.outputs.len();
        assert_eq!((s.tx.outputs[n - 2].value, &s.tx.outputs[n - 2].script_public_key), (lie.bond as u64, &p2pk_spk(&xk(&w.w))), "{what}: the disputer's bond back");
        assert_eq!(s.tx.outputs[n - 1].value, (lie.bond / 2) as u64, "{what}: half of the bond lost");
        let forced = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepEnd"), xk(&w.w), w.payer(&w.w, &f), Some(true));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "{what}: answered right");
    }
}

#[test]
fn a_row_is_answered_with_its_leg_alone() {
    let w = world();
    let account = w.account(5);
    let x = &account.s;
    let a = prices(5);
    let (st, tk) = x.take(SIDE_A, KAS1, &a, T0, account.value as i64).unwrap();
    let f = [fund(xk(&w.w), 33)];
    // Row 3 of an honest trace: right.
    let q = w.pointed(&w.stated(&account, &st, &tk.said()), 5);
    let (b, right) = step_row(&w.lib, &q, &tk, &x.legs[2], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    signed("row answered: right", &b, &w.keys);
    let forced = step_row(&w.lib, &q, &tk, &x.legs[2], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), Some(false));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a right row is answered wrong");
    // Another leg than the trace has there: neither verdict (the row is of the leg its chain names).
    assert!(step_row(&w.lib, &q, &tk, &x.legs[1], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), None).is_err());
    for v in [true, false] {
        let r = step_row(&w.lib, &q, &tk, &x.legs[1], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), Some(v));
        assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "a row is answered {v} with another leg");
    }

    // A trace whose third row adds 7 KAS to A's gain (and carries it on, so that the end pays it): the end is right
    // by its own row, rows 4 and 5 follow from what stands before them, and row 3 is the one that is wrong.
    let mut rows = tk.rows.clone();
    for r in rows[2..].iter_mut() {
        r.gain += 7 * KAS1;
    }
    let lie_tk = ATake { side: SIDE_A, amount: KAS1 + 7 * KAS1, rows, skip: 0, leave: None };
    let after = x.taken(SIDE_A, lie_tk.amount, &lie_tk.rows).unwrap();
    let held = account.value as i64;
    let lie = Stated { arg: lie_tk.arg(), outs: Outs::of(&[Out { key: false, to: after.hash(), value: held - lie_tk.amount }, Out { key: true, to: xk(&w.a), value: lie_tk.amount }]).unwrap(), ..st };
    assert!(x.end_right(&lie_tk, &lie.claimer, &lie.outs, held), "the end follows from the last row as it stands");
    assert_eq!(x.wrong_point(&lie_tk, &lie.claimer, &lie.outs, held, &a), Some(5));
    assert_eq!(row_right(&x.legs[3], &lie_tk.rows[2], &lie_tk.rows[3], false), Some(true));
    let q = w.pointed(&w.stated(&account, &lie, &lie_tk.said()), 5);
    let (b, right) = step_row(&w.lib, &q, &lie_tk, &x.legs[2], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    let s = signed("row answered: wrong", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())));
    let forced = step_row(&w.lib, &q, &lie_tk, &x.legs[2], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), Some(true));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a wrong row is answered right");
    // The first row is answered from nothing before it.
    let q = w.pointed(&w.stated(&account, &st, &tk.said()), 1);
    let (b, right) = step_row(&w.lib, &q, &tk, &x.legs[0], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    signed("first row answered", &b, &w.keys);
    let _ = Row::ZERO;
}

#[test]
fn a_price_is_answered_with_one_citation() {
    let w = world();
    let account = w.account(5);
    let x = &account.s;
    let a = prices(5);
    let (st, tk) = x.take(SIDE_A, KAS1, &a, T0, account.value as i64).unwrap();
    let f = [fund(xk(&w.w), 34)];
    let cite1 = w.script("KpfTCite");
    // The committee's price of market 3 is the one stated (within the tolerance): right.
    let q = w.pointed(&w.stated(&account, &st, &tk.said()), 8);
    let c = w.cited(3, a[3] + a[3] / 400);
    let (b, right) = step_price(&w.lib, &q, x, &tk, &w.script("KpfTStepPrice"), (&cite1, &c), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    let s = signed("price answered: right", &b, &w.keys);
    assert_eq!(s.tx.inputs.len(), 4, "the object, the script, one citation and a coin");
    let forced = step_price(&w.lib, &q, x, &tk, &w.script("KpfTStepPrice"), (&cite1, &c), xk(&w.w), w.payer(&w.w, &f), Some(false));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a right price is answered wrong");
    // The committee's price is a fiftieth away: wrong. The account again.
    let c = w.cited(3, a[3] * 102 / 100);
    let (b, right) = step_price(&w.lib, &q, x, &tk, &w.script("KpfTStepPrice"), (&cite1, &c), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    let s = signed("price answered: wrong", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())));
    let forced = step_price(&w.lib, &q, x, &tk, &w.script("KpfTStepPrice"), (&cite1, &c), xk(&w.w), w.payer(&w.w, &f), Some(true));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a wrong price is answered right");
    // The citation of another market answers nothing: refused by the builder, and by the script when it is passed by
    // (the same transaction with the citation's market moved in the leg shown would not be of the trace's leg).
    let other = w.cited(2, a[3]);
    assert!(step_price(&w.lib, &q, x, &tk, &w.script("KpfTStepPrice"), (&cite1, &other), xk(&w.w), w.payer(&w.w, &f), None).is_err());
}

#[test]
fn a_point_nobody_answers_lapses() {
    let w = world();
    let account = w.account(3);
    let x = &account.s;
    let (st, tk) = x.take(SIDE_A, KAS1, &prices(3), T0, account.value as i64).unwrap();
    let q = w.pointed(&w.stated(&account, &st, &tk.said()), 4);
    let f = [fund(xk(&w.w), 35)];
    let b = point_lapse(&w.lib, &q, &w.script("KpfTPoint"), w.payer(&w.w, &f)).unwrap();
    let s = signed("point lapsed", &b, &w.keys);
    let n = s.tx.outputs.len();
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())));
    assert_eq!((s.tx.outputs[n - 1].value, s.tx.outputs[n - 2].value), (st.bond as u64, st.bond as u64));
    let mut t = b.clone();
    t.tx.inputs[0].sequence = 49;
    assert!(!accepted(&t, &w.keys), "a point lapses before its period is over");
    // A statement of another kind is not disputed by a point (a take on a position has its own answer).
    let other = Stated { kind: 1, arg: blake2b(&[7u8; 32]), ..st };
    let live = w.stated(&account, &other, &[7u8; 32]);
    assert!(point_dispute(&w.lib, &live, &w.script("KpfTPoint"), xk(&w.w), 0, w.payer(&w.w, &f)).is_err());
}

#[test]
fn an_answer_is_as_small_for_sixteen_legs_as_for_two() {
    let w = world();
    let f = [fund(xk(&w.w), 36)];
    let cite1 = w.script("KpfTCite");
    println!("legs | stated | applied | row | price | end   (bytes)");
    for n in [2usize, 5, 16] {
        let account = w.account(n);
        let x = &account.s;
        let a = prices(n);
        let fa = [fund(xk(&w.a), 30)];
        let (b, st, tk) = account_take(&w.lib, &account, &w.script("KpfTChange"), SIDE_A, KAS1, &a, T0, w.payer(&w.a, &fa)).unwrap();
        let stated = b.sign(&w.keys).unwrap();
        let live = nth(&stated, 0, st, 6_510);
        let applied = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).unwrap().sign(&w.keys).unwrap();
        let size = |b: OBuilt| { let s = b.sign(&w.keys).unwrap(); assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok()); s.mass.size };
        let row = size(step_row(&w.lib, &w.pointed(&live, 2 * n as i64 - 1), &tk, &x.legs[n - 1], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), None).unwrap().0);
        let c = w.cited(n - 1, a[n - 1]);
        let price = size(step_price(&w.lib, &w.pointed(&live, 2 * n as i64), x, &tk, &w.script("KpfTStepPrice"), (&cite1, &c), xk(&w.w), w.payer(&w.w, &f), None).unwrap().0);
        let end = size(step_end(&w.lib, &w.pointed(&live, 0), x, &tk, &w.script("KpfTStepEnd"), xk(&w.w), w.payer(&w.w, &f), None).unwrap().0);
        println!("{n:>4} | {:>6} | {:>7} | {row:>5} | {price:>5} | {end:>5}", stated.mass.size, applied.mass.size);
    }
}

/// An account of three legs in which A's margin is thin: 8 KAS against legs worth 120.
fn thin_account(w: &World) -> OLive<Account> {
    let mut account = w.account(3);
    account.s.a_margin = 8 * KAS1;
    account.s.b_margin = 60 * KAS1;
    account.value = (account.s.pot() + KAS1) as u64;
    account
}

/// Every market 3 % against A (down where it is long, up where it is short).
fn against_a(n: usize) -> Vec<i64> {
    (0..n).map(|i| if i % 2 == 0 { P10 * 97 / 100 } else { P10 * 103 / 100 }).collect()
}

#[test]
fn a_liquidation_takes_one_leg_a_step_while_the_side_is_below() {
    let w = world();
    let account = thin_account(&w);
    let x = &account.s;
    let held = account.value as i64;
    // At the entries A is above its maintenance: no step is stated.
    assert!(x.liq_step(SIDE_A, 3, &[P10, P10, P10], T0, held, xk(&w.w)).is_err());
    // 3 % against A on every leg: A lost 3.6 KAS and has 4.4 left, below 4.5 % of the legs' worth.
    let a = against_a(3);
    let f = [fund(xk(&w.w), 30)];
    let (b, st, tk) = account_liq(&w.lib, &account, &w.script("KpfTChange"), SIDE_A, 3, &a, T0, xk(&w.w), w.payer(&w.w, &f)).unwrap();
    assert_eq!((st.kind, tk.skip, st.claimer), (KIND_ACCOUNT_LIQ, 3, xk(&w.w)));
    let s = signed("liquidation step stated", &b, &w.keys);
    let (after, fee) = x.liq_stepped(SIDE_A, 3, &tk.rows).unwrap();
    assert_eq!(fee, tk.amount);
    assert!(fee > 0 && fee < 440_000_000);
    assert_eq!(after.legs.len(), 2);
    assert_eq!((after.legs[0].entry, after.legs[1].entry), (a[0], a[1]), "the legs that stay stand at the prices stated");
    assert_eq!((after.a_margin, after.b_margin), (440_000_000 - fee, 60 * KAS1 + 360_000_000));
    assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LIQ, &tk, &st.claimer, &st.outs, held, &a), None);
    // Applied after the dispute period: the account without the leg, the fee to the liquidator.
    let live = nth(&s, 0, st, 6_510);
    let b = stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).unwrap();
    let r = signed("liquidation step applied", &b, &w.keys);
    let n = r.tx.outputs.len();
    assert_eq!((r.tx.outputs[0].value, &r.tx.outputs[0].script_public_key), (account.value - fee as u64, &hash_spk(&w.lib, &after.hash())));
    assert_eq!((r.tx.outputs[n - 1].value, &r.tx.outputs[n - 1].script_public_key), (fee as u64, &p2pk_spk(&xk(&w.w))));
    // With one leg less A is above its maintenance again: there is no second step.
    assert!(after.liq_step(SIDE_A, 2, &a[..2], T0, held - fee, xk(&w.w)).is_err(), "a side that is above again is not liquidated further");

    // The end of the step is answered right, and its rows and prices by the scripts of a take.
    let fw = [fund(xk(&w.f), 32)];
    let q = w.pointed(&live, 0);
    let (b, right) = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepLiq"), xk(&w.f), w.payer(&w.f, &fw), None).unwrap();
    assert!(right);
    signed("liquidation end answered: right", &b, &w.keys);
    let forced = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepLiq"), xk(&w.f), w.payer(&w.f, &fw), Some(false));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a right step is answered wrong");
    // (The end of a step is not answered by the script of a take's end.)
    let other = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepEnd"), xk(&w.f), w.payer(&w.f, &fw), Some(true));
    assert!(other.is_err(), "the script given is not the kind's");
    let q = w.pointed(&live, 5);
    let (b, right) = step_row(&w.lib, &q, &tk, &x.legs[2], &w.script("KpfTStepRow"), xk(&w.f), w.payer(&w.f, &fw), None).unwrap();
    assert!(right, "the row of the leg that leaves: its second chain passes it by");
    signed("liquidation row answered: right", &b, &w.keys);
    let cite1 = w.script("KpfTCite");
    let q = w.pointed(&live, 2);
    let c = w.cited(0, a[0]);
    let (b, right) = step_price(&w.lib, &q, x, &tk, &w.script("KpfTStepPrice"), (&cite1, &c), xk(&w.f), w.payer(&w.f, &fw), None).unwrap();
    assert!(right);
    signed("liquidation price answered: right", &b, &w.keys);
    // The committee's price of market 0 was 10, not 9.7: A was not below. Wrong: the account again.
    let c = w.cited(0, P10);
    let (b, right) = step_price(&w.lib, &q, x, &tk, &w.script("KpfTStepPrice"), (&cite1, &c), xk(&w.f), w.payer(&w.f, &fw), None).unwrap();
    assert!(!right);
    let s = signed("liquidation price answered: wrong", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())));
}

#[test]
fn a_step_that_is_not_the_rules_has_a_wrong_end() {
    let w = world();
    let account = thin_account(&w);
    let x = &account.s;
    let held = account.value as i64;
    let a = against_a(3);
    let (st, tk) = x.liq_step(SIDE_A, 3, &a, T0, held, xk(&w.w)).unwrap();
    let (after, fee) = x.liq_stepped(SIDE_A, 3, &tk.rows).unwrap();
    let outs = |next: &Account, pay: i64| Outs::of(&[Out { key: false, to: next.hash(), value: held - pay }, Out { key: true, to: xk(&w.w), value: pay }]).unwrap();
    // The account as it would stand after a step at the entries (where A is not below), stated all the same.
    let flat = [P10, P10, P10];
    let flat_rows = x.trace_without(&flat, 3);
    let flat_after = Account { legs: x.legs[..2].to_vec(), a_margin: x.a_margin - fee, ..x.clone() };
    let flat_tk = ATake { side: SIDE_A, amount: fee, rows: flat_rows, skip: 3, leave: None };
    let lies: Vec<(&str, Stated, ATake)> = vec![
        ("a fee a sompi more", { let t = ATake { amount: fee + 1, ..tk.clone() }; Stated { arg: t.arg(), outs: outs(&Account { a_margin: after.a_margin - 1, ..after.clone() }, fee + 1), ..st } }, ATake { amount: fee + 1, ..tk.clone() }),
        ("all the side's margin as the fee", Stated { outs: outs(&Account { a_margin: 0, ..after.clone() }, 440_000_000), ..st }, tk.clone()),
        ("a side that is not below", Stated { arg: flat_tk.arg(), outs: outs(&flat_after, fee), ..st }, flat_tk),
        ("the other side liquidated", { let t = ATake { side: SIDE_B, ..tk.clone() }; Stated { arg: t.arg(), ..st } }, ATake { side: SIDE_B, ..tk.clone() }),
        ("a leg that is none of the account's", { let t = ATake { skip: 4, ..tk.clone() }; Stated { arg: t.arg(), ..st } }, ATake { skip: 4, ..tk.clone() }),
    ];
    let f = [fund(xk(&w.f), 33)];
    for (what, lie, tk) in lies {
        assert!(!x.liq_end_right(&tk, &lie.claimer, &lie.outs, held), "{what}");
        let q = w.pointed(&w.stated(&account, &lie, &tk.said()), 0);
        let (b, right) = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepLiq"), xk(&w.f), w.payer(&w.f, &f), None).unwrap();
        assert!(!right, "{what}");
        let s = signed(&format!("liquidation end answered: wrong ({what})"), &b, &w.keys);
        assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())), "{what}: the account again");
        let forced = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepLiq"), xk(&w.f), w.payer(&w.f, &f), Some(true));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "{what}: answered right");
    }
    // A step whose trace does not pass the leaving leg by (the account would go on with it, the fee paid all the
    // same): the end follows from its own last row, and the row of that leg is the point that is wrong.
    let rows = x.trace(&a);
    let kept = Account { legs: x.legs.iter().zip(&a).map(|(l, p)| XLeg { entry: *p, ..*l }).collect(), a_margin: after.a_margin, b_margin: after.b_margin, ..x.clone() };
    let t = ATake { rows, ..tk.clone() };
    let lie = Stated { arg: t.arg(), outs: outs(&kept, fee), ..st };
    assert_eq!(x.wrong_point_of(KIND_ACCOUNT_LIQ, &t, &lie.claimer, &lie.outs, held, &a), Some(5));
    let q = w.pointed(&w.stated(&account, &lie, &t.said()), 5);
    let (b, right) = step_row(&w.lib, &q, &t, &x.legs[2], &w.script("KpfTStepRow"), xk(&w.f), w.payer(&w.f, &f), None).unwrap();
    assert!(!right);
    signed("liquidation row answered: wrong", &b, &w.keys);
}

#[test]
fn a_statement_nobody_could_answer_does_not_lock_an_object() {
    let w = world();
    let account = w.account(3);
    let x = &account.s;
    let held = account.value as i64;
    let a = prices(3);
    let (st, tk) = x.take(SIDE_A, KAS1, &a, T0, held).unwrap();
    let f = [fund(xk(&w.f), 34)];
    let fa = [fund(xk(&w.a), 30)];

    // 1. A take of kind 1 (a position's, which KpfTTake answers from two legs) stated on an account: anyone shows
    //    the account, and it ends as a statement that is not within the rule, disputed or not.
    let odd = Stated { kind: 1, arg: blake2b(&[7u8; 32]), ..st };
    let live = w.stated(&account, &odd, &[7u8; 32]);
    let b = kind_wrong(&w.lib, &live, &odd, None, x, &w.script("KpfTKind"), xk(&w.f), w.payer(&w.f, &f)).unwrap();
    let s = signed("kind 1 on an account: ended", &b, &w.keys);
    let n = s.tx.outputs.len();
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())));
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), ((odd.bond / 2) as u64, &p2pk_spk(&xk(&w.f))));
    let fw = [fund(xk(&w.w), 31)];
    let (b, dh) = stated_dispute(&w.lib, &live, &w.script("KpfTChangeDispute"), xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    let ds = signed("kind 1 on an account: disputed", &b, &w.keys);
    let disputed: OLive<()> = nth(&ds, 0, (), 6_520);
    assert_eq!(ds.tx.outputs[0].script_public_key, hash_spk(&w.lib, &dh));
    let b = kind_wrong(&w.lib, &disputed, &odd, Some(xk(&w.w)), x, &w.script("KpfTKind"), xk(&w.f), w.payer(&w.f, &f)).unwrap();
    let s = signed("kind 1 on an account, disputed: ended", &b, &w.keys);
    let n = s.tx.outputs.len();
    assert_eq!((s.tx.outputs[n - 2].value, &s.tx.outputs[n - 2].script_public_key), (odd.bond as u64, &p2pk_spk(&xk(&w.w))), "the disputer's bond back");
    // (It does not end a statement of another kind: a take on an account is answered by its points.)
    let good = w.stated(&account, &st, &tk.said());
    assert!(kind_wrong(&w.lib, &good, &st, None, x, &w.script("KpfTKind"), xk(&w.f), w.payer(&w.f, &f)).is_err());

    // 2. A statement of kind 2 or 3 is made with the account's form: without it, or with another's, refused.
    for (what, form) in [("no form", vec![]), ("another account's form", w.account(2).s.form()), ("a form cut short", x.form()[..121].to_vec())] {
        let b = state_raw(&w.lib, &account, &x.fixed_hash(), &x.moving(), &st, &tk.said(), form, &w.script("KpfTChange"), w.payer(&w.a, &fa)).unwrap();
        assert!(!accepted(&b, &w.keys), "a take on an account is stated with {what}");
    }
    let b = state_raw(&w.lib, &account, &x.fixed_hash(), &x.moving(), &odd, &[7u8; 32], x.form(), &w.script("KpfTChange"), w.payer(&w.a, &fa)).unwrap();
    assert!(!accepted(&b, &w.keys), "kind 1 is stated with a form");
    let b = state_raw(&w.lib, &account, &x.fixed_hash(), &x.moving(), &Stated { kind: 5, ..st }, &tk.said(), x.form(), &w.script("KpfTChange"), w.payer(&w.a, &fa)).unwrap();
    assert!(!accepted(&b, &w.keys), "a kind that is none is stated");
    // What is stated has its kind's length, so that the kind's answer can open it: four numbers for kind 1; three
    // numbers and whole rows for kinds 2 and 3.
    for (kind, said) in [(1i64, vec![7u8; 31]), (1, vec![7u8; 36]), (2, [tk.said(), vec![0]].concat()), (2, tk.said()[..23].to_vec()), (3, tk.said()[..60].to_vec())] {
        let odd = Stated { kind, arg: blake2b(&said), ..st };
        let b = state_raw(&w.lib, &account, &x.fixed_hash(), &x.moving(), &odd, &said, if kind >= 2 { x.form() } else { vec![] }, &w.script("KpfTChange"), w.payer(&w.a, &fa)).unwrap();
        assert!(!accepted(&b, &w.keys), "kind {kind} is stated with {} bytes", said.len());
    }

    // 3. A statement of kind 2 is disputed by a point, not whole (whole, nobody could answer it).
    let (b, _) = stated_dispute(&w.lib, &good, &w.script("KpfTChangeDispute"), xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    assert!(!accepted(&b, &w.keys), "a take on an account is disputed whole");

    // 4. A point beyond the trace is of nothing: answered right, without a leg of the trace, and the disputer loses.
    for point in [7, 8, 99] {
        let q = w.pointed(&good, point);
        let (b, right) = step_row(&w.lib, &q, &tk, &x.legs[0], &w.script("KpfTStepRow"), xk(&w.f), w.payer(&w.f, &f), None).unwrap();
        assert!(right);
        signed(&format!("point {point} beyond the trace: right"), &b, &w.keys);
        let forced = step_row(&w.lib, &q, &tk, &x.legs[0], &w.script("KpfTStepRow"), xk(&w.f), w.payer(&w.f, &f), Some(false));
        assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a point beyond the trace is answered wrong");
    }

    // 5. A trace of no row: its end is wrong (and any other point of it is beyond the trace).
    let none = ATake { rows: vec![], ..tk.clone() };
    let empty = Stated { arg: none.arg(), ..st };
    let live = w.stated(&account, &empty, &none.said());
    let q = w.pointed(&live, 0);
    let (b, right) = step_end(&w.lib, &q, x, &none, &w.script("KpfTStepEnd"), xk(&w.f), w.payer(&w.f, &f), None).unwrap();
    assert!(!right);
    signed("a trace of no row: its end is wrong", &b, &w.keys);
    // A take that says it leaves a leg out: its end is wrong.
    let skipping = ATake { skip: 2, rows: x.trace_without(&a, 2), ..tk.clone() };
    let lie = Stated { arg: skipping.arg(), ..st };
    let q = w.pointed(&w.stated(&account, &lie, &skipping.said()), 0);
    let (b, right) = step_end(&w.lib, &q, x, &skipping, &w.script("KpfTStepEnd"), xk(&w.f), w.payer(&w.f, &f), None).unwrap();
    assert!(!right);
    signed("a take that leaves a leg out: its end is wrong", &b, &w.keys);
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
fn every_false_statement_has_a_wrong_point_and_the_scripts_agree_with_the_model() {
    let w = world();
    let mut r = Rng(0x2545_f491_4f6c_dd1d);
    let cite1 = w.script("KpfTCite");
    let f = [fund(xk(&w.w), 37)];
    let (mut honest, mut lies, mut by_point) = (0, 0, [0usize; 3]);
    for k in 0..40 {
        let n = r.range(1, 16) as usize;
        let mut account = w.account(n);
        account.s.a_margin = r.range(10, 60) * n as i64 * KAS1;
        account.s.b_margin = r.range(10, 60) * n as i64 * KAS1;
        account.value = (account.s.pot() + KAS1) as u64;
        let x = account.s.clone();
        let held = account.value as i64;
        // The committee's prices, and the claimer's own near them.
        let c: Vec<i64> = (0..n).map(|_| P10 * r.range(90, 110) / 100).collect();
        let near: Vec<i64> = c.iter().map(|p| p + p * r.range(-30, 30) / 10_000).collect();
        let side = if r.next() % 2 == 0 { SIDE_A } else { SIDE_B };
        let max = x.take_max(side, &near);
        let Ok((st, tk)) = x.take(side, max / 2, &near, T0, held) else { continue };
        // The statement as it is, or with one thing moved.
        let (st, tk) = match k % 5 {
            0 => (st, tk),
            1 => match x.take(side, max + 1 + r.range(0, 5) * KAS1, &near, T0, held) {
                Ok(v) => v,
                Err(_) => continue,
            },
            2 => {
                // A price a twentieth from the committee's, the trace honestly made of it.
                let i = r.range(0, n as i64 - 1) as usize;
                let mut far = near.clone();
                far[i] = c[i] * 105 / 100;
                match x.take(side, 0, &far, T0, held) {
                    Ok(v) => v,
                    Err(_) => continue,
                }
            }
            3 => {
                // A sum moved in one row and carried to the end.
                let i = r.range(0, n as i64 - 1) as usize;
                let mut rows = tk.rows.clone();
                let d = r.range(1, 3) * KAS1 * if side == SIDE_A { 1 } else { -1 };
                for row in rows[i..].iter_mut() {
                    row.gain += d;
                }
                let t = ATake { rows, ..tk.clone() };
                let Some(after) = x.taken(side, t.amount, &t.rows) else { continue };
                let mut outs = vec![Out { key: false, to: after.hash(), value: held - t.amount }];
                if t.amount > 0 {
                    outs.push(Out { key: true, to: st.claimer, value: t.amount });
                }
                (Stated { arg: t.arg(), outs: Outs::of(&outs).unwrap(), ..st }, t)
            }
            _ => {
                // The second chain of one row is another's making (the entries the account goes on with).
                let i = r.range(0, n as i64 - 1) as usize;
                let mut rows = tk.rows.clone();
                rows[i].h = [0x66; 32];
                let t = ATake { rows, ..tk.clone() };
                (Stated { arg: t.arg(), ..st }, t)
            }
        };
        let wrong = x.wrong_point(&tk, &st.claimer, &st.outs, held, &c);
        assert_eq!(wrong.is_none(), k % 5 == 0, "case {k}: only the statement as it is has no wrong point ({wrong:?})");
        // The point the model names (or, for the honest one, a point at random) is answered by its script: the
        // model's verdict is taken, the other refused.
        let point = wrong.unwrap_or_else(|| r.range(0, 2 * n as i64));
        let q = w.pointed(&w.stated(&account, &st, &tk.said()), point);
        let i = ((point + 1) / 2) as usize;
        let built = |v: bool| -> Option<OBuilt> {
            let p = w.payer(&w.w, &f);
            if point == 0 {
                step_end(&w.lib, &q, &x, &tk, &w.script("KpfTStepEnd"), xk(&w.w), p, Some(v)).ok().map(|b| b.0)
            } else if point % 2 == 1 {
                step_row(&w.lib, &q, &tk, &x.legs[i - 1], &w.script("KpfTStepRow"), xk(&w.w), p, Some(v)).ok().map(|b| b.0)
            } else {
                let cited = w.cited(i - 1, c[i - 1]);
                step_price(&w.lib, &q, &x, &tk, &w.script("KpfTStepPrice"), (&cite1, &cited), xk(&w.w), p, Some(v)).ok().map(|b| b.0)
            }
        };
        let model = wrong.is_none();
        for v in [true, false] {
            let took = built(v).is_some_and(|b| accepted(&b, &w.keys));
            assert_eq!(took, v == model, "case {k} ({n} legs, point {point}): the script says {took} to the verdict {v}, the model {model}");
        }
        if model { honest += 1 } else { lies += 1 }
        by_point[if point == 0 { 0 } else if point % 2 == 1 { 1 } else { 2 }] += 1;
    }
    println!("random accounts: {honest} statements of the rule, {lies} not; points answered: end {}, row {}, price {}", by_point[0], by_point[1], by_point[2]);
    assert!(honest >= 4 && lies >= 12 && by_point.iter().all(|c| *c >= 3));
}

/// The chain of the account's legs before the leg `i` (0: 32 zero bytes).
fn chain_before(x: &Account, i: usize) -> [u8; 32] {
    x.legs[..i].iter().fold([0u8; 32], |g, l| blake2b(&[&g[..], &kpf_protocol::thin::leg_bytes(l)].concat()))
}

#[test]
fn review_30_what_the_object_holds_since_changes_no_answer_and_a_statement_stands_again_out_of_its_time() {
    let w = world();
    let account = w.account(3);
    let x = &account.s;
    let a = prices(3);
    let (st, tk) = x.take(SIDE_A, KAS1, &a, T0, account.value as i64).unwrap();
    let f = [fund(xk(&w.w), 33)];
    // F51. The disputer writes the object with a sompi more: the end is right all the same (it counts from what
    // the account held when the take was stated).
    let mut q = w.pointed(&w.stated(&account, &st, &tk.said()), 0);
    q.value += 1;
    let (b, right) = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepEnd"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    signed("end answered right with a sompi more in the object", &b, &w.keys);
    let forced = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepEnd"), xk(&w.w), w.payer(&w.w, &f), Some(false));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a sompi more makes a right end wrong");

    // F53. Row 1 is disputed late in the dispute period and answered right past the account's lateness (100 DAA
    // after the take's time): the statement stands again, is settled and disputed as one in time, and is not ended
    // as late.
    let mut q = w.pointed(&w.stated(&account, &st, &tk.said()), 1);
    q.daa = 6_555;
    let (b, right) = step_row(&w.lib, &q, &tk, &x.legs[0], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(right);
    let s = signed("row answered right, late", &b, &w.keys);
    let again = nth(&s, 0, st.again(), 6_601);
    assert_eq!(s.tx.outputs[0].script_public_key, hash_spk(&w.lib, &st.again().hash()));
    assert!(again.s.in_time(again.daa));
    signed("the statement that stands again is applied", &stated_settle(&w.lib, &again, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).unwrap(), &w.keys);
    let (b, _) = point_dispute(&w.lib, &again, &w.script("KpfTPoint"), xk(&w.w), 0, w.payer(&w.w, &f)).unwrap();
    signed("and disputed again", &b, &w.keys);
    // (As it was first stated, landed at that time, it is out of its time: the control.)
    let first = nth(&s, 0, st, 6_601);
    assert!(stated_settle(&w.lib, &first, &w.script("KpfTChangeSettle"), w.payer(&w.w, &f)).is_err());
}

#[test]
fn review_30_a_trace_with_a_chain_of_no_leg_has_a_wrong_point_and_any_wrong_point_ends_a_dispute() {
    let w = world();
    let account = w.account(3);
    let x = &account.s;
    let held = account.value as i64;
    let a = prices(3);
    let (st, tk) = x.take(SIDE_A, KAS1, &a, T0, held).unwrap();
    // F52. The first two rows carry chains of no leg, the last the account's own root; the last row says A gained
    // 30 KAS more, and the outputs pay it.
    let mut rows = tk.rows.clone();
    rows[0].g = [0x91; 32];
    rows[1].g = [0x92; 32];
    rows[2].gain += 30 * KAS1;
    let amount = KAS1 + 30 * KAS1;
    let lie_tk = ATake { side: SIDE_A, amount, rows, skip: 0, leave: None };
    let last = lie_tk.rows[2];
    let next = Account { a_margin: x.a_margin + last.gain - amount, b_margin: x.b_margin - last.gain, ..x.clone() };
    let p33 = |h: &[u8; 32]| [&[0x20u8][..], h].concat();
    let h = blake2b(&[p33(&blake2b(&[x.terms(), p33(&last.h)].concat())), next.moving()].concat());
    let lie = Stated { arg: lie_tk.arg(), outs: Outs::of(&[Out { key: false, to: h, value: held - amount }, Out { key: true, to: xk(&w.a), value: amount }]).unwrap(), ..st };
    assert!(x.end_right(&lie_tk, &lie.claimer, &lie.outs, held), "the end follows from the last row as it stands");
    // The wrong point is the row after the last chain of no leg: row 3, opened by the account's own chain.
    assert_eq!(x.wrong_point(&lie_tk, &lie.claimer, &lie.outs, held, &a), Some(5));
    let f = [fund(xk(&w.w), 34)];
    let live = w.stated(&account, &lie, &lie_tk.said());
    let q = w.pointed(&live, 5);
    let before3 = chain_before(x, 2);
    let (b, right) = step_row_at(&w.lib, &q, &lie_tk, 5, &before3, &x.legs[2], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(!right, "the chain that opens row 3 is not the chain of row 2");
    let s = signed("row after a chain of no leg: wrong", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())), "the account again");
    let forced = step_row_at(&w.lib, &q, &lie_tk, 5, &before3, &x.legs[2], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), Some(true));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "it is answered right");
    // Opened by the trace's own chain of row 2 there is no leg: neither verdict that way.
    assert!(step_row(&w.lib, &q, &lie_tk, &x.legs[2], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), None).is_err());

    // A dispute of a point nobody can answer (row 1, whose chain is of no leg) does not keep the statement: anyone
    // answers row 3 wrong under it, and the statement ends. The same answer said to be right is refused: only the
    // point disputed is answered right.
    let q = w.pointed(&live, 1);
    for leg in &x.legs {
        assert!(step_row(&w.lib, &q, &lie_tk, leg, &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), None).is_err(), "row 1 has no answer");
    }
    let (b, right) = step_row_at(&w.lib, &q, &lie_tk, 5, &before3, &x.legs[2], &w.script("KpfTStepRow"), xk(&w.f), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    let s = signed("another point answered wrong under a dispute of row 1", &b, &w.keys);
    let n = s.tx.outputs.len();
    assert_eq!(s.tx.outputs[0].script_public_key, hash_spk(&w.lib, &x.hash()));
    assert_eq!((s.tx.outputs[n - 2].value, &s.tx.outputs[n - 2].script_public_key), (lie.bond as u64, &p2pk_spk(&xk(&w.w))), "the disputer's bond back");
    assert_eq!((s.tx.outputs[n - 1].value, &s.tx.outputs[n - 1].script_public_key), ((lie.bond / 2) as u64, &p2pk_spk(&xk(&w.f))), "half of the claimer's to who answered");
    // On an honest statement, a point other than the one disputed is not answered right (that would cost the
    // disputer its bond for a point it did not dispute).
    let q = w.pointed(&w.stated(&account, &st, &tk.said()), 1);
    let r = step_row_at(&w.lib, &q, &tk, 5, &tk.rows[1].g, &x.legs[2], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), Some(true));
    assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "another point than the one disputed is answered right");
    let r = step_end(&w.lib, &q, x, &tk, &w.script("KpfTStepEnd"), xk(&w.w), w.payer(&w.w, &f), Some(true));
    assert!(r.is_err() || !accepted(&r.unwrap().0, &w.keys), "the end is answered right under a dispute of a row");
}

#[test]
fn a_row_of_the_claimers_making_before_the_accounts_own_rows_is_answered_wrong() {
    use kpf_protocol::thin::{row_after, row_judged};
    let w = world();
    let account = w.account(3);
    let x = &account.s;
    let held = account.value as i64;
    let a = prices(3);
    let (st, _) = x.take(SIDE_A, KAS1, &a, T0, held).unwrap();
    // The trace begins with a row that is of no leg: 30 KAS of gain, and the seed of every chain (32 zero bytes) as
    // its chain. The account's own three rows stand on it, each following from the one before.
    let mut rows = vec![Row { a: P10, gain: 30 * KAS1, wa: 0, wb: 0, ea: 0, eb: 0, g: [0; 32], h: [0; 32] }];
    for (i, l) in x.legs.iter().enumerate() {
        let next = row_after(&rows[i], l, a[i], false);
        rows.push(next);
    }
    let amount = KAS1 + 30 * KAS1;
    let lie_tk = ATake { side: SIDE_A, amount, rows, skip: 0, leave: None };
    let last = lie_tk.rows[3];
    assert_eq!(last.g, x.legs_root(), "the last row carries the account's root");
    let next = Account { a_margin: x.a_margin + last.gain - amount, b_margin: x.b_margin - last.gain, ..x.clone() };
    let p33 = |h: &[u8; 32]| [&[0x20u8][..], h].concat();
    let h = blake2b(&[p33(&blake2b(&[x.terms(), p33(&last.h)].concat())), next.moving()].concat());
    let lie = Stated { arg: lie_tk.arg(), outs: Outs::of(&[Out { key: false, to: h, value: held - amount }, Out { key: true, to: xk(&w.a), value: amount }]).unwrap(), ..st };
    // Its end is right by its last row, and each of the account's rows follows from the row before it.
    assert!(x.end_right(&lie_tk, &lie.claimer, &lie.outs, held));
    for i in 1..=3 {
        assert_eq!(row_judged(&lie_tk.rows[i - 1].g, &x.legs[i - 1], &lie_tk.rows[i - 1], &lie_tk.rows[i], false), Some(true), "row {}", i + 1);
    }
    // The wrong point is the first row. Nobody could open its chain: it is answered wrong with no leg of the trace.
    assert_eq!(x.wrong_point(&lie_tk, &lie.claimer, &lie.outs, held, &a), Some(1));
    let f = [fund(xk(&w.w), 35)];
    let live = w.stated(&account, &lie, &lie_tk.said());
    let q = w.pointed(&live, 1);
    let (b, right) = step_row_at(&w.lib, &q, &lie_tk, 1, &[0; 32], &x.legs[0], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    let s = signed("a row of no leg answered wrong", &b, &w.keys);
    assert_eq!((s.tx.outputs[0].value, &s.tx.outputs[0].script_public_key), (account.value, &hash_spk(&w.lib, &x.hash())), "the account again");
    let forced = step_row_at(&w.lib, &q, &lie_tk, 1, &[0; 32], &x.legs[0], &w.script("KpfTStepRow"), xk(&w.w), w.payer(&w.w, &f), Some(true));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a row of no leg is answered right");
    // And under a dispute of another point (its end, which is right), anyone answers that row wrong.
    let q = w.pointed(&live, 0);
    let (b, right) = step_row_at(&w.lib, &q, &lie_tk, 1, &[0; 32], &x.legs[1], &w.script("KpfTStepRow"), xk(&w.f), w.payer(&w.w, &f), None).unwrap();
    assert!(!right);
    signed("a row of no leg answered wrong under a dispute of the end", &b, &w.keys);
    // A trace of fewer rows than the account has legs: its first row carries a chain that is opened by the account's
    // own chain before that leg, which is not the nothing before a first row: wrong.
    let short = ATake { side: SIDE_A, amount: 0, rows: vec![Row { a: a[2], gain: 0, wa: 0, wb: 0, ea: 0, eb: 0, g: x.legs_root(), h: [0x55; 32] }], skip: 0, leave: None };
    let lie = Stated { arg: short.arg(), ..st };
    assert_eq!(x.wrong_point(&short, &lie.claimer, &lie.outs, held, &a), Some(0), "its end is wrong by its outputs");
    let before3 = chain_before(x, 2);
    assert_eq!(row_judged(&before3, &x.legs[2], &Row::ZERO, &short.rows[0], false), Some(false));
}

