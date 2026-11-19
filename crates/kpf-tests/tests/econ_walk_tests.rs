//! The economic walk of the oracle-market family (docs/DESIGN-ORACLE-IMPL.md 29.1; review 11, R11-F5). It is the walk of
//! review 11 (`audit11_walk_tests.rs`: partial hand-overs, mutual and cooperative closes, look-back and late claims,
//! claims vouched by another key, multi-staker disputes, the slot's hostile steps) with one change of method: every side
//! is valued at a simulated FEED price, not at the entry. The feed is one seeded price path for the market (volatile or
//! calm by seed, with gaps and closed sessions); the walk's judge decides a claim true by the feed definition's rule (the
//! two-moment rule of 29.3 on the fixed templates, the price at t0 alone on the older ones).
//!
//! * E-party: in an action step a party does not sign, its value at the present feed (its plain outputs less what it paid
//!   in, plus its side of every live pair at the feed) falls only by a listed cause, each with its bound (29.1): a
//!   renewal's premium, a true liquidation of its side, the keeper fee beyond the loser's value, the rounding of a split or
//!   hand-over, the payout floor, a lapse at the entry that its own close could have avoided, a false claim the judge
//!   upheld (outside the invariant: the judge's failure). The largest loss of each cause is reported against its bound.
//! * E-exit: from the walk's states each party of each live pair can close its own side at the feed by itself (no
//!   underwriter, no counterparty) within quietDaa, finalized W later, paid its value at the feed moved by at most g; a
//!   probe builds that close and its finalize in the engine (a real check, not the lapse date's arithmetic).
//! * E-claim: an honest claim true at t0 and still true when it lands never costs its proposer its bond, however late it
//!   lands (delays up to and beyond lateDaa, across the last valid landing before the expiry).
//! * I-order, I-winner, I-uw, I-staker as in 28.1; the lapse date check of I-exit stays.
//!
//! Every transaction runs every input in the real script engine with the covenant context (`common::run`), the templates
//! compiled from source. The walk records violations instead of stopping, so a run on older templates lists what fails;
//! it detects which rules the templates carry by probing a party's own close once.

mod common;
mod oraclekit;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use kaspa_consensus_core::tx::TransactionOutpoint;
use kaspa_consensus_core::Hash;
use kpf_protocol::bond::{BindArgs, Bond, Claim, Ctx};
use kpf_protocol::judge::{NO, UNIT, YES};
use kpf_protocol::oracle::*;
use oraclekit::*;
use secp256k1::Keypair;

const KAS: i64 = 100_000_000;
/// The contracts' payout floor (`PLAIN_DUST`).
const DUST: i64 = 2_000_000;
/// The least margin of the order in every part it is filled or taken in, and in what it leaves (`KpfOpenO.PART_FLOOR`,
/// review 10 fix): ten payout floors.
const PART_FLOOR: i64 = kpf_protocol::oracle::PART_FLOOR;

const ATTACKER: u8 = 77;
const NOT_PARTIES: [u8; 5] = [31, 60, 70, 71, ATTACKER];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Inv {
    Order,
    Party,
    Winner,
    Exit,
    Claim,
    Uw,
    Staker,
}

// ---------------------------------------------------------------- the feed

/// The walk's simulated feed for one market: a seeded price path (one price per tick), closed sessions in which the
/// price is frozen and no claim is true, and gaps at some ticks and at every reopening.
#[derive(Clone, Debug)]
struct Feed {
    path: Vec<(u64, i64)>,
    closed: Vec<(u64, u64)>,
    calm: bool,
}

impl Feed {
    fn new(calm: bool, start: u64) -> Feed {
        Feed { path: vec![(start, 1_000_000_000)], closed: vec![], calm }
    }
    /// The feed at DAA `d`: the last price published at or before it.
    fn at(&self, d: u64) -> i64 {
        let i = self.path.partition_point(|(t, _)| *t <= d);
        self.path[i.saturating_sub(1)].1
    }
    fn now(&self) -> i64 {
        self.path.last().unwrap().1
    }
    fn closed_at(&self, d: u64) -> bool {
        self.closed.iter().any(|(a, b)| d >= *a && d < *b)
    }
    /// The feed definition's rule for one moment: the session is open and the feed lies within g of the claimed price.
    fn true_at(&self, p: i64, d: u64, g: i64) -> bool {
        let f = self.at(d) as i128;
        let (p, g) = (p as i128, g as i128);
        !self.closed_at(d) && f * 10_000 >= p * (10_000 - g) && f * 10_000 <= p * (10_000 + g)
    }
    fn tick(&mut self, rng: &mut Rng, now: u64) {
        let last = self.now();
        let was_closed = self.closed_at(now.saturating_sub(1)) && !self.closed_at(now);
        if self.closed_at(now) {
            self.path.push((now, last));
            return;
        }
        let mut f = last as i128;
        // A move per tick: up to 2 % on a volatile seed, 0.2 % on a calm one.
        let w = if self.calm { 20 } else { 200 };
        f += f * (rng.below(2 * w + 1) as i128 - w as i128) / 10_000;
        // A gap: at a reopening, or now and then (up to 6 %).
        if was_closed || rng.chance(1) {
            f += f * (rng.below(1_201) as i128 - 600) / 10_000;
        }
        // A closed session (a weekend) starts now and then: 300 to 800 DAA.
        if rng.chance(2) {
            let a = now + 1;
            self.closed.push((a, a + 300 + rng.below(500)));
        }
        self.path.push((now, (f as i64).clamp(500_000_000, 2_000_000_000)));
    }
}

/// A cause of a loss at the feed that E-party allows (29.1), with what the walk measured against its bound.
#[derive(Default, Debug, Clone, Copy)]
struct CauseBook {
    times: usize,
    loss: i64,
    max_loss: i64,
    bound_at_max: i64,
    max_bound: i64,
}

// ---------------------------------------------------------------- builders the kits do not have

fn slot_ctx(sl: &LSlot, lock: u64) -> Ctx {
    Ctx { lock: lock as i64, seq: sl.s.m.p.rate, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true }
}

/// The order's continuation (or its refund) after a part of `n` that took `spend`.
fn order_rest(lo: &LOrder, n: i64, spend: i64) -> (kaspa_consensus_core::tx::TransactionOutput, Option<OOpen>) {
    let o = &lo.s;
    let left = o.size_left - n;
    let rest = (lo.value as i64 - spend).max(0) as u64;
    if left > 0 {
        let c = OOpen { size_left: left, ..o.clone() };
        (out(rest, order_spk(&c), bind(0, lo.id.unwrap())), Some(c))
    } else {
        (out(rest, key_spk(&o.owner), None), None)
    }
}

/// `KpfOpenO.take` of `n` units of `lo` with no pair (the counterparty `ck` funds the rest of the part and authors its
/// genesis): in 0 the order, 1 the slot (bind at `xe` for claim OUT_REF + 1, advancing `adv`), 2 the counterparty's
/// funding; out 0 the order's rest, 1 the part, 2 the slot, 3 the counterparty's change. The order pays the model's
/// `take_spend(n)` plus `extra` (a hostile variant asks one sompi more). Returns the transaction, the part, the slot and
/// the order's continuation.
#[allow(clippy::too_many_arguments)]
fn take_part(lo: &LOrder, n: i64, sl: &LSlot, ck: &Keypair, xp: i64, e: i64, bound: i64, xe: i64, cr: i64, prem: i64, adv: i64, lock: u64, fop: TransactionOutpoint, extra: i64) -> (T, OPair, Bond, Option<OOpen>) {
    let k = kit();
    let o = &lo.s;
    let sc = o.m.p.scale;
    let (bl, bs, lk, sk) = if o.side == L { (xp - o.my_rate, bound, o.owner, x(ck)) } else { (bound, xp + o.my_rate, x(ck), o.owner) };
    let mut part = OPair {
        m: o.m,
        size: n,
        bl,
        bs,
        entry: e,
        long_key: lk,
        short_key: sk,
        b: OBinding { uw_id: sl.idb(), uw_key: sl.s.owner, cover_rate: cr, expiry: xe, premium: prem },
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers::default(),
    };
    if o.side == L {
        part.mk.long = maker_of(&lo.op);
    } else {
        part.mk.short = maker_of(&lo.op);
    }
    let a = BindArgs::new(lock as i64, xe, adv, &[Claim::output(1, q(n, cr, sc))]);
    let b2 = sl.s.bind(&slot_ctx(sl, lock), &a, sl.value as i64).unwrap_or_else(|_| sl.s.clone());
    let args = vec![iv(1), iv(n), iv(xp), iv(e), iv(bound), bv(&x(ck)), bv(&[0; 32]), iv(0), iv(0), iv(0), iv(1), iv(0), iv(xe), iv(cr), iv(prem), bv(&k.pair.prefix), bv(&k.pair.suffix)];
    let fv = 1_000_000_000_000i64;
    let ins = vec![order_in(lo, "take", args), slot_in(sl, "bind", bind_args(&a, &uw_kp())), key_in(fop, fv as u64, 0, ck)];
    let spend = o.take_spend(n) + extra;
    let pv = part.floor();
    let (first, cont) = order_rest(lo, n, spend);
    let mut outs = vec![first, out(pv as u64, pair_spk(&part), None), out(sl.value, bond_spk(&b2), bind(1, sl.id.unwrap())), out((fv + spend - pv) as u64, p2pk_spk(ck), None)];
    seal(&mut outs, 1, 2, fop);
    (build_ext(&ins, outs, lock, &[0, sl.s.m.p.rate as u64, 0]), part, b2, cont)
}

/// `KpfOpenO.fill` of `n` units of `lo` alone (the counterparty `ck` is a signing key, not an order: its maker field is
/// zero): in 0 the order, 1 the slot (bind at `xe` for OUT_REF + 1), 2 the counterparty's funding (it authors the pair's
/// genesis and pays its half of the premium); out 0 the order's rest, 1 the pair, 2 the slot, 3 the premium to the slot's
/// owner, 4 change. The order pays the model's `fill_spend(n, prem)` plus `extra`.
#[allow(clippy::too_many_arguments)]
fn fill_part(lo: &LOrder, n: i64, sl: &LSlot, ck: &Keypair, xp: i64, cbound: i64, xe: i64, cr: i64, prem: i64, adv: i64, lock: u64, fop: TransactionOutpoint, extra: i64) -> (T, OPair, Bond, Option<OOpen>) {
    let k = kit();
    let o = &lo.s;
    let sc = o.m.p.scale;
    let (bl, bs, lk, sk) = if o.side == L { (xp - o.my_rate, cbound, o.owner, x(ck)) } else { (cbound, xp + o.my_rate, x(ck), o.owner) };
    let mut pair = OPair {
        m: o.m,
        size: n,
        bl,
        bs,
        entry: xp,
        long_key: lk,
        short_key: sk,
        b: OBinding { uw_id: sl.idb(), uw_key: sl.s.owner, cover_rate: cr, expiry: xe, premium: prem },
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers::default(),
    };
    if o.side == L {
        pair.mk.long = maker_of(&lo.op);
    } else {
        pair.mk.short = maker_of(&lo.op);
    }
    let a = BindArgs::new(lock as i64, xe, adv, &[Claim::output(1, q(n, cr, sc))]);
    let b2 = sl.s.bind(&slot_ctx(sl, lock), &a, sl.value as i64).unwrap_or_else(|_| sl.s.clone());
    let args = vec![iv(1), iv(n), iv(xp), iv(bl), iv(bs), bv(&x(ck)), bv(&[0; 32]), iv(1), iv(0), iv(xe), iv(cr), iv(prem), bv(&k.pair.prefix), bv(&k.pair.suffix)];
    let fv = 1_000_000_000_000i64;
    let ins = vec![order_in(lo, "fill", args), slot_in(sl, "bind", bind_args(&a, &uw_kp())), key_in(fop, fv as u64, 0, ck)];
    let spend = o.fill_spend(n, prem) + extra;
    let pv = pair.floor();
    let pm = q(n, prem, sc).max(DUST);
    let (first, cont) = order_rest(lo, n, spend);
    let mut outs = vec![
        first,
        out(pv as u64, pair_spk(&pair), None),
        out(sl.value, bond_spk(&b2), bind(1, sl.id.unwrap())),
        out(pm as u64, key_spk(&sl.s.owner), None),
        out((fv + spend - pv - pm) as u64, p2pk_spk(ck), None),
    ];
    seal(&mut outs, 1, 2, fop);
    (build_ext(&ins, outs, lock, &[0, sl.s.m.p.rate as u64, 0]), pair, b2, cont)
}

/// `renew_tx` for any market (the slot's own rate limit as its sequence).
fn renew_at(lp: &LPair, sl: &LSlot, lock: u64, advance: i64) -> (T, OPair, Bond) {
    let (r, pay) = lp.s.renewed().unwrap_or((lp.s.clone(), 0));
    let cover = lp.s.cover_of(lp.s.size);
    let a = BindArgs::new(lock as i64, lp.s.b.expiry + lp.s.m.bond.term, advance, &[Claim::input(0, cover)]);
    let b2 = sl.s.bind(&slot_ctx(sl, lock), &a, sl.value as i64).unwrap_or_else(|_| sl.s.clone());
    let ins = vec![pair_in(lp, "renew", vec![iv(1), iv(0)]), slot_in(sl, "bind", bind_args(&a, &uw_kp())), exec_in(219)];
    let outs = vec![
        out(lp.value - pay as u64, pair_spk(&r), bind(0, lp.id.unwrap())),
        out(sl.value, bond_spk(&b2), bind(1, sl.id.unwrap())),
        out((pay as u64).max(DUST as u64), key_spk(&sl.s.owner), None),
        change(90_000_000_000),
    ];
    (build_ext(&ins, outs, lock, &[0, sl.s.m.p.rate as u64, 0]), r, b2)
}

/// The renewal advance a slot needs to bind expiry `x2`.
fn advance_to(sl: &Bond, x2: i64) -> i64 {
    (x2 / sl.m.p.step - 12 - sl.j0).max(0)
}

/// A side's claimable value in a pair: its payout at the lapse settle before the payout floor (the entry clamped to the
/// range; the short gets the rest of the UTXO's value).
fn lapse_values(p: &OPair, value: u64) -> (i64, i64) {
    let l = q(p.size, p.entry.clamp(p.bl, p.bs) - p.bl, p.m.p.scale);
    (l, value as i64 - l)
}

fn own_lv(p: &OPair, value: u64, side: i64) -> i64 {
    let (l, s) = lapse_values(p, value);
    if side == L {
        l
    } else {
        s
    }
}

