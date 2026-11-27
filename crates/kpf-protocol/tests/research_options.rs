//! Research, numbers only (docs/ARCHITECTURE-CLEARING.md 17): options in a covered account. No
//! script and no object. What is asked of an account is what it can lose by a move of the rate, legs of one
//! underlying counted together (17.4), and the premiums, which lie in the account as coins (17.3).
//!
//! The claims tried here:
//!   two sums   for the legs of one underlying at a price a, with L and U the rate below and above it: R, the
//!              larger of nothing, what the legs lose from a to L and from a to U, is never less than what they
//!              lose at any price from L to U. A held option whose strike is between the ends is counted along
//!              one of its two lines, as the statement says: as worth nothing exercised over the whole move, or
//!              as exercised over the whole move. Both lie below it
//!   a visit    an account that is not due for a step at a price has, after any move of the lower rate, coins and
//!              gains of at least its premiums: the shard is paid what the legs lost and the premiums, whole
//!   the books  a shard takes an option at what it is worth exercised at its entry: no premium moves its equity
//!   a day      within a time T of its day an option gives no credit: it is counted only where it adds to what is
//!              asked, so a hedge stops counting while both legs still stand (N68)
//!   what stays a leg that leaves alone leaves an account that is asked again, as it stays (N65)
//!   a call     (17.9, of the form that is built) a leg of any size entered and called is a claim made of nothing
//!              where it says the time it entered and a leg younger than T asks nothing; a band under a call
//!              takes back the tolerance's two edges and not what the price moved

const BPS: i64 = 10_000;
/// The rate a take and an entry are held to, the rate a step is due below, and the tolerance of a stated price.
const UP: i64 = 550;
const LO: i64 = 450;
const G: i64 = 50;

fn rate_up(w: i64, r: i64) -> i64 {
    (w / BPS) * r + ((w % BPS) * r + BPS - 1) / BPS
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Long,
    Short,
    CallHeld,
    CallWritten,
    PutHeld,
    PutWritten,
}
use Kind::*;

#[derive(Clone, Copy, Debug)]
struct Leg {
    kind: Kind,
    units: i64,
    /// An option's strike; nothing for a futures leg.
    strike: i64,
    /// The market's price where the leg entered.
    entry: i64,
    /// An option's premium a unit.
    premium: i64,
    /// An option within the time T of its day.
    near: bool,
}

impl Leg {
    fn held(&self) -> bool {
        matches!(self.kind, CallHeld | PutHeld)
    }
    fn option(&self) -> bool {
        !matches!(self.kind, Long | Short)
    }
    /// What the user's side of the leg is worth at a price, by its units: a futures leg its price one way or the
    /// other; an option what it is worth exercised, for or against the user.
    fn value(&self, p: i64) -> i64 {
        let ex = match self.kind {
            Long => p,
            Short => -p,
            CallHeld => (p - self.strike).max(0),
            CallWritten => -(p - self.strike).max(0),
            PutHeld => (self.strike - p).max(0),
            PutWritten => -(self.strike - p).max(0),
        };
        ex * self.units
    }
    /// What the leg gained from one price to another.
    fn moved(&self, from: i64, to: i64) -> i64 {
        self.value(to) - self.value(from)
    }
    fn gained(&self, p: i64) -> i64 {
        self.moved(self.entry, p)
    }
    fn premium_all(&self) -> i64 {
        if self.option() { self.premium * self.units } else { 0 }
    }
}

/// The two ends of a move of the rate from a price.
fn ends(a: i64, r: i64) -> (i64, i64) {
    let d = rate_up(a, r);
    ((a - d).max(0), a + d)
}

impl Leg {
    /// A held option's two lines, from the price `a` to a price `p`: worth nothing exercised wherever the price
    /// goes, and exercised wherever it goes.
    fn lines(&self, a: i64, p: i64) -> [i64; 2] {
        let now = self.value(a);
        let exercised = if self.kind == CallHeld { p - self.strike } else { self.strike - p } * self.units;
        [-now, exercised - now]
    }
}

/// 17.4: what the legs of one underlying are asked at the price `a` and the rate `r`, with each held option whose
/// strike is between the ends counted along the line `which` says (a bit a leg).
fn asked_as(legs: &[Leg], a: i64, r: i64, which: u32) -> i64 {
    let (l, u) = ends(a, r);
    let (mut down, mut up) = (0i64, 0i64);
    for (i, x) in legs.iter().enumerate() {
        let (dl, du) = if x.held() && x.strike > l && x.strike < u {
            let line = (which >> i & 1) as usize;
            (x.lines(a, l)[line], x.lines(a, u)[line])
        } else {
            (x.moved(a, l), x.moved(a, u))
        };
        // (Within T of its day: only where it adds to what is asked.)
        let (dl, du) = if x.option() && x.near { (dl.min(0), du.min(0)) } else { (dl, du) };
        down += dl;
        up += du;
    }
    0.max(-down).max(-up)
}

/// The same, with every held option counted along its line of nothing (the first writing of the rule).
fn asked_flat(legs: &[Leg], a: i64, r: i64) -> i64 {
    asked_as(legs, a, r, 0)
}

/// The least a statement can say: the lines chosen as its stater likes.
fn asked_group(legs: &[Leg], a: i64, r: i64) -> i64 {
    (0..1u32 << legs.len()).map(|w| asked_as(legs, a, r, w)).min().unwrap()
}

/// What the legs lose at the worst price from L to U: tried at the ends, at every strike between and a price
/// either side of each.
fn worst(legs: &[Leg], a: i64, r: i64) -> i64 {
    let (l, u) = ends(a, r);
    let mut at = vec![l, u, a];
    for x in legs.iter().filter(|x| x.option()) {
        for p in [x.strike - 1, x.strike, x.strike + 1] {
            if p >= l && p <= u {
                at.push(p);
            }
        }
    }
    at.iter().map(|p| -legs.iter().map(|x| x.moved(a, *p)).sum::<i64>()).max().unwrap().max(0)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: i64) -> i64 {
        (self.next() % n as u64) as i64
    }
    fn leg(&mut self, a: i64) -> Leg {
        let kind = [Long, Short, CallHeld, CallWritten, PutHeld, PutWritten][self.below(6) as usize];
        // (Strikes from far below to far above the price, and many close to it.)
        let strike = if self.below(2) == 0 { a - a / 10 + self.below(a / 5 + 1) } else { 1 + self.below(2 * a) };
        Leg { kind, units: 1 + self.below(50), strike: strike.max(1), entry: a - a / 20 + self.below(a / 10 + 1), premium: self.below(a / 10 + 1), near: self.below(4) == 0 }
    }
}

#[test]
fn what_a_group_is_asked_is_never_less_than_a_move_of_the_rate_loses() {
    let mut rng = Rng(0x17_0004);
    let (mut exact, mut over, mut flat, mut n) = (0, 0i64, 0i64, 0);
    for _ in 0..200_000 {
        let a = 1_000 + rng.below(1_000_000);
        let legs: Vec<Leg> = (0..1 + rng.below(6)).map(|_| rng.leg(a)).collect();
        for r in [UP, LO] {
            let (asked, lost) = (asked_group(&legs, a, r), worst(&legs, a, r));
            // (Whatever lines a statement says, it asks no less than can be lost: the least of them does not.)
            assert!(asked >= lost, "{legs:?} at {a}: asked {asked}, loses {lost}");
            flat += asked_flat(&legs, a, r) - asked;
            // (Exact where no held option has its strike between the ends and none is near its day.)
            let (l, u) = ends(a, r);
            if legs.iter().all(|x| !x.option() || (!x.near && (!x.held() || x.strike <= l || x.strike >= u))) {
                assert_eq!(asked, lost, "{legs:?} at {a}");
                exact += 1;
            } else {
                over += asked - lost;
            }
            n += 1;
        }
    }
    println!("{n} groups: {exact} asked exactly what they can lose; the others {over} more in all, and {flat} more again with every held option counted as worth nothing");
}

/// 17.4's table: ten units at the price 100 (in hundredths), what each account is asked at the entry's rate.
#[test]
fn the_accounts_options_are_held_as_are_asked_their_net_risk() {
    let a = 10_000;
    let fut = |kind| Leg { kind, units: 10, strike: 0, entry: a, premium: 0, near: false };
    let opt = |kind, strike, premium| Leg { kind, units: 10, strike, entry: a, premium, near: false };
    let asked = |legs: &[Leg]| (asked_group(legs, a, UP), legs.iter().map(|x| x.premium_all()).sum::<i64>(), worst(legs, a, UP));
    // (What all are worth by the rate, as today: every leg's underlying.)
    let today = |legs: &[Leg]| legs.iter().map(|x| if x.held() { x.premium_all() } else { rate_up(a * x.units, UP) }).sum::<i64>();

    let call = [opt(CallHeld, 10_000, 500)];
    assert_eq!((asked(&call), today(&call)), ((0, 5_000, 0), 5_000));
    let covered = [fut(Long), opt(CallWritten, 11_000, 200)];
    assert_eq!((asked(&covered), today(&covered)), ((5_500, 2_000, 5_500), 11_000));
    let put = [fut(Short), opt(PutWritten, 9_000, 200)];
    assert_eq!((asked(&put), today(&put)), ((5_500, 2_000, 5_500), 11_000));
    let straddle = [opt(CallHeld, 10_000, 500), opt(PutHeld, 10_000, 400)];
    assert_eq!((asked(&straddle), today(&straddle)), ((0, 9_000, 0), 9_000));
    let spread = [opt(CallHeld, 10_000, 500), opt(CallWritten, 11_000, 200)];
    assert_eq!((asked(&spread), today(&spread)), ((0, 7_000, 0), 10_500));
    let flat = [fut(Long), fut(Short)];
    assert_eq!((asked(&flat), today(&flat)), ((0, 0, 0), 11_000));

    // A spread deep in the money can lose nothing above its written strike, and is asked nothing there: at 120
    // the rate on what both are worth was 5.5 % of 200 and of 1,200.
    let deep = [Leg { entry: 10_000, ..opt(CallHeld, 10_000, 500) }, Leg { entry: 10_000, ..opt(CallWritten, 11_000, 200) }];
    assert_eq!((asked_group(&deep, 12_000, UP), worst(&deep, 12_000, UP)), (0, 0));
    // A call written alone is asked the whole move, as a short is.
    assert_eq!(asked_group(&[opt(CallWritten, 10_000, 200)], a, UP), 5_500);
    // A put bought against a long takes the long's fall away below its strike: counted as exercised over the
    // whole move, it is asked what the long loses down to the strike. Counted as worth nothing, the whole move.
    for (strike, least) in [(9_500, 5_000), (9_900, 1_000), (10_000, 0), (10_600, 0), (9_400, 5_500)] {
        let hedged = [fut(Long), opt(PutHeld, strike, 100)];
        assert_eq!((asked_group(&hedged, a, UP), worst(&hedged, a, UP)), (least, least), "a put at {strike}");
        assert_eq!(asked_flat(&hedged, a, UP), if strike == 10_600 { 0 } else { 5_500 });
    }

    // N69: a short and a call held at 96. What it can lose does not change as the price crosses 101.5 or so
    // (where the strike comes between the ends); counted at its worse end only, what it was asked jumped.
    for p in [10_150, 10_200] {
        let x = [Leg { entry: p, ..fut(Short) }, Leg { entry: p, ..opt(CallHeld, 9_600, 700) }];
        assert_eq!((asked_group(&x, p, UP), worst(&x, p, UP)), (0, 0));
    }
    let x = [Leg { entry: 10_150, ..fut(Short) }, Leg { entry: 10_150, ..opt(CallHeld, 9_600, 700) }];
    assert!(asked_flat(&x, 10_150, UP) > 11_000);

    // N68: a long and as many deep puts. Asked nothing but the premium; within T of the puts' day, the long
    // whole, while both legs still stand.
    let hedge = [fut(Long), opt(PutHeld, 15_000, 100)];
    assert_eq!(asked_group(&hedge, a, UP), 0);
    let near = [fut(Long), Leg { near: true, ..opt(PutHeld, 15_000, 100) }];
    assert_eq!(asked_group(&near, a, UP), 5_500);
    // (And an option near its day still adds what it can lose: a call written alone.)
    assert_eq!(asked_group(&[Leg { near: true, ..opt(CallWritten, 10_000, 200) }], a, UP), 5_500);
}

