//! A shard's books as a tree (docs/ARCHITECTURE-CLEARING.md 17.6, 17.10): the model, with no script yet. The rows
//! are the leaves of a tree of a fixed depth; their order is the order of their keys, kept as a list (each leaf
//! names the key of the leaf after it), not their places in the tree.
//!
//! - A [`Key`] is written plain, 60 bytes: the underlying's market, the day, what the row is of (futures, calls,
//!   puts) and the strike. It says no committee: a shard has one, in its id. An underlying is its market.
//! - A [`Row`] is a key, its market's scale and price's limit (one of each a market: a market's rows have its
//!   futures row's), the clearing side's net units, the gross, what its legs were entered at (kept for a row with
//!   a day only), and the hash of the key of the leaf after it. Its leaf is its hash under a tag of its own.
//! - The head leaf, of the key of nothing, is in every tree: no tree is empty, and every row has a leaf before it.
//!   It is below every key by rule, not by its numbers.
//! - A market's futures row (no day, no strike) is its first row and is put in before any other of the market; a
//!   row with a day is put in after a row of its market, whose scale and limit it has; the futures row is taken
//!   out only where no other row of the market stands. So one scale and one limit a market are held by showing one
//!   leaf, the one before.
//! - [`Books::hash`] is what a shard's state holds of its books, in the form that is built for four rows
//!   (17.8a, 1): the hash of the sum of the rows' gross units, of the root (where the hash of the four rows
//!   stands), and of the hash of the claims' numbers.
//!
//! What a script will be shown is here as [`Shown`]: a row, its place and the six hashes beside its way to the root.

use crate::committee::blake2b;
use crate::cross::XLeg;
use crate::error::{Error, Result};
use crate::state::sm8;

/// The depth of the tree: sixty-four leaves, the head among them.
pub const DEPTH: usize = 6;
pub const LEAVES: usize = 1 << DEPTH;
/// The tags a leaf and an inner node are hashed under (a row of chosen bytes is no inner node).
pub const LEAF_TAG: i64 = 20;
pub const NODE_TAG: i64 = 21;
/// What a row is of.
pub const OF_FUTURES: i64 = 0;
pub const OF_CALLS: i64 = 1;
pub const OF_PUTS: i64 = 2;
/// An empty leaf.
pub const EMPTY: [u8; 32] = [0; 32];

fn p33(h: &[u8; 32]) -> Vec<u8> {
    [&[0x20][..], h].concat()
}

fn i9(v: i64) -> Vec<u8> {
    [&[0x08][..], &sm8(v)].concat()
}

/// Eight bytes as a script reads a number: the magnitude, and the top bit its sign.
fn num(b: &[u8]) -> i64 {
    let mut x: [u8; 8] = b.try_into().expect("eight bytes");
    let below = x[7] & 0x80 != 0;
    x[7] &= 0x7f;
    let m = i64::from_le_bytes(x);
    if below { -m } else { m }
}

fn invalid<T>(why: &str) -> Result<T> {
    Err(Error::Invalid(format!("the books: {why}")))
}

/// The key of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub market: [u8; 32],
    pub day: i64,
    pub of: i64,
    pub strike: i64,
}

impl Key {
    /// The key of nothing: the head leaf's, and what the last leaf names as the key after it.
    pub const HEAD: Key = Key { market: [0; 32], day: 0, of: 0, strike: 0 };

    /// The futures row of a market: its first.
    pub fn futures(market: [u8; 32]) -> Key {
        Key { market, day: 0, of: OF_FUTURES, strike: 0 }
    }

    /// The row a futures leg is counted in.
    pub fn of_leg(l: &XLeg) -> Key {
        Key::futures(l.market)
    }

    /// 60 bytes.
    pub fn bytes(&self) -> Vec<u8> {
        [p33(&self.market), i9(self.day), i9(self.of), i9(self.strike)].concat()
    }

    pub fn hash(&self) -> [u8; 32] {
        blake2b(&self.bytes())
    }

    /// The seven numbers a key is ordered by: the market's 32 bytes as four numbers of 8 bytes, as a script reads
    /// them, then the day, what it is of, the strike. Any total order would do; this one puts the rows of one
    /// underlying next to one another, its futures row first.
    pub fn numbers(&self) -> [i64; 7] {
        let m = |i: usize| num(&self.market[8 * i..8 * i + 8]);
        [m(0), m(1), m(2), m(3), self.day, self.of, self.strike]
    }