/// The rule of `KpfOpenO.amount` for a part of `n`.
fn part_rule(o: &OOpen, n: i64) -> bool {
    o.part_ok(n)
}

fn short_id(k: &[u8; 32]) -> String {
    k[..3].iter().map(|b| format!("{b:02x}")).collect()
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
    fn around(&mut self, c: i64, w: i64) -> i64 {
        let w = w.max(1);
        if self.chance(10) {
            c + (self.below(20 * w as u64) as i64 - 10 * w)
        } else {
            c + (self.below(2 * w as u64 + 1) as i64 - w)
        }
    }
}

#[derive(Clone)]
struct WP {
    lp: LPair,
    b0: OBinding,
    renewals: Vec<u64>,
    born: u64,
    silent: bool,
}

#[derive(Default, Debug)]
struct WBooks {
    steps: BTreeMap<&'static str, usize>,
    refused_as_ruled: usize,
    hostile: BTreeMap<&'static str, usize>,
    checks: BTreeMap<Inv, usize>,
    residuals: BTreeMap<&'static str, usize>,
    /// The longest a liquidatable pair of an on-time underwriter waited for a claim that can be valid (DAA).
    max_claim_delay: u64,
    /// Orders taken or filled in parts by an attacker, the parts, and the sompi lost beyond 28.1's bound (0 when it holds).
    orders: usize,
    parts: usize,
    order_excess: i64,
    /// Review 11: (count, sum, max) of a measured quantity in sompi.
    metrics: BTreeMap<&'static str, (usize, i64, i64)>,
    /// E-party: every cause of a loss at the feed, what it took and its bound.
    causes: BTreeMap<&'static str, CauseBook>,
}

struct Walk {
    rng: Rng,
    seed: u64,
    step: usize,
    now: u64,
    slot: LSlot,
    slot_value: u64,
    pairs: Vec<WP>,
    keys: BTreeMap<[u8; 32], Keypair>,
    spks: BTreeMap<Vec<u8>, [u8; 32]>,
    next: u32,
    books: WBooks,
    violations: Vec<(u64, usize, Inv, String)>,
    /// The claimable-value ledger: plain outputs received less plain inputs spent, per key.
    cash: BTreeMap<[u8; 32], i64>,
    actors: BTreeSet<[u8; 32]>,
    allow: BTreeMap<[u8; 32], Vec<(&'static str, i64)>>,
    feed: Feed,
    /// The templates carry the rules of 29.3 (a party's own close is accepted): the judge then applies the two-moment
    /// rule, closes need no vouch, a void liquidation refunds its proposer.
    new_rules: bool,
}

/// Whether the templates accept a party's own close (no underwriter input): the rules of 29.3.
fn self_close_works() -> bool {
    let (lp, _, _) = opened();
    let lp = Live { daa: 0, ..lp };
    accepted(&self_close_tx(&lp, L, lp.s.size, lp.s.entry, OPEN_DAA as i64, OPEN_DAA, &long_kp()).0)
}

impl Walk {
    fn new(seed: u64) -> Walk {
        let new_rules = self_close_works();
        let mut w = Walk {
            feed: Feed::new(seed % 3 == 0, 300),
            new_rules,
            rng: Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1),
            seed,
            step: 0,
            now: 300,
            slot: slot(20_000_000_000_000),
            slot_value: 20_000_000_000_000,
            pairs: vec![],
            keys: BTreeMap::new(),
            spks: BTreeMap::new(),
            next: 0,
            books: WBooks::default(),
            violations: vec![],
            cash: BTreeMap::new(),
            actors: BTreeSet::new(),
            allow: BTreeMap::new(),
        };
        for s in [41u8, 42, 45, 46, 47, 48, 49, 50, 31, 60, 70, 71, ATTACKER] {
            w.add_key(keypair(s));
        }
        w
    }
    fn add_key(&mut self, kp: Keypair) {
        let k = x(&kp);
        self.keys.insert(k, kp);
        self.spks.insert(key_spk(&k).script().to_vec(), k);
    }
    fn fresh_op(&mut self) -> (TransactionOutpoint, Hash) {
        self.next += 1;
        let mut b = [0x5au8; 32];
        b[..4].copy_from_slice(&self.next.to_le_bytes());
        b[4..12].copy_from_slice(&self.seed.to_le_bytes());
        (TransactionOutpoint { transaction_id: kaspa_consensus_core::tx::TransactionId::from_bytes(b), index: self.next }, Hash::from_bytes(b))
    }
    fn count(&mut self, what: &'static str) {
        *self.books.steps.entry(what).or_default() += 1;
    }
    fn metric(&mut self, what: &'static str, v: i64) {
        let e = self.books.metrics.entry(what).or_default();
        e.0 += 1;
        e.1 += v;
        e.2 = e.2.max(v);
    }
    fn residual(&mut self, what: &'static str) {
        *self.books.residuals.entry(what).or_default() += 1;
    }
    fn checked(&mut self, i: Inv) {
        *self.books.checks.entry(i).or_default() += 1;
    }
    fn key(&self, k: &[u8; 32]) -> Keypair {
        *self.keys.get(k).expect("a known key")
    }
    fn entrant(&mut self) -> Keypair {
        let ks = [45u8, 46, 47, 48, 49, 50, 41, 42, 31];
        keypair(ks[self.rng.below(ks.len() as u64) as usize])
    }
    fn party_key(&mut self, pool: &[u8]) -> Keypair {
        keypair(pool[self.rng.below(pool.len() as u64) as usize])
    }
    fn violation(&mut self, i: Inv, what: String) {
        self.violations.push((self.seed, self.step, i, what));
    }
    fn quiet_ok(&self, lp: &LPair) -> bool {
        self.now >= lp.daa + lp.s.m.p.quiet_daa as u64
    }
    fn hostile(&mut self, what: &'static str, t: &T) {
        assert!(!accepted(t), "seed {} step {}: hostile variant accepted: {what}", self.seed, self.step);
        *self.books.hostile.entry(what).or_default() += 1;
    }
    fn hostile_input(&mut self, what: &'static str, t: &T, i: usize) {
        assert!(!verdicts(t)[i], "seed {} step {}: hostile variant accepted by input {i}: {what}", self.seed, self.step);
        *self.books.hostile.entry(what).or_default() += 1;
    }

    // ------------------------------------------------------------ the claimable-value ledger (I-party)

    /// Books an applied transaction: every plain output to a known key is received, every plain input of one spent.
    fn book(&mut self, t: &T) {
        for e in &t.1 {
            if let Some(k) = self.spks.get(e.script_public_key.script()).copied() {
                *self.cash.entry(k).or_default() -= e.amount as i64;
            }
        }
        for o in &t.0.outputs {
            if let Some(k) = self.spks.get(o.script_public_key.script()).copied() {
                *self.cash.entry(k).or_default() += o.value as i64;
            }
        }
    }
    fn is_party(k: &[u8; 32]) -> bool {
        !NOT_PARTIES.iter().any(|s| x(&keypair(*s)) == *k)
    }
    /// Every party's value at the feed price `f`: its plain outputs less its plain inputs, plus its side of every live
    /// pair at `f` (the long `q(s, clamp(f, bl, bs) - bl)`, the short the rest of the UTXO's value).
    fn ledger_at(&self, f: i64) -> BTreeMap<[u8; 32], i64> {
        let mut v: BTreeMap<[u8; 32], i64> = self.cash.iter().filter(|(k, _)| Self::is_party(k)).map(|(k, c)| (*k, *c)).collect();
        for w in &self.pairs {
            let l = w.lp.s.long_at(f);
            *v.entry(w.lp.s.long_key).or_default() += l;
            *v.entry(w.lp.s.short_key).or_default() += w.lp.value as i64 - l;
        }
        v.retain(|k, _| Self::is_party(k));
        v
    }
    fn actor(&mut self, k: [u8; 32]) {
        self.actors.insert(k);
    }
    /// E-party: a loss of `k` the step may cause, its cause and its bound at the feed.
    fn allow(&mut self, k: [u8; 32], cause: &'static str, bound: i64) {
        self.allow.entry(k).or_default().push((cause, bound.max(0)));
    }
    /// Runs one action and checks E-party at the present feed price (the feed does not move inside an action): every
    /// party that did not act lost no more than the causes the action allows it, each within its bound.
    fn guarded(&mut self, what: &'static str, f: impl FnOnce(&mut Walk)) {
        let price = self.feed.at(self.now);
        let before = self.ledger_at(price);
        self.actors.clear();
        self.allow.clear();
        f(self);
        let after = self.ledger_at(price);
        let keys: BTreeSet<[u8; 32]> = before.keys().chain(after.keys()).copied().collect();
        for k in keys {
            if self.actors.contains(&k) {
                continue;
            }
            let d = after.get(&k).copied().unwrap_or(0) - before.get(&k).copied().unwrap_or(0);
            let causes = self.allow.get(&k).cloned().unwrap_or_default();
            let a: i64 = causes.iter().map(|(_, b)| *b).sum();
            self.checked(Inv::Party);
            if d < 0 {
                // The loss is booked against the cause with the largest bound (the one that explains it).
                let main = causes.iter().max_by_key(|(_, b)| *b).map(|(c, b)| (*c, *b)).unwrap_or(("no cause", 0));
                let e = self.books.causes.entry(main.0).or_default();
                e.times += 1;
                e.loss += -d;
                e.max_bound = e.max_bound.max(main.1);
                if -d > e.max_loss {
                    e.max_loss = -d;
                    e.bound_at_max = a;
                }
            }
            if d < -a {
                let list: Vec<String> = causes.iter().map(|(c, b)| format!("{c} {b}")).collect();
                self.violation(Inv::Party, format!("{what}: party {} lost {} sompi at the feed {price} without signing (allowed {a}: {})", short_id(&k), -d, list.join(", ")));
            }
        }
        // I-uw: no step but the owner's own entries changes the slot's value.
        self.checked(Inv::Uw);
        if self.slot.value != self.slot_value || self.slot.id != Some(SLOT_ID) {
            self.violation(Inv::Uw, format!("{what}: the slot's value or id changed"));
        }
    }

    fn check_bindings(&self) {
        for w in &self.pairs {
            let mut b = w.b0;
            b.expiry += w.renewals.len() as i64 * w.lp.s.m.bond.term;
            assert_eq!(w.lp.s.b, b, "seed {} step {}: a binding changed without a renewal", self.seed, self.step);
            assert!(w.lp.value as i64 >= w.lp.s.floor(), "seed {} step {}: a pair below its floor", self.seed, self.step);
            assert!(w.lp.s.bs - w.lp.s.bl <= w.lp.s.b.cover_rate, "seed {} step {}: range above the cover rate", self.seed, self.step);
        }
    }

    // ------------------------------------------------------------ the walk's ordinary steps (as review 10's walk)

    fn open(&mut self) {
        let m = omarket();
        let xe = (((self.now + 800 + 99) / 100) * 100) as i64;
        let adv = (xe / 100 - self.slot.s.j0 - 12).max(0);
        let mut sp = open_spec();
        sp.lock = self.now;
        sp.xe = xe;
        sp.advance = adv;
        // Opened at the feed (rounded to 0.01 KAS), each order's limit 0.5 KAS beyond it.
        let fx = self.price() / 1_000_000 * 1_000_000;
        sp.x = fx;
        let (op1, id1) = self.fresh_op();
        let (op2, id2) = self.fresh_op();
        let (lk, sk) = (self.party_key(&[41, 45, 46]), self.party_key(&[42, 47, 48]));
        let mk = |op, id, side, kp: &Keypair, daa| Live { op, value: 2_000_000_000_000u64, daa, id: Some(id), s: OOpen { limit: if side == L { fx + 50_000_000 } else { fx - 50_000_000 }, ..sample_order(&m, side, x(kp)) } };
        let (lo, so) = (mk(op1, id1, L, &lk, self.now), mk(op2, id2, S, &sk, self.now));
        let (t, p, b2) = open_tx(&lo, &so, &self.slot, &sp, &|_, _| {});
        if !accepted(&t) {
            self.books.refused_as_ruled += 1;
            return;
        }
        self.actor(x(&lk));
        self.actor(x(&sk));
        self.book(&t);
        self.count("open");
        let lp = live(&t, 2, self.now, &p);
        self.slot = live(&t, 3, self.now, &b2);
        let silent = self.rng.chance(25);
        self.pairs.push(WP { lp, b0: p.b, renewals: vec![], born: self.now, silent });
    }

    fn tick(&mut self) {
        self.now += 10 + self.rng.below(140);
        let now = self.now;
        self.feed.tick(&mut self.rng, now);
    }

    /// The feed now.
    fn price(&self) -> i64 {
        self.feed.at(self.now)
    }

    fn ensure_listed(&mut self, i: usize, side: i64) {
        let w = self.pairs[i].clone();
        let listed = if side == L { w.lp.s.long_ask > 0 } else { w.lp.s.short_bid > 0 };
        if listed || !self.quiet_ok(&w.lp) {
            return;
        }
        let ask = self.rng.around(self.price(), 20_000_000).max(1);
        let kp = self.key(if side == L { &w.lp.s.long_key } else { &w.lp.s.short_key });
        let (t, c) = amend_tx(&w.lp, side, &kp, ask, w.lp.s.m.p.quiet_daa as u64);
        must("list before a hand-over", &t);
        self.actor(x(&kp));
        self.book(&t);
        self.pairs[i].lp = live(&t, 0, self.now, &c);
        self.now += w.lp.s.m.p.quiet_daa as u64;
    }

    fn amend(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        if !self.quiet_ok(&w.lp) {
            return;
        }
        let side = if self.rng.chance(50) { L } else { S };
        let ask = self.rng.around(self.price(), 30_000_000).max(1);
        let kp = self.key(if side == L { &w.lp.s.long_key } else { &w.lp.s.short_key });
        let (t, c) = amend_tx(&w.lp, side, &kp, ask, w.lp.s.m.p.quiet_daa as u64);
        let other = self.key(if side == L { &w.lp.s.short_key } else { &w.lp.s.long_key });
        if other.x_only_public_key() != kp.x_only_public_key() {
            let (h, _) = amend_tx(&w.lp, side, &other, ask, w.lp.s.m.p.quiet_daa as u64);
            self.hostile("amend signed by the other party", &h);
        }
        if ask > w.lp.s.m.p.price_max {
            assert!(!accepted(&t));
            return;
        }
        must("amend", &t);
        self.actor(x(&kp));
        self.book(&t);
        self.count("amend");
        self.pairs[i].lp = live(&t, 0, self.now, &c);
    }

