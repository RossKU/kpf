//! Research, numbers only (docs/ARCHITECTURE-CLEARING.md 12): a shard held to its requirement all the time. No
//! script and no object: one market of futures legs, a clearing party's shard, accounts of users and accounts of
//! the owner's own keys, a price that moves a tick at a time, and whoever watches acting once a tick.
//!
//! The question of reviews 37 to 41: the owner of a shard opens accounts with keys of its own and is owed by its
//! shard. Closing the sure and the cheap ways one at a time left the same at fair odds, because nothing held the
//! shard when it took a leg or as the price moved. Here three things do:
//!   books   the shard knows its net units, its gross units and what its legs were entered at, so that what it has
//!           lost at a price is arithmetic:  equity = held + net * price - basis
//!   gate    a leg enters only where the shard's equity is at least its requirement with the leg
//!   call    a shard whose equity is below its requirement is stepped by anyone: its accounts' legs leave at the
//!           price, each user paid what it is owed, for a fee the shard pays
//! The claim: whatever the owner does with accounts of its own (any size the gate lets in, either way, entering,
//! leaving and drawing when it likes, with the price known to it a tick ahead), every user is paid in full, as long
//! as the price moves in a tick by less than the maintenance rate less the tolerance. And where the three are not
//! there (what is built today), the same owner leaves users unpaid.

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
    /// An account of the owner's own keys.
    own: bool,
    /// Side A's units: above zero long, below zero short. The clearing party has the other side, and no margin in
    /// the account.
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
    /// What users were owed and not paid.
    short: i64,
    /// What the owner's own accounts and its draws took out, less what they put in.
    owner_took: i64,
    fees: i64,
}

