//! The tree of a shard's dated rows (docs/ARCHITECTURE-CLEARING.md 17.8j, 2 to 4): a row listed, with its day's
//! leaf; a row and a leaf taken out; the market's futures row as its head, counting its leaves.

use kpf_protocol::books::{Books, DaySaid, Key, Row, OF_CALLS, OF_PUTS};
use kpf_protocol::cross::XTerms;
use kpf_protocol::reserve::{RowOutBy, Shard, ShardRow};

const SCALE: i64 = 100_000_000;
const LIMIT: i64 = 100_000_000_000_000;

fn terms() -> XTerms {
    XTerms { term: 6_048_000, mmr_bps: 500, g_bps: 50, liq_fee_bps: 50, keeper_fee_min: 10_000_000, carrier: 20_000_000, idle_daa: 864_000, grace_daa: 100, late_daa: 100, quiet_daa: 20 }
}

fn market(m: u8) -> [u8; 32] {
    let mut id = [0u8; 32];
    id[0] = 1 + m;
    id[20] = 7;
    id
}

fn opened() -> (Shard, Books) {
    (Shard::opened(terms(), [3; 32], 1_000 * SCALE, 0, 50, 100, [9; 32]), Books::opened())
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// What holds of every shard and its books made by `row_in` and `row_out`.
fn held(s: &Shard, b: &Books) {
    assert!(b.well_linked() && b.root() == s.root);
    let rows = b.in_order();
    for (i, r) in rows.iter().enumerate().skip(1) {
        assert!(r.key.listed_made());
        // (A day's leaves stand next to one another, the day's leaf the last of them; a row has its day's leaf.)
        if r.key.is_dated() {
            let day = b.row(&Key::day_of(r.key.market, r.key.day)).expect("a row's day has its leaf");
            assert_eq!((day.scale, day.price_max), (r.scale, r.price_max));
            assert!(r.key.same_day(&rows[i + 1].key));
        } else {
            assert!(i + 1 == rows.len() || !r.key.same_day(&rows[i + 1].key));
        }
    }
    // (A market's futures row counts its leaves, and stands while it counts any.)
    for m in 0..6u8 {
        let n = b.count_of(&market(m));
        let head = s.rows.iter().find(|x| x.key == Key::futures(market(m)));
        assert_eq!(head.map_or(0, |x| x.entered), n, "market {m}");
        assert_eq!(head.is_some(), n > 0, "market {m}: no head stands with no leaf and no unit");
    }
    assert!(s.rows.iter().filter(|x| **x != ShardRow::EMPTY).count() <= 4);
}

#[test]
fn a_row_is_listed_with_its_days_leaf_and_the_head_counts() {
    let (s, b) = opened();
    let call = Key { market: market(0), day: 5_000, of: OF_CALLS, strike: 10 * SCALE };
    // The first row of a market and day: the head takes an empty place, the day's leaf is put in with the row.
    assert!(s.row_in(&b, call, SCALE, LIMIT, 0, 1, None).is_err(), "the first row of a day without its leaf");
    let (s1, b1) = s.row_in(&b, call, SCALE, LIMIT, 2, 1, Some(5)).expect("listed");
    held(&s1, &b1);
    assert_eq!(s1.rows[2], ShardRow { entered: 2, ..ShardRow::futures(market(0), SCALE, LIMIT, 0, 0) });
    assert_eq!(b1.row(&Key::day_of(market(0), 5_000)).unwrap().day_said(), Some(DaySaid::NotWritten));
    assert_eq!(b1.in_order().iter().map(|r| r.key).collect::<Vec<_>>(), vec![Key::HEAD, call, Key::day_of(market(0), 5_000)]);
    // A second row of the day: no second leaf; by the head, at its scale and limit and at no other place.
    let put = Key { of: OF_PUTS, ..call };
    assert!(s1.row_in(&b1, put, SCALE, LIMIT, 2, 3, Some(4)).is_err(), "a second leaf of a day");
    assert!(s1.row_in(&b1, put, SCALE, LIMIT, 0, 3, None).is_err(), "an empty place while the market has its row");
    assert!(s1.row_in(&b1, put, SCALE / 10, LIMIT, 2, 3, None).is_err() && s1.row_in(&b1, put, SCALE, LIMIT / 10, 2, 3, None).is_err(), "another scale or limit than the market's");
    assert!(s1.row_in(&b1, call, SCALE, LIMIT, 2, 3, None).is_err(), "a key twice");
    assert!(s1.row_in(&b1, put, SCALE, LIMIT, 2, 1, None).is_err() && s1.row_in(&b1, put, SCALE, LIMIT, 2, 64, None).is_err(), "a place that is taken, or none");
    let (s2, b2) = s1.row_in(&b1, put, SCALE, LIMIT, 2, 3, None).expect("listed");
    held(&s2, &b2);
    assert_eq!(s2.rows[2].entered, 3);
    // Not of a shard that is called, not with other books, not for a key that is no dated row's.
    assert!(Shard { called: 7, ..s2 }.row_in(&b2, Key { strike: 11 * SCALE, ..call }, SCALE, LIMIT, 2, 9, None).is_err());
    assert!(s2.row_in(&b1, Key { strike: 11 * SCALE, ..call }, SCALE, LIMIT, 2, 9, None).is_err());
    for bad in [Key::futures(market(0)), Key::day_of(market(0), 6_000), Key { strike: 0, ..call }, Key { day: 0, ..call }, Key { strike: LIMIT + 1, ..call }, Key { market: [0; 32], ..call }] {
        assert!(s2.row_in(&b2, bad, SCALE, LIMIT, 2, 9, Some(10)).is_err() && s2.row_in(&b2, bad, SCALE, LIMIT, 2, 9, None).is_err(), "{bad:?}");
    }
    // Taken out: a row by its owner; the day's leaf only once no row of its day stands; the head leaves with the
    // last leaf.
    let day = Key::day_of(market(0), 5_000);
    assert!(s2.row_out(&b2, &day, RowOutBy::Owner).is_err(), "a day's leaf while rows of its day stand");
    assert!(s2.row_out(&b2, &call, RowOutBy::DayWritten).is_err() && s2.row_out(&b2, &call, RowOutBy::Called).is_err(), "by anyone, of a shard not called whose day is not written");
    assert!(Shard { called: 7, ..s2 }.row_out(&b2, &call, RowOutBy::Called).is_ok(), "by anyone, of a called shard");
    let (s3, b3) = s2.row_out(&b2, &call, RowOutBy::Owner).unwrap();
    let (s4, b4) = s3.row_out(&b3, &put, RowOutBy::Owner).unwrap();
    held(&s4, &b4);
    assert_eq!((s4.rows[2].entered, b4.in_order().len()), (1, 2));
    let (s5, b5) = s4.row_out(&b4, &day, RowOutBy::DayWritten).unwrap();
    held(&s5, &b5);
    assert_eq!((s5.rows, s5.root, b5.root()), (s.rows, s.root, Books::opened().root()), "the shard as it was opened");
    // A row with a unit stays; a row of a written day leaves by anyone, and no row is listed for that day.
    let mut with_unit = b2.clone();
    let at = with_unit.place_of(&call).unwrap();
    with_unit.leaves[at] = Some(Row { net: -3, gross: 3, entered: 30, ..with_unit.leaves[at].unwrap() });
    assert!(Shard { root: with_unit.root(), ..s2 }.row_out(&with_unit, &call, RowOutBy::Owner).is_err());
    let mut written = b2.clone();
    let at = written.place_of(&day).unwrap();
    written.leaves[at] = Some(Row { entered: 12 * SCALE, ..written.leaves[at].unwrap() });
    let sw = Shard { root: written.root(), ..s2 };
    assert!(sw.row_out(&written, &call, RowOutBy::DayWritten).is_ok());
    assert!(sw.row_in(&written, Key { strike: 11 * SCALE, ..call }, SCALE, LIMIT, 2, 9, None).is_err(), "a row listed for a written day");
}

#[test]
fn rows_listed_and_taken_out_at_random_keep_the_list_the_leaves_and_the_counts() {
    let mut g = Rng(0x17_8A2);
    let (mut s, mut b) = opened();
    let (mut ins, mut outs, mut refused) = (0, 0, 0);
    for _ in 0..6_000 {
        let m = g.below(6) as u8;
        let key = Key { market: market(m), day: 1_000 * (1 + g.below(3) as i64), of: if g.below(2) == 0 { OF_CALLS } else { OF_PUTS }, strike: SCALE * (1 + g.below(4) as i64) };
        if g.below(5) < 3 {
            // (Listed as its owner's tool would: the head's row or the first empty one; the day's leaf where none stands.)
            let r = s.rows.iter().position(|x| x.key == Key::futures(key.market)).or_else(|| s.rows.iter().take(4).position(|x| *x == ShardRow::EMPTY));
            let free: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_none()).collect();
            let stands = b.row(&Key::day_of(key.market, key.day)).is_some();
            let got = match (r, free.len() >= 2) {
                (Some(r), true) => s.row_in(&b, key, SCALE, LIMIT, r, free[0], (!stands).then_some(free[1])),
                _ => Err(kpf_protocol::error::Error::Invalid("no place".into())),
            };
            // (And the other way about the leaf is refused, always.)
            if let (Some(r), true) = (r, free.len() >= 2) {
                assert!(s.row_in(&b, key, SCALE, LIMIT, r, free[0], stands.then_some(free[1])).is_err());
            }
            match got {
                Ok((s2, b2)) => {
                    (s, b) = (s2, b2);
                    ins += 1;
                }
                Err(_) => refused += 1,
            }
        } else {
            let leaves: Vec<Key> = b.in_order().iter().skip(1).map(|r| r.key).collect();
            if leaves.is_empty() {
                continue;
            }
            let k = leaves[g.below(leaves.len() as u64) as usize];
            match s.row_out(&b, &k, RowOutBy::Owner) {
                Ok((s2, b2)) => {
                    (s, b) = (s2, b2);
                    outs += 1;
                }
                Err(_) => {
                    assert!(k.is_day(), "only a day's leaf whose day has a row stays: {k:?}");
                    refused += 1;
                }
            }
        }
        held(&s, &b);
    }
    println!("rows at random: {ins} listed, {outs} taken out, {refused} refused; {} leaves stand", b.in_order().len() - 1);
    assert!(ins > 500 && outs > 500 && refused > 500);
}

