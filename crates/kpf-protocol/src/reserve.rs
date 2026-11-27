//! The reserve (docs/ARCHITECTURE-CLEARING.md 6a): a clearing party's shard, the notes written to it by what happens
//! in the accounts it covers, and what applying a note does. The model of `KpfTShardOpen`, `KpfTLeft`,
//! `KpfTLeftAll`, `KpfTLeftVoid` and `KpfTAcctEnd`, with the scripts' integer arithmetic.
//!
//! - A [`Shard`] has an account's form with a moving part of 171 bytes: its owner's key as A, no second key, what it
//!   holds as A's margin, and its own id at the end. Its fixed part is the hash of its terms and of its books (four
//!   rows, each named by its key: docs 17.8d).
//! - A covered account is an [`Account`] with `shard` set: the same 171 bytes, the id being the shard's.
//! - A note to a shard is an object of its own ([`Left`], [`Rest`]; the old form's notes, of a leg entered or left, a
//!   deposit, or what a user is still owed.

use crate::books::Key;
use crate::committee::blake2b;
use crate::cross::{XLeg, XTerms};
use crate::error::{Error, Result};
use crate::state::{sm8, LONG};
use crate::state::BPS;
use crate::thin::{Account, Out, Outs, Stated, KIND_DRAW};

/// The rows of a shard that the scripts take, until the books are a tree (docs 17.8d, stage 2): the first four places.
pub const SHARD_ROWS: usize = 4;
/// The places a row can stand at in a shard's books: the leaves of the tree but the head's.
pub const PLACES: usize = 63;

/// One of something a place: a row, a price, a row's head. Made of four (the first four places), it has nothing at
/// the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Places<T>(pub [T; PLACES]);

impl<T: Copy + Default> Default for Places<T> {
    fn default() -> Self {
        Places([T::default(); PLACES])
    }
}

impl<T: Copy + Default> From<[T; SHARD_ROWS]> for Places<T> {
    fn from(a: [T; SHARD_ROWS]) -> Self {
        let mut p = Places::default();
        p.0[..SHARD_ROWS].copy_from_slice(&a);
        p
    }
}

impl<T: Copy + Default> From<&[T; SHARD_ROWS]> for Places<T> {
    fn from(a: &[T; SHARD_ROWS]) -> Self {
        (*a).into()
    }
}

impl<T: Copy> From<&Places<T>> for Places<T> {
    fn from(a: &Places<T>) -> Self {
        *a
    }
}

impl<T> std::ops::Deref for Places<T> {
    type Target = [T; PLACES];
    fn deref(&self) -> &[T; PLACES] {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Places<T> {
    fn deref_mut(&mut self) -> &mut [T; PLACES] {
        &mut self.0
    }
}

impl<'a, T> IntoIterator for &'a Places<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T: Copy + Default + PartialEq> PartialEq<[T; SHARD_ROWS]> for Places<T> {
    fn eq(&self, o: &[T; SHARD_ROWS]) -> bool {
        *self == Places::from(*o)
    }
}

impl<T: Copy> Places<T> {
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Places<U> {
        Places(self.0.map(f))
    }
}
/// The least a note holds (`NOTE_MIN` of the scripts).
pub const NOTE_MIN: i64 = 20_000_000;
/// What a shard's output holds beside what the shard holds for its users (`SHARD_CARRIER` of `KpfTShardOpen`): a
/// shard that paid everything out is still an output a block takes.
pub const SHARD_CARRIER: i64 = 100_000_000;

fn p33(h: &[u8; 32]) -> Vec<u8> {
    [&[0x20][..], h].concat()
}

fn i9(v: i64) -> Vec<u8> {
    [&[0x08][..], &sm8(v)].concat()
}

/// A market as an uncovered account's take names the row a leg leaves (`thin::ATake`): 84 bytes, the hash of a
/// kind and of the market's name, the committee, the scale and the price's limit. (A shard's row is named by its
/// key, plain: `RowHead`.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Head {
    pub market: [u8; 32],
    pub committee: [u8; 32],
    pub scale: i64,
    pub price_max: i64,
}

impl Head {
    /// No market: an empty row's, and a deposit's or an owed note's.
    pub const EMPTY: Head = Head { market: [0; 32], committee: [0; 32], scale: 0, price_max: 0 };

    /// The row a leg is counted in, in a shard: named by the hash of its kind and of its market's name. A market's
    /// futures legs have one row and its options another (options are not set against futures legs, and no leg of
    /// the one kind is counted in the row of the other, whatever its market is called).
    pub fn of(l: &XLeg) -> Head {
        let market = if crate::thin::option_of(l).is_some() { option_market(&l.market) } else { futures_market(&l.market) };
        Head { market, committee: l.committee, scale: l.scale, price_max: l.price_max }
    }

    pub fn bytes(&self) -> Vec<u8> {
        [p33(&self.market), p33(&self.committee), i9(self.scale), i9(self.price_max)].concat()
    }

    /// The row of the same market: its name, its scale and its price's limit. (A covered account's leg does not
    /// say its committee: every row of a shard is of the shard's one. Docs 17.2.)
    pub fn same_row(&self, o: &Head) -> bool {
        (self.market, self.scale, self.price_max) == (o.market, o.scale, o.price_max)
    }
}

/// A shard's id: the hash of its opening (`seed`, the hash of its anchor's outpoint), of the one committee whose
/// prices the accounts under it are counted at, and of the terms and periods (lateness, dispute period, bond rate)
/// those accounts have, which are the shard's own. A leg enters an account under the shard only where the leg's
/// committee and the account's terms and periods give this id.
pub fn shard_sid(seed: &[u8; 32], committee: &[u8; 32], terms: &[u8], periods: &[u8]) -> [u8; 32] {
    blake2b(&[&seed[..], committee, terms, periods].concat())
}

/// The name the options of a market are counted under in a shard: `blake2b(push8(13) + push(market))`.
pub fn option_market(market: &[u8; 32]) -> [u8; 32] {
    blake2b(&[i9(crate::thin::OPTION_TAG), p33(market)].concat())
}

/// The name the futures legs of a market are counted under in a shard: `blake2b(push8(14) + push(market))`.
pub fn futures_market(market: &[u8; 32]) -> [u8; 32] {
    blake2b(&[i9(crate::thin::FUTURES_TAG), p33(market)].concat())
}

/// What a leg says of the row it is counted in (docs 17.6, 17.8d), 78 bytes: the row's key, plain (the market, and
/// a day, a kind and a strike: three of nothing for a market's futures row), its market's scale and its price's
/// limit. No committee: every row of a shard is of the shard's one, which its id is opened by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowHead {
    pub key: Key,
    pub scale: i64,
    pub price_max: i64,
}

impl Default for RowHead {
    fn default() -> Self {
        RowHead::EMPTY
    }
}

impl Default for ShardRow {
    fn default() -> Self {
        ShardRow::EMPTY
    }
}

impl RowHead {
    /// No market: an empty row's.
    pub const EMPTY: RowHead = RowHead { key: Key::HEAD, scale: 0, price_max: 0 };

    /// The futures row of a leg's market.
    pub fn of(l: &XLeg) -> RowHead {
        RowHead { key: Key::futures(l.market), scale: l.scale, price_max: l.price_max }
    }

    pub fn bytes(&self) -> Vec<u8> {
        [self.key.bytes(), i9(self.scale), i9(self.price_max)].concat()
    }
}

/// A row of a shard, 105 bytes (the tree's leaf without its tag and the key after it): its key, its market's scale
/// and price's limit, the clearing party's net units in it over the accounts the shard covers (below zero: it is
/// short), the sum of the legs' sizes, and what its legs were entered at (kept in a row with a day: nothing in a
/// futures row, whose legs' entries are in the one number over all rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShardRow {
    pub key: Key,
    pub scale: i64,
    pub price_max: i64,
    pub net: i64,
    pub gross: i64,
    pub entered: i64,
}

impl ShardRow {
    pub const EMPTY: ShardRow = ShardRow { key: Key::HEAD, scale: 0, price_max: 0, net: 0, gross: 0, entered: 0 };

    /// A row of a head, with its units.
    pub fn of(head: RowHead, net: i64, gross: i64) -> ShardRow {
        ShardRow { key: head.key, scale: head.scale, price_max: head.price_max, net, gross, entered: 0 }
    }

    /// The futures row of a market.
    pub fn futures(market: [u8; 32], scale: i64, price_max: i64, net: i64, gross: i64) -> ShardRow {
        ShardRow::of(RowHead { key: Key::futures(market), scale, price_max }, net, gross)
    }

    pub fn head(&self) -> RowHead {
        RowHead { key: self.key, scale: self.scale, price_max: self.price_max }
    }

    /// Whether it is a row of no market.
    pub fn no_market(&self) -> bool {
        self.key.market == [0; 32]
    }

    pub fn bytes(&self) -> Vec<u8> {
        [self.head().bytes(), i9(self.net), i9(self.gross), i9(self.entered)].concat()
    }
}

/// The point of a draw's or a call's dispute that is a row of its trace (the row's number among the trace's, from
/// nothing) and the one that is a price of its list (its number in the list, from nothing): tests and tools say
/// points by these, which move when the books are a tree.
pub fn trace_row_point(row: usize) -> i64 {
    row as i64 + 1
}

pub fn trace_price_point(e: usize) -> i64 {
    (SHARD_ROWS + 1 + e) as i64
}

/// A clearing party's reserve shard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shard {
    /// Its terms, an account's ten: the maintenance rate is the rate on the net, the liquidation fee's rate the rate
    /// on the gross, the tolerance the tolerance.
    pub t: XTerms,
    pub rows: Places<ShardRow>,
    pub owner: [u8; 32],
    pub held: i64,
    pub expiry: i64,
    pub dispute_daa: i64,
    pub bond_bps: i64,
    pub sid: [u8; 32],
    /// What its legs were entered at: the sum of side B's units times their entries (where a position has what A
    /// asks of B). With it the shard knows what its side has gained or lost at given prices.
    pub basis: i64,
    /// Whether it is called (where a position has what B asks of A): nothing, or the time of the call.
    pub called: i64,
    /// What it owes by claims (where a position has side B's margin): nothing while it is not called.
    pub owed: i64,
    /// The claims' numbers of a called shard (docs 17.8c): three of nothing while it is not called.
    pub claims: Claims,
    /// The root of the tree of its dated rows (docs 17.8j, 1): of the head leaf alone while it has none.
    pub root: [u8; 32],
    /// The sum of the gross units of its dated rows: with the four rows', the sum in the books' hash.
    pub dgross: i64,
}

/// The claims' numbers of a called shard (docs 17.8c), beside what it owes by claims of the first class (`owed`,
/// which does not fall when a claim is paid): what it has paid to the first class, what it owes by claims of the
/// second, and what it has paid to those.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Claims {
    pub paid1: i64,
    pub owed2: i64,
    pub paid2: i64,
}

impl Claims {
    /// 27 bytes: what a script hashes of them.
    pub fn bytes(&self) -> Vec<u8> {
        [i9(self.paid1), i9(self.owed2), i9(self.paid2)].concat()
    }
}

impl Shard {
    /// An account with this shard's terms and moving part and no leg: what the bytes are taken from.
    fn shell(&self) -> Account {
        Account { t: self.t, legs: vec![], a_key: self.owner, b_key: [0; 32], a_margin: self.held, b_margin: self.owed, expiry: self.expiry, dispute_daa: self.dispute_daa, bond_bps: self.bond_bps, a_min_counter: self.basis, b_min_counter: self.called, shard: Some(self.sid) }
    }

    /// A shard as `KpfTShardOpen` writes it: no row taken.
    pub fn opened(t: XTerms, owner: [u8; 32], held: i64, expiry: i64, dispute_daa: i64, bond_bps: i64, sid: [u8; 32]) -> Shard {
        Shard { t, rows: Places::default(), owner, held, expiry, dispute_daa, bond_bps, sid, basis: 0, called: 0, owed: 0, claims: Claims::default(), root: Shard::root_opened(), dgross: 0 }
    }

    pub fn terms(&self) -> Vec<u8> {
        self.shell().terms()
    }

    /// The root of a tree of dated rows with the head leaf alone: what a shard is opened with (`KpfTShardOpen`'s
    /// constant).
    pub fn root_opened() -> [u8; 32] {
        crate::books::Books::opened().root()
    }

    /// The four rows the scripts take, 420 bytes.
    pub fn rows_bytes(&self) -> Vec<u8> {
        self.rows.iter().take(SHARD_ROWS).flat_map(|r| r.bytes()).collect()
    }

    /// The sum of the rows' gross units (a call ends where it is nothing).
    pub fn gross_sum(&self) -> i64 {
        self.rows.iter().fold(self.dgross, |a, x| a.saturating_add(x.gross))
    }

    /// The hash of the claims' numbers (docs 17.8c).
    pub fn claims_hash(&self) -> [u8; 32] {
        blake2b(&self.claims.bytes())
    }

    /// What a script that shows the rows is shown beside them, 69 bytes: the sum of the gross and the claims'
    /// three numbers, plain, and the root of the dated rows' tree.
    pub fn books_head(&self) -> Vec<u8> {
        [i9(self.gross_sum()), self.claims.bytes(), p33(&self.root)].concat()
    }

    /// The books' hash (docs 17.6, 17.8a): of the sum of the rows' gross units, of the hash of the rows, and of the
    /// hash of the claims' numbers. A script that touches no row carries it as 32 bytes.
    pub fn books_hash(&self) -> [u8; 32] {
        blake2b(&[i9(self.gross_sum()), p33(&blake2b(&self.rows_bytes())), p33(&self.root), p33(&self.claims_hash())].concat())
    }

    pub fn fixed_hash(&self) -> [u8; 32] {
        blake2b(&[self.terms(), p33(&self.books_hash())].concat())
    }

