//! Review T6 (docs/audit/REVIEW-T6-claims.md): the claims of a called shard (docs/ARCHITECTURE-CLEARING.md 17.8c), at
//! the script engine. The reviewer's file: its world and helpers are its own (after `thin_draw.rs`).
//!
//! Piece (a) of 17.8c: what a called shard has paid. A claim of four numbers; a shard's state with the claims' three
//! numbers; `KpfTCallPay.claim` and `.off`; the time; `KpfTDrawNote`'s check. Every transaction is built, signed and
//! run through `verify::verify`. Shards and claims are made by hand where no script writes them (every state of the
//! second class; every state of B1 and B2): said at each test.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::CMarket;
use kpf_protocol::committee::*;
use kpf_protocol::cross::{XLeg, XTerms};
use kpf_protocol::oracle_build::{OBuilt, OLive, OPlan, OSigned};
use kpf_protocol::reserve::{Called, Claim, Claims, Draw, RowHead, Left, MarkNote, Owed, Rest, Shard, ShardRow, CALL_HOLD, NOTE_MIN, SHARD_CARRIER};
use kpf_protocol::state::{sm8, LONG};
use kpf_protocol::thin::{Account, Out, Outs, Pointed, Stated, KIND_DRAW};
use kpf_protocol::thin_build::*;
use kpf_protocol::tx::{p2pk_spk, Arg, KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use silverscript_abi::ArtifactValue;

const KAS1: i64 = 100_000_000;
const T0: i64 = 6_500;
const P10: i64 = 1_000_000_000;
/// The call's time of the shards made by hand.
const CALLED: i64 = 6_400;

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
/// A coin of `key` that landed at `daa`.
fn coin(key: [u8; 32], n: u8, daa: i64) -> KeyUtxo {
    KeyUtxo { daa: daa as u64, ..fund(key, n) }
}
fn accepted(b: &OBuilt, keys: &KeySet) -> bool {
    match b.sign(keys) {
        Ok(sg) => kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(),
        Err(_) => false,
    }
}
/// Whether a transaction is written and taken.
fn taken(b: kpf_protocol::Result<OBuilt>, keys: &KeySet) -> bool {
    b.is_ok_and(|b| accepted(&b, keys))
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
    /// A shard of the clearing party's (`c`), 100 KAS, no row (as `thin_draw.rs`'s `bare`).
    fn bare(&self) -> OLive<Shard> {
        let s = Shard { t: self.terms(), rows: [ShardRow::EMPTY; 4].into(), owner: xk(&self.c), held: 100 * KAS1, expiry: 0, dispute_daa: 50, bond_bps: 1_000, sid: [0x5d; 32], basis: 0, called: 0, owed: 0, claims: Claims::default(), root: Shard::root_opened(), dgross: 0 };
        OLive { s, value: (s.held + SHARD_CARRIER) as u64, id: Some(Hash::from_bytes([0x73; 32])), op: op(12), daa: 6_100 }
    }
    /// The statement `st` stated on the shard (its bytes `said`), landed at 6,510.
    fn stated(&self, shard: &OLive<Shard>, st: &Stated, said: &[u8]) -> OLive<Stated> {
        let who = if st.claimer == xk(&self.c) { &self.c } else { &self.w };
        let f = [fund(xk(who), 30)];
        let s = &shard.s;
        let b = state_raw(&self.lib, shard, &s.fixed_hash(), &s.moving(), st, said, s.form(), &self.script("KpfTChange"), self.payer(who, &f)).unwrap();
        let sg = b.sign(&self.keys).expect("stated");
        assert!(kpf_protocol::verify::verify(&sg.tx, &sg.entries).is_ok(), "the statement is stated");
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
    // (In the order of their keys: the rows a test puts at the places 0, 1, 2 are the first rows of a trace. Docs 17.8d.)
    let mut markets: Vec<CMarket> = (0..4u8).map(|i| CMarket::testnet(rid.as_bytes(), [0xc0 + i; 32])).collect();
    markets.sort_by_key(|m| kpf_protocol::books::Key::futures(m.id()).numbers());
    let funds = vec![fund(xk(&f), 200)];
    let (g, lib) = lib_genesis(1, 10 * KAS1 as u64, Payer { funds: &funds, change_to: xk(&f), rate: MIN_FEE_RATE }).unwrap();
    let gs = g.sign(&keys).expect("genesis");
    World { c, w, f, keys, markets, lib, scripts: lib_scripts(&gs.tx, 6_000) }
}

fn fleg(w: &World, i: usize, units: i64, dir: i64, entry: i64) -> XLeg {
    let m = &w.markets[i];
    XLeg { market: m.id(), committee: m.committee_id, scale: m.p.scale, price_max: m.p.price_max, size: units * m.p.scale, dir, entry }
}
fn entered(legs: &[XLeg], t: i64) -> Vec<XLeg> {
    legs.iter().map(|l| if l.dir == kpf_protocol::covered::DIR_SPOT { *l } else { kpf_protocol::covered::leg_entered(l, t) }).collect()
}
fn cov(w: &World, shard: &OLive<Shard>, legs: &[XLeg], coins: i64) -> OLive<Account> {
    let s = Account { t: w.terms(), legs: legs.to_vec(), a_key: xk(&w.w), b_key: xk(&w.c), a_margin: coins, b_margin: 0, expiry: 0, dispute_daa: 50, bond_bps: 1_000, a_min_counter: legs.len() as i64, b_min_counter: 0, shard: Some(shard.s.sid) };
    OLive { s, value: coins as u64, id: shard.id, op: op(80), daa: 6_100 }
}
#[allow(dead_code)]
fn out_of(sg: &OSigned, spk: &kaspa_consensus_core::tx::ScriptPublicKey) -> (usize, u64) {
    let k = sg.tx.outputs.iter().position(|o| &o.script_public_key == spk).expect("the output");
    (k, sg.tx.outputs[k].value)
}
/// Whether the transaction pays exactly `value` to the key.
fn pays(sg: &OSigned, key: &[u8; 32], value: i64) -> bool {
    sg.tx.outputs.iter().any(|o| o.script_public_key == p2pk_spk(key) && o.value == value as u64)
}
/// All a signed transaction pays to a key.
fn paid_to(sg: &OSigned, key: &[u8; 32]) -> i64 {
    sg.tx.outputs.iter().filter(|o| o.script_public_key == p2pk_spk(key)).map(|o| o.value as i64).sum()
}

const SEED: [u8; 32] = [0x5e; 32];

/// A shard of 100 KAS with no leg, whose id is what an entry holds it to (as `thin_draw.rs`'s `seeded`).
fn seeded(w: &World) -> OLive<Shard> {
    let x = w.bare();
    let sid = kpf_protocol::reserve::shard_sid(&SEED, &w.markets[0].committee_id, &x.s.terms(), &x.s.moving()[93..120]);
    OLive { s: Shard { sid, ..x.s }, ..x }
}

/// The shard called and its call, as objects (what the settlement of a call writes).
#[allow(dead_code)]
fn called_objs(shard: &OLive<Shard>, prices: [i64; 4], daa: u64) -> (OLive<Shard>, OLive<Called>) {
    let fee = shard.s.t.keeper_fee_min.min(shard.s.held);
    let (next, call) = shard.s.called_at(&prices, T0, fee);
    (OLive { s: next, value: shard.value - fee as u64 - CALL_HOLD as u64, id: shard.id, op: op(90), daa }, OLive { s: call, value: CALL_HOLD as u64, id: shard.id, op: op(91), daa })
}

// ---------------------------------------------------------------- the reviewer's own helpers

fn args_of(b: &mut OBuilt, i: usize) -> &mut Vec<Arg> {
    let OPlan::Contract { args, .. } = &mut b.plans[i] else { panic!("input {i} is not a contract input") };
    args
}
/// A built transaction with the argument `i` of its script (input 1) changed.
fn with_arg(b: &OBuilt, i: usize, a: Arg) -> OBuilt {
    let mut t = b.clone();
    args_of(&mut t, 1)[i] = a;
    t
}
fn bytes_arg(b: &[u8]) -> Arg {
    Arg::V(ArtifactValue::Bytes(b.to_vec()))
}
fn int_arg(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}
/// Input `i` of a built transaction made the object whose state hashes to `h`.
fn object_made(b: &mut OBuilt, lib: &[u8; 32], i: usize, h: &[u8; 32]) {
    b.entries[i].script_public_key = hash_spk(lib, h);
    let OPlan::Contract { redeem, .. } = &mut b.plans[i] else { panic!("input {i} is not an object") };
    *redeem = kpf_protocol::thin_artifacts::kit().object(lib, h);
}
fn p33b(h: &[u8; 32]) -> Vec<u8> {
    [&[0x20u8][..], &h[..]].concat()
}
fn i9b(v: i64) -> Vec<u8> {
    [&[0x08u8][..], &sm8(v)[..]].concat()
}
/// The 36 bytes beside the rows: the sum of the gross, and the claims' three numbers.
fn head_of(gross: i64, c: &Claims) -> Vec<u8> {
    [i9b(gross), c.bytes(), p33b(&Shard::root_opened())].concat()
}
/// The state's hash of the shard `s` with the 36 bytes `head` where its books' own would be, written out as the
/// scripts hash it (`Shard::hash` is asserted against it in B4).
fn shard_hash_with(s: &Shard, head: &[u8]) -> [u8; 32] {
    // (The root of the dated rows' tree, its push, after the claims' numbers: docs 17.8j, 1.)
    let books = blake2b(&[&head[..9], &p33b(&blake2b(&s.rows_bytes()))[..], &head[36..], &p33b(&blake2b(&head[9..36]))[..]].concat());
    blake2b(&[p33b(&blake2b(&[s.terms(), p33b(&books)].concat())), s.moving()].concat())
}
/// The note of the rest in the form of before this change: five fields.
fn old_rest_hash(sid: &[u8; 32], key: &[u8; 32], amount: i64) -> [u8; 32] {
    blake2b(&[i9b(12), p33b(sid), i9b(7), p33b(key), i9b(amount)].concat())
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// A number from `lo` to `hi`, both of them.
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next() % (hi as i128 - lo as i128 + 1) as u64) as i64
    }
}

/// A called shard of no row, made by hand: what it holds, C1, D1, C2, D2.
fn shard_of(base: &OLive<Shard>, h: i64, c1: i64, d1: i64, c2: i64, d2: i64) -> OLive<Shard> {
    let s = Shard { held: h, owed: c1, called: CALLED, claims: Claims { paid1: d1, owed2: c2, paid2: d2 }, ..base.s };
    OLive { s, value: (h + SHARD_CARRIER) as u64, id: base.id, op: op(13), daa: 6_100 }
}
/// A claim on it, as an object that landed long before the time of the claims.
fn claim_of(base: &OLive<Shard>, key: [u8; 32], a1: i64, x1: i64, a2: i64, x2: i64) -> OLive<Claim> {
    OLive { s: Claim { sid: base.s.sid, key, amount1: a1, paid1: x1, amount2: a2, paid2: x2 }, value: NOTE_MIN as u64, id: base.id, op: op(71), daa: 6_100 }
}
/// The call of the shards made by hand.
fn call_of(base: &OLive<Shard>) -> OLive<Called> {
    OLive { s: Called { sid: base.s.sid, time: CALLED, heads: [RowHead::EMPTY; 4].into(), prices: [0; 4].into() }, value: CALL_HOLD as u64, id: base.id, op: op(91), daa: 6_450 }
}
/// What a payment of `p1` to the first class and `p2` to the second writes: the shard after, what is paid, and the
/// claim as it goes on (none where it is whole by them).
fn outcome(s: &Shard, c: &Claim, p1: i64, p2: i64) -> (Shard, i64, Option<Claim>) {
    let mut next = *s;
    next.held = s.held - p1 - p2;
    next.claims = Claims { paid1: s.claims.paid1 + p1, owed2: s.claims.owed2, paid2: s.claims.paid2 + p2 };
    let whole = c.paid1 + p1 == c.amount1 && c.paid2 + p2 == c.amount2;
    (next, p1 + p2, (!whole).then_some(Claim { paid1: c.paid1 + p1, paid2: c.paid2 + p2, ..*c }))
}
/// The transaction of a claim's payment with the outcome given, a coin that landed at `landed` showing the time.
/// (Second round: every payment has an output to the key now, and the builder writes no transaction whose output
/// is too small for a block (below about 0.034 KAS beside a claim written again: its storage mass). The script is
/// asked all the same: such an outcome is written as the builder writes one that pays a note's least, with the
/// shard's output and the key's then set to what the outcome says (a sompi to the key where it says nothing): what
/// whoever tries it would send. The harness's engine does not weigh a transaction.)
fn pay_tx(w: &World, shard: &OLive<Shard>, claim: &OLive<Claim>, o: &(Shard, i64, Option<Claim>), landed: i64) -> kpf_protocol::Result<OBuilt> {
    let f = [coin(xk(&w.f), 36, landed)];
    let built = claim_pay_as_c(&w.lib, shard, claim, o.0, o.1, o.2, &w.script("KpfTCallPay"), w.payer(&w.f, &f));
    if built.is_ok() || o.1 < 0 || o.1 >= NOTE_MIN || (shard.value as i64) < NOTE_MIN {
        return built;
    }
    let mut t = claim_pay_as_c(&w.lib, shard, claim, o.0, NOTE_MIN, o.2, &w.script("KpfTCallPay"), w.payer(&w.f, &f))?;
    let m = t.tx.outputs.len() - 1;
    t.tx.outputs[0].value = shard.value - o.1 as u64;
    t.tx.outputs[m].value = (o.1 as u64 + if o.2.is_none() { claim.value } else { 0 }).max(1);
    Ok(t)
}

// `KpfTCallPay.claim`'s arithmetic, written by the reviewer from the script's text, a line at a time.

fn unit_of(c: i64) -> i64 {
    let mut u = 1;
    if c >= 3_000_000_000 {
        u = 1_000;
    }
    if c >= 3_000_000_000_000 {
        u = 1_000_000;
    }
    if c >= 3_000_000_000_000_000 {
        u = 1_000_000_000;
    }
    if c >= 3_000_000_000_000_000_000 {
        u = 1_000_000_000_000;
    }
    u
}
/// `(a / o) * h + ((a % o) * h) / o`: nothing where a number does not fit a script's, or `o` is nothing.
fn part_of(a: i64, over: i64, of: i64) -> Option<i64> {
    let u = unit_of(of);
    let h = over / u;
    let o = of.checked_sub(1)? / u + 1;
    if o == 0 {
        return None;
    }
    (a / o).checked_mul(h)?.checked_add((a % o).checked_mul(h)? / o)
}
/// The two payments as the script computes them, with no `require` but the arithmetic's own.
fn parts(s: &Shard, c: &Claim) -> Option<(i64, i64)> {
    let (h, c1, d1, c2, d2) = (s.held, s.owed, s.claims.paid1, s.claims.owed2, s.claims.paid2);
    let w = h.checked_add(d1)?.checked_add(d2)?;
    let mut e1 = c.amount1;
    if w < c1 {
        e1 = part_of(c.amount1, w, c1)?;
    }
    let mut p1 = e1.checked_sub(c.paid1)?;
    if p1 > h {
        p1 = h;
    }
    if p1 > c1.checked_sub(d1)? {
        p1 = c1 - d1;
    }
    if p1 < 0 {
        p1 = 0;
    }
    let sp = w.checked_sub(c1)?;
    let mut e2 = 0;
    if sp > 0 {
        e2 = c.amount2;
        if sp < c2 {
            e2 = part_of(c.amount2, sp, c2)?;
        }
    }
    let mut p2 = e2.checked_sub(c.paid2)?;
    if p2 > sp.checked_sub(d2)? {
        p2 = sp - d2;
    }
    if p2 > c2.checked_sub(d2)? {
        p2 = c2 - d2;
    }
    if p2 < 0 {
        p2 = 0;
    }
    Some((p1, p2))
}
/// With every `require` of the entry: nothing where it refuses.
fn script_claim(s: &Shard, c: &Claim, landed: i64) -> Option<(i64, i64)> {
    if s.called == 0 || s.rows != [ShardRow::EMPTY; 4] || c.sid != s.sid {
        return None;
    }
    if landed < s.called.checked_add(s.t.late_daa)?.checked_add(s.dispute_daa.checked_mul(3)?)? {
        return None;
    }
    if c.paid1 < 0 || c.paid1 > c.amount1 || c.amount1 > s.owed || c.paid2 < 0 || c.paid2 > c.amount2 || c.amount2 > s.claims.owed2 || c.amount1.checked_add(c.amount2)? <= 0 || s.held < 0 {
        return None;
    }
    let (p1, p2) = parts(s, c)?;
    let whole = c.paid1 + p1 == c.amount1 && c.paid2 + p2 == c.amount2;
    (p1 + p2 >= NOTE_MIN || whole).then_some((p1, p2))
}
/// The exact share, in 128 bits: `a` times the smaller of `over` and `of`, over `of`, rounded down.
fn exact(a: i64, over: i64, of: i64) -> i64 {
    if of <= 0 || over <= 0 {
        return if of <= 0 && over >= 0 { a } else { 0 };
    }
    (a as i128 * over.min(of) as i128 / of as i128) as i64
}

