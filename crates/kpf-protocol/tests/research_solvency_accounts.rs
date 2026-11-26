//! Research, numbers only (docs/ARCHITECTURE-CLEARING.md 13, after review 44): the third model of a shard held to
//! its requirement all the time, with accounts of more than one leg, in two markets.
//!
//! - A covered account is its user's coins and its legs; a leg's entry stays. Nothing of the clearing side.
//! - **Standing** is of the net: the coins and what all the legs gained or lost, against the maintenance rate of
//!   what all are worth. It decides a step, and it lets a leg in (a gain of one leg is margin for another).
//! - **What leaves as coins** (a take; what a leg that gained is paid where it leaves alone) leaves the coins at
//!   the maintenance and at what the legs that stay have lost: a gain of a leg that stays is no coin.
//! - A leaving is of one leg or of all: of all, the shard is asked the account's net, once, beyond the band. A leg
//!   that lost leaves alone where the coins pay what it lost.
//! - A step: of an account below its maintenance, a leg that lost leaves where the coins pay what it lost, the fee,
//!   and what the legs that stay have lost (a fee is not taken before a loss: review 47); where none does, every
//!   leg leaves.
//! - Stage 4: a leg whose entry is stated and not settled is in the books, and may yet not have entered.
//! - The shard as in the second model: every leg is in the books from its entry, two levels, the rate on the
//!   gross, a call.
//!
//! The claim: whatever the owner does with accounts of its own, of any number of legs, every user is paid in full
//! what it is owed beyond the band, as long as each price moves by less than the maintenance rate less the
//! tolerance between two visits of whoever watches; and no act on an account (an entry, a take, a leaving) makes
//! what the shard is good for fall: only a price does, and a draw, which is held to the upper level. Without the
//! second test (review 44, S2) an act does.

const KAS: i64 = 100_000_000;
const BPS: i64 = 10_000;
const MMR: i64 = 500;
const G: i64 = 50;
const LIQ: i64 = 50;
const GROSS: i64 = (MMR - 3 * G) / 2;
const MARKETS: usize = 2;
type Prices = [i64; MARKETS];

fn rate(w: i64, r: i64) -> i64 {
    (w / BPS) * r + ((w % BPS) * r + BPS - 1) / BPS
}

#[derive(Clone, Copy, Debug)]
struct Leg {
    m: usize,
    /// Side A's units: above zero long, below zero short.
    units: i64,
    entry: i64,
    /// Its entry is stated and not settled: it is in the books, and may yet not have entered (stage 4).
    pending: bool,
}

impl Leg {
    fn worth(&self, p: &Prices) -> i64 {
        self.units.abs() * p[self.m]
    }
    fn gain(&self, p: &Prices) -> i64 {
        self.units * (p[self.m] - self.entry)
    }
}

#[derive(Clone, Debug)]
struct Acct {
    own: bool,
    coins: i64,
    legs: Vec<Leg>,
}

impl Acct {
    fn worth(&self, p: &Prices) -> i64 {
        self.legs.iter().map(|l| l.worth(p)).sum()
    }
    fn gain(&self, p: &Prices) -> i64 {
        self.legs.iter().map(|l| l.gain(p)).sum()
    }
    /// What the legs but one have lost, each counted alone (a gain sets off nothing).
    fn lost_but(&self, p: &Prices, but: Option<usize>) -> i64 {
        self.legs.iter().enumerate().filter(|(i, _)| Some(*i) != but).map(|(_, l)| (-l.gain(p)).max(0)).sum()
    }
    fn below(&self, p: &Prices) -> bool {
        self.coins + self.gain(p) < rate(self.worth(p), MMR - G)
    }
}

#[derive(Clone, Copy, Debug)]
struct Rules {
    /// The second test (review 44, S2). Without: one test, of the net, for all.
    two: bool,
    /// Whoever watches comes every so many ticks.
    every: usize,
    /// A draw is ended by an entry that is not settled (stage 4): nothing is drawn while one is pending.
    guard: bool,
    /// For how many ticks an entry can be kept from settling or ending (by whoever stated it).
    life: usize,
}

