//! Review T15: the option leg in a covered account (docs/ARCHITECTURE-CLEARING.md 17.8i) and the step by what is
//! asked, at the engine. The reviewer's tests; its report is docs/audit/REVIEW-T15-options.md.
//!
//! A test named `t15_yN_...` shows the finding YN of the report as it stands (it passes while the finding is open,
//! and says so in its assertion's message). A test named `t15_held_...` is of what was attacked and held.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::covered::{c_row_judged_at, chain_of_one, leg_more, CSaid, COutcome, ALL, DIR_SPOT, KIND_C_LEAVE, KIND_C_MARK, KIND_C_SPLIT, KIND_C_STEP, KIND_C_TAKE, KIND_C_TOK_OUT, KIND_C_TOK_SOLD};
use kpf_protocol::cross::{XLeg, XTerms};
use kpf_protocol::crow::{crow_after, ends, option_of, CHead, CRow, Part};
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
/// A day far from the statement's time (T is 300).
const FAR: i64 = T0 + 10_000;
/// The four ways of an option leg.
const CALL_HELD: i64 = 3;
const CALL_WRITTEN: i64 = 4;
const PUT_HELD: i64 = 5;
const PUT_WRITTEN: i64 = 6;

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

/// The world of `auditT13_rows.rs`: the user `w`, the clearing party `c`, a third key `f`, four markets, the library.
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
    /// A statement as it stands stated and disputed at `point`, the two transactions not run (the walks: what the
    /// script of a statement and of a dispute take is not what is attacked there; `t15_held_the_short_way...`
    /// holds this to the long way).
    fn pointed_fast(&self, acct: &OLive<Account>, st: &Stated, point: i64) -> OLive<Pointed> {
        let live = OLive { s: *st, value: (st.held + st.bond) as u64, id: acct.id, op: op(81), daa: 6_510 };
        OLive { s: Pointed { st: *st, disputer: xk(&self.w), point }, value: live.value + st.bond as u64, id: acct.id, op: op(82), daa: 6_520 }
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

/// A futures leg of market `i`: `cents` hundredths of a unit. It says the time it entered (6,000).
fn fleg_c(w: &World, i: usize, cents: i64, dir: i64, entry: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: leg_more(6_000, 0, 0, 0), scale: m.p.scale, price_max: m.p.price_max, size: cents * (m.p.scale / 100), dir, entry }
}
fn fleg(w: &World, i: usize, units: i64, dir: i64, entry: i64) -> XLeg {
    fleg_c(w, i, 100 * units, dir, entry)
}
/// A token leg: `cents` hundredths of a unit of a token priced by market `i`, at a discount of 20 %.
fn tleg_c(w: &World, i: usize, cents: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: [0x99; 32], scale: m.p.scale, price_max: m.p.price_max, size: cents * (m.p.scale / 100), dir: DIR_SPOT, entry: 2_000 }
}
/// An option leg of market `i` (docs 17.2, 17.8i): `way` 3 a call held, 4 a call written, 5 a put held, 6 a put
/// written; `cents` hundredths of a unit; its 32 bytes say the time it entered (6,000), its day, its strike and its
/// premium a unit.
fn oleg_c(w: &World, i: usize, way: i64, cents: i64, strike: i64, entry: i64, day: i64, premium: i64) -> XLeg {
    XLeg { dir: way, committee: leg_more(6_000, day, strike, premium), ..fleg_c(w, i, cents, LONG, entry) }
}
fn oleg(w: &World, i: usize, way: i64, units: i64, strike: i64, entry: i64, day: i64, premium: i64) -> XLeg {
    oleg_c(w, i, way, 100 * units, strike, entry, day, premium)
}

/// A covered account of the user's (`w`) under a shard, as an output stands on the chain: its coins, its legs.
fn cov(w: &World, legs: &[XLeg], coins: i64) -> OLive<Account> {
    cov_owing(w, legs, coins, 0)
}
fn cov_owing(w: &World, legs: &[XLeg], coins: i64, owes: i64) -> OLive<Account> {
    let s = Account { t: w.terms(), legs: legs.to_vec(), a_key: xk(&w.w), b_key: xk(&w.c), a_margin: coins, b_margin: owes, expiry: 0, dispute_daa: 50, bond_bps: 1_000, a_min_counter: legs.len() as i64, b_min_counter: 0, shard: Some(SID) };
    OLive { s, value: coins as u64, id: Some(Hash::from_bytes([0x73; 32])), op: op(80), daa: 6_100 }
}
/// The account as a statement's outcome leaves it, as an output.
fn live_of(w: &World, s: &Account) -> OLive<Account> {
    OLive { s: s.clone(), value: s.a_margin as u64, ..cov(w, &[], 0) }
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
fn ruled(w: &World, acct: &OLive<Account>, kind: i64, amount: i64, skip: i64, a: &[i64], claimer: &Keypair) -> Option<(CSaid, COutcome, (bool, bool))> {
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

fn claimer_of<'a>(w: &'a World, kind: i64) -> &'a Keypair {
    if kind == KIND_C_STEP || kind == KIND_C_TOK_SOLD {
        &w.f
    } else {
        &w.w
    }
}

fn up_of(v: i64, bps: i64) -> i64 {
    (v / 10_000) * bps + ((v % 10_000) * bps + 9_999) / 10_000
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
        lo + (self.next() % (hi - lo) as u64) as i64
    }
    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[self.range(0, v.len() as i64) as usize]
    }
}

// ================================================================================================================
// Y1. Tokens stand for a premium in a take; no step asks them for it.

/// Y1 (high). A take (and a split, a mark, tokens leaving, an entry) counts what the token legs count as coins
/// against R **and the premiums**: `m - amount + tk >= RA' + pa + ls`. A step pays a premium "as far as the coins
/// go", and what they do not pay is not owed (17.3); an account that holds tokens is not taken whole, and owes
/// what its coins do not pay of a **loss** only. So beside a token leg the coins that stand for a premium leave by
/// a take, and the premium is then paid by nobody.
///
/// (a) A call written 20 % out of the money for a premium of 2 KAS that the owner put into the account (17.3:
/// "no take reaches them while the leg stands"), and a quarter of a token (counted 2 KAS and a sompi). The user
/// takes the 2 KAS. Then a step is stated with the token's price the tolerance low (the account is due by a
/// sompi's worth: nothing moved), naming the written call: it gained nothing and lost nothing, the coins hold
/// nothing, it leaves alone. The shard is paid nothing of "the larger of the premium and what it lost"; the user
/// has the owner's 2 KAS, its tokens, and no option.
/// (b) The same with a call the user holds for a premium of 5 KAS: the option was held for nothing.
#[test]
fn t15_y1_beside_a_token_leg_a_take_reaches_the_premium_and_a_step_then_asks_nobody_for_it() {
    let w = world();
    let low = P10 / 1_000 * 995;
    let mut said = vec![];
    for (what, option, tok_cents, premium) in [("a call written, the owner's premium", oleg(&w, 0, CALL_WRITTEN, 10, P10 * 12 / 10, P10, FAR, KAS1 / 5), 25, 2 * KAS1), ("a call held, the user's premium", oleg(&w, 0, CALL_HELD, 10, P10 * 12 / 10, P10, FAR, 48 * KAS1 / 100), 60, 48 * KAS1 / 10)] {
        let tok = tleg_c(&w, 1, tok_cents);
        // Without the token leg the premium stays: no take of a sompi.
        let plain = cov(&w, &[option], NOTE_MIN + premium);
        assert!(plain.s.c_state(KIND_C_TAKE, 1, 0, &[P10], T0, plain.value as i64, xk(&w.w)).is_err(), "{what}: with no token leg the premium is held");
        // Closed: beside the token leg the premium is held the same. No take of a sompi by the model, and a take
        // of a sompi written by hand is answered wrong at the engine.
        let acct = cov(&w, &[option, tok], NOTE_MIN + premium);
        acct.s.c_check().expect("a covered account of the form");
        assert!(ruled(&w, &acct, KIND_C_TAKE, 1, 0, &[P10, P10], &w.w).is_none(), "Y1 (closed): {what}: no take of a sompi beside a token leg");
        let head = acct.s.c_head_least(KIND_C_TAKE, T0, &[P10, P10]);
        let rows = acct.s.c_trace_at(KIND_C_TAKE, T0, &head, &[P10, P10], &[P10, P10], |_, _| Part::Stays).unwrap();
        let last = rows[1];
        assert!(last.pa == premium && last.closed().unwrap().0 == 0 && last.tk >= premium, "{what}: the tokens count {} for a premium of {premium}", last.tk);
        let by_hand = CSaid { amount: 1, skip: 0, lc: [0; 32], head, rows };
        assert_eq!(handed(&w, &acct, KIND_C_TAKE, &by_hand, &w.w), (Some(false), (false, true)), "Y1 (closed): {what}: a take of a sompi at the engine");
        // An account that holds a note's least beside the two (its coins paid a loss, say). A step, the token
        // stated the tolerance low: due. The option named leaves alone, and the account owes its premium.
        let next = cov(&w, &[option, tok], NOTE_MIN);
        assert!(next.s.c_state(KIND_C_STEP, 0, 1, &[P10, P10], T0, next.value as i64, xk(&w.f)).is_err(), "{what}: not due at the market's price");
        let (s9, o9, e9) = ruled(&w, &next, KIND_C_STEP, 0, 1, &[P10, low], &w.f).expect("due with the token stated the tolerance low");
        let end = o9.after.clone().unwrap();
        let note = o9.note.clone().unwrap();
        assert_eq!(e9, (true, false), "{what}: the end of the step at the engine");
        assert_eq!((end.legs.clone(), end.a_margin, end.b_margin, note.asked, s9.amount, s9.rows[1].po), (vec![tok], NOTE_MIN, premium, 0, 0, premium), "Y1 (closed): {what}: the premium the coins do not pay is owed");
        // And the tokens do not leave while it is owed.
        assert!(end.c_state(KIND_C_TOK_OUT, 0, 1, &[P10], end.b_min_counter.max(T0), end.a_margin, xk(&w.w)).is_err(), "{what}: the tokens stay while the premium is owed");
        said.push(format!("{what}: no take reaches the premium of {premium}; the option stepped out, the shard paid {} of it and owed {}", -note.asked, end.b_margin));
    }
    println!("T15 Y1 (closed): {said:?}");
}

// ================================================================================================================
// How the tests here state: a step by the prices that show it due (`Account::c_step_prices` reads an option leg as
// a short futures leg: Y6), and with the fee of the rule as it is written out here, from review T14's check list
// and docs 17.8i's "as it is built": not read from the model.

/// The prices a step is due by if it is due by any: for each group, the price within the lower rate of the row's
/// at which its legs gain least: one of the two ends, the row's price, or a strike between (what the legs gain is
/// a line with corners at the strikes; within T of a day, at the row's price as well).
fn least_prices(x: &Account, a: &[i64], t0: i64) -> Vec<i64> {
    let head = x.c_head();
    let mut q = a.to_vec();
    let mut i = 0;
    while i < x.legs.len() {
        let mut j = i;
        while j < x.legs.len() && x.legs[j].market == x.legs[i].market {
            j += 1;
        }
        let dq = up_of(a[i], head.lower());
        let (lo, hi) = ((a[i] - dq).max(0), a[i] + dq);
        let mut cands = vec![lo, hi, a[i]];
        cands.extend(x.legs[i..j].iter().filter_map(option_of).map(|o| o.strike).filter(|k| *k > lo && *k < hi));
        let gains = |p: i64| -> Option<i64> {
            let mut r = CRow::ZERO;
            for (k, l) in x.legs[i..j].iter().enumerate() {
                r = crow_after(KIND_C_STEP, &head, t0, k, &r, if k == 0 { None } else { Some(&x.legs[i + k - 1]) }, l, a[i], p, Part::Stays)??;
            }
            Some(r.m)
        };
        if let Some(best) = cands.iter().filter_map(|p| gains(*p).map(|g| (g, *p))).min() {
            for v in q.iter_mut().take(j).skip(i) {
                *v = best.1;
            }
        }
        i = j;
    }
    q
}

/// A step's end as the rule is written (review T14's list; 17.8i as built), from the last row: `None` where no step
/// of the leg named is right; else (every leg leaves, the fee).
fn step_ruled(x: &Account, row: &CRow) -> Option<(bool, i64)> {
    let (liq, up) = (x.t.liq_fee_bps, x.t.mmr_bps + x.t.g_bps);
    let over = |v: i64| v / up * liq + (v % up) * liq / up;
    let (m, owes, tk) = (x.a_margin - NOTE_MIN, x.b_margin, row.tk);
    let e = m + tk - owes;
    let (ra, rs) = row.closed()?;
    let band = row.band()?;
    let (pa, ps, po, gl, gain) = (row.pa, row.ps, row.po, row.gl, row.gain);
    let pl = pa - ps;
    let mut sx = gain + row.m;
    if sx >= 0 {
        sx = (sx - band).max(0);
    }
    if e + sx >= pa {
        return None;
    }
    let wl = row.w - row.ws;
    let (stay, held_leaves) = (row.ns % 256, row.ns >= 256);
    let base = if wl == 0 { over(row.rl + pl) } else { wl / 10_000 * liq + wl % 10_000 * liq / 10_000 };
    let pol = if wl != 0 {
        0
    } else if held_leaves {
        pl
    } else {
        (pl + gl.min(0)).max(0)
    };
    let nls = (-(gain - gl)).max(0);
    let room = m.max(0);
    let mut all = false;
    let (mut pay, mut prem, mut fee) = (0, 0, 0);
    // (After Y2: not beside tokens.)
    if held_leaves && stay != 0 && tk == 0 {
        all = true;
    } else if gl < 0 {
        pay = -gl;
        prem = pol.min((m - pay).max(0));
        fee = base.min((m - pay - prem - nls - ps).max(0));
        let before = (ra + pa - e - gain).max(0);
        let after = (rs + ps - (e + gain - prem)).max(0);
        if tk != 0 {
            pay = pay.min(room);
        } else if after >= before || m < pay || pol > prem {
            // (After Y3: nor where the coins do not pay all of its premium.)
            all = true;
        }
    } else {
        if row.ls != 0 {
            return None;
        }
        let set = pol.min((gl - row.bl).max(0));
        prem = (pol - set).min(room);
        fee = base.min((m - prem - ps).max(0));
        if tk == 0 {
            let left = m - owes - prem;
            let before = (ra + pa - (m - owes) - gain).max(0);
            let after = (rs + ps - (left + gain - gl)).max(0);
            if after >= before || left < ps || pol - set > prem {
                all = true;
            }
        }
    }
    if all {
        if tk != 0 {
            return None;
        }
        let (pay, x) = if gain < 0 { ((-gain).min(room), 0) } else { (0, (gain - band).max(0)) };
        let set = po.min(x);
        let prem = (po - set).min(room - pay);
        return Some((true, over(ra + pa).min(room - pay - prem)));
    }
    Some((false, fee.min(room - pay - prem)))
}

