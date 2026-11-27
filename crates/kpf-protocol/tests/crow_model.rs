//! The row of 17.8h (`crow.rs`) against the row that is built (`covered::c_row_after`, `c_row_marked`) and against
//! what a group can lose: the seven numbers and the two chains of today's row are in it as they are; what is asked
//! is the model's (`research_account_rows.rs`). No script.

use kpf_protocol::covered::{c_row_after, c_row_marked, leg_more, DIR_SPOT, KIND_C_LEAVE, KIND_C_MARK, KIND_C_STEP, KIND_C_TAKE};
use kpf_protocol::cross::XLeg;
use kpf_protocol::crow::*;
use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::Row;

const HEAD: CHead = CHead { rate: 550, tolerance: 50, t: 400, lines: 0 };
const KAS1: i64 = 100_000_000;

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
}

fn price(market: u8) -> i64 {
    [KAS1 / 10, 3 * KAS1, 77_777_777][market as usize % 3]
}
fn leg(rng: &mut Rng) -> XLeg {
    let market = rng.below(3) as u8;
    let p = price(market);
    XLeg { market: [market + 1; 32], committee: leg_more(6_000, 0, 0, 0), scale: 100_000_000, price_max: 100 * p, size: (1 + rng.below(5_000)) * 10_000_000, dir: if rng.below(2) == 0 { LONG } else { SHORT }, entry: p - p / 50 + rng.below(p / 25) }
}
fn token(market: u8) -> XLeg {
    XLeg { market: [0x40 + market; 32], committee: [7; 32], scale: 100_000_000, price_max: 1_000 * KAS1, size: 12 * KAS1, dir: DIR_SPOT, entry: 2_000 }
}

#[test]
fn a_row_is_256_bytes_with_todays_seven_numbers_where_they_are() {
    let mut r = CRow::ZERO;
    let n = [&mut r.a, &mut r.gain, &mut r.w, &mut r.ws, &mut r.ls, &mut r.gl, &mut r.tk, &mut r.q, &mut r.m, &mut r.ba, &mut r.bl, &mut r.dda, &mut r.dua, &mut r.ra, &mut r.dds, &mut r.dus, &mut r.rs, &mut r.rl, &mut r.pa, &mut r.ps, &mut r.po, &mut r.ddb, &mut r.dub, &mut r.ns];
    for (i, x) in n.into_iter().enumerate() {
        *x = 1_000 + i as i64 * if i % 2 == 0 { 1 } else { -1 };
    }
    (r.g, r.h) = ([0xaa; 32], [0xbb; 32]);
    let b = r.bytes();
    assert_eq!(b.len(), CROW_LEN);
    assert_eq!(CRow::read(&b), Some(r));
    assert!(CRow::read(&b[1..]).is_none());
    let today = Row { a: r.a, gain: r.gain, wa: r.w, wb: r.ws, ea: r.ls, eb: r.gl, tk: r.tk, g: r.g, h: r.h }.c_bytes();
    assert_eq!(&b[..56], &today[..56], "the price and the six sums of today's row");
    assert_eq!((&b[192..224], &b[224..256]), (&today[56..88], &today[88..120]), "the two chains");
    assert_eq!(CHead::read(&HEAD.bytes()), Some(HEAD));
    assert_eq!((HEAD.bytes().len(), HEAD.lower()), (CHEAD_LEN, 450));
    assert_eq!(CHead { rate: 60, tolerance: 50, ..HEAD }.lower(), 0);
}

