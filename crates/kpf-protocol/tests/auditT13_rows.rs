//! Review T13: the account's side, stage 1 (docs/ARCHITECTURE-CLEARING.md 17.8h), at the engine. The reviewer's
//! tests; its report is docs/audit/REVIEW-T13-rows.md.
//!
//! A test named `t13_wN_...` shows the finding WN of the report as it stands (it passes while the finding is
//! open, and says so). A test named `t13_held_...` is of what was attacked and held.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::covered::{c_row_judged_of, chain_of_one, CSaid, ALL, DIR_SPOT, KIND_C_LEAVE, KIND_C_MARK, KIND_C_SPLIT, KIND_C_STEP, KIND_C_TAKE, KIND_C_TOK_OUT};
use kpf_protocol::cross::{XLeg, XTerms};
use kpf_protocol::crow::{crow_after, CHead, CRow, Part};
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::reserve::NOTE_MIN;
use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::{Account, Out, Outs, Pointed, Stated};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;
const P10: i64 = 1_000_000_000;
const SID: [u8; 32] = [0x5d; 32];

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
fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
    let o = &sg.tx.outputs[i];
    OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
}

/// The world of `thin_draw.rs`, without its committee (no price is cited here): the user `w`, the clearing party
/// `c`, a third key `f`, four markets, the library.
struct World {
    c: Keypair,
    w: Keypair,
    f: Keypair,
    keys: KeySet,
    markets: Vec<CMarket>,
    lib: [u8; 32],
    scripts: Vec<LibScript>,
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
    fn who(&self, key: &[u8; 32]) -> &Keypair {
        if *key == xk(&self.c) {
            &self.c
        } else if *key == xk(&self.f) {
            &self.f
        } else {
            &self.w
        }
    }
    /// The statement `st` stated on the account as it is given (what the script of a statement takes, it takes).
    fn stated(&self, acct: &OLive<Account>, st: &Stated, said: &[u8]) -> OLive<Stated> {
        let who = self.who(&st.claimer);
        let f = [fund(xk(who), 34)];
        let b = account_state(&self.lib, acct, &self.script("KpfTChange"), st, said, self.payer(who, &f)).unwrap();
        let sg = b.sign(&self.keys).expect("stated");
        let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
        assert!(v.is_ok(), "the statement is stated: {:?}", v.errors());
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
    let mut keys = KeySet::new();
    for k in [&c, &w, &f] {
        keys.add_keypair(*k);
    }
    let rid = Hash::from_bytes([0x52; 32]);
    let mut markets: Vec<CMarket> = (0..4u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
    markets.sort_by_key(|m| kpf_protocol::books::Key::futures(m.id()).numbers());
    let funds = vec![fund(xk(&f), 200)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    World { c, w, f, keys, markets, lib, scripts: lib_scripts(&gs.tx, 6_000) }
}

/// A futures leg of market `i`: `cents` hundredths of a unit.
fn fleg_c(w: &World, i: usize, cents: i64, dir: i64, entry: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: cents * (m.p.scale / 100), dir, entry }
}
fn fleg(w: &World, i: usize, units: i64, dir: i64, entry: i64) -> XLeg {
    fleg_c(w, i, 100 * units, dir, entry)
}
/// A token leg: `cents` hundredths of a unit of a token priced by market `i`, at a discount of 20 %.
fn tleg_c(w: &World, i: usize, cents: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: [0x99; 32], scale: m.p.scale, price_max: m.p.price_max, size: cents * (m.p.scale / 100), dir: DIR_SPOT, entry: 2_000 }
}

/// A covered account of the user's (`w`) under a shard, as an output stands on the chain: its coins, its legs.
fn cov(w: &World, legs: &[XLeg], coins: i64) -> OLive<Account> {
    cov_owing(w, legs, coins, 0)
}
fn cov_owing(w: &World, legs: &[XLeg], coins: i64, owes: i64) -> OLive<Account> {
    let s = Account { t: w.terms(), legs: legs.to_vec(), a_key: xk(&w.w), b_key: xk(&w.c), a_margin: coins, b_margin: owes, expiry: 0, dispute_daa: 50, bond_bps: 1_000, a_min_counter: legs.len() as i64, b_min_counter: 0, shard: Some(SID) };
    OLive { s, value: coins as u64, id: Some(Hash::from_bytes([0x73; 32])), op: op(80), daa: 6_100 }
}

/// A statement on a covered account written by hand, whatever the rule says of it: its outputs, the account as it is.
fn by_hand(x: &OLive<Account>, kind: i64, s: &CSaid, claimer: [u8; 32]) -> Stated {
    let held = x.value as i64;
    Stated { position: x.s.hash(), window: x.s.dispute_daa, late: x.s.t.late_daa, kind, t0: T0, arg: s.arg(), outs: Outs::of(&[Out { key: false, to: x.s.hash(), value: held }]).unwrap(), bond: x.s.c_bond_for(kind, s.amount), claimer, held }
}

fn verdicts(model: Option<bool>) -> (bool, bool) {
    match model {
        Some(true) => (true, false),
        Some(false) => (false, true),
        None => (false, false),
    }
}

/// What the engine takes of the answer of a row: (right, wrong).
fn row_taken(w: &World, q: &OLive<Pointed>, s: &CSaid, at: i64, prev: &[u8; 32], lb: Option<&XLeg>, leg: &XLeg) -> (bool, bool) {
    let take = |v: bool| {
        let f = [fund(xk(&w.w), 36)];
        c_row(&w.lib, q, s, at, prev, lb, leg, &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.w, &f), Some(v)).is_ok_and(|b| accepted(&b.0, &w.keys))
    };
    (take(true), take(false))
}

fn end_script(kind: i64) -> &'static str {
    match kind {
        12 => "KpfTCSplit",
        13 => "KpfTCMark",
        14 | 15 => "KpfTCTok",
        _ => "KpfTCEnd",
    }
}

/// What the engine takes of the answer of the end of a statement as it stands stated: (right, wrong).
fn end_taken(w: &World, acct: &OLive<Account>, live: &OLive<Stated>, s: &CSaid) -> (bool, bool) {
    let q = w.pointed(live, 0);
    let take = |v: bool| {
        let f = [fund(xk(&w.w), 35)];
        c_end(&w.lib, &q, &acct.s, s, &w.script(end_script(live.s.kind)), xk(&w.f), w.payer(&w.w, &f), Some(v)).is_ok_and(|b| accepted(&b.0, &w.keys))
    };
    (take(true), take(false))
}

/// The model's statement of the rule's, stated; and what the engine takes of its end.
fn ruled(w: &World, acct: &OLive<Account>, kind: i64, amount: i64, skip: i64, a: &[i64], claimer: &Keypair) -> Option<(CSaid, kpf_protocol::covered::COutcome, (bool, bool))> {
    let (st, s, o) = acct.s.c_state(kind, amount, skip, a, T0, acct.value as i64, xk(claimer)).ok()?;
    let live = w.stated(acct, &st, &s.said());
    let e = end_taken(w, acct, &live, &s);
    Some((s, o, e))
}

/// A statement by hand (the outputs: the account as it is), stated; the model's verdict on its end and the engine's.
fn handed(w: &World, acct: &OLive<Account>, kind: i64, s: &CSaid, claimer: &Keypair) -> (Option<bool>, (bool, bool)) {
    let st = by_hand(acct, kind, s, xk(claimer));
    let model = acct.s.c_end_judged(kind, s, &st.claimer, &st.outs, st.held, T0);
    let live = w.stated(acct, &st, &s.said());
    (model, end_taken(w, acct, &live, s))
}

// ================================================================================================================
// W1 (closed). A due account that holds a token leg and a partly hedged group is stepped.

/// A long of 10 and a short of 9 of one market, entered at 10, and a tenth of a token; the coins half a KAS above
/// a note's least. At 9 the long lost 10 and the short gained 9: the account is under by the price the rows say.
/// As the stage was first built (W1), no step of anyone's was right: without the long the account lacks more, so
/// every leg would leave, and every leg is refused beside a token leg. Closed: an account that holds tokens is
/// not taken whole; the long, named, leaves alone, the coins paying what they can and the account owing the rest
/// (16.18). Then the short, which gained beside no leg that lost, is named; and the tokens of an account that
/// owes are sold. Named first, the short is wrong (the long lost: E1), and the token leg is named by no step.
#[test]
fn t13_w1_closed_a_due_account_with_a_token_leg_and_a_partly_hedged_group_is_stepped() {
    let w = world();
    let (long, short, tok) = (fleg(&w, 0, 10, LONG, P10), fleg(&w, 0, 9, SHORT, P10), tleg_c(&w, 1, 10));
    let coins = NOTE_MIN + KAS1 / 2;
    let acct = cov(&w, &[long, short, tok], coins);
    acct.s.c_check().expect("a covered account of the form");
    for price in [P10 * 9 / 10, P10 * 8 / 10] {
        let a = [price, price, P10];
        let q = acct.s.c_step_prices(&a).unwrap();
        let last = *acct.s.c_trace_step(&a, &q, 1).unwrap().last().unwrap();
        let (m, tk) = (coins - NOTE_MIN, last.tk);
        assert_eq!(tk, 8 * KAS1 / 10 + 1);
        assert!(last.gain + last.m < 0 && m + tk + last.gain + last.m < 0, "the account is due at {price}");
        // Named, the short (it gained while the long lost) and the token leg: no step.
        for skip in [2i64, 3] {
            assert!(acct.s.c_state(KIND_C_STEP, 0, skip, &a, T0, coins, xk(&w.f)).is_err(), "a step that names leg {skip} at {price}");
        }
        // Named, the long: it leaves alone; the coins pay what they hold above a note's least, no fee, and the
        // account owes the rest. The engine takes the end right, and not wrong.
        let lost = 10 * (P10 - price);
        let (s, o, e) = ruled(&w, &acct, KIND_C_STEP, 0, 1, &a, &w.f).expect("W1 is open: no step names the long");
        let after = o.after.clone().unwrap();
        assert_eq!(after.legs, vec![short, tok]);
        assert_eq!((after.a_margin, after.b_margin, s.amount, o.note.clone().unwrap().asked, e), (NOTE_MIN, lost - m, 0, -m, (true, false)), "at {price}");
        // The account that goes on: the short gained beside no leg that lost, and is named; it leaves alone (an
        // account that holds tokens is not taken whole), what it gained beyond the band set against what is owed.
        let next = OLive { s: after.clone(), value: after.a_margin as u64, ..acct.clone() };
        // (On the model: the account now holds a note's least, its statement's bond is the least bond, and the
        // answer of a disputed end would pay a quarter of it in an output a block does not take beside the
        // account's own: docs 17.8h, after review T13.)
        let (_, s2, o2) = next.s.c_state(KIND_C_STEP, 0, 1, &[price, P10], T0, next.value as i64, xk(&w.f)).expect("the short is named, the long gone");
        let after2 = o2.after.clone().unwrap();
        assert_eq!((after2.legs.clone(), s2.amount, after2.a_margin, o2.note.clone().map(|n| n.asked)), (vec![tok], 0, NOTE_MIN, Some(0)), "the second step at {price}");
        assert!(after2.b_margin < after.b_margin && after2.b_margin > 0, "what the short gained is set against what is owed: {} then {}", after.b_margin, after2.b_margin);
        // And the tokens of an account that owes are sold.
        let last2 = OLive { s: after2.clone(), value: after2.a_margin as u64, ..acct.clone() };
        // (At or after the time the account says: a step that made it owe wrote it. 16.18.)
        assert!(last2.s.b_min_counter > T0 && last2.s.c_state(kpf_protocol::covered::KIND_C_TOK_SOLD, 0, 1, &[P10], last2.s.b_min_counter, last2.value as i64, xk(&w.f)).is_ok(), "the tokens of an account that owes are sold");
    }
    // The same legs with the tokens' 0.8 KAS as coins: stepped at once, of every leg.
    let plain = cov(&w, &[long, short], coins + 8 * KAS1 / 10);
    let (_, o, e) = ruled(&w, &plain, KIND_C_STEP, 0, 1, &[P10 * 9 / 10; 2], &w.f).expect("a step");
    assert_eq!((o.after.is_none(), e), (true, (true, false)));
    println!("T13 W1 (closed): long 10, short 9, a tenth of a token, coins 0.5 KAS above a note's least: due at 9 and at 8, the long named leaves alone and the account owes the rest; then the short; then the tokens are sold");
}