/// A size as the tests draw them: small, around a note's least, around the edges of the units, or a number of KAS.
fn size(r: &mut Rng, top: bool) -> i64 {
    match r.range(0, 9) {
        0 => r.range(1, 1_000),
        1 => NOTE_MIN + r.range(-3, 3),
        2 => r.range(1, 3 * NOTE_MIN),
        3 | 4 => 3_000_000_000i64 * 1_000i64.pow(r.range(0, if top { 3 } else { 2 }) as u32) + r.range(-2, 2),
        5 => 3_000_000_000i64 * 1_000i64.pow(r.range(0, 2) as u32) / 3 * r.range(1, 7) + r.range(0, 999),
        _ => r.range(1, 5_000) * KAS1 + r.range(0, KAS1 - 1),
    }
}

/// A random called shard of the numbers (H, C1, D1, C2, D2), with D1 no more than C1, D2 no more than C2 and no more
/// than what was ever held beyond C1; and what it was drawn as.
fn random_numbers(r: &mut Rng) -> ((i64, i64, i64, i64, i64), &'static str) {
    let c1 = if r.range(0, 11) == 0 { 0 } else { size(r, true) };
    let c2 = if c1 > 1_000_000_000_000_000_000 { r.range(0, 1_000_000_000) } else if r.range(0, 2) == 0 { 0 } else { size(r, false) };
    let u = unit_of(c1);
    let (w, how): (i64, &'static str) = match r.range(0, 11) {
        0 => (-1, "nothing held"),
        1 => (c1 - 1, "a sompi below what the first is owed"),
        2 => (c1, "what the first is owed"),
        3 => (c1 + 1, "a sompi beyond what the first is owed"),
        4 => (c1 + c2 - 1, "a sompi below all that is owed"),
        5 => (c1 + c2, "all that is owed"),
        6 => (c1 / u * u - 1, "a sompi below a whole number of units"),
        7 => ((c1 as i128 * r.range(1, 999) as i128 / 1_000) as i64, "a part of what the first is owed"),
        8 => (c1 + (c2 as i128 * r.range(1, 999) as i128 / 1_000) as i64, "a part of what the second is owed"),
        9 => (c1 + c2 + r.range(0, 50 * KAS1), "more than is owed"),
        _ => (r.range(0, c1 + c2 + KAS1), "anything"),
    };
    if w < 0 || how == "nothing held" {
        // Nothing held: all it ever held was paid out. Either the first class alone was paid, or the first whole
        // and the second in part.
        if r.range(0, 1) == 0 || c2 == 0 {
            return ((0, c1, r.range(0, c1), c2, 0), "nothing held");
        }
        return ((0, c1, c1, c2, r.range(0, c2)), "nothing held");
    }
    let d1 = match r.range(0, 3) {
        0 => 0,
        1 => c1.min(w),
        _ => r.range(0, c1.min(w)),
    };
    let most2 = c2.min((w - c1).max(0)).min(w - d1);
    let d2 = match r.range(0, 3) {
        0 => 0,
        1 => most2,
        _ => r.range(0, most2),
    };
    ((w - d1 - d2, c1, d1, c2, d2), how)
}

/// A random claim on a shard of those numbers, within its bounds: its amounts no more than its classes are owed,
/// what it was paid no more than its amounts nor than what its class was paid.
fn random_claim(r: &mut Rng, n: (i64, i64, i64, i64, i64)) -> (i64, i64, i64, i64) {
    let (_, c1, d1, c2, d2) = n;
    let pick = |r: &mut Rng, c: i64| -> i64 {
        match r.range(0, 6) {
            0 => 0,
            1 => c,
            2 => 1.min(c),
            3 => NOTE_MIN.min(c),
            4 => (NOTE_MIN - 1).min(c),
            _ => r.range(0, c),
        }
    };
    let (a1, a2) = (pick(r, c1), pick(r, c2));
    let paid = |r: &mut Rng, a: i64, d: i64| -> i64 {
        match r.range(0, 3) {
            0 | 1 => 0,
            2 => a.min(d),
            _ => r.range(0, a.min(d)),
        }
    };
    (a1, paid(r, a1, d1), a2, paid(r, a2, d2))
}

// ================================================================================================================
// B1. The arithmetic at the engine, on random called shards and claims made by hand.

/// B1, the model against the script's text. 300,000 random shards and claims within their bounds, and 60,000 with
/// one bound broken (a claim of more than its class is owed, paid more than it asks, of nothing, below nothing):
/// `Shard::claim_paid_at` is the reviewer's own writing-out of `KpfTCallPay.claim`, a line at a time, in every
/// case; no part is above its exact share (128 bits); nothing is paid beyond what is held; D1 stays within C1,
/// D2 within C2 and within what was ever held beyond C1.
#[test]
fn t6_b1_the_model_is_the_scripts_arithmetic_on_random_shards_and_claims() {
    let w = world();
    let base = seeded(&w);
    let mut r = Rng(0x7_6c1a_1a55);
    let due = shard_of(&base, 0, 0, 0, 0, 0).s.claims_time().unwrap();
    assert_eq!(due, CALLED + w.terms().late_daa + 3 * 50);
    let (mut paid, mut waits, mut whole, mut broken, mut nothing) = (0, 0, 0, 0, 0);
    let mut hows = std::collections::BTreeMap::new();
    let (mut worst1, mut worst2) = (0f64, 0f64);
    for case in 0..360_000 {
        let (n, how) = random_numbers(&mut r);
        let (h, c1, d1, c2, d2) = n;
        assert!(h >= 0 && d1 >= 0 && d1 <= c1 && d2 >= 0 && d2 <= c2 && d2 <= (h + d1 + d2 - c1).max(0), "case {case}: {n:?}");
        let (mut a1, mut x1, mut a2, mut x2) = random_claim(&mut r, n);
        if case % 6 == 5 {
            match r.range(0, 6) {
                0 => a1 = c1 + 1 + r.range(0, 3),
                1 => x1 = a1 + 1,
                2 => a2 = c2 + 1 + r.range(0, 3),
                3 => x2 = a2 + 1,
                4 => (a1, x1, a2, x2) = (0, 0, 0, 0),
                5 => x1 = -1,
                _ => x2 = -1,
            }
            broken += 1;
        }
        let s = shard_of(&base, h, c1, d1, c2, d2).s;
        let c = claim_of(&base, xk(&w.w), a1, x1, a2, x2).s;
        let model = s.claim_paid_at(&c, due).ok();
        let script = script_claim(&s, &c, due);
        assert_eq!(model.map(|x| (x.0, x.1, x.2)), script.map(|(p1, p2)| outcome(&s, &c, p1, p2)), "case {case} ({how}): the shard {n:?}, the claim {:?}", (a1, x1, a2, x2));
        if case % 6 == 5 {
            assert!(model.is_none(), "case {case}: a claim out of its bounds is paid: {n:?} {:?}", (a1, x1, a2, x2));
            continue;
        }
        let wv = h + d1 + d2;
        match script {
            Some((p1, p2)) => {
                let next = model.unwrap().0;
                // (i) nothing beyond what is held; (ii) within what each class is owed, the second never into what
                // the first is still owed; (iii) no claim more than its amounts.
                assert!(p1 >= 0 && p2 >= 0 && p1 + p2 <= h && next.held >= 0, "case {case}");
                assert!(next.claims.paid1 <= c1 && next.claims.paid2 <= c2 && next.claims.paid2 <= (wv - c1).max(0), "case {case}: {n:?}");
                    assert!((p2 == 0 || next.held >= c1 - next.claims.paid1) && x1 + p1 <= a1 && x2 + p2 <= a2, "case {case}: the second class is paid what the first is still owed");
                // No more than its exact share of what was ever held, of each class.
                assert!(p1 == 0 || x1 + p1 <= exact(a1, wv, c1), "case {case}: the first part {} above its exact share {}", x1 + p1, exact(a1, wv, c1));
                assert!(p2 == 0 || x2 + p2 <= exact(a2, wv - c1, c2), "case {case}: the second part above its exact share");
                // (How far below, where nothing but rounding cut it: at least half held.)
                if wv < c1 && wv >= c1 / 2 && x1 == 0 && d1 == 0 && a1 >= KAS1 {
                    worst1 = worst1.max((exact(a1, wv, c1) - p1) as f64 / exact(a1, wv, c1) as f64);
                }
                if wv > c1 && wv - c1 < c2 && wv - c1 >= c2 / 2 && x2 == 0 && d2 == 0 && a2 >= KAS1 && d1 == c1 {
                    worst2 = worst2.max((exact(a2, wv - c1, c2) - p2) as f64 / exact(a2, wv - c1, c2) as f64);
                }
                paid += 1;
                if x1 + p1 == a1 && x2 + p2 == a2 {
                    whole += 1;
                }
                // (iv) What is covered is whole: the first class where all ever held is what it is owed; both where
                // it is all that is owed. (A claim paid no more than its class was: all counted.)
                if wv >= c1 && x1 <= d1 && a1 - x1 <= c1 - d1 {
                    assert_eq!(x1 + p1, a1, "case {case}: the first class is covered and the claim's first part is not whole: {n:?} {:?}", (a1, x1, a2, x2));
                }
            }
            None if a1 + a2 == 0 => nothing += 1,
            None => {
                // It waits: its two parts together are below a note's least and do not make it whole.
                let (p1, p2) = parts(&s, &c).expect("the arithmetic fits");
                assert!(p1 + p2 < NOTE_MIN && !(x1 + p1 == a1 && x2 + p2 == a2), "case {case} ({how}): refused, and not for a part below the least: {n:?} {:?}", (a1, x1, a2, x2));
                waits += 1;
            }
        }
        *hows.entry(how).or_insert(0) += 1;
    }
    println!("T6 B1 (model): 360,000 random called shards and claims: {paid} paid as the script's text computes ({whole} whole), {waits} waiting (a part below a note's least), {nothing} of nothing and {broken} out of their bounds, refused; the largest shortfall against the exact share where at least half is held: {worst1:.3e} (first class), {worst2:.3e} (second); the shards drawn: {hows:?}");
    assert!(paid > 100_000 && waits > 10_000 && whole > 30_000 && worst1 < 2e-6 && worst2 < 2e-6 && hows.len() == 11);
}

/// What can be written of a payment the rule does not make, from the outcome `(p1, p2)`: a sompi more or less to
/// either class, a sompi moved from one to the other, the claim spent where it goes on and written again where it
/// is whole, written again as it was, to another key, asking a sompi more, its two payments the other way round; the shard with what is owed fallen (the form of before), what the second
/// class is owed fallen, nothing said of what was paid, no poorer, not called.
fn lies_of(s: &Shard, c: &Claim, p1: i64, p2: i64) -> Vec<(&'static str, (Shard, i64, Option<Claim>))> {
    let right = outcome(s, c, p1, p2);
    let mut v = vec![];
    for (what, q1, q2) in [("a sompi more to the first class", p1 + 1, p2), ("a sompi more to the second class", p1, p2 + 1), ("a sompi less to the first class", p1 - 1, p2), ("a sompi less to the second class", p1, p2 - 1), ("a sompi moved to the second class", p1 - 1, p2 + 1), ("a sompi moved to the first class", p1 + 1, p2 - 1)] {
        if q1 < 0 || q2 < 0 || q1 + q2 > s.held {
            continue;
        }
        let o = outcome(s, c, q1, q2);
        v.push((what, o));
        // (And with the claim spent, or written again, the other way.)
        let again = Claim { paid1: c.paid1 + q1, paid2: c.paid2 + q2, ..*c };
        v.push((what, (o.0, o.1, if o.2.is_some() { None } else { Some(again) })));
    }
    match right.2 {
        Some(l) => {
            v.push(("the claim again to another key", (right.0, right.1, Some(Claim { key: [7; 32], ..l }))));
            v.push(("the claim again asking a sompi more of the first class", (right.0, right.1, Some(Claim { amount1: l.amount1 + 1, ..l }))));
            v.push(("the claim again asking a sompi more of the second class", (right.0, right.1, Some(Claim { amount2: l.amount2 + 1, ..l }))));
            if l.paid1 != l.paid2 {
                v.push(("the claim again, what it was paid of the first class said as of the second", (right.0, right.1, Some(Claim { paid1: l.paid2, paid2: l.paid1, ..l }))));
            }
            v.push(("the claim spent while it is not whole", (right.0, right.1, None)));
            v.push(("the claim again as it was", (right.0, right.1, Some(*c))));
            v.push(("the claim again as if whole", (right.0, right.1, Some(Claim { paid1: c.amount1, paid2: c.amount2, ..*c }))));
        }
        None => {
            v.push(("the claim written again though it is whole", (right.0, right.1, Some(Claim { paid1: c.amount1, paid2: c.amount2, ..*c }))));
            if p1 + p2 > 0 {
                v.push(("the claim written again as it was though it is whole", (right.0, right.1, Some(*c))));
            }
        }
    }
    let mut x = right.0;
    if p1 > 0 {
        x.owed = s.owed - p1;
        v.push(("what is owed falling by what was paid", (x, right.1, right.2)));
        x.claims.paid1 = s.claims.paid1;
        v.push(("what is owed falling, nothing said of what was paid (the form of before)", (x, right.1, right.2)));
        x = right.0;
        x.claims.paid1 = s.claims.paid1;
        v.push(("nothing said of what the first class was paid", (x, right.1, right.2)));
        x = right.0;
        x.owed = s.owed - c.amount1;
        v.push(("what is owed falling by the claim's amount (the form of before)", (x, right.1, right.2)));
    }
    if p2 > 0 {
        x = right.0;
        x.claims.owed2 = s.claims.owed2 - p2;
        v.push(("what the second class is owed falling", (x, right.1, right.2)));
        x = right.0;
        x.claims.paid2 = s.claims.paid2;
        v.push(("nothing said of what the second class was paid", (x, right.1, right.2)));
        x = right.0;
        x.claims.paid1 = s.claims.paid1 + p1 + p2;
        x.claims.paid2 = s.claims.paid2;
        v.push(("the second class's payment said as the first's", (x, right.1, right.2)));
    }
    if p1 + p2 > 0 {
        x = right.0;
        x.held = s.held;
        v.push(("the shard no poorer", (x, right.1, right.2)));
    }
    x = right.0;
    x.called = 0;
    v.push(("the shard not called", (x, right.1, right.2)));
    v
}