#[test]
fn the_row_holds_what_todays_row_holds_and_asks_what_its_groups_can_lose() {
    let mut rng = Rng(0x6a09_e667_f3bc_c908);
    let (mut netted, mut marks, mut with_token) = (0, 0, 0);
    for round in 0..20_000 {
        let mut legs: Vec<XLeg> = (0..1 + rng.below(6)).map(|_| leg(&mut rng)).collect();
        if round % 5 == 0 && legs.len() < 6 {
            legs.insert(rng.below(legs.len() as i64 + 1) as usize, token(rng.below(2) as u8));
            with_token += 1;
        }
        let a: Vec<i64> = legs.iter().map(|l| if l.dir == DIR_SPOT { 5 * KAS1 } else { price(l.market[0] - 1) }).collect();
        let named = rng.below(legs.len() as i64) as usize;
        // A take (nothing leaves), a leaving of one leg, of every leg, and a mark of one.
        for (kind, which) in [(KIND_C_TAKE, 0), (KIND_C_LEAVE, 1), (KIND_C_LEAVE, 2), (KIND_C_MARK, 3)] {
            let ne = a[named] - a[named] / 100;
            let part = |i: usize| -> Part {
                match which {
                    0 => Part::Stays,
                    1 if i == named && legs[i].dir != DIR_SPOT => Part::Leaves,
                    2 if legs[i].dir != DIR_SPOT => Part::Leaves,
                    3 if i == named && legs[i].dir != DIR_SPOT => Part::Marked(ne),
                    _ => Part::Stays,
                }
            };
            let rows = ctrace(kind, &HEAD, 0, &legs, &a, &a, part).expect("a trace");
            // Today's row, leg by leg.
            let mut old = Row::ZERO;
            for (i, l) in legs.iter().enumerate() {
                old = match part(i) {
                    Part::Marked(ne) => c_row_marked(&old, l, a[i], ne).unwrap(),
                    p => {
                        let mut r = c_row_after(&old, l, a[i], p == Part::Leaves).unwrap();
                        // (Of a mark the legs not named add nothing to the first sum: `c_row_judged_of` has it
                        // so; `c_row_after` does not know the kind.)
                        if kind == KIND_C_MARK && l.dir != DIR_SPOT {
                            r.gain = old.gain;
                        }
                        r
                    }
                };
                let new = rows[i];
                assert_eq!((new.a, new.gain, new.w, new.ws, new.ls, new.gl, new.tk, new.g, new.h), (old.a, old.gain, old.wa, old.wb, old.ea, old.eb, old.tk, old.g, old.h), "kind {kind}, leg {i} of {legs:?}");
                assert_eq!((new.q, new.pa, new.ps, new.po), (new.a, 0, 0, 0));
            }
            marks += (which == 3) as i64;
            // What is asked: over the groups as they stand, of all and of those that stay.
            let last = rows.last().unwrap();
            let asked_at = |stays_only: bool, rate: i64| -> i64 {
                let (mut r, mut dd, mut du) = (0, 0, 0);
                for (i, l) in legs.iter().enumerate() {
                    if i > 0 && legs[i - 1].market != l.market {
                        r += lost(dd, du);
                        (dd, du) = (0, 0);
                    }
                    if l.dir == DIR_SPOT || (stays_only && part(i) != Part::Stays) {
                        continue;
                    }
                    let (lo, up) = ends(a[i], rate).unwrap();
                    let w = |p: i64| (l.size as i128 * p as i128 / l.scale as i128) as i64;
                    let s = if l.dir == LONG { 1 } else { -1 };
                    dd += s * (w(lo) - w(a[i]));
                    du += s * (w(up) - w(a[i]));
                }
                r + lost(dd, du)
            };
            let asked = |stays_only: bool| asked_at(stays_only, HEAD.rate);
            assert_eq!(last.closed(), Some((asked(false), asked(true))), "kind {kind}: {legs:?}");
            let today = (last.w / 10_000) * HEAD.rate + ((last.w % 10_000) * HEAD.rate + 9_999) / 10_000;
            // (The rounding: a step of the price on each leg's units, and a sompi. Review T12, E10.)
            let slack: i64 = legs.iter().map(|l| l.size / l.scale + 2).sum();
            assert!(last.closed().unwrap().0 <= today + slack, "no account is asked more than today's rate on its worth (to the rounding)");
            netted += (last.closed().unwrap().0 < today - 1_000) as i64;
            // The bands: of all, and of what leaves (a marked leg's as well).
            let band = |i: usize| -> i64 {
                let w = (legs[i].size as i128 * a[i] as i128 / legs[i].scale as i128) as i64;
                (w / 10_000) * 100 + ((w % 10_000) * 100 + 9_999) / 10_000
            };
            let fut = |i: &usize| legs[*i].dir != DIR_SPOT;
            // (The band of every leg leaving, docs 17.8i: each leg's entry's part, the tolerance of its worth, and
            // what the groups lose by a move of the tolerance. A hedge: half the gross band; one leg: the band of
            // its worth, twice the tolerance, to a step of the price on its units.)
            let entry_part = |i: usize| -> i64 {
                let w = (legs[i].size as i128 * a[i] as i128 / legs[i].scale as i128) as i64;
                (w / 10_000) * 50 + ((w % 10_000) * 50 + 9_999) / 10_000
            };
            assert_eq!(last.band(), Some((0..legs.len()).filter(fut).map(entry_part).sum::<i64>() + asked_at(false, HEAD.tolerance)), "kind {kind}: {legs:?}");
            let gross: i64 = (0..legs.len()).filter(fut).map(band).sum();
            assert!(last.band().unwrap() <= gross + slack && last.band().unwrap() >= gross / 2 - 1, "the band of every leg is between half the gross band and the whole");
            assert_eq!(last.ns, (0..legs.len()).filter(fut).filter(|i| part(*i) == Part::Stays).count() as i64, "the legs that stay that are not held options");
            assert_eq!(last.bl, (0..legs.len()).filter(fut).filter(|i| part(*i) != Part::Stays).map(band).sum::<i64>());
            assert_eq!(last.m, 0, "no step: nothing moves to a second price");
        }
    }
    println!("the row of 17.8h: 80,000 traces hold today's seven numbers and chains; {netted} ask less than today's rate (legs of one market next to one another), {marks} marks, {with_token} accounts with a token leg");
    assert!(netted > 5_000 && with_token > 3_000);
}