// ================================================================================================================
// 1. `KpfTCRow` against `crow.rs`, number by number.

const NAMES: [&str; 21] = ["a", "gain", "w", "ws", "ls", "gl", "tk", "q", "m", "ba", "bl", "dda", "dua", "ra", "dds", "dus", "rs", "rl", "pa", "ps", "po"];

fn number(r: &mut CRow, k: usize) -> &mut i64 {
    match k {
        0 => &mut r.a,
        1 => &mut r.gain,
        2 => &mut r.w,
        3 => &mut r.ws,
        4 => &mut r.ls,
        5 => &mut r.gl,
        6 => &mut r.tk,
        7 => &mut r.q,
        8 => &mut r.m,
        9 => &mut r.ba,
        10 => &mut r.bl,
        11 => &mut r.dda,
        12 => &mut r.dua,
        13 => &mut r.ra,
        14 => &mut r.dds,
        15 => &mut r.dus,
        16 => &mut r.rs,
        17 => &mut r.rl,
        18 => &mut r.pa,
        19 => &mut r.ps,
        _ => &mut r.po,
    }
}

/// What leg `j` (from 0) does in a statement of `kind` that names `skip` (a mark: entered from now at `amount`).
fn part_of(kind: i64, skip: i64, amount: i64, j: usize, l: &XLeg) -> Part {
    let named = skip == ALL || skip == j as i64 + 1;
    let tok = kind == KIND_C_TOK_OUT || kind == kpf_protocol::covered::KIND_C_TOK_SOLD;
    if !named || (l.dir == DIR_SPOT && !tok) {
        Part::Stays
    } else if kind == KIND_C_MARK {
        Part::Marked(amount)
    } else {
        Part::Leaves
    }
}

/// The statements the walks are of: (what, the account, the kind, the statement of the model's trace).
fn walked(w: &World) -> Vec<(&'static str, OLive<Account>, i64, CSaid)> {
    // Five legs: a group of market 0 that is net long, a hedge of market 1, and a leg of market 0 apart.
    let legs = [fleg(w, 0, 10, LONG, P10), fleg(w, 0, 6, SHORT, P10 * 105 / 100), fleg(w, 1, 5, LONG, P10 * 9 / 10), fleg(w, 1, 5, SHORT, P10 * 92 / 100), fleg(w, 0, 3, LONG, P10)];
    let a = [P10 * 11 / 10, P10 * 11 / 10, P10 * 95 / 100, P10 * 95 / 100, P10 * 109 / 100];
    let acct = cov(w, &legs, 100 * KAS1);
    let head = acct.s.c_head();
    let lc = |skip: i64| if skip >= 1 { chain_of_one(&acct.s.legs[skip as usize - 1]) } else { [0; 32] };
    let mut v = vec![];
    for (what, kind, skip) in [("a take", KIND_C_TAKE, 0), ("a leaving of a leg inside a group", KIND_C_LEAVE, 2), ("a leaving of every leg", KIND_C_LEAVE, ALL), ("a split", KIND_C_SPLIT, 4)] {
        v.push((what, acct.clone(), kind, CSaid { amount: if kind == KIND_C_SPLIT { 10 * KAS1 } else { 0 }, skip, lc: lc(skip), head, rows: acct.s.c_trace(&a, skip).unwrap() }));
    }
    // A step: the prices that show it due (each group's own, off its row's).
    let q = acct.s.c_step_prices(&a).unwrap();
    assert!(q.iter().zip(&a).all(|(x, y)| x != y) && q[0] == q[1] && q[2] == q[3]);
    for skip in [1i64, 5] {
        v.push((if skip == 1 { "a step that names the first leg" } else { "a step that names the last leg" }, acct.clone(), KIND_C_STEP, CSaid { amount: 0, skip, lc: lc(skip), head, rows: acct.s.c_trace_step(&a, &q, skip).unwrap() }));
    }
    // A mark of the first leg (it gained).
    let ne = acct.s.mark_entry(&legs[0], a[0]).unwrap();
    v.push(("a mark", acct.clone(), KIND_C_MARK, CSaid { amount: ne, skip: 1, lc: lc(1), head, rows: acct.s.c_trace_marked(&a, 1, ne).unwrap() }));
    // A token leg between two legs of one market, and one after: it stays (a take), and it leaves (14).
    let tlegs = [fleg(w, 0, 10, LONG, P10), tleg_c(w, 2, 300), fleg(w, 0, 10, SHORT, P10), tleg_c(w, 0, 50)];
    let ta = [P10 * 11 / 10, P10 * 2, P10 * 108 / 100, P10 * 108 / 100];
    let tacct = cov(w, &tlegs, 100 * KAS1);
    v.push(("a take beside token legs", tacct.clone(), KIND_C_TAKE, CSaid { amount: 0, skip: 0, lc: [0; 32], head, rows: tacct.s.c_trace(&ta, 0).unwrap() }));
    v.push(("tokens leave from between two legs of one market", tacct.clone(), KIND_C_TOK_OUT, CSaid { amount: 0, skip: 2, lc: chain_of_one(&tlegs[1]), head, rows: tacct.s.c_trace_tok(KIND_C_TOK_OUT, &ta, 2).unwrap() }));
    v
}

fn claimer_of<'a>(w: &'a World, kind: i64) -> &'a Keypair {
    if kind == KIND_C_STEP {
        &w.f
    } else {
        &w.w
    }
}

/// The model's verdict on row `i` (from 1) of `s` as the account's legs open it, and the engine's.
fn row_both(w: &World, acct: &OLive<Account>, kind: i64, s: &CSaid, i: usize) -> (Option<bool>, (bool, bool)) {
    let legs = &acct.s.legs;
    let before = if i == 1 { CRow::ZERO } else { s.rows[i - 2] };
    let prev = before.g;
    let lb = i.checked_sub(2).map(|k| &legs[k]);
    let model = c_row_judged_of(kind, &prev, lb, &legs[i - 1], &before, &s.rows[i - 1], i as i64, s);
    let live = w.stated(acct, &by_hand(acct, kind, s, xk(claimer_of(w, kind))), &s.said());
    let q = w.pointed(&live, 2 * i as i64 - 1);
    (model, row_taken(w, &q, s, 2 * i as i64 - 1, &prev, lb, &legs[i - 1]))
}

/// Every number of every row, a sompi more and a sompi less, in statements of kinds 7, 8, 9, 12, 13 and 14 (a step
/// with prices of its own; a token leg between two legs of one market): the row is wrong by the model and
/// answered wrong, and not right, at the engine. And the rows as the model writes them are answered right.
#[test]
fn t13_held_every_number_of_a_row_a_sompi_off_is_answered_wrong() {
    let w = world();
    let (mut cases, mut honest) = (0, 0);
    let mut parts: Vec<String> = vec![];
    for (what, acct, kind, s) in walked(&w) {
        for i in 1..=s.rows.len() {
            let got = row_both(&w, &acct, kind, &s, i);
            honest += 1;
            if got != (Some(true), (true, false)) {
                parts.push(format!("{what}: row {i} as the model writes it: {got:?}"));
            }
            for k in 0..21 {
                for d in [1i64, -1] {
                    let mut t = s.clone();
                    *number(&mut t.rows[i - 1], k) += d;
                    let got = row_both(&w, &acct, kind, &t, i);
                    cases += 1;
                    if got != (Some(false), (false, true)) {
                        parts.push(format!("{what}: row {i}, {} by {d}: the model says {:?}, the engine takes (right, wrong) = {:?}", NAMES[k], got.0, got.1));
                    }
                }
            }
        }
    }
    println!("T13 rows, a sompi off: {honest} rows of the model's answered right; {cases} rows with one number a sompi off, each wrong by the model and answered wrong at the engine; apart: {}: {:?}", parts.len(), parts.iter().take(8).collect::<Vec<_>>());
    assert!(parts.is_empty(), "{} rows part", parts.len());
}