// ----------------------------------------------------------------------------------------------------------------
// 17.8j, A3, the model: a draw's and a call's trace over the dated rows.

use kpf_protocol::reserve::{Draw, Traced};

const P10: i64 = 1_000_000_000;

/// A shard of two markets with futures units and dated rows, some with units (written by hand: no leg enters a
/// dated row yet), and its books.
fn booked(g: &mut Rng) -> (Shard, Books) {
    let (mut s, mut b) = opened();
    for m in 0..2u8 {
        for _ in 0..1 + g.below(4) {
            let key = Key { market: market(m), day: 1_000_000 * (1 + g.below(2) as i64), of: if g.below(2) == 0 { OF_CALLS } else { OF_PUTS }, strike: P10 / 10 * (8 + g.below(5) as i64) };
            if b.row(&key).is_some() {
                continue;
            }
            let free: Vec<usize> = (0..64).filter(|i| b.leaves[*i].is_none()).collect();
            let stands = b.row(&Key::day_of(key.market, key.day)).is_some();
            (s, b) = s.row_in(&b, key, SCALE, LIMIT, m as usize, free[0], (!stands).then_some(free[1])).unwrap();
            if g.below(4) != 0 {
                let units = SCALE * (1 + g.below(20) as i64);
                let net = [units, -units, units / 2, 0][g.below(4) as usize];
                let at = b.place_of(&key).unwrap();
                b.leaves[at] = Some(Row { net, gross: units, entered: net / 7, ..b.leaves[at].unwrap() });
                s = Shard { root: b.root(), dgross: s.dgross + units, ..s };
            }
        }
        if g.below(3) != 0 {
            let units = SCALE * (1 + g.below(30) as i64);
            s.rows[m as usize] = ShardRow { net: [units, -units][g.below(2) as usize], gross: units, ..s.rows[m as usize] };
        }
    }
    (s, b)
}