#[test]
fn a_steps_price_is_within_the_lower_rate_and_one_in_a_group() {
    let l = |market: u8, dir: i64| XLeg { market: [market; 32], committee: leg_more(6_000, 0, 0, 0), scale: 100_000_000, price_max: 100 * KAS1, size: 1_000 * KAS1, dir, entry: KAS1 };
    let (long, short, other) = (l(1, LONG), l(1, SHORT), l(2, LONG));
    let a = KAS1;
    let edge = a * 450 / 10_000;
    // A step's price: within the lower rate of the row's, either way; any other statement says the row's.
    for (q, kind, fine) in [(a - edge, KIND_C_STEP, true), (a + edge, KIND_C_STEP, true), (a - edge - 1, KIND_C_STEP, false), (a + edge + 1, KIND_C_STEP, false), (a, KIND_C_TAKE, true), (a - 1, KIND_C_TAKE, false), (a + 1, KIND_C_LEAVE, false)] {
        assert_eq!(crow_after(kind, &HEAD, 0, 0, &CRow::ZERO, None, &long, a, q, Part::Stays).unwrap().is_some(), fine, "{q} of kind {kind}");
    }
    // What a long gains to it: its worth there less its worth at the row's price.
    let down = crow_after(KIND_C_STEP, &HEAD, 0, 0, &CRow::ZERO, None, &long, a, a - edge, Part::Stays).unwrap().unwrap();
    assert_eq!((down.m, down.w, down.dda, down.dua), (-1_000 * edge, 1_000 * a, -1_000 * (a * 550 / 10_000), 1_000 * (a * 550 / 10_000)));
    // The short next to it, of the same market: the same two prices, or no right row; the pair moves nothing.
    let pair = crow_after(KIND_C_STEP, &HEAD, 0, 0, &down, Some(&long), &short, a, a - edge, Part::Stays).unwrap().unwrap();
    assert_eq!((pair.m, pair.dda, pair.dua, pair.closed()), (0, 0, 0, Some((0, 0))));
    assert!(crow_after(KIND_C_STEP, &HEAD, 0, 0, &down, Some(&long), &short, a, a + edge, Part::Stays).unwrap().is_none(), "two prices of a step in one group");
    assert!(crow_after(KIND_C_STEP, &HEAD, 0, 0, &down, Some(&long), &short, a + 1, a - edge, Part::Stays).unwrap().is_none(), "two prices in one group");
    // A leg of another market closes the group and says its own.
    let next = crow_after(KIND_C_STEP, &HEAD, 0, 0, &down, Some(&long), &other, 2 * a, 2 * a + 2 * edge, Part::Leaves).unwrap().unwrap();
    assert_eq!((next.ra, next.rs, next.dda, next.dds), (1_000 * (a * 550 / 10_000), 1_000 * (a * 550 / 10_000), -1_000 * (2 * a * 550 / 10_000), 0));
    assert_eq!((next.rl, next.gl, next.m), (1_000 * (2 * a * 550 / 10_000), 1_000 * a, -1_000 * edge + 1_000 * 2 * edge));
    assert_eq!(next.closed(), Some((3_000 * (a * 550 / 10_000), 1_000 * (a * 550 / 10_000))));
    // A price out of the leg's range, a mark's entry out of range, a token leg marked: no right row.
    assert!(crow_after(KIND_C_TAKE, &HEAD, 0, 0, &CRow::ZERO, None, &long, 0, 0, Part::Stays).unwrap().is_none());
    assert!(crow_after(KIND_C_TAKE, &HEAD, 0, 0, &CRow::ZERO, None, &long, 100 * KAS1 + 1, 100 * KAS1 + 1, Part::Stays).unwrap().is_none());
    assert!(crow_after(KIND_C_MARK, &HEAD, 0, 0, &CRow::ZERO, None, &long, a, a, Part::Marked(0)).unwrap().is_none());
    assert!(crow_after(KIND_C_MARK, &HEAD, 0, 0, &CRow::ZERO, None, &token(0), a, a, Part::Marked(a)).unwrap().is_none());
}