/// A row follows from the row before as that row says it, whatever it says: each number of the row before a sompi
/// off, and the two sums of a group at every pair of signs where the group closes (and at the ends of 64 bits),
/// with the row the model computes from it: the engine takes the model's verdict (right; or neither where a sum
/// does not fit; or wrong where no right row follows).
#[test]
fn t13_held_a_row_follows_from_the_row_before_as_it_is_said_and_a_group_closes_by_the_larger_loss() {
    let w = world();
    let (mut right, mut wrong, mut neither) = (0, 0, 0);
    let mut parts: Vec<String> = vec![];
    let big = i64::MAX;
    let pairs: [(i64, i64); 14] = [(-5, -7), (-7, -5), (5, -7), (-7, 5), (5, 7), (7, 5), (0, 0), (0, -1), (-1, 0), (-1, -1), (big, 1), (-big, 0), (0, -big), (-big, -big)];
    for (what, acct, kind, s) in walked(&w) {
        let legs = acct.s.legs.clone();
        for i in 2..=s.rows.len() {
            let mut bents: Vec<(String, CRow)> = vec![];
            for k in 0..21 {
                for d in [1i64, -1] {
                    let mut b = s.rows[i - 2];
                    *number(&mut b, k) += d;
                    bents.push((format!("{} by {d}", NAMES[k]), b));
                }
            }
            for (x, y) in pairs {
                bents.push((format!("the sums of all at ({x}, {y})"), CRow { dda: x, dua: y, ..s.rows[i - 2] }));
                bents.push((format!("the sums of those that stay at ({x}, {y})"), CRow { dds: x, dus: y, ..s.rows[i - 2] }));
                bents.push((format!("both at ({x}, {y}), R near the end of 64 bits"), CRow { dda: x, dua: y, dds: y, dus: x, ra: big - 6, rs: big - 6, ..s.rows[i - 2] }));
            }
            for (bent, b) in bents {
                let mut t = s.clone();
                t.rows[i - 2] = b;
                let l = &legs[i - 1];
                // (The row the model computes from the row before as bent; where it computes none, the row as it was.)
                if let Some(Some(r)) = crow_after(kind, &s.head, &b, Some(&legs[i - 2]), l, s.rows[i - 1].a, s.rows[i - 1].q, part_of(kind, s.skip, s.amount, i - 1, l)) {
                    t.rows[i - 1] = r;
                }
                let (model, engine) = row_both(&w, &acct, kind, &t, i);
                match model {
                    Some(true) => right += 1,
                    Some(false) => wrong += 1,
                    None => neither += 1,
                }
                if engine != verdicts(model) {
                    parts.push(format!("{what}: row {i} after a row before with {bent}: the model says {model:?}, the engine takes {engine:?}"));
                }
            }
        }
    }
    println!("T13 rows after a row before that is bent: {right} right, {wrong} wrong, {neither} by neither verdict; the engine parts from the model on {}: {:?}", parts.len(), parts.iter().take(8).collect::<Vec<_>>());
    assert!(right >= 500 && wrong >= 20 && neither >= 20, "{right} {wrong} {neither}");
    assert!(parts.is_empty(), "{} rows part", parts.len());
}

fn up_of(v: i64, bps: i64) -> i64 {
    (v / 10_000) * bps + ((v % 10_000) * bps + 9_999) / 10_000
}

/// A step's price at its edges, and a head at its edges: the rule written out here, the model, and the engine.
/// (W5: a step's price so far below nothing that its distance from the row's does not fit is wrong by the model
/// and answered by neither verdict at the engine.)
#[test]
fn t13_a_steps_price_and_a_head_at_their_edges() {
    let w = world();
    let a = P10 * 95 / 100;
    let acct = cov(&w, &[fleg(&w, 0, 10, LONG, P10)], NOTE_MIN + 9 * KAS1);
    let cheap = cov(&w, &[fleg(&w, 0, 10, LONG, 1)], NOTE_MIN + 9 * KAS1);
    let own = acct.s.c_head();
    let dq = up_of(a, 450);
    let head = |rate: i64, tolerance: i64| CHead { rate, tolerance, ..own };
    // (the account, the head, the row's price, the step's price, what the rule says of the row)
    let mut cases: Vec<(&str, &OLive<Account>, CHead, i64, i64, bool)> = vec![
        ("the lower rate below", &acct, own, a, a - dq, true),
        ("a sompi beyond it", &acct, own, a, a - dq - 1, false),
        ("the lower rate above", &acct, own, a, a + dq, true),
        ("a sompi beyond it", &acct, own, a, a + dq + 1, false),
        ("the row's own price", &acct, own, a, a, true),
        ("a price below nothing", &acct, own, a, -1, false),
        ("the largest number", &acct, own, a, i64::MAX, false),
        ("a head of no rate: the row's price", &acct, head(0, 0), a, a, true),
        ("a head of no rate: a sompi off", &acct, head(0, 0), a, a - 1, false),
        ("a head whose tolerance twice is above its rate: the row's price", &acct, head(550, 300), a, a, true),
        ("a head whose tolerance twice is above its rate: a sompi off", &acct, head(550, 300), a, a + 1, false),
        ("a head of the whole: a price of nothing", &acct, head(10_000, 0), a, 0, true),
        ("a head of the whole: twice the price", &acct, head(10_000, 0), a, 2 * a, true),
        ("a head of the whole: a sompi beyond", &acct, head(10_000, 0), a, 2 * a + 1, false),
        ("a head of more than the whole", &acct, head(10_001, 0), a, a, false),
        ("a head of a rate below nothing", &acct, head(-1, 0), a, a, false),
        ("a head of a tolerance below nothing", &acct, head(550, -1), a, a, false),
        ("a head of a tolerance above the whole", &acct, head(10_000, 10_001), a, a, false),
        ("a price of one sompi: a step's price of nothing", &cheap, own, 1, 0, true),
        ("a price of one sompi: a step's price of two", &cheap, own, 1, 2, true),
        ("a price of one sompi: a step's price of three", &cheap, own, 1, 3, false),
    ];
    cases.push(("W5: a step's price at the least number", &acct, own, a, -i64::MAX, false));
    let (mut n, mut w5) = (0, None);
    for (what, x, h, a, q, rule) in cases {
        let l = &x.s.legs[0];
        let honest = crow_after(KIND_C_STEP, &own, &CRow::ZERO, None, l, a, a, Part::Leaves).unwrap().unwrap();
        let row = match crow_after(KIND_C_STEP, &h, &CRow::ZERO, None, l, a, q, Part::Leaves) {
            Some(Some(r)) => r,
            _ => CRow { q, ..honest },
        };
        let s = CSaid { amount: 0, skip: 1, lc: chain_of_one(l), head: h, rows: vec![row] };
        let (model, engine) = row_both(&w, x, KIND_C_STEP, &s, 1);
        n += 1;
        if what.starts_with("W5") {
            w5 = Some((model, engine));
            continue;
        }
        assert_eq!((model, engine), (Some(rule), verdicts(Some(rule))), "{what}");
    }
    // W5 as it stands: wrong by the rule and by the model; the engine takes neither verdict (the script subtracts
    // before it compares). Nobody is paid for answering it; the statement falls when its point lapses.
    assert_eq!(w5, Some((None, (false, false))), "W5 (closed): the model and the engine take neither verdict");
    println!("T13 a step's price and a head at their edges: {n} rows; the rule, the model and the engine agree on all but W5 (a step's price of -(2^63 - 1): wrong by the model, answered by neither verdict)");
}

/// W9. `KpfTCRow` computes what a leg is worth at the two ends before it finds the leg a token leg and counts
/// nothing of them; the model computes none for a token leg. A token leg whose worth at its row's price is within
/// 64 bits and at the upper end is not (9 * 10^18 sompi at the price, 5.5 % more above): the model says the row of
/// the rule's is right, and the engine takes neither verdict. The same leg a tenth the size is answered right.
#[test]
fn t13_w9_a_token_legs_row_is_not_answered_where_its_worth_at_the_upper_end_does_not_fit() {
    let w = world();
    let m = &w.markets[1];
    let tok = |units: i64| XLeg { market: m.id(), committee: [0x99; 32], scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir: DIR_SPOT, entry: 2_000 };
    let mut got = vec![];
    for units in [900_000_000i64, 9_000_000_000] {
        let acct = cov(&w, &[fleg(&w, 0, 10, LONG, P10), tok(units)], 100 * KAS1);
        let rows = acct.s.c_trace(&[P10, P10], 0).expect("the model writes the trace");
        let s = CSaid { amount: KAS1, skip: 0, lc: [0; 32], head: acct.s.c_head(), rows };
        got.push(row_both(&w, &acct, KIND_C_TAKE, &s, 2));
    }
    assert_eq!(got[0], (Some(true), (true, false)), "a token leg worth 9 * 10^17");
    assert_eq!(got[1], (Some(true), (true, false)), "W9 (closed): a token leg's row is answered right whatever its worth at the upper end");
    println!("T13 W9: a token leg worth 9 * 10^18 sompi at its row's price: right by the model, answered by neither verdict at the engine");
}