/// What a dated row is asked alone is never less than what it loses, alone, at any price the move of the rate
/// reaches: the ends, the strike and a sompi either side of each.
#[test]
fn a_dated_row_is_asked_no_less_than_it_can_lose_alone() {
    let mut g = Rng(0x17_8A3);
    let (s, _) = opened();
    let (mut n, mut exact, mut none) = (0, 0, 0);
    for _ in 0..200_000 {
        let p = P10 / 2 + g.below(P10 as u64) as i64;
        let strike = p - p / 10 + g.below((p / 5) as u64) as i64;
        let units = 1 + g.below(50 * SCALE as u64) as i64;
        let row = Row { key: Key { market: market(0), day: 9, of: if g.below(2) == 0 { OF_CALLS } else { OF_PUTS }, strike }, scale: SCALE, price_max: LIMIT, net: [units, -units][g.below(2) as usize], gross: units, entered: 0, next: [0; 32] };
        let (now, _, asked) = s.dated_adds(&row, p, false).unwrap();
        let (l, u) = kpf_protocol::crow::ends(p, s.t.mmr_bps + s.t.g_bps).unwrap();
        let mut worst = 0;
        for x in [l, l + 1, u - 1, u, strike - 1, strike, strike + 1, p] {
            if x >= l && x <= u {
                worst = worst.max(now - s.dated_adds(&row, x, false).unwrap().0);
            }
        }
        assert!(asked >= worst, "{row:?} at {p}: asked {asked}, it loses {worst}");
        n += 1;
        exact += (asked == worst) as i64;
        none += (asked == 0) as i64;
    }
    println!("{n} dated rows alone: {exact} asked exactly what they can lose ({none} of them nothing)");
    assert!(exact > n / 2 && none > 1_000 && none < n / 2);
}