    /// The key of nothing is below every key; the others are in the order of their numbers. (A market's eight
    /// bytes with their top bit set read as a number below nothing: by the numbers alone such a key would be
    /// below the head's. Review T4, T1.)
    pub fn below(&self, o: &Key) -> bool {
        if *self == Key::HEAD || *o == Key::HEAD {
            return *self == Key::HEAD && *o != Key::HEAD;
        }
        self.numbers() < o.numbers()
    }

    /// Whether it is a market's futures row.
    pub fn is_futures(&self) -> bool {
        (self.day, self.of, self.strike) == (0, OF_FUTURES, 0)
    }

    /// A key a row can have: a market; its futures row, or a row of calls or puts with a day and a strike.
    pub fn well_made(&self) -> bool {
        // (Two ids that differ in the sign of a number of nothing are one key by their numbers: neither is one.)
        let plain = (0..4).all(|i| self.market[8 * i..8 * i + 8] != [0, 0, 0, 0, 0, 0, 0, 0x80]);
        // (And the market of nothing is the head's.)
        self.market != [0; 32] && plain && (self.is_futures() || ((self.of == OF_CALLS || self.of == OF_PUTS) && self.day > 0 && self.strike > 0))
    }
}

/// A row of the books.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub key: Key,
    /// Its market's scale and price's limit: one of each a market.
    pub scale: i64,
    pub price_max: i64,
    /// The clearing side's net units, and the gross.
    pub net: i64,
    pub gross: i64,
    /// What the row's legs were entered at, signed as the clearing side's: kept for a row with a day (what a call
    /// counts it at where its day's price was never written); nothing for a futures row, whose legs' entries are
    /// in the one number over all rows.
    pub entered: i64,
    /// The hash of the key of the leaf after this one (of the key of nothing: none is after).
    pub next: [u8; 32],
}

impl Row {
    /// 147 bytes: the tag, the key, five numbers, the hash of the key after.
    pub fn bytes(&self) -> Vec<u8> {
        [i9(LEAF_TAG), self.key.bytes(), i9(self.scale), i9(self.price_max), i9(self.net), i9(self.gross), i9(self.entered), p33(&self.next)].concat()
    }

    pub fn leaf(&self) -> [u8; 32] {
        blake2b(&self.bytes())
    }

    pub fn has_units(&self) -> bool {
        self.gross != 0
    }
}

fn node(l: &[u8; 32], r: &[u8; 32]) -> [u8; 32] {
    blake2b(&[i9(NODE_TAG), l.to_vec(), r.to_vec()].concat())
}

/// The hash of a half of no leaf, at each height (the first: an empty leaf).
fn empties() -> [[u8; 32]; DEPTH + 1] {
    let mut e = [EMPTY; DEPTH + 1];
    for d in 0..DEPTH {
        e[d + 1] = node(&e[d], &e[d]);
    }
    e
}

/// The level above a level (one of the `empties` where both halves are).
fn above(level: &[[u8; 32]], e: &[[u8; 32]; DEPTH + 1], d: usize) -> Vec<[u8; 32]> {
    level.chunks(2).map(|c| if c[0] == e[d] && c[1] == e[d] { e[d + 1] } else { node(&c[0], &c[1]) }).collect()
}

/// A row as a script is shown it: the row, its place among the leaves, and the hashes beside its way to the root
/// (the lowest first).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub row: Row,
    pub place: usize,
    pub beside: [[u8; 32]; DEPTH],
}

/// The root a leaf at a place gives with the hashes beside its way.
pub fn root_of(leaf: &[u8; 32], place: usize, beside: &[[u8; 32]; DEPTH]) -> [u8; 32] {
    if place >= LEAVES {
        // (A place of 64 + p would open what p opens: no place beyond the leaves opens anything. Review T8, P3.)
        return [0xff; 32];
    }
    let mut h = *leaf;
    for (d, b) in beside.iter().enumerate() {
        h = if place >> d & 1 == 0 { node(&h, b) } else { node(b, &h) };
    }
    h
}

