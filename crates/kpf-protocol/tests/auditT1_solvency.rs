//! Review 42 (docs/audit/REVIEW-T1b.md): the numbers model of `research_solvency.rs` (docs/ARCHITECTURE-CLEARING.md
//! 12), copied and extended to attack its claim in its own terms. The model's rules are as they are there (books,
//! gate, call; the same rates); what is added is what an owner or a stranger may do that the model's `run` does
//! not let them:
//!   - an account its owner keeps under statements, which nobody steps (M1 of review 38): `held_by_owner`;
//!   - whoever watches coming every second tick;
//!   - a user's claim that is not paid when it is made waiting for what comes in (a note goes on with the rest),
//!     so that "unpaid" is what is unpaid at the end and not what waited;
//!   - a call stated at the edge of the tolerance.
//! A test named `..._breaks_the_claim` passes while the claim is broken in the model.

const KAS: i64 = 100_000_000;
const BPS: i64 = 10_000;
const MMR: i64 = 500;
const G: i64 = 50;
const LIQ: i64 = 50;

fn rate(w: i64, r: i64) -> i64 {
    (w / BPS) * r + ((w % BPS) * r + BPS - 1) / BPS
}

#[derive(Clone, Copy, Debug)]
struct Acct {
    own: bool,
    /// Its owner keeps it under statements: nobody steps it, by its own numbers or at a call.
    held_by_owner: bool,
    units: i64,
    entry: i64,
    margin: i64,
}

impl Acct {
    fn worth(&self, p: i64) -> i64 {
        self.units.abs() * p
    }
    fn gain(&self, p: i64) -> i64 {
        self.units * (p - self.entry)
    }
}

#[derive(Clone, Debug, Default)]
struct World {
    held: i64,
    accts: Vec<Acct>,
    /// What users are owed and not yet paid (their notes go on with the rest, and are paid as coins come in).
    waiting: i64,
    /// The most that waited at once.
    waited: i64,
    owner_took: i64,
    fees: i64,
    calls: i64,
}