    fn add_margin(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        if !self.quiet_ok(&w.lp) {
            return;
        }
        let p = &w.lp.s;
        let side = if self.rng.chance(50) { L } else { S };
        let head = p.b.cover_rate - (p.bs - p.bl);
        let delta = 1 + self.rng.below((2 * head).max(2) as u64) as i64;
        let kp = self.key(if side == L { &p.long_key } else { &p.short_key });
        let mut c = OPair { mk: Makers::default(), ..p.clone() };
        if side == L {
            c.bl -= delta;
        } else {
            c.bs += delta;
        }
        let sc = p.m.p.scale;
        let add = kpf_protocol::payoff::q(p.size, delta, sc, sc - 1);
        let ins = vec![pair_in(&w.lp, "addMargin", vec![Arg::Sig(kp), iv(side), iv(delta)]), key_in(outpoint(0x92), 1_000_000_000_000, 0, &kp)];
        let outs = vec![out(w.lp.value + add as u64, pair_spk(&c), bind(0, w.lp.id.unwrap())), out(1_000_000_000_000 - add as u64 - 10_000, p2pk_spk(&kp), None)];
        let t = build_ext(&ins, outs, 0, &[seq_quiet(&w.lp), 0]);
        let share_ok = if side == L { c.bl >= p.growth_limit(L) } else { c.bs <= p.growth_limit(S) };
        let legal = c.bl >= 0 && c.bl < c.bs && c.bs <= p.m.p.price_max && c.bs - c.bl <= p.b.cover_rate && share_ok;
        assert_eq!(accepted(&t), legal, "seed {} step {}: addMargin delta {delta} head {head}", self.seed, self.step);
        if legal {
            self.actor(x(&kp));
            self.book(&t);
            self.count("addMargin");
            self.pairs[i].lp = live(&t, 0, self.now, &c);
        } else {
            self.books.refused_as_ruled += 1;
        }
    }

    fn hand_over_rule(p: &OPair, side: i64, xp: i64, nb: i64, ek: &[u8; 32]) -> bool {
        let mm = p.m.p.mmr_bps;
        xp >= 0
            && xp <= p.m.p.price_max
            && *ek != p.b.uw_key
            && if side == L {
                p.long_ask > 0
                    && xp >= p.long_ask
                    && nb >= 0
                    && nb <= p.bl
                    && (xp as i128) * (10_000 - mm) as i128 >= (nb as i128) * 10_000
                    && p.bs - nb <= p.b.cover_rate
                    && (nb == p.bl || nb >= p.growth_limit(L))
            } else {
                p.short_bid > 0
                    && xp <= p.short_bid
                    && nb >= p.bs
                    && nb <= p.m.p.price_max
                    && (nb as i128) * 10_000 >= (xp as i128) * (10_000 + mm) as i128
                    && nb - p.bl <= p.b.cover_rate
                    && (nb == p.bs || nb <= p.growth_limit(S))
            }
    }

    fn transfer_custom(lp: &LPair, side: i64, xp: i64, nb: i64, ek: &Keypair, part: &OPair, pay: i64) -> T {
        let p = &lp.s;
        let leaver = if side == L { p.long_key } else { p.short_key };
        let args = vec![iv(side), iv(p.size), iv(xp), iv(nb), bv(&x(ek)), bv(&[0; 32]), iv(1)];
        let ins = vec![pair_in(lp, "transfer", args), key_in(outpoint(223), 1_000_000_000_000, 0, ek)];
        let keep = lp.value as i64 - p.range_of(p.size) + if pay < DUST { pay } else { 0 };
        let pv = part.range_of(p.size) + keep;
        let mut outs = vec![out(pay.max(DUST) as u64, key_spk(&leaver), None), out(pv as u64, pair_spk(part), None), out(100_000_000_000, p2pk_spk(ek), None)];
        seal(&mut outs, 1, 0, lp.op);
        build_ext(&ins, outs, 0, &[p.m.p.quiet_daa as u64, 0])
    }

