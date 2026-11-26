//! The reserve (docs/ARCHITECTURE-CLEARING.md 6a): a clearing party's shard, the notes written to it by what happens
//! in the accounts it covers, and what applying a note does. The model of `KpfTShardOpen`, `KpfTLegAdd`,
//! `KpfTNoteApply` and `KpfTAcctEnd`, with the scripts' integer arithmetic.
//!
//! - A [`Shard`] has an account's form with a moving part of 171 bytes: its owner's key as A, no second key, what it
//!   holds as A's margin, and its own id at the end. Its fixed part is the hash of its terms and of four market rows.
//! - A covered account is an [`Account`] with `shard` set: the same 171 bytes, the id being the shard's.
//! - A [`SNote`] is an object of its own: a leg entered or left (the units of side B in its market, and the size), a
//!   deposit, or what a user is still owed.

use crate::committee::blake2b;
use crate::cross::{XLeg, XTerms};
use crate::error::{Error, Result};
use crate::state::{sm8, LONG};
use crate::state::BPS;
use crate::thin::{Account, Out, Outs, Stated, KIND_DRAW};

/// The market rows of a shard.
pub const SHARD_ROWS: usize = 4;
/// The least a note holds (`NOTE_MIN` of the scripts).
pub const NOTE_MIN: i64 = 20_000_000;
/// What a shard's output holds beside what the shard holds for its users (`SHARD_CARRIER` of `KpfTShardOpen`): a
/// shard that paid everything out is still an output a block takes.
pub const SHARD_CARRIER: i64 = 100_000_000;
/// A leg entered a covered account.
pub const NOTE_JOIN: i64 = 1;
/// A leg left one.
pub const NOTE_RELEASE: i64 = 2;
/// What the clearing side had: the shard holds it.
pub const NOTE_DEPOSIT: i64 = 3;
/// What an account's pot could not pay its user.
pub const NOTE_OWED: i64 = 4;

fn p33(h: &[u8; 32]) -> Vec<u8> {
    [&[0x20][..], h].concat()
}

fn i9(v: i64) -> Vec<u8> {
    [&[0x08][..], &sm8(v)].concat()
}

/// A market as a leg and a row name it: the first 84 bytes of either.
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

/// A row of a shard: a market, the clearing party's net units in it over the accounts the shard covers (below zero:
/// it is short), and the sum of the legs' sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShardRow {
    pub head: Head,
    pub net: i64,
    pub gross: i64,
}

impl ShardRow {
    pub const EMPTY: ShardRow = ShardRow { head: Head::EMPTY, net: 0, gross: 0 };

    pub fn bytes(&self) -> Vec<u8> {
        [self.head.bytes(), i9(self.net), i9(self.gross)].concat()
    }
}

/// A clearing party's reserve shard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shard {
    /// Its terms, an account's ten: the maintenance rate is the rate on the net, the liquidation fee's rate the rate
    /// on the gross, the tolerance the tolerance.
    pub t: XTerms,
    pub rows: [ShardRow; SHARD_ROWS],
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
}

impl Shard {
    /// An account with this shard's terms and moving part and no leg: what the bytes are taken from.
    fn shell(&self) -> Account {
        Account { t: self.t, legs: vec![], a_key: self.owner, b_key: [0; 32], a_margin: self.held, b_margin: 0, expiry: self.expiry, dispute_daa: self.dispute_daa, bond_bps: self.bond_bps, a_min_counter: self.basis, b_min_counter: self.called, shard: Some(self.sid) }
    }

    /// A shard as `KpfTShardOpen` writes it: no row taken.
    pub fn opened(t: XTerms, owner: [u8; 32], held: i64, expiry: i64, dispute_daa: i64, bond_bps: i64, sid: [u8; 32]) -> Shard {
        Shard { t, rows: [ShardRow::EMPTY; SHARD_ROWS], owner, held, expiry, dispute_daa, bond_bps, sid, basis: 0, called: 0 }
    }

    pub fn terms(&self) -> Vec<u8> {
        self.shell().terms()
    }

