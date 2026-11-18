//! Experiment X6 (DESIGN 8): a seeded, deterministic random walk over open -> list -> split -> transfer ->
//! close-versus-close -> mutual close -> idle settle. Every transaction is built by an executor that pays each
//! party the least its covenant accepts and keeps the rest, and is run in the script engine. For every
//! transaction the test asserts:
//!   - no sompi is created (outputs <= inputs);
//!   - every party that did not consent (a stayer, an amend's counterparty, both sides of an idle settle) has a
//!     payoff function (cash received plus DESIGN 3.1 payouts of all its pairs, as a function of price) that is
//!     unchanged or better at every price of a grid, up to the rounding slack;
//!   - that slack is at most 1 sompi per part the party's pair was cut into;
//!   - every leaver is paid at least what its listing promises, and its remaining position loses at most the
//!     part it sold (plus at most 1 sompi of rounding);
//!   - every party that did not act keeps its idle-settle payout (cash plus the payout of each of its pairs at
//!     that pair's E), up to 1 sompi per split (review 1, F3);
//!   - no pair output carries a non-zero tag other than one written by a hand-over of that transaction (each
//!     exactly once), so no tag outlives its transaction (review 1, F1/F2; DESIGN 4.4);
//!   - the evidence clock (review 3, DESIGN 4.5c): no pair output's clock moves backwards, and a clock restarts
//!     (since = 0) only on an output where a side changed hands; every other output carries since = S of its input;
//!   - every evidence transaction is accepted exactly when the order was fillable through max(exposed, S) + R + L and
//!     the lock time proves max(exposed, S) + R (stale and dead orders and the review-2 lock time are mixed in), and
//!     no accepted liquidation counts evidence for any time before the liquidated side entered.
//!
//! Two-step evidence (DESIGN 4.5d): the evidence walk proposes, challenges, cures and finalizes. A pending part counts
//! in its owners' payoffs as the pair it would be restored to. In addition to the above:
//!   - a challenge is accepted exactly when its counter quote contradicts the claim by g and the counter order was
//!     fillable through max(exposed, P) + R + L with the lock time proving max(exposed, P) + R (P the pending UTXO's
//!     DAA score; old orders, orders placed after P, short-lived orders and quotes one tick short are mixed in);
//!   - a party that did not act never loses value except through a finalized, unrefuted proposal: a proposal, a
//!     challenge and a cure leave every non-acting party's payoff unchanged or better at every grid price (up to the
//!     rounding of a split); a finalize takes from the loser (its side, forfeited) and from the winner at most the fee;
//!     a finalized close leaves the other party its payoff at the quote;
//!   - every bond ends with exactly one of the proposer or the challenger: the proposer's cash grows by exactly bond
//!     and fee (a finalized liquidation), refund (a cure), its payout plus refund (a finalized close), and by nothing
//!     at a challenge, whose bond is the challenger's; the bonds posted equal the bonds returned to proposers plus
//!     those taken by challengers.
//!
//! Review 4 (DESIGN 4.5e). In addition:
//!   - a part with one owner (longKey == shortKey: a forfeited part) is never the subject of an accepted liquidation
//!     proposal (every accepted one is checked, and forfeited parts are attacked with triggering quotes on purpose);
//!   - after a cure, no accepted liquidation proposal against the cured side (the cured pair and every position that
//!     keeps its cured bound) uses a quote within g of the cured claim, and the walk tries such quotes on purpose; the
//!     first quote beyond the cure bound's trigger is accepted;
//!   - no restore or cure output is listed (half of the proposals are made on a listed pair).

mod common;
mod kpf;

use std::collections::HashMap;