// ---------------------------------------------------------------- the option leg (docs 17.8i; review T14's test 12)

/// An option leg of the market `market`: `way` 3 a call held, 4 a call written, 5 a put held, 6 a put written.
fn opt(market: u8, way: i64, units: i64, strike: i64, entry: i64, day: i64, premium: i64) -> XLeg {
    XLeg { market: [market + 1; 32], committee: leg_more(6_000, day, strike, premium), scale: 100_000_000, price_max: 100 * price(market), size: units * 100_000_000, dir: way, entry }
}

/// What an option is worth exercised at a price to its user, counted here with wide numbers: a second way to the
/// same number.
fn worth_exercised(l: &XLeg, p: i64) -> i128 {
    let o = option_of(l).unwrap();
    let w = |x: i64| l.size as i128 * x as i128 / l.scale as i128;
    let ex = if o.call { w(p) - w(o.strike) } else { w(o.strike) - w(p) }.max(0);
    if o.held { ex } else { -ex }
}

#[test]
fn an_option_legs_row_is_what_the_rule_says_and_asks_no_less_than_a_move_of_the_rate_loses() {
    let mut rng = Rng(0x243f_6a88_85a3_08d3);
    let t0 = 7_000;
    let (mut with_between, mut near_n, mut exact, mut more) = (0, 0, 0, 0);
    for round in 0..40_000 {
        // Legs of one to three markets, futures and options of the four ways, some within T of their day.
        let n = 1 + rng.below(6) as usize;
        let legs: Vec<XLeg> = (0..n).map(|_| {
            let market = rng.below(if round % 2 == 0 { 1 } else { 3 }) as u8;
            let p = price(market);
            if rng.below(3) == 0 {
                leg(&mut rng)
            } else {
                let strike = p - p / 10 + rng.below(p / 5);
                let day = if rng.below(4) == 0 { t0 + 1 + rng.below(HEAD.t) } else { t0 + HEAD.t + 1 + rng.below(10_000) };
                opt(market, 3 + rng.below(4), 1 + rng.below(500), strike, p - p / 50 + rng.below(p / 25), day, rng.below(p / 20))
            }
        }).collect();
        // (One price a market: legs of one market next to one another say one.)
        let a: Vec<i64> = legs.iter().map(|l| price(l.market[0] - 1)).collect();
        let lines = rng.below(64);
        let head = CHead { lines, ..HEAD };
        let named = rng.below(n as i64) as usize;
        for (kind, leaves) in [(KIND_C_TAKE, false), (KIND_C_STEP, true)] {
            let part = |i: usize| if leaves && i == named { Part::Leaves } else { Part::Stays };
            let q: Vec<i64> = a.iter().map(|p| if kind == KIND_C_STEP { p - p * 450 / 10_000 } else { *p }).collect();
            let rows = ctrace(kind, &head, t0, &legs, &a, &q, part).expect("a trace");
            let last = *rows.last().unwrap();
            // The premiums; what a leaving pays of them; the legs that stay that are not held options.
            let prem = |l: &XLeg| option_of(l).map(|o| (l.size as i128 * o.premium as i128 / l.scale as i128) as i64).unwrap_or(0);
            let gained = |l: &XLeg, p: i64| -> i64 {
                match option_of(l) {
                    Some(_) => (worth_exercised(l, p) - worth_exercised(l, l.entry)) as i64,
                    None => {
                        let w = |x: i64| (l.size as i128 * x as i128 / l.scale as i128) as i64;
                        if l.dir == LONG { w(p) - w(l.entry) } else { w(l.entry) - w(p) }
                    }
                }
            };
            assert_eq!(last.pa, legs.iter().map(prem).sum::<i64>());
            assert_eq!(last.ps, legs.iter().enumerate().filter(|(i, _)| part(*i) == Part::Stays).map(|(_, l)| prem(l)).sum::<i64>());
            let po: i64 = legs.iter().enumerate().filter(|(i, _)| part(*i) != Part::Stays).map(|(i, l)| match option_of(l) {
                Some(o) if o.held => prem(l),
                Some(_) => (prem(l) - (-gained(l, a[i])).max(0)).max(0),
                None => 0,
            }).sum();
            assert_eq!(last.po, po);
            assert_eq!(last.ns, legs.iter().enumerate().filter(|(i, l)| part(*i) == Part::Stays && !option_of(l).is_some_and(|o| o.held)).count() as i64);
            assert_eq!(last.gain, legs.iter().enumerate().map(|(i, l)| gained(l, a[i])).sum::<i64>());
            assert_eq!(last.gl, if leaves { gained(&legs[named], a[named]) } else { 0 });
            // An option adds nothing to the legs' worth.
            assert_eq!(last.w, legs.iter().enumerate().filter(|(_, l)| option_of(l).is_none()).map(|(i, l)| (l.size as i128 * a[i] as i128 / l.scale as i128) as i64).sum::<i64>());
            if kind == KIND_C_STEP {
                continue;
            }
            // What is asked is no less than what the legs lose at any price a move of the rate reaches, the legs
            // of a market counted together where they stand next to one another (here: a group at a time, as
            // the row closes them); tried at the ends, at every strike within them and a sompi either side.
            // (Within T of its day an option counts only where it loses: the row then asks more.)
            let mut worst_all = 0i64;
            let mut i = 0;
            let mut any_between = false;
            let mut any_near = false;
            while i < n {
                let mut j = i;
                while j < n && legs[j].market == legs[i].market {
                    j += 1;
                }
                let (lo, up) = ends(a[i], HEAD.rate).unwrap();
                let mut at = vec![lo, up, a[i]];
                for l in &legs[i..j] {
                    if let Some(o) = option_of(l) {
                        at.extend([o.strike - 1, o.strike, o.strike + 1].into_iter().filter(|p| *p >= lo && *p <= up));
                        any_between |= o.held && lo < o.strike && o.strike < up;
                        any_near |= o.day - t0 <= HEAD.t;
                    }
                }
                let loss = at.iter().map(|p| -legs[i..j].iter().map(|l| gained(l, *p) - gained(l, a[i])).sum::<i64>()).max().unwrap().max(0);
                worst_all += loss;
                i = j;
            }
            let asked = last.closed().unwrap().0;
            assert!(asked >= worst_all, "asked {asked}, a move of the rate loses {worst_all}: {legs:?} lines {lines}");
            with_between += any_between as i64;
            near_n += any_near as i64;
            if !any_between && !any_near {
                assert_eq!(asked, worst_all, "with no held option between the ends and none near its day, exactly: {legs:?}");
                exact += 1;
            } else {
                more += (asked > worst_all) as i64;
            }
            // The band of every leg leaving is no less than what a price stated the tolerance off, at the
            // entries or at the leaving, could show the legs to have gained more than they did.
            let mut off = 0i64;
            let mut i = 0;
            while i < n {
                let mut j = i;
                while j < n && legs[j].market == legs[i].market {
                    j += 1;
                }
                let (lob, upb) = ends(a[i], HEAD.tolerance).unwrap();
                let mut at = vec![lob, upb];
                for l in &legs[i..j] {
                    if let Some(o) = option_of(l) {
                        at.extend([o.strike - 1, o.strike, o.strike + 1].into_iter().filter(|p| *p >= lob && *p <= upb));
                    }
                }
                // (The leaving: the true price is anywhere within the tolerance of the one stated.)
                off += at.iter().map(|p| -legs[i..j].iter().map(|l| gained(l, *p) - gained(l, a[i])).sum::<i64>()).max().unwrap().max(0);
                // (The entries: each leg's own, either way. An option's is counted at its entry; a futures leg's is
                // the tolerance of its worth at the price stated, as it was built: its worth at its entry is that
                // to what it has moved since.)
                for l in &legs[i..j] {
                    if option_of(l).is_none() {
                        let w = (l.size as i128 * a[i] as i128 / l.scale as i128) as i64;
                        off += (w / 10_000) * HEAD.tolerance + ((w % 10_000) * HEAD.tolerance + 9_999) / 10_000;
                        continue;
                    }
                    let (el, eu) = ends(l.entry, HEAD.tolerance).unwrap();
                    let moved = |e: i64| gained(&XLeg { entry: e, ..*l }, a[i]) - gained(l, a[i]);
                    off += (-moved(el)).max(-moved(eu)).max(0);
                }
                i = j;
            }
            let slack: i64 = legs.iter().map(|l| l.size / l.scale + 2).sum();
            assert!(last.band().unwrap() + slack >= off, "the band {} against {off}: {legs:?}", last.band().unwrap());
        }
    }
    println!("the option leg's row: 40,000 accounts of futures and options; what is asked is exactly what a move of the rate loses on {exact} (no held option between the ends, none near its day), no less on the others ({with_between} with a held option between the ends, {near_n} with one near its day; more on {more})");
    assert!(exact > 5_000 && with_between > 5_000 && near_n > 5_000);
}