    fn transfer(&mut self, i: usize) {
        let ls = if self.rng.chance(50) { L } else { S };
        self.ensure_listed(i, ls);
        let w = self.pairs[i].clone();
        if !self.quiet_ok(&w.lp) {
            return;
        }
        let p = w.lp.s.clone();
        let side = if p.long_ask > 0 && (p.short_bid == 0 || self.rng.chance(50)) { L } else if p.short_bid > 0 { S } else { return };
        let ek = self.entrant();
        let sc = p.m.p.scale;
        let off = self.rng.below(3) as i64 * 50_000_000;
        let (xp, nb) = if side == L {
            let xp = self.rng.around(p.long_ask, 20_000_000).max(0);
            (xp, self.rng.around(p.bl - off, 30_000_000))
        } else {
            let xp = self.rng.around(p.short_bid, 20_000_000).max(0);
            (xp, self.rng.around(p.bs + off, 30_000_000))
        };
        // Review 11: a partial hand-over (the walk of review 10 handed over whole pairs only).
        if self.rng.chance(40) && p.size >= 2 * p.m.p.min_size {
            self.transfer_partial(i, side, xp, nb, ek);
            return;
        }
        let pay = if side == L { kpf_protocol::payoff::owed(p.size, xp - p.bl, p.long_tip, sc) } else { kpf_protocol::payoff::owed(p.size, p.bs - xp, p.short_tip, sc) };
        let mut part = p.rest_of(p.size);
        part.mk.parent = maker_of(&w.lp.op);
        if side == L {
            part.bl = nb;
            part.long_key = x(&ek);
            (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
        } else {
            part.bs = nb;
            part.short_key = x(&ek);
            (part.short_bid, part.short_min, part.short_tip) = (0, 0, 0);
        }
        let legal = Self::hand_over_rule(&p, side, xp, nb, &x(&ek));
        let t = Self::transfer_custom(&w.lp, side, xp, nb, &ek, &part, pay);
        assert_eq!(accepted(&t), legal, "seed {} step {}: transfer side {side} x {xp} nb {nb}", self.seed, self.step);
        if !legal {
            self.books.refused_as_ruled += 1;
            return;
        }
        let mut h = part.clone();
        match self.rng.below(5) {
            0 => {
                if side == L {
                    h.bs += 1
                } else {
                    h.bl -= 1
                }
            }
            1 => {
                if side == L {
                    h.short_key = x(&keypair(49))
                } else {
                    h.long_key = x(&keypair(49))
                }
            }
            2 => h.b.expiry -= 100,
            3 => h.entry += 1,
            _ => {
                if side == L {
                    h.short_bid += 1
                } else {
                    h.long_ask += 1
                }
            }
        }
        if h != part {
            self.hostile("transfer moving the stayer or the binding", &Self::transfer_custom(&w.lp, side, xp, nb, &ek, &h, pay));
        }
        if pay >= DUST + 1 {
            self.hostile("transfer underpaying the leaver", &Self::transfer_custom(&w.lp, side, xp, nb, &ek, &part, pay - 1));
        }
        self.actor(if side == L { p.long_key } else { p.short_key });
        self.actor(x(&ek));
        // The part's range is valued rounded down: the stayer's lapse value may move by one sompi (28.4).
        self.allow(if side == L { p.short_key } else { p.long_key }, "rounding of a split or hand-over", 2);
        self.book(&t);
        self.count("transfer");
        self.add_key(ek);
        self.pairs[i].lp = live(&t, 1, self.now, &part);
    }

    /// Review 11: a transfer of `n < size` units of `side` to the signing taker `ek` (it funds the part's carrier); the
    /// remainder keeps the pair's id. out 0 the leaver's payout, 1 the part (fresh id), 2 the remainder, 3 change.
    #[allow(clippy::too_many_arguments)]
    fn transfer_part_tx(lp: &LPair, side: i64, n: i64, xp: i64, nb: i64, ek: &Keypair, part: &OPair, pay: i64, rest_value: i64) -> T {
        let p = &lp.s;
        let leaver = if side == L { p.long_key } else { p.short_key };
        let args = vec![iv(side), iv(n), iv(xp), iv(nb), bv(&x(ek)), bv(&[0; 32]), iv(1)];
        let ins = vec![pair_in(lp, "transfer", args), key_in(outpoint(0xa3), 1_000_000_000_000, 0, ek)];
        let pv = part.range_of(n) + p.m.p.carrier;
        let mut outs = vec![
            out(pay.max(DUST) as u64, key_spk(&leaver), None),
            out(pv as u64, pair_spk(part), None),
            out(rest_value as u64, pair_spk(&p.rest_of(p.size - n)), bind(0, lp.id.unwrap())),
            out(100_000_000_000, p2pk_spk(ek), None),
        ];
        seal(&mut outs, 1, 0, lp.op);
        build_ext(&ins, outs, 0, &[p.m.p.quiet_daa as u64, 0])
    }

    fn transfer_partial(&mut self, i: usize, side: i64, xp: i64, nb: i64, ek: Keypair) {
        let w = self.pairs[i].clone();
        let p = w.lp.s.clone();
        let ms = p.m.p.min_size;
        let n = ms + self.rng.below((p.size - 2 * ms) as u64 + 1) as i64;
        let sc = p.m.p.scale;
        let pay = if side == L { kpf_protocol::payoff::owed(n, xp - p.bl, p.long_tip, sc) } else { kpf_protocol::payoff::owed(n, p.bs - xp, p.short_tip, sc) };
        let mut part = p.rest_of(n);
        part.mk.parent = maker_of(&w.lp.op);
        if side == L {
            part.bl = nb;
            part.long_key = x(&ek);
            (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
        } else {
            part.bs = nb;
            part.short_key = x(&ek);
            (part.short_bid, part.short_min, part.short_tip) = (0, 0, 0);
        }
        let keep = w.lp.value as i64 - p.range_of(n) + if pay < DUST { pay } else { 0 };
        let legal = Self::hand_over_rule(&p, side, xp, nb, &x(&ek)) && self.split_ok(&p, n);
        let t = Self::transfer_part_tx(&w.lp, side, n, xp, nb, &ek, &part, pay, keep);
        assert_eq!(accepted(&t), legal, "seed {} step {}: partial transfer side {side} n {n} x {xp} nb {nb}", self.seed, self.step);
        if !legal {
            self.books.refused_as_ruled += 1;
            return;
        }
        // Hostile: the remainder one sompi short of what the pair kept; the stayer moved in the remainder.
        self.hostile("partial transfer: the remainder one sompi short", &Self::transfer_part_tx(&w.lp, side, n, xp, nb, &ek, &part, pay, keep - 1));
        let mut h = part.clone();
        if side == L {
            h.short_key = x(&keypair(49));
        } else {
            h.long_key = x(&keypair(49));
        }
        if h != part {
            self.hostile("partial transfer moving the stayer", &Self::transfer_part_tx(&w.lp, side, n, xp, nb, &ek, &h, pay, keep));
        }
        self.actor(if side == L { p.long_key } else { p.short_key });
        self.actor(x(&ek));
        // The stayer's two pieces are each rounded down (one sompi per split) and the part's range too (one sompi).
        self.allow(if side == L { p.short_key } else { p.long_key }, "rounding of a split or hand-over", 3);
        self.book(&t);
        self.count("partial transfer");
        self.add_key(ek);
        self.pairs[i].lp = live(&t, 2, self.now, &p.rest_of(p.size - n));
        self.pairs.push(WP { lp: live(&t, 1, self.now, &part), ..w });
    }

    /// Review 11: a mutual close of the whole pair at the walk's price, both sides listing at it first (each a signed
    /// amend); anyone executes.
    fn mutual(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        if !self.quiet_ok(&w.lp) {
            return;
        }
        let p = w.lp.s.clone();
        let xp = self.price().clamp(p.bl.max(1), p.bs);
        let (lk, sk) = (self.key(&p.long_key), self.key(&p.short_key));
        let q_ = p.m.p.quiet_daa as u64;
        let (ta, a) = amend_tx(&w.lp, L, &lk, xp, q_);
        must("the long lists for the mutual close", &ta);
        let la = live(&ta, 0, self.now, &a);
        let (tb, b) = amend_tx(&la, S, &sk, xp, q_);
        must("the short bids for the mutual close", &tb);
        let lb = live(&tb, 0, self.now, &b);
        let t = mutual_tx(&lb, xp, q_);
        let lp_ = kpf_protocol::payoff::owed(p.size, xp - p.bl, 0, p.m.p.scale);
        if lp_ < DUST || (lb.value as i64 - lp_) < DUST {
            return;
        }
        must(&format!("seed {} step {}: mutual close at {xp}", self.seed, self.step), &t);
        self.hostile("a mutual close one sompi short to the short", &{
            let mut h = t.clone();
            h.0.outputs[2].value -= 1;
            h.0.finalize();
            h
        });
        self.actor(p.long_key);
        self.actor(p.short_key);
        self.book(&ta);
        self.book(&tb);
        self.book(&t);
        self.count("mutual close");
        self.pairs.remove(i);
    }

    /// Review 11: a cooperative close (both owners sign any split); signed by one owner and another key it is refused.
    fn coop(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        let p = w.lp.s.clone();
        let (lk, sk) = (self.key(&p.long_key), self.key(&p.short_key));
        let lpay = p.long_at(self.price()).max(DUST) as u64;
        let mk = |a: &Keypair, b: &Keypair| {
            let ins = vec![pair_in(&w.lp, "coopClose", vec![Arg::Sig(*a), Arg::Sig(*b)]), exec_in(221)];
            let outs = vec![out(lpay, key_spk(&p.long_key), None), out(w.lp.value - lpay, key_spk(&p.short_key), None), change(90_000_000_000)];
            build_ext(&ins, outs, 0, &[0, 0])
        };
        let t = mk(&lk, &sk);
        must("cooperative close", &t);
        if p.long_key != p.short_key {
            self.hostile("a cooperative close signed by one owner and another key", &mk(&lk, &keypair(88)));
        }
        self.actor(p.long_key);
        self.actor(p.short_key);
        self.book(&t);
        self.count("cooperative close");
        self.pairs.remove(i);
    }

    /// Review 11: the slot cannot unbond while it counts the cover of a live pair, and cannot split off a child whose ring
    /// exceeds its own capacity.
    fn slot_hostile(&mut self) {
        let sl = self.slot.clone();
        let (step, tail) = (sl.s.m.p.step, sl.s.m.p.tail);
        if self.pairs.iter().any(|w| w.lp.s.b.expiry + tail > self.now as i64) {
            let d = ((self.now as i64 - tail) / step - sl.s.j0).max(0);
            self.hostile("unbond the slot while it counts live cover", &unbond_tx(&sl, d, self.now));
        }
        if sl.s.ring.iter().sum::<i64>() > 100_000_000 {
            let (t, _, _) = split_tx(&sl, &sl.s.ring, 100_000_000, self.now);
            self.hostile("split the whole ring into a 1 KAS child", &t);
        }
    }

    fn donor_custom(donor: &LPair, xp: i64, e3: i64, bs3: i64, sk3: [u8; 32], p3: &OPair, spay: i64) -> T {
        let d = &donor.s;
        let args = vec![iv(d.size), iv(xp), iv(e3), iv(1), iv(bs3), bv(&sk3), iv(0), iv(0), iv(0), bv(&[0; 32])];
        let fund = key_in(outpoint(0xa1), 1_000_000_000_000, 0, &exec_kp());
        let fop = fund.outpoint;
        let mut outs = vec![out(spay.max(DUST) as u64, key_spk(&d.short_key), None), out(p3.floor() as u64, pair_spk(p3), None), out(400_000_000_000, p2pk_spk(&exec_kp()), None)];
        seal(&mut outs, 1, 1, fop);
        build_ext(&[pair_in(donor, "closeDonor", args), fund], outs, 0, &[d.m.p.quiet_daa as u64, 0])
    }

    fn donor_alone(&mut self, i: usize) {
        self.ensure_listed(i, S);
        let w = self.pairs[i].clone();
        let d = w.lp.s.clone();
        if !self.quiet_ok(&w.lp) || d.short_bid == 0 {
            return;
        }
        let sk3 = if self.rng.chance(5) { d.b.uw_key } else { x(&self.entrant()) };
        let xp = self.rng.around(d.short_bid, 20_000_000).max(0);
        let off = self.rng.below(3) as i64 * 50_000_000;
        let bs3 = self.rng.around(d.bs + off, 30_000_000);
        let e3 = if self.rng.chance(70) { d.entry } else { self.rng.around(d.entry + 10_000_000, 15_000_000) };
        let mut p3 = d.rest_of(d.size);
        p3.short_key = sk3;
        p3.bs = bs3;
        p3.entry = e3;
        (p3.short_bid, p3.short_min, p3.short_tip) = (0, 0, 0);
        p3.mk = Makers { donor: maker_of(&w.lp.op), ..Makers::default() };
        let sc = d.m.p.scale;
        let mut spay = kpf_protocol::payoff::owed(d.size, d.bs - xp, d.short_tip, sc) + w.lp.value as i64 - d.range_of(d.size);
        if spay < DUST {
            spay = 0;
        }
        let legal = xp >= 0 && xp <= d.m.p.price_max && xp <= d.short_bid && bs3 >= d.bs && bs3 <= d.m.p.price_max && e3 == d.entry && sk3 != d.b.uw_key && bs3 - d.bl <= d.b.cover_rate && (bs3 == d.bs || bs3 <= d.growth_limit(S));
        let t = Self::donor_custom(&w.lp, xp, e3, bs3, sk3, &p3, spay);
        assert_eq!(accepted(&t), legal, "seed {} step {}: closeDonor alone x {xp} bs3 {bs3} e3 {e3}", self.seed, self.step);
        if !legal {
            self.books.refused_as_ruled += 1;
            return;
        }
        let mut h = p3.clone();
        match self.rng.below(5) {
            0 => h.bl -= 1,
            1 => h.long_key = x(&keypair(49)),
            2 => h.b.cover_rate += 1,
            3 => h.b.premium = 0,
            _ => h.long_ask += 1,
        }
        if h != p3 {
            self.hostile("closeDonor moving the passive long or its binding", &Self::donor_custom(&w.lp, xp, e3, bs3, sk3, &h, spay));
        }
        if spay > DUST {
            self.hostile("closeDonor underpaying the short", &Self::donor_custom(&w.lp, xp, e3, bs3, sk3, &p3, spay - 1));
        }
        for pr in [800_000_000i64, 1_000_000_000, 1_200_000_000, 1_500_000_000] {
            assert!(p3.long_at(pr) >= d.long_at(pr), "seed {} step {}: the passive long lost at {pr}", self.seed, self.step);
        }
        self.actor(d.short_key);
        self.actor(sk3);
        self.allow(d.long_key, "rounding of a split or hand-over", 2);
        self.book(&t);
        self.count("closeDonor alone");
        self.pairs[i].lp = live(&t, 1, self.now, &p3);
    }

    fn host_custom(host: &LPair, xp: i64, e3: i64, bl3: i64, lk3: [u8; 32], p3: &OPair, lpay: i64) -> T {
        let h = &host.s;
        let args = vec![iv(h.size), iv(xp), iv(e3), iv(1), iv(bl3), bv(&lk3), iv(0), iv(0), iv(0), bv(&[0; 32])];
        let fund = key_in(outpoint(0xa2), 1_000_000_000_000, 0, &exec_kp());
        let fop = fund.outpoint;
        let keep = host.value as i64 - h.range_of(h.size) + if lpay < DUST { lpay } else { 0 };
        let mut outs = vec![out(lpay.max(DUST) as u64, key_spk(&h.long_key), None), out((p3.range_of(h.size) + keep) as u64, pair_spk(p3), None), out(400_000_000_000, p2pk_spk(&exec_kp()), None)];
        seal(&mut outs, 1, 1, fop);
        build_ext(&[pair_in(host, "closeHost", args), fund], outs, 0, &[h.m.p.quiet_daa as u64, 0])
    }

    fn host_alone(&mut self, i: usize) {
        self.ensure_listed(i, L);
        let w = self.pairs[i].clone();
        let h = w.lp.s.clone();
        if !self.quiet_ok(&w.lp) || h.long_ask == 0 {
            return;
        }
        let lk3 = if self.rng.chance(5) { h.b.uw_key } else { x(&self.entrant()) };
        let xp = self.rng.around(h.long_ask, 20_000_000).max(0);
        let off = self.rng.below(3) as i64 * 50_000_000;
        let bl3 = self.rng.around(h.bl - off, 30_000_000);
        let e3 = if self.rng.chance(70) { h.entry } else { self.rng.around(h.entry - 10_000_000, 15_000_000) };
        let mut p3 = h.rest_of(h.size);
        p3.long_key = lk3;
        p3.bl = bl3;
        p3.entry = e3;
        (p3.long_ask, p3.long_min, p3.long_tip) = (0, 0, 0);
        p3.mk = Makers { host: maker_of(&w.lp.op), ..Makers::default() };
        let lpay = kpf_protocol::payoff::owed(h.size, xp - h.bl, h.long_tip, h.m.p.scale);
        let legal = xp >= 0 && xp <= h.m.p.price_max && xp >= h.long_ask && bl3 >= 0 && bl3 <= h.bl && e3 == h.entry && lk3 != h.b.uw_key && h.bs - bl3 <= h.b.cover_rate && (bl3 == h.bl || bl3 >= h.growth_limit(L));
        let t = Self::host_custom(&w.lp, xp, e3, bl3, lk3, &p3, lpay);
        assert_eq!(accepted(&t), legal, "seed {} step {}: closeHost alone x {xp} bl3 {bl3} e3 {e3}", self.seed, self.step);
        if !legal {
            self.books.refused_as_ruled += 1;
            return;
        }
        let mut hh = p3.clone();
        match self.rng.below(4) {
            0 => hh.bs += 1,
            1 => hh.short_key = x(&keypair(49)),
            2 => hh.b.expiry += 100,
            _ => hh.short_bid += 1,
        }
        if hh != p3 {
            self.hostile("closeHost moving the passive short or its binding", &Self::host_custom(&w.lp, xp, e3, bl3, lk3, &hh, lpay));
        }
        if lpay > DUST {
            self.hostile("closeHost underpaying the long", &Self::host_custom(&w.lp, xp, e3, bl3, lk3, &p3, lpay - 1));
        }
        for pr in [800_000_000i64, 1_000_000_000, 1_200_000_000, 1_500_000_000] {
            assert!(p3.short_at(pr) >= h.short_at(pr), "seed {} step {}: the passive short lost at {pr}", self.seed, self.step);
        }
        self.actor(h.long_key);
        self.actor(lk3);
        self.allow(h.short_key, "rounding of a split or hand-over", 2);
        self.book(&t);
        self.count("closeHost alone");
        self.pairs[i].lp = live(&t, 1, self.now, &p3);
    }

    fn cvc(host: &LPair, donor: &LPair, xp: i64) -> (T, OPair) {
        let (h, d) = (&host.s, &donor.s);
        let n = h.size;
        let mut p3 = h.rest_of(n);
        p3.bl = d.bl;
        p3.long_key = d.long_key;
        (p3.long_ask, p3.long_min, p3.long_tip) = (d.long_ask, d.long_min, d.long_tip);
        p3.mk = Makers { host: maker_of(&host.op), donor: maker_of(&donor.op), ..Makers::default() };
        let sc = h.m.p.scale;
        let lpay = kpf_protocol::payoff::owed(n, xp - h.bl, h.long_tip, sc);
        let spay = kpf_protocol::payoff::owed(n, d.bs - xp, d.short_tip, sc) + donor.value as i64 - d.range_of(n);
        let host_args = vec![iv(n), iv(xp), iv(h.entry), iv(2), iv(d.bl), bv(&d.long_key), iv(d.long_ask), iv(d.long_min), iv(d.long_tip), bv(&maker_of(&donor.op))];
        let donor_args = vec![iv(n), iv(xp), iv(h.entry), iv(2), iv(h.bs), bv(&h.short_key), iv(h.short_bid), iv(h.short_min), iv(h.short_tip), bv(&maker_of(&host.op))];
        let ins = vec![pair_in(host, "closeHost", host_args), pair_in(donor, "closeDonor", donor_args), exec_in(229)];
        let keep_h = host.value as i64 - h.range_of(n);
        let mut outs = vec![
            out(lpay.max(DUST) as u64, key_spk(&h.long_key), None),
            out(spay.max(DUST) as u64, key_spk(&d.short_key), None),
            out((p3.range_of(n) + keep_h) as u64, pair_spk(&p3), None),
            change(90_000_000_000),
        ];
        seal(&mut outs, 2, 0, host.op);
        (build_ext(&ins, outs, 0, &[seq_quiet(host), seq_quiet(donor), 0]), p3)
    }

    fn combined(&mut self) {
        let n = self.pairs.len();
        if n < 2 {
            return;
        }
        let i = self.rng.below(n as u64) as usize;
        let Some(j) = (0..n).find(|&j| j != i && self.pairs[j].lp.s.b == self.pairs[i].lp.s.b) else { return };
        let (a, b) = (self.pairs[i].clone(), self.pairs[j].clone());
        if a.lp.s.size != b.lp.s.size || !self.quiet_ok(&a.lp) || !self.quiet_ok(&b.lp) || b.lp.s.entry != a.lp.s.entry {
            return;
        }
        let xp = a.lp.s.entry;
        let (ka, kb) = (self.key(&a.lp.s.long_key), self.key(&b.lp.s.short_key));
        let (ta, ha) = amend_tx(&a.lp, L, &ka, xp, 20);
        let (tb, db) = amend_tx(&b.lp, S, &kb, xp, 20);
        if !accepted(&ta) || !accepted(&tb) {
            return;
        }
        let (host, donor) = (live(&ta, 0, self.now, &ha), live(&tb, 0, self.now, &db));
        if host.s.bs - donor.s.bl > host.s.b.cover_rate || xp < host.s.bl || xp > donor.s.bs || donor.s.bl > host.s.bl || host.s.bs < donor.s.bs {
            return;
        }
        let host_ok = donor.s.bl == host.s.bl || donor.s.bl >= host.s.growth_limit(L);
        let donor_ok = host.s.bs == donor.s.bs || host.s.bs <= donor.s.growth_limit(S);
        let (t, p3) = Self::cvc(&host, &donor, xp);
        if !host_ok || !donor_ok {
            assert!(!accepted(&t), "seed {} step {}: close-versus-close taking a passive side's share", self.seed, self.step);
            self.books.refused_as_ruled += 1;
            return;
        }
        must(&format!("seed {} step {}: close-versus-close", self.seed, self.step), &t);
        self.actor(x(&ka));
        self.actor(x(&kb));
        self.allow(host.s.short_key, "rounding of a split or hand-over", 2);
        self.allow(donor.s.long_key, "rounding of a split or hand-over", 2);
        self.book(&ta);
        self.book(&tb);
        self.book(&t);
        self.count("close-versus-close");
        let (lo, hi) = (i.min(j), i.max(j));
        let w = WP { lp: live(&t, 2, self.now + 20, &p3), b0: a.b0, renewals: a.renewals.clone(), born: a.born, silent: a.silent };
        self.pairs.remove(hi);
        self.pairs.remove(lo);
        self.pairs.push(w);
    }

    /// The underwriter's renewal (as review 10's walk), each one a step of its own for I-party: each side may lose its
    /// half of the premium (rounded), or the whole premium when the entry lies outside the renewed range (the lapse value
    /// of the other side is then already zero or the whole range).
    fn renew(&mut self, i: usize) {
        loop {
            let w = self.pairs[i].clone();
            let p = &w.lp.s;
            let step = p.m.bond.step as u64;
            if (p.b.expiry as u64) > self.now + step || self.now >= p.lapse_from() {
                return;
            }
            let Some((r, pay)) = p.renewed() else { return };
            let d = advance_to(&self.slot.s, p.b.expiry + p.m.bond.term);
            let (t, r2, b2) = renew_tx(&w.lp, &self.slot, self.now, d);
            let rule = w.lp.daa as i64 <= p.b.expiry;
            assert_eq!(verdicts(&t)[0], rule, "seed {} step {}: renewal of a UTXO accepted at {} with expiry {}", self.seed, self.step, w.lp.daa, p.b.expiry);
            if !accepted(&t) {
                self.books.refused_as_ruled += 1;
                if !rule {
                    self.count("renewal refused: the UTXO is younger than its expiry");
                }
                return;
            }
            assert_eq!(r2, r);
            let mut h = t.clone();
            h.0.outputs[0].value -= 1;
            h.0.finalize();
            self.hostile_input("a renewal taking one sompi more", &h, 0);
            if let Some(&prev) = w.renewals.last() {
                if self.now - prev < (p.m.bond.term / 2) as u64 {
                    self.count("renew: a catch-up of a late renewal");
                }
            }
            let (l0, s0) = lapse_values(p, w.lp.value);
            let (l1, s1) = lapse_values(&r2, w.lp.value - pay as u64);
            // E-party: each side pays its half of the premium at the feed (rounded), or the whole premium when the feed lies
            // outside the renewed range (the other side's value at the feed is then already zero or the whole range).
            let pr = p.b.premium;
            let fp = self.price();
            let outside = fp <= r2.bl || fp >= r2.bs;
            let (al, as_) = if outside { (q(p.size, pr, p.m.p.scale) + 2, q(p.size, pr, p.m.p.scale) + 2) } else { (q(p.size, pr - pr / 2, p.m.p.scale) + 1, q(p.size, pr / 2, p.m.p.scale) + 1) };
            if outside {
                self.residual("renewal with the feed outside the range: the premium falls on one side");
            }
            let (lk, sk) = (p.long_key, p.short_key);
            self.guarded("renew", |wk| {
                wk.allow(lk, "premium of a renewal", al);
                wk.allow(sk, "premium of a renewal", as_);
                wk.book(&t);
                wk.slot = live(&t, 1, wk.now, &b2);
                wk.pairs[i].lp = live(&t, 0, wk.now, &r2);
                wk.pairs[i].renewals.push(wk.now);
            });
            assert_eq!((l0 - l1) + (s0 - s1), pay, "seed {} step {}: a renewal moved value between the sides beyond the premium", self.seed, self.step);
            self.count("renew");
            let paid = 1 + self.pairs[i].renewals.len() as i64;
            let most = 1 + (self.now as i64 + step as i64 - w.born as i64) / p.m.bond.term;
            self.checked(Inv::Party);
            if paid > most {
                self.violation(Inv::Party, format!("premiums ahead of the calendar: {paid} by DAA {} for a lineage opened at {} (premium {pay})", self.now, w.born));
            }
        }
    }

    /// The walk's judge: a claim is true if the feed was within g of its price at t0 and, under the rules of 29.3, also at
    /// P, the DAA it landed at (on older templates the feed definition names t0 alone).
    fn judge(&self, p: i64, t0: i64, pl: u64, g: i64) -> bool {
        self.feed.true_at(p, t0.max(0) as u64, g) && (!self.new_rules || self.feed.true_at(p, pl, g))
    }

    /// The part floor of 29.3 (R11-F7): a split of `n` from `p` leaves each side of each piece either nothing at the
    /// entry or at least `PART_FLOOR` (always true on older templates, which have no such rule).
    fn split_ok(&self, p: &OPair, n: i64) -> bool {
        if !self.new_rules || n >= p.size {
            return true;
        }
        p.split_ok(n)
    }

    /// A side's value at the feed `f` in the part `n` of `p` (the short's carries the pair's excess over its range).
    fn side_at(p: &OPair, value: u64, n: i64, side: i64, f: i64) -> i64 {
        let sc = p.m.p.scale;
        let c = f.clamp(p.bl, p.bs);
        if side == L {
            q(n, c - p.bl, sc)
        } else {
            q(n, p.bs - c, sc) + (value as i64 - p.range_of(p.size)).max(0)
        }
    }

    /// A claim and its outcome at the feed. Honest claims price the feed at their t0 and land at once, within lateDaa, or
    /// later; a colluding closer (or underwriter) vouches the best (worst) moment of the last lateDaa moved by g; a
    /// deliberately false liquidation now and then. The walk's watcher contests every claim its judge finds false (now and
    /// then the judge fails: a colluding YES stake, or a contest that lands late). E-party allowances: a true liquidation
    /// takes the loser's value at the feed in the part (bounded by its maintenance margin, since a true claim leaves it
    /// below maintenance at the feed) and the keeper fee may exceed it; a close leaves the non-closer its value at the feed
    /// but for rounding and the payout floor. E-claim: an honest claim true at both moments never costs its bond.
    fn claim(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        let p = w.lp.s.clone();
        if p.long_key == p.short_key || self.feed.closed_at(self.now) {
            return;
        }
        let ms = p.m.p.min_size;
        let half = p.size / 2;
        let n = if self.rng.chance(30) && half >= ms && p.size - half >= ms && self.split_ok(&p, half) { half } else { p.size };
        let now = self.now as i64;
        let (late, g, sc) = (p.m.p.late_daa, p.m.p.g_bps, p.m.p.scale);
        // An honest claim signed `delay` ago lands now: mostly at once, sometimes within lateDaa, sometimes later.
        let delay = if self.rng.chance(12) {
            late + 1 + self.rng.below(60) as i64
        } else if self.rng.chance(20) {
            self.rng.below(late as u64 + 1) as i64
        } else {
            0
        };
        let t0h = (now - delay).max(0);
        let fh = self.feed.at(t0h as u64);
        let win: Vec<(i64, i64)> = self.feed.path.iter().filter(|(d, _)| *d as i64 + late >= now && *d as i64 <= now && !self.feed.closed_at(*d)).map(|(d, v)| (*d as i64, *v)).collect();
        let collude = self.rng.chance(35);
        let (kind, side, price, t0, honest);
        let mut lookback = false;
        if !self.feed.closed_at(t0h as u64) && (p.long_liquidatable(fh) || p.short_liquidatable(fh)) {
            (kind, side, price, t0, honest) = (LIQ, if p.long_liquidatable(fh) { L } else { S }, fh, t0h, true);
        } else if collude && win.iter().any(|(_, v)| p.long_liquidatable(*v) || p.short_liquidatable(*v)) {
            let &(d, v) = win.iter().find(|(_, v)| p.long_liquidatable(*v) || p.short_liquidatable(*v)).unwrap();
            (kind, side, price, t0, honest) = (LIQ, if p.long_liquidatable(v) { L } else { S }, v, d, false);
            lookback = true;
        } else if self.now >= 600 && self.rng.chance(30) && p.range_ok() {
            let pf = p.trigger_of(L) * 9_000 / 10_000;
            if !p.long_liquidatable(pf) {
                return;
            }
            (kind, side, price, t0, honest) = (LIQ, L, pf, now, false);
        } else {
            let sd = if self.rng.chance(50) { L } else { S };
            if collude && !win.is_empty() {
                let &(d, v) = if sd == L { win.iter().max_by_key(|(_, v)| *v).unwrap() } else { win.iter().min_by_key(|(_, v)| *v).unwrap() };
                let pr = if sd == L { v * (10_000 + g) / 10_000 } else { (v * (10_000 - g) + 9_999) / 10_000 };
                (kind, side, price, t0, honest) = (CLOSE, sd, pr, d, false);
                lookback = true;
            } else {
                if self.feed.closed_at(t0h as u64) {
                    return;
                }
                (kind, side, price, t0, honest) = (CLOSE, sd, fh, t0h, true);
            }
            if !self.quiet_ok(&w.lp) || (!self.new_rules && w.silent) {
                // A close is a party entry (quietDaa); on older templates it needs the underwriter's vouch, which a silent
                // underwriter does not give.
                return;
            }
        }
        let closer = if kind == CLOSE { Some(self.key(if side == L { &p.long_key } else { &p.short_key })) } else { None };
        let spec = ClaimSpec { kind, side, n, p: price, t0, lock: self.now };
        let (pt, pw) = if kind == CLOSE && self.new_rules { self_close_tx(&w.lp, side, n, price, t0, self.now, closer.as_ref().unwrap()) } else { propose_tx(&w.lp, &spec, &uw_kp(), closer.as_ref()) };
        if kind == CLOSE && self.new_rules && !p.close_ok(side, price) {
            self.hostile("a party's own close at a price below its maintenance", &pt);
            return;
        }
        if !accepted(&pt) {
            self.books.refused_as_ruled += 1;
            return;
        }
        if kind == LIQ {
            self.hostile("a claim vouched by a key that is not the underwriter's", &propose_tx(&w.lp, &spec, &keypair(45), None).0);
        } else if self.new_rules {
            // The other party's key cannot close this side; and the closer cannot close below its maintenance.
            let other = self.key(if side == L { &p.short_key } else { &p.long_key });
            if x(&other) != x(closer.as_ref().unwrap()) {
                self.hostile("a party's own close signed by the other party", &self_close_tx(&w.lp, side, n, price, t0, self.now, &other).0);
            }
            let bad = if side == L { p.trigger_of(L) - 1 } else { p.trigger_of(S) + 1 };
            if bad > 0 && !p.close_ok(side, bad) {
                self.hostile("a party's own close at a price below its maintenance", &self_close_tx(&w.lp, side, n, bad, t0, self.now, closer.as_ref().unwrap()).0);
            }
        }
        if lookback {
            self.count(if kind == CLOSE { "propose close at the look-back's best price" } else { "propose liquidation at the look-back's worst price" });
        }
        let truth = self.judge(price, t0, self.now, g);
        let f = self.price();
        let (loser, winner) = if kind == CLOSE {
            (if side == L { p.short_key } else { p.long_key }, if side == L { p.long_key } else { p.short_key })
        } else if side == L {
            (p.long_key, p.short_key)
        } else {
            (p.short_key, p.long_key)
        };
        // `loser` here is the party a claim can wrong (the loser of a liquidation, the non-closer of a close).
        let lside = if (kind == CLOSE) == (side == L) { S } else { L };
        let excess = (w.lp.value as i64 - p.range_of(p.size)).max(0);
        let at_f = Self::side_at(&p, w.lp.value, n, lside, f);
        if let Some(c) = &closer {
            self.actor(x(c));
        }
        self.book(&pt);
        self.count(if kind == CLOSE { "propose close" } else if honest { "propose liquidation" } else { "propose false liquidation" });
        if honest && delay > late {
            self.count("an honest claim landing later than lateDaa after its t0");
        }
        let lw = live(&pt, 0, self.now, &pw);
        let pdaa = now;
        self.pairs.remove(i);
        if n < p.size {
            self.allow(p.long_key, "rounding of a split or hand-over", 2);
            self.count("partial claim");
            self.pairs.push(WP { lp: live(&pt, 1, self.now, &p.rest_of(p.size - n)), ..w.clone() });
        }
        let wdaa = p.m.p.challenge_daa as u64;
        if !truth {
            self.false_claim(&w, &p, &lw, &pw, kind, n, loser, at_f + excess);
            return;
        }
        if kind == LIQ && pw.cure_hash != [0; 32] && self.rng.chance(50) {
            let lk = self.key(&loser);
            let (ct, c) = cure_tx(&lw, &lk);
            must("cure", &ct);
            self.actor(loser);
            self.book(&ct);
            self.count("cure");
            if side == L {
                assert!(c.bl >= p.growth_limit(L), "seed {} step {}: a cure took the short's share", self.seed, self.step);
            } else {
                assert!(c.bs <= p.growth_limit(S), "seed {} step {}: a cure took the long's share", self.seed, self.step);
            }
            self.checked(Inv::Winner);
            if pw.valid_at(pdaa) && self.now as i64 > c.b.expiry {
                self.violation(Inv::Winner, format!("a valid liquidation (P {pdaa}) cured after the expiry {}", c.b.expiry));
            }
            self.pairs.push(WP { lp: live(&ct, 1, self.now, &c), ..w });
            return;
        }
        let (ft, s) = finalize_tx(&lw, pdaa, wdaa);
        must(&format!("seed {} step {}: finalize {s:?}", self.seed, self.step), &ft);
        self.book(&ft);
        self.now += wdaa;
        match s {
            Settlement::Close { long, short } => {
                let c = pw.close_price();
                let (lo_, so_) = (q(n, c - p.bl, sc), q(n, p.bs - c, sc));
                let ok = if side == L { short >= so_ || short == 0 } else { long >= lo_ || long == 0 };
                self.checked(Inv::Party);
                if !ok {
                    self.violation(Inv::Party, "a close underpaid the non-closer at the close price".into());
                }
                // The non-closer keeps its value at the feed but for rounding, and a payout below the floor goes to the
                // closer (the contract's rule).
                let got = if side == L { short } else { long };
                self.allow(loser, "rounding of a split or hand-over", 2);
                if got == 0 {
                    self.allow(loser, "the payout floor", DUST);
                }
                self.count("close settled");
                if lookback {
                    self.metric("look-back close: the non-closer below its value at the feed (sompi)", (at_f - if lside == S && n < p.size { excess } else { 0 } - got).max(0));
                }
            }
            Settlement::Forfeit { part, .. } => {
                self.count("liquidation settled");
                let m = p.m.p.mmr_bps as i128;
                let bound = q(n, (f as i128 * m / 10_000) as i64 + 1, sc) + excess + 2;
                self.allow(loser, "a true liquidation of its side (its maintenance margin at the feed)", bound);
                self.allow(winner, "the keeper fee beyond the loser's value", pw.fee + 2);
                if lookback {
                    self.metric("look-back liquidation: the loser's value at the feed, forfeited (sompi)", at_f);
                }
                let wk = if side == L { p.short_key } else { p.long_key };
                assert_eq!((part.long_key, part.short_key), (wk, wk));
                self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
            }
            Settlement::Restore { part, defender, .. } => {
                self.count("void claim restored");
                // E-claim: a true claim that is void (late, or landing after the last valid DAA before the expiry) must
                // not cost its honest proposer the bond.
                self.checked(Inv::Claim);
                if honest && defender > 0 {
                    self.violation(Inv::Claim, format!("an honest {} true at t0 and at P, landing {} DAA after t0 (P {pdaa}, X {}), was void and its bond {} went to the defender", if kind == CLOSE { "close" } else { "liquidation" }, pdaa - t0, p.b.expiry, defender));
                    self.metric("E-claim on the old rules: the bond an honest late claim paid to the defender (sompi)", defender);
                }
                self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
            }
        }
    }

    /// A claim the walk's judge finds false: the watcher contests it within W. Modes: refuted with one staker (0), with
    /// five stakers over three rounds and a deposit after the round's window, the pool settled in random batches (3); the
    /// judge fails: upheld by a colluding YES stake (1), or the contest lands after the window and is ignored (2). A false
    /// claim the judge upholds may take the defender's value: allowed, as the judge's failure outside E-party.
    #[allow(clippy::too_many_arguments)]
    fn false_claim(&mut self, w: &WP, p: &OPair, lw: &LPend, pw: &OPending, kind: i64, n: i64, defender_key: [u8; 32], at_stake: i64) {
        let r = self.rng.below(100);
        let mode = if r < 70 { 0 } else if r < 85 { 3 } else if r < 93 { 1 } else { 2 };
        let st = stake(70, 10 * UNIT as u64, 0, 0xc0);
        let c_daa = if mode == 2 { self.now + p.m.p.challenge_daa as u64 } else { self.now + 1 };
        let (ct, w2, rc, dj) = contest_tx(lw, &st, 70, c_daa);
        must("contest", &ct);
        self.book(&ct);
        let (cw, r0, ld) = (live(&ct, 0, c_daa, &w2), live(&ct, 1, c_daa, &rc), live(&ct, 2, c_daa, &dj));
        let mut receipts = vec![r0];
        let (ld, d_daa) = if mode == 1 {
            let s3 = stake(71, 13 * UNIT as u64, 0, 0xc1);
            let (dt, d2, rs) = deposit_tx(&ld, YES, &[(s3, 71)], c_daa + 10);
            must("deposit on YES", &dt);
            self.book(&dt);
            receipts.push(live(&dt, 1, c_daa + 10, &rs[0]));
            (live(&dt, 0, c_daa + 10, &d2), c_daa + 10)
        } else if mode == 3 {
            let s71 = stake(71, 13 * UNIT as u64, 0, 0xc1);
            let (dt, d2, rs) = deposit_tx(&ld, YES, &[(s71, 71)], c_daa + 10);
            must("deposit 13 on YES", &dt);
            self.book(&dt);
            receipts.push(live(&dt, 1, c_daa + 10, &rs[0]));
            let ld1 = live(&dt, 0, c_daa + 10, &d2);
            let a = c_daa + 20;
            let (s72, s73) = (stake(72, 10 * UNIT as u64, 0, 0xc2), stake(73, 10 * UNIT as u64, 0, 0xc3));
            let (dt, d3, rs) = deposit_tx(&ld1, NO, &[(s72, 72), (s73, 73)], a);
            must("deposit 2 x 10 on NO", &dt);
            self.book(&dt);
            receipts.push(live(&dt, 1, a, &rs[0]));
            receipts.push(live(&dt, 2, a, &rs[1]));
            let ld2 = live(&dt, 0, a, &d3);
            let (hd, _, _) = deposit_tx(&ld2, NO, &[(stake(75, 10 * UNIT as u64, 0, 0xc5), 75)], a + 5);
            self.hostile("a deposit on the leading side", &hd);
            let lateness = a + p.m.judge.round as u64 + 5;
            let s74 = stake(74, 20 * UNIT as u64, 0, 0xc4);
            let (dt, d4, rs) = deposit_tx(&ld2, YES, &[(s74, 74)], lateness);
            must("a YES deposit after the round's window, before the resolution", &dt);
            assert_eq!(d4.side, NO, "it does not take the lead");
            self.book(&dt);
            receipts.push(live(&dt, 1, lateness, &rs[0]));
            (live(&dt, 0, lateness, &d4), lateness)
        } else {
            (ld, c_daa)
        };
        let r_daa = d_daa + p.m.judge.round as u64;
        let mut fake = ld.clone();
        fake.id = Some(Hash::from_bytes([0xdd; 32]));
        fake.op = outpoint(0xdd);
        self.hostile("a verdict from a dispute of another id", &decide_tx(&fake, &cw, r_daa).0);
        let (dt, pool, dw) = decide_tx(&ld, &cw, r_daa);
        must("resolve and decide", &dt);
        self.book(&dt);
        let (lpool, ldw) = (live(&dt, 0, r_daa, &pool), live(&dt, 1, r_daa, &dw));
        let (ft, s) = finalize_tx(&ldw, ldw.s.clock, 0);
        must("finalize the decided part", &ft);
        self.book(&ft);
        match (mode, &s) {
            (0 | 3, Settlement::Restore { part, defender, disputer, .. }) => {
                assert_eq!(*part, p.restore_state(n), "the refuted part is restored exactly");
                assert_eq!(defender + disputer, pw.bond, "the bond to the defender and the disputer");
                self.count(if mode == 3 { "refuted after three rounds with five stakers" } else { "refuted, pool closed" });
            }
            (0 | 3, _) => panic!("seed {} step {}: a refuted claim did not restore: {s:?}", self.seed, self.step),
            (_, Settlement::Forfeit { .. } | Settlement::Close { .. }) => {
                self.allow(defender_key, "a false claim the judge upheld (the judge's failure, outside E-party)", at_stake + 2);
                self.count(if mode == 1 { "upheld by YES, pool with a loser closed" } else { "late contest ignored, pool closed" });
            }
            (_, Settlement::Restore { .. }) => self.count("upheld but void, restored"),
        }
        assert_eq!(pool.side, if mode == 1 { YES } else { NO });
        let lp2 = if mode == 3 {
            let pool0 = lpool.s.clone();
            let mut left = receipts.clone();
            for k in (1..left.len()).rev() {
                let j = self.rng.below(k as u64 + 1) as usize;
                left.swap(k, j);
            }
            let mut cur = lpool.clone();
            let (mut paid_w, mut stakes_w, mut winners, mut tries) = (0i64, 0i64, 0i64, 0);
            while !left.is_empty() && tries < 40 {
                tries += 1;
                let k = (1 + self.rng.below(3) as usize).min(left.len());
                let batch: Vec<LStake> = left[..k].to_vec();
                let (stx, p2, pays) = settle_tx(&cur, &batch, r_daa + 10);
                if !accepted(&stx) {
                    self.count("settle refused: the pool cannot pay these winners yet");
                    left.rotate_left(1);
                    continue;
                }
                self.book(&stx);
                for (r, pay) in batch.iter().zip(&pays) {
                    if r.s.side == pool0.side {
                        assert!(*pay >= r.value, "a winner paid less than its stake");
                        paid_w += *pay as i64;
                        stakes_w += r.value as i64;
                        winners += 1;
                    } else {
                        assert_eq!(*pay, 0, "a loser paid");
                    }
                }
                cur = live(&stx, 0, r_daa + 10, &p2);
                left.drain(..k);
            }
            assert!(left.is_empty(), "seed {} step {}: every receipt settles in some order", self.seed, self.step);
            let l = if pool0.side == YES { pool0.no } else { pool0.yes };
            let burn = (l / 10_000) * pool0.m.p.burn_bps + ((l % 10_000) * pool0.m.p.burn_bps) / 10_000;
            let shares = paid_w - stakes_w;
            self.checked(Inv::Staker);
            if shares > l - burn || shares < l - burn - winners {
                self.violation(Inv::Staker, format!("winners' shares {shares} against the losers' net {}", l - burn));
            }
            let base = cur.s.base;
            if cur.value as i64 - base != l - shares {
                self.violation(Inv::Staker, format!("the pool keeps {} beyond its base, the losers less the shares are {}", cur.value as i64 - base, l - shares));
            }
            self.count("pool settled in batches");
            cur
        } else {
            let (stx, p2, pays) = settle_tx(&lpool, &receipts, r_daa + 10);
            must("settle the receipts", &stx);
            self.book(&stx);
            self.checked(Inv::Staker);
            if mode == 1 && (pays[0] != 0 || pays[1] <= receipts[1].value) {
                self.violation(Inv::Staker, "a pool with a losing receipt paid it, or did not pay the winner a share".into());
            }
            live(&stx, 0, r_daa + 10, &p2)
        };
        let (ctx, _) = pool_close_tx(&lp2);
        must("close the pool", &ctx);
        self.book(&ctx);
        self.now = self.now.max(r_daa + 10);
        match s {
            Settlement::Restore { part, .. } | Settlement::Forfeit { part, .. } => self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w.clone() }),
            Settlement::Close { .. } => {}
        }
        let _ = kind;
    }