use common::*;
use kaspa_consensus_core::tx::{Transaction, TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf::evidence::{cure_draft, finalize_draft, KIND_CLOSE};
use kpf::paths::*;
use kpf::*;
use secp256k1::{Keypair, Secp256k1, SecretKey};

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// Uniform in [a, b] (a <= b).
    fn range(&mut self, a: i64, b: i64) -> i64 {
        assert!(a <= b, "range {a}..{b}");
        a + (self.next() % ((b - a) as u64 + 1)) as i64
    }
    fn coin(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

const MIN: i64 = MIN_SIZE;

fn valid_part(s: i64, n: i64) -> bool {
    n >= MIN && n <= s && (s - n == 0 || s - n >= MIN)
}

#[derive(Default, Debug)]
struct Stats {
    open: usize,
    amend: usize,
    split: usize,
    transfer_open: usize,
    transfer_direct: usize,
    close_vs_close: usize,
    close_partial: usize,
    mutual: usize,
    idle: usize,
    ev_close: usize,
    ev_liq: usize,
    ev_refused: usize,
    challenged: usize,
    challenge_refused: usize,
    cured: usize,
    finalized_close: usize,
    finalized_liq: usize,
    coop_forfeited: usize,
    bonds_posted: i128,
    bonds_to_proposers: i128,
    bonds_to_challengers: i128,
    clock_kept: usize,
    clock_restarted: usize,
    txs: usize,
    max_slack: i128,
    /// Review 4: liquidations of a one-owner part refused; quotes within g of a cured claim refused; first quotes
    /// beyond a cure accepted; listed parts that came back unlisted.
    one_owner_refused: usize,
    within_g_refused: usize,
    beyond_cure_accepted: usize,
    returned_unlisted: usize,
    /// Guaranteed fast settlement (DESIGN 4.5f): guarantees, disputes accepted and refused, releases; every lock's
    /// value and where it went.
    guaranteed_close: usize,
    guaranteed_liq: usize,
    offers: usize,
    disputed: usize,
    dispute_refused: usize,
    dispute_out_of_window: usize,
    released: usize,
    lock_value_in: i128,
    lock_released: i128,
    lock_to_victims: i128,
    lock_burned: i128,
    lock_rewards: i128,
}

#[derive(Clone)]
struct World {
    pairs: Vec<Live>,
    /// Parts with a pending claim (DESIGN 4.5d), counted in payoffs as the pair they would be restored to.
    pendings: Vec<LivePend>,
    /// Guarantors' locks (DESIGN 4.5f).
    locks: Vec<LiveLk>,
    cash: HashMap<[u8; 32], i128>,
}

struct Sim {
    w: World,
    keys: HashMap<[u8; 32], Keypair>,
    rng: Rng,
    ctr: u32,
    f: Keypair,
    st: Stats,
    /// The DAA score of the chain: every UTXO a transaction creates gets it (review 3, the evidence clock).
    daa: u64,
    /// The DAA at which each side of each live pair UTXO entered its position: (outpoint, side) -> DAA.
    entered: HashMap<(TransactionOutpoint, i64), u64>,
    /// Review 4: every live position (pair or pending part) that keeps the bound of a cure: outpoint -> (loser side, cured
    /// bound, quote of the cured claim).
    cured: HashMap<TransactionOutpoint, (i64, i64, i64)>,
    /// DESIGN 4.5f: the pending part each live lock settled, and what the settlement paid its victim.
    lock_src: HashMap<TransactionOutpoint, (LivePend, i64)>,
}

/// Whether `quote` lies within the margin g of a cured liquidation claim at `claim` of the loser `side` (DESIGN 4.5e):
/// an ask with `a * 10^4 >= claim * (10^4 - g)` against a long, a bid with `b * 10^4 <= claim * (10^4 + g)` against a
/// short.
fn within_g(side: i64, claim: i64, quote: i64, g: i64) -> bool {
    if side == LONG {
        quote as i128 * 10_000 >= claim as i128 * (10_000 - g) as i128
    } else {
        quote as i128 * 10_000 <= claim as i128 * (10_000 + g) as i128
    }
}

fn unlisted(p: &Pair) -> bool {
    (p.long_ask, p.long_min, p.long_tip, p.short_bid, p.short_min, p.short_tip) == (0, 0, 0, 0, 0, 0)
}

impl World {
    /// Every position: the live pairs and the restore state of every pending part.
    fn positions(&self) -> Vec<Pair> {
        let mut v: Vec<Pair> = self.pairs.iter().map(|l| l.p.clone()).collect();
        v.extend(self.pendings.iter().map(|w| w.w.restore_pair().expect("restore state")));
        v
    }
    fn payoff(&self, key: &[u8; 32], p: i64) -> i128 {
        let mut v = *self.cash.get(key).unwrap_or(&0);
        for q in self.positions() {
            if &q.long_key == key {
                v += q.long_at(p) as i128;
            }
            if &q.short_key == key {
                v += q.short_at(p) as i128;
            }
        }
        v
    }
    fn cash(&self, key: &[u8; 32]) -> i128 {
        *self.cash.get(key).unwrap_or(&0)
    }
    /// What P8 would give the party if every pair idled now: cash plus the DESIGN 3.1 payout of each of its pairs
    /// at that pair's own entry price E (review 1, F3).
    fn idle_payoff(&self, key: &[u8; 32]) -> i128 {
        let mut v = self.cash(key);
        for q in self.positions() {
            if &q.long_key == key {
                v += q.long_at(q.entry) as i128;
            }
            if &q.short_key == key {
                v += q.short_at(q.entry) as i128;
            }
        }
        v
    }
}

impl Sim {
    fn new(seed: u64) -> Sim {
        Sim {
            w: World { pairs: vec![], pendings: vec![], locks: vec![], cash: HashMap::new() },
            keys: HashMap::new(),
            rng: Rng(seed),
            ctr: (seed as u32) << 12,
            f: keypair(14),
            st: Stats::default(),
            daa: 1_000_000,
            entered: HashMap::new(),
            cured: HashMap::new(),
            lock_src: HashMap::new(),
        }
    }

    fn fresh_key(&mut self) -> Keypair {
        self.ctr += 1;
        let mut b = [0x11u8; 32];
        b[..4].copy_from_slice(&self.ctr.to_be_bytes());
        let kp = Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&b).unwrap());
        self.keys.insert(xonly(&kp), kp);
        kp
    }

    fn fresh_hash(&mut self, tag: u8) -> Hash {
        self.ctr += 1;
        let mut b = [tag; 32];
        b[..4].copy_from_slice(&self.ctr.to_be_bytes());
        Hash::from_bytes(b)
    }

    fn fresh_op(&mut self) -> TransactionOutpoint {
        TransactionOutpoint { transaction_id: TransactionId::from_bytes(self.fresh_hash(0xee).as_bytes()), index: 0 }
    }

    fn fn_(&mut self) -> u32 {
        self.ctr += 1;
        self.ctr
    }

    /// Builds, runs and applies a draft: every input passes, no sompi is created, the spent pairs leave the world
    /// and the created ones join it, and plain outputs to parties (less plain inputs of parties) are their cash.
    /// `handed` lists the (side, tag) of every hand-over of this transaction: each pair output carries exactly
    /// these non-zero tags, each once, and tag 0 everywhere else (DESIGN 4.4, tags live for one transaction).
    fn apply(&mut self, d: &Draft, handed: &[(i64, Hash)]) -> Transaction {
        let mut want: Vec<(i64, Hash)> = handed.iter().copied().filter(|(_, t)| *t != ZERO).collect();
        for (idx, p) in &d.pairs {
            for (side, t) in [(LONG, p.tag_l), (SHORT, p.tag_s)] {
                if t != ZERO {
                    let at = want.iter().position(|w| *w == (side, t));
                    assert!(at.is_some(), "output {idx}: tag {t} on side {side} not written by a hand-over of this transaction");
                    want.remove(at.unwrap());
                }
            }
        }
        assert!(want.is_empty(), "hand-over tags not written: {want:?}");
        // A transaction whose lock time is ahead of the chain waits for it (the hand-over limit S + 4R, the evidence
        // rest): the chain's DAA moves to the lock time.
        if d.lock > self.daa {
            self.daa = d.lock;
        }
        let now = self.daa;
        let (tx, e) = d.build();
        assert_accepted(&tx, &e);
        let ins: u128 = e.iter().map(|u| u.amount as u128).sum();
        let outs: u128 = tx.outputs.iter().map(|o| o.value as u128).sum();
        assert!(outs <= ins, "sompi created: outputs {outs} > inputs {ins}");
        self.st.txs += 1;
        for i in &d.ins {
            if let Spend::P2pk(kp) = &i.spend {
                if self.keys.contains_key(&xonly(kp)) {
                    *self.w.cash.entry(xonly(kp)).or_default() -= i.utxo.amount as i128;
                }
            }
        }
        for o in &tx.outputs {
            let s = o.script_public_key.script();
            if s.len() == 34 && s[0] == 0x20 && s[33] == 0xac {
                let key: [u8; 32] = s[1..33].try_into().unwrap();
                if self.keys.contains_key(&key) {
                    *self.w.cash.entry(key).or_default() += o.value as i128;
                }
            }
        }
        // The evidence clock (review 3, DESIGN 4.5c). Every pair output is bound to the input that authorizes it. One
        // whose two keys are those of that input pair (a continuation, a remainder) keeps the input's clock exactly
        // (since = S); one where a side changed hands (T1's new pair, the part of P3 and P7, P4's re-paired output)
        // restarts it (since = 0, its clock is its own UTXO). So no clock moves backwards, and a clock restarts only
        // on a hand-over.
        // The inputs that carry a position: the pairs, and the pending parts (as the pair they were cut from, with the
        // clock S they recorded).
        struct Src {
            idx: usize,
            op: TransactionOutpoint,
            p: Pair,
            clock: i64,
            restore: Option<Pair>,
        }
        let mut in_pos: Vec<Src> = vec![];
        for (i, inp) in d.ins.iter().enumerate() {
            if let Some(l) = self.w.pairs.iter().find(|l| l.op == inp.outpoint) {
                in_pos.push(Src { idx: i, op: l.op, p: l.p.clone(), clock: l.clock(), restore: None });
            }
            if let Some(w) = self.w.pendings.iter().find(|w| w.op == inp.outpoint) {
                in_pos.push(Src { idx: i, op: w.op, p: w.w.pair_view(), clock: w.w.since, restore: w.w.restore_pair() });
            }
        }
        for (idx, p) in &d.pairs {
            let auth = tx.outputs[*idx].covenant.map(|c| c.authorizing_input as usize);
            let src = auth.and_then(|a| in_pos.iter().find(|s| s.idx == a));
            match src {
                // A refuted claim restores the part with the clock S the pending part recorded (DESIGN 4.5d).
                Some(s) if s.restore.as_ref() == Some(p) => {
                    assert_eq!(p.since, s.clock, "output {idx}: the restored part keeps the clock S");
                    self.st.clock_kept += 1;
                }
                Some(s) if s.p.long_key == p.long_key && s.p.short_key == p.short_key => {
                    assert_eq!(p.since, s.clock, "output {idx}: a pair output that changes no hands keeps the clock S");
                    self.st.clock_kept += 1;
                }
                _ => {
                    assert_eq!(p.since, 0, "output {idx}: a side changed hands, so its clock restarts (since = 0)");
                    self.st.clock_restarted += 1;
                }
            }
            if let Some(s) = src {
                assert!(p.clock_start(now) >= s.clock, "output {idx}: the clock moved backwards");
            }
        }
        // A pending part records the clock S of the pair it was cut from.
        for (idx, w) in &d.pendings {
            let auth = tx.outputs[*idx].covenant.map(|c| c.authorizing_input as usize);
            let s = in_pos.iter().find(|s| Some(s.idx) == auth).expect("a pending part comes from a pair");
            assert_eq!(w.since, s.clock, "output {idx}: the pending part records the pair's clock S");
            // Review 4 (DESIGN 4.5e): no accepted liquidation proposal has a part with one owner as its subject.
            if w.kind == kpf::evidence::KIND_LIQ {
                assert_ne!(w.long_key, w.short_key, "output {idx}: a liquidation of a part whose two sides have one owner was accepted");
            }
        }
        // Review 4 (DESIGN 4.5e): every pair a pending part returns to (restore, cure, forfeit) is unlisted.
        for (idx, p) in &d.pairs {
            let auth = tx.outputs[*idx].covenant.map(|c| c.authorizing_input as usize);
            if let Some(s) = auth.and_then(|a| in_pos.iter().find(|s| s.idx == a)) {
                if s.restore.is_some() {
                    assert!(unlisted(p), "output {idx}: a pair returned from a pending part is listed");
                    if !unlisted(&s.p) {
                        self.st.returned_unlisted += 1;
                    }
                }
            }
        }
        let spent: Vec<TransactionOutpoint> = d.ins.iter().map(|i| i.outpoint).collect();
        let created: Vec<Live> = live_pairs(&tx, d).into_iter().map(|l| l.at(now)).collect();
        let created_w: Vec<LivePend> = live_pendings(&tx, d).into_iter().map(|w| w.at(now)).collect();
        // The cured bound travels with every position that keeps it on the same loser (a remainder, an amend, a pending
        // part and its restore); a cure itself is recorded by `cure`.
        let carry = |idx: usize, op: TransactionOutpoint, lk: [u8; 32], sk: [u8; 32], bl: i64, bs: i64, cured: &mut HashMap<TransactionOutpoint, (i64, i64, i64)>| {
            let auth = tx.outputs[idx].covenant.map(|c| c.authorizing_input as usize);
            let Some(s) = auth.and_then(|a| in_pos.iter().find(|s| s.idx == a)) else { return };
            if let Some(&(side, bound, claim)) = cured.get(&s.op) {
                let same_keys = s.p.long_key == lk && s.p.short_key == sk;
                if same_keys && (if side == LONG { bl } else { bs }) == bound {
                    cured.insert(op, (side, bound, claim));
                }
            }
        };
        for ((idx, _), c) in d.pairs.iter().zip(&created) {
            carry(*idx, c.op, c.p.long_key, c.p.short_key, c.p.bl, c.p.bs, &mut self.cured);
        }
        for ((idx, _), w) in d.pendings.iter().zip(&created_w) {
            carry(*idx, w.op, w.w.long_key, w.w.short_key, w.w.bl, w.w.bs, &mut self.cured);
        }
        for op in &spent {
            self.cured.remove(op);
        }
        // When each side of a created position entered it: inherited from the input it comes from (the same key on the
        // same side), or now for an entrant.
        let enter = |op: TransactionOutpoint, lk: [u8; 32], sk: [u8; 32], ent: &mut HashMap<(TransactionOutpoint, i64), u64>| {
            for (side, key) in [(LONG, lk), (SHORT, sk)] {
                let from = in_pos.iter().find(|s| (if side == LONG { s.p.long_key } else { s.p.short_key }) == key);
                let at = from.and_then(|s| ent.get(&(s.op, side)).copied()).unwrap_or(now);
                ent.insert((op, side), at);
            }
        };
        for c in &created {
            enter(c.op, c.p.long_key, c.p.short_key, &mut self.entered);
        }
        for w in &created_w {
            enter(w.op, w.w.long_key, w.w.short_key, &mut self.entered);
        }
        for op in &spent {
            self.entered.remove(&(*op, LONG));
            self.entered.remove(&(*op, SHORT));
        }
        self.w.pairs.retain(|l| !spent.contains(&l.op));
        self.w.pairs.extend(created);
        self.w.pendings.retain(|w| !spent.contains(&w.op));
        self.w.pendings.extend(created_w);
        let created_l: Vec<LiveLk> = live_locks(&tx, d).into_iter().map(|l| l.at(now)).collect();
        self.w.locks.retain(|l| !spent.contains(&l.op));
        self.w.locks.extend(created_l);
        self.daa += self.rng.range(1, 3_000) as u64;
        for l in &self.w.pairs {
            assert!(l.value as i64 >= l.p.floor(), "pair below its floor");
            assert!(l.p.bl >= 0 && l.p.bl < l.p.bs && l.p.bs <= l.p.m.price_max);
        }
        tx
    }

    /// Prices at which payoffs are compared: every bankruptcy price of the given pairs and its neighbours, the
    /// price bounds, and random prices.
    fn grid(&mut self, ps: &[&Pair]) -> Vec<i64> {
        let mut g = vec![0, 1, PRICE_MAX];
        for p in ps {
            for b in [p.bl, p.bs, p.entry] {
                for d in [-1, 0, 1] {
                    g.push((b + d).clamp(0, PRICE_MAX));
                }
            }
        }
        for _ in 0..24 {
            g.push(self.rng.range(0, 3_000_000_000));
        }
        g
    }

    /// A party that did not consent: its payoff is not lower at any grid price by more than `slack` sompi.
    fn non_consenting(&mut self, before: &World, key: &[u8; 32], slack: i128, grid: &[i64]) {
        for &p in grid {
            let (b, a) = (before.payoff(key, p), self.w.payoff(key, p));
            let loss = b - a;
            assert!(loss <= slack, "non-consenting party lost {loss} sompi at price {p} (slack {slack})");
            self.st.max_slack = self.st.max_slack.max(loss);
        }
    }

    /// A party that did not act: its idle-settle payout (payoff at each pair's E) did not fall by more than `slack`
    /// (1 sompi of rounding per part its pair was cut into).
    fn idle_kept(&mut self, before: &World, key: &[u8; 32], slack: i128) {
        let (b, a) = (before.idle_payoff(key), self.w.idle_payoff(key));
        assert!(b - a <= slack, "idle payout of a party that did not act fell by {} sompi (slack {slack})", b - a);
    }

    /// A leaver of n from pair `src`: paid at least `owed` (unless below dust), and its remaining position falls by
    /// at most the part it sold, plus 1 sompi of rounding.
    fn leaver(&mut self, before: &World, key: &[u8; 32], src: &Pair, n: i64, owed: i64, long: bool, grid: &[i64]) {
        let got = self.w.cash(key) - before.cash(key);
        if owed >= DUST {
            assert!(got >= owed as i128, "leaver paid {got} < {owed}");
        }
        let part = src.with_size(n);
        for &p in grid {
            let sold = if long { part.long_at(p) } else { part.short_at(p) } as i128;
            let (b, a) = (before.payoff(key, p) - before.cash(key), self.w.payoff(key, p) - self.w.cash(key));
            assert!(a >= b - sold - 1, "leaver's remaining position lost more than the part at {p}: {b} -> {a}, part {sold}");
        }
    }

    // ------------------------------------------------------------ actions

    /// T1 between two fresh orders, with the given range if any.
    fn open(&mut self, range: Option<(i64, i64)>) -> Option<Live> {
        self.open_capped(range, i64::MAX)
    }

    /// T1 as `open`, with the fill price (the new pair's E) at most `x_max` when a range is given. Review 3 (DESIGN
    /// 4.5c): both sides are healthy at x and the range is wider than both maintenance bands, so x lies in
    /// [ceil(bl / (1 - mmr)) + 1, floor(bs / (1 + mmr))] and each random rate is at least mmr * x + 1; `None` when the
    /// given range leaves no such x.
    fn open_capped(&mut self, range: Option<(i64, i64)>, x_max: i64) -> Option<Live> {
        let mm = market().mmr_bps;
        let (x, rl, rs) = match range {
            Some((bl, bs)) => {
                let lo = (bl * 10_000 + 10_000 - mm - 1) / (10_000 - mm) + 1;
                let hi = (bs * 10_000 / (10_000 + mm)).min(x_max);
                if lo > hi {
                    return None;
                }
                let x = self.rng.range(lo, hi);
                (x, x - bl, bs - x)
            }
            None => {
                let x = self.rng.range(500_000_000, 2_000_000_000);
                let need = x * mm / 10_000 + 2;
                (x, self.rng.range(need.max(50_000_000), 500_000_000), self.rng.range(need.max(50_000_000), 500_000_000))
            }
        };
        let n = self.rng.range(MIN, 3_000_000_000);
        let (a, b) = (self.fresh_key(), self.fresh_key());
        let mut lo = long_order(&a);
        (lo.size_left, lo.limit, lo.my_rate, lo.min_counter_rate, lo.min_fill, lo.tip) = (n, x + self.rng.range(0, 10_000_000), rl, self.rng.range(0, rs), MIN, self.rng.range(0, 1_000_000));
        let mut so = short_order(&b);
        (so.size_left, so.limit, so.my_rate, so.min_counter_rate, so.min_fill, so.tip) = (n, x - self.rng.range(0, 10_000_000), rs, self.rng.range(0, rl), MIN, self.rng.range(0, 1_000_000));
        let lo = LiveOpen { value: lo.escrow() as u64, o: lo, id: self.fresh_hash(0xa1), op: self.fresh_op() };
        let so = LiveOpen { value: so.escrow() as u64, o: so, id: self.fresh_hash(0xa2), op: self.fresh_op() };
        let fnn = self.fn_();
        let d = open_draft(&lo, &so, n, x, &self.f.clone(), fnn);
        let tx = self.apply(&d, &[(LONG, lo.id), (SHORT, so.id)]);
        self.st.open += 1;
        Some(self.find(&live_pairs(&tx, &d)[0].op))
    }

    fn find(&self, op: &TransactionOutpoint) -> Live {
        self.w.pairs.iter().find(|l| &l.op == op).expect("live pair").clone()
    }

    /// P1 by the owner of `side`; both parties' payoffs are exactly unchanged.
    fn amend(&mut self, l: &Live, side: i64, w: (i64, i64, i64)) -> Live {
        let key = if side == LONG { l.p.long_key } else { l.p.short_key };
        let other = if side == LONG { l.p.short_key } else { l.p.long_key };
        let signer = self.keys[&key];
        let before = self.w.clone();
        let fnn = self.fn_();
        let d = amend_draft(l, &signer, side, w, &self.f.clone(), fnn);
        let tx = self.apply(&d, &[]);
        let g = self.grid(&[&l.p]);
        self.non_consenting(&before, &other, 0, &g);
        self.non_consenting(&before, &key, 0, &g);
        self.idle_kept(&before, &other, 0);
        self.st.amend += 1;
        self.find(&live_pairs(&tx, &d)[0].op)
    }

    fn random_n(&mut self, s: i64, partial: bool) -> i64 {
        if partial && s >= 2 * MIN {
            for _ in 0..20 {
                let n = self.rng.range(MIN, s - MIN);
                if valid_part(s, n) {
                    return n;
                }
            }
        }
        s
    }

    /// A random pair whose two sides belong to two keys (the actions below check a stayer against a leaver); a
    /// forfeited part, both of whose sides are the winner's, is closed by `coop_forfeited` instead.
    fn pick(&mut self) -> Live {
        let cands: Vec<Live> = self.w.pairs.iter().filter(|l| l.p.long_key != l.p.short_key).cloned().collect();
        if cands.is_empty() {
            return self.open(None).expect("a fresh pair");
        }
        let i = self.rng.range(0, cands.len() as i64 - 1) as usize;
        cands[i].clone()
    }

    /// Forfeit and keep, second half (DESIGN 4.5d): the winner closes a forfeited part with its own two signatures and
    /// receives its whole value.
    fn coop_forfeited(&mut self) {
        let Some(l) = self.w.pairs.iter().find(|l| l.p.long_key == l.p.short_key).cloned() else { return };
        let kp = self.keys[&l.p.long_key];
        let before = self.w.clone();
        let fnn = self.fn_();
        let ins = vec![pair_input(&l, "coopClose", vec![Arg::Sig(kp), Arg::Sig(kp)]), funder(&self.f.clone(), fnn, F_FUNDS)];
        let outs = vec![out(l.value, p2pk_x(&l.p.long_key), None), out(0, p2pk_spk(&self.f), None)];
        let d = Draft::new(ins, outs, Some(1));
        self.apply(&d, &[]);
        assert_eq!(self.w.cash(&l.p.long_key) - before.cash(&l.p.long_key), l.value as i128);
        self.st.coop_forfeited += 1;
    }

    /// P3: list one side, then fill it by a KpfOpen or a direct taker, fully or in part.
    fn transfer(&mut self, by_open: bool, partial: bool) {
        let l = self.pick();
        let side = if self.rng.coin() { LONG } else { SHORT };
        let n = self.random_n(l.p.size, partial);
        let (bl, bs, e) = (l.p.bl, l.p.bs, l.p.entry);
        // A KpfOpen takes only at a price on its own side of the kept E (long x <= E, short x >= E); when the
        // listing leaves no such price, a direct taker fills it instead.
        let lo_a = (bl - 100_000_000).max(1);
        let by_open = by_open && if side == LONG { lo_a <= bs.min(e) } else { bl.max(1).max(e) <= bs + 100_000_000 };
        let (ask, x) = if side == LONG {
            let top = if by_open { bs.min(e) } else { bs };
            let a = self.rng.range(lo_a, top);
            (a, self.rng.range(a, top))
        } else {
            let low = if by_open { bl.max(e) } else { bl };
            let b = self.rng.range(bl.max(1).max(low), bs + 100_000_000);
            (b, self.rng.range(low, b))
        };
        let w = (ask, self.rng.range(0, n), self.rng.range(0, 500_000));
        let l = self.amend(&l, side, w);
        let before = self.w.clone();
        let p = l.p.clone();
        let entrant = self.fresh_key();
        let fnn = self.fn_();
        let (spec, ent) = if by_open {
            let mut o = if side == LONG { long_order(&entrant) } else { short_order(&entrant) };
            o.size_left = n + if self.rng.coin() { MIN * self.rng.range(1, 50) } else { 0 };
            o.min_fill = MIN;
            o.tip = self.rng.range(0, 1_000_000);
            o.min_counter_rate = 0;
            let id = self.fresh_hash(0xb1);
            let nb;
            // Review 3 (DESIGN 4.5c): the entrant is healthy at x, so its rate is at least mmr * x.
            let need = (x * p.m.mmr_bps + 9_999) / 10_000;
            if side == LONG {
                let lo = (x - bl).max(need).max(1);
                o.my_rate = self.rng.range(lo, x.max(lo));
                o.limit = x + self.rng.range(0, 1_000_000);
                nb = x - o.my_rate;
            } else {
                let lo = (bs - x).max(need).max(1);
                o.my_rate = self.rng.range(lo, lo + 500_000_000);
                o.limit = x - self.rng.range(0, x.min(1_000_000));
                nb = x + o.my_rate;
            }
            let lo = LiveOpen { value: o.escrow() as u64, o, id, op: self.fresh_op() };
            (TransferSpec { side, n, x, nb, ek: xonly(&entrant), et: id, k: self.rng.range(0, 1) }, Entrant::Open(lo))
        } else {
            // Review 3: the direct taker's bound is also healthy at x (at most x * (1 - mmr) for a long, at least
            // x * (1 + mmr) for a short) and keeps no-downgrade.
            let mm = p.m.mmr_bps;
            let nb = if side == LONG {
                let top = bl.min(x * (10_000 - mm) / 10_000);
                top - self.rng.range(0, top.min(200_000_000))
            } else {
                bs.max((x * (10_000 + mm) + 9_999) / 10_000) + self.rng.range(0, 200_000_000)
            };
            // A direct taker writes any tag it likes (review 1, F1/F2): 0, the pair's own id, the token's id, or
            // a random id. None may freeze the pair.
            let et = match self.rng.range(0, 3) {
                0 => ZERO,
                1 => l.id,
                2 => l.p.m.token_cov_id,
                _ => self.fresh_hash(0xd7),
            };
            (TransferSpec { side, n, x, nb, ek: xonly(&entrant), et, k: self.rng.range(0, 1) }, Entrant::Direct(entrant))
        };
        let d = transfer_draft(&l, &spec, &ent, &self.f.clone(), fnn);
        self.apply(&d, &[(side, spec.et)]);
        let mut ps: Vec<&Pair> = d.pairs.iter().map(|(_, p)| p).collect();
        ps.push(&p);
        let ps: Vec<Pair> = ps.into_iter().cloned().collect();
        let g = self.grid(&ps.iter().collect::<Vec<_>>());
        let split = (n < p.size) as i128;
        let (stayer, leaver) = if side == LONG { (p.short_key, p.long_key) } else { (p.long_key, p.short_key) };
        self.non_consenting(&before, &stayer, split, &g);
        self.idle_kept(&before, &stayer, split);
        let owed = if side == LONG { owed(n, x - bl, p.long_tip, SCALE) } else { owed(n, bs - x, p.short_tip, SCALE) };
        self.leaver(&before, &leaver, &p, n, owed, side == LONG, &g);
        if split == 1 {
            self.st.split += 1;
        }
        if by_open {
            self.st.transfer_open += 1;
        } else {
            self.st.transfer_direct += 1;
        }
    }

    /// P4 close-versus-close between a host (long listing) and a donor (short listing) that satisfy no-downgrade,
    /// opening a suitable donor when the world has none.
    fn close(&mut self) {
        let h = self.pick();
        // No-downgrade both ways, and E(donor) <= E(host) so that some e3 exists (DESIGN 4.5a). Tags play no part.
        // The four parties are distinct keys (a long that left the host and moves from the donor in one
        // transaction acted as a leaver, and the stayer checks below would not apply to it).
        let ok = |h: &Live, d: &Live| {
            let hk = [h.p.long_key, h.p.short_key];
            h.id != d.id
                && d.p.long_key != d.p.short_key
                && d.p.bl <= h.p.bl
                && d.p.bs <= h.p.bs
                && d.p.entry <= h.p.entry
                && !hk.contains(&d.p.long_key)
                && !hk.contains(&d.p.short_key)
        };
        let cands: Vec<Live> = self.w.pairs.iter().filter(|d| ok(&h, d)).cloned().collect();
        let d = if cands.is_empty() || self.rng.coin() {
            let bl = h.p.bl - self.rng.range(0, h.p.bl.min(200_000_000));
            let bs = h.p.bs - self.rng.range(0, (h.p.bs - bl - 2).min(200_000_000));
            let Some(d) = self.open_capped(Some((bl, bs)), h.p.entry) else { return };
            if d.p.entry > h.p.entry {
                return;
            }
            d
        } else {
            let i = self.rng.range(0, cands.len() as i64 - 1) as usize;
            cands[i].clone()
        };
        let h = self.find(&h.op);
        let mut n = 0;
        for _ in 0..30 {
            let m = h.p.size.min(d.p.size);
            let c = if self.rng.coin() { m } else { self.rng.range(MIN, m) };
            if valid_part(h.p.size, c) && valid_part(d.p.size, c) {
                n = c;
                break;
            }
        }
        if n == 0 {
            return;
        }
        let a = self.rng.range((h.p.bl - 100_000_000).max(1), h.p.bs);
        let b = a + self.rng.range(0, 100_000_000);
        let x = self.rng.range(a, b);
        let hw = (a, self.rng.range(0, n), self.rng.range(0, 500_000));
        let dw = (b, self.rng.range(0, n), self.rng.range(0, 500_000));
        let h = self.amend(&h, LONG, hw);
        let d = self.amend(&d, SHORT, dw);
        let before = self.w.clone();
        let fnn = self.fn_();
        let e3 = self.rng.range(d.p.entry, h.p.entry);
        let dr = close_draft(&h, &d, &CloseSpec { n, x, e3 }, &self.f.clone(), fnn);
        self.apply(&dr, &[(LONG, d.id)]);
        self.idle_kept(&before, &h.p.short_key, (n < h.p.size) as i128);
        self.idle_kept(&before, &d.p.long_key, (n < d.p.size) as i128);
        let mut ps: Vec<Pair> = dr.pairs.iter().map(|(_, p)| p.clone()).collect();
        ps.push(h.p.clone());
        ps.push(d.p.clone());
        let g = self.grid(&ps.iter().collect::<Vec<_>>());
        self.non_consenting(&before, &h.p.short_key, (n < h.p.size) as i128, &g);
        self.non_consenting(&before, &d.p.long_key, (n < d.p.size) as i128, &g);
        self.leaver(&before, &h.p.long_key, &h.p, n, owed(n, x - h.p.bl, h.p.long_tip, SCALE), true, &g);
        self.leaver(&before, &d.p.short_key, &d.p, n, owed(n, d.p.bs - x, d.p.short_tip, SCALE), false, &g);
        self.st.close_vs_close += 1;
        if n < h.p.size || n < d.p.size {
            self.st.close_partial += 1;
        }
    }

    /// P4 mutual close: both listings of one pair cross.
    fn mutual(&mut self) {
        let l = self.pick();
        let partial = self.rng.coin();
        let n = self.random_n(l.p.size, partial);
        let a = self.rng.range(l.p.bl.max(1), l.p.bs);
        let b = a + self.rng.range(0, 100_000_000);
        let x = self.rng.range(a, b);
        let lw = (a, self.rng.range(0, n), self.rng.range(0, 500_000));
        let sw = (b, self.rng.range(0, n), self.rng.range(0, 500_000));
        let l = self.amend(&l, LONG, lw);
        let l = self.amend(&l, SHORT, sw);
        let before = self.w.clone();
        let fnn = self.fn_();
        let d = mutual_draft(&l, n, x, &self.f.clone(), fnn);
        self.apply(&d, &[]);
        let g = self.grid(&[&l.p]);
        let p = &l.p;
        self.leaver(&before, &p.long_key, p, n, owed(n, x - p.bl, p.long_tip, SCALE), true, &g);
        self.leaver(&before, &p.short_key, p, n, owed(n, p.bs - x, p.short_tip, SCALE), false, &g);
        self.st.mutual += 1;
    }

    // ------------------------------------------------------------ two-step evidence paths (DESIGN 4.5d)

    /// One proposal on a random pair: P5 by either side, or P6 of a liquidatable side with a fresh proposer, at a
    /// random quote with a fill of exactly ceil(k n) (or more), whole or in part. A third of the orders are fresh; the
    /// others were placed around the pair's clock start S with a soft expiry that may end before the order has rested R
    /// (and L after it) on the pair's clock, and half of those are used with the lock time of review 2. The engine must
    /// accept exactly when the order was fillable through max(exposed, S) + R + L and the lock time proves
    /// max(exposed, S) + R. After an accepted proposal every party's payoff is unchanged (the pending part counts as
    /// its restore state), up to the rounding of a split.
    fn propose(&mut self) {
        use kpf::evidence::*;
        let mut l = self.pick();
        // Review 4: half of the proposals are made on a listed pair, so that a restore or a cure has a listing to drop.
        if self.rng.coin() {
            let side = if self.rng.coin() { LONG } else { SHORT };
            let px = self.rng.range(l.p.bl.max(1), l.p.bs);
            let n = l.p.size;
            let w = (px, self.rng.range(0, n), self.rng.range(0, 500_000));
            l = self.amend(&l, side, w);
        }
        let p = l.p.clone();
        let m = p.m.clone();
        let partial = self.rng.coin();
        let n = self.random_n(p.size, partial);
        let what = self.rng.range(0, 3);
        let a_max = (p.bl * 10_000 - 1) / (10_000 - m.mmr_bps);
        let b_min = p.bs * 10_000 / (10_000 + m.mmr_bps) + 1;
        let fill = n * m.k_bps / 10_000 + if self.rng.coin() { 0 } else { self.rng.range(0, n) };
        let (long, short) = (p.long_key, p.short_key);
        let proposer = self.fresh_key();
        let (act, e) = match what {
            0 => {
                let b = self.rng.range((p.bl - 100_000_000).max(1), p.bs + 100_000_000);
                (Act::Close { side: LONG, signer: self.keys[&long] }, Evidence::bid(b, fill))
            }
            1 => {
                let a = self.rng.range((p.bl - 100_000_000).max(1), p.bs + 100_000_000);
                (Act::Close { side: SHORT, signer: self.keys[&short] }, Evidence::ask(a, fill))
            }
            2 if a_max >= 1 => {
                let a = self.rng.range((a_max - 300_000_000).max(1), a_max);
                (Act::LiqBy { side: LONG, proposer: xonly(&proposer) }, Evidence::ask(a, fill))
            }
            3 if b_min <= PRICE_MAX => {
                let b = self.rng.range(b_min, (b_min + 300_000_000).min(PRICE_MAX));
                (Act::LiqBy { side: SHORT, proposer: xonly(&proposer) }, Evidence::bid(b, fill))
            }
            _ => return,
        };
        // Review 4: against a cured side, half of the time a quote within g of the cured claim (it must be refused).
        let mut forced_within = false;
        let mut e = e;
        if let Act::LiqBy { side, .. } = &act {
            if let Some(&(cs, _, claim)) = self.cured.get(&l.op) {
                if cs == *side && self.rng.coin() {
                    let reach = kpf_protocol::pending::cure_reach_of(&m, cs, claim);
                    let price = if cs == LONG { self.rng.range(reach, claim) } else { self.rng.range(claim, reach) };
                    e = if cs == LONG { Evidence::ask(price, fill) } else { Evidence::bid(price, fill) };
                    forced_within = true;
                }
            }
        }
        let r = m.min_rest_daa as u64;
        let life = m.min_evidence_life;
        let s_clock = l.clock() as u64;
        let mut e = e;
        let stale = self.rng.range(0, 2) > 0;
        let expiry;
        if stale {
            let at = self.rng.range(s_clock.saturating_sub(3 * r) as i64, (s_clock + r) as i64) as u64;
            (e.daa, e.tok_daa) = (at, at);
            expiry = self.rng.range((at + r) as i64, (s_clock + 2 * r) as i64) + if self.rng.coin() { life } else { 0 };
            e.ask.expiry_daa = expiry;
            e.bid.expiry_daa = expiry;
        } else {
            (e.daa, e.tok_daa) = (self.daa, self.daa);
            expiry = if e.kind == Kind::Ask { e.ask.expiry_daa } else { e.bid.expiry_daa };
        }
        let rest_end = e.daa.max(s_clock) + r;
        let fnn = self.fn_();
        let g = PairLeg { l: l.clone(), n, q: 0, act: act.clone() };
        let mut d = ev_draft(&[g.clone()], &e, &self.f.clone(), fnn);
        d.lock = if stale && self.rng.coin() { e.daa + r } else { rest_end.max(self.daa) + self.rng.range(0, 50) as u64 };
        // DESIGN 4.5f: half of the proposals offer part of the bond to a guarantor (written into the pending part).
        if self.rng.coin() {
            let pidx = d.pendings[0].0;
            let mut w = d.pendings[0].1.clone();
            w.offer = self.rng.range(0, w.bond - DUST);
            let mut a = d.args(0);
            a[ARG_OFFER] = iv(w.offer);
            d.set_args(0, a);
            d.outs[pidx].script_public_key = p2sh(&w.art());
            d.pendings[0].1 = w;
        }
        let predicted = expiry >= rest_end as i64 + life && d.lock >= rest_end && !forced_within;
        let (tx0, en0) = d.build();
        let ok = run(&tx0, &en0).unwrap().iter().all(|x| x.is_ok());
        assert_eq!(ok, predicted, "evidence at DAA {} (expiry {expiry}) on a pair whose clock starts at {s_clock}, lock {}: the clock rule (within g of a cure: {forced_within})", e.daa, d.lock);
        if !ok {
            self.st.ev_refused += 1;
            if forced_within {
                self.st.within_g_refused += 1;
            }
            return;
        }
        if let Act::LiqBy { side, .. } = &act {
            let entry = self.entered[&(l.op, *side)];
            let start = e.daa.max(entry);
            assert!(expiry >= (start + r) as i64 + life && d.lock >= start + r, "a liquidation used evidence from before the loser entered (DAA {entry})");
            // Review 4: against a cured side, no accepted proposal uses a quote within g of the cured claim.
            if let Some(&(cs, _, claim)) = self.cured.get(&l.op) {
                if cs == *side {
                    assert!(!within_g(cs, claim, e.price(), m.g_bps), "a cured claim at {claim} was reopened by the quote {} within g", e.price());
                }
            }
        }
        let before = self.w.clone();
        self.apply(&d, &[]);
        let w = &d.pendings[0].1;
        self.st.bonds_posted += w.bond as i128;
        // Nothing changes hands at a proposal: every party keeps its payoff at every price, up to one split.
        let g = self.grid(&[&p]);
        let split = (n < p.size) as i128;
        for k in [long, short] {
            self.non_consenting(&before, &k, split, &g);
            self.idle_kept(&before, &k, split);
        }
        if matches!(act, Act::Close { .. }) {
            self.st.ev_close += 1;
        } else {
            self.st.ev_liq += 1;
        }
        if split == 1 {
            self.st.split += 1;
        }
    }

    fn pick_pending(&mut self) -> Option<LivePend> {
        if self.w.pendings.is_empty() {
            return None;
        }
        let i = self.rng.range(0, self.w.pendings.len() as i64 - 1) as usize;
        Some(self.w.pendings[i].clone())
    }

    /// A challenge of a random pending part with a counter quote at or beyond its refutation bound (or one tick short),
    /// from an order placed before or after P, sometimes short-lived. Accepted exactly as the module docs say; then the
    /// part is restored with every owner's payoff unchanged, the proposer gets nothing and the bond is the challenger's.
    fn challenge(&mut self) {
        use kpf::evidence::*;
        let Some(w) = self.pick_pending() else { return };
        let Some(bound) = w.w.contradiction_bound() else { return };
        let m = w.w.m.clone();
        let (r, life) = (m.min_rest_daa as u64, m.min_evidence_life);
        let ask = w.w.counter_is_ask();
        let short_of = self.rng.range(0, 4) == 0;
        let quote = match (ask, short_of) {
            (true, false) => bound - self.rng.range(0, bound.min(20_000_000)),
            (true, true) => bound + 1,
            (false, false) => bound + self.rng.range(0, (PRICE_MAX - bound).min(20_000_000)),
            (false, true) => bound - 1,
        };
        if quote < 1 || quote > PRICE_MAX {
            return;
        }
        let n = w.w.n;
        let fill = n * m.kc_bps / 10_000 + if self.rng.coin() { 0 } else { self.rng.range(0, n) };
        let mut e = if ask { Evidence::ask(quote, fill) } else { Evidence::bid(quote, fill) };
        let p_clock = w.daa;
        let before_p = self.rng.coin();
        let at = if before_p { p_clock.saturating_sub(self.rng.range(0, 3 * r as i64) as u64) } else { (p_clock + self.rng.range(0, r as i64) as u64).max(1) };
        (e.daa, e.tok_daa) = (at, at);
        let rest_end = at.max(p_clock) + r;
        let short_lived = self.rng.range(0, 3) == 0;
        let expiry = if short_lived { rest_end as i64 + life - self.rng.range(0, 1) } else { 1_000_000_000_000 };
        e.ask.expiry_daa = expiry;
        e.bid.expiry_daa = expiry;
        let fnn = self.fn_();
        let mut d = ch_draft(&[w.clone()], &e, &self.f.clone(), fnn);
        d.lock = if before_p && self.rng.coin() { at + r } else { rest_end.max(self.daa) + self.rng.range(0, 50) as u64 };
        let predicted = !short_of && expiry >= rest_end as i64 + life && d.lock >= rest_end;
        let (tx0, en0) = d.build();
        let ok = run(&tx0, &en0).unwrap().iter().all(|x| x.is_ok());
        assert_eq!(ok, predicted, "challenge at {quote} (bound {bound}) by an order of DAA {at} expiring {expiry} on P = {p_clock}, lock {}", d.lock);
        if !ok {
            self.st.challenge_refused += 1;
            return;
        }
        let before = self.w.clone();
        self.apply(&d, &[]);
        assert_eq!(d.outs[0].value as i64, w.value as i64 - w.w.bond, "the restored part holds the value less the bond");
        assert_eq!(self.w.cash(&w.w.proposer_key), before.cash(&w.w.proposer_key), "a refuted proposer gets nothing back");
        self.st.bonds_to_challengers += w.w.bond as i128;
        let g = self.grid(&[&w.w.pair_view()]);
        for k in [w.w.long_key, w.w.short_key] {
            self.non_consenting(&before, &k, 0, &g);
        }
        self.st.challenged += 1;
    }

    /// A cure by the loser of a random pending liquidation: the proposer gets exactly its refund, the winner's payoff
    /// does not fall at any price (the loser's bound only moves outward).
    fn cure(&mut self) {
        let Some(w) = self.pick_pending() else { return };
        if !w.w.can_cure() {
            return;
        }
        let loser = if w.w.side == LONG { w.w.long_key } else { w.w.short_key };
        let signer = self.keys[&loser];
        let before = self.w.clone();
        let fnn = self.fn_();
        let d = cure_draft(&w, &signer, &self.f.clone(), fnn);
        let tx = self.apply(&d, &[]);
        assert_eq!(self.w.cash(&w.w.proposer_key) - before.cash(&w.w.proposer_key), w.w.refund as i128, "a cure repays the proposer its refund");
        self.st.bonds_to_proposers += w.w.bond as i128;
        let winner = w.w.winner_key();
        if winner != loser && winner != w.w.proposer_key {
            let g = self.grid(&[&w.w.pair_view(), &d.pairs[0].1]);
            self.non_consenting(&before, &winner, 0, &g);
        }
        self.st.cured += 1;
        let c = self.find(&live_pairs(&tx, &d)[0].op);
        self.cured.insert(c.op, (w.w.side, w.w.cure_bound, w.w.quote));
        self.probe_cure(&c, w.w.side, w.w.quote);
    }

    /// Review 4 (DESIGN 4.5e), run without applying: against a freshly cured pair, a liquidation on the farthest quote
    /// within g of the cured claim (fresh evidence, rested, live) is refused, and one on the first quote that triggers the
    /// cure bound is accepted; that quote lies beyond g.
    fn probe_cure(&mut self, c: &Live, side: i64, claim: i64) {
        use kpf::evidence::*;
        let m = c.p.m.clone();
        let (b, mm) = (10_000i128, m.mmr_bps as i128);
        let reach = kpf_protocol::pending::cure_reach_of(&m, side, claim);
        let first = if side == LONG {
            ((c.p.bl as i128 * b + b - mm - 1) / (b - mm) - 1) as i64
        } else {
            (c.p.bs as i128 * b / (b + mm) + 1) as i64
        };
        assert!(!within_g(side, claim, first, m.g_bps), "the first quote that reopens a cure lies beyond g");
        let n = c.p.size;
        let fill = n * m.k_bps / 10_000 + 1;
        let r = m.min_rest_daa as u64;
        for (price, ok) in [(reach, false), (first, true)] {
            if price < 1 || price > m.price_max {
                continue;
            }
            let e = if side == LONG { Evidence::ask(price, fill) } else { Evidence::bid(price, fill) }.placed(self.daa);
            let act = Act::LiqBy { side, proposer: xonly(&keypair(77)) };
            let fnn = self.fn_();
            let mut d = ev_draft(&[PairLeg { l: c.clone(), n, q: 0, act }], &e, &self.f.clone(), fnn);
            d.lock = self.daa + r;
            let (tx, en) = d.build();
            let got = run(&tx, &en).unwrap().iter().all(|x| x.is_ok());
            assert_eq!(got, ok, "a liquidation of a cured pair at {price} (claim {claim}, reach {reach}, side {side})");
            if ok {
                self.st.beyond_cure_accepted += 1;
            } else {
                self.st.within_g_refused += 1;
            }
        }
    }

    /// Review 4 (DESIGN 4.5e): a liquidation of a part whose two sides have one owner (a forfeited part), at a quote that
    /// triggers it by price, with evidence that rested on its clock and stays live, by a fresh proposer, is refused.
    fn propose_one_owner(&mut self) {
        use kpf::evidence::*;
        let Some(l) = self.w.pairs.iter().find(|l| l.p.long_key == l.p.short_key).cloned() else { return };
        let m = l.p.m.clone();
        if !l.p.range_ok() {
            return;
        }
        let side = if self.rng.coin() { LONG } else { SHORT };
        let n = l.p.size;
        let fill = n * m.k_bps / 10_000 + 1;
        let e = if side == LONG {
            let a = (l.p.bl * 10_000 - 1) / (10_000 - m.mmr_bps);
            if a < 1 {
                return;
            }
            Evidence::ask(a, fill)
        } else {
            let b = l.p.bs * 10_000 / (10_000 + m.mmr_bps) + 1;
            if b > m.price_max {
                return;
            }
            Evidence::bid(b, fill)
        }
        .placed(self.daa);
        let proposer = self.fresh_key();
        let fnn = self.fn_();
        let mut d = ev_draft(&[PairLeg { l: l.clone(), n, q: 0, act: Act::LiqBy { side, proposer: xonly(&proposer) } }], &e, &self.f.clone(), fnn);
        d.lock = (self.daa.max(l.clock() as u64)) + m.min_rest_daa as u64;
        let (tx, en) = d.build();
        let r = run(&tx, &en).unwrap();
        assert!(r[0].is_err(), "a liquidation of a one-owner part was accepted");
        // Control: the same draft with a second key on the part is accepted, so the keys are what refuses it.
        let mut two = l.clone();
        two.p.short_key = xonly(&proposer);
        let mut d2 = ev_draft(&[PairLeg { l: two, n, q: 0, act: Act::LiqBy { side, proposer: xonly(&proposer) } }], &e, &self.f.clone(), fnn);
        d2.lock = d.lock;
        let (tx2, en2) = d2.build();
        assert!(run(&tx2, &en2).unwrap().iter().all(|x| x.is_ok()), "the control with two keys");
        self.st.one_owner_refused += 1;
    }

    /// A finalize of a random pending part once W old. A close pays the closer its payoff at the clamped quote plus the
    /// refund and leaves the other party its payoff at the quote; a liquidation forfeits the loser's side, the winner
    /// loses at most the fee at any price, and the proposer gets exactly bond and fee.
    fn finalize(&mut self) {
        let Some(w) = self.pick_pending() else { return };
        // The relative lock: the chain moves to P + W.
        self.daa = self.daa.max(w.daa + w.w.m.challenge_daa as u64);
        let before = self.w.clone();
        let fnn = self.fn_();
        let d = finalize_draft(&w, &self.f.clone(), fnn);
        self.apply(&d, &[]);
        let pv = w.w.pair_view();
        let split_dust = |v: i64| if v > 0 && v < DUST { DUST as i128 } else { 0 };
        if w.w.kind == KIND_CLOSE {
            let (lp, sp) = w.w.finalize_close(w.value);
            let (closer, other, pay) = if w.w.side == LONG { (w.w.long_key, w.w.short_key, lp) } else { (w.w.short_key, w.w.long_key, sp) };
            assert_eq!(self.w.cash(&closer) - before.cash(&closer), pay as i128, "the closer is paid its payoff and the refund");
            self.st.bonds_to_proposers += w.w.bond as i128;
            if other != closer {
                let c = w.w.quote.clamp(w.w.bl, w.w.bs);
                let owed_other = if w.w.side == LONG { q(w.w.n, w.w.bs - c, SCALE, 0) } else { q(w.w.n, c - w.w.bl, SCALE, 0) };
                self.at_quote(&before, &other, w.w.quote, split_dust(owed_other) + 1);
            }
            self.st.finalized_close += 1;
        } else {
            assert_eq!(self.w.cash(&w.w.proposer_key) - before.cash(&w.w.proposer_key), (w.w.bond + w.w.fee) as i128, "the proposer gets bond and fee");
            self.st.bonds_to_proposers += w.w.bond as i128;
            let winner = w.w.winner_key();
            if winner != w.w.proposer_key {
                let g = self.grid(&[&pv, &d.pairs[0].1]);
                // The forfeit bound moves in whole ticks: the range floor may sit up to one tick (q(n, 1)) below the
                // value the part keeps, which stays in the UTXO as excess, outside the DESIGN 3.1 payoff.
                let tick = q(w.w.n, 1, SCALE, SCALE - 1) as i128;
                self.non_consenting(&before, &winner, w.w.fee as i128 + tick + 2, &g);
            }
            self.st.finalized_liq += 1;
        }
    }

    /// A party's payoff at price `p` is not lower than before by more than `slack`.
    fn at_quote(&mut self, before: &World, key: &[u8; 32], p: i64, slack: i128) {
        let (b, a) = (before.payoff(key, p), self.w.payoff(key, p));
        assert!(b - a <= slack, "party lost {} sompi at the evidence quote {p} (slack {slack})", b - a);
    }

    /// P8 on one pair: both sides are paid their DESIGN 3.1 payout at the clamped entry, the short also the rest.
    fn idle(&mut self) {
        let l = self.pick();
        let before = self.w.clone();
        let fnn = self.fn_();
        let d = idle_draft(&l, &self.f.clone(), fnn);
        self.apply(&d, &[]);
        let p = &l.p;
        let gl = self.w.cash(&p.long_key) - before.cash(&p.long_key);
        let gs = self.w.cash(&p.short_key) - before.cash(&p.short_key);
        let lp = p.long_at(p.entry);
        if lp >= DUST {
            assert!(gl >= lp as i128, "idle: long {gl} < {lp}");
        }
        assert_eq!(gl + gs, l.value as i128, "idle: the pair's whole value goes to its two parties");
        self.st.idle += 1;
    }
}