#[derive(Clone, Debug, Default)]
struct World {
    coins: i64,
    accts: Vec<Acct>,
    /// Notes of what is owed, not yet paid: (to an account of the owner's own, the amount).
    owed: Vec<(bool, i64)>,
    short: i64,
    owner_took: i64,
    fees: i64,
    /// What users were refused (a leg that could not leave alone): counted, to see what the second test costs.
    refused: i64,
    /// How often the shard was called, and how often a leg of the owner's that gained left alone.
    calls: i64,
    alone: i64,
    /// By how much acts on accounts (an entry, a take, a leaving) made what the shard is good for fall.
    fell: i64,
    /// By how much draws left the shard below its upper level, counted without the legs whose entry was not
    /// settled when it drew.
    overdrawn: i64,
}

impl World {
    fn legs(&self) -> impl Iterator<Item = &Leg> {
        self.accts.iter().flat_map(|a| a.legs.iter())
    }
    fn equity(&self, p: &Prices) -> i64 {
        self.coins - self.owed.iter().map(|o| o.1).sum::<i64>() - self.legs().map(|l| l.gain(p)).sum::<i64>()
    }
    /// What the shard is good for: its coins, less what it owes by notes and what its accounts have gained, and
    /// what the accounts that lost can pay it: each no more than its coins. (The books' equity counts what an
    /// account lost whole.) No act on an account should make it fall: a price does that, and a draw.
    fn good_for(&self, p: &Prices) -> i64 {
        self.coins - self.owed.iter().map(|o| o.1).sum::<i64>() - self.accts.iter().map(|a| a.gain(p).max(-a.coins)).sum::<i64>()
    }
    fn requirement(&self, p: &Prices, up: bool) -> i64 {
        let (net, gross) = if up { (MMR + G, GROSS + G) } else { (MMR - G, GROSS) };
        (0..MARKETS)
            .map(|m| {
                let n: i64 = self.legs().filter(|l| l.m == m).map(|l| l.units).sum();
                let g: i64 = self.legs().filter(|l| l.m == m).map(|l| l.units.abs()).sum();
                rate(n.abs() * p[m], net) + rate(g * p[m], gross)
            })
            .sum()
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
    fn open(&mut self, own: bool) -> usize {
        self.accts.push(Acct { own, coins: 0, legs: vec![] });
        self.accts.len() - 1
    }
    /// A leg enters an account at the price, with coins added. The account stands at its maintenance with it (of
    /// the net: what its other legs gained is margin for it), and the shard's equity is at least its upper level
    /// with it: else it does not enter, and the coins are not added.
    fn enter(&mut self, a: usize, m: usize, units: i64, add: i64, p: &Prices) -> bool {
        self.accts[a].legs.push(Leg { m, units, entry: p[m], pending: false });
        self.accts[a].coins += add;
        let acct = &self.accts[a];
        if acct.coins + acct.gain(p) < rate(acct.worth(p), MMR + G) || self.equity(p) < self.requirement(p, true) {
            self.accts[a].legs.pop();
            self.accts[a].coins -= add;
            return false;
        }
        true
    }
    /// The most a user may take out of its coins.
    fn take_max(&self, a: usize, p: &Prices, r: Rules) -> i64 {
        let acct = &self.accts[a];
        let keep = if r.two { rate(acct.worth(p), MMR + G) + acct.lost_but(p, None) } else { rate(acct.worth(p), MMR + G) - acct.gain(p) };
        (acct.coins - keep.max(0)).max(0)
    }
    /// Every leg of an account leaves at the prices: the shard is asked the net, once, beyond the band; or is paid
    /// what the account lost, as far as its coins go.
    fn leave_all(&mut self, a: usize, fee: i64, p: &Prices) {
        let mut acct = self.accts.remove(a);
        let net = acct.gain(p);
        let fee = fee.min((acct.coins + net).max(0));
        acct.coins -= fee;
        self.fees += fee;
        if net >= 0 {
            let asked = net - rate(acct.worth(p), 2 * G);
            if asked > 0 {
                self.owed.push((acct.own, asked));
            }
        } else {
            let paid = (-net).min(acct.coins.max(0));
            self.coins += paid;
            if acct.own {
                self.owner_took -= paid;
            }
        }
    }
    /// One leg leaves alone. One that lost: where the coins pay what it lost (without the second test: as far as
    /// they go). One that gained: where the coins that stay are at the maintenance and at what the legs that stay
    /// have lost (without the second test: where the account stands at its maintenance, of the net).
    fn leave_one(&mut self, a: usize, i: usize, fee: i64, p: &Prices, r: Rules) -> bool {
        if self.accts[a].legs.len() == 1 {
            self.leave_all(a, fee, p);
            return true;
        }
        let acct = &self.accts[a];
        let l = acct.legs[i];
        let gain = l.gain(p);
        if gain < 0 {
            if r.two && acct.coins < fee - gain {
                return false;
            }
            let paid = (-gain).min(acct.coins);
            let own = acct.own;
            self.accts[a].coins -= paid;
            self.coins += paid;
            if own {
                self.owner_took -= paid;
            }
            let fee = fee.min(self.accts[a].coins);
            self.accts[a].coins -= fee;
            self.fees += fee;
        } else {
            let ok = if r.two { acct.coins >= rate(acct.worth(p) - l.worth(p), MMR + G) + acct.lost_but(p, Some(i)) } else { acct.coins + acct.gain(p) >= rate(acct.worth(p), MMR) };
            if !ok {
                return false;
            }
            let asked = gain - rate(l.worth(p), 2 * G);
            if asked > 0 {
                self.owed.push((acct.own, asked));
            }
        }
        self.accts[a].legs.remove(i);
        true
    }
    fn draw(&mut self, p: &Prices, r: Rules) {
        self.pay();
        if !self.owed.is_empty() || (r.guard && self.legs().any(|l| l.pending)) {
            return;
        }
        let amount = (self.equity(p) - self.requirement(p, true)).min(self.coins).max(0);
        self.coins -= amount;
        self.owner_took += amount;
        if amount > 0 && self.legs().any(|l| l.pending) {
            // What the books would say without the legs that may yet not have entered.
            let mut settled = self.clone();
            for a in settled.accts.iter_mut() {
                a.legs.retain(|l| !l.pending);
            }
            self.overdrawn += (settled.requirement(p, true) - settled.equity(p)).max(0).min(amount);
        }
    }
    /// An entry that was stated is ended: the leg did not enter. Nothing is paid either way.
    fn unenter(&mut self, a: usize) {
        self.accts[a].legs.retain(|l| !l.pending);
        if self.accts[a].legs.is_empty() {
            self.accts.remove(a);
        }
    }
    /// A visit of whoever watches: every account below its maintenance is stepped (a leg that lost leaves where
    /// the coins pay what it lost and the fee, the one worth most first; where none does, every leg leaves); the
    /// notes are paid; a shard below its lower level is called: every account leaves, and every note is paid the
    /// same share.
    fn watch(&mut self, p: &Prices, r: Rules) {
        let mut a = 0;
        while a < self.accts.len() {
            // (An account whose entry is not settled carries that statement: it is not stepped.)
            if !self.accts[a].below(p) || self.accts[a].legs.iter().any(|l| l.pending) {
                a += 1;
                continue;
            }
            let acct = &self.accts[a];
            let one = (0..acct.legs.len())
                .filter(|i| acct.legs.len() > 1 && acct.legs[*i].gain(p) < 0 && acct.coins >= rate(acct.legs[*i].worth(p), LIQ) - acct.legs[*i].gain(p) + acct.lost_but(p, Some(*i)))
                .max_by_key(|i| acct.legs[*i].worth(p));
            match one {
                Some(i) if r.two => {
                    let fee = rate(acct.legs[i].worth(p), LIQ);
                    assert!(self.leave_one(a, i, fee, p, r));
                }
                _ => {
                    let fee = rate(acct.worth(p), LIQ);
                    self.leave_all(a, fee, p);
                }
            }
        }
        self.pay();
        if self.equity(p) < self.requirement(p, false) {
            self.calls += 1;
            let cap = rate((0..MARKETS).map(|m| self.legs().filter(|l| l.m == m).map(|l| l.units.abs()).sum::<i64>() * p[m]).sum(), LIQ);
            while !self.accts.is_empty() {
                // (A leg that has not entered leaves nothing: its entry is ended.)
                if self.accts[0].legs.iter().any(|l| l.pending) {
                    self.unenter(0);
                } else {
                    self.leave_all(0, 0, p);
                }
            }
            let total: i64 = self.owed.iter().map(|o| o.1).sum();
            if total <= self.coins {
                self.pay();
                let fee = cap.min(self.coins / 2);
                self.coins -= fee;
                self.fees += fee;
            } else {
                let coins = self.coins;
                for o in std::mem::take(&mut self.owed) {
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
    fn end(&mut self, p: &Prices) {
        while !self.accts.is_empty() {
            if self.accts[0].legs.iter().any(|l| l.pending) {
                self.unenter(0);
            } else {
                self.leave_all(0, 0, p);
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
    /// Nothing of its own but its draws.
    Draws,
    /// An account of one leg the way the first market goes next, as large as it gets in; out before it would lose.
    Way,
    /// An account against its users' net in the first market; out before a tick that goes its users' way.
    SetOff,
    /// Review 44, S2: an account with as many units each way in one market, at its floor. Its standing is its
    /// coins and the books show a net of nothing. Once the price has drifted, the leg that gained leaves alone;
    /// the one that lost stays, with coins that are less than it lost.
    Straddle,
    /// The same across the two markets, and it knows the next tick: long in the one that rises more. Then the leg
    /// that gained leaves alone, and it takes what coins it may.
    Spread,
    /// An account whose first leg gained; on that gain alone, a second as large as it gets in, the way its market
    /// goes next; then it takes what coins it may and has the leg that gained leave alone.
    Feed,
    /// Stage 4. It states the entry of a leg of its own against its users' net and keeps it from settling (it
    /// disputes it, a bond a period): the book reads set off, and it draws what a set-off book is asked. Then it
    /// has the entry ended and states another. The leg never enters; the book was one way all the time.
    Phantom,
}

fn user(g: &mut Rng, w: &World) -> Option<usize> {
    let users: Vec<usize> = (0..w.accts.len()).filter(|a| !w.accts[*a].own).collect();
    (!users.is_empty()).then(|| users[g.range(0, users.len() as i64 - 1) as usize])
}

fn own_enter(w: &mut World, legs: &[(usize, i64)], p: &Prices) -> bool {
    let a = w.open(true);
    for (m, units) in legs {
        let add = rate(units.abs() * p[*m], MMR + G);
        if !w.enter(a, *m, *units, add, p) {
            w.accts.remove(a);
            return false;
        }
    }
    true
}

fn run(seed: u64, tick: i64, r: Rules, owner: Owner) -> World {
    let mut g = Rng(seed);
    let mut w = World { coins: 500 * KAS, ..World::default() };
    let mut p: Prices = [10 * KAS, 25 * KAS];
    const T: usize = 80;
    let mut born = 0usize;
    for t in 0..T {
        let next: Prices = [p[0] + p[0] / BPS * g.range(-tick, tick), p[1] + p[1] / BPS * g.range(-tick, tick)];
        let before = w.good_for(&p);
        // Users: accounts of one to three legs; a leg more on what the others gained; a leg out, or all; coins out.
        match g.range(0, 8) {
            0 | 1 => {
                let a = w.open(false);
                for _ in 0..g.range(1, 3) {
                    let (m, units) = (g.range(0, 1) as usize, g.range(1, 40) * if g.range(0, 1) == 0 { 1 } else { -1 });
                    let add = rate(units.abs() * p[m], MMR + G) + g.range(0, 1) * rate(units.abs() * p[m], 300);
                    w.enter(a, m, units, add, &p);
                }
                if w.accts[a].legs.is_empty() {
                    w.accts.remove(a);
                }
            }
            2 => {
                if let Some(a) = user(&mut g, &w) {
                    if w.accts[a].legs.len() < 6 {
                        let (m, units) = (g.range(0, 1) as usize, g.range(1, 40) * if g.range(0, 1) == 0 { 1 } else { -1 });
                        w.enter(a, m, units, 0, &p);
                    }
                }
            }
            3 | 4 => {
                if let Some(a) = user(&mut g, &w) {
                    let i = g.range(0, w.accts[a].legs.len() as i64 - 1) as usize;
                    if !w.leave_one(a, i, 0, &p, r) {
                        w.refused += 1;
                        w.leave_all(a, 0, &p);
                    }
                }
            }
            5 => {
                if let Some(a) = user(&mut g, &w) {
                    w.leave_all(a, 0, &p);
                }
            }
            6 => {
                if let Some(a) = user(&mut g, &w) {
                    w.accts[a].coins -= w.take_max(a, &p, r);
                }
            }
            _ => {}
        }
        // The owner, who knows the next tick.
        let up = [next[0] > p[0], next[1] > p[1]];
        let last = t + 1 == T;
        match owner {
            Owner::Draws => {}
            Owner::Phantom => {
                // It states the entry of a leg of its own against its users' net, so that the book reads set off,
                // and draws what a set-off book is asked. It keeps the entry from settling as long as it can; then
                // the entry is ended, and it states another.
                let pending = w.accts.iter().any(|a| a.own);
                if !pending || t - born >= r.life {
                    let mut a = 0;
                    while a < w.accts.len() {
                        if w.accts[a].own {
                            w.unenter(a);
                        } else {
                            a += 1;
                        }
                    }
                    w.draw(&p, r);
                    let against = -w.legs().filter(|l| l.m == 0).map(|l| l.units).sum::<i64>();
                    if against != 0 && own_enter(&mut w, &[(0, against)], &p) {
                        let a = w.accts.len() - 1;
                        w.accts[a].legs[0].pending = true;
                        born = t;
                    }
                }
                w.draw(&p, r);
            }
            Owner::Way | Owner::SetOff => {
                let mut a = 0;
                while a < w.accts.len() {
                    if w.accts[a].own && (w.accts[a].legs[0].units > 0) != up[0] {
                        w.leave_all(a, 0, &p);
                    } else {
                        a += 1;
                    }
                }
                if owner == Owner::SetOff {
                    let against = -w.legs().filter(|l| l.m == 0).map(|l| l.units).sum::<i64>();
                    if against != 0 && (against > 0) == up[0] {
                        own_enter(&mut w, &[(0, against)], &p);
                    }
                } else {
                    let mut size = 4_000;
                    while size >= 1 {
                        own_enter(&mut w, &[(0, if up[0] { size } else { -size })], &p);
                        size /= 2;
                    }
                }
            }
            Owner::Straddle | Owner::Spread | Owner::Feed => {
                // The leg that gained leaves alone once it gained more than the account's coins (the coins are
                // then less than the other lost); and the owner takes what coins it may.
                for a in 0..w.accts.len() {
                    if !w.accts[a].own || w.accts[a].legs.len() < 2 {
                        continue;
                    }
                    // (It keeps the account at its floor, of the net, as the worth and the net move: coins of its own.)
                    if owner == Owner::Straddle {
                        let floor = rate(w.accts[a].worth(&p), MMR + G) - w.accts[a].gain(&p);
                        w.accts[a].coins = w.accts[a].coins.max(floor);
                    }
                    let i = (0..w.accts[a].legs.len()).max_by_key(|i| w.accts[a].legs[*i].gain(&p)).unwrap();
                    let gain = w.accts[a].legs[i].gain(&p);
                    if (gain > w.accts[a].coins * 3 / 2 || (last && gain > w.accts[a].coins)) && w.leave_one(a, i, 0, &p, r) {
                        w.alone += 1;
                    }
                    if owner != Owner::Straddle {
                        let x = w.take_max(a, &p, r);
                        w.accts[a].coins -= x;
                    }
                }
                if owner == Owner::Feed {
                    // On what a first leg gained, a second as large as it gets in, the way its market goes next.
                    for a in 0..w.accts.len() {
                        if w.accts[a].own && w.accts[a].legs.len() == 1 && w.accts[a].gain(&p) > 0 {
                            let m = 1 - w.accts[a].legs[0].m;
                            let mut size = 4_000;
                            while size >= 1 && !w.enter(a, m, if up[m] { size } else { -size }, 0, &p) {
                                size /= 2;
                            }
                        }
                    }
                }
                if !w.accts.iter().any(|a| a.own && a.legs.len() >= 2) {
                    let mut size = 2_000;
                    while size >= 1 {
                        let legs: Vec<(usize, i64)> = match owner {
                            Owner::Straddle => vec![(0, size), (0, -size)],
                            // (Long the market that goes up next, short the other, of the same worth.)
                            Owner::Spread => {
                                let (l, s) = if up[0] { (0, 1) } else { (1, 0) };
                                vec![(l, size * p[s] / p[l].max(1) + 1), (s, -size)]
                            }
                            _ => vec![(0, if up[0] { size } else { -size })],
                        };
                        if own_enter(&mut w, &legs, &p) {
                            break;
                        }
                        size /= 2;
                    }
                }
            }
        }
        w.fell += (before - w.good_for(&p)).max(0);
        // (The owner of a straddle or a spread does not draw: a shard at its level is called at the first gain of
        // a user, every account leaves, and its legs with them before the price has drifted.)
        if !matches!(owner, Owner::Straddle | Owner::Spread | Owner::Phantom) {
            w.draw(&p, r);
        }
        p = next;
        if (t + 1) % r.every == 0 {
            w.watch(&p, r);
        }
    }
    w.end(&p);
    w
}

const RULES: Rules = Rules { two: true, every: 1, guard: true, life: 1 };

/// Stage 4, in numbers. A leg of the owner's own that is in the books and never enters, against its users' net:
/// the book reads set off. Where it draws on that, the shard is left below its upper level counted without the
/// leg (held by: an entry that is not settled ends a draw). Where it keeps the entry from settling for long, no
/// call is due while its users gain more than the shard holds, whether it drew or not (held by: an entry settles
/// or ends within a few dispute periods, which whoever watches sees to).
#[test]
fn a_leg_that_is_in_the_books_and_never_enters() {
    for (guard, life, tick, holds) in [(false, 1, 200, false), (true, 1, 200, true), (true, 1, MMR - G, true), (true, 80, 200, false), (false, 80, 200, false)] {
        let (mut n, mut over, mut short) = (0, 0, 0);
        for seed in 1..=1_000u64 {
            let w = run(seed * 7_919 + tick as u64, tick, Rules { guard, life, ..RULES }, Owner::Phantom);
            over += w.overdrawn;
            short += w.short;
            n += (w.short > 0) as u64;
        }
        println!("a leg that never enters, kept {life} ticks, ticks of {tick} bps, draws ended by it: {guard}: drawn below the upper level by {} KAS a path; users unpaid on {n} of 1,000 paths, {} KAS a path", over / 1_000 / KAS, short / 1_000 / KAS);
        assert_eq!(over == 0 && n == 0, holds, "guard {guard}, life {life}");
    }
}

fn unpaid(tick: i64, r: Rules, owner: Owner, paths: u64) -> (u64, i64, i64, i64) {
    let (mut n, mut short, mut refused) = (0, 0, 0);
    let (mut calls, mut alone, mut fell) = (0, 0, 0);
    for seed in 1..=paths {
        let w = run(seed * 7_919 + tick as u64, tick, r, owner);
        refused += w.refused;
        calls += w.calls;
        alone += w.alone;
        fell += w.fell;
        if w.short > 0 {
            n += 1;
            short += w.short;
        }
    }
    println!("    ({owner:?}, {tick} bps, two tests {}: {calls} calls, {alone} legs of the owner's that gained left alone)", r.two);
    (n, short / paths as i64 / KAS, refused, fell / paths as i64 / KAS)
}

/// With the two tests: no user is left unpaid by any of the owners, at ticks up to the maintenance rate less the
/// tolerance, with accounts of up to six legs in two markets.
#[test]
fn with_two_tests_no_user_is_unpaid_whatever_the_owner_does_with_accounts_of_several_legs() {
    for owner in [Owner::Draws, Owner::Way, Owner::SetOff, Owner::Straddle, Owner::Spread, Owner::Feed] {
        for tick in [50, 200, MMR - G] {
            let (n, _, refused, fell) = unpaid(tick, RULES, owner, 1_000);
            println!("{owner:?}, ticks of {tick} bps, the two tests: users unpaid on {n} of 1,000 paths; no act made what the shard is good for fall (a leg of a user's refused to leave alone {refused} times)");
            assert_eq!((n, fell), (0, 0), "{owner:?}, ticks of {tick} bps");
        }
    }
}

/// Review 44, S2. With one test, of the net, a leg that gained leaves alone and is paid, and the one that lost
/// stays with coins that are less than it lost: what the shard is good for falls by an act, at no price's move
/// (its owner has drawn what no draw would give it). Whether a user is then unpaid is a matter of how large the
/// users' net is against what is left: the case below has one who is.
#[test]
fn with_one_test_a_leg_that_gained_leaves_alone_and_the_shard_is_good_for_less() {
    // (The spread's owner, who knows the next tick, gains on the net: its account is never short of what a leg
    // lost, and the act takes nothing. The straddle is the case.)
    for owner in [Owner::Straddle] {
        let (n, short, _, fell) = unpaid(200, Rules { two: false, ..RULES }, owner, 1_000);
        println!("{owner:?}, ticks of 200 bps, one test: what the shard is good for fell by {fell} KAS a path, by acts and no price (it holds 500); users unpaid on {n} of 1,000 paths, {short} KAS a path");
        assert!(fell > 10, "{owner:?}: {fell}");
    }
}

/// The reviewer's case of S2, in one account. 1,000 units each way at 10, the coins at the floor. The price drifts
/// up 34 % by steps of 2 %: the account's standing never moves, and nothing is due. A user enters long 300 units.
/// With one test the owner's long leaves alone and is paid what the shard holds, and the user's gain of the next
/// tick is not paid; with two it does not leave alone, and leaving with the other it asks nothing.
#[test]
fn the_case_of_review_44_in_one_account() {
    for two in [false, true] {
        let r = Rules { two, every: 1, guard: true, life: 1 };
        let mut w = World { coins: 1_000 * KAS, ..World::default() };
        let mut p: Prices = [10 * KAS, 25 * KAS];
        assert!(own_enter(&mut w, &[(0, 1_000), (0, -1_000)], &p));
        for _ in 0..15 {
            p[0] += p[0] / BPS * 200;
            // (The owner keeps its account at its floor as what the legs are worth grows: coins of its own.)
            w.accts[0].coins = rate(w.accts[0].worth(&p), MMR + G);
            w.watch(&p, r);
            assert_eq!(w.accts.len(), 1, "nothing is due: the account stands on its net, and the books show a net of nothing");
        }
        // A user enters long; the owner's long leaves alone; the price rises 2 % more.
        let u = w.open(false);
        assert!(w.enter(u, 0, 300, rate(300 * p[0], MMR + G), &p));
        let left = w.leave_one(0, 0, 0, &p, r);
        assert_eq!(left, !two, "the leg that gained leaves alone only with one test");
        w.pay();
        p[0] += p[0] / BPS * 200;
        w.watch(&p, r);
        w.end(&p);
        println!("review 44's case, {}: the user is short {} KAS; a leg that gained left alone: {left}", if two { "two tests" } else { "one test" }, w.short / KAS);
        assert_eq!(w.short > 0, !two);
    }
}

/// The same in one case. A user is long 1,000 units from 10; the owner has drawn to the upper level (775 KAS). The
/// price rises a tick at a time for five ticks. With nothing else in the books a call is due at the first tick,
/// and the user is paid. With a leg of the owner's own in the books that never enters, short 1,000, the books read
/// set off and flat for as long as its entry is kept from settling: no call is due, and at the end the user is
/// short; unless the price moves no more than the bound over that whole time (five ticks of 0.9 %).
#[test]
fn a_leg_that_never_enters_hides_a_call_for_as_long_as_it_is_kept_from_settling() {
    for (life, tick, short) in [(0usize, 200, false), (1, 200, false), (5, 200, true), (5, (MMR - G) / 5, false)] {
        let r = Rules { life, ..RULES };
        let mut w = World { coins: 5_000 * KAS, ..World::default() };
        let mut p: Prices = [10 * KAS, 25 * KAS];
        let u = w.open(false);
        assert!(w.enter(u, 0, 1_000, rate(1_000 * p[0], MMR + G) + 2_000 * KAS, &p));
        w.draw(&p, r);
        assert_eq!(w.coins, rate(10_000 * KAS, MMR + G) + rate(10_000 * KAS, GROSS + G));
        for t in 0..5 {
            // The owner's leg: stated where there is none, ended once it was kept as long as it can be.
            if life > 0 {
                if let Some(a) = w.accts.iter().position(|a| a.own) {
                    if t % life == 0 {
                        w.unenter(a);
                    }
                }
                if !w.accts.is_empty() && !w.accts.iter().any(|a| a.own) && own_enter(&mut w, &[(0, -1_000)], &p) {
                    let a = w.accts.len() - 1;
                    w.accts[a].legs[0].pending = true;
                }
            }
            p[0] += p[0] / BPS * tick;
            w.watch(&p, r);
        }
        let calls = w.calls;
        w.end(&p);
        println!("a leg that never enters, kept {life} ticks, a rise of {tick} bps a tick for five: {calls} calls; the user is short {} KAS", w.short / KAS);
        assert_eq!(w.short > 0, short, "life {life}, ticks of {tick} bps");
    }
}