    /// The moving part, 171 bytes.
    pub fn moving(&self) -> Vec<u8> {
        self.shell().moving()
    }

    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[p33(&self.fixed_hash()), self.moving()].concat())
    }

    /// The row a leg of the head `head` enters: the row of that head, or the first empty one.
    pub fn row_for(&self, head: &RowHead) -> Option<usize> {
        self.rows.iter().take(SHARD_ROWS).position(|r| r.head() == *head).or_else(|| self.rows.iter().take(SHARD_ROWS).position(|r| *r == ShardRow::EMPTY))
    }

    /// The futures row of a market, where the shard has one.
    pub fn row(&self, market: &[u8; 32]) -> Option<&ShardRow> {
        self.rows.iter().find(|r| r.key == Key::futures(*market) && !r.no_market())
    }

    /// The shard with these rows for its own (four at the most; the others of no market).
    pub fn of_rows(&self, rows: &[ShardRow]) -> Shard {
        let mut s = *self;
        s.rows = Places::default();
        s.rows[..rows.len()].copy_from_slice(rows);
        s
    }

    /// The shard with the id its opening by `seed` gives it, for a committee (`shard_sid`: what an entry and a
    /// price's answer hold it to).
    pub fn seeded(&self, seed: &[u8; 32], committee: &[u8; 32]) -> Shard {
        Shard { sid: shard_sid(seed, committee, &self.terms(), &self.moving()[93..120]), ..*self }
    }

    /// Whether the shard's id is its opening's by `seed`, for a committee.
    pub fn is_of(&self, seed: &[u8; 32], committee: &[u8; 32]) -> bool {
        self.sid == shard_sid(seed, committee, &self.terms(), &self.moving()[93..120])
    }

}

/// Side B's units in a leg's market: below zero where A is long. An option counts with its size whichever way it
/// stands (whoever holds it, the other party may come to owe by it as far as its underlying moves).
pub fn units_of(l: &XLeg) -> i64 {
    if l.dir == LONG && crate::thin::option_of(l).is_none() {
        -l.size
    } else {
        l.size
    }
}

impl Account {
    /// `KpfTAcctEnd`: what a covered account of no leg pays its user where it is ended.
    pub fn ended(&self) -> Result<i64> {
        if self.shard.is_none() || !self.legs.is_empty() || self.b_margin != 0 {
            return Err(Error::Invalid("an account is ended where a shard covers it, it has no leg (each leaves by its own statement first), and nothing of a second party's".into()));
        }
        Ok(self.a_margin)
    }
}

/// The note that legs left a covered account (`KpfTLeft`; `docs/ARCHITECTURE-CLEARING.md` 13).
pub const NOTE_LEFT: i64 = 6;
/// What a note that legs left asked and the shard did not hold.
pub const NOTE_REST: i64 = 7;

/// The note that legs left a covered account: the legs (one, or all the account had), the user's key, and what
/// the shard is asked for them; below zero where the account paid the shard (the legs lost), and the note then
/// holds those coins. It is applied whole: each leg is taken out of the shard's books and what is asked is settled,
/// in one transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Left {
    pub sid: [u8; 32],
    pub legs: Vec<XLeg>,
    pub key: [u8; 32],
    pub asked: i64,
}

impl Left {
    /// The chain of legs, as an account's is: for all an account's legs it is the account's own.
    pub fn chain(legs: &[XLeg]) -> [u8; 32] {
        legs.iter().fold([0u8; 32], |g, l| blake2b(&[&g[..], &crate::thin::leg_bytes(l)].concat()))
    }

    pub fn hash(&self) -> [u8; 32] {
        self.hash_of(&Left::chain(&self.legs))
    }

    /// The note's hash with the chain of its legs given.
    pub fn hash_of(&self, chain: &[u8; 32]) -> [u8; 32] {
        blake2b(&[i9(12), p33(&self.sid), i9(NOTE_LEFT), p33(chain), p33(&self.key), i9(self.asked)].concat())
    }

    /// What the note holds: a note's least, and what the account paid for the legs (what is asked, below zero).
    pub fn value(&self) -> i64 {
        NOTE_MIN.saturating_add(self.asked.saturating_neg().max(0))
    }

    /// The hash of the note a call's step writes (tag 9; docs 17.8c): it says, beside what is asked, what the
    /// young legs gained together less their band, which is a claim of the second class.
    pub fn hash_step(&self, young: i64) -> [u8; 32] {
        blake2b(&[i9(12), p33(&self.sid), i9(NOTE_STEP), p33(&Left::chain(&self.legs)), p33(&self.key), i9(self.asked), i9(young)].concat())
    }

    /// The note's hash by its kind: an account's own (6), or a call's step's (9) with what the young gained.
    pub fn hash_as(&self, tag: i64, young: i64) -> [u8; 32] {
        if tag == NOTE_STEP { self.hash_step(young) } else { self.hash() }
    }
}

/// The note that legs left by a call's step (`KpfTCStep`): of a kind of its own, since it says two amounts.
pub const NOTE_STEP: i64 = 9;

/// The note that a leg of a covered account was marked (`KpfTMarkNote`; docs 16.3).
pub const NOTE_MARK: i64 = 13;

/// The note that a leg was marked: the leg as it was, its new entry, the user's key, and what the shard is asked:
/// what the leg gained between its two entries. No unit moves by it: the shard's rows are as they were, and what
/// its legs were entered at takes the new entry for the old.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkNote {
    pub sid: [u8; 32],
    pub leg: XLeg,
    pub entry: i64,
    pub key: [u8; 32],
    pub asked: i64,
}

impl MarkNote {
    pub fn hash(&self) -> [u8; 32] {
        self.hash_of(&Left::chain(&[self.leg]))
    }

    /// The note's hash with the chain of its leg given.
    pub fn hash_of(&self, chain: &[u8; 32]) -> [u8; 32] {
        blake2b(&[i9(12), p33(&self.sid), i9(NOTE_MARK), p33(chain), i9(self.entry), p33(&self.key), i9(self.asked)].concat())
    }

    /// What the note holds: a note's least.
    pub fn value(&self) -> i64 {
        NOTE_MIN
    }
}

/// What an account that goes on from a call's step keeps of its coins, and its output holds at the least: a KAS
/// (review 64, F113).
pub const OWING_KEEP: i64 = 100_000_000;

/// The note that holds what was paid of what a covered account owes its shard (`KpfTOwed`; docs 16.18).
pub const NOTE_OWED: i64 = 15;

/// The note of a payment of what an account owes: the shard and the amount. It holds a note's least and the
/// amount, and its shard takes what it holds above the least, no more than it says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owed {
    pub sid: [u8; 32],
    pub amount: i64,
}

impl Owed {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[i9(12), p33(&self.sid), i9(NOTE_OWED), i9(self.amount)].concat())
    }

    pub fn value(&self) -> i64 {
        NOTE_MIN.saturating_add(self.amount.max(0))
    }
}

/// What a note that legs left asked and the shard did not pay: counted in what the shard owes when it is written,
/// and paid as far as the shard holds, later; or, of a called shard, at one share once no row has a unit (docs
/// 14.6; review 54, F94).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rest {
    pub sid: [u8; 32],
    pub key: [u8; 32],
    pub amount: i64,
}

impl Rest {
    /// The note as a claim: its amount of the first class, nothing of it paid, nothing of the second.
    pub fn claim(&self) -> Claim {
        Claim { sid: self.sid, key: self.key, amount1: self.amount, paid1: 0, amount2: 0, paid2: 0 }
    }

    pub fn hash(&self) -> [u8; 32] {
        self.claim().hash()
    }

    pub fn value(&self) -> i64 {
        NOTE_MIN
    }
}

/// A claim on a shard (docs 17.8c; the note of the rest, tag 7): its amount of the first class and what it has
/// been paid of it, and the same of the second class. While its shard is not called it is a [`Rest`]: an amount of
/// the first class, and three numbers of nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    pub sid: [u8; 32],
    pub key: [u8; 32],
    pub amount1: i64,
    pub paid1: i64,
    pub amount2: i64,
    pub paid2: i64,
}

impl Claim {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[i9(12), p33(&self.sid), i9(NOTE_REST), p33(&self.key), i9(self.amount1), i9(self.paid1), i9(self.amount2), i9(self.paid2)].concat())
    }

    pub fn value(&self) -> i64 {
        NOTE_MIN
    }
}

/// What a shard takes of a note that asks `asked` and whose output holds `holds`: nothing where it asks nothing or
/// something; else what the output holds above a note's least, and no more than the note says it holds.
pub fn held_for(asked: i64, holds: i64) -> i64 {
    if asked >= 0 {
        return 0;
    }
    let more = holds.saturating_sub(NOTE_MIN).max(0);
    if asked > -more { -asked } else { more }
}

impl Shard {
    /// What a futures leg is to the books: the row of its market's futures legs, side B's units in it (the size
    /// below zero where A is long), and its worth at its entry signed as side B's (what it adds to what the legs
    /// were entered at).
    fn book_of(l: &XLeg) -> Result<(RowHead, i64, i64)> {
        if (l.dir != LONG && l.dir != crate::state::SHORT) || l.size <= 0 || l.scale <= 0 || l.entry <= 0 {
            return Err(Error::Invalid("the books are of futures legs: a direction, a size, a scale and an entry".into()));
        }
        let we = worth(l.size, l.scale, l.entry).ok_or_else(|| Error::Invalid("the leg's worth is beyond what is counted".into()))?;
        let head = RowHead::of(l);
        Ok(if l.dir == LONG { (head, -l.size, -we) } else { (head, l.size, we) })
    }

    /// The shard with the leg in its books: its row with the leg's units, and what the legs were entered at with
    /// the leg's entry (what the statement of an entry does, in the transaction that states it).
    pub fn booked(&self, l: &XLeg) -> Result<Shard> {
        let (head, units, basis) = Shard::book_of(l)?;
        let r = self.row_for(&head).ok_or_else(|| Error::Invalid("the shard has no row for the leg's market: its four are other markets'".into()))?;
        let mut s = *self;
        let row = &mut s.rows[r];
        (row.key, row.scale, row.price_max) = (head.key, head.scale, head.price_max);
        row.net = row.net.checked_add(units).ok_or_else(|| Error::Invalid("the row is beyond what is counted".into()))?;
        row.gross = row.gross.checked_add(l.size).ok_or_else(|| Error::Invalid("the row is beyond what is counted".into()))?;
        s.basis = s.basis.checked_add(basis).ok_or_else(|| Error::Invalid("what the legs were entered at is beyond what is counted".into()))?;
        Ok(s)
    }

    /// What an option leg is to the books (docs 17.8j, 5): the dated row of its market, its day, calls or puts, and
    /// its strike; the clearing side's units in it (the size where the user wrote it, the size below nothing
    /// where the user holds it); and what it is worth exercised at its entry, signed the same way: what the
    /// row's number and the shard's one number of what the legs were entered at take. No premium is in the books.
    pub fn book_of_option(l: &XLeg) -> Result<(RowHead, i64, i64)> {
        let Some(o) = crate::crow::option_of(l) else { return Err(Error::Invalid("the leg is no option leg".into())) };
        if l.size <= 0 || l.scale <= 0 || l.entry <= 0 || o.strike <= 0 || o.day <= 0 {
            return Err(Error::Invalid("an option leg has a size, a scale, an entry, a strike and a day".into()));
        }
        let ex = if o.call { l.entry.checked_sub(o.strike) } else { o.strike.checked_sub(l.entry) }.map(|v| v.max(0));
        let we = ex.and_then(|x| worth(l.size, l.scale, x)).ok_or_else(|| Error::Invalid("the leg's worth is beyond what is counted".into()))?;
        let key = Key { market: l.market, day: o.day, of: if o.call { crate::books::OF_CALLS } else { crate::books::OF_PUTS }, strike: o.strike };
        let head = RowHead { key, scale: l.scale, price_max: l.price_max };
        Ok(if o.held { (head, -l.size, -we) } else { (head, l.size, we) })
    }

    /// What a dated row is asked alone, at the price `p` and the upper rate, with the rate on its gross (the row's
    /// gate: docs 17.8j, 7).
    fn dated_gate(&self, row: &crate::books::Row, p: i64) -> Option<i64> {
        let (_, wg, asked) = self.dated_adds(row, p, false)?;
        asked.checked_add(rate_up(wg, gross_rate(&self.t) + self.t.g_bps)?)
    }

    /// `KpfTEnterSD`: the shard with an option leg in its books (docs 17.8j, 5 and 7), `books` being its tree. The
    /// leg's row is listed (no entry puts a row in) and has the leg's scale and limit; the row and what the legs
    /// were entered at take the leg, within bounds; the row's gate: what the shard holds, less what it owes, is
    /// at least what the row is asked with the leg, at the leg's entry; or the row is asked no more with the leg
    /// than without it. The shard's time as a futures entry writes it. Not while the shard is called.
    pub fn entered_d(&self, books: &crate::books::Books, l: &XLeg, t0: i64) -> Result<(Shard, crate::books::Books)> {
        let no = |why: &str| Err(Error::Invalid(format!("the shard does not take the option leg: {why}")));
        if self.called != 0 {
            return no("it is called");
        }
        if books.root() != self.root {
            return no("the books shown are not the shard's");
        }
        let (head, units, basis) = Shard::book_of_option(l)?;
        let Some(row) = books.row(&head.key) else { return no("its series is not listed in the shard's books") };
        if (row.scale, row.price_max) != (head.scale, head.price_max) {
            return no("the leg's scale and limit are not its row's");
        }
        let max = crate::cross::MARGIN_MAX;
        let net = row.net.checked_add(units).filter(|n| (-max..=max).contains(n));
        let gross = row.gross.checked_add(l.size).filter(|g| *g <= max);
        let entered = row.entered.checked_add(basis).filter(|x| *x != i64::MIN);
        let (Some(net), Some(gross), Some(entered), Some(b)) = (net, gross, entered, self.basis.checked_add(basis).filter(|b| *b != i64::MIN)) else { return no("its books are beyond what is counted") };
        let with = crate::books::Row { net, gross, entered, ..row };
        let (Some(need), Some(before)) = (self.dated_gate(&with, l.entry), self.dated_gate(&row, l.entry)) else { return no("its row's requirement is beyond what is counted") };
        if self.held.checked_sub(self.owed).is_none_or(|free| free < need) && need > before {
            return no("the row's gate: what the shard holds, less what it owes, is below what the leg's row is asked with the leg, and the leg raises it");
        }
        let until = self.dispute_daa.checked_mul(2).and_then(|w| w.checked_add(self.t.late_daa)).and_then(|x| x.checked_add(t0));
        let Some(until) = until else { return no("its time is beyond what is counted") };
        if self.expiry.checked_add(self.t.late_daa).is_none_or(|least| until < least) {
            return no("an entry's time is a lateness after the last: the shard's time is too near, or ahead");
        }
        let next = books.moved(&head.key, units, l.size, basis)?;
        let Some(dgross) = self.dgross.checked_add(l.size) else { return no("its books are beyond what is counted") };
        Ok((Shard { root: next.root(), dgross, basis: b, expiry: until, ..*self }, next))
    }

