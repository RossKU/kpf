//! Research, no script: the account's side as stage 1 builds it (docs/ARCHITECTURE-CLEARING.md 17.8h; review T12,
//! E4): futures legs of several markets, the row's sums with a group open and closed, each end's test as one line,
//! a step with its fee, a gain paid out to the user's key (as the built form pays it), and the two rules review
//! T12 put back (E1: a leg that gained is named by a step only where no leg that stays has lost; E3: where the
//! coins do not pay the leg named, every leg leaves). The options' model is `research_options.rs`; this one has
//! no option and no token, and has what that one lacks.

const BPS: i64 = 10_000;
/// The upper rate (the maintenance and the tolerance), the tolerance, a liquidator's fee.
const UP: i64 = 550;
const TOL: i64 = 50;
const LIQ: i64 = 50;
const LO: i64 = UP - 2 * TOL;
const MARKETS: usize = 3;

fn rate_up(w: i64, r: i64) -> i64 {
    (w / BPS) * r + ((w % BPS) * r + BPS - 1) / BPS
}
fn rate_down(w: i64, r: i64) -> i64 {
    (w / BPS) * r + ((w % BPS) * r) / BPS
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Leg {
    market: usize,
    long: bool,
    units: i64,
    entry: i64,
}

impl Leg {
    fn s(&self) -> i64 {
        if self.long { 1 } else { -1 }
    }
    fn w(&self, p: i64) -> i64 {
        self.units * p
    }
    fn gained(&self, p: i64) -> i64 {
        self.s() * (self.w(p) - self.w(self.entry))
    }
}

/// The numbers of a row (the review's check list; no option, no token: PA, PS, PO and TK are nothing).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Row {
    g: i64,
    w: i64,
    ws: i64,
    ls: i64,
    gl: i64,
    m: i64,
    ba: i64,
    bl: i64,
    dda: i64,
    dua: i64,
    ra: i64,
    dds: i64,
    dus: i64,
    rs: i64,
    rl: i64,
}

fn lost(dd: i64, du: i64) -> i64 {
    0.max(-dd).max(-du)
}

impl Row {
    /// R with the last group closed, of all the legs and of those that stay.
    fn closed(&self) -> (i64, i64) {
        (self.ra + lost(self.dda, self.dua), self.rs + lost(self.dds, self.dus))
    }
}

/// Who leaves in a statement.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Out {
    None,
    Leg(usize),
    All,
}

/// The last row of a trace over the legs, at the prices `a` (a market's), a step's prices `q` (a row's), and with
/// the rate `r`. Rows next to one another of one market say one `q`, or the trace is wrong (`None`).
fn trace(legs: &[Leg], a: &[i64; MARKETS], q: &[i64], r: i64, out: Out) -> Option<Row> {
    let mut row = Row::default();
    for (i, x) in legs.iter().enumerate() {
        let p = a[x.market];
        let d = rate_up(p, r);
        let (l, u) = ((p - d).max(0), p + d);
        if (q[i] - p).abs() > rate_up(p, LO) || q[i] < 0 {
            return None;
        }
        if i > 0 {
            if legs[i - 1].market != x.market {
                row.ra += lost(row.dda, row.dua);
                row.rs += lost(row.dds, row.dus);
                (row.dda, row.dua, row.dds, row.dus) = (0, 0, 0, 0);
            } else if q[i] != q[i - 1] {
                return None;
            }
        }
        let leaves = out == Out::All || out == Out::Leg(i);
        let (g, w) = (x.gained(p), x.w(p));
        let (dl, du) = (x.s() * (x.w(l) - w), x.s() * (x.w(u) - w));
        let b = rate_up(w, 2 * TOL);
        row.g += g;
        row.w += w;
        row.m += x.s() * (x.w(q[i]) - w);
        row.ba += b;
        row.dda += dl;
        row.dua += du;
        if leaves {
            row.gl += g;
            row.bl += b;
            row.rl += lost(dl, du);
        } else {
            row.ws += w;
            row.ls += (-g).max(0);
            row.dds += dl;
            row.dus += du;
        }
    }
    Some(row)
}