/// The leg before (item 2). A take of three legs: a long and a short of one market, a long of another.
/// Row 1 is answered whatever leg before is shown (the script reads none; W6: the model says "neither"). Rows 2
/// and 3 are answered right with the leg before, and by neither verdict with another leg, a leg a sompi larger, or
/// none. Where the chain two rows before is false, row 3 takes neither verdict, and row 2, which carries the false
/// chain before it, is answered wrong with any leg before or none, also as the answer of a dispute of row 3. A row
/// more than the account has legs, of a leg nobody holds: right as a row, and the end is wrong.
#[test]
fn t13_held_the_leg_before_is_asked_where_the_row_stands_on_the_row_before() {
    let w = world();
    let legs = [fleg(&w, 0, 10, LONG, P10), fleg(&w, 0, 10, SHORT, P10), fleg(&w, 1, 5, LONG, P10)];
    let a = [P10 * 11 / 10, P10 * 11 / 10, P10 * 9 / 10];
    let acct = cov(&w, &legs, 100 * KAS1);
    let (st, s, _) = acct.s.c_state(KIND_C_TAKE, KAS1, 0, &a, T0, acct.value as i64, xk(&w.w)).unwrap();
    let live = w.stated(&acct, &st, &s.said());
    let g = [[0u8; 32], s.rows[0].g, s.rows[1].g];
    let larger = XLeg { size: legs[0].size + 1, ..legs[0] };
    let mut n = 0;
    // (the row, the leg before shown, what the engine takes)
    let shown: [(usize, Option<&XLeg>, (bool, bool)); 10] = [
        (1, None, (true, false)),
        (1, Some(&legs[1]), (true, false)),
        (2, Some(&legs[0]), (true, false)),
        (2, None, (false, false)),
        (2, Some(&legs[2]), (false, false)),
        (2, Some(&larger), (false, false)),
        (2, Some(&legs[1]), (false, false)),
        (3, Some(&legs[1]), (true, false)),
        (3, Some(&legs[0]), (false, false)),
        (3, None, (false, false)),
    ];
    for (i, lb, want) in shown {
        let q = w.pointed(&live, 2 * i as i64 - 1);
        assert_eq!(row_taken(&w, &q, &s, 2 * i as i64 - 1, &g[i - 1], lb, &legs[i - 1]), want, "row {i} shown {lb:?}");
        n += 1;
    }
    // W6: the model says of row 1 shown a leg before that the script takes neither verdict.
    assert_eq!(c_row_judged_of(KIND_C_TAKE, &g[0], Some(&legs[1]), &legs[0], &CRow::ZERO, &s.rows[0], 1, &s), Some(true), "W6 (closed): the model reads no leg before of the first row, as the script");
    // The chain two rows before, false: row 1 says a chain of its own.
    let mut bent = s.clone();
    bent.rows[0].g = [0x66; 32];
    let lv = w.stated(&acct, &Stated { arg: bent.arg(), ..st }, &bent.said());
    for lb in [Some(&legs[1]), Some(&legs[0]), None] {
        assert_eq!(row_taken(&w, &w.pointed(&lv, 5), &bent, 5, &g[2], lb, &legs[2]), (false, false), "row 3 with a false chain two rows before");
        assert_eq!(row_taken(&w, &w.pointed(&lv, 3), &bent, 3, &g[1], lb, &legs[1]), (false, true), "row 2 after a row of a false chain");
        // (Disputed, row 3; answered, that row 2 is wrong.)
        assert_eq!(row_taken(&w, &w.pointed(&lv, 5), &bent, 3, &g[1], lb, &legs[1]), (false, true), "row 2 as the answer of a dispute of row 3");
        n += 3;
    }
    assert_eq!(acct.s.c_wrong_point(KIND_C_TAKE, &bent, &st.claimer, &st.outs, st.held, T0, &a), Some(3), "the model names row 2");
    // A row more, of a leg nobody holds.
    let ghost = fleg(&w, 2, 7, SHORT, P10);
    let mut more = s.clone();
    more.rows.push(crow_after(KIND_C_TAKE, &s.head, &s.rows[2], Some(&legs[2]), &ghost, P10, P10, Part::Stays).unwrap().unwrap());
    let lv = w.stated(&acct, &Stated { arg: more.arg(), ..st }, &more.said());
    assert_eq!(row_taken(&w, &w.pointed(&lv, 7), &more, 7, &s.rows[2].g, Some(&legs[2]), &ghost), (true, false));
    assert_eq!(end_taken(&w, &acct, &lv, &more), (false, true), "the end of a trace of a leg more");
    // A row less.
    let mut less = s.clone();
    less.rows.pop();
    let lv = w.stated(&acct, &Stated { arg: less.arg(), ..st }, &less.said());
    assert_eq!(end_taken(&w, &acct, &lv, &less), (false, true), "the end of a trace of a leg less");
    println!("T13 the leg before: {} answers at the engine as said", n + 3);
}

// ================================================================================================================
// W2, W3. Which step: the fee inside "lacks less", and the stater's choice of the leg.

/// W2 (closed). A long of 100 and a short of 48 of one market from 10, at 9.95: the long lost 5, the short gained
/// 2.4. With coins that pay the long, the account is due up to 25.88 KAS. As first built, the fee counted in what
/// the account lacks after, and above 7.189 KAS of coins every leg left, for a fee of all that was left. Closed:
/// what it lacks after is counted before the fee, and the long leaves alone at every amount of coins that pays
/// it, for the fee of its worth or what the coins hold beyond its loss.
#[test]
fn t13_w2_closed_the_fee_does_not_turn_a_step_of_one_leg_into_a_step_of_every_leg() {
    let w = world();
    let legs = [fleg(&w, 0, 100, LONG, P10), fleg(&w, 0, 48, SHORT, P10)];
    let a = [P10 * 995 / 1_000; 2];
    let step = |m: i64| cov(&w, &legs, NOTE_MIN + m).s.c_state(KIND_C_STEP, 0, 1, &a, T0, NOTE_MIN + m, xk(&w.f)).ok().map(|(_, s, o)| (o.after.is_some(), s.amount));
    let edge = up_of(52 * a[0], 550) - up_of(48 * a[0], 550);
    let whole = 4_975 * KAS1 / 1_000;
    for m in [5 * KAS1, 5 * KAS1 + edge - 1, 5 * KAS1 + edge, 5 * KAS1 + edge + 1, 8 * KAS1, 5 * KAS1 + whole, 12 * KAS1, 20 * KAS1, 25 * KAS1] {
        assert_eq!(step(m), Some((true, whole.min(m - 5 * KAS1))), "W2 is open: coins {m} above a note's least");
    }
    assert_eq!(step(26 * KAS1), None, "not due");
    assert_eq!(step(5 * KAS1 - 1), Some((false, 5 * KAS1 - 1 - 26 * KAS1 / 10)), "the coins do not pay the long: every leg leaves, the fee what is left when the 2.6 they lost together are paid");
    // At the engine, either side of where the fee turned it.
    for m in [5 * KAS1 + edge - 1, 5 * KAS1 + edge, 12 * KAS1] {
        let (_, o, e) = ruled(&w, &cov(&w, &legs, NOTE_MIN + m), KIND_C_STEP, 0, 1, &a, &w.f).unwrap();
        assert_eq!((o.after.is_some(), e), (true, (true, false)));
    }
    println!("T13 W2 (closed): the long leaves alone at every amount of coins that pays it; the fee is its worth's or what the coins hold beyond its loss");
}


/// W3 (closed). A hedge of 100 units of market 0 at 11 (from 10), a long of 10 of market 1 that gained 3, a long
/// of 1 of market 2 that lost 0.4; coins 2.1 KAS above a note's least; due. As the stage was first built, the
/// step that cures (the small leg) paid its stater nothing, and the step that named the hedge's short took every
/// leg for all the coins, the 2.6 KAS the legs gained together falling under a band of the gross worth: the user
/// lost 4.7 KAS by the stater's choice. Closed: what the legs that stay lost is what they lost together; the
/// fee and the band of a step of every leg are of what is asked (the net); and a leg that gained, named, leaves
/// alone where the account lacks less after it.
#[test]
fn t13_w3_closed_a_step_by_what_is_asked() {
    let w = world();
    let legs = [fleg(&w, 0, 100, LONG, P10), fleg(&w, 0, 100, SHORT, P10), fleg(&w, 1, 10, LONG, P10), fleg_c(&w, 2, 100, LONG, P10)];
    let a = [P10 * 11 / 10, P10 * 11 / 10, P10 * 103 / 100, P10 * 96 / 100];
    let m = 21 * KAS1 / 10;
    let acct = cov(&w, &legs, NOTE_MIN + m);
    // The cure: the small leg leaves, for the fee of its worth (0.5 % of 9.6).
    let (_, cure, e) = ruled(&w, &acct, KIND_C_STEP, 0, 4, &a, &w.f).expect("the step that names the small leg");
    assert_eq!(e, (true, false));
    let after = cure.after.clone().unwrap();
    let fee = 48 * KAS1 / 1_000;
    assert_eq!((cure.fee, after.legs.len(), after.a_margin, cure.note.unwrap().asked), (fee, 3, NOTE_MIN + m - 4 * KAS1 / 10 - fee, -4 * KAS1 / 10), "W3 is open: the step that cures pays nothing");
    assert!((1..=3).all(|k| after.c_state(KIND_C_STEP, 0, k, &a[..3], T0, after.a_margin, xk(&w.f)).is_err()), "cured: no step is due on what stays");
    // The hedge's short named (the coins do not pay it): every leg leaves, for the fee and the band of what is
    // asked: 6.193 KAS (the hedge nothing; 5.5 % of 103 and of 9.6), so 0.563 and 1.126. The user is paid its
    // coins less the fee, and the 2.6 its legs gained together less the band.
    let asked = acct.s.c_trace(&a, 0).unwrap().last().unwrap().closed().unwrap().0;
    let (fee_all, band_all) = (asked * 50 / 550, (asked * 100 + 549) / 550);
    assert_eq!((asked, fee_all, band_all), (6_193 * KAS1 / 1_000, 563 * KAS1 / 1_000, 1_126 * KAS1 / 1_000));
    let (_, end, e) = ruled(&w, &acct, KIND_C_STEP, 0, 2, &a, &w.f).expect("the step that names the hedge's short");
    assert_eq!(e, (true, false));
    assert_eq!((end.after.is_none(), end.fee, end.paid, end.note.unwrap().asked), (true, fee_all, NOTE_MIN + m - fee_all, 26 * KAS1 / 10 - band_all), "W3 is open: every leg for all the coins");
    // (A leg that gained is not named while the legs that stay lost together.)
    assert!(acct.s.c_state(KIND_C_STEP, 0, 1, &a, T0, NOTE_MIN + m, xk(&w.f)).is_err() && acct.s.c_state(KIND_C_STEP, 0, 3, &a, T0, NOTE_MIN + m, xk(&w.f)).is_err());
    // With a tenth of a KAS less the account is still due after the small leg left. The long of market 1, which
    // would have to leave, gained beside a hedge that lost nothing together: it is named, and leaves alone.
    let (_, _, o) = cov(&w, &legs, NOTE_MIN + 2 * KAS1).s.c_state(KIND_C_STEP, 0, 4, &a, T0, NOTE_MIN + 2 * KAS1, xk(&w.f)).unwrap();
    let left = o.after.unwrap();
    let again: Vec<Option<bool>> = (1..=3).map(|k| left.c_state(KIND_C_STEP, 0, k, &a[..3], T0, left.a_margin, xk(&w.f)).ok().map(|v| v.2.after.is_none())).collect();
    assert_eq!(again, vec![None, Some(true), Some(false)], "the hedge's long is not named; its short takes every leg; the long of market 1 leaves alone");
    let next = OLive { s: left.clone(), value: left.a_margin as u64, ..acct.clone() };
    let (_, long, e) = ruled(&w, &next, KIND_C_STEP, 0, 3, &a[..3], &w.f).expect("the long of market 1 is named");
    let rest = long.after.clone().unwrap();
    assert_eq!((e, rest.legs.len(), long.note.unwrap().asked), ((true, false), 2, 3 * KAS1 - 103 * KAS1 / 100));
    assert!((1..=2).all(|k| rest.c_state(KIND_C_STEP, 0, k, &a[..2], T0, rest.a_margin, xk(&w.f)).is_err()), "the hedge alone is not due");
    println!("T13 W3 (closed): the step that cures pays the fee of its leg ({fee}); the step that names the hedge's short takes every leg for the fee and the band of what is asked ({fee_all}, {band_all}); a leg that gained beside a hedge is named and leaves alone");
}