    /// `KpfTLeftD.apply` (docs 17.8j, 9): the shard, and its tree `books`, once a note of legs of which some have a
    /// day is applied: each option leg out of its dated row (the row has it: its units, its scale and limit) and
    /// out of what the legs were entered at; the futures legs and what is asked as `left_holding` has them. Not
    /// of a called shard (its notes are counted in their class: with the call's step for option legs).
    #[allow(clippy::type_complexity)]
    pub fn left_d(&self, books: &crate::books::Books, n: &Left, holds: i64) -> Result<(Shard, crate::books::Books, Vec<usize>, i64, Option<Rest>)> {
        let no = |why: &str| Err(Error::Invalid(format!("the note is not applied: {why}")));
        if self.called != 0 {
            return no("the shard is called");
        }
        if books.root() != self.root {
            return no("the books shown are not the shard's");
        }
        let (mut b, mut s) = (books.clone(), *self);
        for l in n.legs.iter().filter(|l| crate::crow::option_of(l).is_some()) {
            let (head, units, basis) = Shard::book_of_option(l)?;
            let Some(row) = b.row(&head.key) else { return no("an option leg is in no row of the shard's tree") };
            if (row.scale, row.price_max) != (head.scale, head.price_max) || row.gross < l.size {
                return no("an option leg's row is of another scale or limit, or has fewer units than the leg");
            }
            b = b.moved(&head.key, units.checked_neg().unwrap_or(0), -l.size, basis.checked_neg().unwrap_or(0))?;
            let (Some(bs), Some(dg)) = (s.basis.checked_sub(basis).filter(|x| *x != i64::MIN), s.dgross.checked_sub(l.size).filter(|x| *x >= 0)) else { return no("the books are beyond what is counted") };
            (s.basis, s.dgross) = (bs, dg);
        }
        s.root = b.root();
        let rest = Left { legs: n.legs.iter().filter(|l| crate::crow::option_of(l).is_none()).copied().collect(), ..n.clone() };
        if !rest.legs.is_empty() {
            let (next, at, paid, claim) = s.left_holding(&rest, holds)?;
            return Ok((next, b, at, paid, claim));
        }
        // (A note of option legs alone: what is asked, as `left_class` has it of a shard that is not called. What
        // the account paid, which the note holds, is the shard's coins; what is asked is paid as far as it holds,
        // and the rest is owed.)
        if n.sid != self.sid || n.legs.is_empty() || n.legs.len() > 6 {
            return no("it is to another shard, or not of one to six legs");
        }
        let beyond = || Error::Invalid("the books are beyond what is counted".into());
        if n.asked < 0 {
            s.held = s.held.checked_add(held_for(n.asked, holds)).ok_or_else(beyond)?;
        }
        let pay = n.asked.max(0).min(s.held);
        let owed = n.asked.max(0) - pay;
        s.held -= pay;
        s.owed = s.owed.checked_add(owed).ok_or_else(beyond)?;
        Ok((s, b, vec![], pay, (owed > 0).then_some(Rest { sid: n.sid, key: n.key, amount: owed })))
    }

    /// The shard's committee is the leg's, for a leg that enters: the shard's id is the hash of `seed`, of that
    /// committee, and of the terms and periods of the accounts under it (`KpfTEnterS`; review 51, F80).
    pub fn of_committee(&self, seed: &[u8; 32], l: &XLeg) -> bool {
        self.sid == shard_sid(seed, &l.committee, &self.terms(), &self.moving()[93..120])
    }

    /// `KpfTEnterS`: the shard with the leg in its books, in the row `r`: the row is the leg's market's, or no
    /// market's where no other row is that market's; the row and what the legs were entered at take the leg, within
    /// bounds. Not while the shard is called.
    pub fn entered(&self, l: &XLeg, r: usize, t0: i64) -> Result<Shard> {
        let no = |why: &str| Err(Error::Invalid(format!("the shard does not take the leg: {why}")));
        if self.called != 0 {
            return no("it is called");
        }
        let (head, units, basis) = Shard::book_of(l)?;
        let Some(row) = self.rows.get(r).filter(|_| r < SHARD_ROWS) else { return no("it has four rows") };
        // (A market of nothing is no row's: the rows of no market have its key.)
        if row.head() != head && (*row != ShardRow::EMPTY || self.rows.iter().any(|x| x.key == head.key)) {
            return no("the row is another market's, or the leg's market has a row of its own");
        }
        // (A row is made only for a market that is one: no number of its id nothing written below zero. Review T9, Q1.)
        if row.head() != head && !head.key.well_made() {
            return no("a market of which a number of the id is nothing written below zero has no row: it is one market with another id, by their numbers");
        }
        let mut s = *self;
        let max = crate::cross::MARGIN_MAX;
        let net = s.rows[r].net.checked_add(units).filter(|n| (-max..=max).contains(n));
        let gross = s.rows[r].gross.checked_add(l.size).filter(|g| *g <= max);
        let (Some(net), Some(gross), Some(b)) = (net, gross, s.basis.checked_add(basis).filter(|b| *b != i64::MIN)) else { return no("its books are beyond what is counted") };
        // The row's gate (docs 17.8c; `KpfTEnterS`): what the shard holds, less what it owes, is at least the upper
        // requirement of this row with the leg, at the leg's own entry; or the row is asked no more with the leg
        // than without it.
        let asked = |n: i64, g: i64| need_of(worth(n.checked_abs()?, l.scale, l.entry)?, worth(g, l.scale, l.entry)?, &self.t, true);
        let (Some(need), Some(before)) = (asked(net, gross), asked(self.rows[r].net, self.rows[r].gross)) else { return no("its row's requirement is beyond what is counted") };
        if self.held.checked_sub(self.owed).is_none_or(|free| free < need) && need > before {
            return no("the row's gate: what the shard holds, less what it owes, is below the upper requirement of the leg's row with the leg, and the leg raises it");
        }
        // (The row's ninth field, the count of its market's leaves in the tree, stays: 17.8j, 2.)
        s.rows[r] = ShardRow { entered: self.rows[r].entered, ..ShardRow::of(head, net, gross) };
        s.basis = b;
        // (The shard's time: no draw before the entry is settled or ended. Review 51, F82. And an entry's time is
        // a lateness after the last: what this entry writes is a lateness or more after what the shard says.
        // Review 57, F103.)
        let until = self.dispute_daa.checked_mul(2).and_then(|w| w.checked_add(self.t.late_daa)).and_then(|x| x.checked_add(t0));
        let Some(until) = until else { return no("its time is beyond what is counted") };
        if self.expiry.checked_add(self.t.late_daa).is_none_or(|least| until < least) {
            return no("an entry's time is a lateness after the last: the shard's time is too near, or ahead");
        }
        s.expiry = until;
        Ok(s)
    }

    /// `KpfTLeft.apply` (a note of one leg) and `KpfTLeftAll.apply` (of two to six): the shard once the note that
    /// legs left is applied, whole: each leg out of its row and out of what the legs were entered at; what is
    /// asked paid as far as the shard holds, and the note of the rest; or, below zero, what the note holds added
    /// to what the shard holds. Also the row of each leg. Refused where a leg is in no row, or where the shard is
    /// called and something is asked.
    pub fn left(&self, n: &Left) -> Result<(Shard, Vec<usize>, i64, Option<Rest>)> {
        self.left_holding(n, n.value())
    }

    /// Whether a leg is young at this shard's call (docs 17.9): it entered after the call's time less T, T twice a
    /// lateness and a dispute period. No leg is, of a shard that is not called.
    pub fn young(&self, l: &XLeg) -> bool {
        let t = self.t.late_daa.saturating_add(self.dispute_daa).saturating_mul(2);
        self.called != 0 && crate::covered::leg_time(l) > self.called.saturating_sub(t)
    }

    /// Whether anything was paid to a claim of this shard: what is counted after is of the second class.
    pub fn paid_any(&self) -> bool {
        self.claims.paid1.saturating_add(self.claims.paid2) > 0
    }

    /// The books as `KpfTMarkNoteC` is shown them, 101 bytes: the sum of the rows' gross units, the hash of the
    /// rows, the claims' three numbers, and the root of the dated rows' tree.
    pub fn books_open(&self) -> Vec<u8> {
        [i9(self.gross_sum()), blake2b(&self.rows_bytes()).to_vec(), self.claims.bytes(), p33(&self.root)].concat()
    }

    /// What a called shard counts of an amount asked by a note of the legs `legs` (docs 17.8c): of the second
    /// class where a leg of an account's own note is young or anything was paid to a claim; else of the first.
    fn classed(&self, asked: i64, legs: &[XLeg], step: bool) -> (i64, i64) {
        if asked <= 0 {
            return (0, 0);
        }
        if self.paid_any() || (!step && legs.iter().any(|l| self.young(l))) { (0, asked) } else { (asked, 0) }
    }

    /// As `left`, of a note whose output holds `holds`: where the note says it holds what an account paid, the
    /// shard takes what the output holds above a note's least, and no more than the note says (a note is not
    /// taken at its word: a wrong statement nobody disputed writes one that says anything. Review 53, F86).
    pub fn left_holding(&self, n: &Left, holds: i64) -> Result<(Shard, Vec<usize>, i64, Option<Rest>)> {
        let (s, at, pay, claim) = self.left_class(n, NOTE_LEFT, 0, holds)?;
        match claim {
            Some(c) if c.amount2 != 0 => Err(Error::Invalid("the note's amount is a claim of the second class (a leg of it is young at the call, or a claim was paid already): `left_class` gives it".into())),
            c => Ok((s, at, pay, c.map(|c| Rest { sid: c.sid, key: c.key, amount: c.amount1 }))),
        }
    }

    /// `KpfTLeft.apply`, `KpfTLeftAll.apply` (docs 17.8c): a note that legs left, of an account's own (`tag` 6) or
    /// of a call's step (`tag` 9, with what its young legs gained, `young`), applied to this shard. Not called: as
    /// ever, what is asked is paid as far as the shard holds and the rest is owed. Called: nothing is paid, and
    /// what is asked is counted in its class. The shard after, the rows, what is paid, and the claim written.
    pub fn left_class(&self, n: &Left, tag: i64, young: i64, holds: i64) -> Result<(Shard, Vec<usize>, i64, Option<Claim>)> {
        let no = |why: &str| Err(Error::Invalid(format!("the note that legs left is not applied: {why}")));
        let step = tag == NOTE_STEP;
        if (!step && (tag != NOTE_LEFT || young != 0)) || (step && (self.called == 0 || young < 0)) {
            return no("it is an account's own (6), or a call's step's (9), which is of a called shard and says what its young legs gained");
        }
        if step && ((young != 0 && !n.legs.iter().any(|l| self.young(l))) || (n.asked > 0 && n.legs.iter().all(|l| self.young(l)))) {
            return no("a step's note is held to its legs: nothing of the young's where no leg is young, nothing asked above nothing where every leg is");
        }
        if n.sid != self.sid {
            return no("it is to another shard");
        }
        if n.legs.is_empty() || n.legs.len() > 6 {
            return no("it is of one to six legs");
        }
        let beyond = || Error::Invalid("the books are beyond what is counted".into());
        if self.gone(n) {
            return no("a leg of it is in no row of the shard, or the row has fewer units than the note's legs of it: it is applied by nothing, and spent by `KpfTLeftVoid`");
        }
        let mut s = *self;
        let mut at = vec![];
        for l in &n.legs {
            let (head, units, basis) = Shard::book_of(l)?;
            let Some(r) = s.rows.iter().position(|row| row.head() == head) else { return no("a leg is in no row of the shard") };
            let row = &mut s.rows[r];
            if row.gross < l.size {
                return no("a row has fewer units than the leg");
            }
            row.net = row.net.checked_sub(units).filter(|x| *x != i64::MIN).ok_or_else(beyond)?;
            row.gross -= l.size;
            // (A row that counts leaves of its market in the tree keeps its head with no unit: 17.8j, 2.)
            if row.net == 0 && row.gross == 0 && row.entered == 0 {
                (row.key, row.scale, row.price_max) = (Key::HEAD, 0, 0);
            }
            s.basis = s.basis.checked_sub(basis).filter(|x| *x != i64::MIN).ok_or_else(beyond)?;
            at.push(r);
        }
        if n.asked < 0 {
            // (What the account paid for the legs, which the note holds.)
            s.held = s.held.checked_add(held_for(n.asked, holds)).ok_or_else(beyond)?;
        }
        // (Called: nothing is paid, and what is asked is counted in its class. Not called: what is not paid is
        // owed, and the note of the rest says it.)
        let (pay, a1, a2) = if self.called == 0 {
            let pay = n.asked.max(0).min(s.held);
            (pay, n.asked.max(0) - pay, 0)
        } else {
            let (a1, a2) = self.classed(n.asked, &n.legs, step);
            (0, a1, a2.checked_add(young).ok_or_else(beyond)?)
        };
        s.held -= pay;
        s.owed = s.owed.checked_add(a1).ok_or_else(beyond)?;
        s.claims.owed2 = s.claims.owed2.checked_add(a2).ok_or_else(beyond)?;
        Ok((s, at, pay, (a1 > 0 || a2 > 0).then_some(Claim { sid: n.sid, key: n.key, amount1: a1, paid1: 0, amount2: a2, paid2: 0 })))
    }