/// The prices of a step that show an account due if any do: a group's sum is a line in its price, so the end of
/// the lower rate at which the group loses.
fn worst_q(legs: &[Leg], a: &[i64; MARKETS]) -> Vec<i64> {
    let mut q = vec![0; legs.len()];
    let mut i = 0;
    while i < legs.len() {
        let mut j = i;
        while j < legs.len() && legs[j].market == legs[i].market {
            j += 1;
        }
        let net: i64 = legs[i..j].iter().map(|x| x.s() * x.units).sum();
        let p = a[legs[i].market];
        let d = rate_up(p, LO);
        let at = if net > 0 { (p - d).max(0) } else { p + d };
        for k in i..j {
            q[k] = at;
        }
        i = j;
    }
    q
}

/// A step is due (17.8g, D): what a step would leave the account is below nothing.
fn due(m: i64, owes: i64, row: &Row) -> bool {
    let s = row.g + row.m;
    let x = if s < 0 { s } else { (s - row.ba).max(0) };
    m - owes + x < 0
}

fn take_ok(m: i64, amount: i64, row: &Row) -> bool {
    m - amount >= 0 && m - amount >= row.closed().0 + row.ls
}

/// What a step did: the legs that stay, the coins that stay, what the shard was paid, what it was left unpaid,
/// what it paid out to the user's key, the fee.
#[derive(Debug, PartialEq)]
struct Stepped {
    stays: Vec<Leg>,
    coins: i64,
    paid: i64,
    unpaid: i64,
    out: i64,
    fee: i64,
    all: bool,
}

/// A step that names the leg `i` (`guard`: review T12's E1 and E3, as stage 1 builds; without it, the rule of
/// 17.8h's first writing: any leg by "lacks less", and what the coins do not pay owed). `None`: the statement is
/// wrong (not due, or a leg named that the rule refuses).
fn step(legs: &[Leg], m: i64, a: &[i64; MARKETS], i: usize, guard: bool) -> Option<Stepped> {
    let q = worst_q(legs, a);
    let row = trace(legs, a, &q, UP, Out::Leg(i))?;
    if !due(m, 0, &row) {
        return None;
    }
    let (ra, rs) = row.closed();
    let before = (ra - m - row.g).max(0);
    let mut all = false;
    let mut stays: Vec<Leg> = legs.iter().enumerate().filter(|(k, _)| *k != i).map(|(_, x)| *x).collect();
    let (mut coins, mut paid, mut unpaid, mut out, mut fee) = (m, 0, 0, 0, 0);
    if row.gl >= 0 {
        if guard {
            // (E1. With no token held, a leg that gained does not leave alone: every leg.)
            if row.ls != 0 {
                return None;
            }
            all = true;
        } else {
            fee = rate_down(row.w - row.ws, LIQ).min((m - row.ls).max(0));
            let after = (rs - (m - fee + row.g - row.gl)).max(0);
            if after < before {
                out = (row.gl - row.bl).max(0);
                coins = m - fee;
            } else {
                all = true;
            }
        }
    } else {
        fee = rate_down(row.w - row.ws, LIQ).min((m + row.gl - row.ls).max(0));
        let after = (rs - (m + row.g - fee)).max(0);
        if after < before && (m + row.gl >= 0 || !guard) {
            let pay = (-row.gl).min(m.max(0));
            paid = pay;
            // (Without the guard: what the coins do not pay the account owes; counted here as unpaid.)
            unpaid = -row.gl - pay;
            fee = fee.min((m - pay).max(0));
            coins = m - pay - fee;
        } else {
            all = true;
        }
    }
    if all {
        stays.clear();
        let room = m.max(0);
        fee = rate_down(row.w, LIQ);
        (paid, unpaid, out) = (0, 0, 0);
        if row.g < 0 {
            paid = (-row.g).min(room);
            unpaid = -row.g - paid;
        } else {
            out = (row.g - row.ba).max(0);
        }
        fee = fee.min(room - paid);
        coins = 0;
    }
    Some(Stepped { stays, coins, paid, unpaid, out, fee, all })
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
    fn legs(&mut self, a: &[i64; MARKETS]) -> Vec<Leg> {
        (0..1 + self.below(6)).map(|_| {
            let market = self.below(MARKETS as i64) as usize;
            Leg { market, long: self.below(2) == 0, units: 1 + self.below(2_000), entry: a[market] }
        }).collect()
    }
}

