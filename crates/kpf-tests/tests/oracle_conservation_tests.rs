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
//!     close (winners' gains + burn + rounding = losers' stake);
//!   - review 6: no object of the family ever shares a covenant id with a UTXO its parties did not verify (every output
//!     with an id continues a known id from an input of the family, or starts a fresh one carried by no input and no
//!     other output; a hand-over keeping the old id is refused); a receipt settles only in its own claim's pool (each
//!     receipt is checked against the claim it was committed under, and a receipt of another claim is refused);
//!   - review 7: the conservation statement of DESIGN-ORACLE-IMPL 25.3 (`World::accounts`): every fresh output names its
//!     makers, each an input of the step in one role, and every account of family inputs is covered by the outputs that
//!     name it plus its positional payouts plus what the step declares; and at every step a hostile builder merges two
//!     family inputs into one output (two proposals into one pending part, two hand-overs into one part, two long orders
//!     into one pair, a claim and a look-alike into one settled part), refused every time.

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use std::collections::{HashMap, HashSet};

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
    /// Review 7: hostile merges of two family inputs into one output, refused.
    merges_refused: usize,
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
    /// Review 6: every covenant id of the run's objects (the slot's, the orders', and every id born fresh in a step).
    family: HashSet<Hash>,
    /// Review 6: the claim (pending part) id each receipt was committed under.
    receipt_claim: HashMap<TransactionOutpoint, Hash>,
    /// Review 7: the nonzero makers of every fresh family output the run builds, by script.
    makers: HashMap<Vec<u8>, Vec<(&'static str, [u8; 32])>>,
}