    /// `KpfTLeftVoid`: the first leg of a note that is no leg of these books (no futures leg; no row of its
    /// market; or its market's row has fewer units than the note's legs of it): such a note is applied by nothing,
    /// and is spent, the shard as it is (review 51, F81). `None`: every leg is in the books.
    pub fn gone_at(&self, n: &Left) -> Option<usize> {
        n.legs.iter().position(|l| {
            // (An option leg is a leg of the dated rows: `KpfTLeftVoid` judges none, and counts the units of
            // futures legs alone against a futures row. Review T18, U1.)
            if crate::crow::option_of(l).is_some() {
                return false;
            }
            let Ok((head, _, _)) = Shard::book_of(l) else { return true };
            let Some(row) = self.rows.iter().find(|row| row.head() == head) else { return true };
            let units: i64 = n.legs.iter().filter(|x| (x.market, x.scale, x.price_max) == (l.market, l.scale, l.price_max) && x.size > 0 && crate::crow::option_of(x).is_none()).map(|x| x.size).fold(0i64, |a, b| a.saturating_add(b));
            row.gross < units
        })
    }

    pub fn gone(&self, n: &Left) -> bool {
        self.gone_at(n).is_some()
    }

    /// `KpfTLeftVoid`: the shard after a note of a leg that is no leg of its books is spent: as it is, but that it
    /// holds what the note held for it (asked below zero: what an account paid. Review 52, F84).
    pub fn voided(&self, n: &Left) -> Result<Shard> {
        self.voided_holding(n, n.value())
    }

    /// As `voided`, of a note whose output holds `holds` (as `left_holding`).
    pub fn voided_holding(&self, n: &Left, holds: i64) -> Result<Shard> {
        if n.sid != self.sid || n.legs.is_empty() || n.legs.len() > 6 || !self.gone(n) {
            return Err(Error::Invalid("a note is spent so where it is to this shard, of one to six legs, and one of them is no leg of the books".into()));
        }
        let held = self.held.checked_add(held_for(n.asked, holds)).ok_or_else(|| Error::Invalid("the books are beyond what is counted".into()))?;
        Ok(Shard { held, ..*self })
    }

    /// `KpfTMarkNote.apply`: the shard once the note that a leg was marked is applied. What is asked is what the
    /// leg gained between its two entries (a note that asks anything else is applied by nothing); what the legs
    /// were entered at moves by as much, so that the equity is what it was at any price; and it is paid as far as
    /// the shard holds (nothing while it is called), the rest owed and written as a note of the rest. No row is
    /// read: it is applied whatever the rows hold by then.
    pub fn marked(&self, n: &MarkNote) -> Result<(Shard, i64, Option<Rest>)> {
        let (s, pay, claim) = self.marked_class(n)?;
        match claim {
            Some(c) if c.amount2 != 0 => Err(Error::Invalid("the mark's amount is a claim of the second class (its leg is young at the call, or a claim was paid already): `marked_class` gives it".into())),
            c => Ok((s, pay, c.map(|c| Rest { sid: c.sid, key: c.key, amount: c.amount1 }))),
        }
    }

    /// `KpfTMarkNote.apply` (docs 16.3, 17.8c): as `left_class`, of the note that a leg was marked: under a call
    /// its amount is of the second class where the leg is young or anything was paid to a claim.
    pub fn marked_class(&self, n: &MarkNote) -> Result<(Shard, i64, Option<Claim>)> {
        let no = |why: &str| Err(Error::Invalid(format!("the note that a leg was marked is not applied: {why}")));
        if n.sid != self.sid {
            return no("it is to another shard");
        }
        let l = &n.leg;
        if (l.dir != LONG && l.dir != crate::state::SHORT) || l.size <= 0 || l.scale <= 0 || l.entry <= 0 || n.entry <= 0 || n.asked <= 0 {
            return no("it is of a futures leg, an entry, and something asked");
        }
        let beyond = || Error::Invalid("the books are beyond what is counted".into());
        let fits = |v: Option<i64>| v.filter(|x| *x != i64::MIN).ok_or_else(beyond);
        let (we, wn) = (fits(worth(l.size, l.scale, l.entry))?, fits(worth(l.size, l.scale, n.entry))?);
        let dlt = fits(wn.checked_sub(we))?;
        let gained = if l.dir == LONG { dlt } else { fits(0i64.checked_sub(dlt))? };
        if n.asked != gained {
            return no("it asks other than what the leg gained between its two entries");
        }
        let mut s = *self;
        // (Side B's units in the leg: the size below zero where A is long.)
        s.basis = fits(if l.dir == LONG { s.basis.checked_sub(dlt) } else { s.basis.checked_add(dlt) })?;
        let (pay, a1, a2) = if self.called == 0 {
            let pay = n.asked.min(s.held);
            (pay, n.asked - pay, 0)
        } else {
            let (a1, a2) = self.classed(n.asked, std::slice::from_ref(l), false);
            (0, a1, a2)
        };
        s.held -= pay;
        s.owed = s.owed.checked_add(a1).ok_or_else(beyond)?;
        s.claims.owed2 = s.claims.owed2.checked_add(a2).ok_or_else(beyond)?;
        Ok((s, pay, (a1 > 0 || a2 > 0).then_some(Claim { sid: n.sid, key: n.key, amount1: a1, paid1: 0, amount2: a2, paid2: 0 })))
    }

    /// `KpfTOwed.apply`: the shard once a note of what an account paid of what it owed is applied, the note's
    /// output holding `holds`: the shard holds what that is above a note's least, and no more than the note says.
    pub fn paid_in(&self, n: &Owed, holds: i64) -> Result<Shard> {
        if n.sid != self.sid || n.amount <= 0 {
            return Err(Error::Invalid("the note of a payment is to this shard, and of something".into()));
        }
        let more = holds.saturating_sub(NOTE_MIN).clamp(0, n.amount);
        let held = self.held.checked_add(more).ok_or_else(|| Error::Invalid("the books are beyond what is counted".into()))?;
        Ok(Shard { held, ..*self })
    }

    /// `KpfTLeft.rest`: a note of the rest is paid as far as the shard holds.
    pub fn rest_paid(&self, r: &Rest) -> Result<(Shard, i64, Option<Rest>)> {
        if r.sid != self.sid || r.amount <= 0 {
            return Err(Error::Invalid("the note of the rest is to this shard, and of something".into()));
        }
        if r.amount > self.owed {
            // (Never counted: spent, and nothing is paid, called or not.)
            return Ok((*self, 0, None));
        }
        if self.called != 0 {
            return Err(Error::Invalid("the shard is called: its own script pays every note of the rest its share".into()));
        }
        let pay = r.amount.min(self.held);
        if pay <= 0 {
            return Err(Error::Invalid("the shard holds nothing to pay it with".into()));
        }
        Ok((Shard { held: self.held - pay, owed: self.owed - pay, ..*self }, pay, (pay < r.amount).then_some(Rest { amount: r.amount - pay, ..*r })))
    }
}

/// A call of a shard, stated by anyone (`docs/ARCHITECTURE-CLEARING.md` 14.6): what it says is a draw's bytes with
/// nothing where the amount is.
pub const KIND_CALL: i64 = 11;
/// The call's tag among the objects under a shard's id.
pub const NOTE_CALL: i64 = 11;
/// A call's bond is no more than so many of the terms' least fee of a keeper.
pub const CALL_BOND_FEES: i64 = 100;
/// A draw's bond is this at the least.
pub const DRAW_BOND_MIN: i64 = 100_000_000;
/// What a call holds, out of its shard's carrier.
pub const CALL_HOLD: i64 = SHARD_CARRIER / 2;

/// The call of a shard: an object of its own under the shard's id, written where a call is applied. Every step
/// under the call is shown it: the rows' names as they stood, and the price of each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Called {
    pub sid: [u8; 32],
    pub time: i64,
    pub heads: Places<RowHead>,
    pub prices: Places<i64>,
}

impl Called {
    /// The list of the call's prices (docs 17.8b): one for each market a row has (the market plain, its price's
    /// limit, no day, the price), in the order of the markets. (A market of two rows has one price: the first
    /// row's.)
    pub fn list(&self) -> Vec<Priced> {
        let mut list: Vec<Priced> = vec![];
        for (h, p) in self.heads.iter().zip(&self.prices).filter(|(h, _)| h.key.market != [0; 32]) {
            if list.iter().all(|x| x.market != h.key.market) {
                list.push(Priced { market: h.key.market, limit: h.price_max, day: 0, price: *p });
            }
        }
        list.sort_by_key(|p| p.numbers());
        list
    }

    /// What the call's object holds: the list, 56 bytes a price.
    pub fn body(&self) -> Vec<u8> {
        self.list().iter().flat_map(|p| p.bytes()).collect()
    }

    /// The hash of the call of a shard at a time, of a list's bytes (what `KpfTCallEnd` writes out of what a
    /// call's statement says).
    pub fn hash_of(sid: &[u8; 32], time: i64, list: &[u8]) -> [u8; 32] {
        blake2b(&[i9(12), p33(sid), i9(NOTE_CALL), i9(time), p33(&blake2b(list))].concat())
    }

    pub fn hash(&self) -> [u8; 32] {
        Called::hash_of(&self.sid, self.time, &self.body())
    }

    /// What the call's output holds: half of what the shard's output held beside what the shard holds (an output
    /// under a covenant weighs four times a plain one in a block, and a step writes the call again beside a
    /// note: at a note's least the two were four fifths of a block).
    pub fn value(&self) -> i64 {
        CALL_HOLD
    }
}

/// What a draw states (kind 5): the amount, and a price a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Draw {
    pub amount: i64,
    pub prices: Places<i64>,
}

impl Draw {
    /// The amount and the prices of a statement's trace (docs 17.8b): for each row the price its list has at the
    /// place the row names (nothing where it names none). What the statement says is the trace: `Shard::traced`.
    pub fn read(said: &[u8]) -> Option<Draw> {
        let t = Traced::read(said)?;
        let mut prices = Places::<i64>::default();
        for (p, r) in prices.iter_mut().zip(&t.rows) {
            *p = usize::try_from(r.at).ok().and_then(|i| t.list.get(i)).map_or(0, |e| e.price);
        }
        Some(Draw { amount: t.amount, prices })
    }
}

// The engine's integers are 64 bits and checked: a sum or a product beyond them fails the script. These follow the
// scripts' arithmetic step by step and are `None` where the script would fail.
fn worth(n: i64, sc: i64, p: i64) -> Option<i64> {
    (n / sc).checked_mul(p)?.checked_add((n % sc).checked_mul(p / sc)?)?.checked_add((n % sc).checked_mul(p % sc)? / sc)
}

fn rate_up(w: i64, r: i64) -> Option<i64> {
    (w / BPS).checked_mul(r)?.checked_add((w % BPS).checked_mul(r)?.checked_add(BPS - 1)? / BPS)
}

/// The rate on the gross: half the maintenance rate less three times the tolerance. What a book that is set off
/// keeps for the time between one side's leaving and a call (`docs/ARCHITECTURE-CLEARING.md` 12).
pub fn gross_rate(t: &crate::cross::XTerms) -> i64 {
    ((t.mmr_bps - 3 * t.g_bps) / 2).max(0)
}

/// The requirement at its upper level (`up`: both rates counted the tolerance more; a draw and an entry are held to
/// it) or its lower (the rate on the net the tolerance less; a call is right below it), of the worth `wn` of the
/// net units and `wg` of the gross.
fn need_of(wn: i64, wg: i64, t: &crate::cross::XTerms, up: bool) -> Option<i64> {
    let (rn, rg) = if up { (t.mmr_bps + t.g_bps, gross_rate(t) + t.g_bps) } else { ((t.mmr_bps - t.g_bps).max(0), gross_rate(t)) };
    rate_up(wn, rn)?.checked_add(rate_up(wg, rg)?)
}

impl Shard {
    /// The bond of a statement on the shard: its bond rate of what it holds (`KpfTChange`'s arithmetic).
    pub fn bond(&self) -> i64 {
        // (A draw's bond is a KAS at the least: `KpfTChange`. A draw that is not the rule's, answered wrong by
        // its owner's other key, gives the shard back for half of it, and no age holds a draw: reviews 54 and
        // 56, F88, F101.)
        self.shell().bond().max(DRAW_BOND_MIN)
    }

    /// (The worth of the net units summed, of the gross, and what side B's net units are worth: above zero where it
    /// is long.) `None` where a row that has a market has no price within its limit, or a number is beyond 64 bits.
    fn worths(&self, p: &Places<i64>) -> Option<(i64, i64, i64)> {
        let (mut wn, mut wg, mut sn) = (0i64, 0i64, 0i64);
        for (row, p) in self.rows.iter().zip(p) {
            if row.key.market == [0; 32] {
                continue;
            }
            if *p <= 0 || *p > row.price_max || row.scale <= 0 {
                return None;
            }
            let w = worth(row.net.checked_abs()?, row.scale, *p)?;
            wn = wn.checked_add(w)?;
            sn = if row.net < 0 { sn.checked_sub(w)? } else { sn.checked_add(w)? };
            wg = wg.checked_add(worth(row.gross.checked_abs()?, row.scale, *p)?)?;
        }
        Some((wn, wg, sn))
    }

    /// What the shard is asked at the prices `p` (one a row), at its upper level, with `KpfTShardEnd`'s
    /// arithmetic: the maintenance rate of the worth of every row's net units and the rate on the gross of the
    /// worth of every row's gross units, each counted the tolerance more.
    pub fn requirement(&self, p: impl Into<Places<i64>>) -> Option<i64> {
        self.level(p, true)
    }

    /// The requirement at its upper level (`up`) or its lower.
    pub fn level(&self, p: impl Into<Places<i64>>, up: bool) -> Option<i64> {
        let (wn, wg, _) = self.worths(&p.into())?;
        need_of(wn, wg, &self.t, up)
    }

