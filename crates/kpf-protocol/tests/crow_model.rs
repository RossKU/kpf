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
fn a_row_is_232_bytes_with_todays_seven_numbers_where_they_are() {
    let mut r = CRow::ZERO;
    let n = [&mut r.a, &mut r.gain, &mut r.w, &mut r.ws, &mut r.ls, &mut r.gl, &mut r.tk, &mut r.q, &mut r.m, &mut r.ba, &mut r.bl, &mut r.dda, &mut r.dua, &mut r.ra, &mut r.dds, &mut r.dus, &mut r.rs, &mut r.rl, &mut r.pa, &mut r.ps, &mut r.po];
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
    assert_eq!((&b[168..200], &b[200..232]), (&today[56..88], &today[88..120]), "the two chains");
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
            let rows = ctrace(kind, &HEAD, &legs, &a, &a, part).expect("a trace");
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
            let asked = |stays_only: bool| -> i64 {
                let (mut r, mut dd, mut du) = (0, 0, 0);
                for (i, l) in legs.iter().enumerate() {
                    if i > 0 && legs[i - 1].market != l.market {
                        r += lost(dd, du);
                        (dd, du) = (0, 0);
                    }
                    if l.dir == DIR_SPOT || (stays_only && part(i) != Part::Stays) {
                        continue;
                    }
                    let (lo, up) = ends(a[i], HEAD.rate).unwrap();
                    let w = |p: i64| (l.size as i128 * p as i128 / l.scale as i128) as i64;
                    let s = if l.dir == LONG { 1 } else { -1 };
                    dd += s * (w(lo) - w(a[i]));
                    du += s * (w(up) - w(a[i]));
                }
                r + lost(dd, du)
            };
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
            assert_eq!(last.ba, (0..legs.len()).filter(fut).map(band).sum::<i64>());
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
        assert_eq!(crow_after(kind, &HEAD, &CRow::ZERO, None, &long, a, q, Part::Stays).unwrap().is_some(), fine, "{q} of kind {kind}");
    }
    // What a long gains to it: its worth there less its worth at the row's price.
    let down = crow_after(KIND_C_STEP, &HEAD, &CRow::ZERO, None, &long, a, a - edge, Part::Stays).unwrap().unwrap();
    assert_eq!((down.m, down.w, down.dda, down.dua), (-1_000 * edge, 1_000 * a, -1_000 * (a * 550 / 10_000), 1_000 * (a * 550 / 10_000)));
    // The short next to it, of the same market: the same two prices, or no right row; the pair moves nothing.
    let pair = crow_after(KIND_C_STEP, &HEAD, &down, Some(&long), &short, a, a - edge, Part::Stays).unwrap().unwrap();
    assert_eq!((pair.m, pair.dda, pair.dua, pair.closed()), (0, 0, 0, Some((0, 0))));
    assert!(crow_after(KIND_C_STEP, &HEAD, &down, Some(&long), &short, a, a + edge, Part::Stays).unwrap().is_none(), "two prices of a step in one group");
    assert!(crow_after(KIND_C_STEP, &HEAD, &down, Some(&long), &short, a + 1, a - edge, Part::Stays).unwrap().is_none(), "two prices in one group");
    // A leg of another market closes the group and says its own.
    let next = crow_after(KIND_C_STEP, &HEAD, &down, Some(&long), &other, 2 * a, 2 * a + 2 * edge, Part::Leaves).unwrap().unwrap();
    assert_eq!((next.ra, next.rs, next.dda, next.dds), (1_000 * (a * 550 / 10_000), 1_000 * (a * 550 / 10_000), -1_000 * (2 * a * 550 / 10_000), 0));
    assert_eq!((next.rl, next.gl, next.m), (1_000 * (2 * a * 550 / 10_000), 1_000 * a, -1_000 * edge + 1_000 * 2 * edge));
    assert_eq!(next.closed(), Some((3_000 * (a * 550 / 10_000), 1_000 * (a * 550 / 10_000))));
    // A price out of the leg's range, a mark's entry out of range, a token leg marked: no right row.
    assert!(crow_after(KIND_C_TAKE, &HEAD, &CRow::ZERO, None, &long, 0, 0, Part::Stays).unwrap().is_none());
    assert!(crow_after(KIND_C_TAKE, &HEAD, &CRow::ZERO, None, &long, 100 * KAS1 + 1, 100 * KAS1 + 1, Part::Stays).unwrap().is_none());
    assert!(crow_after(KIND_C_MARK, &HEAD, &CRow::ZERO, None, &long, a, a, Part::Marked(0)).unwrap().is_none());
    assert!(crow_after(KIND_C_MARK, &HEAD, &CRow::ZERO, None, &token(0), a, a, Part::Marked(a)).unwrap().is_none());
}