    /// The four rows, 408 bytes.
    pub fn rows_bytes(&self) -> Vec<u8> {
        self.rows.iter().flat_map(|r| r.bytes()).collect()
    }

    pub fn fixed_hash(&self) -> [u8; 32] {
        blake2b(&[self.terms(), p33(&blake2b(&self.rows_bytes()))].concat())
    }

    /// The moving part, 171 bytes.
    pub fn moving(&self) -> Vec<u8> {
        self.shell().moving()
    }

    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[p33(&self.fixed_hash()), self.moving()].concat())
    }

    /// The row a note of the market `head` is applied to: the market's own, or the first empty one.
    pub fn row_for(&self, head: &Head) -> Option<usize> {
        self.rows.iter().position(|r| r.head == *head).or_else(|| self.rows.iter().position(|r| *r == ShardRow::EMPTY))
    }

    /// `KpfTNoteApply`: the shard once the note is applied to its row `r` (a leg's note), what the note's key is
    /// paid (what is owed), and the note that goes on (the rest of what is owed).
    pub fn applied(&self, n: &SNote, r: usize) -> Result<(Shard, i64, Option<SNote>)> {
        let no = |why: &str| Err(Error::Invalid(format!("the note is not applied: {why}")));
        if n.sid != self.sid {
            return no("it is to another shard");
        }
        let mut s = *self;
        match n.kind {
            NOTE_JOIN | NOTE_RELEASE => {
                if n.key != self.owner {
                    return no("it is of an account whose side B is not this shard's owner");
                }
                let Some(row) = s.rows.get_mut(r) else { return no("the shard has four rows") };
                if row.head == Head::EMPTY {
                    if row.net != 0 || row.gross != 0 || n.head == Head::EMPTY {
                        return no("an empty row carries nothing, and a leg's market is one");
                    }
                    row.head = n.head;
                } else if row.head != n.head {
                    return no("the row is another market's");
                }
                let sign = if n.kind == NOTE_JOIN { 1 } else { -1 };
                row.net += sign * n.net;
                row.gross += sign * n.gross;
                // A row that carries no unit again is no market's: it is empty, for the next market that comes.
                if row.net == 0 && row.gross == 0 {
                    row.head = Head::EMPTY;
                }
                Ok((s, 0, None))
            }
            NOTE_DEPOSIT if n.amount > 0 => {
                s.held += n.amount;
                Ok((s, 0, None))
            }
            NOTE_OWED if n.amount > 0 => {
                if n.head.market != self.owner {
                    return no("it is owed by an account whose side B is not this shard's owner");
                }
                let pay = n.amount.min(s.held);
                if pay <= 0 {
                    return no("the shard holds nothing to pay it with");
                }
                s.held -= pay;
                Ok((s, pay, (pay < n.amount).then_some(SNote { amount: n.amount - pay, ..*n })))
            }
            _ => no("its kind is none, or its amount is not above zero"),
        }
    }
}

/// A note to a shard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SNote {
    pub sid: [u8; 32],
    pub kind: i64,
    pub head: Head,
    pub net: i64,
    pub gross: i64,
    pub key: [u8; 32],
    pub amount: i64,
}

impl SNote {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[i9(12), p33(&self.sid), i9(self.kind), self.head.bytes(), i9(self.net), i9(self.gross), p33(&self.key), i9(self.amount)].concat())
    }

    /// Side B's units in a leg's market: below zero where A is long. An option counts with its size whichever way
    /// it stands (whoever holds it, the clearing party may come to owe by it as far as its underlying moves).
    pub fn units(l: &XLeg) -> i64 {
        if l.dir == LONG && crate::thin::option_of(l).is_none() { -l.size } else { l.size }
    }

    /// The note of a leg that entered (`kind` 1) or left (2) an account of side B `b_key`, covered by `sid`.
    pub fn leg(kind: i64, sid: [u8; 32], l: &XLeg, b_key: [u8; 32]) -> SNote {
        SNote { sid, kind, head: Head::of(l), net: SNote::units(l), gross: l.size, key: b_key, amount: 0 }
    }

    /// What the clearing side `b_key` had in an account, to its shard.
    pub fn deposit(sid: [u8; 32], b_key: [u8; 32], amount: i64) -> SNote {
        SNote { sid, kind: NOTE_DEPOSIT, head: Head::EMPTY, net: 0, gross: 0, key: b_key, amount }
    }

    /// What an account's pot could not pay the user `a_key`. It names who owes it, the account's side B, where a
    /// leg's market would stand: a shard pays it only where that is its owner.
    pub fn owed(sid: [u8; 32], a_key: [u8; 32], b_key: [u8; 32], amount: i64) -> SNote {
        SNote { sid, kind: NOTE_OWED, head: Head { market: b_key, ..Head::EMPTY }, net: 0, gross: 0, key: a_key, amount }
    }

    /// The least the note's output holds: its minimum, and for a deposit the amount it carries.
    pub fn value(&self) -> i64 {
        NOTE_MIN + if self.kind == NOTE_DEPOSIT { self.amount } else { 0 }
    }
}