    /// The shard's equity at the prices `p`: what it holds, and what its net units are worth there, less what its
    /// legs were entered at.
    pub fn equity(&self, p: impl Into<Places<i64>>) -> Option<i64> {
        let (_, _, sn) = self.worths(&p.into())?;
        self.held.checked_add(sn)?.checked_sub(self.basis)
    }

    /// `KpfTShardEnd`, step by step: whether the end of a draw is within the rule before its outputs are looked at
    /// (`Some(true)`), is not (`Some(false)`), or cannot be computed by the script (`None`: a sum or a product is
    /// beyond 64 bits, and the script fails whatever verdict is shown). Where it cannot be computed nobody answers
    /// a dispute of the end, the dispute lapses and the draw ends with both bonds returned: such a shard is drawn
    /// from only while nobody disputes. Its rows are that large only by legs its owner signed for.
    pub fn draw_end_judged(&self, d: &Draw, claimer: &[u8; 32], t0: i64) -> Option<bool> {
        let mut ok = *claimer == self.owner && d.amount > 0;
        let (mut wn, mut wg, mut sn) = (0i64, 0i64, 0i64);
        for (row, p) in self.rows.iter().zip(&d.prices) {
            if row.key.market == [0; 32] {
                continue;
            }
            let (net, gross) = (row.net.checked_abs()?, row.gross.checked_abs()?);
            if *p <= 0 || *p > row.price_max || row.scale <= 0 {
                ok = false;
            }
            if ok {
                let w = worth(net, row.scale, *p)?;
                wn = wn.checked_add(w)?;
                sn = if row.net < 0 { sn.checked_sub(w)? } else { sn.checked_add(w)? };
                wg = wg.checked_add(worth(gross, row.scale, *p)?)?;
            }
        }
        let need = need_of(wn, wg, &self.t, true)?;
        // Of coins it holds; its equity then at the upper level; not while it is called. (The script computes the
        // equity whatever it found before: where that does not fit it gives no verdict. And the engine's numbers
        // have no least value of 64 bits.)
        let has = self.held.checked_sub(d.amount)?;
        let equity = has.checked_add(sn)?.checked_sub(self.basis)?;
        if has == i64::MIN || equity == i64::MIN || has.checked_add(sn)? == i64::MIN {
            return None;
        }
        // (Not before the time the shard says; and the time a draw writes is computed whatever was found.)
        self.draw_time(t0)?;
        Some(ok && has >= 0 && self.called == 0 && equity >= need && t0 >= self.expiry)
    }

    /// The time a draw stated at `t0` writes, before which no draw is stated again: its own time and twice the
    /// longest a statement lives (the lateness and two dispute periods). It stands where a position has its
    /// expiry (`docs` 14.5).
    pub fn draw_time(&self, t0: i64) -> Option<i64> {
        let life = self.t.late_daa.checked_add(self.dispute_daa.checked_mul(2)?)?;
        t0.checked_add(life.checked_mul(2)?).filter(|x| *x != i64::MIN)
    }

    /// The shard once `amount` is drawn by a draw stated at `t0`.
    pub fn drawn(&self, amount: i64, t0: i64) -> Shard {
        Shard { held: self.held - amount, expiry: self.draw_time(t0).unwrap_or(self.expiry), ..*self }
    }

    /// `KpfTShardEnd`: whether the end of a draw stated by `claimer` is right for this shard, whose output held
    /// `held` when it was stated.
    pub fn draw_end_right(&self, d: &Draw, claimer: &[u8; 32], outs: &Outs, held: i64, t0: i64) -> bool {
        if self.draw_end_judged(d, claimer, t0) != Some(true) {
            return false;
        }
        Outs::of(&[Out { key: false, to: self.drawn(d.amount, t0).hash(), value: held - d.amount }, Out { key: true, to: *claimer, value: d.amount }]).is_some_and(|o| o == *outs)
    }

    /// `KpfTShardPrice`: whether the price of the row `r` (1 to 4) of a draw is within the tolerance of the
    /// committee's `c`. A row with no market is of nothing: right.
    pub fn draw_price_right(&self, d: &Draw, r: usize, c: i64) -> bool {
        let Some(row) = self.rows.get(r.wrapping_sub(1)) else { return true };
        if row.key.market == [0; 32] {
            return true;
        }
        let (a, g) = (d.prices[r - 1], self.t.g_bps);
        let p = c.min(row.price_max);
        a > 0 && a >= p * (BPS - g) / BPS && a <= ((p * (BPS + g) + BPS - 1) / BPS).min(row.price_max)
    }

    /// The first point of a stated draw that is wrong at the committee's prices `c` (one a row): 0 the end, r the
    /// price of row r. `None`: the draw is the rule's.
    pub fn draw_wrong_point(&self, d: &Draw, claimer: &[u8; 32], outs: &Outs, held: i64, t0: i64, c: impl Into<Places<i64>>) -> Option<i64> {
        match self.traced(d, false) {
            Some(t) => self.trace_wrong_point(&t, false, claimer, outs, held, t0, c),
            None => Some(0),
        }
    }

    /// The first point of a stated trace that is wrong, at the committee's prices `c` (one a row of the books):
    /// 0 the end, 1 to 4 a row, 4 + e the price e of the list (docs 17.8b). `None`: it is the rule's.
    #[allow(clippy::too_many_arguments)]
    pub fn trace_wrong_point(&self, t: &Traced, call: bool, claimer: &[u8; 32], outs: &Outs, held: i64, t0: i64, c: impl Into<Places<i64>>) -> Option<i64> {
        let end = if call { self.call_trace_end_right(t, claimer, outs, held, t0) } else { self.draw_trace_end_right(t, claimer, outs, held, t0) };
        if !end {
            return Some(0);
        }
        if let Some(r) = (1..=SHARD_ROWS).find(|r| self.trace_row_judged(t, *r, call) != Some(true)) {
            return Some(r as i64);
        }
        let c: Places<i64> = c.into();
        // (An entry no row is of is priced as any other, at a price this is not given: only its order and its day
        // are judged here. Review T9, Q2.)
        let cited = |x: &Priced| self.rows.iter().position(|r| r.key.market == x.market && !r.no_market()).map_or(x.price, |i| c[i]);
        (1..=t.list.len()).find(|e| !self.trace_price_right(t, *e, cited(&t.list[*e - 1]))).map(|e| 4 + e as i64)
    }

    /// `KpfTShardEnd`, of a trace: whether the end of a draw is right, its outputs with it.
    pub fn draw_trace_end_right(&self, t: &Traced, claimer: &[u8; 32], outs: &Outs, held: i64, t0: i64) -> bool {
        if self.draw_trace_end_judged(t, claimer, t0) != Some(true) {
            return false;
        }
        Outs::of(&[Out { key: false, to: self.drawn(t.amount, t0).hash(), value: held - t.amount }, Out { key: true, to: *claimer, value: t.amount }]).is_some_and(|o| o == *outs)
    }

    /// The outputs of a call that is the rule's by its trace: the shard called, the call (of the list the trace
    /// says), and the fee to the caller.
    pub fn call_trace_outs(&self, t: &Traced, claimer: &[u8; 32], t0: i64, held: i64) -> Option<Outs> {
        let (ok, pay) = self.call_trace_end_judged(t, t0, held)?;
        if !ok {
            return None;
        }
        let next = Shard { held: self.held - pay, called: t0, ..*self };
        let mut v = vec![(Out { key: false, to: next.hash(), value: held - pay - CALL_HOLD }, false), (Out { key: false, to: Called::hash_of(&self.sid, t0, &t.list_bytes()), value: CALL_HOLD }, true)];
        if pay > 0 {
            v.push((Out { key: true, to: *claimer, value: pay }, false));
        }
        Outs::with_notes(&v)
    }

    /// `KpfTCallEnd`, of a trace: whether the end of a call is right, its outputs with it.
    pub fn call_trace_end_right(&self, t: &Traced, claimer: &[u8; 32], outs: &Outs, held: i64, t0: i64) -> bool {
        self.call_trace_outs(t, claimer, t0, held).is_some_and(|o| o == *outs)
    }

    /// The most the owner draws at the prices `p`.
    pub fn draw_max(&self, p: impl Into<Places<i64>>) -> i64 {
        let p: Places<i64> = p.into();
        if self.called != 0 {
            return 0;
        }
        match (self.equity(p), self.requirement(p)) {
            (Some(e), Some(need)) => (e - need).min(self.held).max(0),
            _ => 0,
        }
    }

    /// The statement of a draw of `amount` at the prices `p` by the shard's owner, on a shard whose output holds
    /// `held`. Refused where the shard would hold less than its requirement at those prices.
    pub fn draw(&self, amount: i64, p: impl Into<Places<i64>>, t0: i64, held: i64) -> Result<(Stated, Draw)> {
        let p: Places<i64> = p.into();
        let d = Draw { amount, prices: p };
        let outs = Outs::of(&[Out { key: false, to: self.drawn(amount, t0).hash(), value: held - amount }, Out { key: true, to: self.owner, value: amount }]).ok_or_else(|| Error::Invalid("the draw's outputs".into()))?;
        if !self.draw_end_right(&d, &self.owner, &outs, held, t0) {
            return Err(Error::Invalid(format!("the shard would hold less than its requirement at these prices (it holds {}, and is asked {:?}), or the amount is not above zero, or it is stated before the time the shard says ({})", self.held, self.requirement(p), self.expiry)));
        }
        let t = self.traced(&d, false).ok_or_else(|| Error::Invalid("the rows at these prices are beyond what is counted: nobody states this draw".into()))?;
        Ok((Stated { position: self.hash(), window: self.dispute_daa, late: self.t.late_daa, kind: KIND_DRAW, t0, arg: t.arg(), outs, bond: self.bond(), claimer: self.owner, held }, d))
    }

    /// The bond of a call: the shard's, no more than a hundred times the terms' least fee of a keeper, and no less
    /// than a note's least (`KpfTChange`: a shard that holds nothing is called still, and a large one by anyone).
    pub fn call_bond(&self) -> i64 {
        // (The cap where it is a number a script computes: review 54, F91.)
        let b = self.shell().bond();
        (if self.t.keeper_fee_min <= i64::MAX / CALL_BOND_FEES { b.min(CALL_BOND_FEES * self.t.keeper_fee_min) } else { b }).max(NOTE_MIN)
    }

    /// `KpfTCallEnd`, step by step, before the outputs are looked at: whether a call at the prices `d` says (and
    /// nothing where a draw says its amount), stated at `t0` on the shard whose output held `held`, is within the
    /// rule; and what the caller is paid out of what the shard holds. `None` where the script computes no verdict
    /// (a sum or a product beyond 64 bits). The prices are each moved the tolerance in the shard's favour: up
    /// where its side is long in the row, down where it is short or has no net unit.
    pub fn call_end_judged(&self, d: &Draw, t0: i64, held: i64) -> Option<(bool, i64)> {
        let mut ok = d.amount == 0 && t0 > 0;
        let g = self.t.g_bps;
        let (mut wn, mut wg, mut sn) = (0i64, 0i64, 0i64);
        for (row, p) in self.rows.iter().zip(&d.prices) {
            if row.key.market == [0; 32] {
                continue;
            }
            let (net, gross) = (row.net.checked_abs()?, row.gross.checked_abs()?);
            let pm = row.price_max;
            if *p <= 0 || *p > pm || row.scale <= 0 {
                ok = false;
            }
            if ok {
                let down = p.checked_mul(BPS - g)? / BPS;
                let q = if row.net > 0 { (p.checked_mul(BPS + g)?.checked_add(BPS - 1)? / BPS).min(pm) } else { down };
                let w = worth(net, row.scale, q)?;
                wn = wn.checked_add(w)?;
                sn = if row.net < 0 { sn.checked_sub(w)? } else { sn.checked_add(w)? };
                wg = wg.checked_add(worth(gross, row.scale, q)?)?;
            }
        }
        let need = need_of(wn, wg, &self.t, false)?;
        let with = self.held.checked_add(sn)?;
        let equity = with.checked_sub(self.basis)?;
        if with == i64::MIN || equity == i64::MIN {
            return None;
        }
        // (The fee whole, or none: a part of it is no payable amount.)
        let fee = self.t.keeper_fee_min.max(0);
        let pay = if self.held < fee { 0 } else { fee };
        let left = held.checked_sub(pay)?.checked_sub(CALL_HOLD)?;
        Some((ok && self.called == 0 && equity < need && left >= CALL_HOLD, pay))
    }

    /// Whether a call is due at the prices `p`: what `call_end_judged` computes, for whoever watches.
    pub fn call_due(&self, p: impl Into<Places<i64>>) -> bool {
        self.call_end_judged(&Draw { amount: 0, prices: p.into() }, 1, i64::MAX / 2).is_some_and(|(ok, _)| ok)
    }

    /// The shard called at `t0`, having paid `pay`; and the call, at the prices `p`. (The call names the market of
    /// every futures row that has a unit: a head of no unit is in no list. Review T17, V4. With units in dated rows
    /// the list is the trace's: with the call's step for option legs, 17.8j's step C.)
    pub fn called_at(&self, p: impl Into<Places<i64>>, t0: i64, pay: i64) -> (Shard, Called) {
        (Shard { held: self.held - pay, called: t0, ..*self }, Called { sid: self.sid, time: t0, heads: self.rows.map(|r| if r.gross != 0 || r.net != 0 { r.head() } else { RowHead::default() }), prices: p.into() })
    }

    /// As `called_at`, of a shard whose tree is `books` (docs 17.8j, C4; review T17, V4): the call names the market
    /// of every row that has a unit, a futures row's and a dated row's by its market's head among the four: the
    /// list a call's trace says (`traced_with`).
    pub fn called_at_with(&self, books: &crate::books::Books, p: impl Into<Places<i64>>, t0: i64, pay: i64) -> (Shard, Called) {
        let dated: Vec<[u8; 32]> = books.in_order().iter().skip(1).filter(|r| r.gross != 0).map(|r| r.key.market).collect();
        let heads = self.rows.map(|r| if r.gross != 0 || r.net != 0 || (!r.no_market() && dated.contains(&r.key.market)) { r.head() } else { RowHead::default() });
        (Shard { held: self.held - pay, called: t0, ..*self }, Called { sid: self.sid, time: t0, heads, prices: p.into() })
    }