impl World {
    fn net(&self) -> i64 {
        -self.accts.iter().map(|a| a.units).sum::<i64>()
    }
    fn gross(&self) -> i64 {
        self.accts.iter().map(|a| a.units.abs()).sum()
    }
    /// The books' equity, as in the model: every account's gain or loss whole.
    fn equity(&self, p: i64) -> i64 {
        self.held - self.waiting - self.accts.iter().map(|a| a.gain(p)).sum::<i64>()
    }
    fn requirement(&self, p: i64) -> i64 {
        rate(self.net().abs() * p, MMR + G) + rate(self.gross() * p, LIQ)
    }
    fn pay_waiting(&mut self) {
        let paid = self.waiting.min(self.held);
        self.held -= paid;
        self.waiting -= paid;
    }
    fn leave(&mut self, i: usize, p: i64) {
        let a = self.accts.remove(i);
        let gain = a.gain(p);
        if gain >= 0 {
            let asked = (gain - rate(a.worth(p), 2 * G)).max(0);
            if a.own {
                // (The owner's own account is paid what is there, after what users wait for.)
                let paid = asked.min((self.held - self.waiting).max(0));
                self.held -= paid;
                self.owner_took += paid;
            } else {
                self.waiting += asked;
                self.pay_waiting();
                self.waited = self.waited.max(self.waiting);
            }
        } else {
            let paid = (-gain).min(a.margin);
            self.held += paid;
            if a.own {
                self.owner_took -= paid;
            }
            self.pay_waiting();
        }
    }
    /// Whoever watches, at the price: accounts below their maintenance are stepped; with `call`, a shard whose
    /// equity is below its requirement at `at` (the price the call is stated at) is stepped, the owner's own
    /// first. An account its owner holds under statements is stepped by neither.
    fn watch(&mut self, p: i64, call: bool) {
        let mut i = 0;
        while i < self.accts.len() {
            let a = self.accts[i];
            if !a.held_by_owner && a.margin + a.gain(p) < rate(a.worth(p), MMR - G) {
                let fee = rate(a.worth(p), LIQ).min((a.margin + a.gain(p)).max(0));
                self.accts[i].margin -= fee;
                self.fees += fee;
                self.leave(i, p);
            } else {
                i += 1;
            }
        }
        if call && self.equity(p) < self.requirement(p) {
            self.call(p);
        }
    }
    fn call(&mut self, p: i64) {
        self.calls += 1;
        self.accts.sort_by_key(|a| (a.held_by_owner, !a.own));
        while self.accts.first().is_some_and(|a| !a.held_by_owner) {
            let fee = rate(self.accts[0].worth(p), LIQ).min((self.held - self.waiting).max(0));
            self.held -= fee;
            self.fees += fee;
            self.leave(0, p);
        }
    }
    fn enter(&mut self, own: bool, held_by_owner: bool, units: i64, p: i64, gate: bool) -> bool {
        let a = Acct { own, held_by_owner, units, entry: p, margin: rate(units.abs() * p, MMR + G) };
        self.accts.push(a);
        if gate && self.equity(p) < self.requirement(p) {
            self.accts.pop();
            return false;
        }
        true
    }
    fn draw(&mut self, p: i64) {
        let amount = (self.equity(p) - self.requirement(p)).min(self.held - self.waiting).max(0);
        self.held -= amount;
        self.owner_took += amount;
    }
    /// The end: every leg that can be stepped leaves at the last price, those that lose first (so that what waits
    /// is what could not be paid at all). An account its owner holds under statements stays: returns what users
    /// still wait for, and what such accounts owe the shard (their loss, as far as their margin goes).
    fn end(&mut self, p: i64) -> (i64, i64) {
        self.accts.sort_by_key(|a| (a.held_by_owner, a.gain(p) > 0));
        while self.accts.first().is_some_and(|a| !a.held_by_owner) {
            self.leave(0, p);
        }
        (self.waiting, self.accts.iter().map(|a| (-a.gain(p)).clamp(0, a.margin)).sum())
    }
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

fn users(w: &mut World, r: &mut Rng, p: i64) {
    if r.range(0, 2) == 0 {
        let units = r.range(1, 40) * if r.range(0, 1) == 0 { 1 } else { -1 };
        w.enter(false, false, units, p, true);
    }
    if r.range(0, 5) == 0 && !w.accts.is_empty() {
        let i = r.range(0, w.accts.len() as i64 - 1) as usize;
        if !w.accts[i].own && w.accts[i].margin + w.accts[i].gain(p) >= 0 {
            w.leave(i, p);
        }
    }
}

/// The model's own owner (an account the way the next tick goes, out of the ones that would lose, all it may
/// draw), with what is added: the owner acting again after the price moved and before whoever watches (`after`),
/// and whoever watches coming every `every` ticks.
fn run(seed: u64, tick: i64, after: bool, every: usize) -> (World, i64) {
    let mut r = Rng(seed);
    let mut w = World { held: 500 * KAS, ..World::default() };
    let mut p = 10 * KAS;
    for t in 0..80 {
        let next = p + p / BPS * r.range(-tick, tick);
        users(&mut w, &mut r, p);
        let up = next > p;
        let mut i = 0;
        while i < w.accts.len() {
            if w.accts[i].own && (w.accts[i].units > 0) != up {
                w.leave(i, p);
            } else {
                i += 1;
            }
        }
        let mut size = 2_000;
        while size >= 1 {
            w.enter(true, false, if up { size } else { -size }, p, true);
            size /= 2;
        }
        w.draw(p);
        p = next;
        if after {
            // The owner first: out of its accounts that won, and all it may draw.
            let mut i = 0;
            while i < w.accts.len() {
                if w.accts[i].own && w.accts[i].gain(p) > 0 {
                    w.leave(i, p);
                } else {
                    i += 1;
                }
            }
            w.draw(p);
        }
        if (t + 1) % every == 0 {
            w.watch(p, true);
        }
    }
    let unpaid = w.end(p).0;
    (w, unpaid)
}

/// The claim as the model makes it, with what is added and nothing of the owner's new: it holds. A user's claim may
/// wait (the owner's draws and its own accounts' winnings left the shard without coins while what the losing
/// accounts owe was still in their margins), and is paid when that comes in.
#[test]
fn the_claim_holds_in_the_models_terms_where_every_account_is_stepped_in_time() {
    for tick in [50, 200, MMR - G] {
        for after in [false, true] {
            let (mut unpaid, mut waited, mut calls) = (0, 0i64, 0i64);
            for seed in 1..=500u64 {
                let (w, short) = run(seed * 7_919 + tick as u64, tick, after, 1);
                unpaid += (short > 0) as i32;
                waited = waited.max(w.waited);
                calls += w.calls;
            }
            println!("ticks of {tick} bps, the owner acting after the price too: {after}: users unpaid at the end on {unpaid} of 500 paths; the most that waited at once {} KAS; {} calls a path", waited / KAS, calls / 500);
            assert_eq!(unpaid, 0, "ticks of {tick} bps, after {after}");
        }
    }
}

/// Whoever watches coming less often: the bound is on the move between its visits, not on a tick. With visits
/// every second tick and ticks of half the bound the claim holds; what happens beyond is printed (the model's
/// users are small beside what the shard holds, so it takes a long absence to leave one unpaid).
#[test]
fn the_bound_is_on_the_move_between_two_visits_of_whoever_watches() {
    let unpaid = |tick: i64, every: usize| (1..=500u64).filter(|seed| run(seed * 7_919 + tick as u64, tick, true, every).1 > 0).count();
    assert_eq!(unpaid((MMR - G) / 2, 2), 0);
    let late: Vec<(usize, usize)> = [2usize, 4, 8, 16].iter().map(|e| (*e, unpaid(MMR - G, *e))).collect();
    println!("ticks of 450 bps, whoever watches every n-th tick: (n, users unpaid on so many of 500 paths) {late:?}");
    assert!(late.iter().any(|(_, u)| *u > 0), "{late:?}");
}

/// H1. The books count an account's loss whole; the shard has it only when somebody steps the account, and nobody
/// steps an account its owner keeps under statements (a statement that changes nothing is right, and an account
/// carries one statement at a time: M1). The owner: two accounts of its own, one each way, as large as the gate
/// lets a pair in (a pair is asked the rate on the gross alone), both kept under statements. As the price goes,
/// the one that wins leaves and is paid in coins; the one that loses stays, and its loss stands in the books as
/// equity, against which the owner draws and no call is made. With ticks of 2 % (within the bound), users are
/// unpaid on many paths for as long as the owner holds that account (what it would pay if it were stepped is
/// its margin, the owner's to give up), and the owner is ahead after giving it up.
#[test]
fn an_account_its_owner_keeps_from_being_stepped_breaks_the_claim() {
    let (mut unpaid, mut short, mut took, mut no_call) = (0, 0i64, 0i64, 0);
    let paths = 500u64;
    for seed in 1..=paths {
        let mut r = Rng(seed * 7_919 + 200);
        let mut w = World { held: 500 * KAS, ..World::default() };
        let mut p = 10 * KAS;
        // The pair, as large as the gate lets in.
        let mut size = 8_000;
        while size >= 1 {
            if w.enter(true, true, size, p, true) {
                if !w.enter(true, true, -size, p, true) {
                    w.accts.pop();
                } else {
                    break;
                }
            }
            size /= 2;
        }
        for _ in 0..80 {
            users(&mut w, &mut r, p);
            w.draw(p);
            p += p / BPS * r.range(-200, 200);
            // The one of the pair that has won 8 % leaves; the other stays where nobody steps it.
            if let Some(i) = w.accts.iter().position(|a| a.own && a.gain(p) > rate(a.worth(p), 800)) {
                w.leave(i, p);
                w.draw(p);
            }
            w.watch(p, true);
        }
        let before = (w.equity(p), w.requirement(p));
        let (s, left_behind) = w.end(p);
        if s > 0 {
            unpaid += 1;
            short += s;
            // The books said the shard was sound to the end.
            no_call += (before.0 >= before.1) as i32;
        }
        // What the owner has: what its accounts and draws took, less the margin it leaves in the account it holds.
        took += w.owner_took - w.accts.iter().map(|a| a.margin).sum::<i64>();
        let _ = left_behind;
    }
    println!("an own pair kept from being stepped, ticks of 200 bps, the rules on: users unpaid on {unpaid} of {paths} paths ({} KAS a path that is), on {no_call} of them with the books' equity at or above the requirement at the end; the owner ahead by {} KAS a path", if unpaid > 0 { short / unpaid as i64 / KAS } else { 0 }, took / paths as i64 / KAS);
    assert!(unpaid > paths as i32 / 10 && took > 0, "{unpaid} unpaid, the owner took {took}");
}

/// H2. A call stated at the edge of the tolerance, and its fee. A shard whose owner has drawn all it may stands at
/// its requirement exactly. (a) With a book that is set off (two users, one each way), the price need not move
/// against it: the requirement is of worth, so a rise of a hundredth of a percent puts the equity below it. The
/// call is right; every leg leaves; the fees, the liquidation rate of the gross, are all the shard has. (b) With
/// a book that is not set off, a call stated at the end of the tolerance that is against the shard is right
/// while its equity at the committee's price is above its requirement by less than the tolerance of its net.
#[test]
fn a_call_at_a_hair_closes_every_user_of_a_shard_at_its_requirement_and_its_fees_are_all_it_has() {
    // (a)
    let p = 10 * KAS;
    let mut w = World { held: 5_000 * KAS, ..World::default() };
    assert!(w.enter(false, false, 1_000, p, true) && w.enter(false, false, -1_000, p, true));
    w.draw(p);
    assert_eq!((w.equity(p), w.net()), (w.requirement(p), 0), "at its requirement, set off");
    let had = w.held;
    let up = p + p / BPS;
    assert!(w.equity(up) < w.requirement(up), "a hundredth of a percent");
    w.watch(up, true);
    assert_eq!((w.calls, w.accts.len()), (1, 0), "called, and every leg left");
    println!("(a) a shard at its requirement over two users of 1,000 units each way, the price up 0.01 %: called; it held {} KAS and whoever stepped has {} KAS; users wait for {} sompi at the end", had / KAS, w.fees / KAS, w.waiting);
    assert!(w.fees * 100 >= had * 99, "the fees are what it held");
    // (b)
    let mut w = World { held: 5_000 * KAS, ..World::default() };
    assert!(w.enter(false, false, 1_000, p, true));
    w.draw(p);
    // The owner tops it up by a quarter of a percent of the net's worth: sound at the committee's price.
    w.held += rate(1_000 * p, 25);
    assert!(w.equity(p) > w.requirement(p));
    let edge = p + rate(p, G);
    assert!(w.equity(edge) < w.requirement(edge), "below at the end of the tolerance that is against it");
    println!("(b) a shard above its requirement by 0.25 % of its net at the committee's price is below it at the price 0.5 % higher: a call stated there is within the tolerance");
}