#[test]
fn the_account_that_stays_is_tested_as_it_stays() {
    // N65. A long and a short of 10,000 units from 100, on 10,100 KAS: asked nothing. The price goes to 101, and
    // the user states that the short leaves.
    let leg = |kind| Leg { kind, units: 10_000, strike: 0, entry: 10_000, premium: 0, near: false };
    let mut acct = Acct { coins: 1_010_000, legs: vec![leg(Long), leg(Short)] };
    let p = 10_100;
    assert_eq!(asked_group(&acct.legs, p, UP), 0);
    assert!(!acct.due(p));
    let lost = -acct.legs[1].gained(p);
    assert_eq!(lost, 1_000_000);
    // As the end is built (16: a leg that lost leaves where the coins pay it): right.
    assert!(acct.coins >= lost);
    acct.coins -= lost;
    acct.legs.remove(1);
    // What stays: a long worth 1,010,000 KAS on 100 KAS of coins and what it gained. A step is due at once, and
    // an entry of that long alone asks five and a half times what the account has.
    let has = acct.coins + acct.gained(p);
    assert_eq!((has, asked_group(&acct.legs, p, UP), acct.due(p)), (1_010_000, 5_560_000, true));
    // The rule: what stays is held to the level of an entry. This leaving is wrong.
    let stands = |x: &Acct, p: i64| x.coins + x.gained(p) >= asked_group(&x.legs, p, UP) + x.premiums();
    assert!(!stands(&acct, p));
    // (With 45,500 KAS more it is right: the move of the rate is rounded up to a step of the price.)
    acct.coins += 4_549_999;
    assert!(!stands(&acct, p));
    acct.coins += 1;
    assert!(stands(&acct, p));
}

/// An account as 17.3 and 17.4 have it: coins, legs of one underlying, and the premiums asked whole.
struct Acct {
    coins: i64,
    legs: Vec<Leg>,
}

impl Acct {
    fn premiums(&self) -> i64 {
        self.legs.iter().map(|x| x.premium_all()).sum()
    }
    fn gained(&self, p: i64) -> i64 {
        self.legs.iter().map(|x| x.gained(p)).sum()
    }
    /// A step is due below this.
    fn due(&self, p: i64) -> bool {
        self.coins + self.gained(p) < asked_group(&self.legs, p, LO) + self.premiums()
    }
}

#[test]
fn an_account_that_is_not_due_pays_its_losses_and_its_premiums_after_a_move_of_the_lower_rate() {
    let mut rng = Rng(0x17_0003);
    let mut tight = 0;
    for _ in 0..200_000 {
        let a = 1_000 + rng.below(1_000_000);
        let legs: Vec<Leg> = (0..1 + rng.below(6)).map(|_| rng.leg(a)).collect();
        let mut acct = Acct { coins: 0, legs };
        // (The least coins at which no step is due at this price.)
        acct.coins = asked_group(&acct.legs, a, LO) + acct.premiums() - acct.gained(a);
        assert!(!acct.due(a));
        let (l, u) = ends(a, LO);
        for p in [l, u, l + rng.below(u - l + 1), l + rng.below(u - l + 1)] {
            // What the account has after every leg left at p: the coins and what the legs gained or lost. The
            // shard is owed the premiums of the options held, and in a step the premium of an option written
            // where that is more than the option lost: no more than every premium.
            let has = acct.coins + acct.gained(p);
            assert!(has >= acct.premiums(), "{:?} from {a} to {p}: has {has}", acct.legs);
            tight += (has == acct.premiums()) as i64;
        }
    }
    println!("{tight} of the moves left exactly the premiums");
}

/// A shard's books as 17.6 has them: what it holds, and for each row the clearing side's net units; what the
/// legs were entered at is one number. An option is taken at what it is worth exercised at its entry.
#[derive(Default)]
struct Books {
    held: i64,
    /// The users' legs, as they are: the clearing side has the other side of each.
    legs: Vec<Leg>,
}

impl Books {
    fn equity(&self, p: i64) -> i64 {
        self.held - self.legs.iter().map(|x| x.gained(p)).sum::<i64>()
    }
}

#[test]
fn no_premium_is_equity_of_a_shard() {
    // N43, N44: an owner's own account holds, or writes, an option of any premium. The books move by nothing
    // where it enters (its entry is the market's price), and by what it is worth exercised afterwards.
    let a = 10_000;
    for kind in [CallHeld, CallWritten, PutHeld, PutWritten] {
        for premium in [0, 500, 70_000, 10_000_000] {
            let mut b = Books { held: 100_000, legs: vec![] };
            let before = b.equity(a);
            b.legs.push(Leg { kind, units: 100, strike: 10_000, entry: a, premium, near: false });
            assert_eq!(b.equity(a), before);
            // (And the account that holds it is asked the premium whole, in coins, whatever the price.)
            let acct = Acct { coins: 0, legs: b.legs.clone() };
            for p in [5_000, 10_000, 10_500, 20_000] {
                assert!(asked_group(&acct.legs, p, UP) + acct.premiums() >= premium * 100);
            }
        }
    }
}

/// A claim's part of what a called shard holds (14.6), exact enough here.
fn part(claim: i64, held: i64, owed: i64) -> i64 {
    if held >= owed { claim } else { (claim as i128 * held as i128 / owed as i128) as i64 }
}

#[test]
fn a_leg_entered_and_called_is_a_claim_of_nothing_by_its_time_and_not_by_a_band() {
    // 17.9 (F116). The committee's price is 103. The tolerance is half a percent.
    let fair = 10_300;
    let edges = |c: i64| (c - rate_up(c, G) + 1, c + c * G / BPS);
    let (lowest, highest) = edges(fair);
    let leg = |units, entry| Leg { kind: Long, units, strike: 0, entry, premium: 0, near: false };
    // An honest user: long 100 units from 99.5. The owner's own account: long 10,000 units entered at the lower
    // edge, in the transaction that takes the shard below its level. The shard holds 1,000 KAS.
    let (honest, own, held) = (leg(100, 9_950), leg(10_000, lowest), 100_000);

    // As built (14.6): no band under a call; the call's price is stated at the upper edge.
    let (h, o) = (honest.gained(highest), own.gained(highest));
    assert!(h > 0 && o > 10 * held);
    let paid = part(h, held, h + o);
    println!("as built: the honest claim {h} is paid {paid}; the owner's claim {o} is paid {}", part(o, held, h + o));
    assert!(paid * 10 < h, "the honest claim is paid less than a tenth");
    // Without the owner's leg the shard pays the honest claim whole.
    assert_eq!(part(h, held, h), h);

    // A band under a call (a leg that gained is asked what it gained less the tolerance twice of its worth): the
    // two edges are taken back where the price did not move.
    let band = |x: &Leg, p: i64| (x.gained(p) - rate_up(p * x.units, 2 * G)).max(0);
    for entry in [lowest, fair, highest] {
        for called in [lowest, fair, highest] {
            assert_eq!(band(&leg(10_000, entry), called), 0, "entered at {entry}, called at {called}");
        }
    }
    // But the price moves between the entry and the call, and nothing bounds the leg: up a tenth of a percent,
    // the owner's leg is a claim beyond its band, and the honest claim, itself less its band, is not paid whole.
    let (_, later) = edges(fair + fair / 1_000);
    let (hb, ob) = (band(&honest, later), band(&own, later));
    println!("a band, the price up 0.1 %: the honest claim {} is {hb} with the band and paid {}; the owner's {ob}", honest.gained(later), part(hb, held, hb + ob));
    assert!(ob > held / 2 && part(hb, held, hb + ob) < hb && hb * 10 < honest.gained(later) * 8);

    // A leg says the time it entered: under a call one younger than T asks nothing where it gained, and pays
    // what it lost. No band. The owner's leg is of the transaction that made the call due.
    let claim = |x: &Leg, p: i64, young: bool| if young { x.gained(p).min(0) } else { x.gained(p) };
    for called in [highest, later, 2 * fair] {
        let (hc, oc) = (claim(&honest, called, false), claim(&own, called, true));
        assert_eq!((oc, hc), (0, honest.gained(called)));
        assert_eq!(part(hc, held, hc + oc), hc.min(held));
    }
    // (Had the price fallen, the owner's account pays its own shard.)
    assert!(claim(&own, fair - fair / 100, true) < 0);
}

// ---------------------------------------------------------------------------------------------------------------
// A shard over time (17.3 to 17.9 together): users with accounts of futures and options, an owner with accounts of
// its own keys who knows the next price, and whoever watches acting once a tick. As `research_solvency.rs` did for
// section 12, with what section 17 adds: premiums as coins, what is asked netted, a day, and a leg's time.
// ---------------------------------------------------------------------------------------------------------------

/// Ticks. A leg younger than this at a call asks nothing where it gained; an option within it of its day gives no
/// credit, and none enters within it of its day.
const T: i64 = 2;
/// The rate on the gross of a shard's books: at its upper level and at its lower.
const GROSS_UP: i64 = 225;
const GROSS_LO: i64 = 125;

#[derive(Clone, Copy, Debug)]
struct Rules {
    /// 17.9: a leg younger than T under a call asks nothing where it gained.
    young: bool,
    /// 17.4 (N68): an option within T of its day gives no credit.
    near: bool,
}

#[derive(Clone, Copy, Debug)]
struct Held {
    leg: Leg,
    entered: i64,
    /// An option's day; nothing for a futures leg.
    day: i64,
}

#[derive(Clone, Debug)]
struct Party {
    own: bool,
    coins: i64,
    legs: Vec<Held>,
}

impl Party {
    fn legs_at(&self, now: i64, rules: Rules) -> Vec<Leg> {
        self.legs.iter().map(|h| Leg { near: rules.near && h.leg.option() && h.day - now <= T, ..h.leg }).collect()
    }
    fn premiums(&self) -> i64 {
        self.legs.iter().map(|h| h.leg.premium_all()).sum()
    }
    fn gained(&self, p: i64) -> i64 {
        self.legs.iter().map(|h| h.leg.gained(p)).sum()
    }
    fn asked(&self, p: i64, r: i64, now: i64, rules: Rules) -> i64 {
        asked_group(&self.legs_at(now, rules), p, r) + self.premiums()
    }
    fn due(&self, p: i64, now: i64, rules: Rules) -> bool {
        !self.legs.is_empty() && self.coins + self.gained(p) < self.asked(p, LO, now, rules)
    }
}

fn mirror(l: &Leg) -> Leg {
    let kind = match l.kind {
        Long => Short,
        Short => Long,
        CallHeld => CallWritten,
        CallWritten => CallHeld,
        PutHeld => PutWritten,
        PutWritten => PutHeld,
    };
    Leg { kind, ..*l }
}

#[derive(Clone, Debug)]
struct Shard {
    rules: Rules,
    now: i64,
    held: i64,
    parties: Vec<Party>,
    /// What users were owed and not paid.
    short: i64,
    /// What accounts owed the shard and could not pay: users', and the owner's own.
    unpaid: i64,
    unpaid_own: i64,
    /// What the owner's accounts and draws took out, less what it put in.
    owner_took: i64,
    called: bool,
}