impl World {
    fn id(&mut self) -> (TransactionOutpoint, Hash) {
        self.next_id += 1;
        let mut b = [0x33u8; 32];
        b[..4].copy_from_slice(&self.next_id.to_le_bytes());
        self.family.insert(Hash::from_bytes(b));
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

    /// An accepted step: the engine accepts it and it keeps the lineage invariant.
    fn ok(&mut self, what: &str, t: &T) {
        must(what, t);
        self.lineage(what, t);
        self.accounts(what, t, &[]);
    }

    /// Review 6, the lineage invariant of every accepted step (the induction of DESIGN-ORACLE-IMPL 24): an output that
    /// carries a covenant id either continues an id the run already knows (the slot's, an order's, or one born fresh
    /// in an earlier step), authorized by an input of the family (a P2SH) carrying it, or starts a fresh id: no input
    /// carries it, no other output carries it, the run never saw it. No plain-key output ever carries an id. So no
    /// object of the family ever shares its covenant id with a UTXO its parties did not verify.
    fn lineage(&mut self, what: &str, t: &T) {
        let ins: HashSet<Hash> = t.1.iter().filter_map(|e| e.covenant_id).collect();
        for o in t.0.outputs.iter() {
            let Some(c) = o.covenant else { continue };
            let id = c.covenant_id;
            assert_eq!(o.script_public_key.script().first(), Some(&0xaa), "{what}: a plain output carries a covenant id");
            if ins.contains(&id) {
                assert!(self.family.contains(&id), "{what}: an output continues an id the run never saw born");
                let a = &t.1[c.authorizing_input as usize];
                assert_eq!(a.covenant_id, Some(id), "{what}: continuation authorized by an input of another id");
                assert_eq!(a.script_public_key.script().first(), Some(&0xaa), "{what}: authorized by a plain input");
            } else {
                assert!(!self.family.contains(&id), "{what}: a genesis reuses a known id");
                let n = t.0.outputs.iter().filter(|x| x.covenant.map(|c| c.covenant_id) == Some(id)).count();
                assert_eq!(n, 1, "{what}: a fresh id carried by more than one output");
                self.family.insert(id);
                self.count("fresh lineage");
            }
        }
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
        self.reg_pair(&p);
        self.lineage("open", &t);
        self.accounts("open", &t, &[]);
        self.merge_fill(&lo, &so, &sp, &t, &p);
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

    /// Review 6: a hand-over. A pair's long lists at the entry price and a signing taker takes it whole (the same key,
    /// so that the run's later closes still sign): the part starts a fresh lineage and its cover moves to the new id;
    /// the same transfer keeping the old id is refused.
    fn hand_over(&mut self) {
        if self.pairs.is_empty() {
            return;
        }
        let i = self.rng.below(self.pairs.len() as u64) as usize;
        let (lp, truth) = self.pairs[i].clone();
        let q = lp.s.m.p.quiet_daa as u64;
        if self.now < lp.daa + q || self.now + 100 >= lp.s.b.expiry as u64 {
            return;
        }
        let ask = lp.s.entry;
        let (at, listed) = amend_tx(&lp, L, &long_kp(), ask, q);
        self.ok("list for a hand-over", &at);
        let ll = live(&at, 0, self.now, &listed);
        self.pairs[i] = (ll.clone(), truth);
        self.tick(q);
        let (tt, part) = transfer_tx(&ll, L, ask, ll.s.bl, &long_kp(), q);
        if !accepted(&tt) {
            return;
        }
        let twin = self.twin_of(&ll);
        self.merge_transfer(&ll, &twin);
        self.reg_pair(&part);
        self.ok("hand-over", &tt);
        self.hostile("a hand-over keeping the pair's id", &transfer_tx_with(&ll, L, ask, ll.s.bl, &long_kp(), q, Lineage::Keep).0);
        let np = live(&tt, 1, self.now, &part);
        assert_ne!(np.id, ll.id, "the entrant's part is under a fresh id");
        if let Some(c) = self.cover.remove(&ll.idb()) {
            self.cover.insert(np.idb(), c);
        }
        self.pairs[i] = (np, truth);
        self.count("hand-over");
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
        // Review 11: a liquidation needs a pair UTXO that can carry a valid claim (KpfPairO.propose); a close is a party
        // entry (quietDaa).
        if (kind == LIQ && !lp.s.liquidatable_from(lp.daa as i64)) || (kind == CLOSE && self.now < lp.daa + lp.s.m.p.quiet_daa as u64) {
            return;
        }
        let c = ClaimSpec { kind, side, n, p, t0, lock: self.now };
        let closer = if kind == CLOSE { Some(if side == L { long_kp() } else { short_kp() }) } else { None };
        let (t, w) = propose_tx(&lp, &c, &uw_kp(), closer.as_ref());
        self.reg_pend(&w);
        self.ok("propose", &t);
        // Hostile: a liquidation vouched by another key (a close carries no vouch since review 11: signed by the closer).
        let (h, _) = if kind == CLOSE { propose_tx(&lp, &c, &uw_kp(), Some(&keypair(77))) } else { propose_tx(&lp, &c, &keypair(77), None) };
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
        self.ok("contest", &t);
        self.count(if late { "contest late" } else { "contest in time" });
        self.books.stakes_committed += st.value as i64;
        let part = live(&t, 0, self.now, &w2);
        let mut con = Contest { c: c.clone(), part, dispute: live(&t, 2, self.now, &d), receipts: vec![live(&t, 1, self.now, &r)], in_time: !late };
        // Review 6: the dispute and its receipts take the claim's own id, which no other claim has.
        assert_eq!(con.dispute.id, c.w.id);
        self.receipt_claim.insert(con.receipts[0].op, c.w.id.unwrap());
        // The underwriter defends a true claim with aged stake (1.25 x the opener's takes the lead).
        if !c.false_claim {
            self.tick(1);
            let (op, _) = self.id();
            let ys = Live { op, value: 20 * UNIT as u64, daa: 0, id: None, s: kpf_protocol::judge::Js::free(x(&uw_kp()), omarket().judge_market()) };
            let (t, d2, rs) = deposit_tx(&con.dispute, YES, &[(ys.clone(), 31)], self.now);
            self.ok("defence", &t);
            self.books.stakes_committed += ys.value as i64;
            con.dispute = live(&t, 0, self.now, &d2);
            con.receipts.push(live(&t, 1, self.now, &rs[0]));
            self.receipt_claim.insert(con.receipts[1].op, c.w.id.unwrap());
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
        self.ok("decide", &t);
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
                if !matches!(s, Settlement::Close { .. }) {
                    self.merge_finalize(&c.w, &t);
                }
                self.ok("finalize", &t);
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
            if !matches!(s, Settlement::Close { .. }) {
                self.merge_finalize(&w, &t);
            }
            self.ok("finalize decided", &t);
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
            Settlement::Restore { part, value, defender, disputer, proposer } => {
                assert!(value + defender + disputer + proposer <= v);
                // Refuted: the bond to the defender and the disputer; void (review 11): back to the proposer.
                assert_eq!(defender + disputer + proposer, w.bond, "the bond, to the sompi");
                self.books.bonds_to_defender += defender;
                self.books.bonds_to_disputer += disputer;
                self.books.bonds_to_proposer += proposer;
                // The parties' positions come back unchanged (keys, bounds, entry, binding).
                assert_eq!((part.long_key, part.short_key, part.bl, part.bs, part.entry, part.b), (w.long_key, w.short_key, w.bl, w.bs, w.entry, w.b));
                let back = live(t, 1, self.now, part);
                self.pairs.push((back, c.truth));
            }
        }
    }

    fn settle_pools(&mut self) {
        while let Some((pool, receipts)) = self.pools.pop() {
            // Review 6: a receipt settles only in its own claim's pool. Every receipt here was committed under this
            // pool's claim; a receipt of another claim (the next pool's) is refused here.
            for r in &receipts {
                assert_eq!(self.receipt_claim.get(&r.op).copied(), pool.id, "a receipt of another claim in this pool");
            }
            if let Some(other) = self.pools.last().and_then(|(_, rs)| rs.first().cloned()) {
                assert_ne!(other.id, pool.id, "two claims share a pool's id");
                self.hostile("a receipt settled in another claim's pool", &settle_tx(&pool, &[other], self.now).0);
            }
            let (t, p2, pays) = settle_tx(&pool, &receipts, self.now);
            self.ok("settle", &t);
            let losers: i64 = receipts.iter().filter(|r| r.s.side != pool.s.side).map(|r| r.value as i64).sum();
            let winners: i64 = receipts.iter().filter(|r| r.s.side == pool.s.side).map(|r| r.value as i64).sum();
            let paid: i64 = pays.iter().map(|x| *x as i64).sum();
            let gain = paid - winners;
            let (ct, burn) = pool_close_tx(&live(&t, 0, self.now, &p2));
            // KpfStake.close: the base (the carrier funded at the opening) is the executor's and the rest is burned at the
            // output the entry names: both declared.
            must("close", &ct);
            self.lineage("close", &ct);
            self.accounts("close", &ct, &[(0, p2.base + burn as i64)]);
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
                    self.lineage("renew", &t);
                    self.accounts("renew", &t, &[(0, lp.s.renewed().map(|r| r.1).unwrap_or(0))]);
                    self.books.premiums += lp.value as i64 - t.0.outputs[0].value as i64;
                    self.slot = live(&t, 1, self.now, &b2);
                    let np = live(&t, 0, self.now, &r);
                    self.cover.insert(np.idb(), (r.cover_of(r.size), (r.b.expiry + r.m.bond.tail) as u64));
                    self.pairs[i] = (np, truth);
                } else {
                    self.count("renewal refused for capacity");
                }
            } else if self.now >= lp.s.lapse_from() {
                // Review 6 (R6-F3): idleDaa after the cover's end by the calendar, whatever the UTXO's age.
                let t = lapse_tx(&lp, self.now, 0);
                self.ok("lapse settle", &t);
                self.hostile("lapse settle one DAA early", &lapse_tx(&lp, lp.s.lapse_from() - 1, 1_000_000));
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
            self.ok("cure", &t);
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
        family: [SLOT_ID].into_iter().collect(),
        receipt_claim: HashMap::new(),
        makers: HashMap::new(),
    };
    for _ in 0..steps {
        let dt = 1 + w.rng.below(40);
        w.tick(dt);
        match w.rng.below(10) {
            0 | 1 => w.open(),
            2 | 3 => w.propose(),
            4 => w.contest(),
            5 => w.cure_some(),
            6 => w.hand_over(),
            _ => {}
        }
        w.walk();
        w.decide();
        w.finalize_claims();
        w.settle_pools();
        w.renew_or_lapse();
        w.hostile_merges();
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
        total.merges_refused += b.merges_refused;
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
    println!("review 7: {} hostile merges of two family inputs into one output refused", total.merges_refused);
    assert!(total.merges_refused > 0);
    for k in ["open", "propose true", "propose false", "contest in time", "decide", "finalize pending", "finalize decided", "renew", "pool settled and closed", "hand-over", "fresh lineage", "step accounted", "fresh output named by its makers"] {
        assert!(total.steps.get(k).copied().unwrap_or(0) > 0, "the run exercised `{k}`");
    }
    assert!(total.false_contested_restored > 0);
    // Committed stake = winners' payouts + burn + the rounding of the pro-rata shares (below one sompi a receipt).
    let rounding = total.stakes_committed - total.stakes_paid - total.burned;
    assert!((0..=1_000).contains(&rounding), "stake rounding {rounding} sompi");
}

// ---------------------------------------------------------------- review 7: the accounting statement and the merges

/// An account of a transaction: the family inputs of one covenant id (they move together: a dispute and its claim at
/// a resolution, a pool and its receipts at a settlement), or one id-less family input (a free stake).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Acc {
    Id(Hash),
    Lone(usize),
}

impl World {
    /// Review 7: the makers of every fresh family output the run builds, by script.
    fn reg_pair(&mut self, p: &OPair) {
        let mk: Vec<(&'static str, [u8; 32])> = p.mk.roles().into_iter().filter(|(_, h)| *h != [0; 32]).collect();
        self.makers.insert(pair_spk(p).script().to_vec(), mk);
    }
    fn reg_pend(&mut self, w: &OPending) {
        let mk = if w.mk_parent == [0; 32] { vec![] } else { vec![("parent", w.mk_parent)] };
        self.makers.insert(pend_spk(w).script().to_vec(), mk);
    }

    /// Review 7, the conservation statement of the family (DESIGN-ORACLE-IMPL 25.3), on every accepted step. An output
    /// of the family NAMES an account when (a) it carries the account's covenant id (a continuation: by the induction
    /// only an input of the id authorizes it), (b) it starts a fresh id and one of its maker fields holds the outpoint
    /// commitment of an input of the account, or (c) it is the family output at the index of an input of the account
    /// and one of the two carries no covenant id (a free stake's receipt, a winner's free stake). Then:
    ///   C1. every fresh family output names at least one input, every maker field it holds names a family input of
    ///       this transaction, and no input is named in two fields or by two fresh outputs (two inputs never accept
    ///       one output in one role, and no input makes two);
    ///   C2. each account's inputs are accounted for: their value is at most the value of the outputs naming the
    ///       account (an output named by several accounts, a documented composition, is counted for each) plus the
    ///       payouts at its inputs' positional slots (output i and M - 1 - i, to a party's key or the burn) plus what
    ///       the step declares (the premium of a renewal). No builder keeps an input's value by merging.
    fn accounts(&mut self, what: &str, t: &T, declared: &[(usize, i64)]) {
        let (tx, ents) = t;
        let (ni, m) = (tx.inputs.len(), tx.outputs.len());
        let fam = |s: &kaspa_consensus_core::tx::ScriptPublicKey| s.script().first() == Some(&0xaa);
        let acc_of = |i: usize| match ents[i].covenant_id {
            Some(c) => Acc::Id(c),
            None => Acc::Lone(i),
        };
        let in_ids: HashSet<Hash> = ents.iter().filter_map(|e| e.covenant_id).collect();
        let mut parties: Vec<Vec<u8>> = [long_kp(), short_kp(), uw_kp(), keypair(70)].iter().map(|k| key_spk(&x(k)).script().to_vec()).collect();
        parties.push(kpf_protocol::lock::burn_spk().script().to_vec());
        let mut named: HashMap<Acc, u64> = HashMap::new();
        let mut makers_seen: HashSet<usize> = HashSet::new();
        for (j, o) in tx.outputs.iter().enumerate() {
            if !fam(&o.script_public_key) {
                continue;
            }
            let mut namers: HashSet<Acc> = HashSet::new();
            if let Some(c) = o.covenant {
                if in_ids.contains(&c.covenant_id) {
                    namers.insert(Acc::Id(c.covenant_id));
                } else {
                    let mk = self.makers.get(o.script_public_key.script()).cloned().unwrap_or_else(|| panic!("{what}: output {j} is a fresh family output the run did not build"));
                    assert!(!mk.is_empty(), "{what}: the fresh output {j} names no maker");
                    for (role, h) in &mk {
                        let i = (0..ni).find(|&i| maker_of(&tx.inputs[i].previous_outpoint) == *h).unwrap_or_else(|| panic!("{what}: output {j} names as its {role} an input not in this transaction"));
                        assert!(fam(&ents[i].script_public_key), "{what}: output {j} names a plain input as its {role}");
                        assert!(makers_seen.insert(i), "{what}: input {i} is named twice as a maker");
                        namers.insert(acc_of(i));
                    }
                    self.count("fresh output named by its makers");
                }
            }
            if j < ni && fam(&ents[j].script_public_key) && (o.covenant.is_none() || ents[j].covenant_id.is_none()) {
                namers.insert(acc_of(j));
            }
            for a in namers {
                *named.entry(a).or_default() += o.value;
            }
        }
        let mut accs: HashMap<Acc, Vec<usize>> = HashMap::new();
        for i in (0..ni).filter(|&i| fam(&ents[i].script_public_key)) {
            accs.entry(acc_of(i)).or_default().push(i);
        }
        for (a, ins) in accs {
            let vin: u64 = ins.iter().map(|&i| ents[i].amount).sum();
            let mut slots: HashSet<usize> = HashSet::new();
            for &i in &ins {
                slots.insert(i);
                if m > i && m - 1 - i >= ni {
                    slots.insert(m - 1 - i);
                }
            }
            let pay: u64 = slots.iter().filter(|&&j| j < m && tx.outputs[j].covenant.is_none() && parties.contains(&tx.outputs[j].script_public_key.script().to_vec())).map(|&j| tx.outputs[j].value).sum();
            let dec: i64 = declared.iter().filter(|(i, _)| ins.contains(i)).map(|(_, d)| *d).sum();
            let have = named.get(&a).copied().unwrap_or(0) + pay + dec as u64;
            assert!(vin <= have, "{what}: account {a:?} of inputs {ins:?} holds {vin} sompi, its outputs and payouts {have}: value kept by the builder");
        }
        self.count("step accounted");
    }

    /// Review 7, a hostile builder's merge: pair `a` and its twin `b` (same state, another outpoint: a live pair of
    /// the run when two share a state, else a look-alike) both propose one close into ONE pending output, named once
    /// by each. Refused every time, by the input it does not name.
    fn merge_propose(&mut self, a: &LPair, b: &LPair) {
        let k = kit();
        let now = self.now as i64;
        let (w, fb) = a.s.pending_of(k, a.s.size, CLOSE, L, a.s.entry, now).expect("a close");
        let args = || vec![iv(a.s.size), iv(CLOSE), iv(L), iv(a.s.entry), iv(now), iv(2), Arg::Sig(long_kp()), iv(fb), iv(0), bv(&k.pending.prefix), bv(&k.pending.suffix)];
        for (named, idx) in [(a, 0usize), (b, 1)] {
            let w = w.made_by(&named.op);
            let ins = vec![pair_in(a, "propose", args()), pair_in(b, "propose", args()), key_in(outpoint(0xd2), 50_000_000_000, 0, &uw_kp()), exec_in(0xd3)];
            let mut outs = vec![out(a.value.max(b.value) + w.bond as u64, pend_spk(&w), None), change(90_000_000_000)];
            seal(&mut outs, 0, idx as u16, named.op);
            let t = build_ext(&ins, outs, self.now, &[0, 0, 0, 0]);
            let v = verdicts(&t);
            assert!(!accepted(&t) && !v[1 - idx], "a merge of two proposals into one pending part was accepted ({v:?})");
            self.books.merges_refused += 1;
        }
    }

    /// A hostile merge of two whole transfers of the long (of `a` and its twin `b`) into one part, named once by each.
    fn merge_transfer(&mut self, a: &LPair, b: &LPair) {
        let ek = long_kp();
        let p = &a.s;
        let pay = kpf_protocol::payoff::owed(p.size, p.long_ask - p.bl, p.long_tip, p.m.p.scale);
        let args = |l: &LPair| vec![iv(L), iv(l.s.size), iv(p.long_ask), iv(p.bl), bv(&x(&ek)), bv(&[0; 32]), iv(2)];
        for (named, idx) in [(a, 0usize), (b, 1)] {
            let mut part = p.rest_of(p.size);
            part.long_key = x(&ek);
            (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
            part.mk.parent = maker_of(&named.op);
            let ins = vec![pair_in(a, "transfer", args(a)), pair_in(b, "transfer", args(b)), exec_in(0xd4)];
            let mut outs = vec![out(pay.max(2_000_000) as u64, key_spk(&p.long_key), None), out(pay.max(2_000_000) as u64, key_spk(&b.s.long_key), None), out(a.value.max(b.value), pair_spk(&part), None), change(90_000_000_000)];
            seal(&mut outs, 2, idx as u16, named.op);
            let q = p.m.p.quiet_daa as u64;
            let t = build_ext(&ins, outs, 0, &[q, q, 0]);
            let v = verdicts(&t);
            assert!(!accepted(&t) && !v[1 - idx], "a merge of two hand-overs into one part was accepted ({v:?})");
            self.books.merges_refused += 1;
        }
    }

    /// A hostile merge of two pending parts (a claim and a look-alike of it) finalizing into one forfeited or restored
    /// part under the claim's id: the look-alike counts no output of its own id and refuses.
    fn merge_finalize(&mut self, c: &LPend, t: &T) {
        let d = Live { op: outpoint(0xd5), value: c.value, daa: c.daa, id: Some(Hash::from_bytes([0xd5; 32])), s: c.s.clone() };
        let ins = vec![pend_in(c, "finalize", vec![]), pend_in(&d, "finalize", vec![]), exec_in(0xd6)];
        let outs = t.0.outputs.clone();
        let seq = c.s.m.p.challenge_daa as u64;
        let h = build_ext(&ins, outs, 0, &[seq, seq, 0]);
        let v = verdicts(&h);
        assert!(!accepted(&h) && !v[1], "two pending parts finalized into one part ({v:?})");
        self.books.merges_refused += 1;
    }

    /// The pair `lp`'s twin for a merge: another live pair of the run with the same state (makers aside), else a
    /// look-alike written by a genesis.
    fn twin_of(&mut self, lp: &LPair) -> LPair {
        let plain = |p: &OPair| OPair { mk: Makers::default(), ..p.clone() };
        let want = plain(&lp.s);
        if let Some((t, _)) = self.pairs.iter().find(|(p, _)| p.op != lp.op && plain(&p.s) == want).cloned() {
            self.count("merge of two real pairs");
            return t;
        }
        genesis_pair(lp.s.clone(), lp.value, 0xd1, lp.daa)
    }

    /// Each step: a hostile builder merges a random live pair and its twin into one pending part.
    fn hostile_merges(&mut self) {
        if self.pairs.is_empty() {
            return;
        }
        let i = self.rng.below(self.pairs.len() as u64) as usize;
        let a = self.pairs[i].0.clone();
        let b = self.twin_of(&a);
        self.merge_propose(&a, &b);
    }
}

impl World {
    /// A hostile merge at an open: a second long order of the same terms runs `fill` into the pair the honest open
    /// writes (its maker is the first order's outpoint). Refused, by the second order.
    fn merge_fill(&mut self, lo: &LOrder, so: &LOrder, sp: &OpenSpec, t: &T, p: &OPair) {
        let k = kit();
        let (op2, id2) = self.id();
        let lo2 = Live { op: op2, value: lo.value, daa: lo.daa, id: Some(id2), s: lo.s.clone() };
        let fill = |ck: [u8; 32], ct: [u8; 32]| -> Vec<Arg> {
            vec![iv(2), iv(sp.n), iv(sp.x), iv(p.bl), iv(p.bs), bv(&ck), bv(&ct), iv(2), iv(0), iv(sp.xe), iv(sp.cover_rate), iv(sp.premium), bv(&k.pair.prefix), bv(&k.pair.suffix)]
        };
        let cover = q(sp.n, sp.cover_rate, p.m.p.scale);
        let a = kpf_protocol::bond::BindArgs::new(sp.lock as i64, sp.xe, sp.advance, &[kpf_protocol::bond::Claim::output(2, cover)]);
        let ins = vec![
            order_in(lo, "fill", fill(so.s.owner, maker_of(&so.op))),
            order_in(so, "fill", fill(lo.s.owner, maker_of(&lo.op))),
            slot_in(&self.slot, "bind", bind_args(&a, &uw_kp())),
            exec_in(200),
            order_in(&lo2, "fill", fill(so.s.owner, maker_of(&so.op))),
        ];
        let mut outs = t.0.outputs.clone();
        let (c2, v2) = order_after(&lo2, sp.n, lo2.s.fill_spend(sp.n, sp.premium));
        outs.insert(4, out(v2, order_spk(&c2.expect("rests on")), bind(4, id2)));
        let h = build_ext(&ins, outs, sp.lock, &[0, 0, 10, 0, 0]);
        let v = verdicts(&h);
        assert!(!accepted(&h) && !v[4], "two long orders filled one pair ({v:?})");
        self.books.merges_refused += 1;
    }
}
