//! The invariant walk of the oracle-market family (docs/DESIGN-ORACLE-IMPL.md 28.1). Review 10 found its critical in a
//! residual a fix had declared bounded per part; this walk states what each role can lose without signing again as a
//! whole-order or whole-lineage quantity and checks it after every step, with hostile steps on by default:
//!
//! * I-order: every resting order taken or filled in parts (by an outsider with its own slot for an order that accepts
//!   any slot, or by the colluding owner of the slot an order names), small parts, parts into the past and parts with a
//!   live cover renewed by the attacker; the order's whole loss is compared with the bound of 28.1.
//! * I-party: every party's claimable value (its plain outputs less what it paid in, plus its side of every live pair
//!   valued at the lapse settle: the entry clamped to the range) may fall in a step it did not sign only by an allowed
//!   reason (a renewal's premium, a liquidation or a close at the vouched price, the lapse settle's payout floor).
//! * I-winner: a valid liquidation is settled, cured into a pair that keeps its cover, or refuted; late cures, claims in
//!   the last window before the expiry and the gap before a renewal can open are driven on purpose.
//! * I-exit: every live pair can be lapse-settled by anyone at its date, which no party can postpone.
//! * I-uw / I-staker: the slot keeps its value through every step; pools settle and close as ruled.
//!
//! Every transaction runs every input in the real script engine with the covenant context (`common::run`), the templates
//! compiled from source. The walk records violations instead of stopping, so a run on older templates lists what fails.

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
    Uw,
    Staker,
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
    truth: i64,
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
    allow: BTreeMap<[u8; 32], i64>,
}

