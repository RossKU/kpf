//! Research, numbers only (docs/ARCHITECTURE-CLEARING.md 12, after review 42): the second model of a shard held to
//! its requirement all the time, with what review 42 found the first had taken for granted.
//!
//! - A covered account holds its user's margin and nothing of the clearing side: the other side is the shard's
//!   books. A leg that gained is paid by the shard, by a note; one that lost pays the shard from its margin.
//! - **Booked legs** (H2): the shard pays nothing for a leg it has not booked; booking is the gate (the shard's
//!   equity at least its requirement with the leg); what is paid is counted from the entry booked.
//! - **A step reaches an account that carries a statement** (H1): an owner that holds its own losing account under
//!   statements of nothing does not keep it from being stepped.
//! - **Two levels** (H3) and **coins** (H4): a draw leaves the equity at the requirement counted the tolerance
//!   more, and is of coins that are there and owed to nobody; a call is right below the requirement counted the
//!   tolerance less. At a call every booked leg leaves, nothing is paid until all have, and then every note is
//!   paid the same share.
//! - Notes are paid when somebody applies them; a statement on the shard waits for the notes before it.
//!
//! The claim: whatever the owner does with accounts of its own, every user is paid in full what it is owed beyond
//! the band, as long as the price moves by less than the maintenance rate less the tolerance between two visits of
//! whoever watches. And each of the two things review 42 found, left out, breaks it.

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
    /// Side A's units: above zero long, below zero short.
    units: i64,
    entry: i64,
    margin: i64,
    booked: bool,
    /// Its account carries a statement of nothing, renewed without a gap, by its own side A.
    held: bool,
}

impl Leg {
    fn worth(&self, p: i64) -> i64 {
        self.units.abs() * p
    }
    fn gain(&self, p: i64) -> i64 {
        self.units * (p - self.entry)
    }
}

#[derive(Clone, Copy, Debug)]
struct Rules {
    /// The shard pays for booked legs alone, and booking is the gate. Without: every leg is paid, the books know
    /// only the legs whose owners told them, and the requirement is asked at a draw alone, of what the shard holds
    /// (as built today).
    book: bool,
    /// A step is stated over a statement.
    over: bool,
    /// Whoever watches comes every so many ticks.
    every: usize,
}

#[derive(Clone, Debug, Default)]
struct World {
    coins: i64,
    legs: Vec<Leg>,
    /// Notes of what is owed, not yet paid: (to an account of the owner's own, the amount).
    owed: Vec<(bool, i64)>,
    short: i64,
    owner_took: i64,
    fees: i64,
}