impl Sim {
    /// DESIGN 4.5f: a guarantee of a random pending part by a fresh guarantor (a liquidation once G old). The settlement
    /// is finalize's with the proposer paid its offer less: the proposer's cash grows by exactly that, the lock holds
    /// exactly lambda x cover + offer, and every party that did not act is treated as at finalize (the loser forfeits, the
    /// winner pays at most the fee, the other party of a close keeps its payoff at the quote).
    fn guarantee(&mut self) {
        use kpf::evidence::*;
        let Some(w) = self.pick_pending() else { return };
        if w.w.kind == KIND_LIQ {
            self.daa = self.daa.max(w.daa + w.w.m.grace_daa as u64);
        }
        assert!(w.w.guaranteeable_at(w.daa), "every part of the walk was accepted while its evidence was visible");
        let gk = xonly(&self.fresh_key());
        let before = self.w.clone();
        let fnn = self.fn_();
        let d = guarantee_draft(&w, &gk, &self.f.clone(), fnn);
        self.apply(&d, &[]);
        let lock = self.w.locks.iter().find(|l| l.l.claim_id == w.id && l.l.clock == w.daa as i64).cloned().expect("the lock is live");
        assert_eq!(lock.value as i64, w.w.lock_min(w.value), "the lock holds exactly lambda x cover + offer");
        assert_eq!(lock.l.cover, w.w.guarantee_cover(w.value));
        self.st.lock_value_in += lock.value as i128;
        self.st.bonds_to_proposers += w.w.bond as i128;
        if w.w.offer > 0 {
            self.st.offers += 1;
        }
        let pv = w.w.pair_view();
        let split_dust = |v: i64| if v > 0 && v < DUST { DUST as i128 } else { 0 };
        let settled;
        if w.w.kind == KIND_CLOSE {
            let (lp, sp) = w.w.guarantee_close(w.value);
            let (closer, other, pay) = if w.w.side == LONG { (w.w.long_key, w.w.short_key, lp) } else { (w.w.short_key, w.w.long_key, sp) };
            assert_eq!(self.w.cash(&closer) - before.cash(&closer), pay as i128, "the closer is paid its payoff and refund less its offer");
            if other != closer {
                let c = w.w.quote.clamp(w.w.bl, w.w.bs);
                let owed_other = if w.w.side == LONG { q(w.w.n, w.w.bs - c, SCALE, 0) } else { q(w.w.n, c - w.w.bl, SCALE, 0) };
                self.at_quote(&before, &other, w.w.quote, split_dust(owed_other) + 1);
            }
            settled = if w.w.side == LONG { sp } else { lp };
            self.st.guaranteed_close += 1;
        } else {
            let (_, pay) = w.w.guarantee_forfeit(w.value);
            assert_eq!(self.w.cash(&w.w.proposer_key) - before.cash(&w.w.proposer_key), pay as i128, "the proposer gets bond and fee less its offer");
            let winner = w.w.winner_key();
            if winner != w.w.proposer_key {
                let g = self.grid(&[&pv, &d.pairs[0].1]);
                let tick = q(w.w.n, 1, SCALE, SCALE - 1) as i128;
                self.non_consenting(&before, &winner, w.w.fee as i128 + tick + 2, &g);
            }
            settled = 0;
            self.st.guaranteed_liq += 1;
        }
        self.lock_src.insert(lock.op, (w, settled));
    }