    /// The outputs of a call that is the rule's: the shard called, the call, and the fee to the caller.
    pub fn call_outs(&self, d: &Draw, claimer: &[u8; 32], t0: i64, held: i64) -> Option<Outs> {
        let (ok, pay) = self.call_end_judged(d, t0, held)?;
        if !ok {
            return None;
        }
        let (next, call) = self.called_at(&d.prices, t0, pay);
        let mut v = vec![(Out { key: false, to: next.hash(), value: held - pay - CALL_HOLD }, false), (Out { key: false, to: call.hash(), value: call.value() }, true)];
        if pay > 0 {
            v.push((Out { key: true, to: *claimer, value: pay }, false));
        }
        Outs::with_notes(&v)
    }

    /// `KpfTCallEnd`: whether the end of a call stated by `claimer` is right for this shard, whose output held
    /// `held` when it was stated.
    pub fn call_end_right(&self, d: &Draw, claimer: &[u8; 32], outs: &Outs, held: i64, t0: i64) -> bool {
        self.call_outs(d, claimer, t0, held).is_some_and(|o| o == *outs)
    }

    /// The first point of a stated call that is wrong at the committee's prices `c` (one a row): 0 the end, r the
    /// price of row r. `None`: the call is the rule's.
    pub fn call_wrong_point(&self, d: &Draw, claimer: &[u8; 32], outs: &Outs, held: i64, t0: i64, c: impl Into<Places<i64>>) -> Option<i64> {
        match self.traced(d, true) {
            Some(t) => self.trace_wrong_point(&t, true, claimer, outs, held, t0, c),
            None => Some(0),
        }
    }

    /// The statement of a call at the prices `p` by `claimer` (anyone), on a shard whose output holds `held`.
    /// Refused where the shard is not below its lower level at those prices.
    pub fn call(&self, p: impl Into<Places<i64>>, t0: i64, held: i64, claimer: [u8; 32]) -> Result<(Stated, Draw)> {
        let d = Draw { amount: 0, prices: p.into() };
        let outs = self.call_outs(&d, &claimer, t0, held).ok_or_else(|| Error::Invalid("no call is due at these prices: the shard's equity, each price moved the tolerance in its favour, is at its lower level or above; or the shard is called; or a row that has a market has no price".into()))?;
        let t = self.traced(&d, true).ok_or_else(|| Error::Invalid("the rows at these prices are beyond what is counted: nobody states this call".into()))?;
        Ok((Stated { position: self.hash(), window: self.dispute_daa, late: self.t.late_daa, kind: KIND_CALL, t0, arg: t.arg(), outs, bond: self.call_bond(), claimer, held }, d))
    }

    /// `KpfTCallPay.claim`: a note of the rest is paid its share by its called shard, once no row has a unit: the
    /// amount times what the shard holds over what it owes, rounded down, and no more than the amount. Above 3,000,000,000 sompi
    /// owed both are counted in a larger unit (what is owed rounded up, what is held down), so that no product
    /// is beyond a script's numbers. The shard after, and what is paid.
    pub fn claim_paid(&self, r: &Rest) -> Result<(Shard, i64)> {
        self.claim_paid_at(&r.claim(), i64::MAX).map(|(s, pay, _)| (s, pay))
    }

    /// The time from which a called shard pays its claims: the call's time, a lateness and three dispute periods
    /// (every statement of before the call is applied by then, and its note counted).
    pub fn claims_time(&self) -> Option<i64> {
        self.called.checked_add(self.t.late_daa)?.checked_add(self.dispute_daa.checked_mul(3)?)
    }

    /// `KpfTCallPay.claim` (docs 17.8c): a claim is paid by its called shard, once no row has a unit and an input
    /// of the transaction landed at `landed`, at or after the time of the claims. W is all the shard ever held
    /// for its claims (what it holds and what it has paid). The first class: the amount where W covers what is
    /// owed, else its part of W over what is owed, rounded down (above 3,000,000,000 sompi owed both are counted
    /// in a larger unit: what is owed rounded up, W down); less what it was paid, and no more than is held or
    /// than its class is still owed. The second class, of what W has beyond what the first is owed, the same.
    /// A part below a note's least waits; a claim that is whole thereby is spent whatever its size. The shard
    /// after, what is paid, and the claim as it goes on where it is not whole.
    pub fn claim_paid_at(&self, c: &Claim, landed: i64) -> Result<(Shard, i64, Option<Claim>)> {
        let no = |why: &str| Err(Error::Invalid(format!("the claim is not paid: {why}")));
        // (By the sum of the gross of every row, as `KpfTCallPay`: a market's head of no unit may stand. T17, V3.)
        if c.sid != self.sid || self.called == 0 || self.gross_sum() != 0 {
            return no("a claim is paid by its shard, called, once no row of it has a unit");
        }
        if self.claims_time().is_none_or(|t| landed < t) {
            return no("no claim is paid before the call's time, a lateness and three dispute periods");
        }
        let (c1, d1, c2, d2, h) = (self.owed, self.claims.paid1, self.claims.owed2, self.claims.paid2, self.held);
        if c.paid1 < 0 || c.paid1 > c.amount1 || c.amount1 > c1 || c.paid2 < 0 || c.paid2 > c.amount2 || c.amount2 > c2 || c.amount1.checked_add(c.amount2).is_none_or(|x| x <= 0) || h < 0 {
            return no("its amounts are no less than what it was paid and no more than its shard owes its class");
        }
        let beyond = || Error::Invalid("the claim is not paid: a sum is beyond what is counted".into());
        let w = h.checked_add(d1).and_then(|x| x.checked_add(d2)).ok_or_else(beyond)?;
        let part = |a: i64, over: i64, of: i64| {
            let u = [(3_000_000_000_000_000_000i64, 1_000_000_000_000i64), (3_000_000_000_000_000, 1_000_000_000), (3_000_000_000_000, 1_000_000), (3_000_000_000, 1_000)].iter().find(|(at, _)| of >= *at).map(|(_, u)| *u).unwrap_or(1);
            let (x, o) = (over / u, (of - 1) / u + 1);
            (a / o) * x + (a % o) * x / o
        };
        let e1 = if w >= c1 { c.amount1 } else { part(c.amount1, w, c1) };
        let p1 = (e1 - c.paid1).min(h).min(c1 - d1).max(0);
        let s = w - c1;
        let e2 = if s <= 0 {
            0
        } else if s >= c2 {
            c.amount2
        } else {
            part(c.amount2, s, c2)
        };
        let p2 = (e2 - c.paid2).min(s - d2).min(c2 - d2).max(0);
        let pay = p1 + p2;
        let whole = c.paid1 + p1 == c.amount1 && c.paid2 + p2 == c.amount2;
        if pay < NOTE_MIN && !whole {
            return no("its part is below a note's least: it waits");
        }
        let next = Shard { held: h - pay, claims: Claims { paid1: d1 + p1, owed2: c2, paid2: d2 + p2 }, ..*self };
        Ok((next, pay, (!whole).then_some(Claim { paid1: c.paid1 + p1, paid2: c.paid2 + p2, ..*c })))
    }

    /// `KpfTCallPay.off`: the call ends where no row has a unit and nothing is owed by claims: the shard not
    /// called.
    pub fn call_off(&self, call: &Called) -> Result<Shard> {
        if call.sid != self.sid || self.called == 0 || self.gross_sum() != 0 || self.claims.paid1 != self.owed || self.claims.paid2 != self.claims.owed2 {
            return Err(Error::Invalid("a call ends where its shard is called, no row of it has a unit and what it has paid its claims is what it owes them".into()));
        }
        Ok(Shard { called: 0, owed: 0, claims: Claims::default(), ..*self })
    }

    /// The form a statement on the shard shows: its terms and the books' hash.
    pub fn form(&self) -> Vec<u8> {
        [self.terms(), self.books_hash().to_vec()].concat()
    }
}

// ---------------------------------------------------------------- a draw and a call as traces (docs 17.8b)

/// Eight bytes as a script reads a number: the magnitude, and the top bit its sign.
fn num8(b: &[u8]) -> i64 {
    let mut x: [u8; 8] = b.try_into().expect("eight bytes");
    let below = x[7] & 0x80 != 0;
    x[7] &= 0x7f;
    let m = i64::from_le_bytes(x);
    if below { -m } else { m }
}

/// A price of the list a draw and a call say (docs 17.8b), 56 bytes: a market, plain (one entry a market, whatever
/// rows are its), its price's limit, a day (nothing: the price now) and the price.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Priced {
    pub market: [u8; 32],
    pub limit: i64,
    pub day: i64,
    pub price: i64,
}

impl Priced {
    pub fn bytes(&self) -> Vec<u8> {
        [self.market.to_vec(), sm8(self.limit).to_vec(), sm8(self.day).to_vec(), sm8(self.price).to_vec()].concat()
    }

    /// What the list is ordered by: the market's 32 bytes as four numbers of 8 bytes, then the day.
    pub fn numbers(&self) -> [i64; 5] {
        let m = |i: usize| num8(&self.market[8 * i..8 * i + 8]);
        [m(0), m(1), m(2), m(3), self.day]
    }

    pub fn below(&self, o: &Priced) -> bool {
        self.numbers() < o.numbers()
    }
}

/// A row of a draw's or a call's trace, 40 bytes: the row's place in the books, the place of its price in the
/// list, and the three sums up to and with it: what the net units are worth, what the gross units are worth, and
/// what the clearing side's net units are worth signed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceRow {
    pub place: i64,
    pub at: i64,
    pub wn: i64,
    pub wg: i64,
    pub sn: i64,
}

/// By what a dated row is taken out of a shard's books (docs 17.8j, 4): its owner's signature; its day's leaf, written;
/// or the shard being called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowOutBy {
    Owner,
    DayWritten,
    Called,
}

impl Shard {
    /// `KpfTRowIn.put` (docs 17.8j, 2 and 4): the owner lists a dated row. `books` are the shard's (their root is
    /// its root). The row `r` of the four is the market's futures row, whose scale and limit the row takes; or an
    /// empty place where no row is the market's, which becomes its head with no unit. Its ninth field counts the
    /// market's leaves in the tree: one more, and two where the day's leaf is put in with the row. Not while the
    /// shard is called.
    pub fn row_in(&self, books: &crate::books::Books, key: Key, scale: i64, limit: i64, r: usize, place: usize, place_day: Option<usize>) -> Result<(Shard, crate::books::Books)> {
        let no = |why: &str| Err(Error::Invalid(format!("no row is put into the shard's books: {why}")));
        if self.called != 0 {
            return no("it is called");
        }
        if books.root() != self.root {
            return no("the books shown are not the shard's");
        }
        let head = RowHead { key: Key::futures(key.market), scale, price_max: limit };
        let Some(row) = self.rows.get(r).filter(|_| r < SHARD_ROWS).copied() else { return no("it has four rows") };
        if row.head() != head {
            let free = row == ShardRow::EMPTY && !self.rows.iter().any(|x| x.key == head.key);
            if !free || !head.key.well_made() || scale <= 0 || scale > crate::state::SCALE_MAX || limit <= 0 || limit > crate::cross::MARGIN_MAX {
                return no("the row named is not its market's, nor an empty place for a market that has none and is one, of a scale and a limit within bounds");
            }
        }
        let next = books.listed(key, scale, limit, place, place_day)?;
        let mut s = *self;
        let more = if place_day.is_some() { 2 } else { 1 };
        s.rows[r] = ShardRow { entered: row.entered + more, ..ShardRow::of(head, row.net, row.gross) };
        s.root = next.root();
        Ok((s, next))
    }

    /// `KpfTRowOut.out` (docs 17.8j, 4): a dated row of no unit is taken out by the owner, by anyone once its day's
    /// leaf is written, or by anyone where the shard is called; a day's leaf by anyone, where no row of its day
    /// stands. The market's futures row counts one less, and is the empty row where it counts nothing and has no
    /// unit.
    pub fn row_out(&self, books: &crate::books::Books, key: &Key, by: RowOutBy) -> Result<(Shard, crate::books::Books)> {
        let no = |why: &str| Err(Error::Invalid(format!("no row is taken out of the shard's books: {why}")));
        if books.root() != self.root {
            return no("the books shown are not the shard's");
        }
        let Some(r) = self.rows.iter().take(SHARD_ROWS).position(|x| x.key == Key::futures(key.market) && !x.no_market()) else { return no("its market has no row among the four") };
        if key.is_dated() {
            let written = books.row(&Key::day_of(key.market, key.day)).and_then(|l| l.day_said()).is_some_and(|x| x != crate::books::DaySaid::NotWritten);
            let may = match by {
                RowOutBy::Owner => true,
                RowOutBy::DayWritten => written,
                RowOutBy::Called => self.called != 0,
            };
            if !may {
                return no("its day's leaf is not written, and the shard is not called: its owner takes it out");
            }
        }
        let next = books.unlisted(key)?;
        let mut s = *self;
        let row = &mut s.rows[r];
        row.entered -= 1;
        if row.entered < 0 {
            return no("the market's row counts no leaf");
        }
        if row.entered == 0 && row.net == 0 && row.gross == 0 {
            *row = ShardRow::EMPTY;
        }
        s.root = next.root();
        Ok((s, next))
    }
}

/// What a draw (kind 5) and a call (kind 11) say (docs 17.8b): an amount (a call's is nothing), the list of prices
/// in order, and a row of the trace a row of the books. `e` and `n` are the two counts as they are written (the
/// list's and the rows' lengths, where the statement is well made).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Traced {
    pub amount: i64,
    pub e: i64,
    pub list: Vec<Priced>,
    pub n: i64,
    pub rows: Vec<TraceRow>,
    /// A row a leaf of the tree of dated rows, in the tree's order, the head's leaf first (docs 17.8j, 6): none for
    /// a shard whose tree has the head's leaf alone.
    pub dated: Vec<DatedTrace>,
}

