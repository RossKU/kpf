//! Research R2 (`docs/RESEARCH-UNIFIED-MARGIN.md`): the model of a re-mark, `thin::marked`. The entries of a position
//! are moved to a price, the gain since becomes margin, and what is above a party's floor is taken out. No script
//! runs it yet: these are the properties a script will be held to.

use kpf_protocol::cross::{SIDE_A, SIDE_B};
use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::{marked, samples, Position};

const KAS1: i64 = 100_000_000;

/// What a side is worth at prices, as every claim counts it: its margin and its gain, within the pot.
fn worth(x: &Position, side: i64, p: [i64; 2]) -> i64 {
    x.x.owed_at(side, p)
}

/// One leg: A long `units` from `entry`, margins in KAS.
fn one(units: i64, entry: i64, a: i64, b: i64) -> Position {
    let mut x = samples::position();
    let scale = x.x.legs[0].scale;
    x.x.legs[0].size = units * scale;
    x.x.legs[0].dir = LONG;
    x.x.legs[0].entry = entry;
    x.x.legs[1].size = 0;
    x.x.a_margin = a * KAS1;
    x.x.b_margin = b * KAS1;
    x.check().unwrap();
    x
}

struct Rng(u64);
impl Rng {
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        lo + (self.0 % (hi - lo) as u64) as i64
    }
}

#[test]
fn a_re_mark_moves_the_gain_into_margin_and_changes_nobodys_worth() {
    let x = one(4, 10 * KAS1, 12, 12);
    // The price rose to 11: A's gain of 4 KAS is A's margin now, and the entry is 11.
    let m = marked(&x, [11 * KAS1, 0], 0, 0).unwrap();
    assert_eq!((m.moved, m.position.x.a_margin, m.position.x.b_margin, m.position.x.legs[0].entry), (4 * KAS1, 16 * KAS1, 8 * KAS1, 11 * KAS1));
    assert_eq!(m.position.pot(), x.pot(), "nothing leaves the position");
    // At every price each side is worth exactly what it was.
    for k in 0..600 {
        let p = [5 * KAS1 + k * (KAS1 / 40) + k, 0];
        assert_eq!(worth(&m.position, SIDE_A, p), worth(&x, SIDE_A, p), "a at {}", p[0]);
        assert_eq!(worth(&m.position, SIDE_B, p), worth(&x, SIDE_B, p), "b at {}", p[0]);
    }
    // Marked again at the same price nothing moves; marked on to another price is the mark at that price.
    assert_eq!(marked(&m.position, [11 * KAS1, 0], 0, 0).unwrap().moved, 0);
    assert_eq!(marked(&m.position, [10 * KAS1 + 7, 0], 0, 0).unwrap().position, marked(&x, [10 * KAS1 + 7, 0], 0, 0).unwrap().position);
}

#[test]
fn the_gain_above_the_floor_is_taken_out_and_nothing_more() {
    // A long 4 from 10 with 12 KAS a side; each asked 6 KAS of the other. At 11 the maintenance (5 % of 44) is 2.2.
    let x = Position { a_min_counter: 6 * KAS1, b_min_counter: 6 * KAS1, ..one(4, 10 * KAS1, 12, 12) };
    let p = [11 * KAS1, 0];
    // A has 16 and keeps 6 (what B asked of it): 10 are usable. B has 8 and keeps 6: 2 are.
    let m = marked(&x, p, 10 * KAS1, 2 * KAS1).unwrap();
    assert_eq!((m.position.x.a_margin, m.position.x.b_margin, m.to_a, m.to_b), (6 * KAS1, 6 * KAS1, 10 * KAS1, 2 * KAS1));
    assert_eq!(m.position.pot() + m.to_a + m.to_b, x.pot());
    assert!(marked(&x, p, 10 * KAS1 + 1, 0).is_err(), "a sompi more than a has above its floor");
    assert!(marked(&x, p, 0, 2 * KAS1 + 1).is_err(), "a sompi more than b has above its floor");
    assert!(marked(&x, p, -1, 0).is_err());
    // Without a re-mark the gain is not margin: at the entry A has nothing above its floor of 6 but its own 6.
    assert!(marked(&x, [10 * KAS1, 0], 6 * KAS1, 0).is_ok() && marked(&x, [10 * KAS1, 0], 6 * KAS1 + 1, 0).is_err());
    // Who took out is worth that much less at every price, and the other what it was, where the pot still covers it.
    for k in 0..200 {
        let q = [10 * KAS1 + k * (KAS1 / 100), 0];
        let (before, after) = (worth(&x, SIDE_A, q), worth(&m.position, SIDE_A, q));
        if after < m.position.pot() && before < x.pot() {
            assert_eq!(after, before - 10 * KAS1, "a at {}", q[0]);
        }
    }
    // The floor is the maintenance where nothing was asked: a position that asks nothing keeps 5 % of the notional.
    let free = one(4, 10 * KAS1, 12, 12);
    let top = marked(&free, p, 16 * KAS1 - 22 * KAS1 / 10, 0).unwrap();
    assert_eq!(top.position.x.a_margin, 22 * KAS1 / 10);
    assert!(marked(&free, p, 16 * KAS1 - 22 * KAS1 / 10 + 1, 0).is_err());
}