    /// DESIGN 4.5f: a dispute of a random lock with a counter quote at or beyond its refutation bound (or one tick short),
    /// from an order placed before or after P, sometimes short-lived, sometimes resting until after the window. Accepted
    /// exactly when a challenge of the claim would have been (clock P, rest R, life L, margin g) and the rest ended by
    /// F + D - R. Then the victim is paid H (and is whole at the counter quote: what the settlement paid it plus H is at
    /// least what its side was worth there), the lock is exactly H + burn + reward, and nobody else's payoff moves.
    fn dispute_lock(&mut self) {
        use kpf::evidence::*;
        if self.w.locks.is_empty() {
            return;
        }
        let i = self.rng.range(0, self.w.locks.len() as i64 - 1) as usize;
        let lock = self.w.locks[i].clone();
        let Some(bound) = lock.l.contradiction_bound() else { return };
        let m = lock.l.m.clone();
        let (r, life) = (m.min_rest_daa as u64, m.min_evidence_life);
        let ask = lock.l.counter_is_ask();
        let short_of = self.rng.range(0, 4) == 0;
        let quote = match (ask, short_of) {
            (true, false) => bound - self.rng.range(0, bound.min(200_000_000)),
            (true, true) => bound + 1,
            (false, false) => bound + self.rng.range(0, (PRICE_MAX - bound).min(200_000_000)),
            (false, true) => bound - 1,
        };
        if quote < 1 || quote > PRICE_MAX {
            return;
        }
        let n = lock.l.n;
        let fill = n * m.kc_bps / 10_000 + if self.rng.coin() { 0 } else { self.rng.range(0, n) };
        let mut e = if ask { Evidence::ask(quote, fill) } else { Evidence::bid(quote, fill) };
        let p_clock = lock.l.clock as u64;
        let deadline = lock.daa as i64 + m.dispute_daa - m.min_rest_daa;
        let at = match self.rng.range(0, 3) {
            0 => p_clock.saturating_sub(self.rng.range(0, 3 * r as i64) as u64),
            1 => (p_clock + self.rng.range(0, r as i64) as u64).max(1),
            // Near the end of the window: the rest ends a little before or after F + D - R.
            _ => (deadline - r as i64 + self.rng.range(-3, 3)).max(1) as u64,
        };
        (e.daa, e.tok_daa) = (at, at);
        let rest_end = at.max(p_clock) + r;
        let short_lived = self.rng.range(0, 3) == 0;
        let expiry = if short_lived { rest_end as i64 + life - self.rng.range(0, 1) } else { 1_000_000_000_000 };
        e.ask.expiry_daa = expiry;
        e.bid.expiry_daa = expiry;
        let fnn = self.fn_();
        let mut d = dp_draft(&[lock.clone()], &e, &self.f.clone(), fnn);
        d.lock = rest_end.max(self.daa) + self.rng.range(0, 50) as u64;
        let in_window = rest_end as i64 <= deadline;
        let predicted = !short_of && expiry >= rest_end as i64 + life && in_window;
        let (tx0, en0) = d.build();
        let ok = run(&tx0, &en0).unwrap().iter().all(|x| x.is_ok());
        assert_eq!(ok, predicted, "dispute at {quote} (bound {bound}) by an order of DAA {at} expiring {expiry} on P = {p_clock}, F = {}, window end {deadline}", lock.daa);
        if !ok {
            self.st.dispute_refused += 1;
            if !in_window {
                self.st.dispute_out_of_window += 1;
            }
            return;
        }
        let before = self.w.clone();
        let tx = self.apply(&d, &[]);
        let (w, settled) = self.lock_src.remove(&lock.op).expect("every lock of the walk comes from a guarantee");
        let h = lock.l.restitution(quote);
        let pay = lock.l.victim_pay(quote);
        let victim = lock.l.victim;
        assert_eq!(victim, w.w.defender_key());
        assert_eq!(self.w.cash(&victim) - before.cash(&victim), pay as i128, "the victim is paid the restitution");
        // Whole at the counter quote: the loser's side of a liquidation, the other party's payout of a close at c.
        let c = quote.clamp(w.w.bl, w.w.bs);
        let fair = if w.w.kind == KIND_LIQ {
            if w.w.side == LONG { q(w.w.n, c - w.w.bl, SCALE, 0) } else { q(w.w.n, w.w.bs - c, SCALE, 0) }
        } else {
            let owe = if w.w.side == LONG { q(w.w.n, c - w.w.bl, SCALE, 0) } else { q(w.w.n, w.w.bs - c, SCALE, 0) };
            w.value as i64 - w.w.refund - owe
        };
        assert!(settled + h >= fair, "the wronged party ends with {} < {fair}, what its side was worth at {quote}", settled + h);
        let burn = lock.l.burn(lock.value, quote);
        if burn > 0 {
            assert_eq!(tx.outputs.last().unwrap().script_public_key, kpf_protocol::lock::burn_spk());
        }
        let reward = lock.value as i64 - pay - burn;
        assert!(reward >= lock.l.reward() && reward <= lock.l.reward() + DUST, "the executor keeps the reward (plus a burn below dust)");
        self.st.lock_to_victims += pay as i128;
        self.st.lock_burned += burn as i128;
        self.st.lock_rewards += reward as i128;
        // Nobody else's payoff moves: a dispute touches no position.
        let g = self.grid(&[&w.w.pair_view()]);
        for k in [w.w.long_key, w.w.short_key] {
            if k != victim {
                self.non_consenting(&before, &k, 0, &g);
            }
        }
        self.st.disputed += 1;
    }