impl Account {
    /// `KpfTLegAdd`: the covered account once the leg is added, with what each party adds to its margin, and the
    /// note that the leg entered. Refused for an account no shard covers, or a leg that is none.
    pub fn leg_added(&self, l: XLeg, da: i64, db: i64) -> Result<(Account, SNote)> {
        let Some(sid) = self.shard else { return Err(Error::Invalid("a leg is added by its note to a covered account: this one names no shard".into())) };
        if !crate::thin::leg_size_ok(&l) || l.entry <= 0 || l.entry > l.price_max || !crate::thin::leg_kind_ok(&l) || da < 0 || db < 0 {
            return Err(Error::Invalid("the leg has no size or one beyond what is computed at its market's limit, no scale, an entry out of its market's limit, neither a direction nor an option's kind and strike, or a margin is taken with it".into()));
        }
        let mut next = self.clone();
        next.legs.push(l);
        if !crate::thin::legs_sum_ok(&next.legs) {
            return Err(Error::Invalid("with this leg the account's legs are worth more at their markets' limits than a trace sums: nobody could state on it".into()));
        }
        next.a_margin += da;
        next.b_margin += db;
        Ok((next, SNote::leg(NOTE_JOIN, sid, &l, self.b_key)))
    }

    /// `KpfTHand`: the covered account once side A is handed over to `nkey`, which adds `da` to the margin. The
    /// legs and their entries stay. Refused where the margin would be below what side B asked of whoever takes
    /// A's place.
    pub fn handed(&self, nkey: [u8; 32], da: i64) -> Result<Account> {
        if self.shard.is_none() || self.b_key == [0; 32] {
            return Err(Error::Invalid("a side is handed over in a covered account".into()));
        }
        if da < 0 || self.a_margin + da < self.b_min_counter {
            return Err(Error::Invalid(format!("who enters holds at least what who leaves held, and at least what side B asked ({} sompi)", self.b_min_counter)));
        }
        Ok(Account { a_key: nkey, a_margin: self.a_margin + da, ..self.clone() })
    }

    /// `KpfTLegMove`: this account without its leg `i` (from 1) and `to` with that leg appended, its entry as it
    /// was; the margins of the two move by `d` (this account's A and B, then the other's). Refused where the two
    /// are not of one clearing party under one shard, a margin would fall below zero, or more than six legs stand
    /// after the leg.
    pub fn leg_moved(&self, i: usize, to: &Account, d: [i64; 4]) -> Result<(Account, Account)> {
        if self.shard.is_none() || self.shard != to.shard || self.b_key != to.b_key || self.terms() != to.terms() || self.periods() != to.periods() {
            return Err(Error::Invalid("a leg moves between two covered accounts of one clearing party under one shard, of the same terms and periods".into()));
        }
        if i == 0 || i > self.legs.len() || self.legs.len() - i > 6 {
            return Err(Error::Invalid("the leg is one of the account's, with at most six after it".into()));
        }
        let mut from = self.clone();
        let leg = from.legs.remove(i - 1);
        from.a_margin += d[0];
        from.b_margin += d[1];
        let mut next = to.clone();
        next.legs.push(leg);
        if !crate::thin::legs_sum_ok(&next.legs) {
            return Err(Error::Invalid("with this leg the other account's legs are worth more at their markets' limits than a trace sums: nobody could state on it".into()));
        }
        next.a_margin += d[2];
        next.b_margin += d[3];
        if from.a_margin < 0 || from.b_margin < 0 || next.a_margin < 0 || next.b_margin < 0 || d[1].checked_add(d[3]).is_none_or(|v| v < 0) {
            return Err(Error::Invalid("a margin would fall below zero, or the clearing side's margin would leave its two accounts (it moves between them, and leaves by its shard alone)".into()));
        }
        Ok((from, next))
    }