    /// The lapse settle at the entry. E-party: the side the feed favours is paid at the entry, below its value at the feed;
    /// that is allowed only where the party could have closed its own side at the feed instead (29.1: a lapse is forced on
    /// a party only by its underwriter's silence, and it had tail + idle after the expiry to close). The walk proves that
    /// close here, in the engine; on templates without a party's own close the loss has no allowance.
    fn lapse(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        if self.now < w.lp.s.lapse_from() {
            refused("lapse settle before it is due", &lapse_tx(&w.lp, w.lp.s.lapse_from() - 1, 0), 0);
            return;
        }
        let t = lapse_tx(&w.lp, self.now, 0);
        must("lapse settle", &t);
        let p = w.lp.s.clone();
        let f = self.price();
        let (lo, so) = p.lapse_out(w.lp.value);
        let la = p.long_at(f);
        let sa = w.lp.value as i64 - la;
        if p.long_key != p.short_key {
            for (k, side, at_f, paid) in [(p.long_key, L, la, lo), (p.short_key, S, sa, so)] {
                let loss = at_f - paid;
                if loss <= 0 {
                    continue;
                }
                let kp = self.key(&k);
                let probe = Live { daa: self.now.saturating_sub(p.m.p.quiet_daa as u64), ..w.lp.clone() };
                let could = self.new_rules && p.close_ok(side, f) && accepted(&self_close_tx(&probe, side, p.size, f, self.now as i64, self.now, &kp).0);
                if could {
                    self.allow(k, "a lapse at the entry its own close could have avoided", loss);
                } else {
                    self.metric("a lapse at the entry the party had no close of its own against (sompi)", loss);
                }
            }
        }
        // The payout floor (a payout below 0.02 KAS goes to the other side), once per lineage end.
        self.allow(p.long_key, "the payout floor", DUST);
        self.allow(p.short_key, "the payout floor", DUST);
        self.book(&t);
        self.count("lapse settle");
        self.pairs.remove(i);
    }