impl Shown {
    pub fn root(&self) -> [u8; 32] {
        root_of(&self.row.leaf(), self.place, &self.beside)
    }
}

/// The books: the leaves. (The sum of the rows' gross units and the claims' numbers, which are hashed with the
/// root, are the shard's: `Books::hash` takes the claims' hash.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Books {
    pub leaves: Vec<Option<Row>>,
}

impl Default for Books {
    fn default() -> Self {
        Books::opened()
    }
}

impl Books {
    /// The books of a shard as it is opened: the head leaf, at the first place.
    pub fn opened() -> Books {
        let mut leaves = vec![None; LEAVES];
        leaves[0] = Some(Row { key: Key::HEAD, scale: 0, price_max: 0, net: 0, gross: 0, entered: 0, next: Key::HEAD.hash() });
        Books { leaves }
    }

    fn level0(&self) -> Vec<[u8; 32]> {
        self.leaves.iter().map(|l| l.map_or(EMPTY, |r| r.leaf())).collect()
    }

    pub fn root(&self) -> [u8; 32] {
        let (mut level, e) = (self.level0(), empties());
        for d in 0..DEPTH {
            level = above(&level, &e, d);
        }
        level[0]
    }

    /// The sum of the rows' gross units: a call ends where it is nothing.
    pub fn gross(&self) -> i64 {
        self.leaves.iter().flatten().fold(0i64, |a, r| a.saturating_add(r.gross))
    }

    /// What a shard's state holds of its books, with the hash of its claims' numbers (docs 17.8a, 1).
    pub fn hash(&self, claims: &[u8; 32]) -> [u8; 32] {
        blake2b(&[i9(self.gross()), p33(&self.root()), p33(claims)].concat())
    }

    /// The hashes beside a place's way to the root.
    pub fn beside(&self, place: usize) -> [[u8; 32]; DEPTH] {
        let mut out = [[0u8; 32]; DEPTH];
        let (mut level, mut i, e) = (self.level0(), place, empties());
        for (d, o) in out.iter_mut().enumerate() {
            *o = level[i ^ 1];
            level = above(&level, &e, d);
            i >>= 1;
        }
        out
    }

    pub fn place_of(&self, key: &Key) -> Option<usize> {
        self.leaves.iter().position(|l| l.is_some_and(|r| r.key == *key))
    }

    pub fn row(&self, key: &Key) -> Option<Row> {
        self.place_of(key).and_then(|i| self.leaves[i])
    }

    pub fn shown(&self, key: &Key) -> Option<Shown> {
        let place = self.place_of(key)?;
        Some(Shown { row: self.leaves[place]?, place, beside: self.beside(place) })
    }

    /// The rows in the order of their keys, the head first. (In books made by `put` and `out` each is the one
    /// the row before names: `well_linked`.)
    pub fn in_order(&self) -> Vec<Row> {
        let mut out: Vec<Row> = self.leaves.iter().flatten().copied().collect();
        out.sort_by(|a, b| if a.key == b.key { std::cmp::Ordering::Equal } else if a.key.below(&b.key) { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater });
        out
    }

    /// Whether the rows are a list from the head: each names the one after it in the keys' order, the last the
    /// key of nothing, and no key is twice.
    pub fn well_linked(&self) -> bool {
        let rows = self.in_order();
        rows.first().is_some_and(|r| r.key == Key::HEAD)
            && rows.windows(2).all(|w| w[0].key.below(&w[1].key) && w[0].next == w[1].key.hash())
            && rows.last().is_some_and(|r| r.next == Key::HEAD.hash())
    }

    /// The leaf before a key that is not in the books: the one whose key is below it and that names a key above
    /// it, or none, as the key after.
    pub fn before(&self, key: &Key) -> Option<Row> {
        let rows = self.in_order();
        rows.iter().rev().find(|r| r.key.below(key)).copied().filter(|_| self.place_of(key).is_none())
    }