/// B1 at the engine. 1,200 random called shards and claims, made by hand (no script writes a state of the second
/// class yet), sizes around the edges of the units (3,000,000,000 x 1000^k sompi, k to 3), a note's least, a sompi;
/// nothing held; all ever held a sompi below, at, and a sompi beyond what the first class is owed. For each:
///  - where the model pays: the transaction of the model's outcome is taken, with a coin that landed at the time
///    of the claims, and refused with that coin a block earlier; and every lie of `lies_of` is refused; (second
///    round, K1) its last output is the key's, of the part, and of what the claim's note held with it where the
///    claim is whole; whoever pays has only its fees less; every lie of `output_lies` is refused;
///  - where the model refuses (a part below a note's least that does not make the claim whole; a claim beyond what
///    its class is owed; paid more than it asks; of nothing): no outcome that can be written is taken: the parts
///    the script's arithmetic gives, with the claim again or spent; nothing paid, the claim again or spent; all it
///    asks.
#[test]
fn t6_b1_at_the_engine_the_models_outcome_is_taken_and_no_other() {
    let w = world();
    let base = seeded(&w);
    let mut r = Rng(0x243f_6a88_85a3_08d3);
    let due = shard_of(&base, 0, 0, 0, 0, 0).s.claims_time().unwrap();
    let (mut ok, mut lies, mut early, mut waiting, mut out_of_bounds, mut refused_outcomes, mut unwritten) = (0, 0, 0, 0, 0, 0, 0);
    let (mut both, mut second_only, mut whole, mut again, mut out_lies) = (0, 0, 0, 0, 0);
    let mut hows = std::collections::BTreeMap::new();
    let mut why_not: std::collections::BTreeMap<String, i32> = std::collections::BTreeMap::new();
    for case in 0..1_200 {
        let (n, how) = random_numbers(&mut r);
        let (h, c1, d1, c2, d2) = n;
        let (mut a1, mut x1, mut a2, mut x2) = random_claim(&mut r, n);
        let breaks = case % 7 == 6;
        if breaks {
            match r.range(0, 4) {
                0 => a1 = c1 + 1,
                1 => x1 = a1 + 1,
                2 => a2 = c2 + 1,
                3 => x2 = a2 + 1,
                _ => (a1, x1, a2, x2) = (0, 0, 0, 0),
            }
        }
        let shard = shard_of(&base, h, c1, d1, c2, d2);
        let claim = claim_of(&base, xk(&w.w), a1, x1, a2, x2);
        let (s, c) = (shard.s, claim.s);
        let what = format!("case {case} ({how}): the shard (H, C1, D1, C2, D2) = {n:?}, the claim (A1, a1, A2, a2) = {:?}", (a1, x1, a2, x2));
        let model = s.claim_paid_at(&c, due).ok();
        assert_eq!(model.map(|x| (x.0, x.1, x.2)), script_claim(&s, &c, due).map(|(p1, p2)| outcome(&s, &c, p1, p2)), "{what}");
        match model {
            Some(o) => {
                let (p1, p2) = (o.0.claims.paid1 - d1, o.0.claims.paid2 - d2);
                let b = pay_tx(&w, &shard, &claim, &o, due).unwrap_or_else(|e| panic!("{what}: {e}"));
                assert!(accepted(&b, &w.keys), "{what}: the model's outcome ({p1}, {p2}) is not taken");
                ok += 1;
                if !taken(pay_tx(&w, &shard, &claim, &o, due - 1), &w.keys) {
                    early += 1;
                } else {
                    panic!("{what}: paid a block before the time of the claims");
                }
                // (Second round, K1: there is always an output to the claim's key, the last: of the part where the
                // claim goes on, of the part and what the claim's note held where it is whole; the claim goes on as
                // an output of its own, holding what it held, where it is not whole. Whoever pays has its fee less
                // and what the script's copy keeps, and nothing else. Every other way of writing the outputs is
                // refused.)
                let sg = b.sign(&w.keys).unwrap();
                let m = sg.tx.outputs.len() - 1;
                let due = o.1 + if o.2.is_none() { claim.value as i64 } else { 0 };
                assert_eq!((sg.tx.outputs[m].value as i64, &sg.tx.outputs[m].script_public_key, paid_to(&sg, &xk(&w.w))), (due, &p2pk_spk(&xk(&w.w)), due), "{what}");
                assert_eq!(1_000 * KAS1 - paid_to(&sg, &xk(&w.f)), sg.fee as i64 + sg.tx.outputs[1].value as i64 - sg.entries[1].amount as i64, "{what}: whoever pays has something of it");
                for (lie, t) in output_lies(&b, &xk(&w.f), o.1, o.2.is_none(), claim.value) {
                    assert!(!accepted(&t, &w.keys), "{what}: taken: {lie}");
                    out_lies += 1;
                }
                assert_eq!(o.2.is_some(), o.2.is_some_and(|l| sg.tx.outputs.iter().any(|x| x.script_public_key == hash_spk(&w.lib, &l.hash()) && x.value == claim.value)), "{what}");
                for (lie, x) in lies_of(&s, &c, p1, p2) {
                    match pay_tx(&w, &shard, &claim, &x, due) {
                        Ok(t) => assert!(!accepted(&t, &w.keys), "{what}: taken: {lie}"),
                        Err(e) => {
                            *why_not.entry(e.to_string()).or_insert(0) += 1;
                            unwritten += 1;
                        }
                    }
                    lies += 1;
                }
                if p1 > 0 && p2 > 0 {
                    both += 1;
                }
                if p1 == 0 && p2 > 0 {
                    second_only += 1;
                }
                if o.2.is_none() { whole += 1 } else { again += 1 }
            }
            None => {
                if breaks || a1 + a2 == 0 { out_of_bounds += 1 } else { waiting += 1 }
                let mut tries: Vec<(Shard, i64, Option<Claim>)> = vec![(s, 0, Some(c)), (s, 0, None)];
                if let Some((p1, p2)) = parts(&s, &c) {
                    let o = outcome(&s, &c, p1, p2);
                    tries.push(o);
                    tries.push((o.0, o.1, if o.2.is_some() { None } else { Some(Claim { paid1: c.paid1 + p1, paid2: c.paid2 + p2, ..c }) }));
                }
                // (All it still asks of the first class, as far as the shard holds; and of both.)
                let rest1 = (a1 - x1).max(0).min(h);
                let rest2 = (a2 - x2).max(0).min(h - rest1);
                for (q1, q2) in [(rest1, 0), (rest1, rest2), (NOTE_MIN.min(h), 0)] {
                    let o = outcome(&s, &c, q1, q2);
                    tries.push(o);
                    tries.push((o.0, o.1, if o.2.is_some() { None } else { Some(Claim { paid1: c.paid1 + q1, paid2: c.paid2 + q2, ..c }) }));
                }
                for x in tries {
                    match pay_tx(&w, &shard, &claim, &x, due) {
                        Ok(t) => assert!(!accepted(&t, &w.keys), "{what}: the model refuses and an outcome is taken: paid {}, the claim {:?}", x.1, x.2),
                        Err(e) => {
                            *why_not.entry(e.to_string()).or_insert(0) += 1;
                            unwritten += 1;
                        }
                    }
                    refused_outcomes += 1;
                }
            }
        }
        *hows.entry(how).or_insert(0) += 1;
    }
    println!("T6 B1 (engine): 1,200 random called shards and claims made by hand: {ok} of the model's outcomes taken ({whole} whole and spent, {again} written again; {both} paying both classes at once, {second_only} the second alone), each refused with its coin a block before the time ({early}); {lies} lies refused ({unwritten} of all the transactions asked for were not written by the builder: more paid than the output holds), and {out_lies} transactions with the outputs written otherwise (the key's a sompi short, to whoever pays, not the last, the part or the note's value alone, the claim again holding less); the model refuses {waiting} for a part below a note's least and {out_of_bounds} out of their bounds or of nothing: {refused_outcomes} outcomes written for them, none taken; the shards drawn: {hows:?}");
    println!("T6 B1 (engine): what the builder did not write, and why: {why_not:?}");
    assert!(out_lies >= 4 * ok && ok >= 400 && lies >= 5_000 && waiting >= 300 && out_of_bounds >= 150 && both >= 40 && second_only >= 30 && whole >= 150 && again >= 150 && hows.len() == 11);
}

// ================================================================================================================
// B2. Sequences: what holds over the life of a called shard.

fn live_shard(base: &OLive<Shard>, s: Shard) -> OLive<Shard> {
    OLive { s, value: (s.held + SHARD_CARRIER) as u64, id: base.id, op: op(13), daa: 6_100 }
}
fn live_claim(base: &OLive<Shard>, c: Claim) -> OLive<Claim> {
    OLive { s: c, value: NOTE_MIN as u64, id: base.id, op: op(71), daa: 6_100 }
}
/// What a claim of the amounts (A1, A2) has been paid in all once it was paid where all the shard ever held is `w`:
/// its two parts, as the script's arithmetic gives them with nothing cut by what is held.
fn parts_at(a1: i64, a2: i64, w: i64, c1: i64, c2: i64) -> (i64, i64) {
    let e1 = if w >= c1 { a1 } else { part_of(a1, w, c1).unwrap() };
    let sp = w - c1;
    let e2 = if sp <= 0 {
        0
    } else if sp >= c2 {
        a2
    } else {
        part_of(a2, sp, c2).unwrap()
    };
    (e1, e2)
}

/// B2, (i) to (v). 6,000 random called shards, each with 2 to 6 claims of both classes, every claim counted (what
/// the shard owes each class is the sum of its claims' amounts); the claims are shown in random orders, again and
/// again, and coins reach the shard at random moments between payments (a sompi, a note's least, a part of what is
/// missing, exactly what is missing). At the model, every step; at the engine, about one step in forty (the
/// model's outcome taken, a lie refused; a claim that waits paid by no outcome), and the end of one call in thirty.
///  (i)   nothing is ever paid beyond what is held;
///  (ii)  D1 is within C1, D2 within C2 and within what was ever held beyond C1 (the second class never has what the
///        first is still owed);
///  (iii) no claim is paid more than its amounts, in all;
///  (iv)  where all ever held covers a class, every claim's part of it is whole by one payment (the first class
///        alone: where the claim has nothing of the second, or the second is covered too); once all ever held is
///        C1 and C2, every claim is paid whole whatever its size and the order, and the call ends; before, it
///        does not;
///  (v)   what a claim has been paid, after any payment, is its two parts of all the shard has ever held and
///        nothing else: the order of payments, and what was paid to others, change no sompi of it; and that is no
///        more than its exact share.
#[test]
fn t6_b2_over_sequences_of_payments_and_coins_the_claims_are_paid_their_parts_and_the_call_ends_when_covered() {
    let w = world();
    let base = seeded(&w);
    let mut r = Rng(0x1319_8a2e_0370_7344);
    let due = shard_of(&base, 0, 0, 0, 0, 0).s.claims_time().unwrap();
    let call = call_of(&base);
    let (mut steps, mut payments, mut waits, mut arrivals, mut engine_ok, mut engine_lies, mut engine_waits, mut engine_offs, mut ends) = (0u64, 0u64, 0u64, 0u64, 0, 0, 0, 0, 0);
    let (mut wait_covered1, mut early_off_refused) = (0u64, 0u64);
    for seq in 0..6_000 {
        let n = r.range(2, 6) as usize;
        let kind = r.range(0, 5);
        let amount = |r: &mut Rng| -> i64 {
            match kind {
                0 => r.range(1, 3 * NOTE_MIN),
                1 => r.range(1, 500) * KAS1 + r.range(0, KAS1 - 1),
                2 => 3_000_000_000i64 * 1_000i64.pow(r.range(0, 2) as u32) / n as i64 + r.range(-2, 2),
                3 => r.range(1, 1_000),
                4 => size(r, false),
                _ => [1, NOTE_MIN - 1, NOTE_MIN, NOTE_MIN + 1, 7 * KAS1][r.range(0, 4) as usize],
            }
        };
        let mut claims: Vec<Claim> = (0..n)
            .map(|_| {
                let (a1, a2) = match r.range(0, 3) {
                    0 | 1 => (amount(&mut r), 0),
                    2 => (0, amount(&mut r)),
                    _ => (amount(&mut r), amount(&mut r)),
                };
                Claim { sid: base.s.sid, key: xk(&w.w), amount1: a1, paid1: 0, amount2: a2, paid2: 0 }
            })
            .collect();
        let (c1, c2): (i64, i64) = (claims.iter().map(|c| c.amount1).sum(), claims.iter().map(|c| c.amount2).sum());
        let h0 = match r.range(0, 7) {
            0 => 0,
            1 => (c1 - 1).max(0),
            2 => c1,
            3 => c1 + 1,
            4 => ((c1 + c2) as i128 * r.range(0, 1_000) as i128 / 1_000) as i64,
            5 => (c1 as i128 * r.range(0, 1_000) as i128 / 1_000) as i64,
            6 => (c1 / unit_of(c1) * unit_of(c1) - 1).max(0),
            _ => r.range(0, NOTE_MIN * 2),
        };
        let mut s = shard_of(&base, h0, c1, 0, c2, 0).s;
        let mut alive = vec![true; n];
        let check = |s: &Shard, claims: &[Claim], what: &str| {
            let wv = s.held + s.claims.paid1 + s.claims.paid2;
            assert!(s.held >= 0 && s.owed == c1 && s.claims.owed2 == c2, "{what}");
            assert!(s.claims.paid1 >= 0 && s.claims.paid1 <= c1 && s.claims.paid2 >= 0 && s.claims.paid2 <= c2 && s.claims.paid2 <= (wv - c1).max(0), "{what}: (ii) {:?} of {wv} ever held, {c1} and {c2} owed", s.claims);
            assert_eq!((claims.iter().map(|c| c.paid1).sum::<i64>(), claims.iter().map(|c| c.paid2).sum::<i64>()), (s.claims.paid1, s.claims.paid2), "{what}: what the claims were paid is what the shard says it paid");
            for c in claims {
                assert!(c.paid1 <= c.amount1 && c.paid2 <= c.amount2 && c.paid1 <= exact(c.amount1, wv, c1) && c.paid2 <= exact(c.amount2, wv - c1, c2), "{what}: (iii), (v) {c:?} of {wv} ever held");
            }
        };
        for step in 0..(10 * n) {
            steps += 1;
            let wv = s.held + s.claims.paid1 + s.claims.paid2;
            let what = format!("sequence {seq}, step {step}: all ever held {wv}, owed {c1} and {c2}, the shard {:?} holding {}", s.claims, s.held);
            if r.range(0, 3) == 0 && wv < c1 + c2 + 10 * KAS1 {
                // Coins reach the shard.
                let missing = (c1 + c2 - wv).max(0);
                let missing1 = (c1 - wv).max(0);
                s.held += match r.range(0, 6) {
                    0 => 1,
                    1 => NOTE_MIN,
                    2 => missing,
                    3 => missing1,
                    4 => (missing as i128 * r.range(1, 999) as i128 / 1_000) as i64,
                    5 => (missing1 - 1).max(0),
                    _ => r.range(0, missing + KAS1),
                };
                arrivals += 1;
                check(&s, &claims, &what);
                continue;
            }
            let Some(k) = (0..n).filter(|k| alive[*k]).nth(r.range(0, n as i64 - 1) as usize % alive.iter().filter(|a| **a).count().max(1)) else { break };
            let c = claims[k];
            let model = s.claim_paid_at(&c, due).ok();
            assert_eq!(model.map(|x| (x.0, x.1, x.2)), script_claim(&s, &c, due).map(|(p1, p2)| outcome(&s, &c, p1, p2)), "{what}: the claim {c:?}");
            let at_engine = steps % 40 == 0;
            match model {
                Some((next, pay, left)) => {
                    let (p1, p2) = (next.claims.paid1 - s.claims.paid1, next.claims.paid2 - s.claims.paid2);
                    // (i)
                    assert!(pay == p1 + p2 && p1 >= 0 && p2 >= 0 && pay <= s.held && next.held == s.held - pay, "{what}: (i)");
                    // (v): its two parts of all ever held, to the sompi: nothing was cut by what is held or by what
                    // its class is still owed.
                    assert_eq!((c.paid1 + p1, c.paid2 + p2), parts_at(c.amount1, c.amount2, wv, c1, c2), "{what}: (v) the claim {c:?}");
                    // (iv): what is covered is whole.
                    if wv >= c1 + c2 {
                        assert_eq!(left, None, "{what}: (iv) covered, and the claim {c:?} is not whole");
                    }
                    if at_engine {
                        let (ls, lc) = (live_shard(&base, s), live_claim(&base, c));
                        assert!(taken(pay_tx(&w, &ls, &lc, &(next, pay, left), due), &w.keys), "{what}: the model's outcome for {c:?} is not taken");
                        engine_ok += 1;
                        let lies = lies_of(&s, &c, p1, p2);
                        let (lie, x) = &lies[r.range(0, lies.len() as i64 - 1) as usize];
                        assert!(!taken(pay_tx(&w, &ls, &lc, x, due), &w.keys), "{what}: taken: {lie}");
                        engine_lies += 1;
                    }
                    s = next;
                    match left {
                        Some(l) => claims[k] = l,
                        None => {
                            claims[k] = Claim { paid1: c.amount1, paid2: c.amount2, ..c };
                            alive[k] = false;
                        }
                    }
                    payments += 1;
                }
                None => {
                    // It waits: its parts together are below a note's least and do not make it whole. Never where
                    // all that is owed is covered.
                    let (p1, p2) = parts(&s, &c).expect("the arithmetic fits");
                    assert!(p1 + p2 < NOTE_MIN && !(c.paid1 + p1 == c.amount1 && c.paid2 + p2 == c.amount2), "{what}: {c:?} refused, and not for a part below the least");
                    assert!(wv < c1 + c2, "{what}: (iv) all that is owed is covered and the claim {c:?} is not paid");
                    // (A claim of the first class alone is never left waiting where the first class is covered.)
                    assert!(!(wv >= c1 && c.amount2 == 0), "{what}: (iv) the first class is covered and {c:?}, of it alone, waits");
                    if wv >= c1 && c.amount1 > c.paid1 {
                        wait_covered1 += 1;
                    }
                    if at_engine {
                        let (ls, lc) = (live_shard(&base, s), live_claim(&base, c));
                        for x in [outcome(&s, &c, p1, p2), (outcome(&s, &c, p1, p2).0, p1 + p2, None), (s, 0, Some(c))] {
                            assert!(!taken(pay_tx(&w, &ls, &lc, &x, due), &w.keys), "{what}: {c:?} waits and is paid");
                        }
                        engine_waits += 1;
                    }
                    waits += 1;
                }
            }
            check(&s, &claims, &what);
            // The call does not end while what was paid is not what is owed.
            if s.claims.paid1 != c1 || s.claims.paid2 != c2 {
                assert!(s.call_off(&call.s).is_err(), "{what}: the call ends");
                if steps % 400 == 0 {
                    let mut open = s;
                    open.called = 0;
                    open.owed = 0;
                    open.claims = Claims::default();
                    assert!(!taken(call_off_as(&w.lib, &live_shard(&base, s), &call, open, &w.script("KpfTCallPay"), w.payer(&w.f, &[coin(xk(&w.f), 36, due)])), &w.keys), "{what}: the call ends at the engine");
                    early_off_refused += 1;
                }
            }
        }
        // The shard is paid up to exactly what it owes, where it holds less: every claim is then whole by one
        // payment, in whatever order, and the call ends.
        let wv = s.held + s.claims.paid1 + s.claims.paid2;
        s.held += (c1 + c2 - wv).max(0);
        let mut order: Vec<usize> = (0..n).filter(|k| alive[*k]).collect();
        for i in (1..order.len()).rev() {
            order.swap(i, r.range(0, i as i64) as usize);
        }
        for k in order {
            let c = claims[k];
            let (next, pay, left) = s.claim_paid_at(&c, due).unwrap_or_else(|e| panic!("sequence {seq}: (iv) covered, and {c:?} is not paid: {e}"));
            assert_eq!((left, pay), (None, c.amount1 - c.paid1 + c.amount2 - c.paid2), "sequence {seq}: (iv) covered, and {c:?} is not whole");
            if seq % 30 == 0 {
                assert!(taken(pay_tx(&w, &live_shard(&base, s), &live_claim(&base, c), &(next, pay, left), due), &w.keys), "sequence {seq}: the last payment of {c:?} is not taken");
                engine_ok += 1;
            }
            s = next;
            claims[k] = Claim { paid1: c.amount1, paid2: c.amount2, ..c };
        }
        check(&s, &claims, &format!("sequence {seq}, at its end"));
        assert_eq!((s.claims.paid1, s.claims.paid2, s.held), (c1, c2, (wv - c1 - c2).max(0)), "sequence {seq}");
        let open = s.call_off(&call.s).unwrap_or_else(|e| panic!("sequence {seq}: (iv) every claim is whole and the call does not end: {e}"));
        assert_eq!((open.called, open.owed, open.claims, open.held), (0, 0, Claims::default(), s.held));
        if seq % 30 == 0 {
            assert!(taken(call_off(&w.lib, &live_shard(&base, s), &call, &w.script("KpfTCallPay"), w.payer(&w.f, &[coin(xk(&w.f), 36, due)])).map(|x| x.0), &w.keys), "sequence {seq}: the call's end is not taken");
            engine_offs += 1;
        }
        ends += 1;
    }
    println!("T6 B2: 6,000 called shards of 2 to 6 claims of both classes: {steps} steps, {payments} payments, {waits} claims shown that wait (a part below a note's least; {wait_covered1} of them with a first part that is covered, behind their second), {arrivals} arrivals of coins; (i) to (v) hold at every step; every one of the {ends} calls ends once paid up to exactly what it owes, each claim whole by one payment. At the engine: {engine_ok} of the model's payments taken, {engine_lies} lies refused, {engine_waits} waiting claims paid by none of three outcomes, {early_off_refused} ends of a call that is not paid up refused, {engine_offs} ends taken");
    assert!(payments > 30_000 && waits > 5_000 && arrivals > 20_000 && engine_ok >= 500 && engine_lies >= 300 && engine_waits >= 100 && engine_offs == 200 && early_off_refused >= 100);
}