#[test]
fn the_rows_sums_ask_what_each_group_can_lose_and_never_less_than_a_move_of_the_rate() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut exact, mut more, mut gap_more) = (0, 0, 0);
    for _ in 0..200_000 {
        let a = [5_000 + rng.below(5_000), 20_000 + rng.below(20_000), 100 + rng.below(900)];
        let mut legs = rng.legs(&a);
        // (Entered at other prices: nothing of what is asked depends on an entry.)
        for x in legs.iter_mut() {
            x.entry = (a[x.market] + rng.below(200) - 100).max(1);
        }
        let q: Vec<i64> = legs.iter().map(|x| a[x.market]).collect();
        let row = trace(&legs, &a, &q, UP, Out::None).unwrap();
        let (ra, rs) = row.closed();
        assert_eq!(ra, rs, "nothing leaves: what stays is all");
        assert_eq!((row.gl, row.bl, row.rl, row.m, row.ws), (0, 0, 0, 0, row.w));
        // What the legs can lose where each market moves by the rate, the legs of a market counted together
        // wherever they stand: the corners.
        let ends: Vec<(i64, i64)> = a.iter().map(|p| ((p - rate_up(*p, UP)).max(0), p + rate_up(*p, UP))).collect();
        let worst: i64 = (0..MARKETS).map(|k| {
            let at = |p: i64| -> i64 { legs.iter().filter(|x| x.market == k).map(|x| x.s() * (x.w(p) - x.w(a[k]))).sum() };
            0.max(-at(ends[k].0)).max(-at(ends[k].1))
        }).sum();
        assert!(ra >= worst, "asked {ra}, a move of the rate loses {worst}: {legs:?}");
        // Exactly that where the legs of each market stand next to one another.
        let mut seen = vec![];
        let together = legs.iter().enumerate().all(|(i, x)| {
            let new = i == 0 || legs[i - 1].market != x.market;
            if new && seen.contains(&x.market) {
                return false;
            }
            if new {
                seen.push(x.market);
            }
            true
        });
        if together {
            assert_eq!(ra, worst, "{legs:?}");
            exact += 1;
        } else {
            more += (ra > worst) as i64;
        }
        // Against today's rate on what the legs are worth: an account whose legs are each of another market is
        // asked that, less a sompi a leg at the most and more by a step of the price on each leg at the most
        // (review T12, E10); no account is asked more than that.
        let today = rate_up(row.w, UP);
        let steps: i64 = legs.iter().map(|x| x.units).sum();
        assert!(ra <= today + steps, "{ra} above today's {today}");
        if legs.iter().enumerate().all(|(i, x)| legs.iter().enumerate().all(|(j, y)| i == j || x.market != y.market)) {
            assert!(ra >= today - legs.len() as i64 && ra <= today + steps, "{ra} against today's {today}: {legs:?}");
        }
        // A leg leaves: what stays is asked what it would be asked in a trace of its own, or more where the
        // leg stood between two groups of one market (they are not counted together across the gap: E6).
        let k = rng.below(legs.len() as i64) as usize;
        let rest: Vec<Leg> = legs.iter().enumerate().filter(|(i, _)| *i != k).map(|(_, x)| *x).collect();
        let stay = trace(&legs, &a, &q, UP, Out::Leg(k)).unwrap();
        let alone = trace(&rest, &a, &q[..rest.len()].iter().zip(&rest).map(|(_, x)| a[x.market]).collect::<Vec<_>>(), UP, Out::None).unwrap();
        let (_, rs) = stay.closed();
        assert!(rs >= alone.closed().0, "what stays is asked less in the trace of a leaving than alone");
        let bridged = k > 0 && k + 1 < legs.len() && legs[k - 1].market == legs[k + 1].market && legs[k].market != legs[k - 1].market;
        if !bridged {
            assert_eq!(rs, alone.closed().0, "{legs:?} without {k}");
        } else {
            gap_more += (rs > alone.closed().0) as i64;
        }
        assert_eq!((stay.g - stay.gl, stay.ws, stay.ls), (alone.g, alone.w, alone.ls));
        assert_eq!(stay.rl, lost(legs[k].s() * (legs[k].w(ends[legs[k].market].0) - legs[k].w(a[legs[k].market])), legs[k].s() * (legs[k].w(ends[legs[k].market].1) - legs[k].w(a[legs[k].market]))));
    }
    println!("the rows' sums: {exact} accounts whose markets stand together asked exactly what a move of the rate loses; {more} of the others asked more; {gap_more} leavings from between two groups of one market ask what stays more than alone");
    assert!(exact > 50_000 && more > 10_000 && gap_more > 100);
}