    /// `KpfTLegOut`: this account without its leg `i` (from 1), by its two parties' signatures: `payb` moves from
    /// A's margin to B's, A takes `take` out (below zero: adds), B adds `addb`. And the note that the leg left.
    /// B's margin does not fall (what B owes A for the leg it pays from its own coins). Refused where the account
    /// names no shard, A's margin would fall below zero, B's would fall, or more than six legs stand after the leg.
    pub fn leg_out(&self, i: usize, payb: i64, take: i64, addb: i64) -> Result<(Account, SNote)> {
        let Some(sid) = self.shard else { return Err(Error::Invalid("a leg leaves by its note from a covered account: this one names no shard".into())) };
        if i == 0 || i > self.legs.len() || self.legs.len() - i > 6 {
            return Err(Error::Invalid("the leg is one of the account's, with at most six after it".into()));
        }
        let mut next = self.clone();
        let leg = next.legs.remove(i - 1);
        let (am, bm) = (self.a_margin.checked_sub(payb).and_then(|v| v.checked_sub(take)), self.b_margin.checked_add(payb).and_then(|v| v.checked_add(addb)));
        let (Some(am), Some(bm)) = (am, bm) else { return Err(Error::Invalid("the margins are beyond what is counted".into())) };
        if addb < 0 || payb < 0 || am < 0 || bm < 0 || am > crate::cross::MARGIN_MAX || bm > crate::cross::MARGIN_MAX {
            return Err(Error::Invalid("A's margin would fall below zero, or the clearing side's margin in the account would fall (what it has there leaves by its shard alone; what it owes for the leg it pays from its own coins)".into()));
        }
        next.a_margin = am;
        next.b_margin = bm;
        Ok((next, SNote::leg(NOTE_RELEASE, sid, &leg, self.b_key)))
    }

    /// `KpfTAcctEnd`: (to A's key, the deposit to the shard) of a covered account with no leg that ends.
    pub fn ended(&self) -> Result<(i64, Option<SNote>)> {
        let Some(sid) = self.shard else { return Err(Error::Invalid("the account names no shard".into())) };
        if !self.legs.is_empty() {
            return Err(Error::Invalid("the account has legs: each leaves by its own statement first".into()));
        }
        Ok((self.a_margin, (self.b_margin > 0).then(|| SNote::deposit(sid, self.b_key, self.b_margin))))
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
        NOTE_MIN + (-self.asked).max(0)
    }
}

/// What a note that legs left asked and the shard did not hold: paid as far as the shard holds, later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rest {
    pub sid: [u8; 32],
    pub key: [u8; 32],
    pub amount: i64,
}

impl Rest {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[i9(12), p33(&self.sid), i9(NOTE_REST), p33(&self.key), i9(self.amount)].concat())
    }

    pub fn value(&self) -> i64 {
        NOTE_MIN
    }
}

impl Shard {
    /// What a futures leg is to the books: the row of its market's futures legs, side B's units in it (the size
    /// below zero where A is long), and its worth at its entry signed as side B's (what it adds to what the legs
    /// were entered at).
    fn book_of(l: &XLeg) -> Result<(Head, i64, i64)> {
        if (l.dir != LONG && l.dir != crate::state::SHORT) || l.size <= 0 || l.scale <= 0 || l.entry <= 0 {
            return Err(Error::Invalid("the books are of futures legs: a direction, a size, a scale and an entry".into()));
        }
        let we = worth(l.size, l.scale, l.entry).ok_or_else(|| Error::Invalid("the leg's worth is beyond what is counted".into()))?;
        let head = Head { market: futures_market(&l.market), committee: l.committee, scale: l.scale, price_max: l.price_max };
        Ok(if l.dir == LONG { (head, -l.size, -we) } else { (head, l.size, we) })
    }