    /// DESIGN 4.5f: a release of a random lock once D old: the whole lock to its owner.
    fn release_lock(&mut self) {
        use kpf::evidence::*;
        if self.w.locks.is_empty() {
            return;
        }
        let i = self.rng.range(0, self.w.locks.len() as i64 - 1) as usize;
        let lock = self.w.locks[i].clone();
        self.daa = self.daa.max(lock.daa + lock.l.m.dispute_daa as u64);
        let before = self.w.clone();
        let fnn = self.fn_();
        let d = release_draft(&lock, &self.f.clone(), fnn);
        self.apply(&d, &[]);
        assert_eq!(self.w.cash(&lock.l.owner) - before.cash(&lock.l.owner), lock.value as i128, "the guarantor gets its whole lock back");
        self.lock_src.remove(&lock.op);
        self.st.lock_released += lock.value as i128;
        self.st.released += 1;
    }
}

/// The X6 evidence walk with guaranteed fast settlement mixed in (DESIGN 4.5f): proposals (half with an offer),
/// challenges, cures, finalizes, guarantees, disputes and releases; then every pending part is finalized or guaranteed,
/// every lock disputed or released, every pair idle-settled. Every bond ends with a proposer or a challenger; every lock
/// ends in exactly one of release or payout plus burn, and its value is accounted for to the sompi.
fn walk_guarantee(seed: u64, st_total: &mut Stats) {
    let mut s = Sim::new(seed ^ 0x6a6a_0000);
    s.open(None);
    s.open(None);
    for _ in 0..18 {
        match s.rng.range(0, 11) {
            0 => s.transfer(true, true),
            1 => s.challenge(),
            2 => s.cure(),
            3 => s.finalize(),
            4 | 5 => s.guarantee(),
            6 | 7 => s.dispute_lock(),
            8 => s.release_lock(),
            _ => s.propose(),
        }
        if s.w.pairs.is_empty() {
            s.open(None);
        }
    }
    while !s.w.pendings.is_empty() {
        if s.rng.coin() {
            s.guarantee();
        } else {
            s.finalize();
        }
    }
    for _ in 0..4 {
        s.dispute_lock();
    }
    while !s.w.locks.is_empty() {
        s.release_lock();
    }
    while s.w.pairs.iter().any(|l| l.p.long_key == l.p.short_key) {
        s.coop_forfeited();
    }
    while !s.w.pairs.is_empty() {
        s.idle();
    }
    let t = &s.st;
    assert_eq!(t.bonds_posted, t.bonds_to_proposers + t.bonds_to_challengers, "every bond ends with a proposer or a challenger");
    assert_eq!(t.guaranteed_close + t.guaranteed_liq, t.disputed + t.released, "every lock ends in exactly one of release or dispute");
    assert_eq!(t.lock_value_in, t.lock_released + t.lock_to_victims + t.lock_burned + t.lock_rewards, "every sompi of every lock is accounted for");
    assert!(s.lock_src.is_empty());
    st_total.guaranteed_close += t.guaranteed_close;
    st_total.guaranteed_liq += t.guaranteed_liq;
    st_total.offers += t.offers;
    st_total.disputed += t.disputed;
    st_total.dispute_refused += t.dispute_refused;
    st_total.dispute_out_of_window += t.dispute_out_of_window;
    st_total.released += t.released;
    st_total.lock_value_in += t.lock_value_in;
    st_total.lock_released += t.lock_released;
    st_total.lock_to_victims += t.lock_to_victims;
    st_total.lock_burned += t.lock_burned;
    st_total.lock_rewards += t.lock_rewards;
    st_total.ev_close += t.ev_close;
    st_total.ev_liq += t.ev_liq;
    st_total.challenged += t.challenged;
    st_total.finalized_close += t.finalized_close;
    st_total.finalized_liq += t.finalized_liq;
    st_total.txs += t.txs;
}