    /// E-exit, a real check: each party of the pair builds its own close of its whole side at the present feed (no
    /// underwriter input, no counterparty), landing quietDaa after the pair's last change at the latest, and its finalize
    /// W later; the closer must be paid its value at the feed less at most g of the price (and, as the short, the pair's
    /// excess over its range, which stays with the long), the other party at least its value at the feed. A party below
    /// its maintenance at the feed exits by a top-up within its share first, or by the liquidation (the market's rule).
    fn exit_probe(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        let p = w.lp.s.clone();
        let f = self.price();
        let (sc, g, m) = (p.m.p.scale, p.m.p.g_bps, p.m.p.mmr_bps);
        for side in [L, S] {
            if side == S && p.long_key == p.short_key {
                continue;
            }
            self.checked(Inv::Exit);
            if self.feed.closed_at(self.now) {
                self.count("exit: waits for the session to reopen");
                continue;
            }
            if !p.close_ok(side, f) {
                let (cl, cs) = (f as i128 * (10_000 - g) as i128 / 10_000, (f as i128 * (10_000 + g) as i128 + 9_999) / 10_000);
                let ok = if side == L { p.growth_limit(L) as i128 * 10_000 <= cl * (10_000 - m) as i128 } else { p.growth_limit(S) as i128 * 10_000 >= cs * (10_000 + m) as i128 };
                self.count(if ok { "exit: a top-up within its share, then its own close" } else { "exit: below maintenance with no room left: the liquidation" });
                continue;
            }
            let kp = self.key(if side == L { &p.long_key } else { &p.short_key });
            let probe = Live { daa: w.lp.daa.min(self.now.saturating_sub(p.m.p.quiet_daa as u64)), ..w.lp.clone() };
            let (t, pw) = self_close_tx(&probe, side, p.size, f, self.now as i64, self.now, &kp);
            if !accepted(&t) {
                self.violation(Inv::Exit, format!("the {} of a pair (X {}, now {}, underwriter {}) cannot close its own side at the feed {f}", if side == L { "long" } else { "short" }, p.b.expiry, self.now, if w.silent { "silent" } else { "renewing" }));
                continue;
            }
            let lw = live(&t, 0, self.now, &pw);
            let (ft, s) = finalize_tx(&lw, self.now as i64, p.m.p.challenge_daa as u64);
            let Settlement::Close { long, short } = s else {
                self.violation(Inv::Exit, format!("a party's own close at the feed did not settle as a close: {s:?}"));
                continue;
            };
            if !accepted(&ft) {
                self.violation(Inv::Exit, "a party's own close could not be finalized W later".into());
                continue;
            }
            let la = p.long_at(f);
            let excess = (w.lp.value as i64 - p.range_of(p.size)).max(0);
            // g of the price, and up to two sompi of rounding (the close price is rounded, then q).
            let tol = q(p.size, (f as i128 * g as i128 / 10_000) as i64 + 1, sc) + 3;
            let (got, fair, other, other_fair) = if side == L { (long - pw.refund, la, short, w.lp.value as i64 - la) } else { (short - pw.refund, w.lp.value as i64 - la - excess, long, la) };
            if fair - got > tol || other < other_fair - 2 {
                self.violation(Inv::Exit, format!("a party's own close at the feed paid the closer {got} (fair {fair}, tolerance {tol}) and the other {other} (fair {other_fair})"));
            }
            self.metric("exit: what an honest closer pays below its value at the feed (sompi)", (fair - got).max(0));
            self.count("exit: its own close at the feed, finalized W later");
        }
    }

    /// E-claim, driven on purpose: an honest liquidation, true at t0 and at P (the scenario's feed stands at its price),
    /// landing 0, lateDaa, lateDaa + 1 and lateDaa + 50 DAA after t0, and one signed before the last valid landing of the
    /// term and delayed past it; and an honest close landing lateDaa + 30 DAA late. None may cost its proposer the bond.
    fn claim_scenario(&mut self) {
        let (lp, _, _) = opened();
        let n = lp.s.size;
        let price = 920_000_000i64;
        let (late, x_) = (lp.s.m.p.late_daa, lp.s.b.expiry);
        let last = OPending::last_valid_landing(&lp.s.m, x_);
        let v = self.rng.below(6);
        let (t0, pl) = match v {
            0 => (500, 500),
            1 => (500, 500 + late),
            2 => (500, 500 + late + 1),
            3 => (500, 500 + late + 50),
            4 => (last - 5, last + 5),
            _ => (500, 500 + late + 30),
        };
        self.checked(Inv::Claim);
        let (t, w) = if v == 5 {
            if self.new_rules {
                let probe = Live { daa: 0, ..lp.clone() };
                self_close_tx(&probe, L, n, lp.s.entry, t0, t0 as u64, &long_kp())
            } else {
                propose_tx(&lp, &ClaimSpec { kind: CLOSE, side: L, n, p: lp.s.entry, t0, lock: t0 as u64 }, &uw_kp(), Some(&long_kp()))
            }
        } else {
            propose_tx(&lp, &ClaimSpec { kind: LIQ, side: L, n, p: price, t0, lock: t0 as u64 }, &uw_kp(), None)
        };
        if !accepted(&t) {
            self.violation(Inv::Claim, format!("an honest claim (variant {v}, t0 {t0}) was refused at its proposal"));
            return;
        }
        let lw = live(&t, 0, pl as u64, &w);
        let (ft, s) = finalize_tx(&lw, pl, lp.s.m.p.challenge_daa as u64);
        if !accepted(&ft) {
            self.violation(Inv::Claim, format!("an honest claim (variant {v}) could not be finalized"));
            return;
        }
        match s {
            Settlement::Restore { defender, .. } if defender > 0 => {
                self.violation(Inv::Claim, format!("an honest claim true at t0 and at P (variant {v}: t0 {t0}, P {pl}, last valid landing {last}) paid its bond {defender} to the defender"));
                self.metric("E-claim on the old rules: the bond an honest late claim paid to the defender (sompi)", defender);
            }
            Settlement::Restore { .. } => self.count("E-claim: an honest claim delayed past the last valid landing, void, its bond refunded"),
            _ => self.count("E-claim: an honest late claim settled"),
        }
    }