    /// The shard with the leg in its books: its row with the leg's units, and what the legs were entered at with
    /// the leg's entry (what the statement of an entry does, in the transaction that states it).
    pub fn booked(&self, l: &XLeg) -> Result<Shard> {
        let (head, units, basis) = Shard::book_of(l)?;
        let r = self.row_for(&head).ok_or_else(|| Error::Invalid("the shard has no row for the leg's market: its four are other markets'".into()))?;
        let mut s = *self;
        let row = &mut s.rows[r];
        row.head = head;
        row.net = row.net.checked_add(units).ok_or_else(|| Error::Invalid("the row is beyond what is counted".into()))?;
        row.gross = row.gross.checked_add(l.size).ok_or_else(|| Error::Invalid("the row is beyond what is counted".into()))?;
        s.basis = s.basis.checked_add(basis).ok_or_else(|| Error::Invalid("what the legs were entered at is beyond what is counted".into()))?;
        Ok(s)
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
        let Some(row) = self.rows.get(r) else { return no("it has four rows") };
        if row.head != head && (*row != ShardRow::EMPTY || self.rows.iter().any(|x| x.head.market == head.market)) {
            return no("the row is another market's, or the leg's market has a row of its own");
        }
        let mut s = *self;
        let max = crate::cross::MARGIN_MAX;
        let net = s.rows[r].net.checked_add(units).filter(|n| (-max..=max).contains(n));
        let gross = s.rows[r].gross.checked_add(l.size).filter(|g| *g <= max);
        let (Some(net), Some(gross), Some(b)) = (net, gross, s.basis.checked_add(basis).filter(|b| *b != i64::MIN)) else { return no("its books are beyond what is counted") };
        s.rows[r] = ShardRow { head, net, gross };
        s.basis = b;
        // (The shard's time: no draw before the entry is settled or ended. Review 51, F82.)
        let until = self.dispute_daa.checked_mul(2).and_then(|w| w.checked_add(self.t.late_daa)).and_then(|x| x.checked_add(t0));
        let Some(until) = until else { return no("its time is beyond what is counted") };
        s.expiry = s.expiry.max(until);
        Ok(s)
    }

    /// `KpfTLeft.apply` (a note of one leg) and `KpfTLeftAll.apply` (of two to six): the shard once the note that
    /// legs left is applied, whole: each leg out of its row and out of what the legs were entered at; what is
    /// asked paid as far as the shard holds, and the note of the rest; or, below zero, what the note holds added
    /// to what the shard holds. Also the row of each leg. Refused where a leg is in no row, or where the shard is
    /// called and something is asked.
    pub fn left(&self, n: &Left) -> Result<(Shard, Vec<usize>, i64, Option<Rest>)> {
        let no = |why: &str| Err(Error::Invalid(format!("the note that legs left is not applied: {why}")));
        if n.sid != self.sid {
            return no("it is to another shard");
        }
        if n.legs.is_empty() || n.legs.len() > 6 || n.asked == i64::MIN {
            return no("it is of one to six legs");
        }
        if self.called != 0 && n.asked > 0 {
            return no("the shard is called: what is asked waits, and is paid its share");
        }
        let beyond = || Error::Invalid("the books are beyond what is counted".into());
        if self.gone(n) {
            return no("a leg of it is in no row of the shard, or the row has fewer units than the note's legs of it: it is applied by nothing, and spent by `KpfTLeftVoid`");
        }
        let mut s = *self;
        let mut at = vec![];
        for l in &n.legs {
            let (head, units, basis) = Shard::book_of(l)?;
            let Some(r) = s.rows.iter().position(|row| row.head == head) else { return no("a leg is in no row of the shard") };
            let row = &mut s.rows[r];
            if row.gross < l.size {
                return no("a row has fewer units than the leg");
            }
            row.net = row.net.checked_sub(units).filter(|x| *x != i64::MIN).ok_or_else(beyond)?;
            row.gross -= l.size;
            if row.net == 0 && row.gross == 0 {
                row.head = Head::EMPTY;
            }
            s.basis = s.basis.checked_sub(basis).filter(|x| *x != i64::MIN).ok_or_else(beyond)?;
            at.push(r);
        }
        if n.asked < 0 {
            // (What the account paid for the legs, which the note holds.)
            s.held = s.held.checked_sub(n.asked).ok_or_else(beyond)?;
            return Ok((s, at, 0, None));
        }
        let pay = n.asked.min(s.held);
        s.held -= pay;
        Ok((s, at, pay, (pay < n.asked).then_some(Rest { sid: n.sid, key: n.key, amount: n.asked - pay })))
    }