#[test]
fn x6_guarantee_random_walk() {
    let mut st = Stats::default();
    for seed in 1..=16u64 {
        walk_guarantee(seed, &mut st);
    }
    println!("X6 guarantee: {st:?}");
    assert!(st.guaranteed_close > 0 && st.guaranteed_liq > 0 && st.offers > 0, "{st:?}");
    assert!(st.disputed > 0 && st.dispute_refused > 0 && st.dispute_out_of_window > 0 && st.released > 0, "{st:?}");
    assert!(st.lock_burned > 0 && st.lock_rewards > 0 && st.lock_to_victims > 0, "{st:?}");
    assert_eq!(st.lock_value_in, st.lock_released + st.lock_to_victims + st.lock_burned + st.lock_rewards);
}

fn walk(seed: u64, st_total: &mut Stats) {
    let mut s = Sim::new(seed);
    s.open(None);
    s.open(None);
    for _ in 0..10 {
        match s.rng.range(0, 5) {
            0 => s.transfer(true, true),
            1 => s.transfer(true, false),
            2 => {
                let partial = s.rng.coin();
                s.transfer(false, partial)
            }
            3 | 4 => s.close(),
            _ => s.mutual(),
        }
        if s.w.pairs.is_empty() {
            s.open(None);
        }
    }
    while !s.w.pairs.is_empty() {
        s.idle();
    }
    let t = &s.st;
    st_total.open += t.open;
    st_total.amend += t.amend;
    st_total.split += t.split;
    st_total.transfer_open += t.transfer_open;
    st_total.transfer_direct += t.transfer_direct;
    st_total.close_vs_close += t.close_vs_close;
    st_total.close_partial += t.close_partial;
    st_total.mutual += t.mutual;
    st_total.idle += t.idle;
    st_total.txs += t.txs;
    st_total.clock_kept += t.clock_kept;
    st_total.clock_restarted += t.clock_restarted;
    st_total.max_slack = st_total.max_slack.max(t.max_slack);
}
/// The X6 walk with the two-step evidence paths mixed in (DESIGN 4.5d): transfers (which bring entrants, tags and new
/// bounds) and close-versus-close between proposals, challenges, cures and finalizes; then every pending part is
/// finalized and every pair idle-settled. Every bond posted ends with a proposer or a challenger.
fn walk_ev(seed: u64, st_total: &mut Stats) {
    let mut s = Sim::new(seed ^ 0x5eed_0000);
    s.open(None);
    s.open(None);
    for _ in 0..16 {
        match s.rng.range(0, 10) {
            0 => s.transfer(true, true),
            1 => s.close(),
            2 | 3 => s.challenge(),
            4 => s.cure(),
            5 => s.finalize(),
            6 => s.coop_forfeited(),
            7 => s.propose_one_owner(),
            _ => s.propose(),
        }
        if s.w.pairs.is_empty() {
            s.open(None);
        }
    }
    while !s.w.pendings.is_empty() {
        s.finalize();
    }
    s.propose_one_owner();
    while s.w.pairs.iter().any(|l| l.p.long_key == l.p.short_key) {
        s.coop_forfeited();
    }
    while !s.w.pairs.is_empty() {
        s.idle();
    }
    let t = &s.st;
    assert_eq!(t.bonds_posted, t.bonds_to_proposers + t.bonds_to_challengers, "every bond ends with a proposer or a challenger");
    st_total.ev_close += t.ev_close;
    st_total.ev_liq += t.ev_liq;
    st_total.ev_refused += t.ev_refused;
    st_total.challenged += t.challenged;
    st_total.challenge_refused += t.challenge_refused;
    st_total.cured += t.cured;
    st_total.finalized_close += t.finalized_close;
    st_total.finalized_liq += t.finalized_liq;
    st_total.coop_forfeited += t.coop_forfeited;
    st_total.bonds_posted += t.bonds_posted;
    st_total.bonds_to_proposers += t.bonds_to_proposers;
    st_total.bonds_to_challengers += t.bonds_to_challengers;
    st_total.clock_kept += t.clock_kept;
    st_total.clock_restarted += t.clock_restarted;
    st_total.split += t.split;
    st_total.txs += t.txs;
    st_total.one_owner_refused += t.one_owner_refused;
    st_total.within_g_refused += t.within_g_refused;
    st_total.beyond_cure_accepted += t.beyond_cure_accepted;
    st_total.returned_unlisted += t.returned_unlisted;
}

