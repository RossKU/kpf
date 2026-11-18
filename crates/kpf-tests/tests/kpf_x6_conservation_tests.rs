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
//!     part it sold (plus at most 1 sompi of rounding).

mod common;
mod kpf;

use std::collections::HashMap;

use common::*;
use kaspa_consensus_core::tx::{Transaction, TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
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
    ev_transfer: usize,
    txs: usize,
    max_slack: i128,
}

#[derive(Clone)]
struct World {
    pairs: Vec<Live>,
    cash: HashMap<[u8; 32], i128>,
}

struct Sim {
    w: World,
    keys: HashMap<[u8; 32], Keypair>,
    rng: Rng,
    ctr: u32,
    f: Keypair,
    st: Stats,
}

impl World {
    fn payoff(&self, key: &[u8; 32], p: i64) -> i128 {
        let mut v = *self.cash.get(key).unwrap_or(&0);
        for l in &self.pairs {
            if &l.p.long_key == key {
                v += l.p.long_at(p) as i128;
            }
            if &l.p.short_key == key {
                v += l.p.short_at(p) as i128;
            }
        }
        v
    }
    fn cash(&self, key: &[u8; 32]) -> i128 {
        *self.cash.get(key).unwrap_or(&0)
    }
}

impl Sim {
    fn new(seed: u64) -> Sim {
        Sim { w: World { pairs: vec![], cash: HashMap::new() }, keys: HashMap::new(), rng: Rng(seed), ctr: (seed as u32) << 12, f: keypair(14), st: Stats::default() }
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
    fn apply(&mut self, d: &Draft) -> Transaction {
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
        let spent: Vec<TransactionOutpoint> = d.ins.iter().map(|i| i.outpoint).collect();
        self.w.pairs.retain(|l| !spent.contains(&l.op));
        self.w.pairs.extend(live_pairs(&tx, d));
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
    fn open(&mut self, range: Option<(i64, i64)>) -> Live {
        let (x, rl, rs) = match range {
            Some((bl, bs)) => {
                let x = self.rng.range(bl + 1, bs - 1);
                (x, x - bl, bs - x)
            }
            None => {
                let x = self.rng.range(500_000_000, 2_000_000_000);
                (x, self.rng.range(50_000_000, 500_000_000), self.rng.range(50_000_000, 500_000_000))
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
        let tx = self.apply(&d);
        self.st.open += 1;
        live_pairs(&tx, &d).remove(0)
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
        let tx = self.apply(&d);
        let g = self.grid(&[&l.p]);
        self.non_consenting(&before, &other, 0, &g);
        self.non_consenting(&before, &key, 0, &g);
        self.st.amend += 1;
        live_pairs(&tx, &d).remove(0)
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

    fn pick(&mut self) -> Live {
        let i = self.rng.range(0, self.w.pairs.len() as i64 - 1) as usize;
        self.w.pairs[i].clone()
    }

    /// P3: list one side, then fill it by a KpfOpen or a direct taker, fully or in part.
    fn transfer(&mut self, by_open: bool, partial: bool) {
        let l = self.pick();
        let side = if self.rng.coin() { LONG } else { SHORT };
        let n = self.random_n(l.p.size, partial);
        let (bl, bs) = (l.p.bl, l.p.bs);
        let (ask, x) = if side == LONG {
            let a = self.rng.range((bl - 100_000_000).max(1), bs);
            (a, self.rng.range(a, bs))
        } else {
            let b = self.rng.range(bl.max(1), bs + 100_000_000);
            (b, self.rng.range(bl, b))
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
            if side == LONG {
                o.my_rate = self.rng.range((x - bl).max(1), x.max(1));
                o.limit = x + self.rng.range(0, 1_000_000);
                nb = x - o.my_rate;
            } else {
                o.my_rate = self.rng.range((bs - x).max(1), bs - x + 500_000_000);
                o.limit = x - self.rng.range(0, x.min(1_000_000));
                nb = x + o.my_rate;
            }
            let lo = LiveOpen { value: o.escrow() as u64, o, id, op: self.fresh_op() };
            (TransferSpec { side, n, x, nb, ek: xonly(&entrant), et: id, k: self.rng.range(0, 1) }, Entrant::Open(lo))
        } else {
            let nb = if side == LONG { bl - self.rng.range(0, bl.min(200_000_000)) } else { bs + self.rng.range(0, 200_000_000) };
            (TransferSpec { side, n, x, nb, ek: xonly(&entrant), et: ZERO, k: self.rng.range(0, 1) }, Entrant::Direct(entrant))
        };
        let d = transfer_draft(&l, &spec, &ent, &self.f.clone(), fnn);
        self.apply(&d);
        let mut ps: Vec<&Pair> = d.pairs.iter().map(|(_, p)| p).collect();
        ps.push(&p);
        let ps: Vec<Pair> = ps.into_iter().cloned().collect();
        let g = self.grid(&ps.iter().collect::<Vec<_>>());
        let split = (n < p.size) as i128;
        let (stayer, leaver) = if side == LONG { (p.short_key, p.long_key) } else { (p.long_key, p.short_key) };
        self.non_consenting(&before, &stayer, split, &g);
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
        let ok = |h: &Live, d: &Live| {
            h.id != d.id
                && d.p.bl <= h.p.bl
                && d.p.bs <= h.p.bs
                && ![h.p.tag_l, h.p.tag_s].contains(&d.id)
                && ![d.p.tag_l, d.p.tag_s].contains(&h.id)
        };
        let cands: Vec<Live> = self.w.pairs.iter().filter(|d| ok(&h, d)).cloned().collect();
        let d = if cands.is_empty() || self.rng.coin() {
            let bl = h.p.bl - self.rng.range(0, h.p.bl.min(200_000_000));
            let bs = h.p.bs - self.rng.range(0, (h.p.bs - bl - 2).min(200_000_000));
            self.open(Some((bl, bs)))
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
        let dr = close_draft(&h, &d, &CloseSpec { n, x }, &self.f.clone(), fnn);
        self.apply(&dr);
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
        self.apply(&d);
        let g = self.grid(&[&l.p]);
        let p = &l.p;
        self.leaver(&before, &p.long_key, p, n, owed(n, x - p.bl, p.long_tip, SCALE), true, &g);
        self.leaver(&before, &p.short_key, p, n, owed(n, p.bs - x, p.short_tip, SCALE), false, &g);
        self.st.mutual += 1;
    }

    // ------------------------------------------------------------ evidence paths (P5, P6, P7)

    /// One evidence action on a random pair: P5 by either side, P6 or P7 of a liquidatable side, at a random quote
    /// with a fill of exactly ceil(k n) (or more), whole or in part. After it:
    ///   - no sompi is created (`apply`);
    ///   - the party that did not act (P5: the other side; P6: the winner) has, at the evidence quote, at least
    ///     its payoff before at that quote; the loser of P6 / P7 at least that less the keeper fee; the stayer of
    ///     P7 at every grid price (no-downgrade); each up to 1 sompi per split, or DUST when a payout of it fell
    ///     below dust and stayed in the remainder.
    fn evidence(&mut self) {
        use kpf::evidence::*;
        let l = self.pick();
        let p = l.p.clone();
        let m = p.m.clone();
        let partial = self.rng.coin();
        let n = self.random_n(p.size, partial);
        let what = self.rng.range(0, 5);
        let a_max = (p.bl * 10_000 - 1) / (10_000 - m.mmr_bps);
        let b_min = p.bs * 10_000 / (10_000 + m.mmr_bps) + 1;
        let fill = n * m.k_bps / 10_000 + if self.rng.coin() { 0 } else { self.rng.range(0, n) };
        let (long, short) = (p.long_key, p.short_key);
        let (act, e, quote) = match what {
            0 => {
                let b = self.rng.range((p.bl - 100_000_000).max(1), p.bs + 100_000_000);
                (Act::Close { side: LONG, signer: self.keys[&long] }, Evidence::bid(b, fill), b)
            }
            1 => {
                let a = self.rng.range((p.bl - 100_000_000).max(1), p.bs + 100_000_000);
                (Act::Close { side: SHORT, signer: self.keys[&short] }, Evidence::ask(a, fill), a)
            }
            2 | 4 if a_max >= 1 => {
                let a = self.rng.range((a_max - 300_000_000).max(1), a_max);
                if what == 2 {
                    (Act::Liq { side: LONG }, Evidence::ask(a, fill), a)
                } else {
                    // The entrant accepts any counter rate >= 0, so x stays at most bs.
                    let x = self.rng.range(a, (a + 100_000_000).min(p.bs).max(a));
                    let who = self.fresh_key();
                    let mut o = long_order(&who);
                    (o.size_left, o.limit, o.min_counter_rate, o.min_fill, o.tip) = (n, x, 0, MIN, 0);
                    o.my_rate = self.rng.range((x - p.bl).max(1), x.max(1));
                    let open = LiveOpen { value: o.escrow() as u64, o, id: self.fresh_hash(0xb3), op: self.fresh_op() };
                    (Act::LiqTransfer { side: LONG, x, open, k: self.rng.range(0, 1) }, Evidence::ask(a, fill), a)
                }
            }
            3 | 5 if b_min <= PRICE_MAX => {
                let b = self.rng.range(b_min, (b_min + 300_000_000).min(PRICE_MAX));
                if what == 3 {
                    (Act::Liq { side: SHORT }, Evidence::bid(b, fill), b)
                } else {
                    let x = self.rng.range((b - 100_000_000).max(p.bl).min(b), b);
                    let who = self.fresh_key();
                    let mut o = short_order(&who);
                    (o.size_left, o.limit, o.min_counter_rate, o.min_fill, o.tip) = (n, x, 0, MIN, 0);
                    o.my_rate = self.rng.range((p.bs - x).max(1), (p.bs - x).max(1) + 300_000_000);
                    let open = LiveOpen { value: o.escrow() as u64, o, id: self.fresh_hash(0xb4), op: self.fresh_op() };
                    (Act::LiqTransfer { side: SHORT, x, open, k: self.rng.range(0, 1) }, Evidence::bid(b, fill), b)
                }
            }
            _ => return,
        };
        let before = self.w.clone();
        let fnn = self.fn_();
        let d = ev_draft(&[PairLeg { l: l.clone(), n, q: 0, act: act.clone() }], &e, &self.f.clone(), fnn);
        self.apply(&d);
        let split = (n < p.size) as i128;
        let c = quote.clamp(p.bl, p.bs);
        let fee = fee_of(&m, n, quote) as i128;
        let dust = |v: i64| if v > 0 && v < DUST { DUST as i128 } else { 0 };
        match act {
            Act::Close { side, .. } => {
                let (lp, sp, _) = close_at(&l, n, c, side, 0);
                let slack = split + dust(if side == LONG { q(n, p.bs - c, SCALE, 0) } else { q(n, c - p.bl, SCALE, 0) });
                let other = if side == LONG { short } else { long };
                self.at_quote(&before, &other, quote, slack);
                // The closer is paid exactly what it is owed (a payout of 0 only under dust).
                let got = self.w.cash(if side == LONG { &long } else { &short }) - before.cash(if side == LONG { &long } else { &short });
                assert_eq!(got, if side == LONG { lp } else { sp } as i128);
                self.st.ev_close += 1;
            }
            Act::Liq { side } => {
                let (winner, loser) = if side == LONG { (short, long) } else { (long, short) };
                let wo = if side == LONG { q(n, p.bs - c, SCALE, 0) } else { q(n, c - p.bl, SCALE, 0) };
                // The winner pays only what the loser's residual lacks of the fee.
                let lr = (if n == p.size { l.value as i64 } else { p.range_of(n) }) - wo;
                let wfee = (fee - lr as i128).max(0);
                self.at_quote(&before, &winner, quote, split + wfee + dust(wo));
                self.at_quote(&before, &loser, quote, split + fee + DUST as i128);
                self.st.ev_liq += 1;
            }
            Act::LiqTransfer { side, .. } => {
                let (stayer, loser) = if side == LONG { (short, long) } else { (long, short) };
                let g = self.grid(&[&p]);
                self.non_consenting(&before, &stayer, split, &g);
                self.at_quote(&before, &loser, quote, split + fee + DUST as i128);
                self.st.ev_transfer += 1;
            }
        }
        if split == 1 {
            self.st.split += 1;
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
        self.apply(&d);
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
    st_total.max_slack = st_total.max_slack.max(t.max_slack);
}

/// The X6 walk with the evidence paths mixed in: transfers (which bring entrants, tags and new bounds) and
/// close-versus-close between evidence actions, then idle settles.
fn walk_ev(seed: u64, st_total: &mut Stats) {
    let mut s = Sim::new(seed ^ 0x5eed_0000);
    s.open(None);
    s.open(None);
    for _ in 0..12 {
        match s.rng.range(0, 5) {
            0 => s.transfer(true, true),
            1 => s.close(),
            _ => s.evidence(),
        }
        if s.w.pairs.is_empty() {
            s.open(None);
        }
    }
    while !s.w.pairs.is_empty() {
        s.idle();
    }
    let t = &s.st;
    st_total.ev_close += t.ev_close;
    st_total.ev_liq += t.ev_liq;
    st_total.ev_transfer += t.ev_transfer;
    st_total.split += t.split;
    st_total.txs += t.txs;
}

#[test]
fn x6_evidence_random_walk() {
    let mut st = Stats::default();
    for seed in 1..=12u64 {
        walk_ev(seed, &mut st);
    }
    println!("X6 evidence: {st:?}");
    assert!(st.ev_close > 0 && st.ev_liq > 0 && st.ev_transfer > 0 && st.split > 0);
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
}