impl World {
    fn net(&self) -> i64 {
        -self.accts.iter().map(|a| a.units).sum::<i64>()
    }
    fn gross(&self) -> i64 {
        self.accts.iter().map(|a| a.units.abs()).sum()
    }
    /// The books' equity: what the shard holds and what its side has gained or lost at the price, each account
    /// counted whole (a script sums, and does not know what an account's margin can pay).
    fn equity(&self, p: i64) -> i64 {
        self.held - self.accts.iter().map(|a| a.gain(p)).sum::<i64>()
    }
    fn requirement(&self, p: i64) -> i64 {
        rate(self.net().abs() * p, MMR + G) + rate(self.gross() * p, LIQ)
    }
    /// A leg leaves at the price: its side A is paid what it gained (from the shard, less the band of the
    /// tolerance) or pays what it lost (from its margin, as far as that goes, into the shard).
    fn leave(&mut self, i: usize, p: i64) {
        let a = self.accts.remove(i);
        let gain = a.gain(p);
        if gain >= 0 {
            let asked = (gain - rate(a.worth(p), 2 * G)).max(0);
            let paid = asked.min(self.held);
            self.held -= paid;
            if a.own {
                self.owner_took += paid;
            } else {
                self.short += asked - paid;
            }
        } else {
            let paid = (-gain).min(a.margin);
            self.held += paid;
            if a.own {
                self.owner_took -= paid;
            }
        }
    }
    /// What whoever watches does at the price, once a tick: an account whose side A is below its maintenance is
    /// stepped (for a fee out of its margin); and, with `call`, a shard below its requirement is stepped: its legs
    /// leave, the owner's own first (the worst order for its users), each for a fee the shard pays.
    fn watch(&mut self, p: i64, call: bool) {
        let mut i = 0;
        while i < self.accts.len() {
            let a = self.accts[i];
            if a.margin + a.gain(p) < rate(a.worth(p), MMR - G) {
                let fee = rate(a.worth(p), LIQ).min((a.margin + a.gain(p)).max(0));
                self.accts[i].margin -= fee;
                self.fees += fee;
                self.leave(i, p);
            } else {
                i += 1;
            }
        }
        if call && self.equity(p) < self.requirement(p) {
            let mut order: Vec<usize> = (0..self.accts.len()).collect();
            order.sort_by_key(|i| !self.accts[*i].own);
            let legs: Vec<Acct> = order.iter().map(|i| self.accts[*i]).collect();
            self.accts = legs;
            while !self.accts.is_empty() {
                let fee = rate(self.accts[0].worth(p), LIQ).min(self.held);
                self.held -= fee;
                self.fees += fee;
                self.leave(0, p);
            }
        }
    }
    /// A leg enters at the price with its maintenance as margin; with `gate`, only where the shard's equity is at
    /// least its requirement with it.
    fn enter(&mut self, own: bool, units: i64, p: i64, gate: bool) -> bool {
        let a = Acct { own, units, entry: p, margin: rate(units.abs() * p, MMR + G) };
        self.accts.push(a);
        if gate && self.equity(p) < self.requirement(p) {
            self.accts.pop();
            return false;
        }
        true
    }
    /// The owner draws all the rule gives it: what the shard holds above its requirement (with `books`, of its
    /// equity; without, of what it holds, as today).
    fn draw(&mut self, p: i64, books: bool) {
        let above = if books { self.equity(p) } else { self.held } - self.requirement(p);
        let amount = above.min(self.held).max(0);
        self.held -= amount;
        self.owner_took += amount;
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

/// One run: users enter legs either way; the owner, who knows the next tick, enters and leaves accounts of its own
/// to be owed, and draws all it may; the price moves by up to `tick` bps a tick. Returns the world at the end,
/// every leg left at the last price.
fn run(seed: u64, tick: i64, rules: bool) -> World {
    let mut r = Rng(seed);
    let mut w = World { held: 500 * KAS, ..World::default() };
    let mut p = 10 * KAS;
    for _ in 0..80 {
        let next = p + p / BPS * r.range(-tick, tick);
        // Users come and go.
        if r.range(0, 2) == 0 {
            let units = r.range(1, 40) * if r.range(0, 1) == 0 { 1 } else { -1 };
            w.enter(false, units, p, rules);
        }
        if r.range(0, 5) == 0 && !w.accts.is_empty() {
            let i = r.range(0, w.accts.len() as i64 - 1) as usize;
            if !w.accts[i].own && w.accts[i].margin + w.accts[i].gain(p) >= 0 {
                w.leave(i, p);
            }
        }
        // The owner: an account of its own the way the price goes next, as large as it gets in; out of the ones
        // that would lose; and all the rule lets it draw.
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
            if w.enter(true, if up { size } else { -size }, p, rules) {
                if !rules {
                    break;
                }
            } else {
                size /= 2;
                continue;
            }
            size /= 2;
        }
        w.draw(p, rules);
        p = next;
        w.watch(p, rules);
    }
    // The end: the owner is gone; every leg leaves at the last price, the owner's own first.
    w.accts.sort_by_key(|a| !a.own);
    while !w.accts.is_empty() {
        w.leave(0, p);
    }
    w
}

/// With the books, the gate and the call: no user is left unpaid, whatever the owner does, on a thousand paths
/// whose ticks are within the maintenance rate less the tolerance.
#[test]
fn a_shard_held_to_its_requirement_all_the_time_pays_its_users_whatever_its_owner_does() {
    for tick in [50, 200, MMR - G] {
        let (mut took, mut fees) = (0i64, 0i64);
        for seed in 1..=1_000u64 {
            let w = run(seed * 7_919 + tick as u64, tick, true);
            assert_eq!(w.short, 0, "tick {tick} bps, seed {seed}: users were owed {} sompi and not paid", w.short);
            took += w.owner_took;
            fees += w.fees;
        }
        println!("ticks of {tick} bps: no user unpaid on 1,000 paths; the owner's accounts and draws took {} KAS a path, whoever stepped {} KAS", took / 1_000 / KAS, fees / 1_000 / KAS);
    }
}

/// Without them (the requirement asked at a draw alone, of what the shard holds): the same owner leaves users
/// unpaid on most paths. This is what reviews 37 to 41 found, at fair odds and with no trick of entry or terms.
#[test]
fn a_shard_held_to_its_requirement_at_its_draws_alone_does_not() {
    let (mut unpaid, mut short) = (0, 0i64);
    for seed in 1..=1_000u64 {
        let w = run(seed * 7_919 + 200, 200, false);
        if w.short > 0 {
            unpaid += 1;
            short += w.short;
        }
    }
    println!("ticks of 200 bps, as built today: users unpaid on {unpaid} of 1,000 paths, {} KAS a path", short / 1_000 / KAS);
    assert!(unpaid > 500, "{unpaid}");
}

/// The limit of the claim: where the price moves in a tick by more than the requirement holds, somebody is unpaid
/// with the rules too. That is the gap any margin has, and the rate is what it is sized by.
#[test]
fn a_gap_beyond_the_maintenance_rate_is_not_covered_by_any_rule() {
    let unpaid = (1..=1_000u64).filter(|seed| run(seed * 7_919 + 1_500, 1_500, true).short > 0).count();
    println!("ticks of 1,500 bps, with the rules: users unpaid on {unpaid} of 1,000 paths");
    assert!(unpaid > 0);
}