    /// A row of no unit put in for a key the books do not have, at a place that is empty: the leaf before names
    /// it, and it names what the leaf before named. A market's futures row is put in with the market's scale and
    /// limit, where the leaf before is of another market; a row with a day where the leaf before is of its
    /// market, whose scale and limit it has. (What a script checks, shown the leaf before and the key it names
    /// plain: the leaf before opens the root; its key is below the key; the key it names is above the key or is
    /// the key of nothing; the place is empty in the root as the leaf before rewrote it.)
    pub fn put(&self, key: Key, scale: i64, price_max: i64, place: usize) -> Result<Books> {
        if !key.well_made() || scale <= 0 || price_max <= 0 {
            return invalid("a row's key is a market's futures row, or its calls or puts of a day and a strike; its scale and limit are above nothing");
        }
        if place >= LEAVES || self.leaves[place].is_some() {
            return invalid("the place is not an empty one");
        }
        let Some(before) = self.before(&key) else { return invalid("the key is in the books already") };
        let same = before.key.market == key.market && before.key != Key::HEAD;
        if key.is_futures() == same || (same && (before.scale, before.price_max) != (scale, price_max)) {
            return invalid("a market's futures row is its first; a row with a day comes after a row of its market, and has its scale and limit");
        }
        let mut b = self.clone();
        let at = self.place_of(&before.key).expect("the leaf before is a leaf");
        b.leaves[place] = Some(Row { key, scale, price_max, net: 0, gross: 0, entered: 0, next: before.next });
        b.leaves[at] = Some(Row { next: key.hash(), ..before });
        Ok(b)
    }

    /// The first empty place.
    pub fn free(&self) -> Option<usize> {
        self.leaves.iter().position(|l| l.is_none())
    }

    /// A row's numbers changed by a leg that enters (the clearing side's units, the leg's size, what it was
    /// entered at) or, with the three the other way, by one that leaves. What the legs were entered at is kept
    /// in a row with a day only.
    pub fn moved(&self, key: &Key, net: i64, gross: i64, entered: i64) -> Result<Books> {
        let Some(place) = self.place_of(key) else { return invalid("no row of the key") };
        let r = self.leaves[place].expect("a row");
        let beyond = || Error::Invalid("the books: a row is beyond what is counted".into());
        let next = Row {
            net: r.net.checked_add(net).filter(|x| *x != i64::MIN).ok_or_else(beyond)?,
            gross: r.gross.checked_add(gross).ok_or_else(beyond)?,
            entered: if key.is_futures() { 0 } else { r.entered.checked_add(entered).filter(|x| *x != i64::MIN).ok_or_else(beyond)? },
            ..r
        };
        if next.gross < 0 {
            return invalid("a row has fewer units than leave it");
        }
        let mut b = self.clone();
        b.leaves[place] = Some(next);
        Ok(b)
    }