/// A statement of the rule's on the account holding `held`, at the prices `a` and the time `t0`: with the lines
/// given, or those `c_head_least` takes; a step at the prices that show it due, of the rule's fee as written here.
/// `Err`: (my reading of the rule says a step is right, and the model's end does not take it).
fn stmt(x: &Account, kind: i64, amount: i64, skip: i64, a: &[i64], t0: i64, held: i64, lines: Option<i64>, claimer: [u8; 32]) -> Result<Option<(Stated, CSaid, COutcome)>, String> {
    if a.len() != x.legs.len() {
        return Ok(None);
    }
    let head = match lines {
        Some(lines) => CHead { lines, ..x.c_head() },
        None => x.c_head_least(kind, t0, a),
    };
    let tok = kind == KIND_C_TOK_OUT || kind == KIND_C_TOK_SOLD;
    let amount = if kind == KIND_C_MARK && amount == 0 {
        match usize::try_from(skip - 1).ok().and_then(|k| x.legs.get(k)).and_then(|l| x.mark_entry(l, a[skip as usize - 1])) {
            Some(ne) => ne,
            None => return Ok(None),
        }
    } else {
        amount
    };
    let part = |i: usize, l: &XLeg| {
        if !((skip == ALL || skip == i as i64 + 1) && (l.dir != DIR_SPOT) == !tok) {
            Part::Stays
        } else if kind == KIND_C_MARK {
            Part::Marked(amount)
        } else {
            Part::Leaves
        }
    };
    let q = if kind == KIND_C_STEP { least_prices(x, a, t0) } else { a.to_vec() };
    let Some(rows) = x.c_trace_at(kind, t0, &head, a, &q, part) else { return Ok(None) };
    let lc = if skip >= 1 { x.legs.get(skip as usize - 1).map(chain_of_one).unwrap_or([0; 32]) } else { [0; 32] };
    let mut s = CSaid { amount, skip, lc, head, rows };
    let mut mine = None;
    if kind == KIND_C_STEP {
        mine = s.rows.last().and_then(|r| step_ruled(x, r));
        s.amount = mine.map(|v| v.1).unwrap_or(0);
    }
    let o = x.c_end(kind, &s, &claimer, held, t0).flatten();
    if kind == KIND_C_STEP && (mine.is_some() != o.is_some() || o.as_ref().is_some_and(|o| Some((o.after.is_none(), o.fee)) != mine)) {
        return Err(format!("a step of leg {skip} at {a:?}: the rule as written here says {mine:?}, the model's end {:?}", o.map(|o| (o.after.is_none(), o.fee))));
    }
    Ok(o.map(|o| (Stated { position: x.hash(), window: x.dispute_daa, late: x.t.late_daa, kind, t0, arg: s.arg(), outs: o.outs, bond: x.c_bond_for(kind, s.amount), claimer, held }, s, o)))
}

/// `stmt`, stated; and what the engine takes of its end.
fn ruled2(w: &World, acct: &OLive<Account>, kind: i64, amount: i64, skip: i64, a: &[i64], lines: Option<i64>) -> Option<(CSaid, COutcome, (bool, bool))> {
    let (st, s, o) = stmt(&acct.s, kind, amount, skip, a, T0, acct.value as i64, lines, xk(claimer_of(w, kind))).expect("the rule as written here and the model agree")?;
    let live = w.stated(acct, &st, &s.said());
    let e = end_taken(w, acct, &live, &s);
    Some((s, o, e))
}

/// With coins of `need` the statement is the rule's and its end is answered right; with a sompi less the same
/// statement is not the rule's and its end is answered wrong.
fn edge(w: &World, what: &str, legs: &[XLeg], need: i64, kind: i64, amount: i64, skip: i64, a: &[i64], lines: Option<i64>) -> (CSaid, COutcome) {
    let claimer = claimer_of(w, kind);
    let rich = cov(w, legs, need);
    rich.s.c_check().unwrap_or_else(|e| panic!("{what}: {e:?}"));
    let (s, o, e) = ruled2(w, &rich, kind, amount, skip, a, lines).unwrap_or_else(|| panic!("{what}: not the rule's with {need}"));
    assert_eq!(e, (true, false), "{what}: at the edge");
    let poor = cov(w, legs, need - 1);
    assert!(stmt(&poor.s, kind, amount, skip, a, T0, need - 1, lines, xk(claimer)).unwrap().is_none(), "{what}: a sompi under, by the model");
    assert_eq!(handed(w, &poor, kind, &s, claimer), (Some(false), (false, true)), "{what}: a sompi under, at the engine");
    (s, o)
}

/// A step is due with coins of `most` and not with a sompi more (the same statement on the richer account: wrong).
fn due_edge(w: &World, what: &str, legs: &[XLeg], owes: i64, most: i64, skip: i64, a: &[i64]) -> (CSaid, COutcome) {
    let poor = cov_owing(w, legs, most, owes);
    let (s, o, e) = ruled2(w, &poor, KIND_C_STEP, 0, skip, a, None).unwrap_or_else(|| panic!("{what}: not due with {most}"));
    assert_eq!(e, (true, false), "{what}: due");
    let rich = cov_owing(w, legs, most + 1, owes);
    assert!((1..=legs.len() as i64).all(|k| stmt(&rich.s, KIND_C_STEP, 0, k, a, T0, most + 1, None, xk(&w.f)).unwrap().is_none()), "{what}: a sompi more, by the model");
    assert_eq!(handed(w, &rich, KIND_C_STEP, &s, &w.f), (Some(false), (false, true)), "{what}: a sompi more, at the engine");
    (s, o)
}

// ================================================================================================================
// Y2. A due account that nobody can step: a held option that lost, beside a leg that has not, and a token leg.

/// Y2 (high). Three rules meet. A held option is named alone only where every leg that stays is a held option
/// (E13): beside a futures leg or a written option it takes every leg. Every leg is refused beside a token leg
/// (an account that holds tokens is not taken whole). And a leg that gained (or stands where it entered) is named
/// only where no leg that stays has lost (O2). So an account of a held option that has lost, a futures leg that
/// has not, and a token leg takes **no step**, whatever it lacks; its tokens are not sold either (it owes nothing).
///
/// A put held on 100 units struck at 11, bought at 10 (worth 100 KAS exercised) for a premium of 1 KAS; a
/// hundredth of a long from 10; a hundredth of a token. Asked 55 and the premium at 10: coins of 57. At 10.5 the
/// put has lost 50 and the account is due; at 11.5 it has lost all 100, 43 KAS more than the coins hold. No step
/// of any leg is right at either price, by the model and at the engine. The same account without its hundredth of
/// a token is stepped at 10.5 (every leg; the shard is paid the 50 and the premium).
#[test]
fn t15_y2_a_held_option_that_lost_beside_a_leg_that_has_not_and_a_token_leg_takes_no_step() {
    let w = world();
    let put = oleg(&w, 0, PUT_HELD, 100, P10 * 11 / 10, P10, FAR, KAS1 / 100);
    let (long, tok) = (fleg_c(&w, 0, 1, LONG, P10), tleg_c(&w, 1, 1));
    let coins = NOTE_MIN + 57 * KAS1;
    // At 10 the account stands, at the level of a take.
    let whole = cov(&w, &[long, put, tok], coins);
    whole.s.c_check().expect("a covered account of the form");
    assert!(stmt(&whole.s, KIND_C_TAKE, KAS1 / 2, 0, &[P10; 3], T0, coins, None, xk(&w.w)).unwrap().is_some(), "it stands at 10");
    let mut said = vec![];
    for price in [P10 * 105 / 100, P10 * 115 / 100] {
        let a = [price, price, P10];
        let q = least_prices(&whole.s, &a, T0);
        let last = *whole.s.c_trace_at(KIND_C_STEP, T0, &whole.s.c_head(), &a, &q, |_, _| Part::Stays).unwrap().last().unwrap();
        let lost = -last.gain;
        assert!(coins - NOTE_MIN + last.tk + last.gain + last.m < last.pa && lost >= 49 * KAS1, "due at {price}: the legs lost {lost}");
        // Closed: the put named leaves alone (an account that holds tokens is not taken whole), and the shard
        // is paid or owed what it lost and its premium. The long (it has not lost while the put has) and the
        // token leg are named by no step.
        for skip in [1i64, 3] {
            assert!(whole.s.c_state(KIND_C_STEP, 0, skip, &a, T0, coins, xk(&w.f)).is_err() && stmt(&whole.s, KIND_C_STEP, 0, skip, &a, T0, coins, None, xk(&w.f)).unwrap().is_none(), "no step of leg {skip} at {price}");
        }
        let rows = whole.s.c_trace_at(KIND_C_STEP, T0, &whole.s.c_head(), &a, &q, |i, _| if i == 0 { Part::Leaves } else { Part::Stays }).unwrap();
        for fee in [0, KAS1 / 100] {
            let s = CSaid { amount: fee, skip: 1, lc: chain_of_one(&whole.s.legs[0]), head: whole.s.c_head(), rows: rows.clone() };
            assert_eq!(handed(&w, &whole, KIND_C_STEP, &s, &w.f), (Some(false), (false, true)), "the end of a step of the long at {price}");
        }
        assert!(whole.s.c_state(KIND_C_STEP, 0, 2, &a, T0, coins, xk(&w.f)).is_ok(), "Y2 (closed): `c_state` states the step of the put at {price}");
        let (s, o, e) = ruled2(&w, &whole, KIND_C_STEP, 0, 2, &a, None).expect("Y2 (closed): the put named takes a step");
        let end = o.after.clone().expect("the put leaves alone");
        let asked = o.note.clone().unwrap().asked;
        let put_lost = -s.rows[2].gl;
        assert_eq!((e, end.legs.clone()), ((true, false), vec![long, tok]), "Y2 (closed): the end at the engine at {price}");
        assert_eq!(-asked + end.b_margin, put_lost + KAS1, "Y2 (closed): at {price} the shard is paid or owed what the put lost and its premium");
        assert_eq!(end.b_margin > 0, put_lost > coins - NOTE_MIN, "it owes only what the coins do not pay");
        said.push(format!("at {price}: the put lost {put_lost}, the coins hold {}; the put leaves alone, the shard is paid {} and owed {}, the fee {}", coins - NOTE_MIN, -asked, end.b_margin, s.amount));
        let _ = lost;
    }
    // Without the hundredth of a token: every leg leaves at 10.5, and the shard is paid.
    let plain = cov(&w, &[long, put], coins);
    let a = [P10 * 105 / 100; 2];
    let (s, o, e) = ruled2(&w, &plain, KIND_C_STEP, 0, 2, &a, None).expect("with no token leg the put named takes every leg");
    let paid = -o.note.clone().unwrap().asked;
    assert_eq!((e, o.after.is_none()), ((true, false), true));
    assert_eq!(paid, -s.rows[1].gain + KAS1, "the shard is paid what the legs lost and the premium");
    println!("T15 Y2 (closed): a put held in the money that lost, a hundredth of a long, a hundredth of a token: {said:?}; without the token leg every leg leaves at 10.5 and the shard is paid {paid}");
}

// ================================================================================================================
// Y3. An option named alone pays of its premium what the coins hold, beside legs that have gained.

