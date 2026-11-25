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

/// Review 43: the second model (`research_solvency_notes.rs`, after review 42), copied, with the rate on the gross
/// a number of its own (the model takes the liquidation fee's), a leg booked after it entered (the gate then
/// counts what it gained since), and owners of the reviewer's making. A test named `..._breaks_...` or
/// `..._is_not_...` passes while that is so in the model.
mod second {
    const KAS: i64 = 100_000_000;
    const BPS: i64 = 10_000;
    const MMR: i64 = 500;
    const G: i64 = 50;
    const LIQ: i64 = 50;

    fn rate(w: i64, r: i64) -> i64 {
        (w / BPS) * r + ((w % BPS) * r + BPS - 1) / BPS
    }

    #[derive(Clone, Copy, Debug)]
    struct Leg {
        own: bool,
        units: i64,
        entry: i64,
        margin: i64,
        booked: bool,
    }

    impl Leg {
        fn worth(&self, p: i64) -> i64 {
            self.units.abs() * p
        }
        fn gain(&self, p: i64) -> i64 {
            self.units * (p - self.entry)
        }
    }

    #[derive(Clone, Debug, Default)]
    struct World {
        /// The rate on the gross (the model's: the liquidation fee's rate, 0.5 %).
        gross_rate: i64,
        coins: i64,
        legs: Vec<Leg>,
        owed: Vec<(bool, i64)>,
        short: i64,
        owner_took: i64,
        fees: i64,
        calls: i64,
        /// What users' legs that were never booked had gained when they left (paid by nobody), and what such legs
        /// paid the shard when they lost.
        forfeited: i64,
        taken_unbooked: i64,
    }