    /// `KpfTLeftVoid`: the first leg of a note that is no leg of these books (no futures leg; no row of its
    /// market; or its market's row has fewer units than the note's legs of it): such a note is applied by nothing,
    /// and is spent, the shard as it is (review 51, F81). `None`: every leg is in the books.
    pub fn gone_at(&self, n: &Left) -> Option<usize> {
        n.legs.iter().position(|l| {
            let Ok((head, _, _)) = Shard::book_of(l) else { return true };
            let Some(row) = self.rows.iter().find(|row| row.head == head) else { return true };
            let units: i64 = n.legs.iter().filter(|x| crate::thin::leg_bytes(x)[..84] == crate::thin::leg_bytes(l)[..84] && x.size > 0).map(|x| x.size).fold(0i64, |a, b| a.saturating_add(b));
            row.gross < units
        })
    }

    pub fn gone(&self, n: &Left) -> bool {
        self.gone_at(n).is_some()
    }

    /// `KpfTLeft.rest`: a note of the rest is paid as far as the shard holds.
    pub fn rest_paid(&self, r: &Rest) -> Result<(Shard, i64, Option<Rest>)> {
        if r.sid != self.sid || r.amount <= 0 || self.called != 0 {
            return Err(Error::Invalid("the note of the rest is to this shard, of something, and paid while it is not called".into()));
        }
        let pay = r.amount.min(self.held);
        if pay <= 0 {
            return Err(Error::Invalid("the shard holds nothing to pay it with".into()));
        }
        Ok((Shard { held: self.held - pay, ..*self }, pay, (pay < r.amount).then_some(Rest { amount: r.amount - pay, ..*r })))
    }
}

/// What a draw states (kind 5): the amount, and a price a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Draw {
    pub amount: i64,
    pub prices: [i64; SHARD_ROWS],
}

impl Draw {
    /// What the statement shows: 40 bytes.
    pub fn said(&self) -> Vec<u8> {
        [sm8(self.amount).to_vec(), self.prices.iter().flat_map(|p| sm8(*p)).collect()].concat()
    }

    pub fn arg(&self) -> [u8; 32] {
        blake2b(&self.said())
    }

    pub fn read(said: &[u8]) -> Option<Draw> {
        if said.len() != 40 {
            return None;
        }
        let int = |i: usize| crate::state::int8(&said[8 * i..8 * i + 8]);
        Some(Draw { amount: int(0), prices: [int(1), int(2), int(3), int(4)] })
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
        self.shell().bond()
    }

    /// (The worth of the net units summed, of the gross, and what side B's net units are worth: above zero where it
    /// is long.) `None` where a row that has a market has no price within its limit, or a number is beyond 64 bits.
    fn worths(&self, p: &[i64; SHARD_ROWS]) -> Option<(i64, i64, i64)> {
        let (mut wn, mut wg, mut sn) = (0i64, 0i64, 0i64);
        for (row, p) in self.rows.iter().zip(p) {
            if row.head.market == [0; 32] {
                continue;
            }
            if *p <= 0 || *p > row.head.price_max || row.head.scale <= 0 {
                return None;
            }
            let w = worth(row.net.checked_abs()?, row.head.scale, *p)?;
            wn = wn.checked_add(w)?;
            sn = if row.net < 0 { sn.checked_sub(w)? } else { sn.checked_add(w)? };
            wg = wg.checked_add(worth(row.gross.checked_abs()?, row.head.scale, *p)?)?;
        }
        Some((wn, wg, sn))
    }