/// Y3 (medium). A leg that gained leaves alone only where the coins hold the premiums **that stay** (O3). Of the
/// premium of the leg that itself leaves, the coins pay what they can and the rest is not owed, whatever the legs
/// that stay have gained; where every leg leaves, that gain is set against the premium first. Whoever states the
/// step chooses, and may be the user.
///
/// A call written far out of the money for 9 KAS (the owner's coins), and a short of another market that gained 5;
/// the coins hold 7 (a loss was paid from them: the walk's account 830). Due. The written call named: it leaves
/// alone, the coins pay 7 of its 9, the account goes on with the short; its user then leaves with the short and
/// is paid the 4.05 it gained beyond the band. The short named instead (O3: the coins do not hold the premium
/// that stays): every leg, 4.05 set against the premium and 4.95 paid by the coins: the shard has its 9. By the
/// choice of the leg the shard has 2 KAS less of the premium and the user 3.29 more (the 2, and the fee of every
/// leg).
#[test]
fn t15_y3_an_option_named_alone_pays_of_its_premium_what_the_coins_hold_beside_a_leg_that_gained() {
    let w = world();
    let k = |hundredths: i64| hundredths * KAS1 / 100;
    let written = oleg(&w, 0, CALL_WRITTEN, 10, P10 * 13 / 10, P10, FAR, 9 * KAS1 / 10);
    let short = fleg(&w, 1, 10, SHORT, P10);
    let a = [P10, P10 * 95 / 100];
    let acct = cov(&w, &[written, short], NOTE_MIN + k(700));
    acct.s.c_check().expect("a covered account of the form");
    // Closed: the coins do not pay the call's premium (9, of which nothing is set against a gain of its own), so
    // the call named does not leave alone: every leg, as where the short is named. The short's gain beyond the
    // band (4.05) is set against the premium and the coins pay 4.95: the shard has its 9 whichever leg is named.
    let fee_all = (k(522) + k(1) / 2 + k(900)) / 550 * 50 + ((k(522) + k(1) / 2 + k(900)) % 550) * 50 / 550;
    let mut got = vec![];
    for skip in [1i64, 2] {
        let (s, o, e) = ruled2(&w, &acct, KIND_C_STEP, 0, skip, &a, None).expect("due");
        assert_eq!((e, o.after.is_none(), s.rows[1].po, s.rows[1].gain, s.rows[1].band(), o.note.clone().unwrap().asked, s.amount, o.paid), ((true, false), true, k(900), k(500), Some(k(95)), -k(495), fee_all, NOTE_MIN + k(700) - k(495) - fee_all), "Y3 (closed): leg {skip} named");
        got.push((o.note.unwrap().asked, o.paid));
    }
    assert_eq!(got[0], got[1], "Y3 (closed): the choice of the leg moves nothing of the premium");
    println!("T15 Y3 (closed): a written call of 9 KAS of premium beside a short that gained 5, coins 7: whichever leg is named every leg leaves, the shard has {} and the user {}", -got[0].0, got[0].1);
}

// ================================================================================================================
// Y6. How the model states (no script): a step's prices, the lines of a split, a token leg named.

/// Y6 (low to medium; the model's `c_state`, no script). (a) `c_step_prices` reads an option leg as a short
/// futures leg and tries the two ends of the lower rate only: a step that is due by a price (a strike between, or
/// the other end) is not stated. Two held calls, one in the money: due by the lower end with coins under 8 KAS
/// above a note's least; `c_state` states it only under 2.42. (In the walk: 1,920 of 5,783 right steps.)
/// (b) `c_head_least` takes the lines that ask least of all the legs as they stay; of a split the least is of
/// what stays and of what moves: a put held against a long, the long split off: by the lines `c_state` takes the
/// put that stays is asked 5.5 KAS that its other line does not ask. (c) `c_state` states a leaving, a split and
/// a step that name a token leg, with a trace in which the token leg stays: the row is wrong (`c_wrong_point`
/// says so), and the end is taken by the model as right.
#[test]
fn t15_y6_how_the_model_states_a_step_a_split_and_a_token_leg_named() {
    let w = world();
    let k = |hundredths: i64| hundredths * KAS1 / 100;
    // (a)
    let two = [oleg(&w, 0, CALL_HELD, 10, P10, P10, FAR, 3 * KAS1 / 10), oleg(&w, 0, CALL_HELD, 10, P10 * 13 / 10, P10, FAR, KAS1 / 2)];
    let at = [P10 / 100 * 102; 2];
    let mut missed = 0;
    for m in [k(100), k(241), k(243), k(500), k(799)] {
        let x = cov(&w, &two, NOTE_MIN + m).s;
        let due = stmt(&x, KIND_C_STEP, 0, 1, &at, T0, NOTE_MIN + m, None, xk(&w.f)).unwrap().is_some();
        let stated = x.c_state(KIND_C_STEP, 0, 1, &at, T0, NOTE_MIN + m, xk(&w.f)).is_ok();
        assert!(due, "due with coins {m}");
        assert!(stated, "Y6 (closed): (a) `c_state` states the step that is due with coins {m}");
        missed += usize::from(!stated);
    }
    assert_eq!(missed, 0);
    assert_eq!(x_step_prices(&cov(&w, &two, NOTE_MIN).s, &at), (at[0] - up_of(at[0], 450), at[0] - up_of(at[0], 450)), "Y6 (closed): the end `c_step_prices` takes is the end the step is due by");
    // (b)
    let hedge = [fleg(&w, 0, 10, LONG, P10), oleg(&w, 0, PUT_HELD, 10, P10, P10, FAR, KAS1 / 10)];
    let amount = NOTE_MIN + k(550);
    let x = cov(&w, &hedge, amount + NOTE_MIN + k(100)).s;
    let by_state = x.c_state(KIND_C_SPLIT, amount, 1, &[P10; 2], T0, x.a_margin, xk(&w.w));
    let by_line = stmt(&x, KIND_C_SPLIT, amount, 1, &[P10; 2], T0, x.a_margin, Some(0), xk(&w.w)).unwrap();
    assert!(by_state.is_ok() && by_line.is_some(), "Y6 (closed): (b) `c_state` states the split by the line that asks least of what stays");
    assert!(cov(&w, &hedge, amount + NOTE_MIN + k(100) - 1).s.c_state(KIND_C_SPLIT, amount, 1, &[P10; 2], T0, amount + NOTE_MIN + k(100) - 1, xk(&w.w)).is_err(), "and with a sompi less no split");
    // (c)
    let with_tok = cov(&w, &[fleg(&w, 0, 10, LONG, P10), tleg_c(&w, 1, 100)], 100 * KAS1);
    let mut named = vec![];
    for (kind, amount) in [(KIND_C_LEAVE, 0), (KIND_C_SPLIT, NOTE_MIN + KAS1)] {
        let got = with_tok.s.c_state(kind, amount, 2, &[P10; 2], T0, with_tok.value as i64, xk(&w.w)).ok().map(|(st, s, _)| with_tok.s.c_wrong_point(kind, &s, &st.claimer, &st.outs, st.held, T0, &[P10; 2]));
        named.push(got);
    }
    assert_eq!(named, vec![None, None], "Y6 (closed): (c) `c_state` states no leaving and no split that names a token leg");
    // (A step: a call written far out of the money whose premium the coins do not hold, and a tenth of a token:
    // due. The token leg is named by no step.)
    let due_tok = cov(&w, &[oleg(&w, 0, CALL_WRITTEN, 10, P10 * 13 / 10, P10, FAR, KAS1 / 5), tleg_c(&w, 1, 10)], NOTE_MIN);
    assert!(due_tok.s.c_state(KIND_C_STEP, 0, 2, &[P10; 2], T0, due_tok.value as i64, xk(&w.f)).is_err(), "Y6 (closed): (c) `c_state` states no step that names a token leg");
    println!("T15 Y6 (closed): (a) every due step is stated by `c_state`; (b) a split is stated by the lines that ask least of it; (c) no leaving, split or step that names a token leg is stated");
}

/// The ends of the lower rate `c_step_prices` takes for the first group, and the one a step of it is due by.
fn x_step_prices(x: &Account, a: &[i64]) -> (i64, i64) {
    (x.c_step_prices(a).unwrap()[0], least_prices(x, a, T0)[0])
}

// ================================================================================================================
// 3 (b), (d) and 4, in numbers: the day approaching; entries the tolerance off; the cap of a leg's fee.

/// (b) A long of 10 from 10 and a put held at 10 for 1 KAS, its day 301 blocks off; coins of the premium and 0.3.
/// While the day is more than T off the account stands (asked its premium alone, by the put's line of being
/// exercised) and no step is due. A block later, with no price moved, it is **due at once**: within T of its day
/// the put counts only where it loses. The long named: it leaves alone (it lost nothing), for a fee of the 0.3 the
/// coins hold beyond the premium; the put stays. The put named (E13): every leg, the coins pay the premium, the
/// fee is the 0.3 left. Neither leaves the shard unpaid. (N68 as written: said in the report as what a user of a
/// hedge to the day is to know.)
/// (d) Review T14's O1 for options: a call held at 9 entered at 9.95 and a call written at 9 entered at 10.05, the
/// committee at 10 both times. Every leg leaves at 10: the two show a gain of 1 KAS from prices alone; the band is
/// their entries' parts, 0.49975 and 0.50025: nothing is asked of the shard.
/// (4) The cap of a leg's fee by what the legs that stay lost together (review T14: the shard can be unpaid by one
/// fee). A long of 100 that lost 1, a long of another market that gained 80, a short of a third that lost 80;
/// coins 6. The first named: it leaves alone for the fee of its worth, 4.995, though the short that stays has lost
/// 80 alone. The second market then falls by the lower rate and every leg leaves: the shard is unpaid 48.595, and
/// with no fee taken it would be unpaid 43.6: one fee.
#[test]
fn t15_held_the_day_approaching_entries_the_tolerance_off_and_the_cap_of_a_legs_fee() {
    let w = world();
    let k = |hundredths: i64| hundredths * KAS1 / 100;
    // (b)
    let day = T0 + 301;
    let legs = [fleg(&w, 0, 10, LONG, P10), oleg(&w, 0, PUT_HELD, 10, P10, P10, day, KAS1 / 10)];
    let x = cov(&w, &legs, NOTE_MIN + k(130)).s;
    let a = [P10; 2];
    assert!(stmt(&x, KIND_C_TAKE, k(30), 0, &a, T0, x.a_margin, None, xk(&w.w)).unwrap().is_some(), "more than T off: asked its premium alone");
    assert!((1..=2).all(|s| stmt(&x, KIND_C_STEP, 0, s, &a, T0, x.a_margin, None, xk(&w.f)).unwrap().is_none()), "more than T off: no step");
    assert!(stmt(&x, KIND_C_TAKE, 1, 0, &a, T0 + 1, x.a_margin, None, xk(&w.w)).unwrap().is_none(), "within T: the long is asked whole");
    let (_, _, o) = stmt(&x, KIND_C_STEP, 0, 1, &a, T0 + 1, x.a_margin, None, xk(&w.f)).unwrap().expect("within T: due with no price moved");
    assert_eq!((o.after.clone().map(|y| (y.legs, y.a_margin)), o.fee, o.note.unwrap().asked), (Some((vec![legs[1]], NOTE_MIN + k(100))), k(30), 0));
    let (_, s, o) = stmt(&x, KIND_C_STEP, 0, 2, &a, T0 + 1, x.a_margin, None, xk(&w.f)).unwrap().expect("the put named");
    assert_eq!((o.after.is_none(), o.fee, o.note.unwrap().asked, o.paid, s.rows[1].closed().unwrap().0), (true, k(30), -k(100), NOTE_MIN, k(550)));
    // (d)
    let pair = [oleg(&w, 0, CALL_HELD, 10, P10 * 9 / 10, P10 / 1_000 * 995, FAR, KAS1 / 10), oleg(&w, 0, CALL_WRITTEN, 10, P10 * 9 / 10, P10 / 1_000 * 1_005, FAR, KAS1 / 10)];
    let y = cov(&w, &pair, NOTE_MIN + k(100));
    let (s, o, e) = ruled2(&w, &y, KIND_C_STEP, 0, 1, &[P10; 2], None).expect("due under its premiums");
    assert_eq!((e, s.rows[1].gain, s.rows[1].band(), s.rows[1].ba, o.after.is_none(), o.note.unwrap().asked), ((true, false), k(100), Some(k(100)), k(100), true, -k(100)), "what prices alone show is under the entries' parts");
    // (4)
    let three = [fleg(&w, 0, 100, LONG, P10), fleg(&w, 1, 100, LONG, P10), fleg(&w, 2, 100, SHORT, P10)];
    let at = [P10 / 1_000 * 999, P10 / 100 * 108, P10 / 100 * 108];
    let z = cov(&w, &three, NOTE_MIN + k(600)).s;
    let (_, s, o) = stmt(&z, KIND_C_STEP, 0, 1, &at, T0, z.a_margin, None, xk(&w.f)).unwrap().expect("due");
    let fee = k(499) + k(1) / 2;
    let left = o.after.clone().unwrap();
    assert_eq!((s.rows[2].ls, o.fee, left.a_margin, left.legs.len()), (k(8000), fee, NOTE_MIN + k(600) - k(100) - fee, 2), "the fee of the leg named is taken beside a leg that lost 80 alone");
    let down = [at[1] - up_of(at[1], 450), at[2]];
    let unpaid = |acct: &Account| {
        let (_, s, o) = (1..=2).find_map(|skip| stmt(acct, KIND_C_STEP, 0, skip, &down, T0, acct.a_margin, None, xk(&w.f)).unwrap().filter(|v| v.2.after.is_none())).expect("every leg leaves");
        -s.rows[1].gain + o.note.unwrap().asked
    };
    let with_fee = unpaid(&left);
    let without = unpaid(&Account { a_margin: left.a_margin + fee, ..left.clone() });
    assert_eq!((with_fee, with_fee - without), (k(4860) - (k(600) - k(100) - fee), fee), "the shard is unpaid one fee more");
    println!("T15 (b) a hedge to the day is due at once T before it (fee {} of the coins beyond the premium); (d) a pair of options entered the tolerance off is paid nothing of the {} prices show; (4) the cap of a leg's fee: the shard unpaid {with_fee}, {without} had no fee been taken", k(30), k(100));
}

// ================================================================================================================
// 2. The ends with premiums, a sompi either side, at the engine. Each `need` is the rule written out here in
// numbers (the rate 5.5 %, the lower rate 4.5 %, the tolerance 0.5 %, the fee 0.5 %), not read from the model.

