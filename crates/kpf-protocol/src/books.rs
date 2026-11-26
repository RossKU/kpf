//! A shard's books as a tree (docs/ARCHITECTURE-CLEARING.md 17.6): the model, with no script yet. The rows are the
//! leaves of a tree of a fixed depth; their order is the order of their keys, kept as a list (each leaf names the
//! key of the leaf after it), not their places in the tree.
//!
//! - A [`Key`] is written plain, 78 bytes: the underlying's market, the scale, the price's limit, the day, what the
//!   row is of (futures, calls, puts) and the strike. It says no committee: a shard has one, in its id.
//! - A [`Row`] is a key, the clearing side's net units, the gross, what its legs were entered at, and the hash of
//!   the key of the leaf after it. Its leaf is its hash under a tag of its own.
//! - The head leaf, of the key of nothing, is in every tree: no tree is empty, and every row has a leaf before it.
//! - [`Books::hash`] is what a shard's state holds where it held the hash of four rows: the hash of the count of
//!   rows that have a unit, of what a called shard has paid to its claims, and of the root.
//!
//! What a script will be shown is here as [`Shown`]: a row, its place and the six hashes beside its way to the root.

use crate::committee::blake2b;
use crate::cross::XLeg;
use crate::error::{Error, Result};
use crate::state::sm8;

/// The depth of the tree: sixty-four leaves.
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
    pub scale: i64,
    pub price_max: i64,
    pub day: i64,
    pub of: i64,
    pub strike: i64,
}

impl Key {
    /// The key of nothing: the head leaf's, and what the last leaf names as the key after it.
    pub const HEAD: Key = Key { market: [0; 32], scale: 0, price_max: 0, day: 0, of: 0, strike: 0 };

    /// The row a futures leg is counted in.
    pub fn futures(l: &XLeg) -> Key {
        Key { market: l.market, scale: l.scale, price_max: l.price_max, day: 0, of: OF_FUTURES, strike: 0 }
    }

    /// 78 bytes.
    pub fn bytes(&self) -> Vec<u8> {
        [p33(&self.market), i9(self.scale), i9(self.price_max), i9(self.day), i9(self.of), i9(self.strike)].concat()
    }

    pub fn hash(&self) -> [u8; 32] {
        blake2b(&self.bytes())
    }

    /// The nine numbers a key is ordered by: the market's 32 bytes as four numbers of 8 bytes, as a script reads
    /// them, then the scale, the limit, the day, what it is of, the strike. Any total order would do; this one
    /// puts the rows of one underlying next to one another, its futures first.
    pub fn numbers(&self) -> [i64; 9] {
        let m = |i: usize| num(&self.market[8 * i..8 * i + 8]);
        [m(0), m(1), m(2), m(3), self.scale, self.price_max, self.day, self.of, self.strike]
    }

    /// The key of nothing is below every key; the others are in the order of their numbers. (A market's eight
    /// bytes with their top bit set read as a number below nothing: by the numbers alone such a key would be
    /// below the head's.)
    pub fn below(&self, o: &Key) -> bool {
        if *self == Key::HEAD || *o == Key::HEAD {
            return *self == Key::HEAD && *o != Key::HEAD;
        }
        self.numbers() < o.numbers()
    }

    /// Of the same underlying: the market, the scale and the price's limit.
    pub fn same_underlying(&self, o: &Key) -> bool {
        (self.market, self.scale, self.price_max) == (o.market, o.scale, o.price_max)
    }
}

/// A row of the books.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub key: Key,
    /// The clearing side's net units, and the gross.
    pub net: i64,
    pub gross: i64,
    /// What the row's legs were entered at, signed as the clearing side's.
    pub entered: i64,
    /// The hash of the key of the leaf after this one (of the key of nothing: none is after).
    pub next: [u8; 32],
}

impl Row {
    /// 147 bytes.
    pub fn bytes(&self) -> Vec<u8> {
        [i9(LEAF_TAG), self.key.bytes(), i9(self.net), i9(self.gross), i9(self.entered), p33(&self.next)].concat()
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

/// The books: the leaves, and the two numbers hashed with their root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Books {
    pub leaves: Vec<Option<Row>>,
    /// The rows that have a unit.
    pub count: i64,
    /// What a called shard has paid to its claims.
    pub paid: i64,
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
        leaves[0] = Some(Row { key: Key::HEAD, net: 0, gross: 0, entered: 0, next: Key::HEAD.hash() });
        Books { leaves, count: 0, paid: 0 }
    }

    fn level0(&self) -> Vec<[u8; 32]> {
        self.leaves.iter().map(|l| l.map_or(EMPTY, |r| r.leaf())).collect()
    }

    pub fn root(&self) -> [u8; 32] {
        let mut level = self.level0();
        while level.len() > 1 {
            level = level.chunks(2).map(|c| node(&c[0], &c[1])).collect();
        }
        level[0]
    }

