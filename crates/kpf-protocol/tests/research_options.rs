//! Research, numbers only (docs/ARCHITECTURE-CLEARING.md 17, second writing): options in a covered account. No
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