/// B2 (vi): a note never counted. A wrong statement nobody disputed can write any note; one of the rest of no more
/// than the shard owes its class is not told from a counted one (the model cannot tell either).
/// Two real claims of 30 and 15 KAS (C1 = 45), and a third note of 20 KAS that nothing counted. The shard holds 45.
///  - The stray shown first: it is paid 20; the claim of 30 is paid 25 (no part is more than C1 less D1) and goes
///    on as (30, 25); the claim of 15 is paid nothing, by no outcome. D1 is C1: THE CALL ENDS, and the two users
///    are 5 and 15 KAS short: what the stray took, to the sompi.
///  - After the end the shard owes nothing: the claim of 15, never paid, is a note of more than is owed:
///    `KpfTLeft.rest` spends it for nothing, by anyone (F96's rule); the claim (30, 25) is opened by no script
///    (`KpfTLeft.rest`, `KpfTDrawNote`): it stands for good, its least in it.
///  - Not ended: coins that reach the shard later pay neither (D1 is C1): the call can only end.
///  - The real claims shown first are whole, and the stray is paid nothing.
///  - Half held (22.5): the stray first has 10, the claim of 30 the 12.5 left (its part is 15), the claim of 15
///    nothing: whoever is first has its part; the last finds what the stray left.
///  - A second call of the same shard later (C1 = 40, one claim): the claim (30, 25) of the first call is paid its
///    5 KAS there, of what the second call's claim is owed.
/// Before this change (read, not run: the script of 563ef01): the stray paid 20, what is owed fell to 25, the claim
/// of 30 was then of more than is owed and paid by nothing, for good; the claim of 15 was paid whole; 10 KAS
/// stayed in a shard called for ever. The users lose the same 20 KAS; who loses them, and whether the call ends,
/// changed. Passes while a note never counted is paid as a counted one.
#[test]
fn t6_b2_vi_a_note_never_counted_takes_its_amount_from_the_real_claims_and_the_call_ends() {
    let w = world();
    let base = seeded(&w);
    let due = shard_of(&base, 0, 0, 0, 0, 0).s.claims_time().unwrap();
    let call = call_of(&base);
    let pay_s = w.script("KpfTCallPay");
    let fl = [coin(xk(&w.f), 36, due)];
    let (x, y, z) = (claim_of(&base, xk(&w.w), 30 * KAS1, 0, 0, 0), claim_of(&base, xk(&w.f), 15 * KAS1, 0, 0, 0), claim_of(&base, xk(&w.c), 20 * KAS1, 0, 0, 0));
    // The stray first.
    let s0 = shard_of(&base, 45 * KAS1, 45 * KAS1, 0, 0, 0);
    let (b, s1, pz, left) = claim_pay_c(&w.lib, &s0, &z, &pay_s, w.payer(&w.f, &fl)).unwrap();
    let sg = signed("vi: the note never counted, paid", &b, &w.keys);
    // (Second round, K1: whole, it has what its note held in the same output.)
    assert!((pz, left) == (20 * KAS1, None) && pays(&sg, &xk(&w.c), pz + z.value as i64), "K2 is closed: a note never counted is not paid as a counted one");
    let s1 = nth(&sg, 0, s1, due as u64 + 1);
    let (b, s2, px, left) = claim_pay_c(&w.lib, &s1, &x, &pay_s, w.payer(&w.f, &fl)).unwrap();
    let sg = signed("vi: the claim of 30, paid what its class is still owed", &b, &w.keys);
    assert_eq!((px, left), (25 * KAS1, Some(Claim { paid1: 25 * KAS1, ..x.s })));
    let (s2, x2) = (nth(&sg, 0, s2, due as u64 + 2), nth(&sg, 2, left.unwrap(), due as u64 + 2));
    assert_eq!((s2.s.held, s2.s.owed, s2.s.claims.paid1), (0, 45 * KAS1, 45 * KAS1));
    assert!(s2.s.claim_paid_at(&y.s, i64::MAX).is_err() && s2.s.claim_paid_at(&x2.s, i64::MAX).is_err());
    for o in [(s2.s, 0, Some(y.s)), (s2.s, 0, None)] {
        assert!(!taken(pay_tx(&w, &s2, &y, &o, due), &w.keys), "the claim of 15 is paid by an outcome of nothing");
    }
    // The call ends.
    let (b, open) = call_off(&w.lib, &s2, &call, &pay_s, w.payer(&w.f, &fl)).unwrap();
    let sg = signed("vi: the call ends, two claims short", &b, &w.keys);
    assert_eq!((open.called, open.owed, open.claims, open.held), (0, 0, Claims::default(), 0));
    let opened = nth(&sg, 0, open, due as u64 + 3);
    // After it: the claim of 15 is spent for nothing by `KpfTLeft.rest`; the claim (30, 25) is opened by nothing.
    let y_rest = OLive { s: Rest { sid: y.s.sid, key: y.s.key, amount: y.s.amount1 }, value: y.value, id: y.id, op: y.op, daa: y.daa };
    assert_eq!(y_rest.s.hash(), y.s.hash());
    let (b, after, left) = rest_apply(&w.lib, &opened, &y_rest, &w.script("KpfTRest"), w.payer(&w.c, &[fund(xk(&w.c), 38)])).unwrap();
    let sg = signed("vi: after the call's end the claim of 15 is spent for nothing", &b, &w.keys);
    assert_eq!((after, left, paid_to(&sg, &y.s.key)), (opened.s, None, 0));
    // (Paid up after the end, the shard pays it no more either: it owes nothing.)
    let rich = OLive { s: Shard { held: 50 * KAS1, ..opened.s }, value: (50 * KAS1 + SHARD_CARRIER) as u64, ..opened.clone() };
    let (b, after, _) = rest_apply(&w.lib, &rich, &y_rest, &w.script("KpfTRest"), w.payer(&w.c, &[fund(xk(&w.c), 38)])).unwrap();
    assert!(accepted(&b, &w.keys) && after == rich.s);
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Shard { held: 35 * KAS1, ..rich.s }.hash());
    t.tx.outputs[0].value -= 15 * KAS1 as u64;
    assert!(!accepted(&t, &w.keys), "a note of more than is owed is paid");
    for (what, shown) in [("its amount", 30 * KAS1), ("what it is still owed", 5 * KAS1)] {
        let mut t = with_arg(&b, 8, int_arg(shown));
        object_made(&mut t, &w.lib, 2, &x2.s.hash());
        assert!(!accepted(&t, &w.keys), "the claim (30, 25) is opened by KpfTLeft.rest, shown {what}");
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Shard { held: 45 * KAS1, ..rich.s }.hash());
        t.tx.outputs[0].value = rich.value - 5 * KAS1 as u64;
        assert!(!accepted(&t, &w.keys), "the claim (30, 25) is paid by KpfTLeft.rest, shown {what}");
    }
    // Not ended: coins reach the shard; neither claim is paid of them.
    let later = live_shard(&base, Shard { held: 20 * KAS1, ..s2.s });
    assert!(later.s.claim_paid_at(&y.s, i64::MAX).is_err() && later.s.claim_paid_at(&x2.s, i64::MAX).is_err());
    for (c, o) in [(&y, outcome(&later.s, &y.s, 15 * KAS1, 0)), (&x2, outcome(&later.s, &x2.s, 5 * KAS1, 0))] {
        assert!(!taken(pay_tx(&w, &later, c, &o, due), &w.keys), "a claim is paid past what its class is owed");
    }
    // The real claims first: whole; the stray nothing.
    let (sx, px, lx) = s0.s.claim_paid_at(&x.s, due).unwrap();
    let (sy, py, ly) = sx.claim_paid_at(&y.s, due).unwrap();
    assert_eq!((px, lx, py, ly, sy.claims.paid1), (30 * KAS1, None, 15 * KAS1, None, 45 * KAS1));
    assert!(sy.claim_paid_at(&z.s, due).is_err());
    assert!(!taken(pay_tx(&w, &live_shard(&base, sy), &z, &(sy, 0, None), due), &w.keys));
    // Half held.
    let half = shard_of(&base, 45 * KAS1 / 2, 45 * KAS1, 0, 0, 0).s;
    let (h1, pz, _) = half.claim_paid_at(&z.s, due).unwrap();
    let (h2, px, lx) = h1.claim_paid_at(&x.s, due).unwrap();
    assert_eq!((pz, px, lx.map(|l| l.paid1), h2.held), (10 * KAS1, 125 * KAS1 / 10, Some(125 * KAS1 / 10), 0), "the stray has its part; the claim of 30 what is left, where its part is 15");
    assert!(h2.claim_paid_at(&y.s, due).is_err());
    // A second call: the claim (30, 25) of the first is paid its 5 KAS of what the second call's claim is owed.
    let second = shard_of(&base, 40 * KAS1, 40 * KAS1, 0, 0, 0);
    let (b, after, p, left) = claim_pay_c(&w.lib, &second, &x2, &pay_s, w.payer(&w.f, &fl)).unwrap();
    assert!(accepted(&b, &w.keys) && (p, left, after.claims.paid1) == (5 * KAS1, None, 5 * KAS1));
    let own = claim_of(&base, xk(&w.f), 40 * KAS1, 0, 0, 0);
    assert_eq!(after.claim_paid_at(&own.s, due).unwrap().1, 35 * KAS1, "the second call's own claim finds 35 of its 40");
    println!("T6 B2 (vi): two claims of 30 and 15 KAS and a note of 20 never counted, 45 held: the stray is paid {} first, the claim of 30 then 25, the claim of 15 nothing; the call ends; the claim of 15 is then spent for nothing by KpfTLeft.rest and the claim (30, 25) is opened by no script, until a second call pays it 5 of another claim's 40", 20 * KAS1);
}

/// What can be written of the OUTPUTS of a claim's payment that is taken (second round, K1): the key's output (the
/// last) a sompi short; to whoever pays; not the last (in the change's place). Where the claim is whole: of the
/// part alone, of what the claim's note held alone. Where it goes on: the claim again holding a sompi less; what
/// the note held paid out with the part, the claim again holding a sompi; the same with the claim again dropped.
fn output_lies(b: &OBuilt, payer: &[u8; 32], pay: i64, whole: bool, note_value: u64) -> Vec<(&'static str, OBuilt)> {
    let m = b.tx.outputs.len() - 1;
    let with = |g: &dyn Fn(&mut OBuilt)| {
        let mut t = b.clone();
        g(&mut t);
        t
    };
    let mut v = vec![
        ("the key's output a sompi short", with(&|t| t.tx.outputs[m].value -= 1)),
        ("the key's output to whoever pays", with(&|t| t.tx.outputs[m].script_public_key = p2pk_spk(payer))),
    ];
    if let Some(ch) = b.change {
        v.push(("the key's output not the last", with(&|t| t.tx.outputs.swap(ch, m))));
    }
    if whole {
        if note_value > 0 {
            v.push(("the part alone, what the note held kept", with(&|t| t.tx.outputs[m].value = pay as u64)));
        }
        if pay > 0 {
            v.push(("what the note held alone, the part kept", with(&|t| t.tx.outputs[m].value = note_value)));
        }
    } else {
        v.push(("the claim again holding a sompi less", with(&|t| t.tx.outputs[2].value -= 1)));
        if note_value > 1 {
            v.push(("what the note held paid out with the part", with(&|t| {
                t.tx.outputs[2].value = 1;
                t.tx.outputs[m].value += note_value - 1;
            })));
        }
    }
    v
}

