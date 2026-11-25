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
}

impl Shard {
    /// An account with this shard's terms and moving part and no leg: what the bytes are taken from.
    fn shell(&self) -> Account {
        Account { t: self.t, legs: vec![], a_key: self.owner, b_key: [0; 32], a_margin: self.held, b_margin: 0, expiry: self.expiry, dispute_daa: self.dispute_daa, bond_bps: self.bond_bps, a_min_counter: 0, b_min_counter: 0, shard: Some(self.sid) }
    }

    /// A shard as `KpfTShardOpen` writes it: no row taken.
    pub fn opened(t: XTerms, owner: [u8; 32], held: i64, expiry: i64, dispute_daa: i64, bond_bps: i64, sid: [u8; 32]) -> Shard {
        Shard { t, rows: [ShardRow::EMPTY; SHARD_ROWS], owner, held, expiry, dispute_daa, bond_bps, sid }
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
        if self.shard.is_none() || self.shard != to.shard || self.b_key != to.b_key {
            return Err(Error::Invalid("a leg moves between two covered accounts of one clearing party under one shard".into()));
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
        if from.a_margin < 0 || from.b_margin < 0 || next.a_margin < 0 || next.b_margin < 0 {
            return Err(Error::Invalid("a margin would fall below zero".into()));
        }
        Ok((from, next))
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

fn need_of(wn: i64, wg: i64, t: &crate::cross::XTerms) -> Option<i64> {
    let need = rate_up(wn, t.mmr_bps)?.checked_add(rate_up(wg, t.liq_fee_bps)?)?;
    need.checked_add(rate_up(need, t.g_bps)?)
}

impl Shard {
    /// The bond of a statement on the shard: its bond rate of what it holds (`KpfTChange`'s arithmetic).
    pub fn bond(&self) -> i64 {
        self.shell().bond()
    }

    /// What the shard is asked to hold at the prices `p` (one a row), with `KpfTShardEnd`'s arithmetic: the rate on
    /// the net of the worth of every row's net units, the rate on the gross of the worth of every row's gross
    /// units, and the tolerance of that more. `None` where a row that has a market has no price within its limit,
    /// or where the numbers are beyond what a script computes (`draw_end_judged`).
    pub fn requirement(&self, p: &[i64; SHARD_ROWS]) -> Option<i64> {
        let (mut wn, mut wg) = (0i64, 0i64);
        for (row, p) in self.rows.iter().zip(p) {
            if row.head.market == [0; 32] {
                continue;
            }
            if *p <= 0 || *p > row.head.price_max || row.head.scale <= 0 {
                return None;
            }
            wn = wn.checked_add(worth(row.net.checked_abs()?, row.head.scale, *p)?)?;
            wg = wg.checked_add(worth(row.gross.checked_abs()?, row.head.scale, *p)?)?;
        }
        need_of(wn, wg, &self.t)
    }

    /// `KpfTShardEnd`, step by step: whether the end of a draw is within the rule before its outputs are looked at
    /// (`Some(true)`), is not (`Some(false)`), or cannot be computed by the script (`None`: a sum or a product is
    /// beyond 64 bits, and the script fails whatever verdict is shown). Where it cannot be computed nobody answers
    /// a dispute of the end, the dispute lapses and the draw ends with both bonds returned: such a shard is drawn
    /// from only while nobody disputes. Its rows are that large only by legs its owner signed for.
    pub fn draw_end_judged(&self, d: &Draw, claimer: &[u8; 32]) -> Option<bool> {
        let mut ok = *claimer == self.owner && d.amount > 0;
        let (mut wn, mut wg) = (0i64, 0i64);
        for (row, p) in self.rows.iter().zip(&d.prices) {
            if row.head.market == [0; 32] {
                continue;
            }
            let (net, gross) = (row.net.checked_abs()?, row.gross.checked_abs()?);
            if *p <= 0 || *p > row.head.price_max || row.head.scale <= 0 {
                ok = false;
            }
            if ok {
                wn = wn.checked_add(worth(net, row.head.scale, *p)?)?;
                wg = wg.checked_add(worth(gross, row.head.scale, *p)?)?;
            }
        }
        let need = need_of(wn, wg, &self.t)?;
        Some(ok && self.held.checked_sub(d.amount)? >= need)
    }

    /// The shard once `amount` is drawn.
    pub fn drawn(&self, amount: i64) -> Shard {
        Shard { held: self.held - amount, ..*self }
    }

    /// `KpfTShardEnd`: whether the end of a draw stated by `claimer` is right for this shard, whose output held
    /// `held` when it was stated.
    pub fn draw_end_right(&self, d: &Draw, claimer: &[u8; 32], outs: &Outs, held: i64) -> bool {
        if self.draw_end_judged(d, claimer) != Some(true) {
            return false;
        }
        Outs::of(&[Out { key: false, to: self.drawn(d.amount).hash(), value: held - d.amount }, Out { key: true, to: *claimer, value: d.amount }]).is_some_and(|o| o == *outs)
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
    pub fn draw_wrong_point(&self, d: &Draw, claimer: &[u8; 32], outs: &Outs, held: i64, c: &[i64; SHARD_ROWS]) -> Option<i64> {
        if !self.draw_end_right(d, claimer, outs, held) {
            return Some(0);
        }
        (1..=SHARD_ROWS).find(|r| !self.draw_price_right(d, *r, c[*r - 1])).map(|r| r as i64)
    }

    /// The most the owner draws at the prices `p`.
    pub fn draw_max(&self, p: &[i64; SHARD_ROWS]) -> i64 {
        self.requirement(p).map(|need| (self.held - need).max(0)).unwrap_or(0)
    }

    /// The statement of a draw of `amount` at the prices `p` by the shard's owner, on a shard whose output holds
    /// `held`. Refused where the shard would hold less than its requirement at those prices.
    pub fn draw(&self, amount: i64, p: [i64; SHARD_ROWS], t0: i64, held: i64) -> Result<(Stated, Draw)> {
        let d = Draw { amount, prices: p };
        let outs = Outs::of(&[Out { key: false, to: self.drawn(amount).hash(), value: held - amount }, Out { key: true, to: self.owner, value: amount }]).ok_or_else(|| Error::Invalid("the draw's outputs".into()))?;
        if !self.draw_end_right(&d, &self.owner, &outs, held) {
            return Err(Error::Invalid(format!("the shard would hold less than its requirement at these prices (it holds {}, and is asked {:?}), or the amount is not above zero", self.held, self.requirement(&p))));
        }
        Ok((Stated { position: self.hash(), window: self.dispute_daa, late: self.t.late_daa, kind: KIND_DRAW, t0, arg: d.arg(), outs, bond: self.bond(), claimer: self.owner, held }, d))
    }

    /// The form a statement on the shard shows: its terms and the hash of its rows.
    pub fn form(&self) -> Vec<u8> {
        [self.terms(), blake2b(&self.rows_bytes()).to_vec()].concat()
    }
}