#[test]
fn a_trace_over_dated_rows_is_judged_a_row_at_a_time_and_leaves_none_out() {
    let mut g = Rng(0x17_8A4);
    let t0 = 5_000;
    let (mut right, mut wrong, mut traces) = (0, 0, 0);
    for round in 0..300 {
        let (s, b) = booked(&mut g);
        let prices = [P10 - P10 / 20 + g.below((P10 / 10) as u64) as i64, P10 + g.below((P10 / 10) as u64) as i64, 0, 0];
        let d = Draw { amount: 1, prices: prices.into() };
        for call in [false, true] {
            let t = s.traced_with(&b, &d, call, t0).expect("a trace");
            let nd = t.dated.len();
            // (The head's leaf is the first row where the tree has any other.)
            assert_eq!(nd, if b.in_order().len() > 1 { b.in_order().len() } else { 0 });
            // (What is said reads back; with no leaf it is the statement of today to the byte.)
            assert_eq!(Traced::read(&t.said()).as_ref(), Some(&t), "round {round}");
            assert_eq!(t.said().len(), 184 + 56 * t.list.len() + if nd == 0 { 0 } else { 9 + 80 * nd });
            for k in 1..=nd + 1 {
                assert_eq!(s.trace_dated_judged(&b, &t, k, call, t0), Some(true), "round {round}: row {k} of {nd}");
                right += 1;
            }
            for at in 1..=4 {
                assert_eq!(s.trace_row_judged(&t, at, call), Some(true), "round {round}: the futures row {at}");
            }
            // The sums of the last row are the sums over every row of the shard.
            let last = t.dated.last().map_or((t.rows[3].wg, t.rows[3].sn), |r| (r.wg, r.sn));
            let mut sn = t.rows[3].sn;
            let mut wg = t.rows[3].wg;
            for leaf in b.in_order().iter().skip(1).filter(|r| r.gross != 0) {
                let p = prices[if leaf.key.market == market(0) { 0 } else { 1 }];
                let (x, y, _) = s.dated_adds(leaf, p, call).unwrap();
                sn += x;
                wg += y;
            }
            assert_eq!(last, (wg, sn));
            if nd == 0 {
                continue;
            }
            traces += 1;
            // Each number of a dated row moved: wrong at its row.
            let k = 1 + g.below(nd as u64) as usize;
            for which in 0..7 {
                let mut lie = t.clone();
                let r = &mut lie.dated[k - 1];
                match which {
                    0 => r.place = (r.place + 1) % 64,
                    1 => r.wn += 1,
                    2 => r.wg += 1,
                    3 => r.sn -= 1,
                    4 => r.ro += 1,
                    5 => r.nk[0] ^= 1,
                    _ => r.at += 1,
                }
                let got = s.trace_dated_judged(&b, &lie, k, call, t0);
                let with_unit = b.leaves[t.dated[k - 1].place as usize].unwrap().gross != 0;
                if which == 6 && !with_unit {
                    // (The place of a price is read by nothing where the leaf has no unit.)
                    assert_eq!(got, Some(true));
                } else {
                    assert_eq!(got, Some(false), "round {round}: number {which} of row {k}");
                    wrong += 1;
                }
            }
            // A row left out: the row after it is of another key than the one carried, and is wrong; the last row
            // left out: the point beyond the last is wrong. A row said twice: wrong at the second.
            let mut cut = t.clone();
            cut.dated.remove(k - 1);
            assert_eq!(s.trace_dated_judged(&b, &cut, k, call, t0), Some(false), "round {round}: row {k} left out");
            let mut twice = t.clone();
            twice.dated.insert(k - 1, t.dated[k - 1]);
            assert_eq!(s.trace_dated_judged(&b, &twice, k + 1, call, t0), Some(false), "round {round}: row {k} twice");
            // A trace that says no dated row at all: wrong at the first point.
            let mut blind = t.clone();
            blind.dated.clear();
            assert_eq!(s.trace_dated_judged(&b, &blind, 1, call, t0), Some(false));
            wrong += 3;
        }
        // An ended day with a unit: a draw is wrong at its row; the row of a call is of another script.
        if let Some((k, leaf)) = b.in_order().iter().skip(1).enumerate().find(|(_, r)| r.gross != 0) {
            let t = s.traced_with(&b, &d, false, t0).unwrap();
            assert!(s.traced_with(&b, &d, false, leaf.key.day).is_none());
            assert_eq!(s.trace_dated_judged(&b, &t, k + 2, false, leaf.key.day), Some(false));
            assert_eq!(s.trace_dated_judged(&b, &t, k + 2, true, leaf.key.day), None);
        }
    }
    println!("traces over dated rows: {traces} with rows; {right} points right, {wrong} lies wrong");
    assert!(traces > 300 && wrong > 2_000);
}