    /// What a shard's state holds of its books.
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[i9(self.count), i9(self.paid), p33(&self.root())].concat())
    }

    /// The hashes beside a place's way to the root.
    pub fn beside(&self, place: usize) -> [[u8; 32]; DEPTH] {
        let mut out = [[0u8; 32]; DEPTH];
        let (mut level, mut i) = (self.level0(), place);
        for o in out.iter_mut() {
            *o = level[i ^ 1];
            level = level.chunks(2).map(|c| node(&c[0], &c[1])).collect();
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

    /// The rows in the order of their keys, the head first: each is the one the row before names.
    pub fn in_order(&self) -> Vec<Row> {
        let mut out = vec![];
        let mut at = self.row(&Key::HEAD);
        while let Some(r) = at {
            out.push(r);
            if r.next == Key::HEAD.hash() || out.len() > LEAVES {
                break;
            }
            at = self.leaves.iter().flatten().find(|x| x.key.hash() == r.next).copied();
        }
        out
    }

    /// The leaf before a key that is not in the books: the one whose key is below it and that names a key above
    /// it, or none, as the key after.
    pub fn before(&self, key: &Key) -> Option<Row> {
        let rows = self.in_order();
        rows.iter().rev().find(|r| r.key.below(key)).copied().filter(|_| self.place_of(key).is_none())
    }

    /// A row of no unit put in for a key the books do not have, at a place that is empty: the leaf before names
    /// it, and it names what the leaf before named. (What a script checks: the leaf before opens the root; its
    /// key is below the key, and the key it names, shown plain, is above the key or is the key of nothing; the
    /// place is empty in the root as the leaf before rewrote it.)
    pub fn put(&self, key: Key, place: usize) -> Result<Books> {
        if key == Key::HEAD || key.scale <= 0 {
            return invalid("the key of nothing is the head's, and a row's scale is above nothing");
        }
        if place >= LEAVES || self.leaves[place].is_some() {
            return invalid("the place is not an empty one");
        }
        let Some(before) = self.before(&key) else { return invalid("the key is in the books already") };
        let mut b = self.clone();
        let at = self.place_of(&before.key).expect("the leaf before is a leaf");
        b.leaves[place] = Some(Row { key, net: 0, gross: 0, entered: 0, next: before.next });
        b.leaves[at] = Some(Row { next: key.hash(), ..before });
        Ok(b)
    }

    /// The first empty place.
    pub fn free(&self) -> Option<usize> {
        self.leaves.iter().position(|l| l.is_none())
    }

    /// A row's numbers changed by a leg that enters (the clearing side's units, the leg's size, what it was
    /// entered at) or, with the three the other way, by one that leaves. The count follows the gross across
    /// nothing.
    pub fn moved(&self, key: &Key, net: i64, gross: i64, entered: i64) -> Result<Books> {
        let Some(place) = self.place_of(key) else { return invalid("no row of the key") };
        let r = self.leaves[place].expect("a row");
        let beyond = || Error::Invalid("the books: a row is beyond what is counted".into());
        let next = Row {
            net: r.net.checked_add(net).filter(|x| *x != i64::MIN).ok_or_else(beyond)?,
            gross: r.gross.checked_add(gross).ok_or_else(beyond)?,
            entered: r.entered.checked_add(entered).filter(|x| *x != i64::MIN).ok_or_else(beyond)?,
            ..r
        };
        if next.gross < 0 {
            return invalid("a row has fewer units than leave it");
        }
        let mut b = self.clone();
        b.count += next.has_units() as i64 - r.has_units() as i64;
        b.leaves[place] = Some(next);
        Ok(b)
    }

    /// A row of no unit taken out, by anyone: the leaf before it names what it named, and its place is empty.
    pub fn out(&self, key: &Key) -> Result<Books> {
        let Some(place) = self.place_of(key) else { return invalid("no row of the key") };
        let r = self.leaves[place].expect("a row");
        if *key == Key::HEAD || r.gross != 0 || r.net != 0 || r.entered != 0 {
            return invalid("the head stays, and a row that has a unit or anything entered stays");
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
        fn key(&mut self) -> Key {
            // (A few markets, some with the top bit of a number set; a few days and strikes.)
            let mut market = [0u8; 32];
            let m = self.below(5) as u8;
            market[0] = m;
            market[15] = if m % 2 == 0 { 0x80 | m } else { m };
            market[31] = if m >= 3 { 0x80 } else { 0 };
            let of = self.below(3) as i64;
            Key { market, scale: 100_000_000, price_max: 1 + self.below(2) as i64, day: if of == 0 { 0 } else { 1_000 * (1 + self.below(3) as i64) }, of, strike: if of == 0 { 0 } else { 10 * (1 + self.below(4) as i64) } }
        }
    }

    /// What holds of any books: the list from the head is every row once, in the order of the keys; the rows of
    /// one underlying are next to one another, its futures first; the count is the rows that have a unit; and
    /// every row opens the root.
    fn holds(b: &Books, every: bool) {
        let rows = b.in_order();
        assert_eq!(rows.len(), b.leaves.iter().flatten().count(), "every row once");
        assert_eq!(rows[0].key, Key::HEAD);
        assert!(rows.windows(2).all(|w| w[0].key.below(&w[1].key) && w[0].next == w[1].key.hash()), "in the order of the keys");
        assert_eq!(rows.last().unwrap().next, Key::HEAD.hash());
        for (i, r) in rows.iter().enumerate() {
            // (No row of another underlying between two of one.)
            let last = rows.iter().rposition(|x| x.key.same_underlying(&r.key)).unwrap();
            assert!(rows[i..=last].iter().all(|x| x.key.same_underlying(&r.key)), "one underlying, next to one another");
            let first = rows.iter().position(|x| x.key.same_underlying(&r.key)).unwrap();
            assert!(rows[first].key.of == OF_FUTURES || rows.iter().all(|x| !(x.key.same_underlying(&r.key) && x.key.of == OF_FUTURES)), "its futures first");
        }
        let root = b.root();
        for (i, r) in rows.iter().enumerate() {
            if every || i == rows.len() / 2 {
                assert_eq!(b.shown(&r.key).unwrap().root(), root, "a row opens the root");
            }
        }
        assert_eq!(b.count, rows.iter().filter(|r| r.has_units()).count() as i64);
    }

    #[test]
    fn the_books_keep_their_rows_in_the_order_of_their_keys_whatever_is_put_in_and_taken_out() {
        let mut r = Rng(0x17_0006);
        for _ in 0..12 {
            let mut b = Books::opened();
            holds(&b, true);
            for _ in 0..120 {
                let key = r.key();
                match (b.place_of(&key), r.below(4)) {
                    (None, _) => {
                        // (At any empty place: the order is the keys', not the places'.)
                        let free: Vec<usize> = (0..LEAVES).filter(|i| b.leaves[*i].is_none()).collect();
                        if free.is_empty() {
                            // (Sixty-four leaves: a full tree takes no key more, at any place.)
                            assert!((0..LEAVES).all(|i| b.put(key, i).is_err()));
                            continue;
                        }
                        let place = free[r.below(free.len() as u64) as usize];
                        let before = b.before(&key).unwrap();
                        let next = b.put(key, place).unwrap();
                        assert_ne!(next.root(), b.root());
                        assert!((0..LEAVES).all(|i| next.put(key, i).is_err()), "no key twice");
                        assert_eq!((next.row(&before.key).unwrap().next, next.row(&key).unwrap().next), (key.hash(), before.next));
                        b = next;
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
                            let hash = b.hash();
                            b = b.out(&key).unwrap();
                            assert!(b.row(&key).is_none() && b.hash() != hash);
                        }
                    }
                }
                holds(&b, false);
            }
            holds(&b, true);
        }
    }

    #[test]
    fn a_row_shown_at_another_place_or_changed_does_not_open_the_root() {
        let mut r = Rng(0x17_0007);
        let mut b = Books::opened();
        while b.leaves.iter().flatten().count() < 20 {
            let key = r.key();
            if b.place_of(&key).is_none() {
                b = b.put(key, b.free().unwrap()).unwrap().moved(&key, -5, 5, -35).unwrap();
            }
        }
        let root = b.root();
        for row in b.in_order() {
            let s = b.shown(&row.key).unwrap();
            assert_eq!(s.root(), root);
            assert_ne!(Shown { place: s.place ^ 1, ..s }.root(), root, "at another place");
            assert_ne!(Shown { place: (s.place + 32) % LEAVES, ..s }.root(), root, "at another place");
            assert_ne!(Shown { row: Row { net: s.row.net + 1, ..s.row }, ..s }.root(), root, "a unit more");
            assert_ne!(Shown { row: Row { next: Key::HEAD.hash(), ..s.row }, ..s }.root() == root, row.next != Key::HEAD.hash(), "another key after");
            // (A leaf is no inner node: two leaves' hashes shown as a row's bytes open nothing.)
            assert_ne!(root_of(&node(&s.row.leaf(), &s.beside[0]), s.place >> 1, &[s.beside[1], s.beside[2], s.beside[3], s.beside[4], s.beside[5], [0; 32]]), root);
        }
        // The head's hash of the books moves with the count, with what was paid, and with the root.
        let h = b.hash();
        assert_ne!(Books { count: b.count + 1, ..b.clone() }.hash(), h);
        assert_ne!(Books { paid: 1, ..b.clone() }.hash(), h);
        // A full tree takes no row more; and the head is never taken out.
        let mut full = Books::opened();
        let mut n = 0i64;
        while full.free().is_some() {
            n += 1;
            let mut market = [0u8; 32];
            market[..8].copy_from_slice(&sm8(n));
            let key = Key { market, scale: 1, price_max: 1, day: 0, of: OF_FUTURES, strike: 0 };
            full = full.put(key, full.free().unwrap()).unwrap();
        }
        assert_eq!(full.in_order().len(), LEAVES);
        assert!(full.out(&Key::HEAD).is_err());
    }
}