#[test]
fn t15_held_the_ends_with_premiums_at_their_edges_a_sompi_either_side() {
    let w = world();
    let k = |hundredths: i64| hundredths * KAS1 / 100;
    let p = |hundredths: i64| P10 / 1_000 * hundredths;
    let long = fleg(&w, 0, 10, LONG, P10);
    let call_w11 = oleg(&w, 0, CALL_WRITTEN, 10, p(1_100), P10, FAR, KAS1 / 5);
    let call_h10 = oleg(&w, 0, CALL_HELD, 10, P10, P10, FAR, KAS1 / 2);
    let put_h10 = oleg(&w, 0, PUT_HELD, 10, P10, P10, FAR, KAS1 / 10);
    let mut n = 0;

    // 7. A covered call at 10.8: the long loses 5.94 below; above, the call written at 11 loses 3.94 of the 5.94
    // the long gains. Asked 5.94 and the premium of 2 the owner put in.
    edge(&w, "a take from a covered call", &[long, call_w11], NOTE_MIN + k(100) + k(594) + k(200), KIND_C_TAKE, KAS1, 0, &[p(1_080); 2], None);
    // 7. A put held at 10 against a long, at 10.3, on its line of being exercised: 3 below (5.665 less the 2.665
    // the put is exercised at there), 3 above (the long's 5.665 less the 8.665 the line loses); on its line of
    // nothing, the long's 5.665. And its premium of 1.
    edge(&w, "a take beside a put held, its line of being exercised", &[long, put_h10], NOTE_MIN + k(100) + k(300) + k(100), KIND_C_TAKE, KAS1, 0, &[p(1_030); 2], Some(0b10));
    edge(&w, "a take beside a put held, its line of nothing", &[long, put_h10], NOTE_MIN + k(100) + k(566) + k(1) / 2 + k(100), KIND_C_TAKE, KAS1, 0, &[p(1_030); 2], Some(0));
    let (s, _) = edge(&w, "a take beside a put held, the lines that ask least", &[long, put_h10], NOTE_MIN + k(100) + k(300) + k(100), KIND_C_TAKE, KAS1, 0, &[p(1_030); 2], None);
    assert_eq!(s.head.lines, 0b10);
    // 8. The long that gained 10 leaves from beside the call written at 11, at 11: the call alone loses 6.05 above;
    // and its premium.
    edge(&w, "a leg that gained leaves from beside a written call", &[long, call_w11], NOTE_MIN + k(605) + k(200), KIND_C_LEAVE, 0, 1, &[p(1_100); 2], None);
    // 8. The long that lost 10 leaves at 9: the coins pay it, and what stays (the call, far from its strike) is at
    // its premium.
    edge(&w, "a leg that lost leaves from beside a written call", &[long, call_w11], NOTE_MIN + k(1000) + k(200), KIND_C_LEAVE, 0, 1, &[p(900); 2], None);
    // 12. A held call moves with its premium of 5 (alone, on its line of nothing, it loses nothing); the long stays.
    let amount = NOTE_MIN + k(500);
    let (s, o) = edge(&w, "a split: a held call moved with its premium", &[long, call_h10], amount + NOTE_MIN + k(550), KIND_C_SPLIT, amount, 2, &[P10; 2], None);
    assert_eq!((o.split.clone().unwrap().legs, o.split.unwrap().a_margin), (vec![call_h10], amount));
    let rich = cov(&w, &[long, call_h10], amount + NOTE_MIN + k(550));
    assert_eq!(handed(&w, &rich, KIND_C_SPLIT, &CSaid { amount: amount - 1, ..s }, &w.w), (Some(false), (false, true)), "a split: the premium a sompi short in the new account");
    // 12. A call written at 10, at 10.5: it lost 5, loses 5.775 more above, and its premium of 2 moves with it; the
    // long stays alone (5.775).
    let call_w10 = oleg(&w, 0, CALL_WRITTEN, 10, P10, P10, FAR, KAS1 / 5);
    let amount = NOTE_MIN + k(577) + k(1) / 2 + k(200) + k(500);
    let (s, _) = edge(&w, "a split: a written call moved with the owner's premium", &[long, call_w10], amount + NOTE_MIN + k(577) + k(1) / 2, KIND_C_SPLIT, amount, 2, &[p(1_050); 2], None);
    let rich = cov(&w, &[long, call_w10], amount + NOTE_MIN + k(577) + k(1) / 2);
    assert_eq!(handed(&w, &rich, KIND_C_SPLIT, &CSaid { amount: amount - 1, ..s }, &w.w), (Some(false), (false, true)), "a split: the new account a sompi under");
    // 12. The long moves from beside the put held at 10: alone it is asked 5.5; the put that stays, on its line of
    // nothing, its premium.
    let amount = NOTE_MIN + k(550);
    edge(&w, "a split: a long moved from beside a held put", &[long, put_h10], amount + NOTE_MIN + k(100), KIND_C_SPLIT, amount, 1, &[P10; 2], Some(0));
    // 13. A mark of the long that gained 10, beside the put held at 10 (far below the ends at 11): the long's 6.05
    // and the premium.
    edge(&w, "a mark beside a held put", &[long, put_h10], NOTE_MIN + k(605) + k(100), KIND_C_MARK, 0, 1, &[p(1_100); 2], None);
    // 14. Tokens leave from beside a held call: the coins hold its premium.
    edge(&w, "tokens leave from beside a held call", &[call_h10, tleg_c(&w, 1, 100)], NOTE_MIN + k(500), KIND_C_TOK_OUT, 0, 2, &[P10; 2], None);
    n += 12 * 2 + 3;

    // 9. Due below the premiums: a call held at 12 for 5, alone, at 10: due with coins under 5. The coins do not
    // pay all of its premium, so it does not leave alone (after Y3): every leg, which is the same leg; the
    // coins pay what they hold of its premium and no account is left.
    let call_h12 = oleg(&w, 0, CALL_HELD, 10, p(1_200), P10, FAR, KAS1 / 2);
    let (_, o) = due_edge(&w, "a held call alone", &[call_h12], 0, NOTE_MIN + k(500) - 1, 1, &[P10]);
    assert_eq!((o.after.clone().map(|x| (x.legs.len(), x.a_margin)), o.note.clone().unwrap().asked, o.fee), (None, -(k(500) - 1), 0));
    // 9. A long that lost 5 at 9.5 (4.275 more at the lower rate below) beside a call held at 11 for 5: due with
    // coins under 14.275.
    let call_h11 = oleg(&w, 0, CALL_HELD, 10, p(1_100), P10, FAR, KAS1 / 2);
    let pair = [long, call_h11];
    let low = [p(950); 2];
    let (_, o) = due_edge(&w, "a long beside a held call", &pair, 0, NOTE_MIN + k(500) + k(427) + k(1) / 2 + k(500) - 1, 1, &low);
    // (The long leaves alone; its fee, 0.5 % of 95, is capped after the premium that stays: O4.)
    assert_eq!((o.after.clone().map(|x| x.legs), o.fee, o.note.unwrap().asked), (Some(vec![call_h11]), k(47) + k(1) / 2, -k(500)));
    for (m, fee) in [(k(1000), 0), (k(1000) + 1, 1), (k(1047), k(47)), (k(1048), k(47) + k(1) / 2)] {
        let (s, o, e) = ruled2(&w, &cov(&w, &pair, NOTE_MIN + m), KIND_C_STEP, 0, 1, &low, None).unwrap();
        assert_eq!((e, s.amount, o.after.map(|x| x.a_margin)), ((true, false), fee, Some(NOTE_MIN + m - k(500) - fee)), "the fee of the long with coins {m}: no more than the coins hold beyond the premium that stays");
        // (A sompi more of fee: wrong.)
        assert_eq!(handed(&w, &cov(&w, &pair, NOTE_MIN + m), KIND_C_STEP, &CSaid { amount: fee + 1, ..s }, &w.f), (Some(false), (false, true)));
        n += 2;
    }
    // (With coins a sompi under the 5 it lost: every leg, in the order loss, premium, fee: nothing for the premium.)
    let (s, o, e) = ruled2(&w, &cov(&w, &pair, NOTE_MIN + k(500) - 1), KIND_C_STEP, 0, 1, &low, None).unwrap();
    assert_eq!((e, o.after.is_none(), s.amount, o.note.unwrap().asked, o.paid), ((true, false), true, 0, -(k(500) - 1), NOTE_MIN));
    // E13. The held call named beside the long: every leg. The coins pay the 5 lost, then the premium of 5, then
    // the fee of what is asked (5.225 and 5, by 50 over 550).
    let fee_all = (k(522) + k(1) / 2 + k(500)) * 50 / 550;
    for (m, pay, fee) in [(k(700), k(700), 0), (k(1000), k(1000), 0), (k(1000) + 1, k(1000), 1), (k(1000) + fee_all, k(1000), fee_all), (k(1300), k(1000), fee_all)] {
        let (s, o, e) = ruled2(&w, &cov(&w, &pair, NOTE_MIN + m), KIND_C_STEP, 0, 2, &low, None).unwrap();
        assert_eq!((e, o.after.is_none(), s.amount, o.note.unwrap().asked, o.paid), ((true, false), true, fee, -pay, NOTE_MIN + m - pay - fee), "every leg with coins {m}: loss, premium, fee");
        assert_eq!(handed(&w, &cov(&w, &pair, NOTE_MIN + m), KIND_C_STEP, &CSaid { amount: fee + 1, ..s }, &w.f), (Some(false), (false, true)));
        n += 2;
    }
    // E13. A held call named beside a written call (far out of the money: nothing lost): every leg; beside only
    // a held call: alone.
    let far_w = oleg(&w, 0, CALL_WRITTEN, 10, p(1_300), P10, FAR, KAS1 / 10);
    let (s, o, e) = ruled2(&w, &cov(&w, &[call_h12, far_w], NOTE_MIN + k(300)), KIND_C_STEP, 0, 1, &[P10; 2], None).unwrap();
    // (Every leg: PO is the held call's 5 and the written call's 1: the coins pay 3 of it.)
    assert_eq!((e, o.after.is_none(), s.rows[1].ns, s.rows[1].po, o.note.unwrap().asked), ((true, false), true, 256 + 1, k(600), -k(300)));
    // (A held call that gained nothing, named beside a held call: E13 lets it leave alone, and O3 does not: here
    // the legs gain nothing to the step's price, so the due account's coins hold less than the premiums, and
    // after they pay of the one that leaves they do not hold the one that stays. Every leg; the coins pay 3 of
    // the 10.)
    let (s, o, e) = ruled2(&w, &cov(&w, &[call_h12, call_h11], NOTE_MIN + k(300)), KIND_C_STEP, 0, 1, &[P10; 2], None).unwrap();
    assert_eq!((e, o.after.is_none(), s.rows[1].ns, s.rows[1].po, o.note.unwrap().asked), ((true, false), true, 256, k(1000), -k(300)));
    n += 2;

    // O3, a sompi either side. A call held at 10 from 10 for 3, at 10.2 (it gained 2), beside a call held at 13 for
    // 5. Named, the first: its band alone is 0.5 (its entry's part) and 0.51 (the leaving's); 0.99 of its premium
    // is set against what it gained beyond that, the coins pay 2.01 more. It leaves alone where the coins then
    // hold the 5 of the premium that stays: 7.01; a sompi under, every leg.
    let a10 = oleg(&w, 0, CALL_HELD, 10, P10, P10, FAR, 3 * KAS1 / 10);
    let b13 = oleg(&w, 0, CALL_HELD, 10, p(1_300), P10, FAR, KAS1 / 2);
    let at = [p(1_020); 2];
    let two = [a10, b13];
    let (s, o, e) = ruled2(&w, &cov(&w, &two, NOTE_MIN + k(701)), KIND_C_STEP, 0, 1, &at, None).expect("due under 8");
    assert_eq!((s.rows[1].bl, s.rows[1].gl, s.rows[1].rl), (k(101), k(200), k(200)));
    assert_eq!((e, o.after.clone().map(|x| (x.legs, x.a_margin)), s.amount, o.note.unwrap().asked), ((true, false), Some((vec![b13], NOTE_MIN + k(500))), 0, -k(201)), "O3: the coins hold the premium that stays: alone");
    let (s, o, e) = ruled2(&w, &cov(&w, &two, NOTE_MIN + k(701) - 1), KIND_C_STEP, 0, 1, &at, None).unwrap();
    // (Every leg: the legs gained 2, under the band of 1.01: 0.99 set against the premiums of 8, the coins pay
    // what they hold of the rest.)
    assert_eq!((e, o.after.is_none(), s.amount, o.note.unwrap().asked), ((true, false), true, 0, -(k(701) - 1)), "O3: a sompi under: every leg");
    // The fee of an option named alone: of what it can lose alone (2) and its premium (3), by 50 over 550; no more
    // than the coins hold beyond what they pay of its premium and the premium that stays.
    let fee_opt = k(500) / 550 * 50 + (k(500) % 550) * 50 / 550;
    for (m, fee) in [(k(701) + 1, 1), (k(701) + fee_opt, fee_opt), (k(799), fee_opt)] {
        let (s, o, e) = ruled2(&w, &cov(&w, &two, NOTE_MIN + m), KIND_C_STEP, 0, 1, &at, None).unwrap();
        assert_eq!((e, s.amount, o.after.map(|x| x.a_margin)), ((true, false), fee, Some(NOTE_MIN + m - k(201) - fee)), "the fee of a held call named alone with coins {m}");
        assert_eq!(handed(&w, &cov(&w, &two, NOTE_MIN + m), KIND_C_STEP, &CSaid { amount: fee + 1, ..s }, &w.f), (Some(false), (false, true)));
        n += 2;
    }
    let (_, _) = due_edge(&w, "two held calls", &two, 0, NOTE_MIN + k(800) - 1, 1, &at);
    n += 2 + 2;

    // PO of every option leg where one leg was named and every leg leaves. A long that lost 5 at 9.5, a call held at
    // 11 for 5, a put written at 9.6 for 2 that lost 1: the put pays 1 of its premium (the shard has the larger of
    // 2 and the 1 it lost). The held call named (E13): every leg; the coins pay the 6 lost, then the 6 of the
    // premiums, then the fee. (The rows were written with one leg leaving: PO is of every option leg.)
    let put_w = oleg(&w, 0, PUT_WRITTEN, 10, p(960), P10, FAR, KAS1 / 5);
    let three = [long, call_h11, put_w];
    for (m, pay) in [(k(400), k(400)), (k(900), k(900)), (k(1200), k(1200)), (k(1250), k(1200))] {
        let (s, o, e) = ruled2(&w, &cov(&w, &three, NOTE_MIN + m), KIND_C_STEP, 0, 2, &[p(950); 3], None).unwrap();
        assert_eq!((e, o.after.is_none(), s.rows[2].po, s.rows[2].ps, s.rows[2].gain, s.rows[2].ns, o.note.unwrap().asked, s.amount), ((true, false), true, k(600), k(200), -k(600), 256 + 2, -pay, m - pay), "PO of every option leg with coins {m}");
        n += 1;
    }
    // (The long named with coins that pay it: it leaves alone, and the account lacks less.)
    let (_, o, e) = ruled2(&w, &cov(&w, &three, NOTE_MIN + k(900)), KIND_C_STEP, 0, 1, &[p(950); 3], None).unwrap();
    assert_eq!((e, o.after.map(|x| x.legs.len()), o.note.unwrap().asked), ((true, false), Some(2), -k(500)));
    // (The written put named, the coins paying the 1 it lost and the 1 of its premium: it leaves alone.)
    let (s, o, e) = ruled2(&w, &cov(&w, &three, NOTE_MIN + k(1300)), KIND_C_STEP, 0, 3, &[p(950); 3], None).unwrap();
    assert_eq!((e, o.after.map(|x| x.legs.len()), s.rows[2].ns, o.note.unwrap().asked), ((true, false), Some(2), 1, -k(200)), "a written put that lost 1 pays the larger of its premium and what it lost");
    n += 2;

    // What is owed (16.18) beside a premium. A call held at 10 from 10 for 3, at 11 (it gained 10; its band alone
    // 0.5 and 0.55), and a tenth of a token; the account owes 4 and holds a note's least. Due. Named, the call
    // leaves alone (an account that holds tokens is not taken whole): of the 8.95 it gained beyond its band, 3
    // are its premium, 4 what the account owes, and 1.95 are asked of the shard.
    let owing = cov_owing(&w, &[a10, tleg_c(&w, 1, 10)], NOTE_MIN, k(400));
    let (s, o, e) = ruled2(&w, &owing, KIND_C_STEP, 0, 1, &[p(1_100), P10], None).expect("due");
    let after = o.after.clone().unwrap();
    assert_eq!((e, s.rows[1].bl, after.legs.len(), after.b_margin, after.a_margin, o.note.unwrap().asked, s.amount), ((true, false), k(105), 1, 0, NOTE_MIN, k(195), 0), "what is owed is set against what the option gained beyond its band and its premium");
    n += 1;

    // A futures leg whose worth rounds to nothing (a hundred-millionth of a unit at half a KAS) reads, in the end,
    // as an option does (it adds nothing to the legs' worth): it has no premium, and pays none. Entered at 5 KAS,
    // it lost 5 sompi; beside the call held at 12 whose premium the coins do not hold it is named: the account
    // lacks no less without it, so every leg leaves; the coins pay the 5 sompi, then what they hold of the
    // call's premium.
    let dust = XLeg { size: 1, ..fleg(&w, 1, 1, LONG, P10 / 2) };
    let (s, o, e) = ruled2(&w, &cov(&w, &[dust, call_h12], NOTE_MIN + k(300)), KIND_C_STEP, 0, 1, &[P10 / 20, P10], None).expect("due under the premium");
    assert_eq!((e, s.rows[1].w, s.rows[1].gl, s.rows[1].ns, s.rows[1].po, o.after.is_none(), o.note.unwrap().asked, s.amount), ((true, false), 0, -5, 0, k(500), true, -k(300), 0));
    n += 1;

    // 10. A long of 10 enters at 10 beside the put held at 10 for 1: by the put's line of being exercised the two
    // are asked nothing but the premium. The end of the entry at the engine, a sompi either side.
    {
        use kpf_protocol::covered::Marked;
        let need = NOTE_MIN + k(100);
        let x = cov(&w, &[put_h10], need);
        let (st, s, marked, after) = x.s.c_enter(long, 0, 0, &[P10], T0, need).expect("an entry at the edge");
        assert_eq!((after.legs.len(), s.head.lines, s.rows[1].closed().unwrap().0, s.rows[1].pa), (2, 0b1, 0, k(100)));
        assert!(cov(&w, &[put_h10], need - 1).s.c_enter(long, 0, 0, &[P10], T0, need - 1).is_err(), "an entry a sompi under, by the model");
        let taken = |marked: &Marked, st: &Stated| {
            let live = OLive { s: *st, value: (st.held + st.bond) as u64, id: x.id, op: op(81), daa: 6_510 };
            let q = w.pointed(&live, 0);
            let take = |v: bool| {
                let f = [fund(xk(&w.w), 35)];
                c_enter_end(&w.lib, &q, marked, &s, &w.script("KpfTCEnter"), xk(&w.f), w.payer(&w.w, &f), Some(v)).is_ok_and(|b| accepted(&b.0, &w.keys))
            };
            (take(true), take(false))
        };
        assert_eq!(taken(&marked, &st), (true, false), "an entry beside a held put, at the edge");
        let poor = Marked(Account { a_margin: need - 1, ..marked.0.clone() });
        let st2 = Stated { position: poor.hash(), held: need - 1, bond: poor.0.c_bond(), ..st };
        assert_eq!((Account::c_enter_end(&poor, &s, &st2.claimer, st2.held, T0).map(|o| o.is_some()), taken(&poor, &st2)), (Some(false), (false, true)), "an entry a sompi under, at the engine");
        n += 2;
    }
    println!("T15 the ends with premiums: {n} ends at the engine, each as the rule written out says, a sompi either side");
}