// ================================================================================================================
// 3. The ends at their edges, a sompi either side, at the engine. Each `need` is the rule written out here in
// numbers (the rate 5.5 %, the lower rate 4.5 %, the band 1 %, the fee 0.5 %), not read from the model.

/// With coins of `need` the statement is the rule's and its end is answered right; with a sompi less the same
/// statement is not the rule's and its end is answered wrong.
fn edge(w: &World, what: &str, legs: &[XLeg], need: i64, kind: i64, amount: i64, skip: i64, a: &[i64]) -> CSaid {
    let claimer = claimer_of(w, kind);
    let rich = cov(w, legs, need);
    let (s, _, e) = ruled(w, &rich, kind, amount, skip, a, claimer).unwrap_or_else(|| panic!("{what}: not the rule's with {need}"));
    assert_eq!(e, (true, false), "{what}: at the edge");
    let poor = cov(w, legs, need - 1);
    assert!(poor.s.c_state(kind, amount, skip, a, T0, need - 1, xk(claimer)).is_err(), "{what}: a sompi under, by the model");
    assert_eq!(handed(w, &poor, kind, &s, claimer), (Some(false), (false, true)), "{what}: a sompi under, at the engine");
    s
}

/// A step is due with coins of `most` and not with a sompi more (the same statement on the richer account: wrong).
fn due_edge(w: &World, what: &str, legs: &[XLeg], owes: i64, most: i64, skip: i64, a: &[i64]) -> kpf_protocol::covered::COutcome {
    let poor = cov_owing(w, legs, most, owes);
    let (s, o, e) = ruled(w, &poor, KIND_C_STEP, 0, skip, a, &w.f).unwrap_or_else(|| panic!("{what}: not due with {most}"));
    assert_eq!(e, (true, false), "{what}: due");
    let rich = cov_owing(w, legs, most + 1, owes);
    assert!((1..=legs.len() as i64).all(|k| rich.s.c_state(KIND_C_STEP, 0, k, a, T0, most + 1, xk(&w.f)).is_err()), "{what}: a sompi more, by the model");
    assert_eq!(handed(w, &rich, KIND_C_STEP, &s, &w.f), (Some(false), (false, true)), "{what}: a sompi more, at the engine");
    o
}

#[test]
fn t13_held_the_ends_at_their_edges_a_sompi_either_side() {
    let w = world();
    let k = |hundredths: i64| hundredths * KAS1 / 100;
    let (p11, p9) = (P10 * 11 / 10, P10 * 9 / 10);
    // A hedge of 10 units of market 0 from 10, at 11 (the long gained 10, the short lost 10), and a long of 5 of
    // market 1 from 10, at 9 (it lost 5; alone it can lose 2.475 more by the rate).
    let (long, short, other) = (fleg(&w, 0, 10, LONG, P10), fleg(&w, 0, 10, SHORT, P10), fleg(&w, 1, 5, LONG, P10));
    let h = [long, short, other];
    let a = [p11, p11, p9];
    let mut n = 0;
    // 7: what stays is at R of all (the hedge nothing, the long 2.475) and at what each lost alone (15).
    edge(&w, "a take of 1 KAS", &h, NOTE_MIN + k(100) + k(247) + k(1) / 2 + k(1500), KIND_C_TAKE, KAS1, 0, &a);
    // 8, the long that gained leaves: what stays, the short alone (6.05) and the other (2.475), and what they lost.
    edge(&w, "a leg that gained leaves a hedge", &h, NOTE_MIN + k(605) + k(247) + k(1) / 2 + k(1500), KIND_C_LEAVE, 0, 1, &a);
    // 8, the short that lost leaves: the coins pay its 10, and the coins and what all gained (-5) are at R of what
    // stays (the long alone 6.05, the other 2.475).
    edge(&w, "a leg that lost leaves a hedge: what stays at the level of an entry", &h, NOTE_MIN + k(500) + k(605) + k(247) + k(1) / 2, KIND_C_LEAVE, 0, 2, &a);
    // 8, a leg that lost leaves beside a leg that gained much: the coins pay it.
    edge(&w, "a leg that lost leaves: the coins pay it", &[short, fleg(&w, 1, 50, LONG, P10)], NOTE_MIN + k(1000), KIND_C_LEAVE, 0, 1, &[p11, p11]);
    // 12, the other leg leaves for an account of its own with a note's least, what it can lose alone and what it
    // lost (7.475); what stays, the hedge, is asked what its short lost.
    let amount = NOTE_MIN + k(747) + k(1) / 2;
    let s = edge(&w, "a split: what stays", &h, amount + NOTE_MIN + k(1000), KIND_C_SPLIT, amount, 3, &a);
    let rich = cov(&w, &h, amount + NOTE_MIN + k(1000));
    assert!(rich.s.c_state(KIND_C_SPLIT, amount - 1, 3, &a, T0, rich.value as i64, xk(&w.w)).is_err());
    assert_eq!(handed(&w, &rich, KIND_C_SPLIT, &CSaid { amount: amount - 1, ..s }, &w.w), (Some(false), (false, true)), "a split: the new account a sompi under");
    // 12, the long leaves the hedge for an account of its own: alone it can lose 6.05; what stays is asked as a
    // leg that gained left.
    let amount = NOTE_MIN + k(605);
    edge(&w, "a split of a hedge: what stays", &h, amount + NOTE_MIN + k(605) + k(247) + k(1) / 2 + k(1500), KIND_C_SPLIT, amount, 1, &a);
    // 13, a mark of the hedge's long: the account as after a take (the hedge nothing; what the short lost).
    edge(&w, "a mark of a hedge's leg that gained", &[long, short], NOTE_MIN + k(1000), KIND_C_MARK, 0, 1, &[p11, p11]);
    // 14, tokens leave: what stays is at R without what they counted.
    edge(&w, "tokens leave", &[long, tleg_c(&w, 1, 100)], NOTE_MIN + k(550), KIND_C_TOK_OUT, 0, 2, &[P10, P10]);
    // 7, beside tokens that count 8 KAS: no coin that is not there.
    edge(&w, "a take beside tokens", &[long, tleg_c(&w, 1, 100)], NOTE_MIN + k(100), KIND_C_TAKE, KAS1, 0, &[P10, P10]);
    n += 9 * 2 + 1;

    // 9. Due: a long of 10 from 10 at 9.5 lost 5 and loses 4.275 more at the lower rate below: due with coins
    // under 9.275 above a note's least.
    let o = due_edge(&w, "a long alone", &[long], 0, NOTE_MIN + k(927) + k(1) / 2 - 1, 1, &[P10 * 95 / 100]);
    assert!(o.after.is_some_and(|x| x.legs.is_empty()));
    // What the legs gained counts less the band, and no less than nothing: a long that gained 10 at 11 keeps 5.05
    // at the lower rate below, 3.95 of it beyond the band of 1.1; an account that owes 10 is due with coins under
    // 6.05.
    let o = due_edge(&w, "a leg that gained, of an account that owes", &[long], k(1000), NOTE_MIN + k(605) - 1, 1, &[p11]);
    assert!(o.after.is_none() && o.note.unwrap().asked == 0, "every leg; what it asks is set against what it owes");
    // The hedge and the other leg: the hedge loses nothing at any price; due with coins under 5 + 2.025.
    let o = due_edge(&w, "a hedge and a leg of another market", &h, 0, NOTE_MIN + k(500) + k(202) + k(1) / 2 - 1, 3, &a);
    assert!(o.after.is_some_and(|x| x.legs.len() == 2), "the leg that lost leaves alone");
    // The same three with the other market's leg between the two (three groups): each of the two is stated its
    // own worst price, the long 4.5 % below and the short 4.5 % above, which no move of one price is. Due with
    // coins under 5 + 2.025 + 2 * 4.95: 9.9 KAS higher than as a hedge.
    let apart = [long, other, short];
    let o = due_edge(&w, "a long and a short of one market apart", &apart, 0, NOTE_MIN + k(500) + k(202) + k(1) / 2 + 2 * k(495) - 1, 2, &[p11, p9, p11]);
    assert!(o.after.is_some());
    // (An account of those three at the level of a take holds 2 * 6.05 + 2.475 + 15: 12.65 KAS above that.)
    let q = cov(&w, &apart, 100 * KAS1).s.c_step_prices(&[p11, p9, p11]).unwrap();
    assert_eq!((q[0], q[2]), (p11 - up_of(p11, 450), p11 + up_of(p11, 450)), "two prices of one market in one step");
    n += 4 * 2;

    // 9, the leg named. A long that lost 5 beside a long of another market that gained 3: with coins of 5 it
    // leaves alone, for no fee; with a sompi less the coins do not pay it and every leg leaves.
    let two = [long, fleg(&w, 1, 5, LONG, P10)];
    let a2 = [P10 * 95 / 100, P10 * 106 / 100];
    let (_, o, e) = ruled(&w, &cov(&w, &two, NOTE_MIN + k(500)), KIND_C_STEP, 0, 1, &a2, &w.f).unwrap();
    assert_eq!((e, o.after.map(|x| (x.legs.len(), x.a_margin)), o.fee), ((true, false), Some((1, NOTE_MIN)), 0));
    let (s, o, e) = ruled(&w, &cov(&w, &two, NOTE_MIN + k(500) - 1), KIND_C_STEP, 0, 1, &a2, &w.f).unwrap();
    assert_eq!((e, o.after.is_none(), o.fee, o.note.unwrap().asked), ((true, false), true, k(74), -k(200)));
    // (That statement, of every leg and its fee, on the account a sompi richer: wrong.)
    assert_eq!(handed(&w, &cov(&w, &two, NOTE_MIN + k(500)), KIND_C_STEP, &s, &w.f), (Some(false), (false, true)));
    // E1's account: a long that gained 3 beside a short that lost 8, coins 5. Named, the long: wrong. Named, the
    // short: every leg, and the shard is paid the 5 the two lost together.
    let e1 = [fleg(&w, 0, 10, LONG, P10), fleg(&w, 1, 10, SHORT, P10)];
    let ae = [P10 * 103 / 100, P10 * 108 / 100];
    let x = cov(&w, &e1, NOTE_MIN + k(500));
    assert!(x.s.c_state(KIND_C_STEP, 0, 1, &ae, T0, x.value as i64, xk(&w.f)).is_err());
    let q = x.s.c_step_prices(&ae).unwrap();
    for fee in [0, k(51), k(52)] {
        let s = CSaid { amount: fee, skip: 1, lc: chain_of_one(&e1[0]), head: x.s.c_head(), rows: x.s.c_trace_step(&ae, &q, 1).unwrap() };
        assert_eq!(handed(&w, &x, KIND_C_STEP, &s, &w.f), (Some(false), (false, true)), "E1: the leg that gained named while another lost");
    }
    let (_, o, e) = ruled(&w, &x, KIND_C_STEP, 0, 2, &ae, &w.f).unwrap();
    assert_eq!((e, o.after.is_none(), o.note.unwrap().asked, o.fee), ((true, false), true, -k(500), 0));
    n += 3 + 3 + 1;

    // The head: each of its three numbers one off, with the rows that head makes: the end is wrong. The lines are
    // held by nothing (stage 1 reads none): the end is right whatever they say.
    let acct = cov(&w, &h, 100 * KAS1);
    let (st, s, _) = acct.s.c_state(KIND_C_TAKE, KAS1, 0, &a, T0, acct.value as i64, xk(&w.w)).unwrap();
    let with = |head: CHead| CSaid { head, rows: kpf_protocol::crow::ctrace(KIND_C_TAKE, &head, &h, &a, &a, |_| Part::Stays).unwrap(), ..s.clone() };
    for d in [1i64, -1] {
        for (what, head) in [("rate", CHead { rate: s.head.rate + d, ..s.head }), ("tolerance", CHead { tolerance: s.head.tolerance + d, ..s.head }), ("T", CHead { t: s.head.t + d, ..s.head })] {
            let t = with(head);
            let st2 = Stated { arg: t.arg(), ..st };
            assert_eq!((acct.s.c_end_judged(KIND_C_TAKE, &t, &st2.claimer, &st2.outs, st2.held, T0), end_taken(&w, &acct, &w.stated(&acct, &st2, &t.said()), &t)), (Some(false), (false, true)), "a head whose {what} is off by {d}");
            n += 1;
        }
    }
    for lines in [1i64, -1, i64::MAX] {
        let t = with(CHead { lines, ..s.head });
        let st2 = Stated { arg: t.arg(), ..st };
        assert_eq!((acct.s.c_end_judged(KIND_C_TAKE, &t, &st2.claimer, &st2.outs, st2.held, T0), end_taken(&w, &acct, &w.stated(&acct, &st2, &t.said()), &t)), (Some(true), (true, false)), "the lines are held by nothing");
        n += 1;
    }
    println!("T13 the ends at their edges: {n} ends at the engine, each as the rule written out says, a sompi either side");
}