/// K1, closed (second round). It was: a payment that makes a claim whole and is below a note's least asks no
/// output, so the last part of any claim that was paid a part before was whoever pays it's. Now the payment that
/// makes a claim whole pays its key, in ONE output, the part and what the claim's note held, whatever the part's
/// size; a payment that does not make it whole is a note's least or more and pays the part alone, the claim going
/// on with what it held.
/// The scenario of the first round, every transaction at the engine: ten claims of 1 KAS (C1 = 10); the shard
/// holds 8.1. Each claim is paid 0.81 KAS (the output is the part alone; the claim goes on as (1, 0.81) holding
/// its 0.2 KAS). The owner pays in the 1.9 KAS that are missing (`KpfTAddC`) and pays each claim whole itself:
/// the payment is 0.19 KAS, below a note's least; the transaction has an output of 0.39 KAS to the user (the part
/// and the note's least). Refused, for each of the ten: that output a sompi short; to the owner; not the last; of
/// the part alone (0.19); of the note's least alone (0.2). The call ends. The users have 10 of 10 KAS and their
/// ten notes' 0.2 KAS; whoever paid has, of every transaction, its fee less and the 200,000 sompi of the script's
/// copy less, and nothing else: the owner is out its 1.9 KAS and its fees.
/// And: a claim whole by ONE payment has its amount and what its note held (0.2 KAS; 0.5 where the note held 0.5);
/// a claim of a sompi is paid a sompi and its note's least in one output (it had no output); a claim that says it
/// was paid all it asks (by hand) is spent for what its note held, to its key; a part's payment cannot take the
/// note's value out (the claim again holds what it held).
#[test]
fn t6_k1_closed_the_last_part_of_a_claim_and_what_its_note_held_are_paid_to_its_key() {
    let w = world();
    let base = seeded(&w);
    let pay_s = w.script("KpfTCallPay");
    let user = xk(&w.w);
    let mut shard = shard_of(&base, 81 * KAS1 / 10, 10 * KAS1, 0, 0, 0);
    let due = shard.s.claims_time().unwrap();
    let (ff, fo) = ([coin(xk(&w.f), 36, due)], [coin(xk(&w.c), 38, due)]);
    let mut claims: Vec<OLive<Claim>> = (0..10u8).map(|i| OLive { op: op(100 + i), ..claim_of(&base, user, KAS1, 0, 0, 0) }).collect();
    let (mut to_users, mut refused, mut payers_cost) = (0i64, 0, 0i64);
    // What whoever paid has of a transaction: its coin less the fee and what the script's copy keeps.
    let cost_of = |sg: &OSigned, payer: &[u8; 32]| -> i64 { 1_000 * KAS1 - paid_to(sg, payer) };
    let copy_fee = |sg: &OSigned| sg.tx.outputs[1].value as i64 - sg.entries[1].amount as i64;
    // Each claim paid its part: 0.81 KAS, the part alone; the claim goes on holding what it held.
    for (i, c) in claims.iter_mut().enumerate() {
        let (b, next, p, left) = claim_pay_c(&w.lib, &shard, c, &pay_s, w.payer(&w.f, &ff)).unwrap();
        let sg = signed(&format!("K1: claim {i} paid its part"), &b, &w.keys);
        let left = left.expect("the claim goes on");
        let m = sg.tx.outputs.len() - 1;
        assert_eq!((p, left.paid1, sg.tx.outputs[m].value, &sg.tx.outputs[m].script_public_key, paid_to(&sg, &user)), (81 * KAS1 / 100, 81 * KAS1 / 100, 81 * KAS1 as u64 / 100, &p2pk_spk(&user), 81 * KAS1 / 100));
        assert_eq!((sg.tx.outputs[2].value, &sg.tx.outputs[2].script_public_key), (NOTE_MIN as u64, &hash_spk(&w.lib, &left.hash())), "the claim again, holding what it held");
        assert_eq!(cost_of(&sg, &xk(&w.f)), sg.fee as i64 + copy_fee(&sg), "whoever pays a part has its fees less, and nothing else");
        for (lie, t) in output_lies(&b, &xk(&w.f), p, false, c.value) {
            assert!(!accepted(&t, &w.keys), "K1, claim {i} paid a part: taken: {lie}");
            refused += 1;
        }
        to_users += paid_to(&sg, &user);
        payers_cost += cost_of(&sg, &xk(&w.f));
        shard = nth(&sg, 0, next, due as u64 + 1 + i as u64);
        *c = nth(&sg, 2, left, due as u64 + 1 + i as u64);
    }
    assert_eq!((shard.s.held, shard.s.claims.paid1, to_users), (0, 81 * KAS1 / 10, 81 * KAS1 / 10));
    // The owner pays in what is missing, and pays each claim whole itself.
    let (b, next) = shard_fund(&w.lib, &shard, &w.script("KpfTAddC"), 19 * KAS1 / 10, w.payer(&w.c, &fo)).unwrap();
    let sg = signed("K1: the owner pays its called shard up", &b, &w.keys);
    let mut owners_cost = cost_of(&sg, &xk(&w.c));
    assert_eq!(owners_cost, 19 * KAS1 / 10 + sg.fee as i64 + copy_fee(&sg));
    shard = nth(&sg, 0, next, due as u64 + 20);
    for (i, c) in claims.iter().enumerate() {
        let (b, next, p, left) = claim_pay_c(&w.lib, &shard, c, &pay_s, w.payer(&w.c, &fo)).unwrap();
        assert_eq!((p, left), (19 * KAS1 / 100, None), "the last part, below a note's least");
        let sg = signed(&format!("K1: claim {i} paid whole by the owner"), &b, &w.keys);
        let m = sg.tx.outputs.len() - 1;
        assert_eq!((sg.tx.outputs[m].value as i64, &sg.tx.outputs[m].script_public_key, paid_to(&sg, &user)), (19 * KAS1 / 100 + NOTE_MIN, &p2pk_spk(&user), 19 * KAS1 / 100 + NOTE_MIN), "K1 is open: the last part and what the note held are not paid to the claim's key");
        assert_eq!(cost_of(&sg, &xk(&w.c)), sg.fee as i64 + copy_fee(&sg), "K1 is open: whoever pays the last part has something of it");
        let lies = output_lies(&b, &xk(&w.c), p, true, c.value);
        assert_eq!(lies.len(), 5);
        for (lie, t) in lies {
            assert!(!accepted(&t, &w.keys), "K1 is open, claim {i} paid whole: taken: {lie}");
            refused += 1;
        }
        to_users += paid_to(&sg, &user);
        owners_cost += cost_of(&sg, &xk(&w.c));
        shard = nth(&sg, 0, next, due as u64 + 21 + i as u64);
    }
    assert_eq!((shard.s.held, shard.s.claims.paid1, shard.s.owed, to_users), (0, 10 * KAS1, 10 * KAS1, 10 * KAS1 + 10 * NOTE_MIN), "the users have 10 of 10 KAS, and their notes' 0.2 KAS each");
    let (b, open) = call_off(&w.lib, &shard, &call_of(&base), &pay_s, w.payer(&w.c, &fo)).unwrap();
    signed("K1: the call ends", &b, &w.keys);
    assert_eq!((open.called, open.held, open.owed), (0, 0, 0));

    // A claim whole by one payment: its amount and what its note held, in one output; a sompi less refused.
    let rich = shard_of(&base, 50 * KAS1, 30 * KAS1, 0, 0, 0);
    let mut seen = vec![];
    for (what, amount, x1, holds) in [("12 KAS", 12 * KAS1, 0, NOTE_MIN), ("12 KAS, its note holding 0.5", 12 * KAS1, 0, KAS1 / 2), ("a note's least", NOTE_MIN, 0, NOTE_MIN), ("a sompi below a note's least", NOTE_MIN - 1, 0, NOTE_MIN), ("a sompi", 1, 0, NOTE_MIN), ("paid all it asks already (by hand)", 12 * KAS1, 12 * KAS1, NOTE_MIN)] {
        let c = OLive { value: holds as u64, ..claim_of(&base, user, amount, x1, 0, 0) };
        let (b, _, p, left) = claim_pay_c(&w.lib, &rich, &c, &pay_s, w.payer(&w.f, &ff)).unwrap_or_else(|e| panic!("{what}: {e}"));
        let sg = signed(&format!("K1: a claim of {what} paid whole"), &b, &w.keys);
        let m = sg.tx.outputs.len() - 1;
        assert_eq!((p, left, sg.tx.outputs[m].value as i64, &sg.tx.outputs[m].script_public_key), (amount - x1, None, amount - x1 + holds, &p2pk_spk(&user)), "{what}");
        assert_eq!(cost_of(&sg, &xk(&w.f)), sg.fee as i64 + copy_fee(&sg), "{what}");
        for (lie, t) in output_lies(&b, &xk(&w.f), p, true, c.value) {
            assert!(!accepted(&t, &w.keys), "K1, a claim of {what} paid whole: taken: {lie}");
            refused += 1;
        }
        seen.push((what, p, sg.tx.outputs[m].value, sg.fee, sg.mass.size));
    }
    // (By hand, what no script writes: a claim of a sompi whose note holds 1,000 sompi. Its payment's one output is
    // 1,001 sompi; whether a block takes so small an output is the harness's to say. A counted claim's note holds
    // a note's least or more: every script that writes one asks it.)
    let thin = OLive { value: 1_000, ..claim_of(&base, user, 1, 0, 0, 0) };
    let small = taken(claim_pay_c(&w.lib, &rich, &thin, &pay_s, w.payer(&w.f, &ff)).map(|x| x.0), &w.keys);
    println!("T6 K1 (closed): ten claims of 1 KAS on a shard of 8.1: each paid 0.81, then, paid up by the owner, 0.19 and its note's 0.2 in one output; the users have {to_users} sompi; the owner is out {owners_cost} (the 1.9 KAS and its fees), whoever paid the parts {payers_cost}; {refused} transactions with other outputs refused. Whole by one payment (what, the part, the output, the fee, bytes): {seen:?}. A claim of a sompi whose note holds 1,000 sompi (by hand) is paid: {small}");
}

// ================================================================================================================
// B3. The time.

/// A built transaction with every input landed at `all`, and the input `i` at `one`.
fn landed_at(b: &OBuilt, all: i64, i: usize, one: i64) -> OBuilt {
    let mut t = b.clone();
    for e in t.entries.iter_mut() {
        e.block_daa_score = all as u64;
    }
    t.entries[i].block_daa_score = one as u64;
    t
}

/// B3. No claim is paid before the call's time, a lateness and three dispute periods; an input that landed then
/// shows it, and the script is told which (`f`, its last argument, the twelfth from nothing).
///  - `f` names each of the four inputs in turn (the shard, the script's copy, the claim, a coin): taken where
///    that input landed at the time and every other long before; refused where it landed a block earlier, though
///    every OTHER input landed after the time; refused for an input beyond the last, and for one below nothing.
///  - The periods are the shard's own (its moving part's bytes 94 to 102 and 103 to 111, which its id is the hash
///    of: `KpfTShardOpen`, `KpfTEnterS`; no script writes them, read in every script that writes a moving part): a
///    shard of a lateness of 13 and a dispute period of 7 pays from its call's time and 34; shown a moving part of
///    other periods the script does not open the shard.
///  - Before the time, what a called shard's owner can do: add to it (taken: the claims' numbers carried; a note of
///    a leg of no row spent on it by `KpfTLeftVoid` carries them too); pay a
///    note of the rest by `KpfTLeft.rest` (refused: called); end the call where D1 is C1 and D2 is C2 (taken, long
///    before the time: with nothing owed, at once; `off` reads no time); a draw is answered wrong (called).
#[test]
fn t6_b3_no_claim_is_paid_before_the_time_whatever_input_is_named_and_the_periods_are_the_shards() {
    let w = world();
    let base = seeded(&w);
    let pay_s = w.script("KpfTCallPay");
    let shard = shard_of(&base, 50 * KAS1, 30 * KAS1, 0, 0, 0);
    let claim = claim_of(&base, xk(&w.w), 12 * KAS1, 0, 0, 0);
    let due = shard.s.claims_time().unwrap();
    assert_eq!(due, CALLED + 100 + 150);
    let o = shard.s.claim_paid_at(&claim.s, due).unwrap();
    assert!(shard.s.claim_paid_at(&claim.s, due - 1).is_err());
    let b = pay_tx(&w, &shard, &claim, &o, due).unwrap();
    assert!(accepted(&b, &w.keys));
    assert_eq!((b.tx.inputs.len(), args_of(&mut b.clone(), 1).len()), (4, 13), "the shard, the script's copy, the claim, a coin; thirteen arguments");
    assert!(matches!(&args_of(&mut b.clone(), 1)[12], Arg::V(ArtifactValue::Int(3))), "the builder names the coin");
    let names = ["the shard", "the script's copy", "the claim", "a coin"];
    for (i, name) in names.iter().enumerate() {
        let t = with_arg(&landed_at(&b, 6_000, i, due), 12, int_arg(i as i64));
        assert!(accepted(&t, &w.keys), "{name} landed at the time and is named: not taken");
        let t = with_arg(&landed_at(&b, due + 1_000, i, due - 1), 12, int_arg(i as i64));
        assert!(!accepted(&t, &w.keys), "{name} landed a block before the time and is named: taken");
        // (Another input is named than the one that landed at the time.)
        let t = with_arg(&landed_at(&b, 6_000, i, due), 12, int_arg(((i + 1) % 4) as i64));
        assert!(!accepted(&t, &w.keys), "{name} landed at the time and another is named: taken");
    }
    for f in [4i64, 5, 1_000, -1, -4, i64::MAX, i64::MIN + 1] {
        assert!(!accepted(&with_arg(&landed_at(&b, due + 1_000, 0, due + 1_000), 12, int_arg(f)), &w.keys), "an input {f} is named");
    }
    // The shard's own periods.
    let mut other = shard.clone();
    other.s.t.late_daa = 13;
    other.s.dispute_daa = 7;
    let due2 = other.s.claims_time().unwrap();
    assert_eq!(due2, CALLED + 13 + 21);
    let o2 = other.s.claim_paid_at(&claim.s, due2).unwrap();
    assert!(taken(pay_tx(&w, &other, &claim, &o2, due2), &w.keys) && !taken(pay_tx(&w, &other, &claim, &o2, due2 - 1), &w.keys));
    // (The first shard shown with the moving part of the second: its claim would be paid 216 blocks sooner.)
    let early = pay_tx(&w, &shard, &claim, &o, due2).unwrap();
    assert!(!accepted(&early, &w.keys));
    assert!(matches!(&args_of(&mut early.clone(), 1)[6], Arg::V(ArtifactValue::Bytes(x)) if x[..] == shard.s.moving()[..]), "the moving part is argument 6");
    assert!(!accepted(&with_arg(&early, 6, bytes_arg(&other.s.moving())), &w.keys), "a shard is paid from the time of a moving part that is not its own");
    let mut zero = shard.s;
    zero.called = 1;
    assert!(!accepted(&with_arg(&early, 6, bytes_arg(&zero.moving())), &w.keys), "a shard is paid from another call's time");

    // Before the time. The owner adds to its called shard, whose numbers are not nothing: carried.
    let part = shard_of(&base, 10 * KAS1, 30 * KAS1, 7 * KAS1, 5 * KAS1, 0);
    let fc = [fund(xk(&w.c), 38)];
    let (b, next) = shard_fund(&w.lib, &part, &w.script("KpfTAddC"), 3 * KAS1, w.payer(&w.c, &fc)).unwrap();
    let sg = signed("B3: the owner adds to a called shard that has paid", &b, &w.keys);
    assert_eq!((next.held, next.owed, next.claims, next.called, &sg.tx.outputs[0].script_public_key), (13 * KAS1, 30 * KAS1, part.s.claims, CALLED, &hash_spk(&w.lib, &next.hash())));
    // A note of a leg of a market that has no row is spent on the called shard that has paid (`KpfTLeftVoid`): the
    // shard as it is, its numbers carried.
    let ghost = OLive { s: Left { sid: base.s.sid, legs: entered(&[fleg(&w, 2, 3, LONG, P10)], 0), key: xk(&w.w), asked: 0 }, value: NOTE_MIN as u64, id: base.id, op: op(60), daa: 6_200 };
    let b = left_void(&w.lib, &part, &ghost, 0, &w.script("KpfTLeftVoid"), w.payer(&w.f, &[fund(xk(&w.f), 36)])).unwrap();
    let sg = signed("B3: a note of a leg of no row spent on a called shard that has paid", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (part.value, &hash_spk(&w.lib, &part.s.hash())));
    // A note of the rest is not paid by `KpfTLeft.rest` while the shard is called: the transaction of a shard that is
    // not called, with the called one in its place.
    let quiet = live_shard(&base, Shard { called: 0, ..shard.s });
    let rest = OLive { s: Rest { sid: base.s.sid, key: xk(&w.w), amount: 12 * KAS1 }, value: NOTE_MIN as u64, id: base.id, op: op(71), daa: 6_100 };
    let ff = [fund(xk(&w.f), 36)];
    let (b, after, left) = rest_apply(&w.lib, &quiet, &rest, &w.script("KpfTRest"), w.payer(&w.f, &ff)).unwrap();
    assert!(accepted(&b, &w.keys) && (after.held, after.owed, left) == (38 * KAS1, 18 * KAS1, None));
    assert!(rest_apply(&w.lib, &shard, &rest, &w.script("KpfTRest"), w.payer(&w.f, &ff)).is_err());
    for next in [Shard { called: CALLED, ..after }, Shard { called: CALLED, owed: 30 * KAS1, ..after }, Shard { called: CALLED, owed: 30 * KAS1, claims: Claims { paid1: 12 * KAS1, owed2: 0, paid2: 0 }, ..after }] {
        let mut t = with_arg(&b, 6, bytes_arg(&shard.s.moving()));
        object_made(&mut t, &w.lib, 0, &shard.s.hash());
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &next.hash());
        assert!(!accepted(&t, &w.keys), "a note of the rest is paid by KpfTLeft.rest on a called shard");
    }
    // The call's end reads no time: where what was paid is what is owed it is taken, every input landed long before
    // the time of the claims. With nothing owed and no row, that is from the block the call is applied in.
    let call = call_of(&base);
    for (what, s) in [("nothing owed", shard_of(&base, 50 * KAS1, 0, 0, 0, 0)), ("all that is owed paid", shard_of(&base, 50 * KAS1, 30 * KAS1, 30 * KAS1, 4 * KAS1, 4 * KAS1))] {
        let (b, open) = call_off(&w.lib, &s, &call, &pay_s, w.payer(&w.f, &ff)).unwrap();
        let t = landed_at(&b, CALLED + 1, 0, CALLED + 1);
        assert!(accepted(&t, &w.keys), "B3, {what}: the call does not end before the time of the claims");
        assert_eq!((open.called, open.owed, open.claims, open.held), (0, 0, Claims::default(), 50 * KAS1), "{what}");
    }
    // A draw on a called shard, stated by its owner whatever the rule says: its end is answered wrong, and not right.
    let on = shard_of(&base, 50 * KAS1, 30 * KAS1, 7 * KAS1, 0, 0);
    let x = on.s;
    let d = Draw { amount: KAS1, prices: [0; 4].into() };
    let said = x.traced(&d, false).expect("a trace").said();
    let outs = Outs::of(&[Out { key: false, to: x.drawn(KAS1, T0).hash(), value: on.value as i64 - KAS1 }, Out { key: true, to: xk(&w.c), value: KAS1 }]).unwrap();
    let st = Stated { position: x.hash(), window: x.dispute_daa, late: x.t.late_daa, kind: KIND_DRAW, t0: T0, arg: blake2b(&said), outs, bond: x.bond(), claimer: xk(&w.c), held: on.value as i64 };
    let q = w.pointed(&w.stated(&on, &st, &said), 0);
    let fw = [fund(xk(&w.w), 31)];
    let (b, right) = shard_end(&w.lib, &q, &x, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &fw), None).unwrap();
    assert!(!right, "the model answers a draw on a called shard right");
    let sg = signed("B3: the end of a draw on a called shard answered wrong", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (on.value, &hash_spk(&w.lib, &x.hash())), "the shard again, its numbers as they were");
    let forced = shard_end(&w.lib, &q, &x, &d, &w.script("KpfTShardEnd"), xk(&w.w), w.payer(&w.w, &fw), Some(true));
    assert!(forced.is_err() || !accepted(&forced.unwrap().0, &w.keys), "a draw on a called shard is answered right");
    println!("T6 B3: a claim is paid where the input named (each of the shard, the script's copy, the claim, a coin) landed at the call's time, a lateness and three dispute periods ({due}), and not a block earlier nor for another input's landing; the periods are the shard's; before the time the owner adds, pays no note of the rest, draws nothing, and ends the call only where what was paid is what is owed");
}

// ================================================================================================================
// B4. The call's end.