// ================================================================================================================
// 3. The rule's own money, on the model: walks of accounts with options.

/// Whether a step is due on the account at the prices `a` and the time `t0`, by the end's test written out, at the
/// prices that show it; and the last row it is due by.
fn due_now(x: &Account, a: &[i64], t0: i64) -> Option<(bool, CRow)> {
    let q = least_prices(x, a, t0);
    let last = *x.c_trace_at(KIND_C_STEP, t0, &x.c_head(), a, &q, |_, _| Part::Stays)?.last()?;
    let mut sx = last.gain + last.m;
    if sx >= 0 {
        sx = (sx - last.band()?).max(0);
    }
    Some((x.a_margin - NOTE_MIN + last.tk - x.b_margin + sx < last.pa, last))
}

#[derive(Default, Debug)]
struct OWalked {
    accounts: usize,
    with_options: usize,
    takes: usize,
    leavings: usize,
    splits: usize,
    steps_alone: usize,
    steps_all: usize,
    /// Steps stated while nothing but the time had changed since the account stood (a leg came within T of its day).
    steps_by_time: usize,
    fees_by_time: i64,
    /// Lives that ended because a leg reached its day (the day's transaction is the seventh step's).
    reached_day: usize,
    /// Visits at which a step was due and none was right.
    stuck: usize,
    stuck_lost: i64,
    /// Statements of the user's after which a step was due at the same prices and time; and by what it was: a
    /// take, a leaving, a split.
    due_after_own: usize,
    due_after: [usize; 3],
    /// What the legs that left lost beyond what the shard was paid or is owed.
    unpaid_loss: i64,
    unpaid_loss_on: usize,
    /// What the option legs that left were to pay of their premiums beyond what was set against a gain or paid.
    unpaid_prem: i64,
    unpaid_prem_on: usize,
    unpaid_prem_most: i64,
    /// Of those: on accounts whose coins were not all taken by the step (the user kept coins or tokens).
    unpaid_prem_kept: i64,
    unpaid_prem_kept_on: usize,
    /// Due steps that `Account::c_state` (its `c_step_prices`) does not state.
    not_stated: usize,
    due_seen: usize,
    /// Where the rule as written here and the model's end part.
    parts: Vec<String>,
    /// The first steps that left a premium unpaid, as they were.
    notes: Vec<String>,
}