/// 10, an entry, on the model (`c_enter`, `c_enter_end`; the engine against the model is review 47's walk). A long
/// of 10 from 12, at 11 (it lost 10). A short of 4 of its market enters at 11: the two are asked the rate on the 6
/// between them (3.63), and the coins and what all gained stand at that. The leg before is stated at the entry's
/// price, or there is no trace. The place said is the count of the legs, or the end is wrong.
#[test]
fn t13_held_an_entry_at_its_edge_on_the_model() {
    use kpf_protocol::covered::CEnterSaid;
    let w = world();
    let p11 = P10 * 11 / 10;
    let (long, short) = (fleg(&w, 0, 10, LONG, P10 * 12 / 10), fleg(&w, 0, 4, SHORT, p11));
    let need = NOTE_MIN + 10 * KAS1 + 363 * KAS1 / 100;
    let x = cov(&w, &[long], need).s;
    let (st, s, marked, after) = x.c_enter(short, 0, 0, &[p11], T0, need).expect("an entry at the edge");
    assert_eq!((after.legs.len(), s.place, s.rows[1].closed().unwrap().0, s.rows[1].gain), (2, 2, 363 * KAS1 / 100, -10 * KAS1));
    assert!(cov(&w, &[long], need - 1).s.c_enter(short, 0, 0, &[p11], T0, need - 1).is_err(), "a sompi under");
    assert!(x.c_enter(short, 0, 0, &[p11 + 1], T0, need).is_err(), "the leg before at another price than the entry's");
    for place in [1, 3, 0, -1] {
        let t = CEnterSaid { place, ..s.clone() };
        assert_eq!(Account::c_enter_end(&marked, &t, &st.claimer, st.held, T0), Some(None), "the place {place}");
    }
    // (A short of another market in its place: the two are asked apart, 6.05 and 2.42.)
    let _ = &after;
    let apart = fleg(&w, 1, 4, SHORT, p11);
    let need2 = NOTE_MIN + 10 * KAS1 + 605 * KAS1 / 100 + 242 * KAS1 / 100;
    assert!(cov(&w, &[long], need2).s.c_enter(apart, 0, 0, &[p11], T0, need2).is_ok() && cov(&w, &[long], need2 - 1).s.c_enter(apart, 0, 0, &[p11], T0, need2 - 1).is_err());
}

/// 10 at the engine: the rows of an entry (`KpfTCRow` reads the head and the rows at other places, and names no leg
/// that leaves). A long of another market and a long of 10 of market 0; a short of 6 of market 0 enters at 11, next
/// to it. Each row as the model writes it is answered right; each number of each row a sompi off, wrong. The last
/// row: of another leg than the one said, at another price than its entry (its group at that price), and the leg
/// before at another price than the entry's (two prices in a group): wrong.
#[test]
fn t13_held_the_rows_of_an_entry_a_sompi_off_and_its_last_row() {
    use kpf_protocol::covered::{c_enter_row_judged, CEnterSaid, KIND_C_ENTER};
    let w = world();
    let p11 = P10 * 11 / 10;
    let x = cov(&w, &[fleg(&w, 1, 5, LONG, P10), fleg(&w, 0, 10, LONG, P10)], 100 * KAS1).s;
    let (st, s, marked, _) = x.c_enter(fleg(&w, 0, 6, SHORT, p11), 0, 0, &[P10 * 9 / 10, p11], T0, 100 * KAS1).expect("an entry");
    let all = marked.0.legs.clone();
    assert_eq!((s.rows.len(), s.rows[2].closed().unwrap().0), (3, 5 * up_of(P10 * 9 / 10, 550) + 4 * up_of(p11, 550)));
    let both = |t: &CEnterSaid, i: usize| -> (Option<bool>, (bool, bool)) {
        let before = if i == 1 { CRow::ZERO } else { t.rows[i - 2] };
        let lb = i.checked_sub(2).map(|k| &all[k]);
        let model = c_enter_row_judged(&before.g, lb, &all[i - 1], &before, &t.rows[i - 1], i as i64, t);
        let st2 = Stated { arg: t.arg(), ..st };
        // (The statement as an entry leaves it on the chain: what the marked account held, and its bond.)
        let live = OLive { s: st2, value: (st2.held + st2.bond) as u64, id: Some(Hash::from_bytes([0x73; 32])), op: op(81), daa: 6_510 };
        let q = w.pointed(&live, 2 * i as i64 - 1);
        let take = |v: bool| {
            let f = [fund(xk(&w.w), 36)];
            c_enter_row(&w.lib, &q, t, 2 * i as i64 - 1, &before.g, lb, &all[i - 1], &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.w, &f), Some(v)).is_ok_and(|b| accepted(&b.0, &w.keys))
        };
        (model, (take(true), take(false)))
    };
    let mut n = 0;
    for i in 1..=3 {
        assert_eq!(both(&s, i), (Some(true), (true, false)), "row {i} of the entry");
        for k in 0..21 {
            for d in [1i64, -1] {
                let mut t = s.clone();
                *number(&mut t.rows[i - 1], k) += d;
                assert_eq!(both(&t, i), (Some(false), (false, true)), "row {i} of the entry, {} by {d}", NAMES[k]);
                n += 1;
            }
        }
    }
    // The last row of another leg than the one said.
    let other = CEnterSaid { leg: XLeg { size: s.leg.size + 1, ..s.leg }, ..s.clone() };
    assert_eq!(both(&other, 3), (Some(false), (false, true)), "the leg said is another");
    // Its group a sompi above the entry: row 2 is right as a row, the last is not at its entry.
    let at = |a2: i64, a3: i64| {
        let r2 = crow_after(KIND_C_ENTER, &s.head, &s.rows[0], Some(&all[0]), &all[1], a2, a2, Part::Stays).unwrap().unwrap();
        // (As if the last leg began a group, where the two prices differ: the row the arithmetic gives.)
        let r3 = crow_after(KIND_C_ENTER, &s.head, &r2, if a2 == a3 { Some(&all[1]) } else { None }, &all[2], a3, a3, Part::Stays).unwrap().unwrap();
        CEnterSaid { rows: vec![s.rows[0], r2, r3], ..s.clone() }
    };
    let above = at(p11 + 1, p11 + 1);
    assert_eq!((both(&above, 2), both(&above, 3)), ((Some(true), (true, false)), (Some(false), (false, true))), "the last row at another price than its entry");
    let two = at(p11 + 1, p11);
    assert_eq!((both(&two, 2), both(&two, 3)), ((Some(true), (true, false)), (Some(false), (false, true))), "the leg before at another price than the entry's");
    println!("T13 the rows of an entry: 3 rows right; {n} with a number a sompi off and 3 of the last row's own, wrong, at the engine");
}