/// B4. `KpfTCallPay.off` ends a call where no row has a unit, D1 is C1 and D2 is C2, and writes C1 and the three
/// numbers as nothing, the shard not called, holding the call's coins. Refused, each by a transaction written for
/// it: D1 a sompi below C1, and above; D2 a sompi below C2, and above; a row with a unit; four empty rows and a sum
/// of the gross that is not nothing (an object no script writes); the numbers left as they were, each of them; what
/// is owed left; the shard left called; the shard as it was; another call's time said; a sompi less in the shard.
/// And a shard after its call's end is the shard it was before the call, to the byte, holding what the claims
/// left: it takes an entry, and its owner draws from it.
#[test]
fn t6_b4_the_call_ends_where_what_was_paid_is_what_is_owed_and_the_shard_is_then_as_any_other() {
    let w = world();
    let base = seeded(&w);
    let pay_s = w.script("KpfTCallPay");
    let ff = [fund(xk(&w.f), 36)];
    let call = call_of(&base);
    let done = shard_of(&base, 7 * KAS1, 30 * KAS1, 30 * KAS1, 20 * KAS1, 20 * KAS1);
    assert_eq!(done.s.hash(), shard_hash_with(&done.s, &done.s.books_head()), "the reviewer's own hash of a shard is the model's");
    assert_eq!(done.s.books_head(), head_of(0, &done.s.claims));
    let (b, open) = call_off(&w.lib, &done, &call, &pay_s, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("B4: the call ends", &b, &w.keys);
    assert_eq!((sg.tx.outputs[0].value, &sg.tx.outputs[0].script_public_key), (done.value + CALL_HOLD as u64, &hash_spk(&w.lib, &open.hash())));
    assert_eq!(open, Shard { held: 7 * KAS1, ..base.s }, "the shard after its call's end is the shard of before the call, holding what the claims left");
    assert_eq!(open.books_head(), [i9b(0), i9b(0), i9b(0), i9b(0), p33b(&Shard::root_opened())].concat());
    let mut refused = 0;
    // What is paid is not what is owed.
    for (what, d1, d2) in [("D1 a sompi below C1", 30 * KAS1 - 1, 20 * KAS1), ("D1 a sompi above C1", 30 * KAS1 + 1, 20 * KAS1), ("D2 a sompi below C2", 30 * KAS1, 20 * KAS1 - 1), ("D2 a sompi above C2", 30 * KAS1, 20 * KAS1 + 1), ("nothing paid", 0, 0), ("the first class paid, the second not at all", 30 * KAS1, 0), ("the second class paid, the first not", 0, 20 * KAS1)] {
        let s = shard_of(&base, 7 * KAS1, 30 * KAS1, d1, 20 * KAS1, d2);
        assert!(call_off(&w.lib, &s, &call, &pay_s, w.payer(&w.f, &ff)).is_err(), "{what}: the model ends the call");
        let t = call_off_as(&w.lib, &s, &call, open, &pay_s, w.payer(&w.f, &ff)).unwrap();
        assert!(!accepted(&t, &w.keys), "{what}: the call ends");
        // (The control of the way it is written: the same with what is paid made what is owed is taken.)
        let mut ok = s;
        ok.s.claims = Claims { paid1: 30 * KAS1, owed2: 20 * KAS1, paid2: 20 * KAS1 };
        assert!(accepted(&call_off_as(&w.lib, &ok, &call, open, &pay_s, w.payer(&w.f, &ff)).unwrap(), &w.keys));
        refused += 1;
    }
    // A row with a unit.
    let leg = fleg(&w, 0, 1, LONG, P10);
    let booked = live_shard(&base, Shard { rows: base.s.booked(&leg).unwrap().rows, basis: base.s.booked(&leg).unwrap().basis, ..done.s });
    assert!(call_off(&w.lib, &booked, &call, &pay_s, w.payer(&w.f, &ff)).is_err());
    for next in [Shard { called: 0, owed: 0, claims: Claims::default(), ..booked.s }, open] {
        assert!(!accepted(&call_off_as(&w.lib, &booked, &call, next, &pay_s, w.payer(&w.f, &ff)).unwrap(), &w.keys), "the call ends while a row has a unit");
        refused += 1;
    }
    // Four empty rows and a sum of the gross that is not nothing: an object no script writes (the argument beside the
    // rows is the fifth from nothing; the shard is input 0).
    assert!(matches!(&args_of(&mut b.clone(), 1)[5], Arg::V(ArtifactValue::Bytes(x)) if x[..] == done.s.books_head()[..]) && matches!(&args_of(&mut b.clone(), 1)[4], Arg::V(ArtifactValue::Bytes(x)) if x[..] == done.s.rows_bytes()[..]));
    for sum in [1i64, 5, -1, i64::MAX] {
        let head = head_of(sum, &done.s.claims);
        for after in [head_of(sum, &Claims::default()), head_of(0, &Claims::default())] {
            let mut t = with_arg(&b, 5, bytes_arg(&head));
            object_made(&mut t, &w.lib, 0, &shard_hash_with(&done.s, &head));
            t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_hash_with(&open, &after));
            assert!(!accepted(&t, &w.keys), "the call ends on books of no row that say a sum of {sum}");
            refused += 1;
        }
    }
    // (The control of that way of writing an object: the same with the sum of nothing is the transaction itself.)
    let mut t = with_arg(&b, 5, bytes_arg(&head_of(0, &done.s.claims)));
    object_made(&mut t, &w.lib, 0, &shard_hash_with(&done.s, &head_of(0, &done.s.claims)));
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_hash_with(&open, &head_of(0, &Claims::default())));
    assert!(accepted(&t, &w.keys));
    // What is written.
    let d = done.s;
    for (what, next) in [
        ("the shard as it was", d),
        ("not called, the numbers and what is owed left", Shard { called: 0, ..d }),
        ("what is owed left", Shard { called: 0, claims: Claims::default(), ..d }),
        ("the three numbers left", Shard { called: 0, owed: 0, ..d }),
        ("what the first class was paid left", Shard { called: 0, owed: 0, claims: Claims { paid1: d.claims.paid1, owed2: 0, paid2: 0 }, ..d }),
        ("what the second class is owed left", Shard { called: 0, owed: 0, claims: Claims { paid1: 0, owed2: d.claims.owed2, paid2: 0 }, ..d }),
        ("what the second class was paid left", Shard { called: 0, owed: 0, claims: Claims { paid1: 0, owed2: 0, paid2: d.claims.paid2 }, ..d }),
        ("still called", Shard { owed: 0, claims: Claims::default(), ..d }),
        ("the owner paid what is held", Shard { held: 0, ..open }),
        ("a sompi more held", Shard { held: open.held + 1, ..open }),
    ] {
        assert!(!accepted(&call_off_as(&w.lib, &done, &call, next, &pay_s, w.payer(&w.f, &ff)).unwrap(), &w.keys), "the call ends and writes: {what}");
        refused += 1;
    }
    // The call: another time, another body, the call of another shard.
    assert!(matches!(&args_of(&mut b.clone(), 1)[7], Arg::V(ArtifactValue::Int(t)) if *t == CALLED), "the call's time is argument 7");
    assert!(!accepted(&with_arg(&b, 7, int_arg(CALLED + 1)), &w.keys) && !accepted(&with_arg(&b, 8, bytes_arg(&[7u8; 32])), &w.keys));
    let elsewhere = OLive { s: Called { sid: [0x44; 32], ..call.s }, ..call.clone() };
    assert!(call_off(&w.lib, &done, &elsewhere, &pay_s, w.payer(&w.f, &ff)).is_err());
    assert!(!accepted(&call_off_as(&w.lib, &done, &elsewhere, open, &pay_s, w.payer(&w.f, &ff)).unwrap(), &w.keys), "the call of another shard ends this one's");
    // A claim in the call's place.
    let c = claim_of(&base, xk(&w.w), 30 * KAS1, 0, 0, 0);
    let mut t = b.clone();
    object_made(&mut t, &w.lib, 2, &c.s.hash());
    assert!(!accepted(&t, &w.keys), "a claim in the call's place ends the call");
    refused += 4;

    // The shard after: it takes an entry, and its owner draws from it.
    let opened = nth(&sg, 0, open, 6_700);
    let acct = cov(&w, &opened, &[], 20 * KAS1);
    let t0 = 6_800;
    let (b, _, _, _, _, next) = c_enter_state(&w.lib, &acct, &opened, &w.script("KpfTEnterS"), SEED, leg, 5 * KAS1, &[], t0, w.payer(&w.w, &[coin(xk(&w.w), 33, t0)])).unwrap();
    let sg = signed("B4: an entry on the shard after its call's end", &b, &w.keys);
    assert_eq!((next.rows[0].gross, next.called, next.owed, next.claims, &sg.tx.outputs[2].script_public_key), (leg.size, 0, 0, Claims::default(), &hash_spk(&w.lib, &next.hash())));
    let fc = [fund(xk(&w.c), 30)];
    let (b, st, _) = shard_draw(&w.lib, &opened, &w.script("KpfTChange"), 2 * KAS1, [0; 4], T0, w.payer(&w.c, &fc)).unwrap();
    let sg = signed("B4: a draw stated on the shard after its call's end", &b, &w.keys);
    let live = nth(&sg, 0, st, 6_510);
    let ap = signed("B4: the draw applied", &stated_settle(&w.lib, &live, &w.script("KpfTChangeSettle"), w.payer(&w.c, &fc)).unwrap(), &w.keys);
    assert_eq!((ap.tx.outputs[0].value, &ap.tx.outputs[0].script_public_key), (opened.value - 2 * KAS1 as u64, &hash_spk(&w.lib, &open.drawn(2 * KAS1, T0).hash())));
    println!("T6 B4: the call ends where D1 is C1 and D2 is C2 and no row has a unit, the numbers written as nothing; {refused} transactions that end it otherwise refused; the shard after is the shard of before the call, takes an entry and is drawn from");
}

// ================================================================================================================
// B5. The notes of the rest, everywhere.

/// B5. A note of the rest is a claim of four numbers in every script that holds one by its hash. Each of the three
/// (`KpfTLeft.rest`, `KpfTCallPay.claim`, `KpfTDrawNote.end` of kind 7), in a transaction of its builder that is
/// taken (the control), with the note made another object (input 2):
///  - the note in the form of before (five fields, one amount): refused by all three;
///  - a claim partly paid (A1, a1 above nothing): refused by `KpfTLeft.rest` on a shard that is not called, shown
///    its amount or what it is still owed, and by `KpfTDrawNote` (it ends no draw);
///  - a claim of the second class (0, 0, A2, 0), and one of both: refused by `KpfTLeft.rest` and `KpfTDrawNote`;
///  - a claim of nothing (0, 0, 0, 0): refused by all three (it ends no draw: `amount > 0` is new there);
///  - a note of one leg (tag 6) and a call (tag 11) of the same shard in the claim's place: refused.
#[test]
fn t6_b5_a_note_of_the_rest_is_a_claim_of_four_numbers_in_every_script_that_holds_one() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let key = xk(&w.w);
    let sid = base.s.sid;
    let rest = OLive { s: Rest { sid, key, amount: 12 * KAS1 }, value: NOTE_MIN as u64, id: base.id, op: op(71), daa: 6_100 };
    assert_eq!(rest.s.hash(), Claim { sid, key, amount1: 12 * KAS1, paid1: 0, amount2: 0, paid2: 0 }.hash());
    assert_eq!(rest.s.hash(), blake2b(&[i9b(12), p33b(&sid), i9b(7), p33b(&key), i9b(12 * KAS1), i9b(0), i9b(0), i9b(0)].concat()), "the claim's hash, written out");
    assert_ne!(rest.s.hash(), old_rest_hash(&sid, &key, 12 * KAS1));
    // The three transactions.
    let quiet = live_shard(&base, Shard { held: 50 * KAS1, owed: 30 * KAS1, ..base.s });
    let (b_rest, ..) = rest_apply(&w.lib, &quiet, &rest, &w.script("KpfTRest"), w.payer(&w.f, &ff)).unwrap();
    let called = shard_of(&base, 50 * KAS1, 30 * KAS1, 0, 0, 0);
    let due = called.s.claims_time().unwrap();
    let (b_claim, ..) = claim_pay(&w.lib, &called, &rest, &w.script("KpfTCallPay"), w.payer(&w.f, &[coin(xk(&w.f), 36, due)])).unwrap();
    let (st, d) = quiet.s.draw(KAS1, [0; 4], T0, quiet.value as i64).unwrap();
    let live = w.stated(&quiet, &st, &quiet.s.traced(&d, false).unwrap().said());
    let fw = [fund(xk(&w.w), 37)];
    let b_draw = draw_note_rest(&w.lib, &live, &st, None, &quiet.s, &rest, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &fw)).unwrap();
    signed("B5: a draw ended by a note of the rest", &b_draw, &w.keys);
    // (Where each is shown the amount: `KpfTLeft.rest` and `KpfTCallPay.claim` argument 8, `KpfTDrawNote.end` 19.)
    let cases: [(&str, &OBuilt, usize); 3] = [("KpfTLeft.rest", &b_rest, 8), ("KpfTCallPay.claim", &b_claim, 8), ("KpfTDrawNote.end", &b_draw, 19)];
    let claim = |a1: i64, x1: i64, a2: i64, x2: i64| Claim { sid, key, amount1: a1, paid1: x1, amount2: a2, paid2: x2 }.hash();
    let leg = fleg(&w, 0, 1, LONG, P10);
    let mut refused = 0;
    for (what, b, at) in cases {
        assert!(accepted(b, &w.keys), "{what}: the control");
        assert!(matches!(&args_of(&mut b.clone(), 1)[at], Arg::V(ArtifactValue::Int(a)) if *a == 12 * KAS1), "{what}: the amount is argument {at}");
        // (The control of the way a note is put in its place: the note itself, and the transaction is taken.)
        let put = |h: [u8; 32], shown: i64| -> OBuilt {
            let mut t = with_arg(b, at, int_arg(shown));
            object_made(&mut t, &w.lib, 2, &h);
            for o in t.tx.outputs.iter_mut().filter(|o| o.script_public_key == hash_spk(&w.lib, &rest.s.hash())) {
                o.script_public_key = hash_spk(&w.lib, &h);
            }
            t
        };
        assert!(accepted(&put(rest.s.hash(), 12 * KAS1), &w.keys), "{what}: (control) the note put in its own place");
        let mut others: Vec<(&str, [u8; 32], i64)> = vec![
            ("the note in the form of before", old_rest_hash(&sid, &key, 12 * KAS1), 12 * KAS1),
            ("a claim of nothing", claim(0, 0, 0, 0), 0),
            ("a claim of nothing, shown as of 12 KAS", claim(0, 0, 0, 0), 12 * KAS1),
            ("a note that a leg left", Left { sid, legs: vec![leg], key, asked: 12 * KAS1 }.hash(), 12 * KAS1),
            ("the shard's call", call_of(&base).s.hash(), 12 * KAS1),
        ];
        if what != "KpfTCallPay.claim" {
            others.extend([
                ("a claim partly paid, its amount shown", claim(12 * KAS1, 5 * KAS1, 0, 0), 12 * KAS1),
                ("a claim partly paid, what it is still owed shown", claim(12 * KAS1, 5 * KAS1, 0, 0), 7 * KAS1),
                ("a claim paid whole", claim(12 * KAS1, 12 * KAS1, 0, 0), 12 * KAS1),
                ("a claim of the second class", claim(0, 0, 12 * KAS1, 0), 12 * KAS1),
                ("a claim of both classes", claim(12 * KAS1, 0, 3 * KAS1, 0), 12 * KAS1),
                ("a claim of both classes, the sum shown", claim(12 * KAS1, 0, 3 * KAS1, 0), 15 * KAS1),
            ]);
        }
        for (which, h, shown) in others {
            assert!(!accepted(&put(h, shown), &w.keys), "{what}: taken with {which} in the note's place");
            refused += 1;
        }
    }
    // (`KpfTCallPay.claim` shown a claim partly paid as one that was not: the hash is of the four numbers.)
    let part = claim_of(&base, key, 12 * KAS1, 5 * KAS1, 0, 0);
    let mut t = b_claim.clone();
    object_made(&mut t, &w.lib, 2, &part.s.hash());
    assert!(!accepted(&t, &w.keys), "a claim partly paid is paid as one of which nothing was");
    // A draw is ended by a note of the rest of a sompi (the least above nothing), and not by one of more than is owed.
    let tiny = OLive { s: Rest { amount: 1, ..rest.s }, ..rest.clone() };
    assert!(taken(draw_note_rest(&w.lib, &live, &st, None, &quiet.s, &tiny, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &fw)), &w.keys));
    let over = OLive { s: Rest { amount: 30 * KAS1 + 1, ..rest.s }, ..rest.clone() };
    assert!(draw_note_rest(&w.lib, &live, &st, None, &quiet.s, &over, &w.script("KpfTDrawNote"), xk(&w.w), w.payer(&w.w, &fw)).is_err());
    println!("T6 B5: KpfTLeft.rest, KpfTCallPay.claim and KpfTDrawNote.end (kind 7) each take the note of four numbers, three of nothing; {refused} transactions with another object in its place refused (the form of before, a claim of nothing, partly paid, of the second class, a note of a leg, the call)");
}

// ================================================================================================================
// B6. A shard that is not called is as before; and the same note under a call.