/// The byte that begins the dated rows of a trace, after the four rows: with it a statement that has them is of an
/// odd length beyond a trace's 184 bytes, and one that has none is of an even one.
pub const DATED_TAG: u8 = 0x44;
/// The most dated rows a trace says: the head's leaf and the sixty-three a tree has beside it.
pub const DATED_MAX: usize = 64;

/// A row of a trace that is of a leaf of the tree (a dated row, or a day's leaf), 80 bytes: the leaf's place, the
/// place of its market's price in the list, the three sums of a trace's row up to and with it, what the dated rows
/// are asked up to and with it, and the hash of the key the leaf names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DatedTrace {
    pub place: i64,
    pub at: i64,
    pub wn: i64,
    pub wg: i64,
    pub sn: i64,
    pub ro: i64,
    pub nk: [u8; 32],
}

impl DatedTrace {
    pub fn bytes(&self) -> Vec<u8> {
        [[self.place, self.at, self.wn, self.wg, self.sn, self.ro].into_iter().flat_map(sm8).collect::<Vec<u8>>(), self.nk.to_vec()].concat()
    }
}

/// The point of a draw's or a call's dispute that is the dated row `k` of its trace (from 1; one beyond the last is
/// the point that no leaf is left out at the end).
pub fn trace_dated_point(k: usize) -> i64 {
    100 + k as i64
}

impl Traced {
    /// The list's bytes: what a call's object holds.
    pub fn list_bytes(&self) -> Vec<u8> {
        self.list.iter().flat_map(|p| p.bytes()).collect()
    }

    /// 184 bytes and 56 a price.
    pub fn said(&self) -> Vec<u8> {
        let rows: Vec<u8> = self.rows.iter().flat_map(|r| [r.place, r.at, r.wn, r.wg, r.sn].into_iter().flat_map(sm8).collect::<Vec<u8>>()).collect();
        let dated: Vec<u8> = if self.dated.is_empty() { vec![] } else { [vec![DATED_TAG], sm8(self.dated.len() as i64).to_vec(), self.dated.iter().flat_map(|r| r.bytes()).collect()].concat() };
        [sm8(self.amount).to_vec(), sm8(self.e).to_vec(), self.list_bytes(), sm8(self.n).to_vec(), rows, dated].concat()
    }

    pub fn arg(&self) -> [u8; 32] {
        blake2b(&self.said())
    }

    /// What is said read back, where its length is a trace's (the counts are read as they are written).
    pub fn read(said: &[u8]) -> Option<Traced> {
        if said.len() < 184 {
            return None;
        }
        let n = |at: usize| num8(&said[at..at + 8]);
        // (With dated rows, of an odd length: the count of the list is read as it is written, and after the four
        // rows come a byte, a count and 80 bytes a row. Docs 17.8j, 6.)
        let has = (said.len() - 184) % 2 == 1;
        let e = if has { usize::try_from(n(8)).ok().filter(|e| *e <= SHARD_ROWS)? } else { (said.len() - 184) / 56 };
        let base = 184 + 56 * e;
        let mut dated = vec![];
        if has {
            if said.len() < base + 9 + 80 || said[base] != DATED_TAG || (said.len() - base - 9) % 80 != 0 {
                return None;
            }
            let nd = (said.len() - base - 9) / 80;
            if nd > DATED_MAX || n(base + 1) != nd as i64 {
                return None;
            }
            dated = (0..nd).map(|i| base + 9 + 80 * i).map(|a| DatedTrace { place: n(a), at: n(a + 8), wn: n(a + 16), wg: n(a + 24), sn: n(a + 32), ro: n(a + 40), nk: said[a + 48..a + 80].try_into().expect("32 bytes") }).collect();
        } else if (said.len() - 184) % 56 != 0 || e > SHARD_ROWS {
            return None;
        }
        let list = (0..e).map(|i| 16 + 56 * i).map(|a| Priced { market: said[a..a + 32].try_into().expect("32 bytes"), limit: n(a + 32), day: n(a + 40), price: n(a + 48) }).collect();
        let at = 16 + 56 * e;
        let rows = (0..SHARD_ROWS).map(|i| at + 8 + 40 * i).map(|a| TraceRow { place: n(a), at: n(a + 8), wn: n(a + 16), wg: n(a + 24), sn: n(a + 32) }).collect();
        // (Only what is written back the same: a number of nothing has two forms in eight bytes, and a script
        // that compares bytes takes one. Review T5, R1.)
        Some(Traced { amount: n(0), e: n(8), list, n: n(at), rows, dated }).filter(|t| t.said() == said)
    }
}

impl Shard {
    /// The price a row is counted at: the price said for a draw; for a call the price moved the tolerance in the
    /// shard's favour (up where its side is long in the row and no more than the limit, down otherwise).
    fn counted_at(&self, row: &ShardRow, p: i64, call: bool) -> Option<i64> {
        if !call {
            return Some(p);
        }
        let g = self.t.g_bps;
        let down = p.checked_mul(BPS - g)? / BPS;
        Some(if row.net > 0 { (p.checked_mul(BPS + g)?.checked_add(BPS - 1)? / BPS).min(row.price_max) } else { down })
    }

    /// The trace of a draw or a call at the prices `d` gives, one a row (docs 17.8b): the list of the rows' prices
    /// in order, and a row of the trace a row of the books. A row with no market, and a row whose price is not
    /// one (nothing, or beyond its limit), adds nothing: the second is answered wrong at its point. `None` where
    /// a worth or a sum is beyond 64 bits, and where two rows of one market are given two prices or have two
    /// limits (a market has one entry of the list): nobody states such a trace.
    pub fn traced(&self, d: &Draw, call: bool) -> Option<Traced> {
        let mut list: Vec<Priced> = vec![];
        // (A market's head of no unit is in no list and reads no price: docs 17.8j, 6.)
        for (r, p) in self.rows.iter().zip(&d.prices).filter(|(r, _)| !r.no_market() && (r.gross != 0 || r.net != 0)) {
            match list.iter().find(|x| x.market == r.key.market) {
                Some(x) if (x.limit, x.price) == (r.price_max, *p) => {}
                Some(_) => return None,
                None => list.push(Priced { market: r.key.market, limit: r.price_max, day: 0, price: *p }),
            }
        }
        list.sort_by_key(|p| p.numbers());
        let (mut wn, mut wg, mut sn) = (0i64, 0i64, 0i64);
        let mut rows = vec![];
        for (i, (row, p)) in self.rows.iter().zip(&d.prices).enumerate().take(SHARD_ROWS) {
            let mut at = 0;
            if row.key.market != [0; 32] && (row.gross != 0 || row.net != 0) {
                at = list.iter().position(|x| x.market == row.key.market && x.limit == row.price_max)? as i64;
                if *p > 0 && *p <= row.price_max && row.scale > 0 {
                    let q = self.counted_at(row, *p, call)?;
                    let w = worth(row.net.checked_abs()?, row.scale, q)?;
                    wn = wn.checked_add(w)?;
                    sn = if row.net < 0 { sn.checked_sub(w)? } else { sn.checked_add(w)? };
                    wg = wg.checked_add(worth(row.gross.checked_abs()?, row.scale, q)?)?;
                }
            }
            rows.push(TraceRow { place: i as i64, at, wn, wg, sn });
        }
        Some(Traced { amount: d.amount, e: list.len() as i64, list, n: SHARD_ROWS as i64, rows, dated: vec![] })
    }

    /// What a dated row of a day to come adds to a trace at its market's price `p` (docs 17.8j, 6): to the clearing
    /// side's worth, what its net is worth exercised; to the gross's worth, its gross at the price; and what it is
    /// asked alone by a move of the rate. Where the clearing side wrote: what its net is worth exercised at the
    /// worse end, less the same now. Where it holds: what it is worth now less the same at the worse end, and all
    /// it is worth now where the strike is between the ends. A call counts it at the price moved the tolerance in
    /// the shard's favour and at the lower rate. `None` beyond 64 bits.
    pub fn dated_adds(&self, row: &crate::books::Row, p: i64, call: bool) -> Option<(i64, i64, i64)> {
        let calls = row.key.of == crate::books::OF_CALLS;
        // (A call's price is moved up only where the row's worth to the shard rises with it: it holds calls, or
        // wrote puts. With no net unit, down, as a futures row. Review T17, V2.)
        let (g, holds) = (self.t.g_bps, row.net > 0);
        let q = if !call {
            p
        } else if (holds && calls) || (row.net < 0 && !calls) {
            (p.checked_mul(BPS + g)?.checked_add(BPS - 1)? / BPS).min(row.price_max)
        } else {
            p.checked_mul(BPS - g)? / BPS
        };
        let rate = if call { (self.t.mmr_bps - g).max(0) } else { self.t.mmr_bps + g };
        let (l, u) = crate::crow::ends(q, rate)?;
        let u = u.min(row.price_max);
        let k = row.key.strike;
        let ex = |x: i64| if calls { x.checked_sub(k).map(|v| v.max(0)) } else { k.checked_sub(x).map(|v| v.max(0)) };
        let units = row.net.checked_abs()?;
        let w = |x: i64| worth(units, row.scale, ex(x)?);
        let (now, low, high) = (w(q)?, w(l)?, w(u)?);
        let asked = if row.net < 0 {
            low.max(high).checked_sub(now)?.max(0)
        } else if row.net > 0 && l < k && k < u {
            now
        } else if row.net > 0 {
            now.checked_sub(low.min(high))?.max(0)
        } else {
            0
        };
        Some((if row.net < 0 { now.checked_neg()? } else { now }, worth(row.gross, row.scale, q)?, asked))
    }

    /// The trace of a draw or a call over the shard's four rows and the leaves of `books` (its tree: their root
    /// is its root), at the time `t0`: `traced`, and a row a leaf. A leaf of no unit (a day's leaf among them)
    /// carries the sums. `None` as `traced`, and where a dated row with a unit has no price or is of an ended day
    /// (a draw is wrong over one, and a call's row of one is answered by another script: 17.8j, 6 and D).
    pub fn traced_with(&self, books: &crate::books::Books, d: &Draw, call: bool, t0: i64) -> Option<Traced> {
        let leaves: Vec<crate::books::Row> = books.in_order().into_iter().skip(1).collect();
        // (The list has the market of every row that has a unit: a futures row's, and a dated row's by its market's
        // head among the four.)
        let mut list: Vec<Priced> = vec![];
        let price_of = |m: &[u8; 32]| self.rows.iter().zip(&d.prices).find(|(r, _)| r.key.market == *m && !r.no_market()).map(|(r, p)| (r.price_max, *p));
        for m in self.rows.iter().filter(|r| !r.no_market() && (r.gross != 0 || r.net != 0)).map(|r| r.key.market).chain(leaves.iter().filter(|r| r.gross != 0).map(|r| r.key.market)) {
            if !list.iter().any(|x| x.market == m) {
                let (limit, price) = price_of(&m)?;
                list.push(Priced { market: m, limit, day: 0, price });
            }
        }
        list.sort_by_key(|p| p.numbers());
        let (mut wn, mut wg, mut sn) = (0i64, 0i64, 0i64);
        let mut rows = vec![];
        for (i, (row, p)) in self.rows.iter().zip(&d.prices).enumerate().take(SHARD_ROWS) {
            let mut at = 0;
            if row.key.market != [0; 32] && (row.gross != 0 || row.net != 0) {
                at = list.iter().position(|x| x.market == row.key.market && x.limit == row.price_max)? as i64;
                if *p > 0 && *p <= row.price_max && row.scale > 0 {
                    let q = self.counted_at(row, *p, call)?;
                    let w = worth(row.net.checked_abs()?, row.scale, q)?;
                    wn = wn.checked_add(w)?;
                    sn = if row.net < 0 { sn.checked_sub(w)? } else { sn.checked_add(w)? };
                    wg = wg.checked_add(worth(row.gross.checked_abs()?, row.scale, q)?)?;
                }
            }
            rows.push(TraceRow { place: i as i64, at, wn, wg, sn });
        }
        let mut ro = 0i64;
        let mut dated = vec![];
        // (The head's leaf is the first row where the tree has any other: it adds nothing and names the first.)
        if !leaves.is_empty() {
            let head = books.row(&Key::HEAD)?;
            dated.push(DatedTrace { place: books.place_of(&Key::HEAD)? as i64, at: 0, wn, wg, sn, ro, nk: head.next });
        }
        for leaf in &leaves {
            let mut at = 0;
            if leaf.gross != 0 {
                at = list.iter().position(|x| x.market == leaf.key.market && x.limit == leaf.price_max)? as i64;
                let p = list[at as usize].price;
                if !leaf.key.is_dated() || leaf.key.day <= t0 || p <= 0 || p > leaf.price_max || leaf.scale <= 0 {
                    return None;
                }
                let (s, g, r) = self.dated_adds(leaf, p, call)?;
                sn = sn.checked_add(s)?;
                wg = wg.checked_add(g)?;
                ro = ro.checked_add(r)?;
            }
            dated.push(DatedTrace { place: books.place_of(&leaf.key)? as i64, at, wn, wg, sn, ro, nk: leaf.next });
        }
        Some(Traced { amount: d.amount, e: list.len() as i64, list, n: SHARD_ROWS as i64, rows, dated })
    }