impl World {
    fn net(&self) -> i64 {
        -self.legs.iter().filter(|l| l.booked).map(|l| l.units).sum::<i64>()
    }
    fn gross(&self) -> i64 {
        self.legs.iter().filter(|l| l.booked).map(|l| l.units.abs()).sum()
    }
    /// The books' equity: the coins, less what is owed by notes, less what the booked legs have gained at the price
    /// (each counted whole: a script sums).
    fn equity(&self, p: i64, r: Rules) -> i64 {
        if !r.book {
            return self.coins;
        }
        self.coins - self.owed.iter().map(|o| o.1).sum::<i64>() - self.legs.iter().filter(|l| l.booked).map(|l| l.gain(p)).sum::<i64>()
    }
    fn requirement(&self, p: i64, net_rate: i64) -> i64 {
        rate(self.net().abs() * p, net_rate) + rate(self.gross() * p, LIQ)
    }
    /// Notes are paid in full as they come, the owner's own first (the worst order for its users), as far as the
    /// coins go; what is not paid waits.
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
    /// A leg leaves at the price. One that gained leaves a note of what the shard is asked (beyond the band; with
    /// the booking rule, only where the leg was booked); one that lost pays the shard from its margin.
    fn leave(&mut self, i: usize, p: i64, r: Rules) {
        let l = self.legs.remove(i);
        let gain = l.gain(p);
        if gain >= 0 {
            let asked = (gain - rate(l.worth(p), 2 * G)).max(0);
            if asked > 0 && (l.booked || !r.book) {
                self.owed.push((l.own, asked));
            }
        } else {
            let paid = (-gain).min(l.margin);
            self.coins += paid;
            if l.own {
                self.owner_took -= paid;
            }
        }
    }
    fn reached(&self, l: &Leg, r: Rules) -> bool {
        r.over || !l.held
    }
    /// A leg enters at the price with its maintenance as margin. `book`: its entry is booked at once (with the
    /// booking rule, only where the shard's equity is at least its requirement with it: else it does not enter).
    fn enter(&mut self, own: bool, units: i64, p: i64, r: Rules, book: bool, held: bool) -> bool {
        self.legs.push(Leg { own, units, entry: p, margin: rate(units.abs() * p, MMR + G), booked: book, held });
        if r.book && book && self.equity(p, r) < self.requirement(p, MMR + G) {
            self.legs.pop();
            return false;
        }
        true
    }
    /// The owner draws all the rule gives it. With the rules: the equity above the requirement counted the
    /// tolerance more, of coins that are there once every note is paid. Without: what the shard holds above its
    /// requirement.
    fn draw(&mut self, p: i64, r: Rules) {
        self.pay();
        if !self.owed.is_empty() {
            return;
        }
        let amount = (self.equity(p, r) - self.requirement(p, MMR + G)).min(self.coins).max(0);
        self.coins -= amount;
        self.owner_took += amount;
    }
    /// What whoever watches does at a visit: steps every account it reaches whose side A is below its maintenance;
    /// has the notes paid; and, with the rules, calls a shard below its requirement counted the tolerance less:
    /// every booked leg it reaches leaves, and then every note is paid the same share, the fees after.
    fn watch(&mut self, p: i64, r: Rules) {
        let mut i = 0;
        while i < self.legs.len() {
            let l = self.legs[i];
            if l.margin + l.gain(p) < rate(l.worth(p), MMR - G) && self.reached(&l, r) {
                let fee = rate(l.worth(p), LIQ).min((l.margin + l.gain(p)).max(0));
                self.legs[i].margin -= fee;
                self.fees += fee;
                self.leave(i, p, r);
            } else {
                i += 1;
            }
        }
        self.pay();
        if r.book && self.equity(p, r) < self.requirement(p, MMR - G) {
            let cap = rate(self.gross() * p, LIQ);
            let mut i = 0;
            while i < self.legs.len() {
                let l = self.legs[i];
                if l.booked && self.reached(&l, r) {
                    self.leave(i, p, r);
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
    /// The end: the owner is gone. Every leg that is reached leaves at the price; what is owed is paid as far as
    /// the coins go, the owner's own first; what a user is not paid is its loss.
    fn end(&mut self, p: i64, r: Rules) {
        let mut i = 0;
        while i < self.legs.len() {
            let l = self.legs[i];
            if self.reached(&l, r) {
                self.leave(i, p, r);
            } else {
                i += 1;
            }
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Owner {
    /// An account of its own the way the price goes next, as large as it gets in; out of the ones that would lose.
    Way,
    /// A pair of its own accounts, one each way, both held under statements; the one that won leaves, the one
    /// that lost stands (review 42, H1).
    Pair,
    /// An account of its own the way the price goes next whose entry it never has booked (review 42, H2).
    Unbooked,
}

fn run(seed: u64, tick: i64, r: Rules, owner: Owner) -> World {
    let mut g = Rng(seed);
    let mut w = World { coins: 500 * KAS, ..World::default() };
    let mut p = 10 * KAS;
    for t in 0..80usize {
        let next = p + p / BPS * g.range(-tick, tick);
        // Users come (booked as they enter) and go.
        if g.range(0, 2) == 0 {
            let units = g.range(1, 40) * if g.range(0, 1) == 0 { 1 } else { -1 };
            w.enter(false, units, p, r, true, false);
        }
        if g.range(0, 5) == 0 && !w.legs.is_empty() {
            let i = g.range(0, w.legs.len() as i64 - 1) as usize;
            if !w.legs[i].own && w.legs[i].margin + w.legs[i].gain(p) >= 0 {
                w.leave(i, p, r);
            }
        }
        // The owner, who knows the next tick.
        let up = next > p;
        let mut i = 0;
        while i < w.legs.len() {
            let l = w.legs[i];
            let out = match owner {
                // (It leaves what would lose; under the pair, what has won: the loser stands.)
                Owner::Way | Owner::Unbooked => l.own && (l.units > 0) != up,
                Owner::Pair => l.own && l.gain(p) > rate(l.worth(p), 2 * G),
            };
            if out {
                w.leave(i, p, r);
            } else {
                i += 1;
            }
        }
        let mut size = 4_000;
        while size >= 1 {
            let entered = match owner {
                Owner::Way => w.enter(true, if up { size } else { -size }, p, r, true, false),
                Owner::Unbooked => w.enter(true, if up { size } else { -size }, p, r, false, false),
                Owner::Pair => {
                    let a = w.enter(true, size, p, r, true, true);
                    let b = a && w.enter(true, -size, p, r, true, true);
                    if a && !b {
                        w.legs.pop();
                    }
                    a && b
                }
            };
            if entered && (!r.book || owner == Owner::Unbooked) {
                break;
            }
            size /= 2;
        }
        w.draw(p, r);
        p = next;
        if (t + 1) % r.every == 0 {
            w.watch(p, r);
        }
    }
    w.end(p, r);
    w
}

const RULES: Rules = Rules { book: true, over: true, every: 1 };

fn unpaid(tick: i64, r: Rules, owner: Owner, paths: u64) -> (u64, i64) {
    let (mut n, mut short) = (0, 0);
    for seed in 1..=paths {
        let w = run(seed * 7_919 + tick as u64, tick, r, owner);
        if w.short > 0 {
            n += 1;
            short += w.short;
        }
    }
    (n, short / paths as i64 / KAS)
}

/// With the rules: no user is left unpaid by any of the three owners, at ticks up to the maintenance rate less the
/// tolerance.
#[test]
fn with_booked_legs_steps_over_statements_and_two_levels_no_user_is_unpaid() {
    for owner in [Owner::Way, Owner::Pair, Owner::Unbooked] {
        for tick in [50, 200, MMR - G] {
            let (n, _) = unpaid(tick, RULES, owner, 1_000);
            println!("{owner:?}, ticks of {tick} bps, the rules: users unpaid on {n} of 1,000 paths");
            assert_eq!(n, 0, "{owner:?}, ticks of {tick} bps");
        }
    }
}

/// Review 42, H1. Where a step does not reach an account that carries a statement, the owner's pair leaves users
/// unpaid, at ticks well within the bound.
#[test]
fn an_account_held_under_statements_breaks_it_where_a_step_does_not_reach_it() {
    let (n, short) = unpaid(200, Rules { over: false, ..RULES }, Owner::Pair, 1_000);
    println!("Pair, ticks of 200 bps, steps that do not reach a held account: users unpaid on {n} of 1,000 paths, {short} KAS a path");
    assert!(n > 100, "{n}");
}

/// Review 42, H2 (and the reserve as built today). Where the shard pays for legs it never booked and is held to
/// its requirement at its draws alone, the owner's unbooked account leaves users unpaid.
#[test]
fn a_leg_paid_that_was_never_booked_breaks_it() {
    let (n, short) = unpaid(200, Rules { book: false, ..RULES }, Owner::Unbooked, 1_000);
    println!("Unbooked, ticks of 200 bps, as built today: users unpaid on {n} of 1,000 paths, {short} KAS a path");
    assert!(n > 500, "{n}");
}

/// The bound is on the move between two visits of whoever watches.
#[test]
fn the_bound_is_on_the_move_between_two_visits() {
    for (tick, every, holds) in [(100, 2, true), (100, 4, true), (MMR - G, 4, false)] {
        let (n, _) = unpaid(tick, Rules { every, ..RULES }, Owner::Way, 1_000);
        println!("Way, ticks of {tick} bps, a visit every {every} ticks: users unpaid on {n} of 1,000 paths");
        assert_eq!(n == 0, holds, "ticks of {tick} bps, every {every}");
    }
}

/// And for a book that is set off it is a smaller bound, which the rate on the gross gives. Nothing gates a
/// leaving: one side of a set-off book leaves, and until the next visit the other side stands against a shard that
/// holds what a set-off book is asked. Two users, 1,000 units each way at 10; the owner draws all it may, and the
/// shard holds the rate on the gross of 20,000 KAS: 100 KAS. The short leaves. The price rises before the visit.
#[test]
fn one_side_of_a_set_off_book_that_leaves_is_held_by_the_rate_on_the_gross_and_the_band() {
    for (tick, short) in [(100, false), (150, false), (200, false), (MMR - G, true)] {
        let mut w = World { coins: 1_000 * KAS, ..World::default() };
        let p = 10 * KAS;
        assert!(w.enter(false, 1_000, p, RULES, true, false) && w.enter(false, -1_000, p, RULES, true, false));
        w.draw(p, RULES);
        assert_eq!(w.coins, 100 * KAS, "the rate on the gross, and nothing on a net of nothing");
        w.leave(1, p, RULES);
        assert!(w.equity(p, RULES) < w.requirement(p, MMR - G), "the book is one way, and the shard below what that is asked");
        w.draw(p, RULES);
        assert_eq!(w.coins, 100 * KAS, "no draw below the requirement");
        let p = p + p / BPS * tick;
        w.watch(p, RULES);
        println!("a set-off book, one side gone, a rise of {tick} bps before the visit: the user is short {} KAS", w.short / KAS);
        assert_eq!(w.short > 0, short, "a rise of {tick} bps");
    }
}