#[test]
fn a_side_below_its_maintenance_is_not_re_marked() {
    // A long 4 from 10 with 4 KAS: at 9.4 it has 1.6 KAS and the maintenance is 1.88.
    let x = one(4, 10 * KAS1, 4, 12);
    assert!(marked(&x, [95 * KAS1 / 10, 0], 0, 0).is_ok());
    let e = marked(&x, [94 * KAS1 / 10, 0], 0, 0).unwrap_err().to_string();
    assert!(e.contains("below its maintenance"), "{e}");
    // The other side: B short with 4 KAS when the price rises.
    let y = one(4, 10 * KAS1, 12, 4);
    assert!(marked(&y, [106 * KAS1 / 10, 0], 0, 0).is_err());
    // A price of nothing, or above the market's limit.
    assert!(marked(&x, [0, 0], 0, 0).is_err());
    assert!(marked(&x, [x.x.legs[0].price_max + 1, 0], 0, 0).is_err());
}

#[test]
fn re_marks_of_random_positions_of_one_and_two_legs_keep_every_worth() {
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut done, mut refused) = (0, 0);
    for case in 0..400 {
        let mut x = samples::position();
        let two = r.range(0, 2) == 1;
        for i in 0..2 {
            let l = &mut x.x.legs[i];
            l.size = if i == 1 && !two { 0 } else { r.range(1, 30) * l.scale + r.range(0, l.scale) };
            l.dir = if r.range(0, 2) == 1 { LONG } else { SHORT };
            l.entry = r.range(5 * KAS1, 25 * KAS1);
        }
        x.x.a_margin = r.range(KAS1, 200 * KAS1);
        x.x.b_margin = r.range(KAS1, 200 * KAS1);
        x.a_min_counter = r.range(0, 2) * r.range(0, 20 * KAS1);
        x.b_min_counter = r.range(0, 2) * r.range(0, 20 * KAS1);
        if x.check().is_err() {
            continue;
        }
        let near = |r: &mut Rng, e: i64| e / 100 * r.range(80, 121) + r.range(0, 1_000);
        let p = [near(&mut r, x.x.legs[0].entry), if two { near(&mut r, x.x.legs[1].entry) } else { 0 }];
        let Ok(m) = marked(&x, p, 0, 0) else {
            refused += 1;
            continue;
        };
        done += 1;
        assert_eq!(m.position.pot(), x.pot(), "case {case}");
        assert_eq!(m.position.x.a_margin - x.x.a_margin, m.moved, "case {case}");
        for _ in 0..40 {
            let q = [near(&mut r, p[0]), if two { near(&mut r, p[1]) } else { 0 }];
            assert_eq!(worth(&m.position, SIDE_A, q), worth(&x, SIDE_A, q), "case {case}: a at {q:?}");
            assert_eq!(worth(&m.position, SIDE_B, q), worth(&x, SIDE_B, q), "case {case}: b at {q:?}");
        }
        // The most each takes out leaves it at its floor, and the two together what the pot held.
        let floor = |margin: i64, asked: i64, mm: i64| margin - mm.max(asked).min(margin);
        let notional: i64 = (0..2).filter(|i| x.x.legs[*i].size > 0).map(|i| x.x.legs[i].worth(p[i])).sum();
        let mm = x.x.maint(notional);
        let (ta, tb) = (floor(m.position.x.a_margin, x.b_min_counter, mm), floor(m.position.x.b_margin, x.a_min_counter, mm));
        let t = marked(&x, p, ta, tb).unwrap();
        assert_eq!(t.position.pot() + ta + tb, x.pot(), "case {case}");
        assert!(marked(&x, p, ta + 1, tb).is_err() && marked(&x, p, ta, tb + 1).is_err(), "case {case}: a sompi above the floor");
    }
    println!("random re-marks: {done} done, {refused} refused (a side below its maintenance at the price)");
    assert!(done >= 150 && refused >= 30, "{done} {refused}");
}