/// `tokens`: every account holds a token leg. `moves`: the most a market's price moves between two visits, in
/// basis points. `policy`: which right step whoever watches takes: 0 any, 1 the one that pays it most.
fn option_walk(w: &World, seed: u64, accounts: usize, tokens: bool, moves: i64, policy: i64) -> OWalked {
    let mut r = Rng(seed);
    let mut out = OWalked::default();
    let prices_of = |legs: &[XLeg], p: &[i64; 3]| -> Vec<i64> { legs.iter().map(|l| p[w.markets.iter().position(|x| x.id() == l.market).unwrap()]).collect() };
    for n in 0..accounts {
        let mut p = [P10 * r.range(80, 121) / 100, P10 * r.range(80, 121) / 100, P10 * r.range(80, 121) / 100];
        let mut t = T0;
        let count = r.range(1, 5) as usize;
        let mut m = r.range(0, 2) as usize;
        let mut legs: Vec<XLeg> = vec![];
        for j in 0..count {
            if j > 0 && r.range(0, 3) == 0 {
                m = (m + 1 + r.range(0, 2) as usize) % 3;
            }
            let cents = r.range(50, 2_000);
            if r.range(0, 5) < 2 {
                legs.push(fleg_c(w, m, cents, if r.range(0, 2) == 0 { LONG } else { SHORT }, p[m] / 100 * r.range(96, 105)));
            } else {
                let strike = match r.range(0, 4) {
                    0 => p[m],
                    _ => p[m] / 100 * r.range(88, 113),
                };
                legs.push(oleg_c(w, m, r.pick(&[CALL_HELD, CALL_WRITTEN, PUT_HELD, PUT_WRITTEN]), cents, strike, p[m] / 100 * r.range(97, 104), T0 + r.range(301, 6_000), p[m] / 1_000 * r.range(0, 60)));
            }
        }
        if tokens {
            legs.push(tleg_c(w, 2, r.range(1, 400)));
        }
        let mut x = cov(w, &legs, NOTE_MIN).s;
        let a0 = prices_of(&x.legs, &p);
        let head = x.c_head_least(KIND_C_TAKE, t, &a0);
        let Some(last) = x.c_trace_at(KIND_C_TAKE, t, &head, &a0, &a0, |_, _| Part::Stays).and_then(|v| v.last().copied()) else { continue };
        // (At the level of a take and a little more, in coins: what its tokens count stands above that.)
        x.a_margin = NOTE_MIN + (last.closed().unwrap().0 + last.pa + last.ls - if tokens { last.tk.min(last.pa / 2) } else { 0 }).max(0) + last.w / 1_000 * r.range(0, 30);
        if x.c_check().is_err() {
            continue;
        }
        out.accounts += 1;
        if x.legs.iter().any(|l| option_of(l).is_some()) {
            out.with_options += 1;
        }
        let mut moved = true;
        let mut history: Vec<String> = vec![format!("coins {} at {:?}", x.a_margin - NOTE_MIN, p)];
        'life: for _ in 0..10 {
            for pass in 0..2 {
                // (pass 0: after the prices moved; pass 1: after only the time did.)
                if x.legs.iter().filter_map(option_of).any(|o| o.day <= t) {
                    out.reached_day += 1;
                    break 'life;
                }
                // Whoever watches: every right step, until none is.
                loop {
                    let a = prices_of(&x.legs, &p);
                    if x.legs.iter().all(|l| l.dir == DIR_SPOT) {
                        break 'life;
                    }
                    let mut right = vec![];
                    for skip in 1..=x.legs.len() as i64 {
                        // (No step names a token leg: its row is wrong. Y6: the model's end takes it.)
                        if x.legs[skip as usize - 1].dir == DIR_SPOT {
                            continue;
                        }
                        match stmt(&x, KIND_C_STEP, 0, skip, &a, t, x.a_margin, None, xk(&w.f)) {
                            Ok(Some((_, s, o))) => {
                                out.due_seen += 1;
                                if x.c_state(KIND_C_STEP, 0, skip, &a, t, x.a_margin, xk(&w.f)).is_err() {
                                    out.not_stated += 1;
                                }
                                right.push((skip, s, o));
                            }
                            Ok(None) => {}
                            Err(e) => out.parts.push(format!("account {n} (seed {seed:#x}) at {t}: {e}: {:?}", x.legs)),
                        }
                    }
                    if right.is_empty() {
                        if let Some((true, last)) = due_now(&x, &a, t) {
                            out.stuck += 1;
                            out.stuck_lost += (-last.gain - (x.a_margin - NOTE_MIN)).max(0);
                        }
                        break;
                    }
                    let k = if policy == 1 { (0..right.len()).max_by_key(|k| right[*k].2.fee).unwrap() } else { r.range(0, right.len() as i64) as usize };
                    let (skip, s, o) = right.swap_remove(k);
                    if !moved {
                        out.steps_by_time += 1;
                        out.fees_by_time += o.fee;
                    }
                    // What the shard was paid, against what the legs that left lost and were to pay of their premiums.
                    let last = *s.rows.last().unwrap();
                    let asked = o.note.as_ref().unwrap().asked;
                    let room = (x.a_margin - NOTE_MIN).max(0);
                    let (lost, paid_loss, due_prem, set, owed) = match &o.after {
                        None => {
                            let lost = (-last.gain).max(0);
                            let x_ = if last.gain < 0 { 0 } else { (last.gain - last.band().unwrap()).max(0) };
                            (lost, lost.min(room), last.po, last.po.min(x_), 0)
                        }
                        Some(y) => {
                            let l = &x.legs[skip as usize - 1];
                            let pl = last.pa - last.ps;
                            let pol = match option_of(l) {
                                None => 0,
                                Some(o) if o.held => pl,
                                Some(_) => (pl + last.gl.min(0)).max(0),
                            };
                            let lost = (-last.gl).max(0);
                            let owed = y.b_margin - x.b_margin;
                            (lost, lost - owed.max(0), pol, pol.min((last.gl - last.bl).max(0)), owed.max(0))
                        }
                    };
                    // (The note asks, below nothing, what the coins pay of loss and of premium.)
                    let paid = (-asked).max(0);
                    let paid_prem = (paid - paid_loss).max(0);
                    if lost > paid_loss + owed {
                        out.unpaid_loss += lost - paid_loss - owed;
                        out.unpaid_loss_on += 1;
                    }
                    // What the shard is short of a premium, net: what the legs that left were to pay of their
                    // premiums, less what they gained (a gain under the band is the shard's already), beyond what
                    // the coins paid. And what the user keeps beside it: coins, what its tokens count, and what
                    // the legs that stay have gained.
                    let gained = if o.after.is_none() { last.gain.max(0) } else { last.gl.max(0) };
                    let short = (due_prem - set - paid_prem).min(due_prem - gained - paid_prem);
                    if short > 0 {
                        out.unpaid_prem += short;
                        out.unpaid_prem_on += 1;
                        out.unpaid_prem_most = out.unpaid_prem_most.max(short);
                        let kept = match &o.after {
                            None => o.paid - NOTE_MIN,
                            Some(y) => y.a_margin - NOTE_MIN + last.tk + (last.gain - last.gl).max(0),
                        };
                        if kept > 0 {
                            out.unpaid_prem_kept += short.min(kept);
                            out.unpaid_prem_kept_on += 1;
                        }
                        if out.notes.len() < 12 {
                            out.notes.push(format!("account {n} at {t} (moved {moved}), skip {skip}, every leg {}: coins {}, gain {}, m {}, band {:?}, R {:?}, pa {}, ps {}, po {}, gl {}, bl {}, ls {}, asked {asked}, short {short}; legs {:?}; history {:?}", o.after.is_none(), x.a_margin - NOTE_MIN, last.gain, last.m, last.band(), last.closed(), last.pa, last.ps, last.po, last.gl, last.bl, last.ls, x.legs.iter().map(|l| (l.dir, l.size / 1_000_000, l.entry, option_of(l).map(|o| (o.strike, o.day, o.premium)))).collect::<Vec<_>>(), history));
                        }
                    }
                    history.push(format!("step of {skip} at {t} {:?}: asked {asked}, fee {}", p, o.fee));
                    match o.after {
                        Some(y) => {
                            out.steps_alone += 1;
                            x = y;
                        }
                        None => {
                            out.steps_all += 1;
                            assert!(!tokens, "every leg left beside a token leg");
                            break 'life;
                        }
                    }
                }
                if pass == 1 {
                    break;
                }
                // The user: a statement of its own, at the markets' prices.
                let a = prices_of(&x.legs, &p);
                // (A leaving and a split name no token leg: the row of one named by them is wrong.)
                let len = x.legs.iter().filter(|l| l.dir != DIR_SPOT).count() as i64;
                if len == 0 {
                    break 'life;
                }
                let act = r.range(0, 6);
                let after = match act {
                    0..=2 => {
                        let head = x.c_head_least(KIND_C_TAKE, t, &a);
                        let last = *x.c_trace_at(KIND_C_TAKE, t, &head, &a, &a, |_, _| Part::Stays).unwrap().last().unwrap();
                        let mm = x.a_margin - NOTE_MIN;
                        let most = mm.min(mm + last.tk - last.closed().unwrap().0 - last.pa - last.ls).min(mm - last.pa);
                        if most > 0 && t >= x.b_min_counter && x.b_margin == 0 {
                            let (_, _, o) = stmt(&x, KIND_C_TAKE, most, 0, &a, t, x.a_margin, None, xk(&w.w)).unwrap().expect("the most a take gives is the rule's");
                            assert!(stmt(&x, KIND_C_TAKE, most + 1, 0, &a, t, x.a_margin, None, xk(&w.w)).unwrap().is_none());
                            out.takes += 1;
                            o.after
                        } else {
                            None
                        }
                    }
                    3 => stmt(&x, KIND_C_LEAVE, 0, r.range(1, len + 1), &a, t, x.a_margin, None, xk(&w.w)).unwrap().and_then(|(_, _, o)| {
                        out.leavings += 1;
                        o.after
                    }),
                    4 if len >= 2 => {
                        // A split: the leg with what it is asked alone, as the lines of a take have it.
                        let skip = r.range(1, len + 1);
                        let head = x.c_head_least(KIND_C_TAKE, t, &a);
                        x.c_trace_at(KIND_C_SPLIT, t, &head, &a, &a, |i, l| if i as i64 + 1 == skip && l.dir != DIR_SPOT { Part::Leaves } else { Part::Stays }).and_then(|v| v.last().copied()).and_then(|last| {
                            let amount = NOTE_MIN + last.rl + last.pa - last.ps + (-last.gl).max(0);
                            stmt(&x, KIND_C_SPLIT, amount, skip, &a, t, x.a_margin, None, xk(&w.w)).unwrap().and_then(|(_, _, o)| {
                                out.splits += 1;
                                o.after
                            })
                        })
                    }
                    _ => None,
                };
                if let Some(y) = after {
                    history.push(format!("the user's at {t} {:?}: coins {} to {}, legs {} to {}", p, x.a_margin - NOTE_MIN, y.a_margin - NOTE_MIN, x.legs.len(), y.legs.len()));
                    x = y;
                    if x.legs.is_empty() {
                        break 'life;
                    }
                    if due_now(&x, &prices_of(&x.legs, &p), t).is_some_and(|d| d.0) {
                        out.due_after_own += 1;
                        out.due_after[(act.max(2) - 2) as usize] += 1;
                    }
                }
                // The time goes on (a statement of the user's is two lives of a statement apart from the next).
                t += 400 + r.range(0, 300);
                moved = false;
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
            moved = true;
        }
    }
    out
}

/// Accounts of one to four legs, futures and options of the four ways (strikes within 12 % of the price, days 300
/// to 6,000 blocks off, premiums up to 6 % of the price), from the level of a take, over ten visits: whoever
/// watches takes every right step (at the prices that show it due); the user takes the most a take gives (by the
/// lines that ask least), leaves with a futures leg, splits a leg off; the time goes on (legs come within T of
/// their days) and each market's price moves by no more than the lower rate, half the time by all of it. Every
/// statement at the markets' prices.
///
/// Held: no account is due after its user's own statement; a due account always takes a step; the shard is unpaid
/// no loss; and the rule as written in this file (`step_ruled`) is the model's end on every step. A premium is
/// short (net of what the legs that left gained) only as Y3 has it: an option named alone beside a leg that
/// gained; 7 steps of 3,341 where whoever watches takes any right step, none where it takes the one that pays
/// it most. Counted beside: the steps that came by the time alone (a hedge that stops counting T before a day),
/// and the right steps `Account::c_state` does not state (Y6).
#[test]
fn t15_held_on_the_model_with_options_the_shard_is_unpaid_nothing_at_the_markets_prices() {
    let w = world();
    let said = |o: &OWalked| {
        format!(
            "{} accounts ({} with options), {} takes, {} leavings, {} splits; steps: {} of a leg, {} of every leg, {} with nothing but the time changed (fees {}); {} lives ended at a leg's day; stuck {} (the legs then lost {} beyond the coins); due after its user's own statement {}; a loss unpaid: {} on {}; a premium short, net of what the legs that left gained: {} on {} (the most {}), of it beside coins, tokens or a gain the user keeps {} on {}; right steps `c_state` does not state: {} of {}; the rule as written here parts from the model's end on {}; the first short: {:?}",
            o.accounts, o.with_options, o.takes, o.leavings, o.splits, o.steps_alone, o.steps_all, o.steps_by_time, o.fees_by_time, o.reached_day, o.stuck, o.stuck_lost, o.due_after_own, o.unpaid_loss, o.unpaid_loss_on, o.unpaid_prem, o.unpaid_prem_on, o.unpaid_prem_most, o.unpaid_prem_kept, o.unpaid_prem_kept_on, o.not_stated, o.due_seen, o.parts.len(), o.notes.first().map(|s| s.chars().take(400).collect::<String>())
        )
    };
    for policy in [0, 1] {
        let o = option_walk(&w, 0x15_0010 + policy as u64, 6_000, false, 450, policy);
        println!("T15 walk with options, whoever watches taking {}: {}", if policy == 1 { "the step that pays most" } else { "any right step" }, said(&o));
        assert!(o.parts.is_empty(), "the rule as written here and the model part: {:?}", o.parts.iter().take(4).collect::<Vec<_>>());
        assert!(o.takes > 1_000 && o.steps_alone > 300 && o.steps_all > 300 && o.steps_by_time > 50, "{}", said(&o));
        assert_eq!((o.stuck, o.due_after_own, o.unpaid_loss, o.due_after), (0, 0, 0, [0; 3]), "{}", said(&o));
        // (Y3: every premium that is short is short beside something the user keeps; none where whoever steps
        // takes the step that pays it most, which is the step of every leg.)
        assert_eq!((o.unpaid_prem, o.unpaid_prem_on), (o.unpaid_prem_kept, o.unpaid_prem_kept_on), "{}", said(&o));
        assert_eq!(o.unpaid_prem_kept, 0, "Y3 (closed): no premium is short beside what the user keeps: {}", said(&o));
    }
}

/// The same walk with a token leg in every account (whoever watches taking the step that pays most): Y1 and Y2 as
/// they come. No loss is unpaid (what the coins do not pay is owed); accounts are stuck, and premiums are short
/// beside tokens the user keeps.
#[test]
fn t15_y1_y2_on_the_model_with_a_token_leg_in_every_account() {
    let w = world();
    let o = option_walk(&w, 0x15_0012, 6_000, true, 450, 1);
    println!("T15 walk with options and a token leg in every account: {} accounts, {} steps of a leg; stuck at {} visits (the legs then lost {} beyond the coins); a premium short on {} steps, {} in all (the most {}), {} of it beside what the user keeps; a loss unpaid {}; due after its user's own statement {}", o.accounts, o.steps_alone, o.stuck, o.stuck_lost, o.unpaid_prem_on, o.unpaid_prem, o.unpaid_prem_most, o.unpaid_prem_kept, o.unpaid_loss, o.due_after_own);
    assert!(o.parts.is_empty(), "the rule as written here and the model part: {:?}", o.parts.iter().take(4).collect::<Vec<_>>());
    assert_eq!((o.unpaid_loss, o.due_after_own, o.steps_all), (0, 0, 0));
    assert_eq!(o.stuck, 0, "Y2 (closed): no account with a token leg is stuck");
    assert_eq!(o.unpaid_prem_kept, 0, "Y1 (closed): no premium is short beside tokens the user keeps");
}

// ================================================================================================================
// 1. `KpfTCRow` against `crow.rs` for option legs: a walk at the engine.

const NAMES: [&str; 24] = ["a", "gain", "w", "ws", "ls", "gl", "tk", "q", "m", "ba", "bl", "dda", "dua", "ra", "dds", "dus", "rs", "rl", "pa", "ps", "po", "ddb", "dub", "ns"];

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
        20 => &mut r.po,
        21 => &mut r.ddb,
        22 => &mut r.dub,
        _ => &mut r.ns,
    }
}

/// What leg `j` (from 0) does in a statement of `kind` that names `skip` (a mark: entered from now at `amount`).
fn part_of(kind: i64, skip: i64, amount: i64, j: usize, l: &XLeg) -> Part {
    let named = skip == ALL || skip == j as i64 + 1;
    let tok = kind == KIND_C_TOK_OUT || kind == KIND_C_TOK_SOLD;
    if !named || (l.dir == DIR_SPOT && !tok) {
        Part::Stays
    } else if kind == KIND_C_MARK {
        Part::Marked(amount)
    } else {
        Part::Leaves
    }
}

fn chained(before: &[u8; 32], l: &XLeg) -> [u8; 32] {
    kpf_protocol::committee::blake2b(&[&before[..], &kpf_protocol::thin::leg_bytes(l)].concat())
}

/// The row of leg `j` after `before`, as the model writes it; where the model has no right row (or a sum does not
/// fit), the row a stater that ignores the one rule would write: the row of the same leg with a day far off, a
/// strike and a premium within their range, in a kind that lets it leave, carrying the true leg's chains; and where
/// that has none either, the row before with the leg's chain.
fn row_or_bent(kind: i64, head: &CHead, j: usize, before: &CRow, lb: Option<&XLeg>, l: &XLeg, a: i64, q: i64, part: Part) -> CRow {
    if let Some(Some(r)) = crow_after(kind, head, T0, j, before, lb, l, a, q, part) {
        return r;
    }
    let g = chained(&before.g, l);
    let h = if part == Part::Stays { chained(&before.h, l) } else { before.h };
    if let Some(o) = option_of(l) {
        let l2 = XLeg { committee: leg_more(6_000, if o.day <= T0 { FAR } else { o.day }, o.strike.clamp(1, l.price_max), o.premium.max(0)), ..*l };
        let (kind2, part2, q2) = if part == Part::Stays || kind == KIND_C_STEP || kind == KIND_C_SPLIT { (kind, part, q) } else { (KIND_C_SPLIT, Part::Leaves, a) };
        if let Some(Some(r)) = crow_after(kind2, head, T0, j, before, lb, &l2, a, q2, part2) {
            return CRow { q, g, h, ..r };
        }
    }
    CRow { a, q, g, ..*before }
}

