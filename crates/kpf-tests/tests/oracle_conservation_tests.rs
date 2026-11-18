//! The seeded conservation run across the three families of objects of an oracle market (pairs and their pending
//! parts, the underwriter's slot, the judge's stakes and disputes), and the model-against-engine replay of the integrated
//! flow (docs/DESIGN-ORACLE-IMPL.md, third slice).
//!
//! Each seed plays a market for a few terms: pairs open on the slot while it has capacity, a true price walks, the
//! underwriter vouches true claims and, sometimes, false ones; watchers contest (in time, late, or not at all), the
//! underwriter defends true claims with aged stake, the judge is honest; parts are cured, finalized, decided, restored;
//! pairs renew in the last step before their expiry or lapse and settle; the slot advances. Every step is built from the
//! library model's values and run in the script engine, which must accept it; a hostile variant of the step (one sompi
//! less to a party, one DAA early, another key) must be refused. After every step:
//!   - no sompi created: every branch that adds no funding requires at most what its inputs hold;
//!   - the cover the slot counts never exceeds rho x its value, and the cover of every live pair bound to it (until its
//!     expiry plus the tail) is counted;
//!   - a party that did not act loses value only through an uncontested or judge-upheld claim (or the agreed premium at a
//!     renewal, or the lapse settlement at its entry price, both documented); a false claim contested in time never
//!     settles;
//!   - every claim's bond ends in exactly one documented place (its proposer, its defender, the disputer's share), to
//!     the sompi; every stake committed to a dispute ends as a winner's payout or a loser's forfeit, and a pool's books
//!     close (winners' gains + burn + rounding = losers' stake).

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use std::collections::HashMap;

use common::*;
use kaspa_consensus_core::tx::TransactionOutpoint;
use kaspa_consensus_core::Hash;
use kpf_protocol::judge::{UNIT, YES};
use kpf_protocol::oracle::*;
use oraclekit::*;

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
}

#[derive(Clone)]
struct Claim {
    w: LPend,
    truth: i64,
    false_claim: bool,
}

#[derive(Clone)]
struct Contest {
    c: Claim,
    part: LPend,
    dispute: LStake,
    receipts: Vec<LStake>,
    in_time: bool,
}

#[derive(Default, Debug)]
struct Books {
    steps: HashMap<&'static str, usize>,
    hostile_refused: usize,
    bonds_posted: i64,
    bonds_to_proposer: i64,
    bonds_to_defender: i64,
    bonds_to_disputer: i64,
    stakes_committed: i64,
    stakes_paid: i64,
    burned: i64,
    false_settled_uncontested: usize,
    false_contested_restored: usize,
    premiums: i64,
}

struct World {
    rng: Rng,
    now: u64,
    slot: LSlot,
    pairs: Vec<(LPair, i64)>,
    claims: Vec<Claim>,
    contests: Vec<Contest>,
    decided: Vec<(Claim, LPend, bool)>,
    pools: Vec<(LStake, Vec<LStake>)>,
    next_id: u32,
    books: Books,
    /// Live cover per pair id (size x coverRate) with the end of its liability (X + tail).
    cover: HashMap<[u8; 32], (i64, u64)>,
}

impl World {
    fn id(&mut self) -> (TransactionOutpoint, Hash) {
        self.next_id += 1;
        let mut b = [0x33u8; 32];
        b[..4].copy_from_slice(&self.next_id.to_le_bytes());
        (TransactionOutpoint { transaction_id: kaspa_consensus_core::tx::TransactionId::from_bytes(b), index: self.next_id }, Hash::from_bytes(b))
    }
    fn tick(&mut self, n: u64) {
        self.now += n;
    }
    fn count(&mut self, what: &'static str) {
        *self.books.steps.entry(what).or_default() += 1;
    }
    fn hostile(&mut self, what: &str, t: &T) {
        assert!(!accepted(t), "hostile variant accepted: {what}");
        self.books.hostile_refused += 1;
    }

    /// The cover invariants: the slot within rho x its value; every pair's live cover counted at its expiry.
    fn check_cover(&self) {
        let s = &self.slot.s;
        let sum = kpf_protocol::bond::ring_sum(&s.ring).unwrap();
        assert!(sum <= capacity(self.slot.value as i64, s.m.p.rho_bps), "the slot counts more than rho x K");
        let mut need: HashMap<i64, i64> = HashMap::new();
        for (_, (c, end)) in self.cover.iter() {
            if *end > self.now {
                *need.entry(*end as i64 - s.m.p.tail).or_default() += c;
            }
        }
        for (x, c) in need {
            assert!(s.counted_at(x) >= c, "cover of expiry {x} not counted: {} < {c}", s.counted_at(x));
        }
    }