    impl World {
        fn new(coins: i64, gross_rate: i64) -> World {
            World { coins, gross_rate, ..World::default() }
        }
        fn net(&self) -> i64 {
            -self.legs.iter().filter(|l| l.booked).map(|l| l.units).sum::<i64>()
        }
        fn gross(&self) -> i64 {
            self.legs.iter().filter(|l| l.booked).map(|l| l.units.abs()).sum()
        }
        fn equity(&self, p: i64) -> i64 {
            self.coins - self.owed.iter().map(|o| o.1).sum::<i64>() - self.legs.iter().filter(|l| l.booked).map(|l| l.gain(p)).sum::<i64>()
        }
        fn requirement(&self, p: i64, net_rate: i64) -> i64 {
            rate(self.net().abs() * p, net_rate) + rate(self.gross() * p, self.gross_rate)
        }
        fn pay(&mut self) {
            self.owed.sort_by_key(|o| !o.0);
            for o in self.owed.iter_mut() {
                let paid = o.1.min(self.coins);
                self.coins -= paid;
                o.1 -= paid;
                if o.0 {
                    self.owner_took += paid;
                }
            }
            self.owed.retain(|o| o.1 > 0);
        }
        fn leave(&mut self, i: usize, p: i64) {
            let l = self.legs.remove(i);
            let gain = l.gain(p);
            if gain >= 0 {
                let asked = (gain - rate(l.worth(p), 2 * G)).max(0);
                if asked > 0 && l.booked {
                    self.owed.push((l.own, asked));
                } else if !l.own {
                    self.forfeited += asked;
                }
            } else {
                let paid = (-gain).min(l.margin);
                self.coins += paid;
                if l.own {
                    self.owner_took -= paid;
                } else if !l.booked {
                    self.taken_unbooked += paid;
                }
            }
        }
        /// A leg enters at the price with its maintenance as margin; `book`: booked as it enters, where the gate
        /// lets it.
        fn enter(&mut self, own: bool, units: i64, p: i64, book: bool) -> bool {
            self.legs.push(Leg { own, units, entry: p, margin: rate(units.abs() * p, MMR + G), booked: false });
            if book && !self.book(self.legs.len() - 1, p) {
                self.legs.pop();
                return false;
            }
            true
        }
        /// A leg is booked at the price: the gate is the shard's equity, counting what the leg has gained since it
        /// entered, at least the requirement with it.
        fn book(&mut self, i: usize, p: i64) -> bool {
            self.legs[i].booked = true;
            if self.equity(p) < self.requirement(p, MMR + G) {
                self.legs[i].booked = false;
            }
            self.legs[i].booked
        }
        fn draw(&mut self, p: i64) {
            self.pay();
            if !self.owed.is_empty() {
                return;
            }
            let amount = (self.equity(p) - self.requirement(p, MMR + G)).min(self.coins).max(0);
            self.coins -= amount;
            self.owner_took += amount;
        }
        fn watch(&mut self, p: i64) {
            let mut i = 0;
            while i < self.legs.len() {
                let l = self.legs[i];
                if l.margin + l.gain(p) < rate(l.worth(p), MMR - G) {
                    let fee = rate(l.worth(p), LIQ).min((l.margin + l.gain(p)).max(0));
                    self.legs[i].margin -= fee;
                    self.fees += fee;
                    self.leave(i, p);
                } else {
                    i += 1;
                }
            }
            self.pay();
            if self.equity(p) < self.requirement(p, MMR - G) {
                self.calls += 1;
                let cap = rate(self.gross() * p, self.gross_rate);
                let mut i = 0;
                while i < self.legs.len() {
                    if self.legs[i].booked {
                        self.leave(i, p);
                    } else {
                        i += 1;
                    }
                }
                let total: i64 = self.owed.iter().map(|o| o.1).sum();
                if total <= self.coins {
                    self.pay();
                    let fee = cap.min(self.coins);
                    self.coins -= fee;
                    self.fees += fee;
                } else {
                    let coins = self.coins;
                    for o in self.owed.drain(..) {
                        let paid = (o.1 as i128 * coins as i128 / total as i128) as i64;
                        self.coins -= paid;
                        if o.0 {
                            self.owner_took += paid;
                        } else {
                            self.short += o.1 - paid;
                        }
                    }
                }
            }
        }
        fn end(&mut self, p: i64) {
            while !self.legs.is_empty() {
                self.leave(0, p);
            }
            self.pay();
            self.short += self.owed.iter().filter(|o| !o.0).map(|o| o.1).sum::<i64>();
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

    fn users(w: &mut World, g: &mut Rng, p: i64) {
        if g.range(0, 2) == 0 {
            let units = g.range(1, 40) * if g.range(0, 1) == 0 { 1 } else { -1 };
            w.enter(false, units, p, true);
        }
        if g.range(0, 5) == 0 && !w.legs.is_empty() {
            let i = g.range(0, w.legs.len() as i64 - 1) as usize;
            if !w.legs[i].own && w.legs[i].margin + w.legs[i].gain(p) >= 0 {
                w.leave(i, p);
            }
        }
    }

    fn leave_own(w: &mut World, p: i64, which: impl Fn(&Leg) -> bool) {
        let mut i = 0;
        while i < w.legs.len() {
            if w.legs[i].own && which(&w.legs[i]) {
                w.leave(i, p);
            } else {
                i += 1;
            }
        }
    }

    /// The owner that sets its users off and leaves them: each tick it enters an account of its own against its
    /// users' net (the book is then set off, and asked the rate on the gross alone), draws all it may, and, where
    /// the next tick goes its users' way, leaves that account before the price moves.
    fn set_off_and_leave(seed: u64, tick: i64, gross_rate: i64) -> World {
        let mut g = Rng(seed);
        let mut w = World::new(500 * KAS, gross_rate);
        let mut p = 10 * KAS;
        for _ in 0..80 {
            let next = p + p / BPS * g.range(-tick, tick);
            users(&mut w, &mut g, p);
            leave_own(&mut w, p, |_| true);
            let net: i64 = w.legs.iter().filter(|l| l.booked).map(|l| l.units).sum();
            if net != 0 {
                w.enter(true, -net, p, true);
            }
            w.draw(p);
            if (next > p) == (net > 0) {
                leave_own(&mut w, p, |_| true);
            }
            p = next;
            w.watch(p);
        }
        w.end(p);
        w
    }

    /// The owner that books late: it enters accounts of its own of every size the way the price goes next, none
    /// booked; after the tick it books the largest the gate lets in (the gate counting what the leg gained), has
    /// it leave, and draws; and it acts again after the price moved and before whoever watches.
    fn book_late(seed: u64, tick: i64) -> World {
        let mut g = Rng(seed);
        let mut w = World::new(500 * KAS, LIQ);
        let mut p = 10 * KAS;
        for _ in 0..80 {
            let next = p + p / BPS * g.range(-tick, tick);
            users(&mut w, &mut g, p);
            let mut size = 4_000;
            while size >= 1 {
                w.enter(true, if next > p { size } else { -size }, p, false);
                size /= 2;
            }
            w.draw(p);
            p = next;
            // After the price: book what the gate takes, largest first; out of everything of its own; draw.
            let mut i = 0;
            while i < w.legs.len() {
                if w.legs[i].own && !w.legs[i].booked {
                    w.book(i, p);
                }
                i += 1;
            }
            leave_own(&mut w, p, |_| true);
            w.draw(p);
            w.watch(p);
        }
        w.end(p);
        w
    }

    fn unpaid(f: impl Fn(u64) -> World, paths: u64) -> (u64, i64, i64) {
        let (mut n, mut short, mut took) = (0, 0, 0);
        for seed in 1..=paths {
            let w = f(seed * 7_919);
            if w.short > 0 {
                n += 1;
                short += w.short;
            }
            took += w.owner_took;
        }
        (n, short / paths as i64 / KAS, took / paths as i64 / KAS)
    }

    /// N3. The bound of a set-off book is the bound of every book, at the owner's choice: an account of its own
    /// against its users' net makes any book set off (and lets the owner draw what a one-way book was asked), and
    /// leaving it before a tick that goes its users' way makes that side stand against a shard that holds the rate
    /// on the gross. With the rules on: no user unpaid at ticks of 1.5 %; users unpaid at ticks of 3 % and of 4.5 %,
    /// which the claim's bound (the maintenance rate less the tolerance) includes.
    #[test]
    fn an_owner_that_sets_its_users_off_and_leaves_them_breaks_the_claim_within_its_bound() {
        let within = unpaid(|s| set_off_and_leave(s + 150, 150, LIQ), 1_000);
        let three = unpaid(|s| set_off_and_leave(s + 300, 300, LIQ), 1_000);
        let whole = unpaid(|s| set_off_and_leave(s + 450, 450, LIQ), 1_000);
        println!("the owner that sets its users off and leaves them, the rate on the gross 0.5 %: users unpaid on (paths of 1,000, KAS a path) {within:?} at ticks of 1.5 %, {three:?} at 3 %, {whole:?} at 4.5 %");
        assert_eq!(within.0, 0);
        assert!(three.0 > 0 && whole.0 > three.0, "{three:?} {whole:?}");
    }

    /// And with the rate on the gross that section 12 computes for the two bounds to be one (half of the
    /// maintenance rate less three times the tolerance: 1.75 %), the same owner leaves nobody unpaid at ticks of
    /// 4.5 %; with 1.5 % it does.
    #[test]
    fn the_rate_on_the_gross_that_makes_the_two_bounds_one_holds_against_that_owner() {
        let at = unpaid(|s| set_off_and_leave(s + 450, 450, (MMR - 3 * G) / 2), 1_000);
        let under = unpaid(|s| set_off_and_leave(s + 450, 450, 150), 1_000);
        println!("the same owner at ticks of 4.5 %: with a rate on the gross of 1.75 %, users unpaid on {at:?}; with 1.5 %, on {under:?}");
        assert_eq!(at.0, 0);
        assert!(under.0 > 0);
    }

    /// Booking late, the gate counting what the leg gained since it entered: could not be broken. An owner with
    /// accounts of its own of every size, entered the way of the next tick and booked after it, takes no more
    /// than the room above the requirement, which was its to draw.
    #[test]
    fn a_leg_booked_after_it_gained_takes_no_more_than_the_room_the_gate_counts() {
        for tick in [50, 200, MMR - G] {
            let r = unpaid(|s| book_late(s + tick as u64, tick), 1_000);
            println!("the owner that books late, ticks of {tick} bps: users unpaid on {r:?}");
            assert_eq!(r.0, 0, "ticks of {tick} bps");
        }
    }

    /// N1. A leg that is not booked: what it loses is taken and what it gains is paid by nobody. The model has a
    /// leg that lost pay the shard from its margin whether it was booked or not, and a leg that gained be paid only
    /// where it was booked. For the owner's own unbooked account that is the point (review 42, H2). For a user
    /// whose entry is settled and whose booking is late or refused it is a bet it can only lose: here 100 users of
    /// 100 units, half each way, entered and not booked; the price moves 2 % and all leave. The claim's measure, users unpaid of what they are owed, is nothing.
    #[test]
    fn what_an_unbooked_leg_loses_is_taken_and_what_it_gains_is_not_paid() {
        let p = 10 * KAS;
        let mut w = World::new(100 * KAS, LIQ);
        for i in 0..100 {
            assert!(w.enter(false, if i % 2 == 0 { 100 } else { -100 }, p, false), "the leg is in its account, not booked");
        }
        let p = p + p / BPS * 200;
        let had = w.coins;
        while !w.legs.is_empty() {
            w.leave(0, p);
        }
        w.pay();
        println!("100 users of 100 units, half each way, entered and never booked, a move of 2 %: the half that gained is paid nothing of {} KAS (beyond the band), the half that lost pays the shard {} KAS; the shard held {} KAS and holds {}; users unpaid of what they are owed: {}", w.forfeited / KAS, w.taken_unbooked / KAS, had / KAS, w.coins / KAS, w.short);
        assert!(w.forfeited > 400 * KAS && w.taken_unbooked > 900 * KAS && w.short == 0 && w.owed.is_empty());
    }

    /// N2. The two levels are one for a book that is set off: they differ by the tolerance of the **net**. A shard
    /// drawn to its upper level over two users, one each way, is below its lower level after a rise of a
    /// hundredth of a percent (the gross is counted at the price); it is called, every user is closed, and the
    /// fee, capped at the rate on the gross, is all it held.
    #[test]
    fn a_set_off_book_at_its_requirement_is_called_at_a_hair_and_the_capped_fee_is_all_it_held() {
        let p = 10 * KAS;
        let mut w = World::new(1_000 * KAS, LIQ);
        assert!(w.enter(false, 1_000, p, true) && w.enter(false, -1_000, p, true));
        w.draw(p);
        let had = w.coins;
        assert_eq!((had, w.requirement(p, MMR + G), w.requirement(p, MMR - G)), (100 * KAS, 100 * KAS, 100 * KAS), "the two levels are one where the net is nothing");
        w.watch(p + p / BPS);
        println!("a set-off book at its requirement, the price up 0.01 %: called {} time, {} legs standing, whoever called has {} KAS of the {} it held, the shard {} sompi", w.calls, w.legs.len(), w.fees / KAS, had / KAS, w.coins);
        assert!(w.calls == 1 && w.legs.is_empty() && w.fees * 100 >= had * 99 && w.short == 0);
    }

    /// The share of a call, and the owner's own notes in it: could not be turned against the users. At a gap of
    /// 10 % a user long 1,000 units alone over a shard at its upper level is paid 600 KAS of the 890 it is owed.
    /// With a pair of the owner's own accounts of 100,000 units each way beside it (booked: the gate asked the rate
    /// on their gross of the shard), the one that lost pays in what its margin holds, the one that won shares with
    /// the user, and the user is paid more, not less: the owner's accounts brought more to the call than their
    /// share takes. An account of the owner's own on the user's side, booked, brings what the user's brought.
    #[test]
    fn the_owners_own_notes_take_no_more_of_a_failing_shard_than_they_brought() {
        let p = 10 * KAS;
        let gap = p + p / BPS * 1_000;
        let run = |own: &[i64]| {
            let mut w = World::new(100_000 * KAS, LIQ);
            assert!(w.enter(false, 1_000, p, true));
            for u in own {
                assert!(w.enter(true, *u, p, true));
            }
            w.draw(p);
            let held = w.coins;
            w.watch(gap);
            assert!(w.calls == 1 && w.legs.is_empty());
            (held / KAS, w.short / KAS, w.owner_took / KAS)
        };
        let alone = run(&[]);
        let pair = run(&[100_000, -100_000]);
        let beside = run(&[100_000]);
        println!("a gap of 10 %, a user long 1,000 units owed 890 KAS: (the shard held, the user is short, the owner's accounts and draws took) alone {alone:?}; with an own pair of 100,000 each way {pair:?}; with an own account of 100,000 the user's way {beside:?}");
        assert!(pair.1 <= alone.1 && beside.1 <= alone.1 + 1, "the owner's own notes took from the user's share");
    }
}