/// B6. The life of one note of the rest on a shard that is NOT called, at the engine, as before the change but for
/// the note's hash: a note of one leg that asks 50 KAS is applied to a shard that holds 20: 20 are paid, the note
/// of the rest is (30, 0, 0, 0) and the shard owes 30 (written with what is owed left as it was, or with a note of
/// another form: refused); the shard holding nothing, `KpfTLeft.rest` pays nothing; the owner adds 10: the note
/// is paid 10 and goes on as (20, 0, 0, 0), the shard owing 20: what is owed FALLS as a note is paid, where the
/// shard is not called, and its three numbers stay nothing. Then the same shard called, with that note
/// outstanding and 5 KAS held: what it owes has the note already; the claim (20, 0, 0, 0) is paid by `claim` its
/// part, 5 KAS, from the time of the claims, what is owed standing at 20; and `rest` pays it nothing.
#[test]
fn t6_b6_a_note_of_the_rest_on_a_shard_that_is_not_called_lives_as_before_and_is_a_claim_under_a_call() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let key = xk(&w.w);
    let leg = fleg(&w, 0, 10, LONG, P10);
    let shard = live_shard(&base, Shard { held: 20 * KAS1, ..base.s.booked(&leg).unwrap() });
    let note = OLive { s: Left { sid: base.s.sid, legs: entered(&[leg], 0), key, asked: 50 * KAS1 }, value: NOTE_MIN as u64, id: base.id, op: op(60), daa: 6_200 };
    let (b, s1, rest) = left_apply(&w.lib, &shard, &note, &w.script(left_script_on(&(&shard).s, (&note).s.legs.len())), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("B6: a note of one leg applied to a shard that is not called and holds less than is asked", &b, &w.keys);
    let rest = rest.expect("a note of the rest");
    assert_eq!((s1.held, s1.owed, s1.claims, s1.rows, rest), (0, 30 * KAS1, Claims::default(), [ShardRow::EMPTY; 4].into(), Rest { sid: base.s.sid, key, amount: 30 * KAS1 }));
    assert!(pays(&sg, &key, 20 * KAS1));
    assert_eq!((&sg.tx.outputs[0].script_public_key, &sg.tx.outputs[2].script_public_key, sg.tx.outputs[2].value), (&hash_spk(&w.lib, &s1.hash()), &hash_spk(&w.lib, &Claim { sid: base.s.sid, key, amount1: 30 * KAS1, paid1: 0, amount2: 0, paid2: 0 }.hash()), NOTE_MIN as u64), "the note of the rest is the claim (30, 0, 0, 0), holding a note's least");
    for (what, shard_as, note_as) in [
        ("what is owed left as it was", Shard { owed: 0, ..s1 }.hash(), rest.hash()),
        ("the note of the rest in the form of before", s1.hash(), old_rest_hash(&base.s.sid, &key, 30 * KAS1)),
        ("the note of the rest as partly paid", s1.hash(), Claim { paid1: 1, ..rest.claim() }.hash()),
        ("the note of the rest with a second amount", s1.hash(), Claim { amount2: 30 * KAS1, ..rest.claim() }.hash()),
        ("what was paid said as paid to the first class", Shard { claims: Claims { paid1: 20 * KAS1, owed2: 0, paid2: 0 }, ..s1 }.hash(), rest.hash()),
    ] {
        let mut t = b.clone();
        t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &shard_as);
        t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &note_as);
        assert!(!accepted(&t, &w.keys), "B6: {what}");
    }
    let (s1, rest1) = (nth(&sg, 0, s1, 6_300), nth(&sg, 2, rest, 6_300));
    // Nothing held: nothing is paid. Ten KAS added: paid 10, the note goes on, what is owed falls.
    assert!(rest_apply(&w.lib, &s1, &rest1, &w.script("KpfTRest"), w.payer(&w.f, &ff)).is_err());
    let fc = [fund(xk(&w.c), 38)];
    let (b, s2) = shard_fund(&w.lib, &s1, &w.script("KpfTAddC"), 10 * KAS1, w.payer(&w.c, &fc)).unwrap();
    let s2 = nth(&signed("B6: the owner adds 10 KAS", &b, &w.keys), 0, s2, 6_310);
    let (b, s3, rest2) = rest_apply(&w.lib, &s2, &rest1, &w.script("KpfTRest"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("B6: the note of the rest paid a part by a shard that is not called", &b, &w.keys);
    let rest2 = rest2.expect("the note goes on");
    assert_eq!((s3.held, s3.owed, s3.claims, rest2.amount), (0, 20 * KAS1, Claims::default(), 20 * KAS1), "what is owed falls as the note is paid: the shard is not called");
    assert!(pays(&sg, &key, 10 * KAS1));
    assert_eq!(&sg.tx.outputs[2].script_public_key, &hash_spk(&w.lib, &Claim { sid: base.s.sid, key, amount1: 20 * KAS1, paid1: 0, amount2: 0, paid2: 0 }.hash()), "the note that goes on is (20, 0, 0, 0): not (30, 10, 0, 0)");
    let mut t = b.clone();
    t.tx.outputs[0].script_public_key = hash_spk(&w.lib, &Shard { owed: 30 * KAS1, claims: Claims { paid1: 10 * KAS1, owed2: 0, paid2: 0 }, ..s3 }.hash());
    t.tx.outputs[2].script_public_key = hash_spk(&w.lib, &Claim { paid1: 10 * KAS1, ..rest1.s.claim() }.hash());
    assert!(!accepted(&t, &w.keys), "B6: a shard that is not called pays a note as a called one does");
    // The same shard called (by hand: the call's applying writes the time and nothing else), holding 5 KAS.
    let rest2 = nth(&sg, 2, rest2, 6_320);
    let called = live_shard(&base, Shard { called: CALLED, held: 5 * KAS1, ..s3 });
    let due = called.s.claims_time().unwrap();
    assert!(rest_apply(&w.lib, &called, &rest2, &w.script("KpfTRest"), w.payer(&w.f, &ff)).is_err());
    assert!(claim_pay(&w.lib, &called, &rest2, &w.script("KpfTCallPay"), w.payer(&w.f, &ff)).is_err(), "before the time of the claims");
    let (b, s4, paid) = claim_pay(&w.lib, &called, &rest2, &w.script("KpfTCallPay"), w.payer(&w.f, &[coin(xk(&w.f), 36, due)])).unwrap();
    let sg = signed("B6: the same note, a claim under a call, paid its part", &b, &w.keys);
    assert_eq!((paid, s4.held, s4.owed, s4.claims), (5 * KAS1, 0, 20 * KAS1, Claims { paid1: 5 * KAS1, owed2: 0, paid2: 0 }));
    assert_eq!(&sg.tx.outputs[2].script_public_key, &hash_spk(&w.lib, &Claim { paid1: 5 * KAS1, ..rest2.s.claim() }.hash()), "the claim goes on as (20, 5, 0, 0)");
    println!("T6 B6: a note of the rest on a shard that is not called: written (30, 0, 0, 0) with 30 owed, paid 10 and written (20, 0, 0, 0) with 20 owed; under a call the same note is paid its part by KpfTCallPay.claim and goes on as (20, 5, 0, 0), 20 owed still");
}

// ================================================================================================================
// Sizes.

/// The transactions of this change, in bytes, as their builders write them: a claim paid whole; a claim below a
/// note's least paid whole (second round, K1: with its output, as every payment has); a claim paid a part and
/// written again; the call's end; a
/// note of one leg applied to a shard that is not called (paid; and with a note of the rest written), and to a
/// called one (a claim written).
#[test]
fn t6_the_sizes_of_the_transactions_of_a_claim_and_of_a_calls_end() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let kit = kpf_protocol::thin_artifacts::kit();
    let sizes: Vec<(&str, usize)> = ["KpfTCallPay", "KpfTLeft", "KpfTLeftAll", "KpfTLeftVoid", "KpfTMarkNote", "KpfTDrawNote", "KpfTEnterS", "KpfTShardPrice", "KpfTShardRow"].iter().map(|n| (*n, kit.script(n).unwrap().len())).collect();
    // (At 563ef01: 1,691; 2,374; 4,226; 2,017; 1,154; 1,700; 2,463; 2,700; 2,098. `KpfTCallPay` was 2,518 in the
    // first round: K1's output is 5 bytes.)
    // (Review T7, docs 17.8c (b) and (c): KpfTCStep 5,439 from 5,056; KpfTLeft 2,916 from 2,506; KpfTLeftAll 5,093 from 4,310; KpfTLeftVoid 2,088 from 2,030; KpfTMarkNote 1,419 from 1,190; KpfTEnterS 2,982 from 2,484.)
    assert_eq!(sizes, vec![("KpfTCallPay", 2461), ("KpfTLeft", 1705), ("KpfTLeftAll", 4476), ("KpfTLeftVoid", 2110), ("KpfTMarkNote", 1188), ("KpfTDrawNote", 1742), ("KpfTEnterS", 3039), ("KpfTShardPrice", 2360), ("KpfTShardRow", 2169)]);
    let due = shard_of(&base, 0, 0, 0, 0, 0).s.claims_time().unwrap();
    let fl = [coin(xk(&w.f), 36, due)];
    let pay_s = w.script("KpfTCallPay");
    let key = xk(&w.w);
    let rich = shard_of(&base, 50 * KAS1, 30 * KAS1, 0, 0, 0);
    let (b, ..) = claim_pay_c(&w.lib, &rich, &claim_of(&base, key, 12 * KAS1, 0, 0, 0), &pay_s, w.payer(&w.f, &fl)).unwrap();
    let whole = signed("sizes: a claim paid whole", &b, &w.keys).mass.size;
    let (b, ..) = claim_pay_c(&w.lib, &rich, &claim_of(&base, key, NOTE_MIN / 4, 0, 0, 0), &pay_s, w.payer(&w.f, &fl)).unwrap();
    let dust = signed("sizes: a claim paid whole below a note's least", &b, &w.keys).mass.size;
    let poor = shard_of(&base, 10 * KAS1, 30 * KAS1, 0, 0, 0);
    let (b, ..) = claim_pay_c(&w.lib, &poor, &claim_of(&base, key, 12 * KAS1, 0, 0, 0), &pay_s, w.payer(&w.f, &fl)).unwrap();
    let part = signed("sizes: a claim paid a part and written again", &b, &w.keys).mass.size;
    let both = shard_of(&base, 50 * KAS1, 30 * KAS1, 3 * KAS1, 40 * KAS1, KAS1);
    let (b, ..) = claim_pay_c(&w.lib, &both, &claim_of(&base, key, 12 * KAS1, 2 * KAS1, 20 * KAS1, KAS1), &pay_s, w.payer(&w.f, &fl)).unwrap();
    let two = signed("sizes: a claim of both classes paid a part and written again", &b, &w.keys).mass.size;
    let done = shard_of(&base, 7 * KAS1, 30 * KAS1, 30 * KAS1, 0, 0);
    let (b, _) = call_off(&w.lib, &done, &call_of(&base), &pay_s, w.payer(&w.f, &ff)).unwrap();
    let off = signed("sizes: the call's end", &b, &w.keys).mass.size;
    let leg = fleg(&w, 0, 10, LONG, P10);
    let mut applied = vec![];
    for (what, held, called, asked) in [("not called, paid whole", 100 * KAS1, 0, 50 * KAS1), ("not called, a note of the rest written", 20 * KAS1, 0, 50 * KAS1), ("called, a claim written", 20 * KAS1, CALLED, 50 * KAS1), ("not called, nothing asked", 20 * KAS1, 0, 0)] {
        let shard = live_shard(&base, Shard { held, called, ..base.s.booked(&leg).unwrap() });
        let note = OLive { s: Left { sid: base.s.sid, legs: entered(&[leg], 0), key, asked }, value: NOTE_MIN as u64, id: base.id, op: op(60), daa: 6_200 };
        let (b, ..) = left_apply(&w.lib, &shard, &note, &w.script(left_script_on(&(&shard).s, (&note).s.legs.len())), w.payer(&w.f, &ff)).unwrap();
        applied.push((what, signed(&format!("sizes: a note of one leg applied, {what}"), &b, &w.keys).mass.size));
    }
    println!("T6 sizes: the scripts {sizes:?}; a claim paid whole {whole} bytes (3,624 before), below a note's least {dust}; paid a part and written again {part}, of both classes {two}; the call's end {off}; a note of one leg applied: {applied:?} (4,314 before, nothing asked)");
    // (Second round, K1: 4,450, 4,345, 4,485, 4,497 and 4,340 in the first: the script is 5 bytes more, and a claim
    // below a note's least paid whole has its output.)
    // (Review T9, docs 17.8d: 4,455, 4,454, 4,490, 4,502 and 4,345 before: each 56 bytes more. `KpfTCallPay` is 44
    // bytes more (2,567 from 2,523: the row of no market written out is nine pushes where it was six) and is
    // shown 12 bytes more of rows (420 from 408). A claim paid and a call's end are DEARER by stage 1.)
    assert_eq!((whole, dust, part, two, off), ((4_438, 4_437, 4_473, 4_485, 4_328)));
    // (Review T7, docs 17.8c (b): `KpfTLeft` is 410 bytes longer (the class at the applying) and is shown one
    // argument more: a note of one leg applied was 4,550, 4,585, 4,533 and 4,441.)
    // (Review T9, docs 17.8d: 4,961, 4,996, 4,944 and 4,852 before: each 10 bytes more. `KpfTLeft` is 2 bytes less
    // and is shown 12 bytes more of rows.)
    // (17.8e: 4,971, 5,006, 4,954 and 4,862 before: each 965 bytes less. `KpfTLeft` no longer carries the paying of
    // a note of the rest, which is `KpfTRest`.)
    // (17.8e, 2: a shard that is not called has its notes applied by the script that carries nothing of the classes:
    // 275 bytes less; the called shard's, the third, by the twin: 4 more, its check of the state.)
    assert_eq!(applied.iter().map(|x| x.1).collect::<Vec<_>>(), vec![3_795, 3_830, 4_057, 3_686]); // (17.8j, A1: the root, 33 bytes, and 31 of script.)
}

// ================================================================================================================
// What piece (a) leaves as it was, said for piece (b).