impl Walk {
    fn new(seed: u64) -> Walk {
        let mut w = Walk {
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
    fn ledger(&self) -> BTreeMap<[u8; 32], i64> {
        let mut v: BTreeMap<[u8; 32], i64> = self.cash.iter().filter(|(k, _)| Self::is_party(k)).map(|(k, c)| (*k, *c)).collect();
        for w in &self.pairs {
            let (l, s) = lapse_values(&w.lp.s, w.lp.value);
            *v.entry(w.lp.s.long_key).or_default() += l;
            *v.entry(w.lp.s.short_key).or_default() += s;
        }
        v.retain(|k, _| Self::is_party(k));
        v
    }
    fn actor(&mut self, k: [u8; 32]) {
        self.actors.insert(k);
    }
    fn allow_loss(&mut self, k: [u8; 32], v: i64) {
        *self.allow.entry(k).or_default() += v.max(0);
    }
    /// Runs one action and checks I-party: every party that did not act lost no more than the action allows it.
    fn guarded(&mut self, what: &'static str, f: impl FnOnce(&mut Walk)) {
        let before = self.ledger();
        self.actors.clear();
        self.allow.clear();
        f(self);
        let after = self.ledger();
        let keys: BTreeSet<[u8; 32]> = before.keys().chain(after.keys()).copied().collect();
        for k in keys {
            if self.actors.contains(&k) {
                continue;
            }
            let d = after.get(&k).copied().unwrap_or(0) - before.get(&k).copied().unwrap_or(0);
            let a = self.allow.get(&k).copied().unwrap_or(0);
            self.checked(Inv::Party);
            if d < -a {
                self.violation(Inv::Party, format!("{what}: party {} lost {} sompi without signing (allowed {a})", short_id(&k), -d));
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
        let (op1, id1) = self.fresh_op();
        let (op2, id2) = self.fresh_op();
        let (lk, sk) = (self.party_key(&[41, 45, 46]), self.party_key(&[42, 47, 48]));
        let mk = |op, id, side, kp: &Keypair, daa| Live { op, value: 2_000_000_000_000u64, daa, id: Some(id), s: sample_order(&m, side, x(kp)) };
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
        self.pairs.push(WP { lp, truth: 1_000_000_000, b0: p.b, renewals: vec![], born: self.now, silent });
    }

    fn signed_price(&mut self) {
        self.now += 10 + self.rng.below(140);
        for w in self.pairs.iter_mut() {
            let d = (self.rng.below(5) as i64 - 2) * 10_000_000;
            w.truth = (w.truth + d).clamp(800_000_000, 1_300_000_000);
        }
    }

    fn ensure_listed(&mut self, i: usize, side: i64) {
        let w = self.pairs[i].clone();
        let listed = if side == L { w.lp.s.long_ask > 0 } else { w.lp.s.short_bid > 0 };
        if listed || !self.quiet_ok(&w.lp) {
            return;
        }
        let ask = self.rng.around(w.truth, 20_000_000).max(1);
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
        let ask = self.rng.around(w.truth, 30_000_000).max(1);
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
        self.allow_loss(if side == L { p.short_key } else { p.long_key }, 1);
        self.book(&t);
        self.count("transfer");
        self.add_key(ek);
        self.pairs[i].lp = live(&t, 1, self.now, &part);
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
        self.allow_loss(d.long_key, 1);
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
        self.allow_loss(h.short_key, 1);
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
        self.allow_loss(host.s.short_key, 1);
        self.allow_loss(donor.s.long_key, 1);
        self.book(&ta);
        self.book(&tb);
        self.book(&t);
        self.count("close-versus-close");
        let (lo, hi) = (i.min(j), i.max(j));
        let w = WP { lp: live(&t, 2, self.now + 20, &p3), truth: a.truth, b0: a.b0, renewals: a.renewals.clone(), born: a.born, silent: a.silent };
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
            let pr = p.b.premium;
            let clamped = r2.entry <= r2.bl || r2.entry >= r2.bs;
            let (al, as_) = if clamped { (q(p.size, pr, p.m.p.scale) + 2, q(p.size, pr, p.m.p.scale) + 2) } else { (q(p.size, pr - pr / 2, p.m.p.scale) + 1, q(p.size, pr / 2, p.m.p.scale) + 1) };
            if clamped {
                self.residual("renewal with the entry outside the range: the premium falls on one side's lapse value");
            }
            let (lk, sk) = (p.long_key, p.short_key);
            self.guarded("renew", |wk| {
                wk.allow_loss(lk, al);
                wk.allow_loss(sk, as_);
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

    /// A claim (as review 10's walk) and its outcome, with the I-party allowances: a liquidation (true, or upheld by the
    /// judge) may take the loser's side; a close at the vouched price may take what the non-closer had above its payoff
    /// at the close price (which the walk checks it gets).
    fn claim(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        let p = w.lp.s.clone();
        if p.long_key == p.short_key {
            return;
        }
        let ms = p.m.p.min_size;
        let n = if self.rng.chance(30) && p.size / 2 >= ms && p.size - p.size / 2 >= ms { p.size / 2 } else { p.size };
        let liq_side = if p.long_liquidatable(w.truth) { Some(L) } else if p.short_liquidatable(w.truth) { Some(S) } else { None };
        let t0 = self.now as i64;
        let kind;
        let side;
        let price;
        let mut false_claim = false;
        if let Some(sd) = liq_side {
            (kind, side, price) = (LIQ, sd, w.truth);
        } else if self.now >= 600 && self.rng.chance(30) && p.range_ok() {
            let pf = p.trigger_of(L) * 9_000 / 10_000;
            if !p.long_liquidatable(pf) {
                return;
            }
            (kind, side, price, false_claim) = (LIQ, L, pf, true);
        } else {
            (kind, side, price) = (CLOSE, if self.rng.chance(50) { L } else { S }, w.truth);
        }
        let closer = if kind == CLOSE { Some(self.key(if side == L { &p.long_key } else { &p.short_key })) } else { None };
        let (pt, pw) = propose_tx(&w.lp, &ClaimSpec { kind, side, n, p: price, t0, lock: self.now }, &uw_kp(), closer.as_ref());
        if !accepted(&pt) {
            self.books.refused_as_ruled += 1;
            return;
        }
        let (lv_l, lv_s) = lapse_values(&p, w.lp.value);
        let (loser, winner, lv_loser) = if side == L { (p.long_key, p.short_key, lv_l) } else { (p.short_key, p.long_key, lv_s) };
        if let Some(c) = &closer {
            self.actor(x(c));
        }
        self.book(&pt);
        self.count(if kind == CLOSE { "propose close" } else if false_claim { "propose false liquidation" } else { "propose liquidation" });
        let lw = live(&pt, 0, self.now, &pw);
        let pdaa = self.now as i64;
        self.pairs.remove(i);
        if n < p.size {
            // A split values the long's two parts each rounded down: at most one sompi of its lapse value moves to the
            // short (28.4).
            self.allow_loss(p.long_key, 1);
            self.count("partial claim");
            self.pairs.push(WP { lp: live(&pt, 1, self.now, &p.rest_of(p.size - n)), ..w.clone() });
        }
        if false_claim {
            let mode = self.rng.below(3);
            let st = stake(70, 10 * UNIT as u64, 0, 0xc0);
            let c_daa = if mode == 2 { self.now + p.m.p.challenge_daa as u64 } else { self.now + 10 };
            let (ct, w2, rc, dj) = contest_tx(&lw, &st, 70, c_daa);
            must("contest", &ct);
            self.book(&ct);
            let (cw, r, ld) = (live(&ct, 0, c_daa, &w2), live(&ct, 1, c_daa, &rc), live(&ct, 2, c_daa, &dj));
            let mut receipts = vec![r];
            let (ld, d_daa) = if mode == 1 {
                let s3 = stake(71, 13 * UNIT as u64, 0, 0xc1);
                let (dt, d2, rs) = deposit_tx(&ld, YES, &[(s3, 71)], c_daa + 10);
                must("deposit on YES", &dt);
                self.book(&dt);
                receipts.push(live(&dt, 1, c_daa + 10, &rs[0]));
                (live(&dt, 0, c_daa + 10, &d2), c_daa + 10)
            } else {
                (ld, c_daa)
            };
            let r_daa = d_daa + p.m.judge.round as u64;
            let (dt, pool, dw) = decide_tx(&ld, &cw, r_daa);
            must("resolve and decide", &dt);
            self.book(&dt);
            let (lpool, ldw) = (live(&dt, 0, r_daa, &pool), live(&dt, 1, r_daa, &dw));
            let (ft, s) = finalize_tx(&ldw, ldw.s.clock, 0);
            must("finalize the decided part", &ft);
            self.book(&ft);
            match (mode, &s) {
                (0, Settlement::Restore { part, defender, disputer, .. }) => {
                    assert_eq!(*part, p.restore_state(n), "the refuted part is restored exactly");
                    assert_eq!(defender + disputer, pw.bond, "the bond to the defender and the disputer");
                    self.count("refuted, pool closed");
                }
                (0, _) => panic!("seed {} step {}: a refuted claim did not restore: {s:?}", self.seed, self.step),
                (_, Settlement::Forfeit { .. }) => {
                    // The judge upheld it (colluders' YES stake, or the defender's contest came late): its verdict stands.
                    self.allow_loss(loser, lv_loser);
                    self.count(if mode == 1 { "upheld by YES, pool with a loser closed" } else { "late contest ignored, pool closed" });
                }
                (_, Settlement::Restore { defender, disputer, .. }) => {
                    assert_eq!((*defender, *disputer), (0, 0), "an upheld but void claim's bond goes back to its proposer (review 11)");
                    self.count("upheld but void, restored");
                }
                (_, Settlement::Close { .. }) => panic!("a liquidation settled as a close"),
            }
            assert_eq!(pool.side, if mode == 1 { YES } else { NO });
            let (stx, p2, pays) = settle_tx(&lpool, &receipts, r_daa + 10);
            must("settle the receipts", &stx);
            self.book(&stx);
            self.checked(Inv::Staker);
            if mode == 1 && (pays[0] != 0 || pays[1] <= receipts[1].value) {
                self.violation(Inv::Staker, "a pool with a losing receipt paid it, or did not pay the winner a share".into());
            }
            let lp2 = live(&stx, 0, r_daa + 10, &p2);
            let (ctx, _) = pool_close_tx(&lp2);
            must("close the pool", &ctx);
            self.book(&ctx);
            self.now = self.now.max(r_daa + 10);
            let (Settlement::Restore { part, .. } | Settlement::Forfeit { part, .. }) = s else { unreachable!() };
            self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
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
            // I-winner: a cure of a valid claim leaves a pair that keeps its cover (renewable: accepted no later than its
            // expiry). The walk cures at the claim's own DAA, so this always holds here; `winner` drives the late cure.
            self.checked(Inv::Winner);
            if pw.valid_at(pdaa) && self.now as i64 > c.b.expiry {
                self.violation(Inv::Winner, format!("a valid liquidation (P {pdaa}) cured after the expiry {}", c.b.expiry));
            }
            self.pairs.push(WP { lp: live(&ct, 1, self.now, &c), ..w });
            return;
        }
        let (ft, s) = finalize_tx(&lw, pdaa, 200);
        must(&format!("seed {} step {}: finalize {s:?}", self.seed, self.step), &ft);
        self.book(&ft);
        self.now += 200;
        match s {
            Settlement::Close { long, short } => {
                let c = pw.close_price();
                let (lo_, so_) = (q(n, c - p.bl, p.m.p.scale), q(n, p.bs - c, p.m.p.scale));
                let ok = if side == L { short >= so_ || short == 0 } else { long >= lo_ || long == 0 };
                self.checked(Inv::Party);
                if !ok {
                    self.violation(Inv::Party, "a close underpaid the non-closer at the close price".into());
                }
                self.allow_loss(winner, if side == L { lv_s } else { lv_l });
                self.count("close settled");
            }
            Settlement::Forfeit { part, .. } => {
                self.count("liquidation settled");
                self.allow_loss(loser, lv_loser);
                let wk = if side == L { p.short_key } else { p.long_key };
                assert_eq!((part.long_key, part.short_key), (wk, wk));
                self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
            }
            Settlement::Restore { part, defender, .. } => {
                assert_eq!(defender, 0, "a void claim's bond goes back to its proposer (review 11)");
                self.count("void claim restored");
                self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
            }
        }
    }

    fn lapse(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        if self.now < w.lp.s.lapse_from() {
            refused("lapse settle before it is due", &lapse_tx(&w.lp, w.lp.s.lapse_from() - 1, 0), 0);
            return;
        }
        let t = lapse_tx(&w.lp, self.now, 0);
        must("lapse settle", &t);
        // The payout floor (a payout below 0.02 KAS goes to the other side), once per lineage end.
        self.allow_loss(w.lp.s.long_key, DUST);
        self.allow_loss(w.lp.s.short_key, DUST);
        self.book(&t);
        self.count("lapse settle");
        self.pairs.remove(i);
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
        self.allow_loss(if ls == L { p.short_key } else { p.long_key }, 1);
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
    /// renewing it (the only actor who can move it, and only at the agreed premium).
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
    }

    fn play(&mut self, steps: usize) {
        for s in 0..steps {
            self.step = s;
            self.signed_price();
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
                90..=94 => self.winner_scenario(),
                _ => {}
            }
            for k in 0..self.pairs.len() {
                if k < self.pairs.len() && !self.pairs[k].silent && self.rng.chance(60) {
                    self.renew(k);
                }
            }
            self.check_bindings();
        }
        self.check_exit();
    }
}

fn run_walks(seeds: std::ops::RangeInclusive<u64>, steps: usize) -> (WBooks, Vec<(u64, usize, Inv, String)>) {
    let mut total = WBooks::default();
    let mut violations = vec![];
    for seed in seeds {
        let mut w = Walk::new(seed);
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
        violations.extend(w.violations);
    }
    println!("walk steps: {:?}", total.steps);
    println!("hostile variants refused: {:?} (total {})", total.hostile, total.hostile.values().sum::<usize>());
    println!("invariant checks: {:?}", total.checks);
    println!("residuals met (as stated in 28.4): {:?}", total.residuals);
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
            _ => v.chars().take(60).collect(),
        };
        *kinds.entry(format!("{i:?}: {k}")).or_default() += 1;
    }
    for (k, c) in &kinds {
        println!("  {c:>5}  {k}");
    }
    (total, violations)
}

/// The invariant walk (28.1, 28.2): 20 seeds x 400 steps with every hostile step on. Every invariant holds after every
/// step; the walk reached every entry point and every hostile shape.
#[test]
fn invariant_walk_holds_with_hostile_steps_on() {
    let (total, violations) = run_walks(1..=20, 400);
    let mut shown: BTreeMap<Inv, usize> = BTreeMap::new();
    for (s, st, i, v) in &violations {
        let c = shown.entry(*i).or_default();
        if *c < 6 {
            println!("seed {s} step {st}: {i:?}: {v}");
        }
        *c += 1;
    }
    for k in ["open", "amend", "addMargin", "transfer", "take with a pair", "closeDonor alone", "closeHost alone", "renew", "propose close", "partial claim", "lapse settle", "liquidation settled", "hostile: an order taken in parts", "hostile: an order filled in parts", "hostile: a cure after the expiry", "hostile: a late liquidation, claimed after the renewal"] {
        assert!(total.steps.get(k).copied().unwrap_or(0) > 0, "the walk never ran {k}");
    }
    assert!(total.hostile.values().sum::<usize>() > 1_000);
    assert!(violations.is_empty(), "{} invariant violations", violations.len());
}