    fn open(&mut self) {
        let m = omarket();
        let xe = (((self.now + 800 + 99) / 100) * 100) as i64;
        let j0 = self.slot.s.j0;
        let adv = (xe / 100 - j0 - 12).max(0);
        let mut sp = open_spec();
        sp.lock = self.now;
        sp.xe = xe;
        sp.advance = adv;
        let (op1, id1) = self.id();
        let (op2, id2) = self.id();
        let mk = |op, id, side, kp: &secp256k1::Keypair, daa| Live { op, value: 2_000_000_000_000u64, daa, id: Some(id), s: sample_order(&m, side, x(kp)) };
        let (lo, so) = (mk(op1, id1, L, &long_kp(), self.now), mk(op2, id2, S, &short_kp(), self.now));
        let (t, p, b2) = open_tx(&lo, &so, &self.slot, &sp, &|_, _| {});
        let cover = p.cover_of(p.size);
        let room = capacity(self.slot.value as i64, m.bond.rho_bps) - kpf_protocol::bond::ring_sum(&self.slot.s.advanced(adv, self.now as i64).unwrap()).unwrap();
        let model_ok = cover <= room;
        // The underwriter keeps room for one renewal per pair (a renewal counts the new term before the old one ends),
        // but now and then binds anyway, so that the slot's own refusal is exercised.
        if room < 2 * cover && model_ok && !self.rng.chance(15) {
            return;
        }
        assert_eq!(accepted(&t), model_ok, "open: the engine and the model disagree on capacity (room {room}, cover {cover})");
        if !model_ok {
            self.count("open refused for capacity");
            return;
        }
        // Hostile: the pair's value one sompi below its floor.
        let (mut h, _, _) = open_tx(&lo, &so, &self.slot, &sp, &|o, _| o[2].value -= 1);
        h.0.finalize();
        self.hostile("open with the pair one sompi short", &h);
        self.count("open");
        let lp = live(&t, 2, self.now, &p);
        self.cover.insert(lp.idb(), (cover, (xe + m.bond.tail) as u64));
        self.slot = live(&t, 3, self.now, &b2);
        self.books.premiums += t.0.outputs[4].value as i64;
        self.pairs.push((lp, 1_000_000_000));
    }

    fn walk(&mut self) {
        for i in 0..self.pairs.len() {
            let d = self.rng.below(5) as i64 - 2;
            let p = &mut self.pairs[i].1;
            *p = (*p + d * 10_000_000).clamp(800_000_000, 1_300_000_000);
        }
    }

    fn propose(&mut self) {
        if self.pairs.is_empty() {
            return;
        }
        let i = self.rng.below(self.pairs.len() as u64) as usize;
        let (lp, truth) = self.pairs[i].clone();
        if self.now < lp.daa + 1 {
            return;
        }
        let k = kit();
        let lie = self.rng.chance(30);
        let n = lp.s.size;
        let t0 = self.now as i64 - self.rng.below(60) as i64;
        let (kind, side, p) = if lie {
            // A false liquidation of the long: a price that triggers while the truth does not.
            if lp.s.long_liquidatable(truth) {
                return;
            }
            (LIQ, L, lp.s.bl * 95 / 100)
        } else if lp.s.long_liquidatable(truth) {
            (LIQ, L, truth)
        } else if lp.s.short_liquidatable(truth) {
            (LIQ, S, truth)
        } else if self.rng.chance(40) {
            (CLOSE, if self.rng.chance(50) { L } else { S }, truth)
        } else {
            return;
        };
        if lp.s.pending_of(k, n, kind, side, p, t0).is_none() {
            return;
        }
        let c = ClaimSpec { kind, side, n, p, t0, lock: self.now };
        let closer = if kind == CLOSE { Some(if side == L { long_kp() } else { short_kp() }) } else { None };
        let (t, w) = propose_tx(&lp, &c, &uw_kp(), closer.as_ref());
        must("propose", &t);
        // Hostile: vouched by another key.
        let (h, _) = propose_tx(&lp, &c, &keypair(77), closer.as_ref());
        self.hostile("propose vouched by a stranger", &h);
        self.count(if lie { "propose false" } else { "propose true" });
        self.books.bonds_posted += w.bond;
        let lw = live(&t, 0, self.now, &w);
        self.pairs.remove(i);
        self.claims.push(Claim { w: lw, truth, false_claim: lie });
    }