    /// A row of no unit taken out: the leaf before it names what it named, and its place is empty. A market's
    /// futures row only where the row after it is of another market, or none. (By anyone, for a row whose day has
    /// passed; by the shard's owner, for a futures row: that is the script's to hold.)
    pub fn out(&self, key: &Key) -> Result<Books> {
        let Some(place) = self.place_of(key) else { return invalid("no row of the key") };
        let r = self.leaves[place].expect("a row");
        if *key == Key::HEAD || r.gross != 0 || r.net != 0 || r.entered != 0 {
            return invalid("the head stays, and a row that has a unit or anything entered stays");
        }
        if key.is_futures() && self.leaves.iter().flatten().any(|x| x.key.hash() == r.next && x.key.market == key.market) {
            return invalid("a market's futures row stays while another row of the market stands");
        }
        let Some(at) = self.leaves.iter().position(|l| l.is_some_and(|x| x.next == key.hash())) else { return invalid("no leaf names the row") };
        let mut b = self.clone();
        let before = b.leaves[at].expect("a row");
        b.leaves[at] = Some(Row { next: r.next, ..before });
        b.leaves[place] = None;
        Ok(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
        fn market(&mut self) -> [u8; 32] {
            // (A few markets, some with the top bit of a number set.)
            let mut market = [0u8; 32];
            let m = self.below(6) as u8;
            // (No number of an id is nothing written below zero: such an id is no market's, `Key::well_made`.)
            market[0] = 1 + m;
            market[15] = if m % 2 == 0 { 0x80 | (1 + m) } else { 1 + m };
            market[24] = 5;
            market[31] = if m >= 3 { 0x80 } else { 0 };
            market
        }
        fn key(&mut self) -> Key {
            let market = self.market();
            match self.below(3) as i64 {
                0 => Key::futures(market),
                of => Key { market, day: 1_000 * (1 + self.below(3) as i64), of, strike: 10 * (1 + self.below(4) as i64) },
            }
        }
    }

    fn scale_of(market: &[u8; 32]) -> (i64, i64) {
        (100_000_000 * (1 + (market[0] % 2) as i64), 1_000_000 + market[0] as i64)
    }

    /// What holds of any books: the list from the head is every row once, in the order of the keys; the rows of
    /// one market are next to one another, its futures row first, with one scale and one limit; the sum of the
    /// gross is the rows'; and every row opens the root.
    fn holds(b: &Books, every: bool) {
        let rows = b.in_order();
        assert!(b.well_linked(), "a list from the head");
        assert_eq!(rows.len(), b.leaves.iter().flatten().count(), "every row once");
        assert_eq!(rows[0].key, Key::HEAD);
        assert!(rows.windows(2).all(|w| w[0].key.below(&w[1].key) && w[0].next == w[1].key.hash()), "in the order of the keys");
        assert_eq!(rows.last().unwrap().next, Key::HEAD.hash());
        for (i, r) in rows.iter().enumerate().skip(1) {
            let first = rows.iter().position(|x| x.key.market == r.key.market).unwrap();
            let last = rows.iter().rposition(|x| x.key.market == r.key.market).unwrap();
            assert!(rows[first..=last].iter().all(|x| x.key.market == r.key.market && (x.scale, x.price_max) == (r.scale, r.price_max)), "one market, next to one another, one scale and one limit");
            assert!(rows[first].key.is_futures(), "a market's first row is its futures row");
            assert!(i >= first && r.key.well_made() && (!r.key.is_futures() || r.entered == 0));
        }
        assert_eq!(b.gross(), rows.iter().map(|r| r.gross).sum::<i64>());
        let root = b.root();
        for (i, r) in rows.iter().enumerate() {
            if every || i == rows.len() / 2 {
                assert_eq!(b.shown(&r.key).unwrap().root(), root, "a row opens the root");
            }
        }
    }

    #[test]
    fn the_books_keep_their_rows_in_the_order_of_their_keys_whatever_is_put_in_and_taken_out() {
        let mut r = Rng(0x17_0006);
        let (mut put, mut refused, mut out) = (0, 0, 0);
        for _ in 0..12 {
            let mut b = Books::opened();
            holds(&b, true);
            for _ in 0..160 {
                let key = r.key();
                let (scale, limit) = scale_of(&key.market);
                match (b.place_of(&key), r.below(4)) {
                    (None, _) => {
                        let free: Vec<usize> = (0..LEAVES).filter(|i| b.leaves[*i].is_none()).collect();
                        if free.is_empty() {
                            // (Sixty-four leaves: a full tree takes no key more, at any place.)
                            assert!((0..LEAVES).all(|i| b.put(key, scale, limit, i).is_err()));
                            continue;
                        }
                        // (At any empty place: the order is the keys', not the places'.)
                        let place = free[r.below(free.len() as u64) as usize];
                        let has_futures = b.place_of(&Key::futures(key.market)).is_some();
                        if !key.is_futures() && !has_futures {
                            assert!(b.put(key, scale, limit, place).is_err(), "a row with a day before its market's futures row");
                            refused += 1;
                            continue;
                        }
                        let before = b.before(&key).unwrap();
                        assert!(b.put(key, scale + 1, limit, place).is_err() || key.is_futures(), "another scale than its market's");
                        let next = b.put(key, scale, limit, place).unwrap();
                        assert_ne!(next.root(), b.root());
                        assert!((0..LEAVES).all(|i| next.put(key, scale, limit, i).is_err()), "no key twice");
                        assert_eq!((next.row(&before.key).unwrap().next, next.row(&key).unwrap().next), (key.hash(), before.next));
                        b = next;
                        put += 1;
                    }
                    (Some(_), 0 | 1) => {
                        let size = 1 + r.below(50) as i64;
                        let long = r.below(2) == 0;
                        b = b.moved(&key, if long { -size } else { size }, size, if long { -size * 7 } else { size * 7 }).unwrap();
                    }
                    (Some(_), 2) => {
                        // (Every unit leaves, as it entered: the row is of no unit and nothing entered.)
                        let row = b.row(&key).unwrap();
                        if row.has_units() {
                            assert!(b.out(&key).is_err(), "a row that has a unit stays");
                            assert!(b.moved(&key, 0, -row.gross - 1, 0).is_err(), "no more units leave than the row has");
                            b = b.moved(&key, -row.net, -row.gross, -row.entered).unwrap();
                        }
                    }
                    (Some(_), _) => {
                        if !b.row(&key).unwrap().has_units() {
                            let others = b.leaves.iter().flatten().any(|x| x.key.market == key.market && x.key != key);
                            if key.is_futures() && others {
                                assert!(b.out(&key).is_err(), "a market's futures row stays while another of its rows stands");
                                refused += 1;
                            } else {
                                let root = b.root();
                                b = b.out(&key).unwrap();
                                assert!(b.row(&key).is_none() && b.root() != root);
                                out += 1;
                            }
                        }
                    }
                }
                holds(&b, false);
            }
            holds(&b, true);
        }
        assert!(put > 100 && refused > 20 && out > 20, "{put} put in, {refused} refused, {out} taken out");
    }

    #[test]
    fn a_row_shown_at_another_place_or_changed_does_not_open_the_root() {
        let mut r = Rng(0x17_0007);
        let mut b = Books::opened();
        while b.leaves.iter().flatten().count() < 20 {
            let key = r.key();
            let (scale, limit) = scale_of(&key.market);
            if b.place_of(&key).is_none() {
                if let Ok(x) = b.put(key, scale, limit, b.free().unwrap()) {
                    b = x.moved(&key, -5, 5, -35).unwrap();
                }
            }
        }
        let root = b.root();
        for row in b.in_order() {
            let s = b.shown(&row.key).unwrap();
            assert_eq!(s.root(), root);
            assert_ne!(Shown { place: s.place ^ 1, ..s }.root(), root, "at another place");
            assert_ne!(Shown { place: (s.place + 32) % LEAVES, ..s }.root(), root, "at another place");
            assert_ne!(Shown { place: s.place + LEAVES, ..s }.root(), root, "at a place beyond the leaves");
            assert_ne!(Shown { row: Row { net: s.row.net + 1, ..s.row }, ..s }.root(), root, "a unit more");
            assert_ne!(Shown { row: Row { scale: s.row.scale + 1, ..s.row }, ..s }.root(), root, "another scale");
            assert_ne!(Shown { row: Row { next: Key::HEAD.hash(), ..s.row }, ..s }.root() == root, row.next != Key::HEAD.hash(), "another key after");
            // (A leaf is no inner node: two leaves' hashes shown as a row's bytes open nothing.)
            assert_ne!(root_of(&node(&s.row.leaf(), &s.beside[0]), s.place >> 1, &[s.beside[1], s.beside[2], s.beside[3], s.beside[4], s.beside[5], [0; 32]]), root);
        }
        // The books' hash moves with the sum of the gross, with the root, and with the claims' hash.
        let h = b.hash(&[1; 32]);
        assert_ne!(b.hash(&[2; 32]), h);
        let key = b.in_order()[1].key;
        assert_ne!(b.moved(&key, 0, 1, 0).unwrap().hash(&[1; 32]), h);
        // A full tree takes no row more; the head is never taken out; and a key that is not well made is no row's.
        let mut full = Books::opened();
        let mut n = 0i64;
        while full.free().is_some() {
            n += 1;
            let mut market = [0u8; 32];
            market[..8].copy_from_slice(&sm8(n));
            full = full.put(Key::futures(market), 1, 1, full.free().unwrap()).unwrap();
        }
        assert_eq!(full.in_order().len(), LEAVES);
        assert!(full.out(&Key::HEAD).is_err());
        let mut twin = [0u8; 32];
        twin[7] = 0x80;
        for bad in [Key::HEAD, Key::futures(twin), Key { market: [0; 32], day: 9, of: OF_CALLS, strike: 5 }, Key { market: [7; 32], day: 0, of: OF_CALLS, strike: 5 }, Key { market: [7; 32], day: 9, of: 3, strike: 5 }, Key { market: [7; 32], day: 9, of: OF_PUTS, strike: 0 }] {
            assert!(Books::opened().put(bad, 1, 1, 1).is_err(), "{bad:?}");
        }
    }
}