/// An account of the walk: one to six legs (futures, tokens and options of the four ways) in groups, and the price
/// stated for each. `edge` of the strikes, days, premiums and entries are at the places where the rule turns.
fn random_legs(w: &World, r: &mut Rng, most: i64) -> (Vec<XLeg>, Vec<i64>) {
    let n = r.range(1, most + 1) as usize;
    let price = |r: &mut Rng| match r.range(0, 12) {
        0 => r.pick(&[1i64, 2, 7, 19, 181, 10_000, 9_999]),
        1 => P10 * 1_000 + r.range(-5, 6),
        _ => P10 * r.range(80, 121) / 100 + r.range(-3, 4),
    };
    let (mut legs, mut a) = (vec![], vec![]);
    let (mut m, mut p) = (r.range(0, 4) as usize, price(r));
    for j in 0..n {
        if j > 0 && r.range(0, 3) == 0 {
            m = (m + 1 + r.range(0, 3) as usize) % 4;
            p = price(r);
        }
        let mk = &w.markets[m];
        let size = match r.range(0, 8) {
            0 => r.pick(&[1i64, 3, 99_999_999, 100_000_001, 33_333_333]),
            1 => 12_345 * (mk.p.scale / 100),
            _ => r.pick(&[1i64, 37, 100, 1_000, 250]) * (mk.p.scale / 100),
        };
        let base = XLeg { market: mk.id(), committee: leg_more(6_000, 0, 0, 0), scale: mk.p.scale, price_max: mk.p.price_max, size, dir: LONG, entry: p };
        let kind = r.range(0, 10);
        let leg = if kind < 3 {
            XLeg { dir: if r.range(0, 2) == 0 { LONG } else { SHORT }, entry: (p / 100 * r.range(92, 109) + r.range(0, 3)).max(1), ..base }
        } else if kind == 3 {
            // (A discount out of its range, named by the kinds of its own, is Y5's: not walked here.)
            XLeg { dir: DIR_SPOT, committee: [0x99; 32], entry: r.pick(&[2_000i64, 1, 10_000, 550, 0]), ..base }
        } else {
            let way = r.pick(&[CALL_HELD, CALL_WRITTEN, PUT_HELD, PUT_WRITTEN]);
            let (lo, up) = ends(p, 550).unwrap();
            let (lob, upb) = ends(p, 50).unwrap();
            let strike = match r.range(0, 24) {
                0 => lo,
                1 => lo + 1,
                2 => lo - 1,
                3 => up,
                4 => up - 1,
                5 => up + 1,
                6 => p,
                7 => p + 1,
                8 => p - 1,
                9 => lob,
                10 => lob + 1,
                11 => lob - 1,
                12 => upb,
                13 => upb - 1,
                14 => upb + 1,
                15 => p / 10 * 12,
                16 => p / 10 * 7,
                17 => r.pick(&[1i64, mk.p.price_max, mk.p.price_max - 1]),
                18 if r.range(0, 3) == 0 => r.pick(&[0i64, -5, mk.p.price_max + 1]),
                _ => lo + r.range(0, (up - lo).max(1) + 1),
            };
            let day = match r.range(0, 16) {
                0 => T0 + 300,
                1 => T0 + 301,
                2 => T0 + 299,
                3 => T0 + 1,
                4 => T0 + 2,
                5 if r.range(0, 2) == 0 => r.pick(&[T0, T0 - 1, 6_001]),
                _ => FAR,
            };
            let premium = match r.range(0, 12) {
                0 => 0,
                1 => 1,
                2 => p,
                3 => 10_000_000_000_000,
                4 if r.range(0, 4) == 0 => -1,
                _ => r.pick(&[KAS1 / 2, KAS1 / 5, 3 * KAS1, 31_415_926]),
            };
            let k = strike.clamp(1, mk.p.price_max);
            let entry = match r.range(0, 12) {
                0 => k,
                1 => k + 1,
                2 => k - 1,
                3 => k / 9_950 * 10_000,
                4 => k / 9_950 * 10_000 + r.range(-2, 3),
                5 => k / 10_050 * 10_000 + r.range(-2, 3),
                6 => p / 100 * 93,
                7 => p / 100 * 108,
                _ => p,
            };
            XLeg { dir: way, committee: leg_more(6_000, day, strike, premium), entry: entry.clamp(1, mk.p.price_max), ..base }
        };
        legs.push(leg);
        a.push(p);
    }
    (legs, a)
}

/// A step's prices: one a group, at the edges of the lower rate, a sompi beyond them, and at the strikes within.
fn random_step_prices(r: &mut Rng, legs: &[XLeg], a: &[i64]) -> Vec<i64> {
    let mut q = a.to_vec();
    let mut i = 0;
    while i < legs.len() {
        let mut j = i;
        while j < legs.len() && legs[j].market == legs[i].market {
            j += 1;
        }
        let dq = up_of(a[i], 450);
        let strikes: Vec<i64> = legs[i..j].iter().filter_map(option_of).map(|o| o.strike).filter(|k| (a[i] - k).abs() <= dq).collect();
        let x = match r.range(0, 10) {
            0 | 1 => a[i],
            2 => a[i] - dq,
            3 => a[i] + dq,
            4 if r.range(0, 3) == 0 => a[i] - dq - 1,
            5 if r.range(0, 3) == 0 => a[i] + dq + 1,
            6 | 7 if !strikes.is_empty() => r.pick(&strikes) + r.range(-1, 2),
            _ => a[i] - dq + r.range(0, 2 * dq + 1),
        };
        for v in q.iter_mut().take(j).skip(i) {
            *v = x.max(0);
        }
        i = j;
    }
    q
}

#[derive(Default, Debug)]
struct RowsWalked {
    statements: usize,
    rows: usize,
    right: usize,
    wrong: usize,
    neither: usize,
    /// Rows of option legs, by the model's verdict (right, wrong).
    option_right: usize,
    option_wrong: usize,
    held_between: usize,
    near: usize,
    off: usize,
    off_right: usize,
    lines_flipped: usize,
    lines_changed: usize,
    parts: Vec<String>,
}

/// The model's verdict on row `i` (from 1) of `s` on the account's legs, and what the engine takes of its answer.
fn row_both(w: &World, acct: &OLive<Account>, kind: i64, s: &CSaid, i: usize) -> (Option<bool>, (bool, bool)) {
    let legs = &acct.s.legs;
    let before = if i == 1 { CRow::ZERO } else { s.rows[i - 2] };
    let lb = i.checked_sub(2).map(|k| &legs[k]);
    let model = c_row_judged_at(kind, T0, &before.g, lb, &legs[i - 1], &before, &s.rows[i - 1], i as i64, s);
    let q = w.pointed_fast(acct, &by_hand(acct, kind, s, xk(claimer_of(w, kind))), 2 * i as i64 - 1);
    (model, row_taken(w, &q, s, 2 * i as i64 - 1, &before.g, lb, &legs[i - 1]))
}

/// The walk: `accounts` accounts, a statement of one of `kinds` on each; every row as the model writes it (or as a
/// stater that ignores the one rule would) is judged by the model and answered at the engine, which must take the
/// model's verdict and no other. On every `off`-th statement each of the 24 numbers of each right row, a sompi
/// more and a sompi less. For each held option's place, the head's bit set and cleared: the rows as they were, and
/// the rows written again.
fn rows_walk(w: &World, seed: u64, accounts: usize, kinds: &[i64], off: usize) -> RowsWalked {
    let mut r = Rng(seed);
    let mut out = RowsWalked::default();
    for n in 0..accounts {
        let (legs, a) = random_legs(w, &mut r, 6);
        let kind = r.pick(kinds);
        let acct = cov(w, &legs, 100 * KAS1);
        let len = legs.len() as i64;
        // (A take names no leg and a split, a mark and the tokens' kinds name one: their ends hold that. A row is
        // of whatever its statement says.)
        let skip = match kind {
            KIND_C_TAKE if r.range(0, 8) != 0 => 0,
            KIND_C_STEP | KIND_C_LEAVE if r.range(0, 5) == 0 => ALL,
            _ if r.range(0, 12) == 0 => ALL,
            _ => r.range(1, len + 1),
        };
        let q = if kind == KIND_C_STEP { random_step_prices(&mut r, &legs, &a) } else { a.clone() };
        let amount = if kind == KIND_C_MARK { (a[(skip.max(1) - 1) as usize] / 100 * r.range(97, 104)).max(1) } else { KAS1 };
        let lines = match r.range(0, 6) {
            0 => 0,
            1 => 63,
            2 => r.range(0, 64) + (1i64 << 40),
            _ => r.range(0, 64),
        };
        let t = match r.range(0, 12) {
            0 => 0,
            1 => -1,
            2 => 1_000_000,
            _ => 300,
        };
        let head = CHead { lines, t, ..acct.s.c_head() };
        let write = |head: &CHead, from: usize, rows: &mut Vec<CRow>| {
            rows.truncate(from);
            for j in from..legs.len() {
                let before = if j == 0 { CRow::ZERO } else { rows[j - 1] };
                rows.push(row_or_bent(kind, head, j, &before, j.checked_sub(1).map(|k| &legs[k]), &legs[j], a[j], q[j], part_of(kind, skip, amount, j, &legs[j])));
            }
        };
        let mut rows = vec![];
        write(&head, 0, &mut rows);
        let lc = if skip >= 1 { chain_of_one(&legs[skip as usize - 1]) } else { [0; 32] };
        let s = CSaid { amount, skip, lc, head, rows };
        out.statements += 1;
        let what = |i: usize| format!("account {n} (seed {seed:#x}), kind {kind}, skip {skip}, lines {lines}, T {t}, row {i} of {len}: leg {:?} (its 32 bytes: {:?}) at {} and {}", legs[i - 1], option_of(&legs[i - 1]), a[i - 1], q[i - 1]);
        for i in 1..=legs.len() {
            let (model, engine) = row_both(w, &acct, kind, &s, i);
            out.rows += 1;
            match model {
                Some(true) => out.right += 1,
                Some(false) => out.wrong += 1,
                None => out.neither += 1,
            }
            if let Some(o) = option_of(&legs[i - 1]) {
                match model {
                    Some(true) => out.option_right += 1,
                    Some(false) => out.option_wrong += 1,
                    None => {}
                }
                let (lo, up) = ends(a[i - 1], 550).unwrap();
                if model == Some(true) && o.held && lo < o.strike && o.strike < up {
                    out.held_between += 1;
                }
                if model == Some(true) && o.day - T0 <= t {
                    out.near += 1;
                }
            }
            if engine != verdicts(model) {
                out.parts.push(format!("{}: the model says {model:?}, the engine takes (right, wrong) = {engine:?}", what(i)));
            }
            // Each number a sompi off.
            if off > 0 && n % off == 0 && model == Some(true) {
                for k in 0..24 {
                    for d in [1i64, -1] {
                        let mut bent = s.clone();
                        *number(&mut bent.rows[i - 1], k) += d;
                        let (model, engine) = row_both(w, &acct, kind, &bent, i);
                        out.off += 1;
                        if model == Some(true) {
                            out.off_right += 1;
                        }
                        // (A price a sompi off may be a right row of its own: a leg too small for a sompi to count,
                        // an option far from its strike. Every other number is the model's or the row is wrong.)
                        if engine != verdicts(model) || (model != Some(false) && k != 0 && k != 7) {
                            out.parts.push(format!("{}, {} by {d}: the model says {model:?}, the engine takes {engine:?}", what(i), NAMES[k]));
                        }
                    }
                }
            }
            // The head's bit of this place, set and cleared.
            if option_of(&legs[i - 1]).is_some_and(|o| o.held) || (off > 0 && n % off == 0) {
                let flipped = CHead { lines: lines ^ (1 << (i - 1)), ..head };
                let as_was = CSaid { head: flipped, ..s.clone() };
                let (m1, e1) = row_both(w, &acct, kind, &as_was, i);
                let mut again = as_was.clone();
                write(&flipped, i - 1, &mut again.rows);
                let (m2, e2) = row_both(w, &acct, kind, &again, i);
                out.lines_flipped += 1;
                if again.rows[i - 1] != s.rows[i - 1] {
                    out.lines_changed += 1;
                }
                // (In a step the line is of nothing whatever the bit: the row is the same row.)
                let same = again.rows[i - 1] == s.rows[i - 1];
                if e1 != verdicts(m1) || e2 != verdicts(m2) || m2 != model || (model == Some(true) && (m1 != Some(same) || (kind == KIND_C_STEP && !same))) {
                    out.parts.push(format!("{}, its bit of the lines flipped: the row as it was {m1:?} {e1:?}, written again {m2:?} {e2:?} (it was {model:?})", what(i)));
                }
            }
        }
    }
    out
}

fn rows_walk_said(what: &str, o: &RowsWalked) {
    println!("T15 rows walk, {what}: {} statements, {} rows ({} right, {} wrong, {} by neither verdict; of option legs {} right and {} wrong; {} of a held option between the ends, {} within T of the day); {} rows with a number a sompi off ({} of them right rows of another price); {} bits of the lines flipped ({} change the row); the engine parts from the model on {}: {:#?}", o.statements, o.rows, o.right, o.wrong, o.neither, o.option_right, o.option_wrong, o.held_between, o.near, o.off, o.off_right, o.lines_flipped, o.lines_changed, o.parts.len(), o.parts.iter().take(6).collect::<Vec<_>>());
}