#[test]
fn a_hedge_next_to_what_it_hedges_is_asked_what_each_lost_alone_and_is_not_due_by_a_move() {
    let a = [10_000, 30_000, 500];
    let legs = [Leg { market: 0, long: true, units: 1_000, entry: 10_000 }, Leg { market: 0, long: false, units: 1_000, entry: 10_000 }];
    let q = [10_000, 10_000];
    let row = trace(&legs, &a, &q, UP, Out::None).unwrap();
    assert_eq!((row.closed().0, row.ls, row.w), (0, 0, 20_000_000), "asked nothing, where today's rate on its worth asks {}", rate_up(row.w, UP));
    assert!(take_ok(0, 0, &row));
    // The price moves: each lost alone what it lost, and the pair is due at no price (it loses nothing).
    let moved = [10_400, 30_000, 500];
    let row = trace(&legs, &moved, &worst_q(&legs, &moved), UP, Out::None).unwrap();
    assert_eq!((row.g, row.ls, row.m, row.closed().0), (0, 400_000, 0, 0));
    assert!(!due(0, 0, &row), "a pair that loses nothing is never due");
    assert!(!take_ok(399_999, 0, &row) && take_ok(400_000, 0, &row), "a take asks what the short lost alone");
    // Review T12, E7: with no coins, neither leg leaves by its user's statement (what stays alone would be under).
    // The leg that lost: the coins do not pay it. The leg that gained: what stays is asked its R and its loss.
    let short_out = trace(&legs, &moved, &[10_400, 10_400], UP, Out::Leg(1)).unwrap();
    assert!(short_out.gl < 0 && 0 + short_out.gl < 0);
    let long_out = trace(&legs, &moved, &[10_400, 10_400], UP, Out::Leg(0)).unwrap();
    assert!(long_out.gl > 0 && long_out.closed().1 + long_out.ls > 0);
    // The same two legs with another market's leg between them are not netted: asked as two.
    let apart = [legs[0], Leg { market: 1, long: true, units: 1, entry: 30_000 }, legs[1]];
    let row = trace(&apart, &a, &[10_000, 30_000, 10_000], UP, Out::None).unwrap();
    assert_eq!(row.closed().0, 2 * 550_000 + rate_up(30_000, UP));
    // And two prices of a step on rows next to one another of one market: wrong.
    assert!(trace(&legs, &a, &[10_000, 10_001], UP, Out::None).is_none());
    assert!(trace(&apart, &a, &[9_550, 30_000, 10_450], UP, Out::None).is_some(), "two groups of one market that are apart say two prices");
}

#[test]
fn a_step_names_a_leg_that_gained_only_where_none_that_stays_lost() {
    // Review T12, E1, in its numbers: a long of one market that gained 3,000 and a short of another that lost
    // 8,000, each worth 100,000; the coins 5,000 (above a note's least).
    // (Prices of five digits: the two ends are the rate on the price rounded up to a step of the price, so at a
    // price of 100 a rate of 5.5 % is asked as 6 %. Review T12, E10: a step of the price on a leg's units.)
    let a = [10_000, 10_000, 10_000];
    let legs = [Leg { market: 0, long: true, units: 10, entry: 9_700 }, Leg { market: 1, long: false, units: 10, entry: 9_200 }];
    let m = 5_000;
    let row = trace(&legs, &a, &worst_q(&legs, &a), UP, Out::Leg(0)).unwrap();
    assert_eq!((row.g, row.gl, row.ls, row.closed()), (-5_000, 3_000, 8_000, (11_000, 5_500)));
    assert!(due(m, 0, &row));
    // By "lacks less" alone the long is named: its gain less the band leaves for the user's key (no fee: a fee is
    // not taken before what the legs that stay lost), and the short's loss is then above the coins.
    let loose = step(&legs, m, &a, 0, false).unwrap();
    assert_eq!((loose.all, loose.out, loose.fee, loose.coins), (false, 2_000, 0, 5_000));
    let then = step(&loose.stays, loose.coins, &a, 0, false).unwrap();
    assert_eq!((then.paid, then.unpaid), (5_000, 3_000), "the shard is left 3,000 unpaid, having paid out 2,000");
    // As it is built: wrong. The short named: the coins do not pay it; every leg leaves, and the shard is paid.
    assert!(step(&legs, m, &a, 0, true).is_none(), "a leg that gained is named beside one that lost");
    let right = step(&legs, m, &a, 1, true).unwrap();
    assert_eq!((right.all, right.paid, right.unpaid, right.out), (true, 5_000, 0, 0));

    // Review T12, E3: a long that gained 20,000 beside a short that lost 14,000, the coins 1,000. Without the
    // rule the short leaves alone and an account with no token owes 13,000; as built every leg leaves: the legs
    // gained 6,000 together, the shard pays that less the band, and nothing is owed.
    let legs = [Leg { market: 0, long: true, units: 10, entry: 8_000 }, Leg { market: 1, long: false, units: 10, entry: 8_600 }];
    let loose = step(&legs, 1_000, &a, 1, false).unwrap();
    assert_eq!((loose.all, loose.paid, loose.unpaid), (false, 1_000, 13_000));
    let right = step(&legs, 1_000, &a, 1, true).unwrap();
    assert_eq!((right.all, right.paid, right.unpaid, right.out), (true, 0, 0, 6_000 - 2_000));
}