/// Item 5, of review 47's walk of "what a shard is good for" (`auditT1_builders`): its take of "the most the rule
/// gives" is still `m - a note's least - (the rate on W + LS)`, the most of the rule before. On accounts as that
/// walk makes them (one to six legs of four markets, a price a market): how often that amount is the most the rule
/// now gives, how often it is refused (R is above the rate on W by the rounding of the ends), and how often the
/// rule gives more (legs counted together).
#[test]
fn t13_w8_closed_the_most_a_take_gives_is_by_what_is_asked_and_not_by_the_rate_on_the_worth() {
    let w = world();
    let mut r = Rng(0x13_0008);
    let (mut same, mut refused, mut more, mut most_more) = (0, 0, 0, 0i64);
    for _ in 0..5_000 {
        let n = r.range(1, 7) as usize;
        let legs: Vec<XLeg> = (0..n).map(|_| fleg_c(&w, r.range(0, 4) as usize, r.range(50, 3_000), if r.range(0, 2) == 0 { LONG } else { SHORT }, P10 * r.range(80, 121) / 100)).collect();
        let p: Vec<i64> = (0..4).map(|_| P10 / 10_000 * r.range(8_800, 11_200)).collect();
        let a: Vec<i64> = legs.iter().map(|l| p[w.markets.iter().position(|m| m.id() == l.market).unwrap()]).collect();
        let x = cov(&w, &legs, 0).s;
        let last = *x.c_trace(&a, 0).unwrap().last().unwrap();
        let old = up_of(last.w, 550) + last.ls;
        let new = last.closed().unwrap().0 + last.ls;
        let coins = NOTE_MIN + old.max(new) + 10 * KAS1;
        let x = Account { a_margin: coins, ..x };
        let old_most = coins - NOTE_MIN - old;
        let is = x.c_state(KIND_C_TAKE, old_most, 0, &a, T0, coins, xk(&w.w)).is_ok();
        assert_eq!(is, old >= new);
        if !is {
            refused += 1;
        } else if old == new {
            same += 1;
        } else {
            more += 1;
            most_more = most_more.max(old - new);
        }
    }
    println!("T13 W8: of 5,000 accounts as review 47's walk makes them, the take it calls the most the rule gives is the most on {same}, is refused on {refused} (R above the rate on W by the rounding), and is below the most on {more} (by up to {most_more} sompi)");
    // (W8 is closed where review 47's walk takes the most by R: `auditT1_builders.rs`. This counts how far the two part.)
    assert!(more > 1_000 && same + more + refused == 5_000, "the accounts changed");
}

// ================================================================================================================
// 4. The rule's own money: walks of accounts on the model the scripts are held to (`covered.rs`).

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo) as u64) as i64
    }
}

/// The price stated for each leg: its market's, each group (legs of one market next to one another) bent by the
/// basis points `bend` gives its number.
fn stated_prices(w: &World, legs: &[XLeg], p: &[i64; 3], bend: &dyn Fn(usize) -> i64) -> Vec<i64> {
    let mut group = 0;
    legs.iter()
        .enumerate()
        .map(|(i, l)| {
            if i > 0 && legs[i - 1].market != l.market {
                group += 1;
            }
            let m = w.markets.iter().position(|x| x.id() == l.market).unwrap();
            p[m] / 10_000 * (10_000 + bend(group))
        })
        .collect()
}

/// Whether a step is due on the account at the prices `a`, by the end's test written out (the prices that show it).
fn due_at(x: &Account, a: &[i64]) -> bool {
    if x.legs.is_empty() {
        return false;
    }
    let q = x.c_step_prices(a).unwrap();
    let last = *x.c_trace_step(a, &q, 1).unwrap().last().unwrap();
    let mut sx = last.gain + last.m;
    if sx >= 0 {
        sx = (sx - up_of(last.w, 2 * x.t.g_bps)).max(0);
    }
    x.a_margin - NOTE_MIN + last.tk - x.b_margin + sx < 0
}

#[derive(Default, Debug)]
struct Walked {
    accounts: usize,
    takes: usize,
    leavings: usize,
    marks: usize,
    entries: usize,
    steps_alone: usize,
    steps_all: usize,
    ended: usize,
    /// Visits at which a step was due and none was right.
    stuck: usize,
    /// Statements of the user's after which a step was due at the same prices.
    due_after_own: usize,
    /// Where every leg left: what the legs lost together beyond what the shard was paid.
    unpaid: i64,
    unpaid_on: usize,
    unpaid_most: i64,
    /// What the legs lost at the markets' prices beyond what the shard was paid. (A loss is settled at the price
    /// stated, within the tolerance: this counts the tolerance itself, and is no measure of the rule.)
    unpaid_true: i64,
    unpaid_true_on: usize,
    /// Visits at which both a step of one leg and a step of every leg were right, and the sum of the fees each way.
    choice: usize,
    fee_alone: i64,
    fee_all: i64,
    /// What a step of every leg kept of what the legs had gained together (the band).
    band_kept: i64,
}

/// `tolerance`: 0, every statement at the markets' prices; 1, the user states each group's price the tolerance
/// either way as serves it, and whoever steps states each group's price either way as shows a step due.
/// `policy`: which right step whoever watches takes: 0 any, 1 the one that pays it most.
/// `moves`: the most a market's price moves between two visits, in basis points.
fn walk(w: &World, seed: u64, accounts: usize, tolerance: i64, policy: i64, moves: i64) -> Walked {
    let mut r = Rng(seed);
    let mut out = Walked::default();
    let g = w.terms().g_bps;
    let bends: Vec<Box<dyn Fn(usize) -> i64>> = if tolerance == 0 {
        vec![Box::new(|_| 0)]
    } else {
        vec![Box::new(|_| 0), Box::new(move |_| g), Box::new(move |_| -g), Box::new(move |k| if k % 2 == 0 { g } else { -g }), Box::new(move |k| if k % 2 == 0 { -g } else { g })]
    };
    for _ in 0..accounts {
        let mut p = [P10 * r.range(80, 121) / 100, P10 * r.range(80, 121) / 100, P10 * r.range(80, 121) / 100];
        let n = r.range(1, 5) as usize;
        let leg = |r: &mut Rng, p: &[i64; 3], entry_pct: i64| {
            let m = if r.range(0, 5) == 0 { 2 } else { r.range(0, 2) as usize };
            fleg_c(w, m, r.range(50, 2_000), if r.range(0, 2) == 0 { LONG } else { SHORT }, p[m] / 100 * entry_pct)
        };
        let legs: Vec<XLeg> = (0..n)
            .map(|_| {
                let pct = r.range(92, 109);
                leg(&mut r, &p, pct)
            })
            .collect();
        // (At the level of a take and a little more, at the markets' prices.)
        let a0 = stated_prices(w, &legs, &p, &|_| 0);
        let mut x = cov(w, &legs, NOTE_MIN).s;
        let last = *x.c_trace(&a0, 0).unwrap().last().unwrap();
        x.a_margin = NOTE_MIN + last.closed().unwrap().0 + last.ls + last.w / 1_000 * r.range(0, 30);
        out.accounts += 1;
        let mut t = T0;
        'life: for _ in 0..10 {
            // Whoever watches: every right step, until none is.
            loop {
                let mut right = vec![];
                for bend in &bends {
                    let a = stated_prices(w, &x.legs, &p, bend.as_ref());
                    for skip in 1..=x.legs.len() as i64 {
                        if let Ok((_, s, o)) = x.c_state(KIND_C_STEP, 0, skip, &a, t, x.a_margin, xk(&w.f)) {
                            right.push((s, o));
                        }
                    }
                }
                if right.is_empty() {
                    if due_at(&x, &stated_prices(w, &x.legs, &p, &|_| 0)) {
                        out.stuck += 1;
                    }
                    break;
                }
                let (alone, all): (Vec<_>, Vec<_>) = right.iter().partition(|(_, o)| o.after.is_some());
                if !alone.is_empty() && !all.is_empty() {
                    out.choice += 1;
                    out.fee_alone += alone.iter().map(|(_, o)| o.fee).max().unwrap();
                    out.fee_all += all.iter().map(|(_, o)| o.fee).max().unwrap();
                }
                let k = if policy == 1 { (0..right.len()).max_by_key(|k| right[*k].1.fee).unwrap() } else { r.range(0, right.len() as i64) as usize };
                let (s, o) = right.swap_remove(k);
                match o.after {
                    Some(y) => {
                        out.steps_alone += 1;
                        x = y;
                    }
                    None => {
                        out.steps_all += 1;
                        out.ended += 1;
                        let gain = s.rows.last().unwrap().gain;
                        let asked = o.note.unwrap().asked;
                        let true_gain = x.c_trace(&stated_prices(w, &x.legs, &p, &|_| 0), 0).unwrap().last().unwrap().gain;
                        if true_gain < 0 && -true_gain > (-asked).max(0) {
                            out.unpaid_true += -true_gain - (-asked).max(0);
                            out.unpaid_true_on += 1;
                        }
                        if gain < 0 && -gain > -asked {
                            out.unpaid += -gain + asked;
                            out.unpaid_on += 1;
                            out.unpaid_most = out.unpaid_most.max(-gain + asked);
                        }
                        if gain > 0 {
                            out.band_kept += gain - asked;
                        }
                        break 'life;
                    }
                }
            }
            // The user: a statement of its own, at prices that serve it.
            t += 1_000;
            let act = r.range(0, 6);
            let mut done = false;
            for bend in &bends {
                if done || x.legs.is_empty() {
                    break;
                }
                let a = stated_prices(w, &x.legs, &p, bend.as_ref());
                let n = x.legs.len() as i64;
                let after = match act {
                    0 | 1 => {
                        // The most a take gives: the best of the prices that may be stated.
                        let most = bends
                            .iter()
                            .map(|b| {
                                let a = stated_prices(w, &x.legs, &p, b.as_ref());
                                let last = *x.c_trace(&a, 0).unwrap().last().unwrap();
                                (x.a_margin - NOTE_MIN - last.closed().unwrap().0 - last.ls, a)
                            })
                            .max_by_key(|v| v.0)
                            .unwrap();
                        done = true;
                        if most.0 > 0 {
                            let (_, _, o) = x.c_state(KIND_C_TAKE, most.0, 0, &most.1, t, x.a_margin, xk(&w.w)).expect("the most a take gives is the rule's");
                            assert!(x.c_state(KIND_C_TAKE, most.0 + 1, 0, &most.1, t, x.a_margin, xk(&w.w)).is_err());
                            // (What the shard is good for does not fall: the coins that stay pay what the legs
                            // lost together at the prices stated.)
                            let gain = x.c_trace(&most.1, 0).unwrap().last().unwrap().gain;
                            assert!(x.a_margin - NOTE_MIN - most.0 + gain >= 0, "the most a take gives leaves less than the legs lost");
                            out.takes += 1;
                            o.after
                        } else {
                            None
                        }
                    }
                    2 => x.c_state(KIND_C_LEAVE, 0, if r.range(0, 4) == 0 { ALL } else { r.range(1, n + 1) }, &a, t, x.a_margin, xk(&w.w)).ok().map(|(_, s, o)| {
                        out.leavings += 1;
                        if o.after.is_none() {
                            let gain = s.rows.last().unwrap().gain;
                            let asked = o.note.as_ref().unwrap().asked;
                            if gain < 0 && -gain > -asked {
                                out.unpaid += -gain + asked;
                                out.unpaid_on += 1;
                                out.unpaid_most = out.unpaid_most.max(-gain + asked);
                            }
                        }
                        o.after.unwrap_or(Account { legs: vec![], a_min_counter: 0, ..x.clone() })
                    }),
                    3 => x.c_state(KIND_C_MARK, 0, r.range(1, n + 1), &a, t, x.a_margin, xk(&w.w)).ok().and_then(|(_, _, o)| {
                        out.marks += 1;
                        o.after
                    }),
                    4 if n < 6 => {
                        // A leg enters at its group's price, with the coins it is asked alone or none (a hedge).
                        let l = leg(&mut r, &p, 100);
                        let m = w.markets.iter().position(|y| y.id() == l.market).unwrap();
                        let entry = x.legs.iter().zip(&a).rev().take_while(|(y, _)| y.market == l.market).map(|(_, v)| *v).next().unwrap_or(p[m] / 10_000 * (10_000 + bend(n as usize)));
                        let l = XLeg { entry, ..l };
                        let more = if r.range(0, 2) == 0 { 0 } else { up_of(l.size / (l.scale / 100) * (entry / 100), 560) };
                        x.c_enter(l, more, 0, &a, t, x.a_margin).ok().map(|v| {
                            out.entries += 1;
                            v.3
                        })
                    }
                    _ => {
                        done = true;
                        None
                    }
                };
                if let Some(y) = after {
                    done = true;
                    let ended = y.legs.is_empty() && act == 2;
                    x = y;
                    if ended {
                        out.ended += 1;
                        break 'life;
                    }
                    if bends.iter().any(|b| due_at(&x, &stated_prices(w, &x.legs, &p, b.as_ref()))) {
                        out.due_after_own += 1;
                    }
                }
            }
            // The prices move: each market's by no more than `moves`, and half the time by all of it.
            for v in p.iter_mut() {
                let most = *v / 10_000 * moves;
                let d = match r.range(0, 4) {
                    0 => most,
                    1 => -most,
                    _ => r.range(-most, most + 1),
                };
                *v += d;
            }
        }
    }
    out
}