    /// `KpfTShardRowD`: whether the dated row `k` (from 1) of a trace is right for these books, at the time `t0`.
    /// The first is of the head's leaf (the fourth row's sums, and the key the head names); a later one of the leaf
    /// whose key the row before carries. One beyond the last is right where no key is left; with no dated row
    /// said, where the head's leaf names none. `None` where the script
    /// computes no verdict: a sum beyond 64 bits; a hash carried that is no leaf's key; a call's row of an ended
    /// day with a unit (another script's: D).
    pub fn trace_dated_judged(&self, books: &crate::books::Books, t: &Traced, k: usize, call: bool, t0: i64) -> Option<bool> {
        if k == 0 || t.rows.len() != SHARD_ROWS {
            return Some(false);
        }
        // (A point beyond the one after the last row is of nothing: right. Review T17, V1.)
        if k > t.dated.len() + 1 {
            return Some(true);
        }
        let head = books.row(&Key::HEAD)?;
        let last4 = t.rows[SHARD_ROWS - 1];
        if t.dated.is_empty() {
            // (No dated row is said: right where the head's leaf, shown, names none.)
            return Some(head.next == Key::HEAD.hash());
        }
        if k == 1 {
            // (The first is the head's leaf: at its place, the fourth row's sums, nothing asked, the key it names.)
            let cur = t.dated[0];
            return Some(books.place_of(&Key::HEAD) == usize::try_from(cur.place).ok() && (cur.wn, cur.wg, cur.sn, cur.ro) == (last4.wn, last4.wg, last4.sn, 0) && cur.nk == head.next);
        }
        let prev = t.dated[k - 2];
        if k == t.dated.len() + 1 {
            return Some(prev.nk == Key::HEAD.hash());
        }
        if prev.nk == Key::HEAD.hash() {
            return Some(false);
        }
        let cur = t.dated[k - 1];
        let leaf = books.leaves.iter().flatten().find(|r| r.key != Key::HEAD && r.key.hash() == prev.nk).copied()?;
        let mut ok = books.place_of(&leaf.key) == usize::try_from(cur.place).ok() && cur.nk == leaf.next;
        if leaf.gross == 0 {
            return Some(ok && (cur.wn, cur.wg, cur.sn, cur.ro) == (prev.wn, prev.wg, prev.sn, prev.ro));
        }
        if cur.at < 0 || cur.at >= t.list.len() as i64 {
            return Some(false);
        }
        let en = t.list[cur.at as usize];
        let p = en.price;
        if en.market != leaf.key.market || en.limit != leaf.price_max || en.day != 0 || p <= 0 || p > leaf.price_max || leaf.scale <= 0 || !leaf.key.is_dated() {
            return Some(false);
        }
        if leaf.key.day <= t0 {
            // (A draw is wrong while a row of an ended day has a unit. A call's: by its day's leaf, another script.)
            return if call { None } else { Some(false) };
        }
        let (s, g, r) = self.dated_adds(&leaf, p, call)?;
        ok = ok && cur.wn == prev.wn && cur.wg == prev.wg.checked_add(g)? && cur.sn == prev.sn.checked_add(s)? && cur.ro == prev.ro.checked_add(r)?;
        Some(ok)
    }

    /// `KpfTShardRow`: whether the row `at` (1 to 4) of a trace is right for these books. `None` where the script
    /// computes no verdict (a worth or a sum beyond 64 bits).
    pub fn trace_row_judged(&self, t: &Traced, at: usize, call: bool) -> Option<bool> {
        let (Some(row), Some(cur)) = (self.rows.get(at.wrapping_sub(1)), t.rows.get(at.wrapping_sub(1))) else { return Some(false) };
        let prev = if at >= 2 { t.rows[at - 2] } else { TraceRow { place: 0, at: 0, wn: 0, wg: 0, sn: 0 } };
        let ok = cur.place == at as i64 - 1;
        // (A row with no market, and a market's head of no unit, add nothing and read no price: 17.8j, 6.)
        if row.key.market == [0; 32] || (row.gross == 0 && row.net == 0) {
            return Some(ok && (cur.wn, cur.wg, cur.sn) == (prev.wn, prev.wg, prev.sn));
        }
        if cur.at < 0 || cur.at >= t.list.len() as i64 {
            return Some(false);
        }
        let en = t.list[cur.at as usize];
        let (p, sc, pm) = (en.price, row.scale, row.price_max);
        if !ok || en.market != row.key.market || en.limit != pm || en.day != 0 || p <= 0 || p > pm || sc <= 0 {
            return Some(false);
        }
        let q = self.counted_at(row, p, call)?;
        let (w, v) = (worth(row.net.checked_abs()?, sc, q)?, worth(row.gross.checked_abs()?, sc, q)?);
        let sn = if row.net < 0 { prev.sn.checked_sub(w)? } else { prev.sn.checked_add(w)? };
        Some(cur.wn == prev.wn.checked_add(w)? && cur.wg == prev.wg.checked_add(v)? && cur.sn == sn)
    }

    /// `KpfTShardPrice`: whether the price `e` (from 1) of a trace's list is right: in its order after the one
    /// before it, of no day, and within the tolerance of the committee's `c`. One beyond the list is of nothing:
    /// right. (Where the order or the day is wrong, no price is cited.)
    pub fn trace_price_right(&self, t: &Traced, e: usize, c: i64) -> bool {
        let Some(en) = t.list.get(e.wrapping_sub(1)) else { return true };
        if (e >= 2 && !t.list[e - 2].below(en)) || en.day != 0 {
            return false;
        }
        let (a, g) = (en.price, self.t.g_bps);
        let p = c.min(en.limit);
        a > 0 && a >= p * (BPS - g) / BPS && a <= ((p * (BPS + g) + BPS - 1) / BPS).min(en.limit)
    }

    /// The last row's sums, and whether the counts written are the trace's.
    fn trace_sums(t: &Traced) -> (bool, i64, i64, i64) {
        let made = t.e == t.list.len() as i64 && t.n == SHARD_ROWS as i64 && t.rows.len() == SHARD_ROWS;
        match t.dated.last() {
            Some(r) => (made, r.wn, r.wg, r.sn),
            None => t.rows.last().map_or((false, 0, 0, 0), |r| (made, r.wn, r.wg, r.sn)),
        }
    }

    /// What the dated rows of a trace are asked, by its last row (nothing where it has none).
    fn trace_asked(t: &Traced) -> i64 {
        t.dated.last().map_or(0, |r| r.ro)
    }

    /// `KpfTShardEnd`, of a trace: whether the end of a draw is within the rule, by the last row's sums and no row
    /// read. `None` where the script computes no verdict.
    pub fn draw_trace_end_judged(&self, t: &Traced, claimer: &[u8; 32], t0: i64) -> Option<bool> {
        let (made, wn, wg, sn) = Shard::trace_sums(t);
        let ok = made && *claimer == self.owner && t.amount > 0;
        let need = need_of(wn, wg, &self.t, true)?.checked_add(Shard::trace_asked(t))?;
        let has = self.held.checked_sub(t.amount)?;
        let equity = has.checked_add(sn)?.checked_sub(self.basis)?;
        if has == i64::MIN || equity == i64::MIN || has.checked_add(sn)? == i64::MIN {
            return None;
        }
        self.draw_time(t0)?;
        Some(ok && has >= 0 && self.called == 0 && equity >= need && t0 >= self.expiry)
    }

    /// `KpfTCallEnd`, of a trace: whether a call is the rule's, by the last row's sums, and what the caller is
    /// paid. `None` where the script computes no verdict.
    pub fn call_trace_end_judged(&self, t: &Traced, t0: i64, held: i64) -> Option<(bool, i64)> {
        let (made, wn, wg, sn) = Shard::trace_sums(t);
        let ok = made && t.amount == 0 && t0 > 0;
        let need = need_of(wn, wg, &self.t, false)?.checked_add(Shard::trace_asked(t))?;
        let with = self.held.checked_add(sn)?;
        let equity = with.checked_sub(self.basis)?;
        if with == i64::MIN || equity == i64::MIN {
            return None;
        }
        let fee = self.t.keeper_fee_min.max(0);
        let pay = if self.held < fee { 0 } else { fee };
        let left = held.checked_sub(pay)?.checked_sub(CALL_HOLD)?;
        Some((ok && self.called == 0 && equity < need && left >= CALL_HOLD, pay))
    }
}

#[cfg(test)]
mod trace_tests {
    use super::*;

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

    fn terms() -> XTerms {
        XTerms { term: 0, mmr_bps: 500, g_bps: 50, liq_fee_bps: 50, keeper_fee_min: 10_000_000, carrier: 0, idle_daa: 0, grace_daa: 0, late_daa: 100, quiet_daa: 0 }
    }

    fn shard(r: &mut Rng) -> Shard {
        let mut s = Shard::opened(terms(), [7; 32], 0, 0, 50, 1_000, [9; 32]);
        for i in 0..SHARD_ROWS {
            if r.below(4) == 0 {
                continue;
            }
            // (Names whose numbers have a sign, and equal first numbers.)
            let mut market = [0u8; 32];
            market[0] = r.below(3) as u8;
            market[7] = if r.below(2) == 0 { 0x80 } else { 0 };
            market[20] = 1 + i as u8;
            let huge = r.below(40) == 0;
            let gross = if huge { i64::MAX / 3 } else { 1 + r.below(1_000_000_000_000) };
            let net = (r.below(2 * gross.min(1 << 40) + 1) - gross.min(1 << 40)).clamp(-gross, gross);
            s.rows[i] = ShardRow::futures(market, 100_000_000, 1_000_000_000_000, net, gross);
        }
        s.held = r.below(1_000_000_000_000);
        s.basis = r.below(2_000_000_000_000) - 1_000_000_000_000;
        s.expiry = r.below(3) * 5_000;
        if r.below(20) == 0 {
            s.called = 6_000;
        }
        s
    }

    /// 17.8b's claim: a trace of the three sums, judged a row at a time and at its end, gives the verdict the end
    /// gives today over the four rows. And a trace with any number moved is wrong at its own point.
    #[test]
    fn a_trace_judged_a_row_at_a_time_is_the_verdict_of_the_end_over_the_rows() {
        let mut r = Rng(0x17_08b);
        let (mut right, mut wrong, mut none, mut lies) = (0, 0, 0, 0);
        for _ in 0..40_000 {
            let s = shard(&mut r);
            let mut prices = Places::<i64>::default();
            for (p, row) in prices.iter_mut().zip(&s.rows) {
                *p = match r.below(12) {
                    0 => 0,
                    1 => row.price_max + 1,
                    _ => 1 + r.below(3_000_000_000),
                };
            }
            let claimer = if r.below(10) == 0 { [8; 32] } else { s.owner };
            let t0 = r.below(12_000);
            for call in [false, true] {
                let d = Draw { amount: if call { (r.below(10) == 0) as i64 } else { r.below(s.held.max(1) + 5) }, prices };
                let old = if call { s.call_end_judged(&d, t0, s.held + 300_000_000).map(|x| x.0) } else { s.draw_end_judged(&d, &claimer, t0) };
                let Some(t) = s.traced(&d, call) else {
                    // (Or judged wrong, where a row before the one that does not fit had no price: the loop over
                    // the rows stopped there. A trace's row is answered by itself.)
                    assert_ne!(old, Some(true), "what nobody can state, the end did not judge right");
                    none += 1;
                    continue;
                };
                assert_eq!(Traced::read(&t.said()).as_ref(), Some(&t));
                assert_eq!(t.said().len(), 184 + 56 * t.list.len());
                assert!(t.list.windows(2).all(|w| w[0].below(&w[1])), "the list in order, no market twice");
                let rows: Vec<Option<bool>> = (1..=SHARD_ROWS).map(|at| s.trace_row_judged(&t, at, call)).collect();
                let end = if call { s.call_trace_end_judged(&t, t0, s.held + 300_000_000).map(|x| x.0) } else { s.draw_trace_end_judged(&t, &claimer, t0) };
                let all = rows.iter().all(|x| *x == Some(true)) && end == Some(true);
                let any_wrong = rows.contains(&Some(false)) || end == Some(false);
                match old {
                    Some(true) => {
                        assert!(all, "right by the end over the rows: every row and the end right");
                        right += 1;
                    }
                    Some(false) => {
                        assert!(any_wrong, "wrong by the end over the rows: a row or the end wrong");
                        wrong += 1;
                    }
                    None => {
                        assert!(!all, "no verdict by the end over the rows: not all right by the trace");
                        none += 1;
                    }
                }
                assert_eq!(all, old == Some(true));
                if !all {
                    continue;
                }
                // Any number of a right trace moved is wrong at its own point (and the list's order at a price's).
                for i in 0..SHARD_ROWS {
                    for f in 0..5 {
                        let mut lie = t.clone();
                        let x = &mut lie.rows[i];
                        *[&mut x.place, &mut x.at, &mut x.wn, &mut x.wg, &mut x.sn][f] += 1;
                        let empty = s.rows[i].key.market == [0; 32];
                        // (An empty row's place of a price is read by nothing; a row's place of a price moved
                        // names another entry or none.)
                        let judged = s.trace_row_judged(&lie, i + 1, call);
                        if f == 1 && empty {
                            assert_eq!(judged, Some(true));
                        } else {
                            assert_eq!(judged, Some(false), "row {i}, number {f}");
                        }
                        lies += 1;
                    }
                }
                for (e, written) in [(t.e + 1, t.n), (t.e, t.n + 1), (t.e, t.n - 1)] {
                    let lie = Traced { e, n: written, ..t.clone() };
                    let end = if call { s.call_trace_end_judged(&lie, t0, s.held + 300_000_000).map(|x| x.0) } else { s.draw_trace_end_judged(&lie, &claimer, t0) };
                    assert_eq!(end, Some(false), "a count written that is not the trace's");
                }
                if t.list.len() >= 2 {
                    let mut lie = t.clone();
                    lie.list.swap(0, 1);
                    assert!(!s.trace_price_right(&lie, 2, lie.list[1].price), "the list out of its order");
                    let twice = Traced { list: vec![t.list[0], t.list[0]], ..t.clone() };
                    assert!(!s.trace_price_right(&twice, 2, t.list[0].price), "a market twice");
                }
                for (e, en) in t.list.iter().enumerate() {
                    assert!(s.trace_price_right(&t, e + 1, en.price));
                    assert!(!s.trace_price_right(&t, e + 1, en.price * 2 + 1_000));
                    let dated = Traced { list: t.list.iter().map(|x| Priced { day: 9, ..*x }).collect(), ..t.clone() };
                    assert!(!s.trace_price_right(&dated, e + 1, en.price), "a day, before the day is built");
                }
                assert!(s.trace_price_right(&t, t.list.len() + 1, 1), "a point beyond the list is of nothing");
            }
        }
        println!("{right} right, {wrong} wrong, {none} of no verdict; {lies} numbers moved, each wrong at its row");
        assert!(right > 2_000 && wrong > 2_000 && none > 100);
    }
}