impl Shard {
    fn equity(&self, p: i64) -> i64 {
        self.held - self.parties.iter().map(|a| a.gained(p)).sum::<i64>()
    }
    /// What the books are asked: the two sums over the clearing side's legs (every held option counted as worth
    /// nothing), and a rate on the gross.
    fn requirement(&self, p: i64, r: i64, gross: i64) -> i64 {
        let side: Vec<Leg> = self.parties.iter().flat_map(|a| a.legs_at(self.now, self.rules)).map(|l| mirror(&l)).collect();
        let units: i64 = side.iter().map(|l| l.units).sum();
        asked_flat(&side, p, r) + rate_up(units * p, gross)
    }
    fn is_paid(&mut self, i: usize, owed: i64) {
        // (16.18: what the coins do not pay the account owes. Its coins go below nothing; what it is paid later
        // is set against that; it is unpaid where the account is gone with it: `gone`.)
        let a = &mut self.parties[i];
        let paid = owed.min(a.coins.max(0));
        a.coins -= owed;
        self.held += paid;
    }
    /// An account that is gone: what it still owes is unpaid; an account of the owner's gives its coins back.
    fn gone(&mut self, a: &Party) {
        if a.coins < 0 {
            if a.own {
                self.unpaid_own -= a.coins;
            } else {
                self.unpaid -= a.coins;
            }
        } else if a.own {
            self.owner_took += a.coins;
        }
    }
    /// What an account owes, taken out of its coins: for whoever sets a claim against it.
    fn owed_out(&mut self, i: usize) -> i64 {
        let debt = (-self.parties[i].coins).max(0);
        self.parties[i].coins += debt;
        debt
    }
    /// An account with no leg left: its coins are its user's.
    fn close(&mut self, i: usize) {
        let a = self.parties.remove(i);
        self.gone(&a);
    }
    /// A leg enters, the premium of a written option paid in by the owner; the account has what an entry asks and
    /// `more` beside. No gate.
    fn enter(&mut self, own: bool, legs: Vec<Held>, p: i64, more: i64) {
        let mut a = Party { own, coins: 0, legs };
        a.coins = a.asked(p, UP, self.now, self.rules) + more;
        let written: i64 = a.legs.iter().filter(|h| h.leg.option() && !h.leg.held()).map(|h| h.leg.premium_all()).sum();
        // (The owner's pocket: every coin of an account of its own, and the premiums it pays its users.)
        self.owner_took -= if own { a.coins } else { written };
        self.parties.push(a);
    }
    /// A user's step, or its own leaving of an account of futures: every leg leaves at the price. What all gained
    /// is asked of the shard less the band of the futures legs; what all lost the coins pay, and in a step the
    /// premiums of what it holds and the larger of the premium and the loss of what it wrote.
    fn leave(&mut self, i: usize, p: i64, step: bool) {
        let a = self.parties[i].clone();
        let gain = a.gained(p);
        let mut owed = (-gain).max(0);
        if step {
            for h in a.legs.iter().filter(|h| h.leg.option()) {
                let lost = (-h.leg.gained(p)).max(0);
                owed += if h.leg.held() { h.leg.premium_all() } else { (h.leg.premium_all() - lost).max(0) };
            }
        }
        let band: i64 = a.legs.iter().filter(|h| !h.leg.option()).map(|h| rate_up(h.leg.units * p, 2 * G)).sum();
        self.parties[i].legs.clear();
        self.pays_into(i, (gain - band).max(0));
        self.is_paid(i, owed);
        self.close(i);
    }
    /// What the shard pays an account stays in it as coins (17.5: a gain is no coin of its user's while legs stay).
    fn pays_into(&mut self, i: usize, asked: i64) {
        // (Set against what the account owes first: no coin moves for that.)
        let debt = self.owed_out(i);
        let set = debt.min(asked);
        self.parties[i].coins -= debt - set;
        let asked = asked - set;
        let paid = asked.min(self.held);
        self.held -= paid;
        self.parties[i].coins += paid;
        if !self.parties[i].own {
            self.short += asked - paid;
        }
    }
    /// The day's transaction: an option whose day has come leaves at the price. What it gained, less its premium
    /// where the user holds it, is paid into the account; what it lost and the premium the coins pay.
    fn days(&mut self, p: i64) {
        for i in 0..self.parties.len() {
            let ended: Vec<Held> = self.parties[i].legs.iter().filter(|h| h.leg.option() && h.day <= self.now).copied().collect();
            self.parties[i].legs.retain(|h| !(h.leg.option() && h.day <= self.now));
            for h in ended {
                let net = h.leg.gained(p) - if h.leg.held() { h.leg.premium_all() } else { 0 };
                if net >= 0 {
                    self.pays_into(i, net);
                } else {
                    self.is_paid(i, -net);
                }
            }
        }
        let mut i = 0;
        while i < self.parties.len() {
            if self.parties[i].legs.is_empty() {
                self.close(i);
            } else {
                i += 1;
            }
        }
    }
    /// A take: all the rule gives (the coins at what is asked at the entry's rate, the premiums, and what each leg
    /// lost counted alone).
    fn take(&mut self, i: usize, p: i64) {
        let a = &self.parties[i];
        let alone: i64 = a.legs.iter().map(|h| (-h.leg.gained(p)).max(0)).sum();
        let amount = (a.coins - a.asked(p, UP, self.now, self.rules) - alone).max(0);
        self.parties[i].coins -= amount;
        if self.parties[i].own {
            self.owner_took += amount;
        }
    }
    /// The owner pays in what the upper level asks.
    fn top_up(&mut self, p: i64) {
        let lacks = (self.requirement(p, UP, GROSS_UP) - self.equity(p)).max(0);
        self.held += lacks;
        self.owner_took -= lacks;
    }
    /// The owner draws all the rule gives it.
    fn draw(&mut self, p: i64) {
        let amount = (self.equity(p) - self.requirement(p, UP, GROSS_UP)).min(self.held).max(0);
        self.held -= amount;
        self.owner_took += amount;
    }
    /// What whoever watches does at the price, once a tick: a step of every account that
    /// is due; and a call of a shard below its lower level.
    fn watch(&mut self, p: i64) {
        let mut i = 0;
        while i < self.parties.len() {
            if self.parties[i].due(p, self.now, self.rules) {
                self.leave(i, p, true);
            } else {
                i += 1;
            }
        }
        if self.equity(p) < self.requirement(p, LO, GROSS_LO) {
            self.call(p);
        }
    }
    /// A call: every leg leaves at the price; no premium moves and no band is taken. The accounts that lost pay
    /// first; the claims are paid at one share of what the shard then holds.
    fn call(&mut self, p: i64) {
        self.called = true;
        let mut claims = vec![];
        for i in 0..self.parties.len() {
            let a = &self.parties[i];
            // (N78: the legs of an account that are young are counted together: nothing where together they
            // gained, paid where together they lost. Leg by leg, a long and a short entered together and called a
            // tick later would pay the one and ask nothing for the other.)
            let (mut old, mut young) = (0i64, 0i64);
            for h in &a.legs {
                let g = h.leg.gained(p);
                if self.rules.young && self.now - h.entered < T {
                    young += g;
                } else {
                    old += g;
                }
            }
            let own = a.own;
            let net = old + young.min(0) - self.owed_out(i);
            if net < 0 {
                self.is_paid(i, -net);
            } else {
                claims.push((own, net));
            }
        }
        let (held, owed) = (self.held, claims.iter().map(|c| c.1).sum::<i64>());
        for (own, claim) in claims {
            let paid = part(claim, held, owed);
            self.held -= paid;
            if own {
                self.owner_took += paid;
            } else {
                self.short += claim - paid;
            }
        }
        // (What is left of the accounts and of the shard is their owners'.)
        for a in std::mem::take(&mut self.parties) {
            self.gone(&a);
        }
        self.owner_took += self.held;
        self.held = 0;
    }
}

/// One run: users enter the accounts options are held as; the owner, who knows the next price, enters accounts of
/// its own (a leg of any size the way the price goes; a hedge that ends by the clock; options of any premium),
/// takes and draws all the rules give; the price moves by up to `tick` bps a tick. `plays`: which of the owner's
/// three plays it makes (a bit each); with a fourth bit the second play is a user's.
fn run(seed: u64, tick: i64, rules: Rules, plays: u32) -> Shard {
    let mut r = Rng(seed | 1);
    let mut w = Shard { rules, now: 0, held: 5_000_000, parties: vec![], short: 0, unpaid: 0, unpaid_own: 0, owner_took: -5_000_000, called: false };
    let mut p = 100_000i64;
    for now in 0..60 {
        w.now = now;
        let next = p + p / BPS * (r.below(2 * tick + 1) - tick);
        let fut = |kind, units| Held { leg: Leg { kind, units, strike: 0, entry: p, premium: 0, near: false }, entered: now, day: 0 };
        let opt = |kind, units, strike: i64, premium: i64, day| Held { leg: Leg { kind, units, strike: strike.max(1), entry: p, premium, near: false }, entered: now, day };
        // Users come: what options are held as, a day some ticks ahead.
        if r.below(2) == 0 {
            let (n, day) = (1 + r.below(40), now + T + 1 + r.below(6));
            let (near, far, prem) = (p - p / 50 + r.below(p / 25 + 1), p + p / 10, p / 100 + r.below(p / 50));
            let legs = match r.below(7) {
                0 => vec![fut(if r.below(2) == 0 { Long } else { Short }, n)],
                1 => vec![opt(if r.below(2) == 0 { CallHeld } else { PutHeld }, n, near, prem, day)],
                2 => vec![fut(Long, n), opt(CallWritten, n, far, prem / 2, day)],
                3 => vec![fut(Short, n), opt(PutWritten, n, p - p / 10, prem / 2, day)],
                4 => vec![opt(CallHeld, n, near, prem, day), opt(PutHeld, n, near, prem, day)],
                5 => vec![opt(CallHeld, n, near, prem, day), opt(CallWritten, n, far, prem / 3, day)],
                _ => vec![fut(Long, n), opt(PutHeld, n, p - p / 30, prem, day)],
            };
            let more = r.below(p / 20 + 1) * n / 10;
            w.enter(false, legs, p, more);
            // (No gate is computed at an entry: an owner that signs a user's leg in keeps its shard at its level, or
            // is called at the next visit. It pays in what the upper level asks with the leg.)
            w.top_up(p);
        }
        // A user's account of futures alone leaves at times, where it has not lost more than it holds.
        if r.below(5) == 0 && !w.parties.is_empty() {
            let i = r.below(w.parties.len() as i64) as usize;
            let a = &w.parties[i];
            if !a.own && a.legs.iter().all(|h| !h.leg.option()) && a.coins + a.gained(p) >= 0 {
                w.leave(i, p, false);
            }
        }
        // The owner. Out of the futures accounts that would lose; a take from every account of its own.
        let up = next > p;
        let mut i = 0;
        while i < w.parties.len() {
            let a = &w.parties[i];
            let loses = a.own && a.legs.len() == 1 && !a.legs[0].leg.option() && (a.legs[0].leg.kind == Long) != up && a.coins + a.gained(p) >= 0;
            if loses {
                w.leave(i, p, false);
            } else {
                if w.parties[i].own {
                    w.take(i, p);
                }
                i += 1;
            }
        }
        let play = r.below(4);
        match if play < 3 && plays >> play & 1 == 0 { 3 } else { play } {
            // A leg of any size the way the price goes.
            0 => w.enter(true, vec![fut(if up { Long } else { Short }, 500 + r.below(20_000))], p, 0),
            // A hedge that ends by the clock: a long and as many deep puts, or a short and deep calls.
            1 => {
                let (n, day) = (500 + r.below(5_000), now + T + 1);
                // (The day comes before the visit after next: the leg that stays moves a tick alone, the way the
                // owner cannot know yet. It takes either way.)
                let legs = if r.below(2) == 0 { vec![fut(Long, n), opt(PutHeld, n, p + p / 2, 1, day)] } else { vec![fut(Short, n), opt(CallHeld, n, p / 2, 1, day)] };
                // (With the fourth bit it is a user's play, against the clearing party.)
                let own = plays >> 3 & 1 == 0;
                w.enter(own, legs, p, 0);
                if !own {
                    w.top_up(p);
                }
            }
            // Options of any premium, held or written, the way the price goes.
            2 => {
                let kind = [if up { CallHeld } else { PutHeld }, if up { PutWritten } else { CallWritten }][r.below(2) as usize];
                w.enter(true, vec![opt(kind, 100 + r.below(5_000), p, r.below(1_000_000), now + T + 1 + r.below(3))], p, 0);
            }
            _ => {}
        }
        w.draw(p);
        // The price moves. A day falls between two visits: an option whose day has come is of the price before,
        // and what stays in its account has moved without it. The owner is faster than whoever watches: it sends
        // the day's transactions and draws again, on the books as they are before any account is stepped.
        let before = p;
        p = next;
        w.now = now + 1;
        w.days(before);
        w.draw(p);
        w.watch(p);
        if w.called {
            return w;
        }
    }
    // The end: a call at the last price.
    w.call(p);
    w
}

#[test]
fn a_shard_of_options_pays_its_users_whatever_its_owner_does() {
    let rules = Rules { young: true, near: true };
    for plays in [0b0111, 0b0110, 0b0010, 0b1010, 0b0000] {
        for tick in [50, 200, 400] {
            let (mut took, mut calls, mut ticks) = (0i64, 0, 0i64);
            for seed in 1..=1_000u64 {
                let w = run(seed * 7_919 + tick as u64, tick, rules, plays);
                assert_eq!(w.short, 0, "plays {plays:04b}, ticks of {tick} bps, seed {seed}: users were owed {} and not paid", w.short);
                assert_eq!((w.unpaid, w.unpaid_own), (0, 0), "plays {plays:04b}, ticks of {tick} bps, seed {seed}: an account did not pay the shard");
                took += w.owner_took;
                calls += (w.now < 60) as i64;
                ticks += w.now;
            }
            println!("plays {plays:04b}, ticks of {tick} bps: no user unpaid on 1,000 paths of {} ticks ({calls} ended by a call before the last tick); the owner took {} a path", ticks / 1_000, took / 1_000);
        }
    }
}

#[test]
fn without_the_legs_time_users_are_unpaid_and_without_the_days_rule_the_shard_is() {
    // The owner's leg of any size, as the form that is built has it (17.9): its users are left unpaid.
    let (mut short, mut paths) = (0i64, 0);
    for seed in 1..=1_000u64 {
        let w = run(seed * 7_919 + 200, 200, Rules { young: false, near: true }, 0b0001);
        short += w.short;
        paths += (w.short > 0) as i64;
    }
    println!("without a leg's time: users unpaid on {paths} of 1,000 paths, {} a path", short / 1_000);
    assert!(paths > 100);

    // A user's hedge that ends by the clock (N68): the account cannot pay what the leg that stays lost.
    let (mut unpaid, mut paths) = (0i64, 0);
    for seed in 1..=1_000u64 {
        let w = run(seed * 7_919 + 200, 200, Rules { young: true, near: false }, 0b1010);
        unpaid += w.unpaid;
        paths += (w.unpaid > 0) as i64;
    }
    println!("without the day's rule: the shard unpaid by its users on {paths} of 1,000 paths, {} a path", unpaid / 1_000);
    assert!(paths > 100);
}