/// 4 (a) and (c), in numbers. A hedge of 10 units from 10 with 100 KAS above a note's least.
/// (a) At 10 its user takes all 100. At 15 the short has lost 50 and the account holds a note's least: no step is
/// due at any price that may be stated; no take, no mark, and neither leg leaves by its user's statement; every
/// leg leaves, the shard is asked nothing and paid nothing, and that is what the two legs made together.
/// (c) At 11 the user states every price as serves it, the tolerance off: takes the most (at 10.945 the short
/// lost 9.45), lets the short go (the coins pay 9.45 of a loss of 10 at the market's price), then the long at
/// 11.055 (its gain less the band). What it has taken out in all is below what it put in: the band on the leg that
/// gained is more than the tolerance gave on the leg that lost.
#[test]
fn t13_held_a_hedge_that_took_all_and_a_hedge_stated_the_tolerance_off() {
    let w = world();
    let (long, short) = (fleg(&w, 0, 10, LONG, P10), fleg(&w, 0, 10, SHORT, P10));
    let put_in = NOTE_MIN + 100 * KAS1;
    let x = cov(&w, &[long, short], put_in);
    // (a)
    let (_, o, e) = ruled(&w, &x, KIND_C_TAKE, 100 * KAS1, 0, &[P10; 2], &w.w).expect("all the coins but a note's least");
    assert_eq!(e, (true, false));
    let bare = o.after.unwrap();
    let t = bare.b_min_counter;
    for price in [P10 * 15 / 10, P10 * 15 / 10 / 1_000 * 1_005, P10 * 15 / 10 / 1_000 * 995, P10 / 2] {
        let a = [price; 2];
        assert!((1..=2).all(|k| bare.c_state(KIND_C_STEP, 0, k, &a, t, NOTE_MIN, xk(&w.f)).is_err()), "a hedge is due at {price}");
        assert!(bare.c_state(KIND_C_TAKE, 1, 0, &a, t, NOTE_MIN, xk(&w.w)).is_err());
        assert!((1..=2).all(|k| bare.c_state(KIND_C_LEAVE, 0, k, &a, t, NOTE_MIN, xk(&w.w)).is_err() && bare.c_state(KIND_C_MARK, 0, k, &a, t, NOTE_MIN, xk(&w.w)).is_err() && bare.c_state(KIND_C_SPLIT, NOTE_MIN, k, &a, t, NOTE_MIN, xk(&w.w)).is_err()));
        let (_, _, o) = bare.c_state(KIND_C_LEAVE, 0, ALL, &a, t, NOTE_MIN, xk(&w.w)).expect("every leg leaves");
        assert_eq!((o.note.unwrap().asked, o.paid), (0, NOTE_MIN));
    }
    // (c)
    let (lo, hi) = (P10 * 11 / 10 / 1_000 * 995, P10 * 11 / 10 / 1_000 * 1_005);
    let most = 100 * KAS1 - 10 * (lo - P10);
    assert!(x.s.c_state(KIND_C_TAKE, most + 1, 0, &[lo; 2], T0, put_in, xk(&w.w)).is_err());
    let (_, o, e) = ruled(&w, &x, KIND_C_TAKE, most, 0, &[lo; 2], &w.w).expect("the most a take gives at the lowest price");
    assert_eq!(e, (true, false));
    let y = o.after.unwrap();
    let t = y.b_min_counter;
    // (No step at any price that may be stated.)
    assert!([lo, P10 * 11 / 10, hi].iter().all(|p| (1..=2).all(|k| y.c_state(KIND_C_STEP, 0, k, &[*p; 2], t, y.a_margin, xk(&w.f)).is_err())));
    let (_, _, o) = y.c_state(KIND_C_LEAVE, 0, 2, &[lo; 2], t, y.a_margin, xk(&w.w)).expect("the short leaves at the lowest price");
    let paid_shard = -o.note.unwrap().asked;
    let z = o.after.unwrap();
    assert_eq!((paid_shard, z.a_margin), (10 * (lo - P10), NOTE_MIN));
    let t = z.b_min_counter;
    let (_, _, o) = z.c_state(KIND_C_LEAVE, 0, 1, &[hi], t, z.a_margin, xk(&w.w)).expect("the long leaves at the highest price");
    let asked = o.note.unwrap().asked;
    assert_eq!(asked, 10 * (hi - P10) - up_of(10 * hi, 100));
    let out = most + asked + o.after.unwrap().a_margin;
    assert!(out < put_in, "the user took out {out} of {put_in}");
    println!("T13 a hedge stated the tolerance off: of {put_in} put in, {out} taken out (the shard was paid {paid_shard} and asked {asked})");
}

/// Accounts of one to four futures legs of three markets (mostly two: groups form), from the level of a take, over
/// ten visits: whoever watches takes every right step; the user takes the most a take gives, leaves with a leg or
/// with all, marks a leg, enters a leg (often into a group); then each market's price moves by no more than the
/// lower rate (the move that the end of a step counts to), half the time by all of it.
/// Every statement at the markets' prices: no account is due after its user's own statement; a due account always
/// takes a step; **the shard is unpaid nothing**, whichever step is taken.
#[test]
fn t13_held_on_the_model_the_shard_is_unpaid_nothing_at_the_markets_prices() {
    let w = world();
    for policy in [0, 1] {
        let o = walk(&w, 0x13_0001 + policy as u64, 6_000, 0, policy, 450);
        println!("T13 walk at the markets' prices, whoever watches taking {}: {o:?}", if policy == 1 { "the step that pays most" } else { "any right step" });
        assert!(o.takes > 1_000 && o.leavings > 300 && o.marks > 100 && o.entries > 300 && o.steps_alone > 300 && o.steps_all > 100, "{o:?}");
        assert_eq!((o.stuck, o.due_after_own, o.unpaid), (0, 0, 0), "{o:?}");
    }
}

/// The same with every price stated the tolerance off as serves whoever states it (a group a price: two groups of
/// one market that are apart, two prices). What is measured: W4 (how much the shard is then unpaid, and how
/// often an account is due after its user's own statement).
#[test]
fn t13_w4_on_the_model_with_prices_stated_the_tolerance_off() {
    let w = world();
    for (moves, policy) in [(450, 0), (450, 1), (350, 0)] {
        let o = walk(&w, 0x13_0002 + moves as u64 + policy as u64, 4_000, 1, policy, moves);
        println!("T13 walk with prices stated the tolerance off, moves of {moves} basis points at the most, whoever watches taking {}: {o:?}", if policy == 1 { "the step that pays most" } else { "any right step" });
        assert_eq!(o.stuck, 0, "{o:?}");
    }
}