    fn take_pair(&mut self, i: usize) {
        let ls = if self.rng.chance(50) { L } else { S };
        self.ensure_listed(i, ls);
        let w = self.pairs[i].clone();
        let p = w.lp.s.clone();
        if !self.quiet_ok(&w.lp) || (ls == L && p.long_ask == 0) || (ls == S && p.short_bid == 0) {
            return;
        }
        let ek = self.entrant();
        let rate = [50_000_000i64, 100_000_000, 200_000_000, 300_000_000][self.rng.below(4) as usize];
        let xp = if ls == L { self.rng.around(p.long_ask, 20_000_000) } else { self.rng.around(p.short_bid, 20_000_000) }.max(0);
        let (op, id) = self.fresh_op();
        let o = OOpen { size_left: p.size, min_fill: p.size, my_rate: rate, limit: xp, min_counter_rate: 0, ..sample_order(&p.m, ls, x(&ek)) };
        let lo = Live { op, value: 2_000_000_000_000u64, daa: self.now, id: Some(id), s: o };
        let (t, part) = take_tx(&w.lp, &lo, &self.slot, xp, self.now, true);
        let nb = if ls == L { xp - rate } else { xp + rate };
        let rule = Self::hand_over_rule(&p, ls, xp, nb, &x(&ek));
        assert_eq!(verdicts(&t)[0], rule, "seed {} step {}: take with a pair, side {ls} x {xp} nb {nb}", self.seed, self.step);
        if !accepted(&t) {
            self.books.refused_as_ruled += 1;
            return;
        }
        assert!(if ls == L { p.entry >= xp } else { p.entry <= xp }, "the order took an entry against it");
        assert!(part.range_ok() && part.b.cover_rate >= part.bs - part.bl && part.b == p.b);
        let a = BindArgs::new(self.now as i64, p.b.expiry, 0, &[Claim::output(2, p.cover_of(p.size))]);
        let b2 = self.slot.s.bind(&Ctx { seq: 10, ..slot_ctx(&self.slot, self.now) }, &a, self.slot.value as i64).expect("the slot's bind");
        self.slot = live(&t, 3, self.now, &b2);
        self.actor(if ls == L { p.long_key } else { p.short_key });
        self.actor(x(&ek));
        self.allow(if ls == L { p.short_key } else { p.long_key }, "rounding of a split or hand-over", 2);
        self.book(&t);
        self.count("take with a pair");
        self.add_key(ek);
        self.pairs[i].lp = live(&t, 2, self.now, &part);
    }

    // ------------------------------------------------------------ hostile: resting orders (I-order)

    /// A resting order, taken (no pair) or filled alone in parts by an attacker: an outsider with a genesis slot of its
    /// own when the order accepts any slot, or the colluding owner of the slot the order names. Parts of every size the
    /// order's own terms allow and smaller (the contract decides), into the past or with a live cover that the attacker
    /// renews up to twice at the order's highest premium; every part is lapse-settled at its date. The order's whole
    /// loss (its escrow less its refund and every lapse payout to its owner) is compared with I-order (28.1).
    fn order_scenario(&mut self) {
        let testnet = self.rng.chance(50);
        let m = if testnet { OMarket::testnet(feed_hash_of(b"invariant walk: a testnet-default market")) } else { omarket() };
        let (step, term) = (m.bond.step as u64, m.bond.term as u64);
        let sc = m.p.scale;
        let t0: u64 = if testnet { 80_000_000 } else { 20_000 } + self.now;
        let side = if self.rng.chance(50) { L } else { S };
        let via_fill = self.rng.chance(40);
        let live_cover = self.rng.chance(40);
        let named = self.rng.chance(50);
        let (sop, sid) = self.fresh_op();
        let mut sl = Live { op: sop, value: 1_000 * KAS as u64, daa: 0, id: Some(sid), s: Bond::new(x(&uw_kp()), m.bond_market(), 0) };
        let owner = x(&self.party_key(&[41, 42, 45, 46, 47, 48, 49, 50]));
        let base = sample_order(&m, side, owner);
        let tip = [0, 1_000_000, 5_000_000][self.rng.below(3) as usize];
        let prem_max = [0, 2_000_000, 10_000_000][self.rng.below(3) as usize];
        let size = if testnet { 2 * sc } else { 20 * sc };
        let o = OOpen { size_left: size, min_fill: m.p.min_size, tip, prem_max, uw_accept: if named { sl.idb() } else { [0; 32] }, ..base };
        let e0 = o.fill_spend(size, prem_max) + KAS;
        let odaa = if via_fill && !live_cover { t0 - 3 * term } else { t0 - 10 };
        let (oop, oid) = self.fresh_op();
        let mut lo = Live { op: oop, value: e0 as u64, daa: odaa, id: Some(oid), s: o.clone() };
        let xe: i64 = if via_fill {
            (odaa + term).div_ceil(step) as i64 * step as i64
        } else if live_cover {
            ((t0 + term) / step * step) as i64
        } else {
            (step * (1 + self.rng.below(3))) as i64
        };
        let att = keypair(ATTACKER);
        let xp = 1_000_000_000i64;
        // The counterparty's bound and the cover rate: a long order meets a short at 11.50 (cover 3), a short order a long
        // at 9 (cover 3.20); the attacker writes the entry the take allows that is worst for the order (its own limit).
        let (cbound, cr) = if side == L { (xp + 150_000_000, 300_000_000) } else { (xp - 100_000_000, 320_000_000) };
        let sizes: [i64; 7] = if testnet { [1, 2, 5, 19, 20, 50, 100] } else { [1, 2, 3, 5, 10, 20, 1] };
        let max_parts = 2 + self.rng.below(8);
        let (mut taken, mut parts_n, mut tries) = (0i64, 0usize, 0u64);
        let mut parts: Vec<LPair> = vec![];
        let mut refund: Option<i64> = None;
        while parts_n < max_parts as usize && refund.is_none() && tries < 20 {
            tries += 1;
            let mut n = m.p.min_size * sizes[self.rng.below(7) as usize];
            if self.rng.chance(15) || n > lo.s.size_left {
                n = lo.s.size_left;
            }
            // A fill's expiry is at least a term after the order's own UTXO: only the first part of an aged order can lie in
            // the past, later parts of a continuation get a live cover.
            let xe = if via_fill { xe.max(((lo.daa + term).div_ceil(step) * step) as i64) } else { xe };
            let adv = advance_to(&sl.s, xe);
            let (fop, _) = self.fresh_op();
            let build = |lo: &LOrder, extra: i64| {
                if via_fill {
                    fill_part(lo, n, &sl, &att, xp, cbound, xe, cr, prem_max, adv, t0, fop, extra)
                } else {
                    take_part(lo, n, &sl, &att, xp, xp, cbound, xe, cr, prem_max, adv, t0, fop, extra)
                }
            };
            let spend_model = if via_fill { lo.s.fill_spend(n, prem_max) } else { lo.s.take_spend(n) };
            if spend_model > lo.value as i64 {
                break;
            }
            let (t, part, b2, cont) = build(&lo, 0);
            let legal = part_rule(&lo.s, n);
            if !legal {
                self.hostile_input(if via_fill { "a fill of a part the order's rule refuses" } else { "a take of a part the order's rule refuses" }, &t, 0);
                continue;
            }
            must(&format!("seed {} step {}: a part of {n} (fill {via_fill}, testnet {testnet}, left {})", self.seed, self.step, lo.s.size_left), &t);
            // Hostile: the order pays one sompi more than its terms allow.
            if (lo.value as i64) > spend_model {
                let (h, ..) = build(&lo, 1);
                self.hostile_input("an order paying one sompi more than its terms", &h, 0);
            }
            if named && self.rng.chance(30) {
                // Hostile: an order that names its slot, taken through another slot.
                let (sop2, sid2) = self.fresh_op();
                let other = Live { op: sop2, value: 1_000 * KAS as u64, daa: 0, id: Some(sid2), s: Bond::new(x(&uw_kp()), m.bond_market(), 0) };
                let (ho, ..) = if via_fill { fill_part(&lo, n, &other, &att, xp, cbound, xe, cr, prem_max, advance_to(&other.s, xe), t0, fop, 0) } else { take_part(&lo, n, &other, &att, xp, xp, cbound, xe, cr, prem_max, advance_to(&other.s, xe), t0, fop, 0) };
                self.hostile_input("an order that names its slot, through another slot", &ho, 0);
            }
            taken += n;
            parts_n += 1;
            parts.push(live(&t, 1, t0, &part));
            sl = live(&t, 2, t0, &b2);
            match cont {
                Some(c) => lo = live(&t, 0, t0, &c),
                None => refund = Some(t.0.outputs[0].value as i64),
            }
        }
        if parts.is_empty() {
            return;
        }
        // A live cover: the attacker (the slot's owner) renews every part, up to twice, at the order's highest premium.
        let mut prem_paid = 0i64;
        let mut renewals = 0i64;
        if live_cover {
            for _ in 0..self.rng.below(3) {
                for pi in 0..parts.len() {
                    let p = parts[pi].clone();
                    let lock = (p.s.b.expiry - step as i64 + 1).max(0) as u64;
                    if p.s.renewed().is_none() {
                        continue;
                    }
                    let (t, r2, b2) = renew_at(&p, &sl, lock, advance_to(&sl.s, p.s.b.expiry + m.bond.term));
                    if !accepted(&t) {
                        continue;
                    }
                    prem_paid += own_lv(&p.s, p.value, side) - own_lv(&r2, t.0.outputs[0].value, side);
                    renewals += 1;
                    parts[pi] = live(&t, 0, lock, &r2);
                    sl = live(&t, 1, lock, &b2);
                }
            }
        }
        // Every part is lapse-settled at its date.
        let mut back = 0i64;
        for p in &parts {
            let at = p.s.lapse_from().max(t0);
            let t = lapse_tx(p, at, 0);
            must(&format!("seed {} step {}: the lapse settle of a part", self.seed, self.step), &t);
            let (l, s) = p.s.lapse_out(p.value);
            back += if side == L { l } else { s };
        }
        let rest = refund.unwrap_or(lo.value as i64);
        let loss = e0 - rest - back;
        // I-order (28.1): the tip and the first term's premium half on the quantity taken, one carrier for the order, the
        // premium of every renewal of its parts, at most one sompi of rounding per part, and the payout floor of a part
        // whose lapse value those renewals brought below it (at most D / (F - D) of those renewals' premium).
        let half = if via_fill { o.premium_half(prem_max) } else { 0 };
        let bound = q(taken, tip, sc) + q(taken, half, sc) + m.p.carrier + prem_paid + prem_paid * DUST / (PART_FLOOR - DUST) + parts_n as i64 + 2 * renewals;
        self.checked(Inv::Order);
        self.books.orders += 1;
        self.books.parts += parts_n;
        if loss > bound {
            self.books.order_excess += loss - bound;
            self.violation(
                Inv::Order,
                format!(
                    "{} order ({}, {} slot, {}, testnet {testnet}) in {parts_n} parts of {taken} units: lost {loss} sompi, bound {bound} (tip {tip}, premMax {prem_max}, renewals {renewals})",
                    if side == L { "long" } else { "short" },
                    if via_fill { "filled" } else { "taken" },
                    if named { "named" } else { "any" },
                    if live_cover { "live cover" } else { "into the past" }
                ),
            );
        }
        self.count(if via_fill { "hostile: an order filled in parts" } else { "hostile: an order taken in parts" });
    }

    /// R9-F1's shape: a take with no pair into an expiry in the past, then a renewal, which the pair refuses.
    fn hostile_take(&mut self) {
        if self.now < 4_000 {
            return;
        }
        let (op, id) = self.fresh_op();
        let o = OOpen { min_fill: 2_000_000_000, ..sample_order(&omarket(), L, x(&long_kp())) };
        let lo = Live { op, value: 2_000_000_000_000u64, daa: self.now, id: Some(id), s: o };
        let sl = Live { op: outpoint(0x99), value: K, daa: 0, id: Some(Hash::from_bytes([0x5b; 32])), s: Bond::new(x(&uw_kp()), omarket().bond_market(), 0) };
        let (fop, _) = self.fresh_op();
        let (t, part, b2, _) = take_part(&lo, lo.s.size_left, &sl, &keypair(ATTACKER), 1_000_000_000, 1_000_000_000, 1_220_000_000, 1_200, 320_000_000, lo.s.prem_max, 0, self.now, fop, 0);
        must("the hostile take", &t);
        let lp = live(&t, 1, self.now, &part);
        let s2 = live(&t, 2, self.now, &b2);
        let (rt, _, _) = renew_tx(&lp, &s2, self.now, advance_to(&s2.s, lp.s.b.expiry + lp.s.m.bond.term));
        self.checked(Inv::Party);
        if verdicts(&rt)[0] {
            self.violation(Inv::Party, format!("R9-F1: a part taken at DAA {} with expiry 1,200 was renewed", self.now));
        } else {
            *self.books.hostile.entry("a renewal of a part taken into the past").or_default() += 1;
        }
        self.count("hostile: take into the past, renewal refused");
    }

    // ------------------------------------------------------------ hostile: the winner of a liquidation (I-winner)