    fn contest(&mut self) {
        if self.claims.is_empty() {
            return;
        }
        let i = self.rng.below(self.claims.len() as u64) as usize;
        let c = self.claims[i].clone();
        let p = c.w.daa;
        let w_end = p + c.w.s.m.p.challenge_daa as u64;
        // Watchers contest false claims in time most of the time, true claims sometimes; some contests come late.
        let want = if c.false_claim { self.rng.chance(80) } else { self.rng.chance(20) };
        if !want {
            return;
        }
        let late = self.now >= w_end;
        if late && !self.rng.chance(30) {
            return;
        }
        let (op, _) = self.id();
        let st = Live { op, value: 10 * UNIT as u64, daa: 0, id: None, s: kpf_protocol::judge::Js::free(x(&keypair(70)), omarket().judge_market()) };
        let (t, w2, r, d) = contest_tx(&c.w, &st, 70, self.now);
        must("contest", &t);
        self.count(if late { "contest late" } else { "contest in time" });
        self.books.stakes_committed += st.value as i64;
        let part = live(&t, 0, self.now, &w2);
        let mut con = Contest { c: c.clone(), part, dispute: live(&t, 2, self.now, &d), receipts: vec![live(&t, 1, self.now, &r)], in_time: !late };
        // The underwriter defends a true claim with aged stake (1.25 x the opener's takes the lead).
        if !c.false_claim {
            self.tick(1);
            let (op, _) = self.id();
            let ys = Live { op, value: 20 * UNIT as u64, daa: 0, id: None, s: kpf_protocol::judge::Js::free(x(&uw_kp()), omarket().judge_market()) };
            let (t, d2, rs) = deposit_tx(&con.dispute, YES, &[(ys.clone(), 31)], self.now);
            must("defence", &t);
            self.books.stakes_committed += ys.value as i64;
            con.dispute = live(&t, 0, self.now, &d2);
            con.receipts.push(live(&t, 1, self.now, &rs[0]));
        }
        self.claims.remove(i);
        self.contests.push(con);
    }

    fn decide(&mut self) {
        let ready: Vec<usize> = (0..self.contests.len())
            .filter(|&i| {
                let d = &self.contests[i].dispute;
                let st = if d.s.start == -1 { d.daa as i64 } else { d.s.start };
                self.now as i64 >= st + d.s.m.p.round
            })
            .collect();
        let Some(&i) = ready.first() else { return };
        let con = self.contests.remove(i);
        let d = &con.dispute;
        let st = if d.s.start == -1 { d.daa as i64 } else { d.s.start };
        // Hostile: one DAA before the round's window.
        let (h, _, _) = decide_tx(d, &con.part, (st + d.s.m.p.round - 1) as u64);
        self.hostile("resolve before the window", &h);
        let (t, pool, w2) = decide_tx(d, &con.part, self.now);
        must("decide", &t);
        self.count("decide");
        // The honest judge: NO on a false claim, YES on a true one; a late contest is ignored.
        let expect = if !con.in_time { UPHELD } else if con.c.false_claim { REFUTED } else { UPHELD };
        assert_eq!(w2.status, expect, "the verdict reaches the part");
        self.pools.push((live(&t, 0, self.now, &pool), con.receipts.clone()));
        self.decided.push((con.c.clone(), live(&t, 1, self.now, &w2), con.in_time));
    }

    fn finalize_claims(&mut self) {
        // Pending parts past W.
        let mut i = 0;
        while i < self.claims.len() {
            let c = self.claims[i].clone();
            if self.now >= c.w.daa + c.w.s.m.p.challenge_daa as u64 && self.rng.chance(70) {
                let (h, _) = finalize_tx(&c.w, c.w.daa as i64, c.w.s.m.p.challenge_daa as u64 - 1);
                self.hostile("finalize one DAA early", &h);
                let (t, s) = finalize_tx(&c.w, c.w.daa as i64, c.w.s.m.p.challenge_daa as u64);
                must("finalize", &t);
                self.count("finalize pending");
                if c.false_claim && !matches!(s, Settlement::Restore { .. }) {
                    self.books.false_settled_uncontested += 1;
                }
                self.settled(&c, &t, &s);
                self.claims.remove(i);
            } else {
                i += 1;
            }
        }
        // Decided parts at once.
        while let Some((c, w, in_time)) = self.decided.pop() {
            let (t, s) = finalize_tx(&w, w.s.clock, 0);
            must("finalize decided", &t);
            self.count("finalize decided");
            if c.false_claim && in_time {
                assert!(matches!(s, Settlement::Restore { .. }), "a false claim contested in time never settles");
                self.books.false_contested_restored += 1;
            }
            if c.false_claim && !in_time && !matches!(s, Settlement::Restore { .. }) {
                self.books.false_settled_uncontested += 1;
            }
            self.settled(&c, &t, &s);
        }
    }