// ---------------------------------------------------------------------------------------------------------------
// Review T2, third round (docs/audit/REVIEW-T2-options.md): the same shard with a stronger owner and stronger
// users. Nothing above is changed. What `run` does not try, and `run_x` does, a switch each:
//   top_own   the owner pays its shard up after entries of its own as well (to the upper level, or only to the
//             lower): the shard is not called by the entry itself, and the run goes on past it to the days
//   edge      every price a key of the owner's states is at the tolerance's edge: its legs' entries, its takes,
//             its leavings, the steps of its accounts where one is due even there, its draws, and the call
//   set_off   the owner enters a futures leg against its users' net, draws on the book set off, and is out before
//             the tick that goes against it (review 43, N3)
//   informed  half the users know the next price as the owner does, and enter the way it goes
//   farm      the owner pays nothing in where it signs in a user that will gain: no gate is computed, the shard
//             is called at the next visit, and the user's leg is young
//   pair      a user's long and short of one size; a tick later the leg that lost leaves alone: as the end is
//             built (the coins pay it, nothing else is asked), or under 17.4 (what stays is held to an entry)
//   junior    under a call a young leg's gain is paid after every other claim, of what is left, not nothing
//   exact     the shard's levels count every held option at exactly what it can lose (no line chosen against the
//             owner): the least any statement of a draw could say, and the latest a call could be due
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
struct Extra {
    top_own: u8,
    edge: bool,
    set_off: bool,
    informed: bool,
    farm: bool,
    pair: u8,
    junior: bool,
    exact: bool,
    /// The legs of an account that are young at a call are counted together: nothing where together they gained,
    /// paid where together they lost (in place of each leg by itself).
    young_net: bool,
    /// The edge for users' accounts as well: a step of one by its user's other key where it is due even at its
    /// best price, and the call's price at the edge at which all accounts together have gained most.
    edge_users: bool,
    /// A step is due where the account has less than its premiums at some price of a move of the lower rate (what
    /// it can lose, exactly), not below what its lines ask: the latest any step could be right.
    witness: bool,
    /// Users take all the take's rule gives them, every tick, as the owner's accounts do.
    users_take: bool,
    /// The rate on the gross is of futures units only: an option row adds nothing to it.
    gross_futures: bool,
    /// The shard's levels by rows (17.8j, 6): the legs of one series (calls or puts, a strike, a day) netted, and
    /// the futures legs.
    rows: bool,
    /// A call is due by one price a market that shows it (17.8j, 6): the lower level is what the side loses at
    /// the worst price of the move, exactly; the upper level stays along the line of nothing.
    shown: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Seen {
    /// What users' young legs had gained at a call and were not paid.
    forgone: i64,
    /// What they were paid of it (with `junior`).
    young_paid: i64,
    /// Users' accounts stepped, and entered.
    stepped: i64,
    entered: i64,
}

/// A shard's rows (17.8j, 5 and 6), as legs of the clearing side: the futures legs netted, and the legs of one
/// series (calls or puts, a strike, a day) netted. A row where the clearing side holds is a held option (counted
/// along the line of nothing between the ends); where it wrote, a written one. A row within T of its day is near.
fn netted(legs: &[Held], now: i64, near: bool) -> Vec<Leg> {
    let mut rows: Vec<((u8, i64, i64), i64)> = vec![];
    for h in legs {
        let (of, net) = match h.leg.kind {
            Long => (0, -h.leg.units),
            Short => (0, h.leg.units),
            CallHeld => (1, -h.leg.units),
            CallWritten => (1, h.leg.units),
            PutHeld => (2, -h.leg.units),
            PutWritten => (2, h.leg.units),
        };
        let key = if of == 0 { (0, 0, 0) } else { (of, h.leg.strike, h.day) };
        match rows.iter_mut().find(|r| r.0 == key) {
            Some(r) => r.1 += net,
            None => rows.push((key, net)),
        }
    }
    let kind = |of: u8, holds: bool| match (of, holds) {
        (0, true) => Long,
        (0, false) => Short,
        (1, true) => CallHeld,
        (1, false) => CallWritten,
        (2, true) => PutHeld,
        _ => PutWritten,
    };
    rows.into_iter().filter(|r| r.1 != 0).map(|((of, strike, day), net)| Leg { kind: kind(of, net > 0), units: net.abs(), strike, entry: 0, premium: 0, near: near && of != 0 && day - now <= T }).collect()
}

/// The two edges of a stated price, rounded inward.
fn edges_of(p: i64) -> (i64, i64) {
    (p - p * G / BPS, p + p * G / BPS)
}

/// What legs lose at the worst price of a move of the rate, an option near its day counted only where it loses.
fn worst_near(legs: &[Leg], a: i64, r: i64) -> i64 {
    let (l, u) = ends(a, r);
    let mut at = vec![l, u, a];
    for x in legs.iter().filter(|x| x.option() && x.strike > l && x.strike < u) {
        at.push(x.strike);
    }
    at.sort();
    at.dedup();
    let lost = |p: i64| -legs.iter().map(|x| if x.option() && x.near { x.moved(a, p).min(0) } else { x.moved(a, p) }).sum::<i64>();
    at.iter().map(|p| lost(*p)).max().unwrap().max(0)
}

impl Shard {
    fn level(&self, p: i64, upper: bool, x: Extra) -> i64 {
        let mut side: Vec<Leg> = self.parties.iter().flat_map(|a| a.legs_at(self.now, self.rules)).map(|l| mirror(&l)).collect();
        let units: i64 = side.iter().filter(|l| !(x.gross_futures && l.option())).map(|l| l.units).sum();
        if x.rows {
            let held: Vec<Held> = self.parties.iter().flat_map(|a| a.legs.iter().copied()).collect();
            side = netted(&held, self.now, self.rules.near);
        }
        let (r, gross) = if upper { (UP, GROSS_UP) } else { (LO, GROSS_LO) };
        (if x.exact || (x.shown && !upper) { worst_near(&side, p, r) } else { asked_flat(&side, p, r) }) + rate_up(units * p, gross)
    }
    fn due_x(&self, i: usize, p: i64, x: Extra) -> bool {
        let a = &self.parties[i];
        if !x.witness {
            return a.due(p, self.now, self.rules);
        }
        !a.legs.is_empty() && a.coins + a.gained(p) < worst_near(&a.legs_at(self.now, self.rules), p, LO) + a.premiums()
    }
    fn pay_in(&mut self, p: i64, upper: bool, x: Extra) {
        let lacks = (self.level(p, upper, x) - self.equity(p)).max(0);
        self.held += lacks;
        self.owner_took -= lacks;
    }
    fn stated(p: i64, x: Extra) -> Vec<i64> {
        let (lo, hi) = edges_of(p);
        if x.edge { vec![lo, p, hi] } else { vec![p] }
    }
    fn draw_x(&mut self, p: i64, x: Extra) {
        let amount = Self::stated(p, x).iter().map(|a| (self.equity(*a) - self.level(*a, true, x)).min(self.held).max(0)).max().unwrap();
        self.held -= amount;
        self.owner_took += amount;
    }
    fn take_x(&mut self, i: usize, p: i64, x: Extra) {
        let a = &self.parties[i];
        let amount = Self::stated(p, x).iter().map(|s| {
            let alone: i64 = a.legs.iter().map(|h| (-h.leg.gained(*s)).max(0)).sum();
            (a.coins - a.asked(*s, UP, self.now, self.rules) - alone).max(0)
        }).max().unwrap();
        self.parties[i].coins -= amount;
        if self.parties[i].own {
            self.owner_took += amount;
        }
    }
    /// The price within the tolerance at which an account's legs have gained most.
    fn best_for(&self, i: usize, p: i64, x: Extra) -> i64 {
        Self::stated(p, x).into_iter().max_by_key(|a| self.parties[i].gained(*a)).unwrap()
    }
    /// One leg leaves an account alone, at the price: what it gained less its band is paid in; what it lost the
    /// coins pay.
    fn drop_leg(&mut self, i: usize, k: usize, p: i64) {
        let h = self.parties[i].legs.remove(k);
        let g = h.leg.gained(p);
        if g >= 0 {
            self.pays_into(i, (g - rate_up(h.leg.units * p, 2 * G)).max(0));
        } else {
            self.is_paid(i, -g);
        }
    }
    fn watch_x(&mut self, p: i64, x: Extra, seen: &mut Seen) {
        let mut i = 0;
        while i < self.parties.len() {
            let own = self.parties[i].own;
            // (A step is right where the account is due at the price stated. An account of the owner's that is due
            // even at its best price is stepped there by the owner's other key; whoever watches states the price.)
            let at = if own || x.edge_users { self.best_for(i, p, x) } else { p };
            if self.due_x(i, at, x) {
                seen.stepped += !own as i64;
                self.leave(i, at, true);
            } else if self.due_x(i, p, x) {
                seen.stepped += !own as i64;
                self.leave(i, p, true);
            } else {
                i += 1;
            }
        }
        if self.equity(p) < self.level(p, false, x) {
            self.call_x(p, x, seen);
        }
    }
    /// A call, as `call`, at the price its caller states; a young leg's gain apart: nothing, or (junior) paid at
    /// one share of what is left when every other claim is paid.
    fn call_x(&mut self, p: i64, x: Extra, seen: &mut Seen) {
        self.called = true;
        let own_at = |a: i64| self.parties.iter().filter(|q| q.own || x.edge_users).map(|q| q.gained(a)).sum::<i64>();
        let at = Self::stated(p, x).into_iter().max_by_key(|a| own_at(*a)).unwrap();
        let (mut first, mut after) = (vec![], vec![]);
        for i in 0..self.parties.len() {
            let a = &self.parties[i];
            let (mut net, mut gained, mut lost) = (0i64, 0i64, 0i64);
            for h in &a.legs {
                let g = h.leg.gained(at);
                if !(self.rules.young && self.now - h.entered < T) {
                    net += g
                } else if g > 0 {
                    gained += g
                } else {
                    lost += g
                }
            }
            let young = if x.young_net { (gained + lost).max(0) } else { gained };
            net += if x.young_net { (gained + lost).min(0) } else { lost };
            let own = a.own;
            net -= self.owed_out(i);
            if net < 0 {
                self.is_paid(i, -net);
            } else {
                first.push((own, net));
            }
            if young > 0 {
                after.push((own, young));
            }
        }
        let (held, owed) = (self.held, first.iter().map(|c| c.1).sum::<i64>());
        for (own, claim) in first {
            let paid = part(claim, held, owed);
            self.held -= paid;
            if own {
                self.owner_took += paid;
            } else {
                self.short += claim - paid;
            }
        }
        let (held, owed) = (self.held, after.iter().map(|c| c.1).sum::<i64>());
        for (own, claim) in after {
            let paid = if x.junior { part(claim, held, owed) } else { 0 };
            self.held -= paid;
            if own {
                self.owner_took += paid;
            } else {
                seen.forgone += claim - paid;
                seen.young_paid += paid;
            }
        }
        for a in std::mem::take(&mut self.parties) {
            self.gone(&a);
        }
        self.owner_took += self.held;
        self.held = 0;
    }
}

fn run_x(seed: u64, tick: i64, rules: Rules, plays: u32, x: Extra) -> (Shard, Seen) {
    let mut r = Rng(seed | 1);
    let mut w = Shard { rules, now: 0, held: 5_000_000, parties: vec![], short: 0, unpaid: 0, unpaid_own: 0, owner_took: -5_000_000, called: false };
    let mut seen = Seen::default();
    let mut p = 100_000i64;
    for now in 0..60 {
        w.now = now;
        let next = p + p / BPS * (r.below(2 * tick + 1) - tick);
        let up = next > p;
        let (lo, hi) = edges_of(p);
        let fut = |kind, units| Held { leg: Leg { kind, units, strike: 0, entry: p, premium: 0, near: false }, entered: now, day: 0 };
        let opt = |kind, units, strike: i64, premium: i64, day| Held { leg: Leg { kind, units, strike: strike.max(1), entry: p, premium, near: false }, entered: now, day };
        // (A leg of the owner's own enters at the edge at which it has gained most at the committee's price.)
        let edged = |h: Held| {
            let (at_lo, at_hi) = (Leg { entry: lo, ..h.leg }.gained(p), Leg { entry: hi, ..h.leg }.gained(p));
            if x.edge && at_lo != at_hi {
                Held { leg: Leg { entry: if at_lo > at_hi { lo } else { hi }, ..h.leg }, ..h }
            } else {
                h
            }
        };
        // A user's pair, a tick old: the leg that lost leaves alone.
        if x.pair > 0 {
            for i in 0..w.parties.len() {
                let a = &w.parties[i];
                if a.own || a.legs.len() != 2 || a.legs.iter().any(|h| h.leg.option() || h.entered != now - 1) || a.legs[0].leg.kind == a.legs[1].leg.kind {
                    continue;
                }
                let k = if a.legs[0].leg.gained(p) < 0 { 0 } else { 1 };
                let lost = -a.legs[k].leg.gained(p);
                if lost <= 0 {
                    continue;
                }
                // (The user pays in what the end asks: what the leg lost; and under 17.4, what the leg that stays
                // is asked as an entry, less what it gained.)
                let stay = a.legs[1 - k].leg;
                let needs = lost + if x.pair == 2 { (asked_group(&[stay], p, UP) - stay.gained(p)).max(0) } else { 0 };
                w.parties[i].coins = w.parties[i].coins.max(needs);
                w.drop_leg(i, k, p);
            }
        }
        if r.below(2) == 0 {
            let (n, day) = (1 + r.below(40), now + T + 1 + r.below(6));
            let (near, far, prem) = (p - p / 50 + r.below(p / 25 + 1), p + p / 10, p / 100 + r.below(p / 50));
            let roll = r.below(7);
            let mut legs = match roll {
                0 => vec![fut(if r.below(2) == 0 { Long } else { Short }, n)],
                1 => vec![opt(if r.below(2) == 0 { CallHeld } else { PutHeld }, n, near, prem, day)],
                2 => vec![fut(Long, n), opt(CallWritten, n, far, prem / 2, day)],
                3 => vec![fut(Short, n), opt(PutWritten, n, p - p / 10, prem / 2, day)],
                4 => vec![opt(CallHeld, n, near, prem, day), opt(PutHeld, n, near, prem, day)],
                5 => vec![opt(CallHeld, n, near, prem, day), opt(CallWritten, n, far, prem / 3, day)],
                _ => vec![fut(Long, n), opt(PutHeld, n, p - p / 30, prem, day)],
            };
            let mut more = r.below(p / 20 + 1) * n / 10;
            if x.informed && r.below(2) == 0 {
                legs = match r.below(3) {
                    0 => vec![fut(if up { Long } else { Short }, n)],
                    1 => vec![opt(if up { CallHeld } else { PutHeld }, n, p, prem, day)],
                    _ => vec![opt(if up { PutWritten } else { CallWritten }, n, p, prem, day)],
                };
            } else if x.pair > 0 && roll == 0 {
                let n = 200 + r.below(3_000);
                legs = vec![fut(Long, n), fut(Short, n)];
                more = 0;
            }
            let gains = legs.iter().map(|h| h.leg.gained(next)).sum::<i64>() > 0;
            seen.entered += 1;
            w.enter(false, legs, p, more);
            if !(x.farm && gains) {
                w.pay_in(p, true, x);
            }
        }
        if r.below(5) == 0 && !w.parties.is_empty() {
            let i = r.below(w.parties.len() as i64) as usize;
            let a = &w.parties[i];
            if !a.own && a.legs.iter().all(|h| !h.leg.option()) && a.coins + a.gained(p) >= 0 {
                w.leave(i, p, false);
            }
        }
        let mut i = 0;
        while i < w.parties.len() {
            let a = &w.parties[i];
            let loses = a.own && a.legs.len() == 1 && !a.legs[0].leg.option() && (a.legs[0].leg.kind == Long) != up && a.coins + a.gained(p) >= 0;
            if loses {
                let at = w.best_for(i, p, x);
                w.leave(i, at, false);
            } else {
                if w.parties[i].own || x.users_take {
                    w.take_x(i, p, x);
                }
                i += 1;
            }
        }
        let play = r.below(4);
        let mut entered = false;
        match if play < 3 && plays >> play & 1 == 0 { 3 } else { play } {
            0 => {
                w.enter(true, vec![edged(fut(if up { Long } else { Short }, 500 + r.below(20_000)))], p, 0);
                entered = true;
            }
            1 => {
                let (n, day) = (500 + r.below(5_000), now + T + 1);
                let legs = if r.below(2) == 0 { vec![fut(Long, n), opt(PutHeld, n, p + p / 2, 1, day)] } else { vec![fut(Short, n), opt(CallHeld, n, p / 2, 1, day)] };
                let own = plays >> 3 & 1 == 0;
                w.enter(own, legs, p, 0);
                if !own {
                    w.pay_in(p, true, x);
                } else {
                    entered = true;
                }
            }
            2 => {
                let kind = [if up { CallHeld } else { PutHeld }, if up { PutWritten } else { CallWritten }][r.below(2) as usize];
                w.enter(true, vec![edged(opt(kind, 100 + r.below(5_000), p, r.below(1_000_000), now + T + 1 + r.below(3)))], p, 0);
                entered = true;
            }
            _ => {
                // (Against its users' net of futures, where the next tick does not go against the leg.)
                let net: i64 = w.parties.iter().filter(|a| !a.own).flat_map(|a| a.legs.iter()).map(|h| match h.leg.kind {
                    Long => h.leg.units,
                    Short => -h.leg.units,
                    _ => 0,
                }).sum();
                if x.set_off && net != 0 && (net < 0) == up {
                    w.enter(true, vec![edged(fut(if net > 0 { Short } else { Long }, net.abs()))], p, 0);
                }
            }
        }
        if entered && x.top_own > 0 {
            w.pay_in(p, x.top_own == 1, x);
        }
        w.draw_x(p, x);
        let before = p;
        p = next;
        w.now = now + 1;
        w.days(before);
        w.draw_x(p, x);
        w.watch_x(p, x, &mut seen);
        if w.called {
            return (w, seen);
        }
    }
    w.call_x(p, x, &mut seen);
    (w, seen)
}

/// Over `paths` runs: on how many users were unpaid, a user's account did not pay, an account of the owner's did
/// not pay; what users' young legs gave up, a path; the ticks a path ran; what the owner took, a path; users'
/// accounts stepped, a path.
fn tally(tick: i64, rules: Rules, plays: u32, x: Extra, paths: u64) -> [i64; 8] {
    let mut t = [0i64; 8];
    for seed in 1..=paths {
        let (w, seen) = run_x(seed * 7_919 + tick as u64, tick, rules, plays, x);
        t[0] += (w.short > 0) as i64;
        t[1] += (w.unpaid > 0) as i64;
        t[2] += (w.unpaid_own > 0) as i64;
        t[3] += seen.forgone;
        t[4] += w.now;
        t[5] += w.owner_took;
        t[6] += seen.stepped;
        t[7] += seen.entered;
    }
    for k in 3..6 {
        t[k] /= paths as i64;
    }
    t
}

const X0: Extra = Extra { top_own: 0, edge: false, set_off: false, informed: false, farm: false, pair: 0, junior: false, exact: false, young_net: false, edge_users: false, witness: false, users_take: false, gross_futures: false, rows: false, shown: false };
const BOTH: Rules = Rules { young: true, near: true };

/// The claim of `a_shard_of_options_pays_its_users_whatever_its_owner_does`, against an owner that pays its shard
/// up after its own entries (so that a run goes on to the days), states every price at the tolerance's edge and
/// sets its book off; against users that know the next price and take all a take gives; with the shard's levels
/// and the steps counted at exactly what can be lost. The young legs of an account are counted together (N78).
#[test]
fn review_t2_a_stronger_owner_and_stronger_users_leave_nobody_unpaid() {
    let hard = Extra { young_net: true, edge: true, edge_users: true, ..X0 };
    let sets: Vec<(&str, u32, Extra)> = vec![
        ("the owner pays its shard up, at the edge, sets off", 0b0111, Extra { top_own: 1, set_off: true, ..hard }),
        ("the same, levels and steps exact", 0b0111, Extra { top_own: 1, set_off: true, exact: true, witness: true, ..hard }),
        ("its shard paid up to the lower level only", 0b0110, Extra { top_own: 2, ..hard }),
        ("users that know the next price, and take", 0b0000, Extra { informed: true, users_take: true, ..hard }),
        ("the same, levels and steps exact", 0b0000, Extra { informed: true, users_take: true, exact: true, witness: true, ..hard }),
        ("a user's hedge by the clock, exact", 0b1010, Extra { exact: true, witness: true, ..hard }),
        ("no pay-in where a user will gain", 0b0000, Extra { informed: true, farm: true, ..hard }),
        ("one leg of a pair leaves, under 17.4", 0b0000, Extra { pair: 2, young_net: true, ..X0 }),
    ];
    for (name, plays, x) in sets {
        for tick in [50, 200, 400, 450] {
            let t = tally(tick, BOTH, plays, x, 1_000);
            println!("T2 {name:50} {tick:3} bps: unpaid {} {} {}; {} ticks a path; users stepped {} of {}; young gains not paid {} a path", t[0], t[1], t[2], t[4], t[6], t[7], t[3]);
            assert_eq!((t[0], t[1], t[2]), (0, 0, 0), "{name}, ticks of {tick} bps");
        }
    }
}

/// N65 in the simulation: a user holds a long and a short of one size on nothing; a tick later the leg that lost
/// leaves alone, as the end is built today. What stays is a leg on a fraction of what an entry asks.
#[test]
fn review_t2_one_leg_of_a_pair_leaves_as_the_end_is_built() {
    for tick in [50, 200, 400] {
        let t = tally(tick, BOTH, 0b0000, Extra { pair: 1, young_net: true, ..X0 }, 1_000);
        println!("T2 a pair, the leg that lost leaves as built, {tick} bps: users' accounts unpaid on {} of 1,000 paths", t[1]);
        assert!(t[1] > 100);
    }
}

/// N78. "A leg younger than T asks nothing where it gained and pays what it lost": said of a leg, a hedge that is
/// young at a call pays the leg that lost whole and is asked nothing for the other.
#[test]
fn review_t2_a_hedge_that_is_young_at_a_call_pays_the_leg_that_lost() {
    let shard = |legs: Vec<Leg>| {
        let mut a = Party { own: false, coins: 0, legs: legs.into_iter().map(|leg| Held { leg, entered: 0, day: 9 }).collect() };
        a.coins = a.asked(100_000, UP, 0, BOTH);
        Shard { rules: BOTH, now: 1, held: 50_000_000, parties: vec![a], short: 0, unpaid: 0, unpaid_own: 0, owner_took: 0, called: false }
    };
    let fut = |kind| Leg { kind, units: 1_000, strike: 0, entry: 100_000, premium: 0, near: false };
    let put = Leg { kind: PutHeld, units: 1_000, strike: 100_000, entry: 100_000, premium: 2_000, near: false };
    // A long and a short of 1,000 units, asked nothing: the price up 1 %, a call.
    // A long and a put bought against it at the money, asked the premium: the price down 4 %, a call.
    for (legs, p, lost, coins) in [(vec![fut(Long), fut(Short)], 101_000, 1_000_000, 0), (vec![fut(Long), put], 96_000, 4_000_000, 2_000_000)] {
        // Leg by leg: the account owes all that the leg that lost lost, and has its coins (the premium) at most.
        let (mut w, mut seen) = (shard(legs.clone()), Seen::default());
        assert_eq!(w.parties[0].coins, coins);
        w.call_x(p, X0, &mut seen);
        assert_eq!((w.unpaid, seen.forgone), (lost - coins, lost));
        // Its young legs together: they gained nothing and lost nothing.
        let (mut w, mut seen) = (shard(legs), Seen::default());
        w.call_x(p, Extra { young_net: true, ..X0 }, &mut seen);
        assert_eq!((w.unpaid, seen.forgone, w.short), (0, 0, 0));
    }
    // In the simulation: one leg of a pair leaves under 17.4's rule, the young counted leg by leg.
    let t = tally(50, BOTH, 0b0000, Extra { pair: 2, ..X0 }, 1_000);
    println!("T2 young legs counted leg by leg, ticks of 50 bps: users' accounts unpaid on {} of 1,000 paths", t[1]);
    assert!(t[1] > 0);
}

/// N79. What the claim does not see: a user whose leg is young at a call is paid nothing of what it gained. An
/// owner that pays nothing in where it signs in a user that will gain is called at the next visit.
#[test]
fn review_t2_what_the_young_give_up() {
    for tick in [50, 200, 400] {
        let x = Extra { informed: true, young_net: true, ..X0 };
        let (plain, farm, after) = (tally(tick, BOTH, 0, x, 1_000), tally(tick, BOTH, 0, Extra { farm: true, ..x }, 1_000), tally(tick, BOTH, 0, Extra { farm: true, junior: true, ..x }, 1_000));
        println!("T2 young gains not paid, a path, {tick} bps: {} (the owner took {}); the owner pays nothing in for a user that will gain: {} ({}); paid after the other claims: {} ({})", plain[3], plain[5], farm[3], farm[5], after[3], after[5]);
        assert!(farm[3] > plain[3] && after[3] < farm[3]);
        assert_eq!((farm[0], after[0]), (0, 0));
    }
    // (With the owner's first play, as `run` has it: every path ends at a call within a few ticks.)
    let t = tally(200, BOTH, 0b0111, Extra { young_net: true, ..X0 }, 1_000);
    println!("T2 the plays 0111 as `run` has them, 200 bps: {} ticks a path; young gains not paid {} a path", t[4], t[3]);
}

/// N68, the owner's own account: a hedge that ends by the clock, in a shard its owner keeps paid up (in `run` the
/// entry itself has the shard called, and the leg is young). Without the day's rule its accounts end unpaid.
#[test]
fn review_t2_the_owners_own_hedge_by_the_clock_needs_the_days_rule() {
    for tick in [200, 400] {
        let x = Extra { top_own: 1, young_net: true, ..X0 };
        let (without, with) = (tally(tick, Rules { young: true, near: false }, 0b0010, x, 1_000), tally(tick, BOTH, 0b0010, x, 1_000));
        println!("T2 the owner's hedge by the clock, its shard paid up, {tick} bps: without the day's rule users unpaid on {} of 1,000 paths, its own accounts on {}; with it {} and {}", without[0], without[2], with[0], with[2]);
        // (With what an account owes carried as the form has it (16.18), what the owner's account does not pay
        // at its hedge's day is set against what it is paid later, and its users are paid: before, the walk
        // counted it gone at once, and users were unpaid on 131 paths at 400 bps. The accounts themselves end
        // unpaid without the rule.)
        assert!(without[2] > 500);
        assert_eq!((with[0], with[1], with[2]), (0, 0, 0));
    }
}

/// Where the claim ends: at the lower rate. And the rate on the gross of option rows: nobody is unpaid without it
/// within that rate; a shard is called sooner.
#[test]
fn review_t2_the_edge_of_the_claim_and_the_gross_of_option_rows() {
    let x = Extra { informed: true, users_take: true, exact: true, witness: true, young_net: true, edge: true, edge_users: true, ..X0 };
    for (name, plays, x) in [("users that know and take", 0b0000, x), ("the owner's plays, its shard paid up", 0b0111, Extra { top_own: 1, set_off: true, informed: false, ..x })] {
        let mut first = 0;
        for tick in [400, 425, 450, 475, 500, 550, 600, 700, 800] {
            let t = tally(tick, BOTH, plays, x, 1_000);
            if first == 0 && t[0] + t[1] + t[2] > 0 {
                first = tick;
            }
            assert!(tick > 450 || t[0] + t[1] + t[2] == 0);
        }
        println!("T2 {name}: somebody is first unpaid at ticks of {first} bps");
        assert!(first > 450);
    }
    for (name, plays, x) in [("plain", 0b0000, Extra { young_net: true, ..X0 }), ("users that know and take, exact", 0b0000, x), ("the owner's plays, exact", 0b0111, Extra { top_own: 1, set_off: true, informed: false, ..x }), ("a user's hedge by the clock", 0b1010, Extra { informed: false, ..x })] {
        for tick in [50, 200, 400, 450] {
            let (with, without) = (tally(tick, BOTH, plays, x, 1_000), tally(tick, BOTH, plays, Extra { gross_futures: true, ..x }, 1_000));
            // (At ticks of the lower rate itself, 4.5 %, a call stated at the edge asks an account that lost the
            // tolerance more than the tick took: the bound is the lower rate less the tolerance. See the lines.)
            assert!(tick > 400 || (with[0], with[1], with[2]) == (0, 0, 0));
            println!("T2 no rate on the gross of option rows, {name:32} {tick:3} bps: unpaid {} {} {} (with it {} {} {}); {} ticks a path where {} with it; young gains not paid {} where {}", without[0], without[1], without[2], with[0], with[1], with[2], without[4], with[4], without[3], with[3]);
            // (Up to ticks of 4 % nobody is unpaid without it. At 4.5 % it is what pays: see the lines printed.)
            assert!(tick > 400 || (without[0], without[1], without[2]) == (0, 0, 0));
        }
    }
}

/// N81. The shard, of a book that one side leaves between two visits. A long and a short of one size set off: the
/// shard is asked the rate on the gross and nothing else. One of them leaves; the other gains a tick before anybody
/// comes. What is there is twice the rate on the gross at the level the shard stood at: 4.5 % at its upper level,
/// 2.5 % at its lower. Under a call no band is taken.
#[test]
fn review_t2_a_book_that_one_side_leaves_is_held_by_twice_the_rate_on_the_gross() {
    let n = 1_000;
    let leg = |kind| Held { leg: Leg { kind, units: n, strike: 0, entry: 100_000, premium: 0, near: false }, entered: 0, day: 0 };
    let user = |kind| Party { own: false, coins: 6_000_000, legs: vec![leg(kind)] };
    for (upper, tick, short) in [(true, 4_500, 0), (true, 4_600, 100_000), (false, 2_500, 0), (false, 3_000, 500_000), (false, 4_000, 1_500_000)] {
        let mut w = Shard { rules: BOTH, now: 9, held: 0, parties: vec![user(Long), user(Short)], short: 0, unpaid: 0, unpaid_own: 0, owner_took: 0, called: false };
        let mut seen = Seen::default();
        w.held = w.level(100_000, upper, X0);
        assert_eq!(w.held, if upper { 4_500_000 } else { 2_500_000 });
        w.watch_x(100_000, X0, &mut seen);
        assert!(!w.called);
        // The short leaves, at the price it entered at. The long gains a tick; whoever watches comes.
        w.leave(1, 100_000, false);
        w.watch_x(100_000 + tick, X0, &mut seen);
        assert!(w.called);
        assert_eq!(w.short, short, "the shard at its {} level, a tick of {tick}", if upper { "upper" } else { "lower" });
    }
    // In the simulation, where the owner draws to the upper level at every tick, it shows only with every price
    // at the edge: a user pair, one leg leaving under 17.4 rule.
    let t = tally(400, BOTH, 0b0000, Extra { pair: 2, young_net: true, edge: true, edge_users: true, ..X0 }, 1_000);
    println!("T2 one leg of a pair leaves under 17.4, every price at the edge, 400 bps: users unpaid on {} of 1,000 paths", t[0]);
}

/// N80. A held option between the ends is counted along either of its lines, and whoever states picks. Whoever
/// states a step is not the account's user: with the lines that ask most, a step is "due" on accounts that have,
/// after any move of the lower rate, their premiums and more.
#[test]
fn review_t2_a_step_stated_with_the_lines_that_ask_most() {
    let mut rng = Rng(0x17_0080);
    let (mut held, mut due) = (0, 0);
    for _ in 0..100_000 {
        let a = 1_000 + rng.below(1_000_000);
        let legs: Vec<Leg> = (0..1 + rng.below(6)).map(|_| Leg { near: false, ..rng.leg(a) }).collect();
        let (l, u) = ends(a, LO);
        if !legs.iter().any(|x| x.held() && x.strike > l && x.strike < u) {
            continue;
        }
        held += 1;
        let mut acct = Acct { coins: 0, legs };
        // Half as much again as the lines that ask least ask: the account is well above what it can lose.
        let least = asked_group(&acct.legs, a, LO);
        acct.coins = least + least / 2 + acct.premiums() - acct.gained(a);
        assert!(!acct.due(a) && acct.coins + acct.gained(a) >= worst(&acct.legs, a, LO) + acct.premiums());
        let most = (0..1u32 << acct.legs.len()).map(|w| asked_as(&acct.legs, a, LO, w)).max().unwrap();
        due += (acct.coins + acct.gained(a) < most + acct.premiums()) as i64;
    }
    println!("T2 of {held} accounts with a held option between the ends, each with half as much again as its least lines ask, {due} are due for a step by the lines that ask most");
    assert!(due * 10 > held);
}

/// 17.4's rule of a step (N80), in numbers. Whoever states a step names one price `q` within the lower rate of the
/// row's price; the rows sum what the legs gain from the row's price to it (an option within T of its day only
/// where it loses); and the step is due where the coins, what the legs gained and that sum are below the premiums.
fn due_at(acct: &Acct, a: i64, q: i64) -> bool {
    let moved: i64 = acct.legs.iter().map(|x| if x.option() && x.near { x.moved(a, q).min(0) } else { x.moved(a, q) }).sum();
    acct.coins + acct.gained(a) + moved < acct.premiums()
}

/// M1 (17.8g, D): the same test counting what a step would leave the account. What the legs gained to the price
/// named, where it is above nothing, is paid in less the band of the futures legs, and no less than nothing: an
/// account whose premiums lie in what its futures legs gained is due a band sooner, and pays whole. (An account
/// whose legs set each other off gained nothing and gives up no band: it is not due for it.)
fn due_stepped(acct: &Acct, a: i64, q: i64) -> bool {
    let moved: i64 = acct.legs.iter().map(|x| if x.option() && x.near { x.moved(a, q).min(0) } else { x.moved(a, q) }).sum();
    let gain = acct.gained(a) + moved;
    let band: i64 = acct.legs.iter().filter(|x| !x.option()).map(|x| rate_up(x.units * a, 2 * G)).sum();
    acct.coins + if gain >= 0 { (gain - band).max(0) } else { gain } < acct.premiums()
}

/// The prices a stater would try: the two ends, and every strike between them with the price either side.
fn prices_to_try(legs: &[Leg], a: i64) -> Vec<i64> {
    let (l, u) = ends(a, LO);
    let mut at = vec![l, u, a];
    for x in legs.iter().filter(|x| x.option()) {
        at.extend([x.strike - 1, x.strike, x.strike + 1].into_iter().filter(|p| *p >= l && *p <= u));
    }
    at
}

/// The rule is exact and leaves nothing to choose against the account:
///  - a step is due by some price exactly where the account, after some move of the lower rate, has less than its
///    premiums (legs far from their day): no account that pays whatever the price does is stepped, whoever states;
///  - an account no price shows due has, after any move of the lower rate, its premiums or more;
///  - a step due by a price is due by the two sums with the lines that ask least as well (what its own user would
///    state): the rule by a price steps no account the sums would not, and fewer;
///  - against the lines that ask most (`review_t2_a_step_stated_with_the_lines_that_ask_most`: one account in ten
///    and more was "due" on half as much again as it can lose): none of those accounts is due by any price.
#[test]
fn a_step_is_due_by_one_price_that_shows_it() {
    let mut rng = Rng(0x17_0085);
    let (mut due_n, mut not_due_n, mut sums_only, mut spared) = (0, 0, 0, 0);
    for case in 0..150_000 {
        let a = 1_000 + rng.below(1_000_000);
        let legs: Vec<Leg> = (0..1 + rng.below(6)).map(|_| rng.leg(a)).collect();
        let far = legs.iter().all(|x| !x.near);
        let mut acct = Acct { coins: 0, legs };
        // (Coins around the level a step is due at by the sums: from well below to half as much again above.)
        let level = asked_group(&acct.legs, a, LO) + acct.premiums() - acct.gained(a);
        acct.coins = level - level.abs() / 2 + rng.below(level.abs() + 2);
        let tried = prices_to_try(&acct.legs, a);
        let due = tried.iter().any(|q| due_at(&acct, a, *q));
        let (l, u) = ends(a, LO);
        if due {
            due_n += 1;
            // By the sums with the lines that ask least, it is due as well.
            assert!(acct.due(a), "case {case}: due by a price and not by the sums: {:?}", acct.legs);
            // And where no leg is near its day, the price that shows it is a price the account does not pay at.
            if far {
                let q = *tried.iter().find(|q| due_at(&acct, a, **q)).unwrap();
                assert!(acct.coins + acct.gained(q) < acct.premiums());
            }
        } else {
            not_due_n += 1;
            sums_only += acct.due(a) as i64;
            // After any move of the lower rate it has its premiums. (Sampled beside the prices tried.)
            for q in tried.iter().copied().chain((0..4).map(|_| l + rng.below(u - l + 1))) {
                assert!(!due_at(&acct, a, q), "case {case}: a price between those tried shows it due");
                if far {
                    assert!(acct.coins + acct.gained(q) >= acct.premiums(), "case {case}: {:?} from {a} to {q}", acct.legs);
                }
            }
        }
        // The lines that ask most: an account with half as much again as it can lose is due by no price.
        if far {
            let safe = Acct { coins: worst(&acct.legs, a, LO) * 3 / 2 + acct.premiums() - acct.gained(a), legs: acct.legs.clone() };
            let most = (0..1u32 << safe.legs.len()).map(|w| asked_as(&safe.legs, a, LO, w)).max().unwrap();
            if safe.coins + safe.gained(a) < most + safe.premiums() {
                spared += 1;
                assert!(!prices_to_try(&safe.legs, a).iter().any(|q| due_at(&safe, a, *q)));
            }
        }
    }
    println!("a step by a price: {due_n} accounts due, {not_due_n} not ({sums_only} of those due by the two sums with the lines that ask least: the sums step more accounts than can fail to pay); {spared} accounts that the lines that ask most would step are due by no price");
    assert!(due_n > 20_000 && not_due_n > 20_000 && spared > 1_000);
}

/// What an account lacks of the level of an entry at a price: what it is asked and its premiums, less its coins and
/// what its legs gained.
fn lacks(a: &Acct, p: i64) -> i64 {
    (asked_group(&a.legs, p, UP) + a.premiums() - a.coins - a.gained(p)).max(0)
}

/// A step of the legs named (a bit a leg) at a price, as 17.3 has it: they leave at the price. What they gained
/// together, less the band of the futures legs among them, is paid into the account; what they lost together the
/// coins pay, with the premium of each option held among them and, of each written, what its premium is above
/// what it lost. The account that stays, what the shard was paid, and what it was owed and not paid.
fn stepped(a: &Acct, named: u32, p: i64) -> (Acct, i64, i64) {
    let (stays, paid, unpaid, _) = stepped_net(a, named, p);
    (stays, paid, unpaid)
}

/// As `stepped`, and what the shard has of the step: what it was paid less what it paid into the account.
fn stepped_net(a: &Acct, named: u32, p: i64) -> (Acct, i64, i64, i64) {
    let (stays, paid, owes, net) = stepped_owing(a, 0, named, p);
    (stays, paid, owes, net)
}

/// As `stepped_net`, of an account that owes its shard (16.18): what the legs that leave gained is set against what
/// the account owes before any of it is coins of the account, and what the coins do not pay is owed.
fn stepped_owing(a: &Acct, owes: i64, named: u32, p: i64) -> (Acct, i64, i64, i64) {
    let (mut gain, mut owed, mut band) = (0i64, 0i64, 0i64);
    let mut stay = vec![];
    for (i, x) in a.legs.iter().enumerate() {
        if named >> i & 1 == 0 {
            stay.push(*x);
            continue;
        }
        gain += x.gained(p);
        if x.option() {
            let lost = (-x.gained(p)).max(0);
            owed += if x.held() { x.premium_all() } else { (x.premium_all() - lost).max(0) };
        } else {
            band += rate_up(x.units * p, 2 * G);
        }
    }
    owed += (-gain).max(0);
    let asks = (gain - band).max(0);
    let set_off = asks.min(owes);
    let coins = a.coins + asks - set_off;
    let paid = owed.min(coins);
    (Acct { coins: coins - paid, legs: stay }, paid, owes - set_off + owed - paid, paid - (asks - set_off))
}

/// 17.4: "a step of some legs is right only where the account lacks less after it than before it; else every leg
/// leaves", in the cases 17.8 names and at random.
#[test]
fn a_step_of_some_legs_is_right_only_where_the_account_lacks_less() {
    // Ten units; prices in hundredths. A long from 100 and a call written at 110 for a premium of 2, which the
    // shard's owner paid into the account: it has what an entry asks, 55 a unit's worth of the rate, and the 20.
    let long = Leg { kind: Long, units: 10, strike: 0, entry: 10_000, premium: 0, near: false };
    let call = Leg { kind: CallWritten, units: 10, strike: 11_000, entry: 10_000, premium: 200, near: false };
    let mut acct = Acct { coins: 0, legs: vec![long, call] };
    acct.coins = asked_group(&acct.legs, 10_000, UP) + acct.premiums();
    assert_eq!((acct.coins, acct.premiums()), (7_500, 2_000));
    // The price falls 7 %: the long lost 70 a unit... 7,000; the account has 500 and its premiums are 2,000: due.
    let p = 9_300;
    assert!(due_at(&acct, p, p) && prices_to_try(&acct.legs, p).iter().any(|q| due_at(&acct, p, *q)));
    let before = lacks(&acct, p);
    // (a) The long stepped away from the written call: the coins pay what it lost; the call stays on 500. The
    // account lacks less: right.
    let (stays, paid, unpaid) = stepped(&acct, 0b01, p);
    assert_eq!((stays.coins, paid, unpaid, stays.legs.len(), before, lacks(&stays, p)), (500, 7_000, 0, 1, 6_620, 1_500));
    // It is still due (500 against a premium of 2,000): the call leaves next, and the shard is paid the 500.
    assert!(due_at(&stays, p, p));
    let (none, paid2, unpaid2) = stepped(&stays, 0b1, p);
    // (b) Every leg at once: the same coins reach the shard, and the same is not paid.
    let (all, paid_all, unpaid_all) = stepped(&acct, 0b11, p);
    assert_eq!((paid + paid2, unpaid + unpaid2, none.coins), (paid_all, unpaid_all, all.coins));
    assert_eq!((paid_all, unpaid_all), (7_500, 1_500));
    // (c) The call named alone, worth nothing, for its premium: the shard has the 2,000 and the long stays on what
    // is left. The account lacks as much as before: wrong (17.3).
    let (stays, paid, _) = stepped(&acct, 0b10, p);
    assert_eq!((paid, stays.coins, lacks(&stays, p)), (2_000, 5_500, before));
    // (And had that step been taken: the long, stepped after it, pays 5,500 of the 7,000 it lost: the shard has
    // the same 7,500, in another order; what the rule keeps is the order: what was lost before what was priced.)
    let (_, paid2, unpaid2) = stepped(&stays, 0b1, p);
    assert_eq!((paid + paid2, unpaid2), (7_500, 1_500));

    // N71: a held option whose premium the coins hold, through a step of every other leg. A long from 100 and a
    // put held at 90 for a premium of 1 (the user's own 10): the price falls 7 %.
    let put = Leg { kind: PutHeld, units: 10, strike: 9_000, entry: 10_000, premium: 100, near: false };
    let mut acct = Acct { coins: 0, legs: vec![long, put] };
    acct.coins = asked_group(&acct.legs, 10_000, UP) + acct.premiums() + 2_000;
    let before = lacks(&acct, p);
    let (stays, paid, unpaid) = stepped(&acct, 0b01, p);
    // The long's loss is paid whole, the put stays with its premium in coins beside it: the account lacks nothing,
    // and is due at no price. The option goes on.
    assert_eq!((paid, unpaid, stays.legs.len(), lacks(&stays, p)), (7_000, 0, 1, 0));
    assert!(before > 0 && stays.coins >= stays.premiums() && !prices_to_try(&stays.legs, p).iter().any(|q| due_at(&stays, p, *q)));
    // With coins that do not pay the long's loss, the put stays on nothing: the account lacks its premium, less
    // than before (the long's risk is gone): the step of the long alone is right, and the put is due next.
    acct.coins -= 3_000;
    let (stays, _, unpaid) = stepped(&acct, 0b01, p);
    assert!(unpaid > 0 && stays.coins == 0 && lacks(&stays, p) == 1_000 && lacks(&stays, p) < lacks(&acct, p) && due_at(&stays, p, p));

    // At random: accounts that are due by a price, every step of some of their legs.
    let mut rng = Rng(0x17_0086);
    let (mut due_n, mut right, mut wrong, mut premium_only, mut ends_n, mut less_n, mut least, mut apart_less) = (0, 0, 0, 0, 0, 0, 0i64, 0);
    for _ in 0..60_000 {
        let a = 1_000 + rng.below(1_000_000);
        let legs: Vec<Leg> = (0..2 + rng.below(4)).map(|_| Leg { near: false, ..rng.leg(a) }).collect();
        let mut acct = Acct { coins: 0, legs };
        let level = worst(&acct.legs, a, LO) + acct.premiums() - acct.gained(a);
        acct.coins = (level - rng.below(level.abs() / 2 + 2)).max(0);
        if !prices_to_try(&acct.legs, a).iter().any(|q| due_at(&acct, a, *q)) {
            continue;
        }
        due_n += 1;
        let before = lacks(&acct, a);
        let n = acct.legs.len() as u32;
        let (all, _, _, net_all) = stepped_net(&acct, (1 << n) - 1, a);
        assert!(all.legs.is_empty());
        for named in 1..(1u32 << n) - 1 {
            let (stays, _, _, net) = stepped_net(&acct, named, a);
            if lacks(&stays, a) >= before {
                wrong += 1;
                // (A step that names only options written that lost nothing, where they add nothing to what is
                // asked, is among these: it takes a premium and leaves the account as it was.)
                let only_written = acct.legs.iter().enumerate().all(|(i, x)| named >> i & 1 == 0 || (x.kind == CallWritten || x.kind == PutWritten) && x.gained(a) == 0);
                premium_only += (only_written && asked_group(&stays.legs, a, UP) == asked_group(&acct.legs, a, UP)) as i64;
                continue;
            }
            right += 1;
            // What the shard has of it, followed by a step of every leg that stays at the same price, against a
            // step of every leg at once: what it was paid less what it paid in. (Found, not claimed: where legs
            // leave apart, what one gained is paid in less its band, and is no longer set against what another
            // lost before the coins pay.)
            let (_, _, owes, _) = stepped_net(&acct, named, a);
            let (_, _, _, net_rest) = stepped_owing(&stays, owes, (1 << stays.legs.len()) - 1, a);
            apart_less += (net + stepped_net(&stays, (1 << stays.legs.len()) - 1, a).3 < net_all) as i64;
            if net + net_rest >= net_all {
                ends_n += 1;
            } else {
                // (Found: only by the band. Together, the bands of the futures legs are taken from what all
                // gained; apart, a leg's band is at most what that leg gained. Never by more than the bands.)
                let bands: i64 = acct.legs.iter().filter(|x| !x.option()).map(|x| rate_up(x.units * a, 2 * G)).sum();
                assert!(net_all - net - net_rest <= bands);
                less_n += 1;
                least = least.max(net_all - net - net_rest);
            }
        }
    }
    println!("a step of some legs: {due_n} accounts due by a price; of every step of some of their legs {right} are right (the account lacks less) and {wrong} wrong ({premium_only} of them an option written named for its premium alone); followed by a step of what stays, the shard has as much as of a step of every leg at once, or more, in {ends_n} of the right ones, and less in {less_n} (by {least} at the most); without what the account owes carried from the first step to the second, less in {apart_less}");
    // What the account owes is carried (16.18): legs that leave apart leave the shard what they leave it together,
    // but for the band: never less by more than the bands of the futures legs (asserted above).
    assert!(apart_less > less_n);
    assert!(due_n > 5_000 && right > 5_000 && wrong > 5_000 && premium_only > 100);
}

/// 17.5, read as it is built for a futures leg that gained and leaves (16): what an ended option gained, less its
/// premium, is asked of the shard and paid to its user, out of the account. So it leaves at its day only where the
/// account stands without it, on its coins: the coins at what the legs that stay are asked at the entry's rate,
/// their premiums, and what each of them lost counted alone. Else it stays: its gain at its day's price counts for
/// the account's standing, moves no more, and is no coin; it leaves when the test passes, or with every leg.
///
/// The case 17.8 names: a call held at 100 for a premium of 5 whose day comes, and a call written at 110 for 2 that
/// stands on. And at random: a held option that ends in the money beside legs that go on, the price then moving a
/// lower rate a visit; with the rule and without it (the gain paid out at the day whatever stays).
#[test]
fn a_held_leg_that_gained_at_its_day_leaves_only_where_the_account_stands_without_it() {
    // What the legs that stay ask of the coins for a leg that gained to leave (and for a take).
    let stands = |coins: i64, stay: &[Leg], p: i64| coins >= asked_group(stay, p, UP) + stay.iter().map(|x| x.premium_all() + (-x.gained(p)).max(0)).sum::<i64>();
    // Ten units, prices in hundredths. The spread at its entry: asked its two premiums (the 50 are the user's
    // coins, the 20 the owner paid in), and nothing by the rate.
    let held = Leg { kind: CallHeld, units: 10, strike: 10_000, entry: 10_000, premium: 500, near: false };
    let written = Leg { kind: CallWritten, units: 10, strike: 11_000, entry: 10_000, premium: 200, near: false };
    let coins = asked_group(&[held, written], 10_000, UP) + 7_000;
    assert_eq!(coins, 7_000);
    // The held call's day, at 120: it gained 200 a... 20,000, less its premium 15,000; the written call lost 10,000
    // and goes on. Without the held call the account is asked 6,600 by the rate, the 2,000 and the 10,000: 18,600.
    let pd = 12_000;
    let net = held.gained(pd) - held.premium_all();
    assert_eq!((net, written.gained(pd), asked_group(&[written], pd, UP)), (15_000, -10_000, 6_600));
    assert!(!stands(coins, &[written], pd));
    // So it stays: 15,000 that count and are no coin. The account has 7,000 + 15,000 - 10,000 and is due at no
    // price; a move of the lower rate up (5,400 more lost) leaves it its premium and more.
    let has = |kept: i64, p: i64| coins + kept + written.gained(p);
    let (_, u) = ends(pd, LO);
    assert_eq!((has(net, pd), has(net, u)), (12_000, 6_600));
    assert!(has(net, u) >= written.premium_all());
    // Paid out at the day whatever stays (no rule): the written call stands on 7,000 having lost 10,000: the
    // account lacks at once, and a step of it leaves the shard unpaid 3,000 and the premium.
    assert_eq!(has(0, pd), -3_000);
    // (With 11,600 more of its own in the account it stands without the held call, which then leaves.)
    assert!(!stands(coins + 11_599, &[written], pd) && stands(coins + 11_600, &[written], pd));

    // At random.
    let mut rng = Rng(0x17_0087);
    let (mut kept_n, mut left_n, mut unpaid_rule, mut unpaid_none, mut paths_none, mut later, mut by_band, mut unpaid_band, mut sooner) = (0, 0, 0i64, 0i64, 0, 0, 0, 0i64, 0);
    for _ in 0..40_000 {
        let a = 1_000 + rng.below(1_000_000);
        // A held option in the money at its day, and one to three legs that go on.
        let kind = if rng.below(2) == 0 { CallHeld } else { PutHeld };
        let strike = if kind == CallHeld { a - 1 - rng.below(a / 10) } else { a + 1 + rng.below(a / 10) };
        let ended = Leg { kind, units: 1 + rng.below(50), strike, entry: a - a / 20 + rng.below(a / 10 + 1), premium: rng.below(a / 20 + 1), near: false };
        let stay: Vec<Leg> = (0..1 + rng.below(3)).map(|_| Leg { near: false, ..rng.leg(a) }).collect();
        let net = ended.gained(a) - ended.premium_all();
        if net <= 0 {
            continue;
        }
        // The account before the day: not due, by a random margin (its premiums lie in its coins).
        let all: Vec<Leg> = stay.iter().copied().chain([ended]).collect();
        let whole = Acct { coins: 0, legs: all.clone() };
        let floor = worst(&all, a, LO) + whole.premiums() - whole.gained(a);
        let coins = (floor + rng.below(floor.abs() / 2 + 2)).max(0);
        for (rule, band_counted) in [(true, false), (false, false), (true, true)] {
            // (The premium of the ended leg is paid out of what it gained: the coins are as they were.)
            let (mut coins, mut kept, mut legs, mut p) = (coins, net, stay.clone(), a);
            let mut unpaid = 0i64;
            for tick in 0..6 {
                // The ended leg leaves where the rule lets it (without the rule: at once).
                if kept > 0 && (!rule || stands(coins, &legs, p)) {
                    if rule && !band_counted {
                        left_n += (tick == 0) as i64;
                        later += (tick > 0) as i64;
                    }
                    kept = 0;
                } else if kept > 0 && tick == 0 && !band_counted {
                    kept_n += 1;
                }
                // Whoever watches: a step of every leg where the account is due by a price (what is kept counts).
                let acct = Acct { coins: coins + kept, legs: legs.clone() };
                let due_plain = prices_to_try(&legs, p).iter().any(|q| due_at(&acct, p, *q));
                let due = if band_counted { prices_to_try(&legs, p).iter().any(|q| due_stepped(&acct, p, *q)) } else { due_plain };
                sooner += (band_counted && due && !due_plain) as i64;
                // (An account that the test with the band shows due at the day itself was due before it: it is
                // not one this test would have let stand. The path is not counted.)
                if band_counted && due && tick == 0 {
                    break;
                }
                if due {
                    let (_, _, owes) = stepped(&acct, (1 << legs.len()) - 1, p);
                    // (Found: with the rule, what a step leaves unpaid is never more than the band of the
                    // futures legs that leave in it: the account had its premiums in what its legs gained, and
                    // a step pays what they gained less their band.)
                    let band: i64 = legs.iter().filter(|x| !x.option()).map(|x| rate_up(x.units * p, 2 * G)).sum();
                    assert!(!rule || owes <= band, "unpaid {owes} beyond the band {band}");
                    assert!(!band_counted || owes == 0, "M1: unpaid {owes} with the band counted");
                    by_band += (rule && !band_counted && owes > 0) as i64;
                    unpaid += owes;
                    legs.clear();
                    break;
                }
                // The user takes all a take gives it (never what is kept: it is no coin).
                let alone: i64 = legs.iter().map(|x| (-x.gained(p)).max(0)).sum();
                let took = (coins - asked_group(&legs, p, UP) - legs.iter().map(|x| x.premium_all()).sum::<i64>() - alone).max(0);
                coins -= took;
                // The price moves, by the lower rate at the most.
                let (l, u) = ends(p, LO);
                p = l + rng.below(u - l + 1);
                // (An account that took all a take gives is due at no price right after a move of the lower rate,
                // by the test that counts the band as well: review T11, D8.)
                if took > 0 && kept == 0 {
                    let after = Acct { coins, legs: legs.clone() };
                    assert!(!due_stepped(&after, p, p) && !due_at(&after, p, p));
                }
            }
            if band_counted {
                unpaid_band += unpaid;
            } else if rule {
                unpaid_rule += unpaid;
            } else {
                unpaid_none += unpaid;
                paths_none += (unpaid > 0) as i64;
            }
        }
    }
    println!("a held leg at its day: by the rule its gain stayed in the account on {kept_n} paths and left at its day on {left_n} (on {later} it left at a later visit); the shard unpaid {unpaid_rule} in all with the rule, on {by_band} paths and never by more than the band of the futures legs a step takes; paid out at the day whatever stays, the shard is unpaid on {paths_none} paths, {unpaid_none} in all; with the band counted in a step's test (M1) the shard is unpaid {unpaid_band}, {sooner} visits stepping an account the plain test does not");
    assert!(kept_n > 3_000 && left_n > 1_000 && paths_none > 300 && unpaid_none > 10 * unpaid_rule && unpaid_band == 0 && sooner > 0);
}

// ---------------------------------------------------------------------------------------------------------------
// 17.8j, step 0: a shard's requirement by rows.
// ---------------------------------------------------------------------------------------------------------------

/// What a shard is asked by its rows (the legs of one series netted) is never less than what its side can lose at
/// any price a move of the rate reaches, and never more than what it is asked leg by leg. Legs of one series set
/// each other off exactly at every price and at their day, so a row near its day is netted as any other: there
/// the rows ask less than the legs counted one by one near their day, and still no less than can be lost.
#[test]
fn a_shards_rows_ask_no_less_than_its_side_can_lose_and_no_more_than_its_legs() {
    let mut g = Rng(0x17_8A);
    let (mut n, mut exact, mut below_legs, mut below_near, mut more) = (0, 0, 0, 0, 0i64);
    for _ in 0..200_000 {
        let a = 10_000 + g.below(20_000);
        let now = 10;
        let strikes = [a - a / 20, a - a / 50, a, a + a / 50, a + a / 20];
        let held: Vec<Held> = (0..1 + g.below(8))
            .map(|_| {
                let mut leg = g.leg(a);
                if leg.option() {
                    leg.strike = strikes[g.below(5) as usize];
                }
                Held { leg, entered: 0, day: if leg.option() { now + 1 + g.below(3) * 2 } else { 0 } }
            })
            .collect();
        for r in [LO, UP] {
            let side_plain: Vec<Leg> = held.iter().map(|h| mirror(&h.leg)).collect();
            let side_near: Vec<Leg> = held.iter().map(|h| mirror(&Leg { near: h.leg.option() && h.day - now <= T, ..h.leg })).collect();
            let rows = netted(&held, now, true);
            let rows_plain = netted(&held, now, false);
            let lost = worst(&side_plain, a, r);
            assert_eq!(lost, worst(&rows_plain, a, r), "netting a series changes nothing of what is lost at any price");
            let (by_rows, by_legs) = (asked_flat(&rows, a, r), asked_flat(&side_near, a, r));
            assert!(by_rows >= lost, "the rows ask {by_rows}, the side can lose {lost}: {held:?} at {a}");
            assert!(by_rows >= worst_near(&rows, a, r));
            assert!(by_rows <= by_legs, "the rows ask {by_rows}, the legs {by_legs}: {held:?} at {a}");
            n += 1;
            exact += (by_rows == lost) as i64;
            below_legs += (by_rows < by_legs) as i64;
            below_near += (by_rows < worst_near(&side_near, a, r)) as i64;
            more += by_rows - lost;
        }
    }
    println!("{n} books by rows: {exact} asked exactly what the side can lose (the others {more} more in all); {below_legs} asked less than leg by leg, {below_near} of them less than the legs counted one by one near their day");
    assert!(exact > n / 10 && below_legs > 1_000 && below_near > 1_000);
}

/// The walks of review T2 with the shard's levels by rows: an owner that pays its shard up, states at the edge and
/// sets its book off; users that know the next price and take; a hedge by the clock. Nobody is left unpaid.
#[test]
fn with_the_shards_levels_by_rows_nobody_is_left_unpaid() {
    let hard = Extra { young_net: true, edge: true, edge_users: true, rows: true, ..X0 };
    let sets: Vec<(&str, u32, Extra)> = vec![
        ("the owner pays its shard up, at the edge, sets off", 0b0111, Extra { top_own: 1, set_off: true, ..hard }),
        ("its shard paid up to the lower level only", 0b0110, Extra { top_own: 2, ..hard }),
        ("users that know the next price, and take", 0b0000, Extra { informed: true, users_take: true, ..hard }),
        ("the same, steps exact", 0b0000, Extra { informed: true, users_take: true, witness: true, ..hard }),
        ("a user's hedge by the clock", 0b1010, Extra { witness: true, ..hard }),
        ("the owner's own hedge by the clock", 0b1001, Extra { top_own: 1, ..hard }),
        ("no pay-in where a user will gain", 0b0000, Extra { informed: true, farm: true, ..hard }),
    ];
    for (name, plays, x) in sets {
        for tick in [50, 200, 400, 450] {
            let t = tally(tick, BOTH, plays, x, 1_000);
            let flat = tally(tick, BOTH, plays, Extra { rows: false, ..x }, 1_000);
            // (And a call by one price a market that shows it.)
            let s = tally(tick, BOTH, plays, Extra { shown: true, ..x }, 1_000);
            println!("rows {name:50} {tick:3} bps: unpaid {} {} {}; {} ticks a path; the owner took {} a path (leg by leg {}); a call by a price that shows it: unpaid {} {} {}, {} ticks a path", t[0], t[1], t[2], t[4], t[5], flat[5], s[0], s[1], s[2], s[4]);
            assert_eq!((t[0], t[1], t[2]), (0, 0, 0), "{name}, ticks of {tick} bps");
            assert_eq!((s[0], s[1], s[2]), (0, 0, 0), "{name}, ticks of {tick} bps, a call by a price that shows it");
        }
    }
}

/// 17.8j's second writing, 6: a shard's rows asked a row at a time (no two rows counted together). Never less than
/// the rows counted together, so never less than the side can lose; what it costs is the owner's coins: how much
/// more, on the same books, and for a book of calls written against futures held.
#[test]
fn a_shards_rows_asked_one_by_one_ask_no_less_than_counted_together() {
    let alone = |rows: &[Leg], a: i64, r: i64| rows.iter().map(|x| asked_flat(std::slice::from_ref(x), a, r)).sum::<i64>();
    let mut g = Rng(0x17_8B);
    let (mut n, mut same, mut together, mut one_by_one) = (0, 0, 0i64, 0i64);
    for _ in 0..200_000 {
        let a = 10_000 + g.below(20_000);
        let now = 10;
        let strikes = [a - a / 20, a - a / 50, a, a + a / 50, a + a / 20];
        let held: Vec<Held> = (0..1 + g.below(8))
            .map(|_| {
                let mut leg = g.leg(a);
                if leg.option() {
                    leg.strike = strikes[g.below(5) as usize];
                }
                Held { leg, entered: 0, day: if leg.option() { now + 1 + g.below(3) * 2 } else { 0 } }
            })
            .collect();
        let rows = netted(&held, now, true);
        for r in [LO, UP] {
            let (t, o) = (asked_flat(&rows, a, r), alone(&rows, a, r));
            assert!(o >= t && t >= worst(&netted(&held, now, false), a, r), "{rows:?} at {a}: one by one {o}, together {t}");
            // (A row alone gives no credit: near its day it is asked what it is asked far from it.)
            assert_eq!(o, alone(&netted(&held, now, false), a, r));
            n += 1;
            same += (o == t) as i64;
            together += t;
            one_by_one += o;
        }
    }
    // The clearing side wrote 10 calls at 100 and holds 10 futures, the price 100: together 55, one by one 110.
    let book = [Leg { kind: CallWritten, units: 10, strike: 10_000, entry: 0, premium: 0, near: false }, Leg { kind: Long, units: 10, strike: 0, entry: 0, premium: 0, near: false }];
    assert_eq!((asked_flat(&book, 10_000, UP), alone(&book, 10_000, UP)), (5_500, 11_000));
    println!("{n} books: asked one by one the same as together on {same}; in all {one_by_one} against {together} ({} % more)", (one_by_one - together) * 100 / together);
    assert!(same > n / 10 && one_by_one > together);
}