    /// The fixture pair (X = 1,200, step 100, W 200), liquidatable from a DAA `t` near its expiry. Variants: the latest
    /// claim the rule calls valid, then (a) the loser's cure after X with no finalize, (b) a cure before X, (c) the
    /// finalize; and (d) a pair that becomes liquidatable late: the claim now if valid, else the on-time underwriter's
    /// renewal first and the claim after it (the delay until a claim can be valid is measured).
    fn winner_scenario(&mut self) {
        let (lp, sl, _) = opened();
        let k = kit();
        let p = 920_000_000i64;
        let n = lp.s.size;
        let (x_, w_, step) = (lp.s.b.expiry, lp.s.m.p.challenge_daa, lp.s.m.bond.step);
        let valid = |pp: i64| lp.s.pending_of(k, n, LIQ, L, p, pp).map(|(w, _)| w.valid_at(pp)).unwrap_or(false);
        let variant = self.rng.below(4);
        if variant < 3 {
            let Some(pl) = (lp.daa as i64..=x_).rev().find(|&pp| valid(pp)) else { panic!("no valid claim at all") };
            let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n, p, t0: pl, lock: pl as u64 }, pl as u64);
            let window = x_ - (pl + w_);
            match variant {
                0 => {
                    let at = (x_ + 1) as u64 + self.rng.below(50);
                    let (ct, c) = cure_tx(&lw, &long_kp());
                    must("a late cure", &ct);
                    let late = live(&ct, 1, at, &c);
                    let rt = renew_tx(&late, &sl, at, advance_to(&sl.s, x_ + lp.s.m.bond.term)).0;
                    assert!(!verdicts(&rt)[0], "a pair cured after its expiry was renewed");
                    self.checked(Inv::Winner);
                    if window < step {
                        self.violation(Inv::Winner, format!("a valid liquidation (P {pl}) finalizable only {window} DAA before the expiry {x_} was cured at {at}: the cover lapses and the pair settles at the entry"));
                    } else {
                        self.residual("a valid liquidation left unfinalized for a whole step, cured after the expiry: lapses at the entry");
                    }
                    self.count("hostile: a cure after the expiry");
                }
                1 => {
                    let at = (pl as u64).max(x_ as u64 - self.rng.below((x_ - pl) as u64 + 1));
                    let (ct, c) = cure_tx(&lw, &long_kp());
                    must("a cure before the expiry", &ct);
                    let lc = live(&ct, 1, at, &c);
                    let lock = at.max((x_ - step) as u64);
                    let rt = renew_tx(&lc, &sl, lock, advance_to(&sl.s, x_ + lp.s.m.bond.term)).0;
                    self.checked(Inv::Winner);
                    if !accepted(&rt) {
                        self.violation(Inv::Winner, format!("a pair cured at {at} (before the expiry {x_}) could not be renewed"));
                    }
                    self.count("hostile: a cure before the expiry, renewed");
                }
                _ => {
                    let (ft, s) = finalize_tx(&lw, pl, w_ as u64);
                    self.checked(Inv::Winner);
                    if !accepted(&ft) || !matches!(s, Settlement::Forfeit { .. }) {
                        self.violation(Inv::Winner, format!("the latest valid claim (P {pl}) did not forfeit at its finalize"));
                    }
                    self.count("hostile: the latest valid claim finalized");
                }
            }
            return;
        }
        // (d) liquidatable from t in the last W + 2 steps before X.
        let t = (x_ - w_ - 2 * step + self.rng.below((w_ + 2 * step) as u64 + 1) as i64).max(lp.daa as i64);
        self.checked(Inv::Winner);
        if valid(t) {
            let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n, p, t0: t, lock: t as u64 }, t as u64);
            let (ft, s) = finalize_tx(&lw, t, w_ as u64);
            if !accepted(&ft) || !matches!(s, Settlement::Forfeit { .. }) {
                self.violation(Inv::Winner, format!("a valid claim at {t} did not forfeit"));
            }
            self.count("hostile: a late liquidation, valid at once");
            return;
        }
        let at = t.max(x_ - step) as u64;
        let (rt, r, _) = renew_tx(&lp, &sl, at, advance_to(&sl.s, x_ + lp.s.m.bond.term));
        if !accepted(&rt) {
            self.violation(Inv::Winner, format!("the on-time renewal at {at} was refused"));
            return;
        }
        let lr = live(&rt, 0, at, &r);
        let ok = lr.s.pending_of(k, n, LIQ, L, p, at as i64).map(|(w, _)| w.valid_at(at as i64)).unwrap_or(false);
        if !ok {
            self.violation(Inv::Winner, format!("liquidatable from {t}: no valid claim even after the on-time renewal at {at}"));
            return;
        }
        let lw = proposed(&lr, &ClaimSpec { kind: LIQ, side: L, n, p, t0: at as i64, lock: at }, at);
        let (ft, s) = finalize_tx(&lw, at as i64, w_ as u64);
        if !accepted(&ft) || !matches!(s, Settlement::Forfeit { .. }) {
            self.violation(Inv::Winner, format!("the claim after the renewal at {at} did not forfeit"));
        }
        self.books.max_claim_delay = self.books.max_claim_delay.max(at - t as u64);
        if at > t as u64 {
            self.residual("a late liquidation waits for the renewal window before a claim can be valid");
        }
        self.count("hostile: a late liquidation, claimed after the renewal");
    }

    // ------------------------------------------------------------ I-exit

    /// From the walk's last state: every live pair can be lapse-settled by anyone at its date (not one DAA before), and
    /// that date lies at most a term, a step, the tail and the idle time after the present unless the underwriter keeps
    /// renewing it; and E-exit's probe for every party of every live pair.
    fn check_exit(&mut self) {
        for w in self.pairs.clone() {
            let p = &w.lp.s;
            let at = p.lapse_from().max(self.now);
            self.checked(Inv::Exit);
            if !accepted(&lapse_tx(&w.lp, at, 0)) {
                self.violation(Inv::Exit, format!("a pair could not be lapse-settled at its date {at}"));
            }
            if p.lapse_from() > 0 && accepted(&lapse_tx(&w.lp, p.lapse_from() - 1, 0)) {
                self.violation(Inv::Exit, "a lapse settle one DAA early was accepted".into());
            }
            let bound = self.now + (p.m.bond.term + p.m.bond.step + p.m.bond.tail + p.m.p.idle_daa) as u64;
            if p.lapse_from() > bound {
                self.violation(Inv::Exit, format!("a lapse date {} beyond the stated bound {bound}", p.lapse_from()));
            }
        }
        for i in 0..self.pairs.len() {
            self.exit_probe(i);
        }
    }

    fn play(&mut self, steps: usize) {
        for s in 0..steps {
            self.step = s;
            self.tick();
            if self.pairs.len() < 4 && self.rng.chance(40) {
                self.guarded("open", |w| w.open());
                if self.rng.chance(50) {
                    self.guarded("open", |w| w.open());
                }
            }
            if self.pairs.is_empty() {
                continue;
            }
            let i = self.rng.below(self.pairs.len() as u64) as usize;
            match self.rng.below(100) {
                0..=13 => self.guarded("amend", |w| w.amend(i)),
                14..=22 => self.guarded("addMargin", |w| w.add_margin(i)),
                23..=33 => self.guarded("transfer", |w| w.transfer(i)),
                34..=42 => self.guarded("closeDonor", |w| w.donor_alone(i)),
                43..=49 => self.guarded("closeHost", |w| w.host_alone(i)),
                50..=56 => self.guarded("close-versus-close", |w| w.combined()),
                57..=66 => self.guarded("claim", |w| w.claim(i)),
                67..=70 => self.guarded("lapse settle", |w| w.lapse(i)),
                71..=73 => self.hostile_take(),
                74..=80 => self.guarded("take with a pair", |w| w.take_pair(i)),
                81..=89 => self.order_scenario(),
                90..=92 => self.winner_scenario(),
                93..=94 => self.claim_scenario(),
                95..=96 => self.guarded("mutual close", |w| w.mutual(i)),
                97 => self.guarded("cooperative close", |w| w.coop(i)),
                _ => self.slot_hostile(),
            }
            for k in 0..self.pairs.len() {
                if k < self.pairs.len() && !self.pairs[k].silent && self.rng.chance(60) {
                    self.renew(k);
                }
            }
            if !self.pairs.is_empty() && self.rng.chance(30) {
                let k = self.rng.below(self.pairs.len() as u64) as usize;
                self.exit_probe(k);
            }
            self.check_bindings();
        }
        self.check_exit();
    }
}

fn run_walks(seeds: std::ops::RangeInclusive<u64>, steps: usize) -> (WBooks, Vec<(u64, usize, Inv, String)>) {
    let mut total = WBooks::default();
    let mut violations = vec![];
    let mut calm = 0;
    for seed in seeds {
        let mut w = Walk::new(seed);
        if w.feed.calm {
            calm += 1;
        }
        w.play(steps);
        for (k, v) in w.books.steps.iter() {
            *total.steps.entry(k).or_default() += v;
        }
        for (k, v) in w.books.hostile.iter() {
            *total.hostile.entry(k).or_default() += v;
        }
        for (k, v) in w.books.checks.iter() {
            *total.checks.entry(*k).or_default() += v;
        }
        for (k, v) in w.books.residuals.iter() {
            *total.residuals.entry(k).or_default() += v;
        }
        total.refused_as_ruled += w.books.refused_as_ruled;
        total.max_claim_delay = total.max_claim_delay.max(w.books.max_claim_delay);
        total.orders += w.books.orders;
        total.parts += w.books.parts;
        total.order_excess += w.books.order_excess;
        for (k, (c, s, m)) in w.books.metrics.iter() {
            let e = total.metrics.entry(k).or_default();
            e.0 += c;
            e.1 += s;
            e.2 = e.2.max(*m);
        }
        for (k, c) in w.books.causes.iter() {
            let e = total.causes.entry(k).or_default();
            e.times += c.times;
            e.loss += c.loss;
            e.max_bound = e.max_bound.max(c.max_bound);
            if c.max_loss > e.max_loss {
                e.max_loss = c.max_loss;
                e.bound_at_max = c.bound_at_max;
            }
        }
        if w.new_rules {
            total.residuals.entry("the templates carry the rules of 29.3 (walks)").and_modify(|c| *c += 1).or_insert(1);
        }
        violations.extend(w.violations);
    }
    println!("calm seeds: {calm}");
    for (k, c) in &total.causes {
        println!("E-party cause: {k}: {} losses, {:.8} KAS in all, the largest {:.8} KAS against its bound {:.8} KAS (largest bound {:.8} KAS)", c.times, c.loss as f64 / 1e8, c.max_loss as f64 / 1e8, c.bound_at_max as f64 / 1e8, c.max_bound as f64 / 1e8);
    }
    for (k, (c, s, m)) in &total.metrics {
        println!("measured: {k}: {c} times, {:.4} KAS in all, at most {:.4} KAS", *s as f64 / 1e8, *m as f64 / 1e8);
    }
    println!("walk steps: {:?}", total.steps);
    println!("hostile variants refused: {:?} (total {})", total.hostile, total.hostile.values().sum::<usize>());
    println!("invariant checks: {:?} (total {})", total.checks, total.checks.values().sum::<usize>());
    println!("residuals met: {:?}", total.residuals);
    println!("refused as the rules say: {}; orders attacked {} in {} parts, lost beyond I-order {} sompi; longest wait for a valid claim {} DAA", total.refused_as_ruled, total.orders, total.parts, total.order_excess, total.max_claim_delay);
    let mut by: BTreeMap<Inv, usize> = BTreeMap::new();
    for (_, _, i, _) in &violations {
        *by.entry(*i).or_default() += 1;
    }
    println!("violations by invariant: {by:?}");
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for (_, _, i, v) in &violations {
        let k = match v.find(')') {
            Some(e) if *i == Inv::Order => v[..=e].to_string(),
            _ => v.chars().take(70).collect(),
        };
        *kinds.entry(format!("{i:?}: {k}")).or_default() += 1;
    }
    for (k, c) in &kinds {
        println!("  {c:>5}  {k}");
    }
    (total, violations)
}

fn seeds_from(var: &str, d: (u64, u64)) -> (u64, u64) {
    std::env::var(var).ok().and_then(|s| s.split_once("..").map(|(a, b)| (a.parse().unwrap(), b.parse().unwrap()))).unwrap_or(d)
}

fn report(total: &WBooks, violations: &[(u64, usize, Inv, String)], must_run: &[&str]) {
    let mut shown: BTreeMap<Inv, usize> = BTreeMap::new();
    for (s, st, i, v) in violations {
        let c = shown.entry(*i).or_default();
        if *c < 8 {
            println!("seed {s} step {st}: {i:?}: {v}");
        }
        *c += 1;
    }
    assert!(violations.is_empty(), "{} invariant violations", violations.len());
    for k in must_run {
        assert!(total.steps.get(k).copied().unwrap_or(0) > 0, "the walk never ran {k}");
    }
    assert!(total.hostile.values().sum::<usize>() > 1_000);
}

/// R11-F5: the economic walk, 20 seeds x 400 steps (seeds 1 to 20; a third of them calm), every hostile step on, every
/// side valued at the feed. E-party, E-exit, E-claim, I-order, I-winner, I-uw and I-staker hold after every step; the
/// largest loss of every E-party cause is printed against its bound.
#[test]
fn r11_holds_the_economic_walk_at_the_feed_price() {
    let (a, b) = seeds_from("ECON_SEEDS", (1, 20));
    let (total, violations) = run_walks(a..=b, 400);
    report(&total, &violations, &["open", "amend", "transfer", "renew", "propose close", "partial claim", "lapse settle", "liquidation settled", "close settled", "partial transfer", "refuted, pool closed", "exit: its own close at the feed, finalized W later", "E-claim: an honest late claim settled", "an honest claim landing later than lateDaa after its t0"]);
}

/// The same walk on the fixture with the contest window, the quiet time and lateDaa in the proportions of the testnet
/// defaults of review 11 (W 2 minutes = 1,200 DAA, quiet 60 s, lateDaa 10 s: here W 24, quiet 12, lateDaa 2 against a
/// step of 100), 8 seeds x 400 steps.
#[test]
fn r11_holds_the_economic_walk_with_a_short_contest_window() {
    let mut m = omarket();
    (m.p.challenge_daa, m.p.quiet_daa, m.p.late_daa) = (24, 12, 2);
    assert!(m.check().is_ok());
    set_omarket(Some(m));
    let (a, b) = seeds_from("ECON_SHORT_SEEDS", (101, 108));
    let (total, violations) = run_walks(a..=b, 400);
    set_omarket(None);
    report(&total, &violations, &["open", "renew", "propose close", "lapse settle", "close settled", "exit: its own close at the feed, finalized W later"]);
}