    /// What the shard is asked at the prices `p` (one a row), at its upper level, with `KpfTShardEnd`'s
    /// arithmetic: the maintenance rate of the worth of every row's net units and the rate on the gross of the
    /// worth of every row's gross units, each counted the tolerance more.
    pub fn requirement(&self, p: &[i64; SHARD_ROWS]) -> Option<i64> {
        self.level(p, true)
    }

    /// The requirement at its upper level (`up`) or its lower.
    pub fn level(&self, p: &[i64; SHARD_ROWS], up: bool) -> Option<i64> {
        let (wn, wg, _) = self.worths(p)?;
        need_of(wn, wg, &self.t, up)
    }

    /// The shard's equity at the prices `p`: what it holds, and what its net units are worth there, less what its
    /// legs were entered at.
    pub fn equity(&self, p: &[i64; SHARD_ROWS]) -> Option<i64> {
        let (_, _, sn) = self.worths(p)?;
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
            if row.head.market == [0; 32] {
                continue;
            }
            let (net, gross) = (row.net.checked_abs()?, row.gross.checked_abs()?);
            if *p <= 0 || *p > row.head.price_max || row.head.scale <= 0 {
                ok = false;
            }
            if ok {
                let w = worth(net, row.head.scale, *p)?;
                wn = wn.checked_add(w)?;
                sn = if row.net < 0 { sn.checked_sub(w)? } else { sn.checked_add(w)? };
                wg = wg.checked_add(worth(gross, row.head.scale, *p)?)?;
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
        if row.head.market == [0; 32] {
            return true;
        }
        let (a, g) = (d.prices[r - 1], self.t.g_bps);
        let p = c.min(row.head.price_max);
        a > 0 && a >= p * (BPS - g) / BPS && a <= ((p * (BPS + g) + BPS - 1) / BPS).min(row.head.price_max)
    }

    /// The first point of a stated draw that is wrong at the committee's prices `c` (one a row): 0 the end, r the
    /// price of row r. `None`: the draw is the rule's.
    pub fn draw_wrong_point(&self, d: &Draw, claimer: &[u8; 32], outs: &Outs, held: i64, t0: i64, c: &[i64; SHARD_ROWS]) -> Option<i64> {
        if !self.draw_end_right(d, claimer, outs, held, t0) {
            return Some(0);
        }
        (1..=SHARD_ROWS).find(|r| !self.draw_price_right(d, *r, c[*r - 1])).map(|r| r as i64)
    }

    /// The most the owner draws at the prices `p`.
    pub fn draw_max(&self, p: &[i64; SHARD_ROWS]) -> i64 {
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
    pub fn draw(&self, amount: i64, p: [i64; SHARD_ROWS], t0: i64, held: i64) -> Result<(Stated, Draw)> {
        let d = Draw { amount, prices: p };
        let outs = Outs::of(&[Out { key: false, to: self.drawn(amount, t0).hash(), value: held - amount }, Out { key: true, to: self.owner, value: amount }]).ok_or_else(|| Error::Invalid("the draw's outputs".into()))?;
        if !self.draw_end_right(&d, &self.owner, &outs, held, t0) {
            return Err(Error::Invalid(format!("the shard would hold less than its requirement at these prices (it holds {}, and is asked {:?}), or the amount is not above zero, or it is stated before the time the shard says ({})", self.held, self.requirement(&p), self.expiry)));
        }
        Ok((Stated { position: self.hash(), window: self.dispute_daa, late: self.t.late_daa, kind: KIND_DRAW, t0, arg: d.arg(), outs, bond: self.bond(), claimer: self.owner, held }, d))
    }

    /// The form a statement on the shard shows: its terms and the hash of its rows.
    pub fn form(&self) -> Vec<u8> {
        [self.terms(), blake2b(&self.rows_bytes()).to_vec()].concat()
    }
}