#[test]
fn an_account_at_the_level_of_a_take_is_stepped_in_time_and_pays_whole() {
    // Accounts of one to six legs over three markets, entered at the level of a take and something more; the
    // prices move by the lower rate at the most between two visits; whoever watches steps what is due, naming a
    // leg at random (a statement that is wrong is not made). By the rule that is built the shard is never left
    // unpaid; by "lacks less" alone (any leg named, what the coins do not pay owed) it is.
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let (mut steps, mut alone, mut unpaid_built, mut unpaid_loose, mut paths_loose, mut ended) = (0, 0, 0i64, 0i64, 0, 0);
    for path in 0..30_000 {
        for guard in [true, false] {
            let mut r2 = Rng(0x1000_0000_0000_0001 + path as u64 * 0x9e37_79b9);
            let mut a = [5_000 + r2.below(5_000), 20_000 + r2.below(20_000), 100 + r2.below(900)];
            let mut legs = r2.legs(&a);
            let q: Vec<i64> = legs.iter().map(|x| a[x.market]).collect();
            let row = trace(&legs, &a, &q, UP, Out::None).unwrap();
            let mut m = row.closed().0 + row.ls + r2.below(1 + row.w / 200);
            assert!(take_ok(m, 0, &row));
            // (At the level of a take an account is due at no price: the upper rate holds the lower.)
            assert!(!due(m, 0, &trace(&legs, &a, &worst_q(&legs, &a), UP, Out::None).unwrap()));
            let mut unpaid = 0;
            for _ in 0..12 {
                for p in a.iter_mut() {
                    let d = rate_up(*p, LO);
                    *p = (*p + r2.below(2 * d + 1) - d).max(1);
                }
                // Whoever watches steps until nothing is due.
                while !legs.is_empty() {
                    let row = trace(&legs, &a, &worst_q(&legs, &a), UP, Out::None).unwrap();
                    if !due(m, 0, &row) {
                        break;
                    }
                    let from = r2.below(legs.len() as i64) as usize;
                    let done = (0..legs.len()).map(|k| (from + k) % legs.len()).find_map(|k| step(&legs, m, &a, k, guard)).expect("a step that is due has a leg to name");
                    if guard {
                        steps += 1;
                        alone += (!done.all) as i64;
                    }
                    unpaid += done.unpaid;
                    legs = done.stays;
                    m = done.coins;
                }
                if legs.is_empty() {
                    ended += guard as i64;
                    break;
                }
                let _ = rng.next();
            }
            if guard {
                unpaid_built += unpaid;
            } else {
                unpaid_loose += unpaid;
                paths_loose += (unpaid > 0) as i64;
            }
        }
    }
    println!("steps at the level of a take: {steps} steps on 30,000 accounts ({alone} of a leg alone, {ended} accounts ended); as built the shard is unpaid {unpaid_built}; by \"lacks less\" alone it is unpaid {unpaid_loose} on {paths_loose} accounts");
    assert_eq!(unpaid_built, 0);
    assert!(steps > 5_000 && alone > 500 && paths_loose > 50 && unpaid_loose > 0);
}