/// The short way to a disputed point (`pointed_fast`) is the long way's object: the same hash and the same value.
#[test]
fn t15_held_the_short_way_to_a_disputed_point_is_the_long_way() {
    let w = world();
    let legs = [fleg(&w, 0, 10, LONG, P10), oleg(&w, 0, PUT_HELD, 10, P10, P10, FAR, KAS1 / 10)];
    let acct = cov(&w, &legs, 100 * KAS1);
    let (st, s, _) = acct.s.c_state(KIND_C_TAKE, KAS1, 0, &[P10; 2], T0, acct.value as i64, xk(&w.w)).unwrap();
    let long = w.pointed(&w.stated(&acct, &st, &s.said()), 3);
    let short = w.pointed_fast(&acct, &st, 3);
    assert_eq!((long.s.hash(), long.value, long.id), (short.s.hash(), short.value, short.id));
    assert_eq!(row_taken(&w, &short, &s, 3, &s.rows[0].g, Some(&legs[0]), &legs[1]), (true, false));
    assert_eq!(row_taken(&w, &long, &s, 3, &s.rows[0].g, Some(&legs[0]), &legs[1]), (true, false));
}

/// Kinds 7, 9 and 12 (a take, a step with prices of its own, a split): the rows of futures, token and option legs.
#[test]
fn t15_held_rows_of_takes_steps_and_splits_the_engine_takes_the_models_verdict() {
    let w = world();
    let o = rows_walk(&w, 0x15_0001, 2_400, &[KIND_C_TAKE, KIND_C_STEP, KIND_C_STEP, KIND_C_SPLIT], 0);
    rows_walk_said("kinds 7, 9, 12", &o);
    assert!(o.parts.is_empty(), "{} rows part", o.parts.len());
    assert!(o.option_right >= 2_000 && o.option_wrong >= 300 && o.held_between >= 400 && o.near >= 200 && o.lines_changed >= 100, "{o:?}");
}

// ---------------------------------------------------------------------------------------------- an entry (10)

/// Kind 10: a leg enters beside the legs of the walk (five at the most): a futures leg at its entry, next to a leg
/// of its market or apart; at another price than its entry; an option or a token as the leg the entry says (wrong
/// whatever the row sums). The rows of the legs the account has are a take's (the head's lines count).
#[test]
fn t15_held_rows_of_an_entry_beside_options_the_engine_takes_the_models_verdict() {
    use kpf_protocol::covered::{c_enter_row_judged_at, CEnterSaid, Marked, KIND_C_ENTER};
    let w = world();
    let mut r = Rng(0x15_0004);
    let (mut rows_n, mut right, mut wrong, mut neither, mut last_option, mut last_off, mut off) = (0, 0, 0, 0, 0, 0, 0);
    let mut parts: Vec<String> = vec![];
    for n in 0..900 {
        let (mut legs, mut a) = random_legs(&w, &mut r, 5);
        let last = *legs.last().unwrap();
        let what = r.range(0, 10);
        let (m, p) = if r.range(0, 2) == 0 && last.dir != DIR_SPOT { (w.markets.iter().position(|x| x.id() == last.market).unwrap(), *a.last().unwrap()) } else { ((w.markets.iter().position(|x| x.id() == last.market).unwrap() + 1) % 4, P10 * r.range(80, 121) / 100) };
        let mk = &w.markets[m];
        let base = XLeg { market: mk.id(), committee: leg_more(T0, 0, 0, 0), scale: mk.p.scale, price_max: mk.p.price_max, size: r.pick(&[100i64, 37, 1_000]) * (mk.p.scale / 100), dir: if r.range(0, 2) == 0 { LONG } else { SHORT }, entry: p };
        let leg = match what {
            0 => XLeg { entry: p + 1, ..base },
            1 | 2 => {
                last_option += 1;
                XLeg { dir: r.pick(&[CALL_HELD, CALL_WRITTEN, PUT_HELD, PUT_WRITTEN]), committee: leg_more(T0, FAR, p / 100 * r.range(95, 106), KAS1 / 5), ..base }
            }
            3 => XLeg { dir: DIR_SPOT, committee: [0x99; 32], entry: 2_000, ..base },
            _ => base,
        };
        legs.push(leg);
        a.push(p);
        let acct = cov(&w, &legs, 100 * KAS1);
        let head = CHead { lines: r.range(0, 128), ..acct.s.c_head() };
        let mut rows: Vec<CRow> = vec![];
        for j in 0..legs.len() {
            let before = if j == 0 { CRow::ZERO } else { rows[j - 1] };
            rows.push(row_or_bent(KIND_C_ENTER, &head, j, &before, j.checked_sub(1).map(|k| &legs[k]), &legs[j], a[j], a[j], Part::Stays));
        }
        let s = CEnterSaid { add: 0, r: 0, leg, head, place: legs.len() as i64, rows };
        let marked = Marked(acct.s.clone());
        let held = acct.value as i64;
        let both = |t: &CEnterSaid, i: usize| -> (Option<bool>, (bool, bool)) {
            let before = if i == 1 { CRow::ZERO } else { t.rows[i - 2] };
            let lb = i.checked_sub(2).map(|k| &legs[k]);
            let model = c_enter_row_judged_at(T0, &before.g, lb, &legs[i - 1], &before, &t.rows[i - 1], i as i64, t);
            let st = Stated { position: marked.hash(), window: acct.s.dispute_daa, late: acct.s.t.late_daa, kind: KIND_C_ENTER, t0: T0, arg: t.arg(), outs: Outs::of(&[Out { key: false, to: acct.s.hash(), value: held }]).unwrap(), bond: marked.0.c_bond(), claimer: xk(&w.w), held };
            let q = OLive { s: Pointed { st, disputer: xk(&w.w), point: 2 * i as i64 - 1 }, value: (held + 2 * st.bond) as u64, id: acct.id, op: op(82), daa: 6_520 };
            let take = |v: bool| {
                let f = [fund(xk(&w.w), 36)];
                c_enter_row(&w.lib, &q, t, 2 * i as i64 - 1, &before.g, lb, &legs[i - 1], &w.script("KpfTCRow"), xk(&w.f), w.payer(&w.w, &f), Some(v)).is_ok_and(|b| accepted(&b.0, &w.keys))
            };
            (model, (take(true), take(false)))
        };
        for i in 1..=legs.len() {
            let (model, engine) = both(&s, i);
            rows_n += 1;
            match model {
                Some(true) => right += 1,
                Some(false) => wrong += 1,
                None => neither += 1,
            }
            let is_last = i == legs.len();
            // (The leg an entry says: a futures leg at its entry, or the row is wrong.)
            let rule_last = is_last && (what <= 3) && model != Some(false);
            if engine != verdicts(model) || rule_last {
                parts.push(format!("entry {n}, row {i} of {} ({what}): leg {:?} at {}: the model says {model:?}, the engine takes {engine:?}", legs.len(), legs[i - 1], a[i - 1]));
            }
            if is_last && what <= 3 {
                last_off += 1;
            }
            if n % 30 == 0 && model == Some(true) {
                for k in 0..24 {
                    for d in [1i64, -1] {
                        let mut bent = s.clone();
                        *number(&mut bent.rows[i - 1], k) += d;
                        let (model, engine) = both(&bent, i);
                        off += 1;
                        if engine != verdicts(model) || (model != Some(false) && k != 0 && k != 7) || (is_last && model != Some(false)) {
                            parts.push(format!("entry {n}, row {i}, {} by {d}: the model says {model:?}, the engine takes {engine:?}", NAMES[k]));
                        }
                    }
                }
            }
        }
    }
    println!("T15 rows of an entry: {rows_n} rows ({right} right, {wrong} wrong, {neither} by neither verdict); {last_off} last rows of a leg no entry says or not at its entry ({last_option} of an option), each wrong; {off} rows with a number a sompi off; the engine parts from the model on {}: {:#?}", parts.len(), parts.iter().take(6).collect::<Vec<_>>());
    assert!(parts.is_empty(), "{} rows part", parts.len());
    assert!(right >= 1_500 && last_option >= 100 && off >= 2_000, "{right} {last_option} {off}");
}

// ---------------------------------------------------------------------------------------------- Y4, Y5

/// Y4 (low; an account not of the form). `KpfTCRow` counts a leg's worth at the two ends, and at its entry, as a
/// futures leg's before it reads what the leg is; the model counts for an option leg only what its rule reads.
/// (a) A call held on its line of nothing, whose worth fits at its row's price and not at the upper end (900,000
/// units at 100,000 KAS: 9 * 10^18 sompi, 5.5 % more above): right by the model, answered by neither verdict at
/// the engine (review T13's W9, for an option leg). (b) An option leg that is wrong before anything (a strike of
/// nothing) and whose worth at its entry does not fit: wrong by the model, answered by neither verdict. Neither leg
/// is of the form (`leg_size_ok`): what holds an option leg to its bounds is the seventh step's entry.
#[test]
fn t15_y4_an_option_legs_row_where_a_worth_the_rule_does_not_read_does_not_fit() {
    let w = world();
    let m = &w.markets[0];
    let p = 10_000_000_000_000i64;
    let big = |strike: i64, entry: i64| XLeg { market: m.id(), committee: leg_more(6_000, FAR, strike, KAS1), scale: m.p.scale, price_max: m.p.price_max, size: 900_000 * m.p.scale, dir: CALL_HELD, entry };
    let small = XLeg { size: 9_000 * m.p.scale, ..big(p, p) };
    let mut got = vec![];
    for (leg, lines) in [(small, 0), (big(p, p), 0), (big(p, p), 1), (big(0, m.p.price_max), 0)] {
        let acct = cov(&w, &[leg], 100 * KAS1);
        let head = CHead { lines, ..acct.s.c_head() };
        let s = CSaid { amount: KAS1, skip: 0, lc: [0; 32], head, rows: vec![row_or_bent(KIND_C_TAKE, &head, 0, &CRow::ZERO, None, &leg, p, p, Part::Stays)] };
        got.push(row_both(&w, &acct, KIND_C_TAKE, &s, 1));
    }
    assert!(!kpf_protocol::thin::leg_size_ok(&big(p, p)) && kpf_protocol::thin::leg_size_ok(&small));
    assert_eq!(got[0], (Some(true), (true, false)), "a hundredth the size: right, and answered right");
    assert_eq!(got[1], (Some(true), (false, false)), "Y4 is closed: (a) the model and the engine agree on a held call on its line of nothing whose worth at the upper end does not fit");
    assert_eq!(got[2], (None, (false, false)), "on its line of being exercised the upper end is read: neither, by both");
    assert_eq!(got[3], (Some(false), (false, false)), "Y4 is closed: (b) the model and the engine agree on a leg that is wrong before anything and whose worth at its entry does not fit");
    println!("T15 Y4 (open): an option leg out of the form: (a) right by the model, (b) wrong by the model; the engine takes neither verdict of either");
}

/// Y5 (low; a token leg; not of this stage). A token leg whose discount is out of its range (above the whole, or
/// below nothing), named by the kinds of its own (14, 15): the model's row is right (`crow_after` reads the
/// discount only where the leg stays) and the engine answers it wrong (`KpfTCRow` reads it before it knows that
/// the leg leaves). No account of the form has such a leg (`c_check`, `spot_added`).
#[test]
fn t15_y5_a_token_leg_named_whose_discount_is_out_of_range_is_right_by_the_model_and_wrong_at_the_engine() {
    let w = world();
    let mut got = vec![];
    for (disc, kind) in [(10_001i64, KIND_C_TOK_OUT), (10_001, KIND_C_TOK_SOLD), (-1, KIND_C_TOK_OUT), (10_000, KIND_C_TOK_OUT)] {
        let legs = [fleg(&w, 0, 10, LONG, P10), XLeg { entry: disc, ..tleg_c(&w, 1, 100) }];
        let acct = cov(&w, &legs, 100 * KAS1);
        let head = acct.s.c_head();
        let r1 = row_or_bent(kind, &head, 0, &CRow::ZERO, None, &legs[0], P10, P10, Part::Stays);
        let r2 = row_or_bent(kind, &head, 1, &r1, Some(&legs[0]), &legs[1], P10, P10, Part::Leaves);
        let s = CSaid { amount: 0, skip: 2, lc: chain_of_one(&legs[1]), head, rows: vec![r1, r2] };
        got.push(row_both(&w, &acct, kind, &s, 2));
    }
    assert_eq!(got[3], (Some(true), (true, false)), "a discount of the whole: right");
    for g in &got[..3] {
        assert_eq!(*g, (Some(false), (false, true)), "Y5 (closed): the model and the engine agree on a token leg named whose discount is out of range");
    }
    println!("T15 Y5 (closed): a token leg named by kinds 14 and 15 whose discount is out of range: wrong by the model and at the engine");
}

/// Kinds 8, 13, 14 and 15: an option named by a leaving, a mark, or as tokens, is wrong; one that stays is counted.
#[test]
fn t15_held_rows_of_leavings_marks_and_tokens_the_engine_takes_the_models_verdict() {
    let w = world();
    let o = rows_walk(&w, 0x15_0002, 1_600, &[KIND_C_LEAVE, KIND_C_MARK, KIND_C_TOK_OUT, KIND_C_TOK_SOLD], 0);
    rows_walk_said("kinds 8, 13, 14, 15", &o);
    assert!(o.parts.is_empty(), "{} rows part", o.parts.len());
    assert!(o.option_right >= 1_500 && o.option_wrong >= 800, "{o:?}");
}

/// Each of a row's 24 numbers a sompi off, in every kind: wrong by the model and answered wrong, and not right.
#[test]
fn t15_held_every_number_of_a_row_a_sompi_off_is_answered_wrong() {
    let w = world();
    let o = rows_walk(&w, 0x15_0003, 200, &[KIND_C_TAKE, KIND_C_STEP, KIND_C_SPLIT, KIND_C_STEP, KIND_C_LEAVE, KIND_C_MARK, KIND_C_TOK_OUT], 1);
    rows_walk_said("a sompi off", &o);
    assert!(o.parts.is_empty(), "{} rows part", o.parts.len());
    assert!(o.off >= 20_000 && o.option_right >= 250, "{o:?}");
}
