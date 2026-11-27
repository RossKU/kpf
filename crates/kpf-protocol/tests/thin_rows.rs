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