/// Two things that are not of piece (a), run so that their numbers are on the table for piece (b).
///  1. A claim counted after payments began (T29; only a mark's note can be: every other empties a row). A shard
///     that has ever held 150; X, a claim of 100, is paid whole. A note of 100 is then applied (by hand: what is
///     owed is 200, a claim (100, 0, 0, 0) written; the applying of a mark's note under a call is not run here): it
///     is paid 50, all that is held, where its part of 150 over 200 is 75, and X has 100 where its part is 75. As
///     before this change, to the sompi. What is new: the late claim goes on as (100, 50), the shard stays called,
///     and 50 KAS that reach it later are that claim's (before: it was spent with its 50 and the call ended).
///  2. A claim of both classes whose first part is below a note's least waits behind its second: the first class
///     covered (0.1 KAS owed and held, and 1,000 sompi more), a claim of 0.1 KAS of the first class and 100 KAS of
///     the second is paid by no outcome (its two parts are 0.1 KAS and 1,000 sompi: below the least together, and
///     not whole); the same 0.1 KAS as a claim of the first class alone is paid whole. Nothing is lost by it (the
///     second class cannot have what the first is still owed); it is the order a user sees.
#[test]
fn t6_for_piece_b_a_claim_counted_late_and_a_first_part_that_waits_behind_its_second() {
    let w = world();
    let base = seeded(&w);
    let due = shard_of(&base, 0, 0, 0, 0, 0).s.claims_time().unwrap();
    // 1.
    let s0 = shard_of(&base, 150 * KAS1, 100 * KAS1, 0, 0, 0);
    let x = claim_of(&base, xk(&w.w), 100 * KAS1, 0, 0, 0);
    let (s1, px, left) = s0.s.claim_paid_at(&x.s, due).unwrap();
    assert_eq!((px, left, s1.held), (100 * KAS1, None, 50 * KAS1));
    // (The mark's note, applied to the called shard by `KpfTMarkNote`: a long of 10 units marked from 10 to 20
    // gained 100 KAS. It reads no row: it is applied to books of no row. Nothing is paid; what is owed is 200;
    // the three numbers are carried; the claim (100, 0, 0, 0) is written.)
    let mark = OLive { s: MarkNote { sid: base.s.sid, leg: fleg(&w, 0, 10, LONG, P10), entry: 2 * P10, key: xk(&w.f), asked: 100 * KAS1 }, value: NOTE_MIN as u64, id: base.id, op: op(61), daa: 6_300 };
    let ff = [fund(xk(&w.f), 36)];
    // (Review T7, docs 17.8c, piece (b): T29 IS CLOSED HERE. Once anything was paid, what is counted is of the
    // second class: the mark's note is applied by `KpfTMarkNote`, which opens the books (68 bytes) and writes
    // what is owed of the second class: 100 owed of the first still, 100 of the second, the claim (0, 0, 100, 0).
    // The form of the first class is refused, by the model and at the engine. The late claim is then paid what
    // the shard has beyond what the first class is owed: 50, as before in coins, but now BEHIND X by right: X is
    // whole because its class is covered, not because it came first. This test asserted the hole: the note
    // counted in the first class, 200 owed.)
    let paid_shard = live_shard(&base, s1);
    assert!(s1.paid_any() && mark_apply(&w.lib, &paid_shard, &mark, &w.script(mark_script_on(&(&paid_shard).s)), w.payer(&w.f, &ff)).is_err(), "T29 is open: a note counted after a payment is of the first class");
    let first = mark_apply_as(&w.lib, &paid_shard, &mark, Shard { owed: 200 * KAS1, basis: s1.marked_class(&mark.s).unwrap().0.basis, ..s1 }, 0, Some(Rest { sid: base.s.sid, key: xk(&w.f), amount: 100 * KAS1 }), &w.script(mark_script_on(&(&paid_shard).s)), w.payer(&w.f, &ff));
    assert!(first.is_err() || !accepted(&first.unwrap().0, &w.keys), "T29 is open: the engine counts a note in the first class after a payment");
    let (b, late, m) = mark_apply_c(&w.lib, &paid_shard, &mark, &w.script(mark_script_on(&(&paid_shard).s)), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("for (b): a mark's note applied to a called shard that has paid", &b, &w.keys);
    assert_eq!((late.owed, late.held, late.claims, m), (100 * KAS1, 50 * KAS1, Claims { paid1: 100 * KAS1, owed2: 100 * KAS1, paid2: 0 }, Some(Claim { sid: base.s.sid, key: xk(&w.f), amount1: 0, paid1: 0, amount2: 100 * KAS1, paid2: 0 })));
    let (late, m) = (nth(&sg, 0, late, 6_700), nth(&sg, 2, m.unwrap(), 6_700));
    let o = late.s.claim_paid_at(&m.s, due).unwrap();
    assert_eq!((o.1, o.2, o.0.held, o.0.claims.paid1, o.0.claims.paid2), (50 * KAS1, Some(Claim { paid2: 50 * KAS1, ..m.s }), 0, 100 * KAS1, 50 * KAS1), "the late claim has what is beyond the first class");
    assert!(taken(pay_tx(&w, &late, &m, &o, due), &w.keys));
    assert!(o.0.call_off(&call_of(&base).s).is_err(), "the call ends with the late claim half paid");
    let more = live_shard(&base, Shard { held: 50 * KAS1, ..o.0 });
    let m2 = live_claim(&base, o.2.unwrap());
    let o2 = more.s.claim_paid_at(&m2.s, due).unwrap();
    assert_eq!((o2.1, o2.2, o2.0.claims.paid1, o2.0.claims.paid2), (50 * KAS1, None, 100 * KAS1, 100 * KAS1));
    assert!(taken(pay_tx(&w, &more, &m2, &o2, due), &w.keys) && o2.0.call_off(&call_of(&base).s).is_ok());
    // (The same counted late beside a claim that was on time and not yet shown: X and Y of 100 each, 100 ever
    // held; X is paid its 50; a note of 200 is then counted (400 owed): it is paid the 50 that are held, its own
    // part of 100 over 400; Y, whose part was 50, now has a part of 25 and finds nothing. X has 50 where its part
    // is now 25: what Y lost. T29, in the first class alone: piece (b)'s "second once anything was paid".)
    let two = shard_of(&base, 100 * KAS1, 200 * KAS1, 0, 0, 0).s;
    let (y, big) = (claim_of(&base, xk(&w.c), 100 * KAS1, 0, 0, 0).s, claim_of(&base, xk(&w.f), 200 * KAS1, 0, 0, 0).s);
    let (t1, px, _) = two.claim_paid_at(&x.s, due).unwrap();
    assert_eq!((px, two.claim_paid_at(&y, due).unwrap().1), (50 * KAS1, 50 * KAS1));
    // (Review T7: the note of 200 is now counted by the engine, a mark's note applied after X was paid: of the
    // second class. What is owed of the first class stays 200: Y finds its 50, and the late claim nothing while
    // the first class is not covered. Before: counted by hand in the first class (400 owed), it took the 50 that
    // were held and Y found nothing.)
    let _ = big;
    let mark200 = OLive { s: MarkNote { sid: base.s.sid, leg: fleg(&w, 0, 10, LONG, P10), entry: 3 * P10, key: xk(&w.f), asked: 200 * KAS1 }, value: NOTE_MIN as u64, id: base.id, op: op(62), daa: 6_300 };
    let (b, t2, late200) = mark_apply_c(&w.lib, &live_shard(&base, t1), &mark200, &w.script(mark_script_on(&(&live_shard(&base, t1)).s)), w.payer(&w.f, &ff)).unwrap();
    signed("for (b): a note of 200 counted after a claim was paid a part", &b, &w.keys);
    let late200 = late200.unwrap();
    assert_eq!((t2.owed, t2.claims.owed2, late200.amount1, late200.amount2), (200 * KAS1, 200 * KAS1, 0, 200 * KAS1));
    assert!(t2.claim_paid_at(&late200, due).is_err(), "the late claim is paid before the first class is covered");
    let (t3, py, left) = t2.claim_paid_at(&y, due).unwrap();
    assert_eq!((py, left, t3.held), (50 * KAS1, Some(Claim { paid1: 50 * KAS1, ..y }), 0), "T29 is open: Y, on time and not yet shown, does not find its part");
    // 2.
    let tenth = KAS1 / 10;
    let s = shard_of(&base, tenth + 1_000, tenth, 0, 100 * KAS1, 0);
    let both = claim_of(&base, xk(&w.w), tenth, 0, 100 * KAS1, 0);
    assert_eq!(parts(&s.s, &both.s), Some((tenth, 1_000)));
    assert!(s.s.claim_paid_at(&both.s, due).is_err());
    for o in [outcome(&s.s, &both.s, tenth, 1_000), outcome(&s.s, &both.s, tenth, 0), (s.s, 0, Some(both.s))] {
        assert!(!taken(pay_tx(&w, &s, &both, &o, due), &w.keys), "a claim of both classes whose parts are below the least together is paid");
    }
    let first = claim_of(&base, xk(&w.w), tenth, 0, 0, 0);
    let o = s.s.claim_paid_at(&first.s, due).unwrap();
    assert_eq!((o.1, o.2), (tenth, None));
    assert!(taken(pay_tx(&w, &s, &first, &o, due), &w.keys));
    println!("T6, for piece (b): a claim counted after another was paid whole has 50 of the 150 ever held where its part is 75 (as before the change), and now goes on for the rest; a claim of 0.1 KAS of the first class and 100 KAS of the second waits where the first class is covered");
}

/// F114 as it was found (review 65), in its own numbers, at the engine: a called shard holds 100 KAS against claims
/// of 200 (here 120 and 80); an account that owes pays 60 KAS later, which reach the shard by the note of what was
/// paid (`KpfTOwed.apply`). Before: every claim was paid a half and spent, the shard owed nothing, and `off` gave
/// the 60 to its owner. Now: the claims are paid 60 and 40 and go on; the call does not end; the note is applied
/// (the shard's numbers carried: the script carries the books unopened); the claims are paid 36 and 24 more, their
/// parts of the 160 ever held; the call does not end, with 40 KAS still owed and nothing held: the owner has
/// nothing of the 60. Coins reach a called shard of no row by this note and by its owner's addition (`KpfTAddC`:
/// B3, and `thin_draw.rs`); no other script adds to what such a shard holds (a note that pays in takes a leg out
/// of a row: read).
#[test]
fn t6_f114_what_an_account_pays_late_is_the_claims_and_not_the_owners() {
    let w = world();
    let base = seeded(&w);
    let ff = [fund(xk(&w.f), 36)];
    let pay_s = w.script("KpfTCallPay");
    let call = call_of(&base);
    let s0 = shard_of(&base, 100 * KAS1, 200 * KAS1, 0, 0, 0);
    let due = s0.s.claims_time().unwrap();
    let fl = [coin(xk(&w.f), 36, due)];
    let (c1, c2) = (claim_of(&base, xk(&w.w), 120 * KAS1, 0, 0, 0), OLive { op: op(72), ..claim_of(&base, xk(&w.f), 80 * KAS1, 0, 0, 0) });
    let (b, s1, p1, l1) = claim_pay_c(&w.lib, &s0, &c1, &pay_s, w.payer(&w.f, &fl)).unwrap();
    let sg = signed("F114: the claim of 120 paid a half", &b, &w.keys);
    let (s1, c1) = (nth(&sg, 0, s1, due as u64 + 1), nth(&sg, 2, l1.unwrap(), due as u64 + 1));
    let (b, s2, p2, l2) = claim_pay_c(&w.lib, &s1, &c2, &pay_s, w.payer(&w.f, &fl)).unwrap();
    let sg = signed("F114: the claim of 80 paid a half", &b, &w.keys);
    let (s2, c2) = (nth(&sg, 0, s2, due as u64 + 2), nth(&sg, 2, l2.unwrap(), due as u64 + 2));
    assert_eq!((p1, p2, s2.s.held, s2.s.owed, s2.s.claims.paid1), (60 * KAS1, 40 * KAS1, 0, 200 * KAS1, 100 * KAS1));
    // The call does not end, and the shard does not go to its owner (what F114 was).
    let mut open = s2.s;
    open.called = 0;
    open.owed = 0;
    open.claims = Claims::default();
    assert!(call_off(&w.lib, &s2, &call, &pay_s, w.payer(&w.f, &fl)).is_err());
    assert!(!taken(call_off_as(&w.lib, &s2, &call, open, &pay_s, w.payer(&w.f, &fl)), &w.keys), "F114 is open: the call ends with every claim paid a half");
    // The 60 KAS an account paid of what it owed.
    let note = Owed { sid: base.s.sid, amount: 60 * KAS1 };
    let note = OLive { value: note.value() as u64, s: note, id: base.id, op: op(62), daa: due as u64 + 5 };
    let (b, s3) = owed_apply(&w.lib, &s2, &note, &w.script("KpfTOwed"), w.payer(&w.f, &ff)).unwrap();
    let sg = signed("F114: the note of what an account paid, applied to the called shard", &b, &w.keys);
    assert_eq!((s3.held, s3.owed, s3.claims, s3.called), (60 * KAS1, 200 * KAS1, s2.s.claims, CALLED));
    let s3 = nth(&sg, 0, s3, due as u64 + 10);
    let (b, s4, q1, l1) = claim_pay_c(&w.lib, &s3, &c1, &pay_s, w.payer(&w.f, &ff)).unwrap();
    // (The shard landed after the time of the claims: its own input shows it; the coin landed long before.)
    assert!(matches!(&args_of(&mut b.clone(), 1)[12], Arg::V(ArtifactValue::Int(0))), "the builder names the shard");
    let sg = signed("F114: the claim of 120 paid of what came late", &b, &w.keys);
    assert!(pays(&sg, &xk(&w.w), q1));
    let s4 = nth(&sg, 0, s4, due as u64 + 20);
    let (b, s5, q2, l2) = claim_pay_c(&w.lib, &s4, &c2, &pay_s, w.payer(&w.f, &ff)).unwrap();
    let sg = signed("F114: the claim of 80 paid of what came late", &b, &w.keys);
    assert_eq!((q1, q2, l1.map(|c| c.paid1), l2.map(|c| c.paid1), s5.held, s5.claims.paid1), (36 * KAS1, 24 * KAS1, Some(96 * KAS1), Some(64 * KAS1), 0, 160 * KAS1), "each has its part of the 160 KAS ever held");
    let s5 = nth(&sg, 0, s5, due as u64 + 30);
    let mut open = s5.s;
    open.called = 0;
    open.owed = 0;
    open.claims = Claims::default();
    assert!(call_off(&w.lib, &s5, &call, &pay_s, w.payer(&w.f, &fl)).is_err());
    assert!(!taken(call_off_as(&w.lib, &s5, &call, open, &pay_s, w.payer(&w.f, &fl)), &w.keys), "F114 is open: the call ends with 40 KAS of claims not paid");
    // (And a draw: none while it is called: B3.)
    println!("T6 F114: 100 KAS held against claims of 120 and 80: paid {p1} and {p2}; 60 KAS an account paid later reach the shard: the claims are paid {q1} and {q2} of them; the call does not end and the owner has nothing of them");
}

/// K4 (said, not a fault of the arithmetic). How far below its exact share a part is. `KpfTCallPay`'s own words and
/// 17.8a's have "at most 7 parts in ten million less than its exact share". That is the bound where nearly all
/// that is owed is held. A part is `floor(A h / o)` with h the units ever held (rounded down) and o the units owed
/// (rounded up): it is below the exact share by up to a unit in h and a unit in o, so by about u/W + u/C1 of
/// itself, which grows as less is held. With 3,000,000,001 sompi owed (a unit of a thousand):
///  - half held (1,500,000,999): a claim of all of it has 1,499,999,500, 1,499 sompi (1.0 part in a million) less;
///  - a thousandth held (3,000,999): it has 2,999,999, 1,000 sompi (333 parts in a million) less;
///  - 999 sompi held: nothing (no whole unit).
/// In sompi it is never more than what a unit and a rounding hold: below 2 u for all the claims of a class
/// together and a sompi a claim (300,000 random shards in B1: none paid above its exact share), and what it
/// leaves stays held and is the claims' when more comes. Taken at the engine as the model has them.
#[test]
fn t6_k4_how_far_below_its_exact_share_a_part_is_where_little_is_held() {
    let w = world();
    let base = seeded(&w);
    let due = shard_of(&base, 0, 0, 0, 0, 0).s.claims_time().unwrap();
    let c1 = 3_000_000_001i64;
    let c = claim_of(&base, xk(&w.w), c1, 0, 0, 0);
    let mut seen = vec![];
    for (held, part) in [(1_500_000_999i64, 1_499_999_500i64), (3_000_999, 2_999_999), (999, 0), (c1 - 1, 2_999_999_001), (c1, c1)] {
        let s = shard_of(&base, held, c1, 0, 0, 0);
        assert_eq!(parts(&s.s, &c.s), Some((part, 0)), "{held} held");
        let short = exact(c1, held, c1) - part;
        assert!(short >= 0 && short < 2_000);
        seen.push((held, part, short, short as f64 / exact(c1, held, c1).max(1) as f64));
        match s.s.claim_paid_at(&c.s, due) {
            Ok(o) => {
                assert_eq!(o.1, part);
                assert!(taken(pay_tx(&w, &s, &c, &o, due), &w.keys), "{held} held");
            }
            Err(_) => assert!(part < NOTE_MIN),
        }
    }
    assert!(seen[0].3 > 0.7e-6 && seen[1].3 > 300e-6, "K4: the shortfall is within 7 parts in ten million everywhere: {seen:?}");
    println!("T6 K4: 3,000,000,001 sompi owed, a claim of all of it: (held, its part, sompi below its exact share, as a part of it) {seen:?}");
}

/// K3 (the tool, not a script). What is kept of a shard and of a claim (`aobj.rs`).
///  - CLOSED (second round): a shard kept BEFORE this change (its terms, rows and moving part, and nothing of the
///    three numbers) reads back, as the shard of no numbers it is; and a shard that has none is kept in those same
///    bytes, so what was kept before and what is kept now are one form. (It was refused: `encode` wrote 27 bytes
///    more than the file had.) The same for a statement kept on such a shard.
///  - A shard with numbers that are not nothing is kept with them, 27 bytes more, and reads back.
///  - One form only: the bytes of a shard of no numbers with 27 bytes of nothing after them (what the first
///    round's tree wrote for every shard) are refused, as anything is that the object does not write itself.
///  - STILL SO: a claim partly paid has no form: a note of the rest is kept by its key and one amount, and no
///    amount makes the hash of (A1, a1 above nothing, 0, 0) (run for the two amounts a tool could try). So a
///    claim that `KpfTCallPay.claim` wrote again is kept by nothing. (And no command pays a claim or ends a call:
///    read, `kpf-cli/src`.)
#[test]
fn t6_k3_closed_a_shard_kept_before_the_three_numbers_reads_back_and_a_claim_partly_paid_has_no_form() {
    use kpf_protocol::aobj::{AObj, Host};
    let w = world();
    let base = seeded(&w);
    // As it was kept before the change: the kind, the terms, the rows, the moving part.
    let before = [vec![21u8], base.s.terms(), base.s.rows_bytes(), base.s.moving()].concat();
    assert_eq!(AObj::decode(&before).expect("K3 is open: a shard kept before the claims' numbers is not read back"), AObj::Shard(base.s));
    assert_eq!(AObj::Shard(base.s).encode(), before, "a shard of no numbers is kept as before they were");
    let called = Shard { called: CALLED, owed: 30 * KAS1, ..base.s };
    assert_eq!(AObj::decode(&AObj::Shard(called).encode()).unwrap(), AObj::Shard(called));
    assert_eq!(AObj::Shard(called).encode().len(), before.len(), "called, nothing paid yet: no numbers kept");
    // With numbers.
    for (d1, c2, d2) in [(12 * KAS1, 20 * KAS1, 0), (1, 0, 0), (0, 1, 0), (0, 0, 1), (30 * KAS1, 20 * KAS1, 20 * KAS1)] {
        let s = shard_of(&base, 7 * KAS1, 30 * KAS1, d1, c2, d2).s;
        let kept = AObj::Shard(s).encode();
        assert_eq!((kept.len(), &kept[kept.len() - 27..]), (before.len() + 27, &s.claims.bytes()[..]));
        assert_eq!(AObj::decode(&kept).unwrap(), AObj::Shard(s));
    }
    assert!(AObj::decode(&[before.clone(), Claims::default().bytes()].concat()).is_err(), "one form: numbers of nothing are not written");
    assert!(AObj::decode(&before[..before.len() - 1]).is_err() && AObj::decode(&[before.clone(), vec![8u8; 9]].concat()).is_err());
    // A statement kept on a shard of no numbers, and on one with numbers: both read back.
    for s in [base.s, shard_of(&base, 7 * KAS1, 30 * KAS1, 12 * KAS1, 0, 0).s] {
        let d = Draw { amount: KAS1, prices: [0; 4].into() };
        let said = s.traced(&d, false).expect("a trace").said();
        let outs = Outs::of(&[Out { key: false, to: s.drawn(KAS1, T0).hash(), value: s.held + SHARD_CARRIER - KAS1 }, Out { key: true, to: xk(&w.c), value: KAS1 }]).unwrap();
        let st = Stated { position: s.hash(), window: s.dispute_daa, late: s.t.late_daa, kind: KIND_DRAW, t0: T0, arg: blake2b(&said), outs, bond: s.bond(), claimer: xk(&w.c), held: s.held + SHARD_CARRIER };
        let kept = AObj::Stated { on: Host::Shard(s), st, said: said.clone() };
        assert_eq!(AObj::decode(&kept.encode()).unwrap(), kept);
    }
    // A claim partly paid: no form.
    let c = claim_of(&base, xk(&w.w), 30 * KAS1, 25 * KAS1, 0, 0).s;
    for amount in [30 * KAS1, 5 * KAS1] {
        let kept = Rest { sid: c.sid, key: c.key, amount };
        assert_eq!(AObj::decode(&AObj::Rest(kept).encode()).unwrap(), AObj::Rest(kept));
        assert_ne!(kept.hash(), c.hash());
    }
}