    /// Books one settlement: where the bond went, the part that comes back, no value created.
    fn settled(&mut self, c: &Claim, t: &T, s: &Settlement) {
        let w = &c.w.s;
        let v = c.w.value as i64;
        match s {
            Settlement::Close { long, short } => {
                assert!(long + short <= v, "a close pays at most the part");
                self.books.bonds_to_proposer += w.bond;
                self.cover.remove(&c.w.idb());
            }
            Settlement::Forfeit { part, value, uw } => {
                assert!(value + uw <= v);
                assert_eq!(*uw, w.bond + w.fee);
                self.books.bonds_to_proposer += w.bond;
                // The forfeited part is the winner's alone; it is not liquidatable again (one owner).
                assert_eq!(part.long_key, part.short_key);
                self.cover.remove(&c.w.idb());
            }
            Settlement::Restore { part, value, defender, disputer } => {
                assert!(value + defender + disputer <= v);
                assert_eq!(defender + disputer, w.bond, "the bond to the defender and the disputer, to the sompi");
                self.books.bonds_to_defender += defender;
                self.books.bonds_to_disputer += disputer;
                // The parties' positions come back unchanged (keys, bounds, entry, binding).
                assert_eq!((part.long_key, part.short_key, part.bl, part.bs, part.entry, part.b), (w.long_key, w.short_key, w.bl, w.bs, w.entry, w.b));
                let back = live(t, 1, self.now, part);
                self.pairs.push((back, c.truth));
            }
        }
    }

    fn settle_pools(&mut self) {
        while let Some((pool, receipts)) = self.pools.pop() {
            let (t, p2, pays) = settle_tx(&pool, &receipts, self.now);
            must("settle", &t);
            let losers: i64 = receipts.iter().filter(|r| r.s.side != pool.s.side).map(|r| r.value as i64).sum();
            let winners: i64 = receipts.iter().filter(|r| r.s.side == pool.s.side).map(|r| r.value as i64).sum();
            let paid: i64 = pays.iter().map(|x| *x as i64).sum();
            let gain = paid - winners;
            let (ct, burn) = pool_close_tx(&live(&t, 0, self.now, &p2));
            must("close", &ct);
            assert!(gain + burn as i64 <= losers && losers - gain - burn as i64 <= receipts.len() as i64, "a pool's books close to the sompi");
            self.books.stakes_paid += paid;
            self.books.burned += burn as i64;
            self.count("pool settled and closed");
        }
    }

    fn renew_or_lapse(&mut self) {
        let mut i = 0;
        while i < self.pairs.len() {
            let (lp, truth) = self.pairs[i].clone();
            let x = lp.s.b.expiry as u64;
            if self.now + 100 >= x && self.now < x + 100 && lp.s.renewed().is_some() && self.rng.chance(85) {
                let adv = ((x as i64 + 800) / 100 - self.slot.s.j0 - 12).max(0);
                let (t, r, b2) = renew_tx(&lp, &self.slot, self.now, adv);
                if accepted(&t) {
                    let (h, _, _) = renew_tx(&lp, &self.slot, x - 101, adv);
                    self.hostile("renewal before the last step", &h);
                    self.count("renew");
                    self.books.premiums += lp.value as i64 - t.0.outputs[0].value as i64;
                    self.slot = live(&t, 1, self.now, &b2);
                    let np = live(&t, 0, self.now, &r);
                    self.cover.insert(np.idb(), (r.cover_of(r.size), (r.b.expiry + r.m.bond.tail) as u64));
                    self.pairs[i] = (np, truth);
                } else {
                    self.count("renewal refused for capacity");
                }
            } else if self.now >= x + 200 && self.now >= lp.daa + 1_000 {
                let t = lapse_tx(&lp, self.now, 1_000);
                must("lapse settle", &t);
                self.hostile("lapse settle one DAA early", &lapse_tx(&lp, x + 199, 1_000));
                self.count("lapse settle");
                self.cover.remove(&lp.idb());
                self.pairs.remove(i);
                continue;
            }
            i += 1;
        }
    }