#[test]
fn x6_evidence_random_walk() {
    let mut st = Stats::default();
    for seed in 1..=16u64 {
        walk_ev(seed, &mut st);
    }
    println!("X6 evidence: {st:?}");
    assert!(st.ev_close > 0 && st.ev_liq > 0 && st.split > 0, "{st:?}");
    assert!(st.challenged > 0 && st.challenge_refused > 0 && st.cured > 0 && st.finalized_close > 0 && st.finalized_liq > 0 && st.coop_forfeited > 0, "{st:?}");
    // Review 3: some evidence was refused by the clock rule (stale or dead before resting R on the pair's clock), and
    // clocks were both kept and restarted.
    assert!(st.ev_refused > 0 && st.clock_kept > 0 && st.clock_restarted > 0, "{st:?}");
    assert_eq!(st.bonds_posted, st.bonds_to_proposers + st.bonds_to_challengers);
    // Review 4 (DESIGN 4.5e): each new rule was exercised.
    assert!(st.one_owner_refused > 0 && st.within_g_refused > 0 && st.beyond_cure_accepted > 0 && st.returned_unlisted > 0, "{st:?}");
}

#[test]
fn x6_conservation_random_walk() {
    let mut st = Stats::default();
    for seed in 1..=16u64 {
        walk(seed, &mut st);
    }
    println!("X6: {st:?}");
    assert!(st.open > 0 && st.amend > 0 && st.split > 0 && st.transfer_open > 0 && st.transfer_direct > 0);
    assert!(st.close_vs_close > 0 && st.close_partial > 0 && st.mutual > 0 && st.idle > 0);
    assert!(st.max_slack <= 1);
    assert!(st.clock_kept > 0 && st.clock_restarted > 0, "{st:?}");
}