#[test]
fn an_option_is_named_by_no_leaving_and_no_mark_and_an_ended_leg_has_no_row() {
    let p = price(0);
    let call = opt(0, 3, 10, p, p, 9_000, p / 50);
    let row = |kind: i64, t0: i64, part: Part| crow_after(kind, &HEAD, t0, 0, &CRow::ZERO, None, &call, p, p, part).unwrap();
    assert!(row(KIND_C_TAKE, 7_000, Part::Stays).is_some() && row(KIND_C_STEP, 7_000, Part::Leaves).is_some());
    assert!(row(KIND_C_LEAVE, 7_000, Part::Leaves).is_none(), "an option leaves by no statement of its user's (nobody exercises before the day)");
    assert!(row(KIND_C_LEAVE, 7_000, Part::Stays).is_some(), "it stays where a futures leg beside it leaves");
    assert!(row(KIND_C_MARK, 7_000, Part::Marked(p)).is_none(), "a mark names no option");
    assert!(row(KIND_C_TAKE, 9_000, Part::Stays).is_none() && row(KIND_C_TAKE, 8_999, Part::Stays).is_some(), "an ended leg has no row: it leaves by the day's transaction");
    // A held call at the money, far from its day: asked nothing but its premium (its line of nothing loses what
    // it is worth exercised: nothing); near its day the same. Written: what it loses at the upper end.
    let far = row(KIND_C_TAKE, 7_000, Part::Stays).unwrap();
    assert_eq!((far.closed(), far.pa, far.ns, far.w), (Some((0, 0)), 10 * (p / 50), 0, 0));
    let written = crow_after(KIND_C_TAKE, &HEAD, 7_000, 0, &CRow::ZERO, None, &XLeg { dir: 4, ..call }, p, p, Part::Stays).unwrap().unwrap();
    assert_eq!((written.closed(), written.ns), (Some((10 * (p * 550 / 10_000), 10 * (p * 550 / 10_000))), 1));
    // A long of the same units next to the written call: a covered call, asked what the long loses below.
    let long = XLeg { market: call.market, committee: leg_more(6_000, 0, 0, 0), scale: call.scale, price_max: call.price_max, size: call.size, dir: LONG, entry: p };
    let first = crow_after(KIND_C_TAKE, &HEAD, 7_000, 0, &CRow::ZERO, None, &long, p, p, Part::Stays).unwrap().unwrap();
    let covered = crow_after(KIND_C_TAKE, &HEAD, 7_000, 1, &first, Some(&long), &XLeg { dir: 4, ..call }, p, p, Part::Stays).unwrap().unwrap();
    assert_eq!(covered.closed().unwrap().0, 10 * (p * 550 / 10_000), "a long and a call written at the money: the long's loss below, nothing above");
    // A put held at the money beside the long, on its line of being exercised: asked nothing below (the put pays
    // what the long loses) and nothing above; on its line of nothing, the long's loss.
    let put = XLeg { dir: 5, ..call };
    let protected = |lines: i64| crow_after(KIND_C_TAKE, &CHead { lines, ..HEAD }, 7_000, 1, &first, Some(&long), &put, p, p, Part::Stays).unwrap().unwrap().closed().unwrap().0;
    assert_eq!((protected(0b10), protected(0)), (0, 10 * (p * 550 / 10_000)));
    // In a step the line is of nothing whatever the head says.
    let stepped = crow_after(KIND_C_STEP, &CHead { lines: 0b10, ..HEAD }, 7_000, 1, &first, Some(&long), &put, p, p, Part::Stays).unwrap().unwrap();
    assert_eq!(stepped.closed().unwrap().0, 10 * (p * 550 / 10_000));
}