    fn cure_some(&mut self) {
        if let Some(i) = (0..self.claims.len()).find(|&i| self.claims[i].w.s.cure(self.claims[i].w.value).is_some() && !self.claims[i].false_claim) {
            if !self.rng.chance(30) {
                return;
            }
            let c = self.claims.remove(i);
            let loser = if c.w.s.side == L { long_kp() } else { short_kp() };
            let (t, cured) = cure_tx(&c.w, &loser);
            must("cure", &t);
            self.count("cure");
            self.books.bonds_to_proposer += c.w.s.bond;
            self.pairs.push((live(&t, 1, self.now, &cured), c.truth));
        }
    }
}

fn play(seed: u64, steps: usize) -> Books {
    let mut w = World {
        rng: Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1),
        now: 1_000,
        slot: slot(K),
        pairs: vec![],
        claims: vec![],
        contests: vec![],
        decided: vec![],
        pools: vec![],
        next_id: seed as u32 * 10_000,
        books: Books::default(),
        cover: HashMap::new(),
    };
    for _ in 0..steps {
        let dt = 1 + w.rng.below(40);
        w.tick(dt);
        match w.rng.below(10) {
            0 | 1 => w.open(),
            2 | 3 => w.propose(),
            4 => w.contest(),
            5 => w.cure_some(),
            _ => {}
        }
        w.walk();
        w.decide();
        w.finalize_claims();
        w.settle_pools();
        w.renew_or_lapse();
        w.check_cover();
    }
    // Drain: every claim, contest and decided part ends.
    for _ in 0..40 {
        w.tick(100);
        w.decide();
        w.finalize_claims();
        w.settle_pools();
        if w.claims.is_empty() && w.contests.is_empty() && w.decided.is_empty() {
            break;
        }
    }
    assert!(w.claims.is_empty() && w.contests.is_empty() && w.decided.is_empty(), "every claim and dispute ends");
    let b = &w.books;
    assert_eq!(b.bonds_posted, b.bonds_to_proposer + b.bonds_to_defender + b.bonds_to_disputer, "every bond in exactly one place");
    w.books
}

#[test]
fn seeded_conservation_and_replay_across_pairs_bonds_and_stakes() {
    let mut total = Books::default();
    for seed in 1..=10 {
        let b = play(seed, 150);
        for (k, v) in b.steps {
            *total.steps.entry(k).or_default() += v;
        }
        total.hostile_refused += b.hostile_refused;
        total.bonds_posted += b.bonds_posted;
        total.bonds_to_proposer += b.bonds_to_proposer;
        total.bonds_to_defender += b.bonds_to_defender;
        total.bonds_to_disputer += b.bonds_to_disputer;
        total.stakes_committed += b.stakes_committed;
        total.stakes_paid += b.stakes_paid;
        total.burned += b.burned;
        total.false_settled_uncontested += b.false_settled_uncontested;
        total.false_contested_restored += b.false_contested_restored;
        total.premiums += b.premiums;
    }
    let mut steps: Vec<_> = total.steps.iter().collect();
    steps.sort();
    println!("10 seeds x 150 steps: {steps:?}");
    println!(
        "hostile variants refused {}; bonds posted {} KAS: to proposers {}, defenders {}, disputers {}; stakes committed {} KAS, paid back {} KAS, burned {} KAS; false claims restored after a timely contest {}, settled for want of one {}; premiums {} KAS",
        total.hostile_refused,
        total.bonds_posted / 100_000_000,
        total.bonds_to_proposer / 100_000_000,
        total.bonds_to_defender / 100_000_000,
        total.bonds_to_disputer / 100_000_000,
        total.stakes_committed / 100_000_000,
        total.stakes_paid / 100_000_000,
        total.burned / 100_000_000,
        total.false_contested_restored,
        total.false_settled_uncontested,
        total.premiums / 100_000_000
    );
    for k in ["open", "propose true", "propose false", "contest in time", "decide", "finalize pending", "finalize decided", "renew", "pool settled and closed"] {
        assert!(total.steps.get(k).copied().unwrap_or(0) > 0, "the run exercised `{k}`");
    }
    assert!(total.false_contested_restored > 0);
    // Committed stake = winners' payouts + burn + the rounding of the pro-rata shares (below one sompi a receipt).
    let rounding = total.stakes_committed - total.stakes_paid - total.burned;
    assert!((0..=1_000).contains(&rounding), "stake rounding {rounding} sompi");
}
