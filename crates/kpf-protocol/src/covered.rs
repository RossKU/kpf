//! A covered account in the form of `docs/ARCHITECTURE-CLEARING.md` 13: its user's coins and its legs, and nothing
//! of the clearing side (the other side is its shard's books). The model of `KpfTCRow` and `KpfTCEnd`, with the
//! scripts' integer arithmetic.
//!
//! - A leg's entry stays. What the legs gained or lost counts for the user's standing and moves no coin until a leg
//!   leaves.
//! - Three kinds of statement: the user takes coins out (7), legs leave by the user's statement (8), a step of the
//!   user's liquidation (9). Each is stated with a trace, a row a leg, as an account's are; a row has a position's
//!   length (112 bytes) and other sums.
//! - A note's least of the user's coins is no coin of the account's in any test: it stays with the user whatever
//!   happens (review 47, F75).
//! - Two tests. **Standing** is of the net (the coins and what all the legs gained or lost, against the maintenance
//!   of what all are worth): it decides a step. **What leaves as coins** (a take; what a leg that gained is asked
//!   of the shard where it leaves alone) leaves the coins at the maintenance of the legs that stay and at what
//!   those have lost, each counted alone: a gain of a leg that stays is no coin.
//! - Six legs at most, counted in the moving part where a position has its first ask; and, where it has its
//!   second, the time before which the user states nothing again: a take and a leaving write their own time and
//!   twice the longest a statement lives, so that between two of them the account is free for a step.
//! - A leaving is of one leg or of all. Of all, the shard is asked the account's net, once, beyond the band. A leg
//!   that lost leaves alone where the coins pay what it lost.
//! - A leg enters by a statement of the user and of the shard's owner (kind 10), stated in a transaction that
//!   spends the shard: the books take the leg there. The statement stands on the account marked (a push more in
//!   its moving part); ended, the mark is undone and a note takes the leg out of the books again.
//! - What a leaving writes is one note to the shard ([`Left`]): the legs, the user's key, and what the shard is
//!   asked; below zero where the account pays the shard, and the note then holds those coins.

use crate::committee::blake2b;
use crate::cross::XLeg;
use crate::crow::{crow_after, CHead, CRow, Part, CHEAD_LEN, CROW_LEN};
use crate::error::{Error, Result};
use crate::reserve::{Left, NOTE_MIN};
use crate::state::{sm8, BPS, LOCK_TIME_THRESHOLD, LONG, SHORT};
use crate::thin::{leg_bytes, Account, Out, Outs, Row, Stated};

/// The user takes coins out of a covered account.
pub const KIND_C_TAKE: i64 = 7;
/// Legs leave a covered account by its user's statement: one, or all.
pub const KIND_C_LEAVE: i64 = 8;
/// A step of the liquidation of a covered account's user.
pub const KIND_C_STEP: i64 = 9;
/// One covered account made two: a leg leaves it for an account of its own (`docs/ARCHITECTURE-CLEARING.md` 16.2).
pub const KIND_C_SPLIT: i64 = 12;

/// A gain taken out of a leg that stays: the leg is from now entered at a price the user says, and what it gained
/// up to there is asked of the shard (`docs/ARCHITECTURE-CLEARING.md` 16.3).
pub const KIND_C_MARK: i64 = 13;

/// A token leg leaves a covered account by its user's statement (`docs/ARCHITECTURE-CLEARING.md` 16.12).
pub const KIND_C_TOK_OUT: i64 = 14;
/// A token leg is sold in a step of its user's liquidation: the claimer pays the account and has the ticket.
pub const KIND_C_TOK_SOLD: i64 = 15;
/// The note that a token leg left: the ticket its vault pays.
pub const NOTE_TICKET: i64 = 14;

/// Whether a kind is one of a covered account's own that `KpfTChange` states (7 to 9, 12 to 15).
pub fn c_kind(kind: i64) -> bool {
    (KIND_C_TAKE..=KIND_C_STEP).contains(&kind) || (KIND_C_SPLIT..=KIND_C_TOK_SOLD).contains(&kind)
}

/// The tag of a vault's state, and of a token's market's name.
pub const VAULT_TAG: i64 = 16;
pub const SPOT_TAG: i64 = 18;

/// The token a vault holds: its covenant id, the hash of its program's template, the lengths of the program
/// before and after its state, and its extension commitment (`docs/ARCHITECTURE-CLEARING.md` 16.12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenOf {
    pub id: [u8; 32],
    pub tpl: [u8; 32],
    pub pre: i64,
    pub suf: i64,
    pub ext: [u8; 32],
}

impl TokenOf {
    /// A token of the reference KCC-20 program (the one this crate has: `vendor/kob`).
    pub fn reference(id: [u8; 32], ext: [u8; 32]) -> TokenOf {
        let t = crate::artifacts::token_template();
        TokenOf { id, tpl: t.hash, pre: t.prefix.len() as i64, suf: t.suffix.len() as i64, ext }
    }

    /// The name of the market that prices the token: the hash of the token. No token is priced as another.
    pub fn market(&self) -> [u8; 32] {
        let i9 = |v: i64| [&[8u8][..], &sm8(v)].concat();
        blake2b(&[i9(SPOT_TAG), p33(&self.id), p33(&self.tpl), p33(&self.ext)].concat())
    }
}

/// A vault: an object under a covenant id of its own, which is the owner of the tokens held for one token leg of
/// a covered account. It says the id its account's objects are under, the token, and the amount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vault {
    pub rid: [u8; 32],
    pub token: TokenOf,
    pub amount: i64,
}

impl Vault {
    pub fn hash(&self) -> [u8; 32] {
        let i9 = |v: i64| [&[8u8][..], &sm8(v)].concat();
        let t = &self.token;
        blake2b(&[i9(12), i9(VAULT_TAG), p33(&self.rid), p33(&t.id), p33(&t.tpl), i9(t.pre), i9(t.suf), p33(&t.ext), i9(self.amount)].concat())
    }
}

/// A ticket: the note, under the account's id, that the token leg of the chain `chain` (the leg alone) left its
/// account, to the key `key`. The leg names its vault and the amount; the vault pays the ticket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub chain: [u8; 32],
    pub key: [u8; 32],
}

impl Ticket {
    pub fn hash(&self) -> [u8; 32] {
        let i9 = |v: i64| [&[8u8][..], &sm8(v)].concat();
        blake2b(&[i9(12), i9(NOTE_TICKET), p33(&self.chain), p33(&self.key)].concat())
    }

    pub fn value(&self) -> i64 {
        NOTE_MIN
    }
}
/// The direction of a token leg (`docs/ARCHITECTURE-CLEARING.md` 16.12): a token held for the account, counted as
/// coins at its worth less a discount. Where a futures leg has its committee it names its vault, and where a
/// futures leg has its entry it says its discount, in basis points.
pub const DIR_SPOT: i64 = 7;

/// In the place of the leg that leaves: every leg.
pub const ALL: i64 = -1;
/// The most legs a covered account has (what one transaction steps under a call, and one note takes out).
pub const C_LEGS_MAX: usize = 6;

fn p33(h: &[u8; 32]) -> Vec<u8> {
    [&[0x20][..], h].concat()
}

// The engine's integers are 64 bits and checked, and have no least value: these are `None` where a script fails.
fn ck(v: Option<i64>) -> Option<i64> {
    v.filter(|x| *x != i64::MIN)
}
pub(crate) fn add(a: i64, b: i64) -> Option<i64> {
    ck(a.checked_add(b))
}
pub(crate) fn sub(a: i64, b: i64) -> Option<i64> {
    ck(a.checked_sub(b))
}
pub(crate) fn worth(n: i64, sc: i64, p: i64) -> Option<i64> {
    if sc == 0 {
        return None;
    }
    add(add(ck((n / sc).checked_mul(p))?, ck((n % sc).checked_mul(p / sc))?)?, ck((n % sc).checked_mul(p % sc))? / sc)
}
pub(crate) fn rate_up(w: i64, r: i64) -> Option<i64> {
    add(ck((w / BPS).checked_mul(r))?, add(ck((w % BPS).checked_mul(r))?, BPS - 1)? / BPS)
}
pub(crate) fn rate_down(w: i64, r: i64) -> Option<i64> {
    add(ck((w / BPS).checked_mul(r))?, ck((w % BPS).checked_mul(r))? / BPS)
}

/// What a covered account's leg says where a leg of an account no shard covers has its committee (docs 17.2):
/// the time it entered, and an option's day, strike and premium a unit (nothing, for a futures leg). Its committee
/// is its shard's, shown apart wherever a price of it is answered. (A token leg names its vault there.)
pub fn leg_more(time: i64, day: i64, strike: i64, premium: i64) -> [u8; 32] {
    let mut b = [0u8; 32];
    for (i, x) in [time, day, strike, premium].iter().enumerate() {
        b[8 * i..8 * i + 8].copy_from_slice(&sm8(*x));
    }
    b
}

/// The time a covered account's leg entered, as a script reads the eight bytes.
pub fn leg_time(l: &XLeg) -> i64 {
    let mut b: [u8; 8] = l.committee[..8].try_into().expect("eight bytes");
    let below = b[7] & 0x80 != 0;
    b[7] &= 0x7f;
    let m = i64::from_le_bytes(b);
    if below { -m } else { m }
}

/// The leg as it stands in a covered account it entered at `t0`.
pub fn leg_entered(l: &XLeg, t0: i64) -> XLeg {
    XLeg { committee: leg_more(t0, 0, 0, 0), ..*l }
}

/// (The row before docs 17.8h, kept as what the row of `crow.rs` is held against in `tests/crow_model.rs`: its seven
/// numbers and two chains are in that row as they are here.)
/// A row of a covered account's trace. It has a position's row's bytes, and these sums over the legs up to it at
/// the prices stated: `gain`, what all gained or lost; `wa`, what all are worth; `wb`, what the legs that stay are
/// worth; `ea`, what the legs that stay have lost, each counted alone; `eb`, what the legs that leave gained or
/// lost. `g` is the chain of the legs, `h` the chain of the legs that stay (their entries as they are).
pub fn c_row_after(before: &Row, l: &XLeg, a: i64, leaves: bool) -> Option<Row> {
    let w = worth(l.size, l.scale, a)?;
    let d = sub(w, worth(l.size, l.scale, l.entry)?)?;
    let d = if l.dir == LONG { d } else { sub(0, d)? };
    let g = blake2b(&[&before.g[..], &leg_bytes(l)].concat());
    if l.dir == DIR_SPOT {
        // (A token leg stays: nothing for the legs' worth or for what they gained; what it counts is the sixth sum.)
        let counted = add(rate_down(w, sub(BPS, l.entry)?)?, 1)?;
        return Some(Row { a, tk: add(before.tk, counted)?, g, h: blake2b(&[&before.h[..], &leg_bytes(l)].concat()), ..*before });
    }
    let (gain, wa) = (add(before.gain, d)?, add(before.wa, w)?);
    Some(if leaves {
        Row { a, gain, wa, wb: before.wb, ea: before.ea, eb: add(before.eb, d)?, tk: before.tk, g, h: before.h }
    } else {
        Row { a, gain, wa, wb: add(before.wb, w)?, ea: add(before.ea, (-d).max(0))?, eb: before.eb, tk: before.tk, g, h: blake2b(&[&before.h[..], &leg_bytes(l)].concat()) }
    })
}

/// The row of the leg that a mark names (kind 13): it stays, entered from now at `ne`. Of a mark the sums are:
/// `gain`, what the marked leg has unrealized beyond `ne` at the price stated (no other leg adds to it); `wa`,
/// what all are worth; `wb`, what the others are worth; `ea`, what the others have lost, each counted alone; `eb`,
/// what the marked leg gained up to `ne`.
pub fn c_row_marked(before: &Row, l: &XLeg, a: i64, ne: i64) -> Option<Row> {
    let w = worth(l.size, l.scale, a)?;
    let wne = worth(l.size, l.scale, ne)?;
    let (r, kept) = (sub(wne, worth(l.size, l.scale, l.entry)?)?, sub(w, wne)?);
    let (r, kept) = if l.dir == LONG { (r, kept) } else { (sub(0, r)?, sub(0, kept)?) };
    let g = blake2b(&[&before.g[..], &leg_bytes(l)].concat());
    let at = XLeg { entry: ne, ..*l };
    Some(Row { a, gain: add(before.gain, kept)?, wa: add(before.wa, w)?, wb: before.wb, ea: before.ea, eb: add(before.eb, r)?, tk: before.tk, g, h: blake2b(&[&before.h[..], &leg_bytes(&at)].concat()) })
}

/// The chain of one leg that leaves alone (what its note names it by).
pub fn chain_of_one(l: &XLeg) -> [u8; 32] {
    Left::chain(&[*l])
}

/// What a statement on a covered account shows: the amount (a take's; a step's fee), the leg that leaves (0 none,
/// [`ALL`] every leg), the chain of the leg that leaves alone, the head its rows compute with (the rate, the
/// tolerance, T and the lines: docs 17.8h), and the trace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CSaid {
    pub amount: i64,
    pub skip: i64,
    pub lc: [u8; 32],
    pub head: CHead,
    pub rows: Vec<CRow>,
}

/// Where a statement's rows begin: after two numbers, a chain and the head.
pub const C_SAID_AT: usize = 48 + CHEAD_LEN;

fn crows_bytes(rows: &[CRow]) -> Vec<u8> {
    rows.iter().flat_map(|r| r.bytes()).collect()
}

impl CSaid {
    pub fn said(&self) -> Vec<u8> {
        [sm8(self.amount).to_vec(), sm8(self.skip).to_vec(), self.lc.to_vec(), self.head.bytes(), crows_bytes(&self.rows)].concat()
    }

    pub fn arg(&self) -> [u8; 32] {
        blake2b(&self.said())
    }

    pub fn read(said: &[u8]) -> Option<CSaid> {
        if said.len() < C_SAID_AT || (said.len() - C_SAID_AT) % CROW_LEN != 0 {
            return None;
        }
        let int = |b: &[u8]| crate::state::int8(b);
        let rows = said[C_SAID_AT..].chunks(CROW_LEN).map(CRow::read).collect::<Option<Vec<CRow>>>()?;
        Some(CSaid { amount: int(&said[..8]), skip: int(&said[8..16]), lc: said[16..48].try_into().expect("32 bytes"), head: CHead::read(&said[48..C_SAID_AT])?, rows })
    }
}

/// `KpfTCRow`: row `i` (from 1) is opened by a chain `prev` and a leg that hash to its chain (`None` where they do
/// not, or where a sum does not fit: the script then takes neither verdict). `leg_before` is the leg of the row
/// before (none for the first row), which the script is shown and holds to that row's chain: a leg of another
/// market closes the group, a leg of the same market says the same prices. The row is wrong where `prev` is not
/// the chain of the row before it, where the leg is no futures or token leg, where a price is out of its range,
/// where the leg leaves alone and is not the one the statement names by its chain; and else right exactly when it
/// is the row of `crow::crow_after`.
pub fn c_row_judged(prev: &[u8; 32], leg_before: Option<&XLeg>, l: &XLeg, before: &CRow, row: &CRow, i: i64, s: &CSaid) -> Option<bool> {
    c_row_judged_of(KIND_C_LEAVE, prev, leg_before, l, before, row, i, s)
}

/// As `c_row_judged`, of a statement of `kind`: of a mark (13) the leg named stays, at the entry the statement
/// says where a take has its amount, and the other legs add nothing to the first sum; of a step (9) a row says a
/// second price, within the lower rate of its first.
#[allow(clippy::too_many_arguments)]
pub fn c_row_judged_of(kind: i64, prev: &[u8; 32], leg_before: Option<&XLeg>, l: &XLeg, before: &CRow, row: &CRow, i: i64, s: &CSaid) -> Option<bool> {
    c_row_judged_with(kind, &s.head, s.amount, s.skip, &s.lc, prev, &two_before(&s.rows, i), leg_before, l, before, row, i)
}

/// The chain two rows before row `i` (from 1), as the rows say it: what the script opens the row before's chain
/// from, with the leg before.
fn two_before(rows: &[CRow], i: i64) -> [u8; 32] {
    usize::try_from(i - 3).ok().and_then(|k| rows.get(k)).map(|r| r.g).unwrap_or([0; 32])
}

#[allow(clippy::too_many_arguments)]
fn c_row_judged_with(kind: i64, head: &CHead, amount: i64, skip: i64, lc: &[u8; 32], prev: &[u8; 32], g2: &[u8; 32], leg_before: Option<&XLeg>, l: &XLeg, before: &CRow, row: &CRow, i: i64) -> Option<bool> {
    if row.g == [0; 32] {
        return Some(false);
    }
    if blake2b(&[&prev[..], &leg_bytes(l)].concat()) != row.g {
        return None;
    }
    if *prev != before.g {
        return Some(false);
    }
    // (The row before's chain is opened by the leg before, from the chain two rows before: or the script fails.)
    match leg_before {
        Some(b) if i > 1 => {
            if blake2b(&[&g2[..], &leg_bytes(b)].concat()) != before.g {
                return None;
            }
        }
        // (The script reads no leg before of the first row: review T13, W6.)
        _ if i <= 1 => {}
        _ => return None,
    }
    let leaves = skip == i || skip == ALL;
    // (A token leg is named by no statement but of the kinds of its own, 14 and 15; and those name a token leg, one.)
    let tok = kind == KIND_C_TOK_OUT || kind == KIND_C_TOK_SOLD;
    if (skip == i && chain_of_one(l) != *lc) || (l.dir == DIR_SPOT && leaves && !tok) || (tok && (skip == ALL || (leaves && l.dir != DIR_SPOT))) {
        // (The script computes nothing of a row it has found wrong: but a row whose price is out of range is
        // wrong by that as well, so the order does not matter here.)
        return Some(false);
    }
    let part = if !leaves { Part::Stays } else if kind == KIND_C_MARK { Part::Marked(amount) } else { Part::Leaves };
    Some(crow_after(kind, head, before, leg_before.filter(|_| i > 1), l, row.a, row.q, part)? == Some(*row))
}

/// What a statement on a covered account makes of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct COutcome {
    /// The account as it goes on; `None` where every leg left and it is ended.
    pub after: Option<Account>,
    /// What goes to the user's key (a take's amount; what an ended account held, less what it paid).
    pub paid: i64,
    /// The fee of a step, to whoever stated it.
    pub fee: i64,
    /// The note to the shard that legs left.
    pub note: Option<Left>,
    /// The account of the leg that left for an account of its own (a split).
    pub split: Option<Account>,
    /// The note to the shard that a leg was marked.
    pub mark: Option<crate::reserve::MarkNote>,
    /// The ticket of a token leg that left.
    pub ticket: Option<Ticket>,
    /// The note of what a sale paid the shard of what the account owed.
    pub owed: Option<crate::reserve::Owed>,
    pub outs: Outs,
}

impl Account {
    /// What this account's statements say for their rows to compute with: its terms' rate (the maintenance and the
    /// tolerance) and tolerance, and T, twice its lateness and its dispute period (docs 17.8h).
    pub fn c_head(&self) -> CHead {
        CHead { rate: self.t.mmr_bps + self.t.g_bps, tolerance: self.t.g_bps, t: 2 * (self.t.late_daa + self.dispute_daa), lines: 0 }
    }

    /// The trace of a statement of `kind` at the prices `a` and a step's prices `q`, one a leg, each leg doing what
    /// `part` says of it. `None` where a sum does not fit or no right trace is (two legs of one market next to
    /// one another are stated at one price).
    pub fn c_trace_of(&self, kind: i64, a: &[i64], q: &[i64], part: impl Fn(usize, &XLeg) -> Part) -> Option<Vec<CRow>> {
        if a.len() != self.legs.len() || q.len() != self.legs.len() {
            return None;
        }
        crate::crow::ctrace(kind, &self.c_head(), &self.legs, a, q, |i| part(i, &self.legs[i]))
    }

    /// The trace of a statement on this covered account at the prices `a`, one a leg; `skip`: the leg that leaves
    /// (0 none, [`ALL`] every leg). `None` where a sum does not fit or no right trace is.
    pub fn c_trace(&self, a: &[i64], skip: i64) -> Option<Vec<CRow>> {
        // (A token leg is named by no statement but of its own kinds: here it stays, its row is wrong where it is
        // named, and the end sees what it counts.)
        self.c_trace_of(KIND_C_LEAVE, a, a, |i, l| if (skip == ALL || skip == i as i64 + 1) && l.dir != DIR_SPOT { Part::Leaves } else { Part::Stays })
    }

    /// The trace of a step that names the leg `skip` (or [`ALL`]) at the prices `a`, due by the prices `q`.
    pub fn c_trace_step(&self, a: &[i64], q: &[i64], skip: i64) -> Option<Vec<CRow>> {
        self.c_trace_of(KIND_C_STEP, a, q, |i, l| if (skip == ALL || skip == i as i64 + 1) && l.dir != DIR_SPOT { Part::Leaves } else { Part::Stays })
    }

    /// The prices a step is due by if it is due by any: for each group (legs of one market next to one another),
    /// the end of the lower rate at which the group gains least.
    pub fn c_step_prices(&self, a: &[i64]) -> Option<Vec<i64>> {
        let lower = self.c_head().lower();
        let mut q = a.to_vec();
        let mut i = 0;
        while i < self.legs.len() {
            let mut j = i;
            while j < self.legs.len() && self.legs[j].market == self.legs[i].market {
                j += 1;
            }
            let p = *a.get(i)?;
            let d = rate_up(p, lower)?;
            let (lo, hi) = (sub(p, d)?.max(0), add(p, d)?);
            let (mut down, mut up) = (0i64, 0i64);
            for l in self.legs[i..j].iter().filter(|l| l.dir != DIR_SPOT) {
                let w = worth(l.size, l.scale, p)?;
                let s = |x: i64| if l.dir == LONG { Some(x) } else { sub(0, x) };
                down = add(down, s(sub(worth(l.size, l.scale, lo)?, w)?)?)?;
                up = add(up, s(sub(worth(l.size, l.scale, hi)?, w)?)?)?;
            }
            for x in q.iter_mut().take(j).skip(i) {
                *x = if down <= up { lo } else { hi };
            }
            i = j;
        }
        Some(q)
    }

    /// The trace of a mark of the leg `skip` (from 1) at the prices `a`, the leg from now entered at `ne`.
    pub fn c_trace_marked(&self, a: &[i64], skip: i64, ne: i64) -> Option<Vec<CRow>> {
        self.c_trace_of(KIND_C_MARK, a, a, |i, l| if (skip == ALL || skip == i as i64 + 1) && l.dir != DIR_SPOT { Part::Marked(ne) } else { Part::Stays })
    }

    /// The trace of a statement that names the token leg `skip` (from 1): tokens leave (14) or are sold (15).
    pub fn c_trace_tok(&self, kind: i64, a: &[i64], skip: i64) -> Option<Vec<CRow>> {
        // (These kinds name a token leg: there is no trace of them that names another.)
        if usize::try_from(skip - 1).ok().and_then(|k| self.legs.get(k)).is_some_and(|l| l.dir != DIR_SPOT) {
            return None;
        }
        self.c_trace_of(kind, a, a, |i, _| if skip == i as i64 + 1 { Part::Leaves } else { Part::Stays })
    }

    /// The entry nearest the price `a` that a mark of the leg `l` says: what the leg keeps unrealized there is
    /// the tolerance twice of its worth at `a`, at the least. `None` where there is none within the leg's limits
    /// (or the leg is too small for a price's step to count).
    pub fn mark_entry(&self, l: &XLeg, a: i64) -> Option<i64> {
        let g2 = add(self.t.g_bps, self.t.g_bps)?;
        let w = worth(l.size, l.scale, a)?;
        let band = rate_up(w, g2)?;
        let step = rate_up(a, g2)?;
        let mut ne = if l.dir == LONG { sub(a, step)? } else { add(a, step)? };
        for _ in 0..64 {
            if ne <= 0 || ne > l.price_max {
                return None;
            }
            let wne = worth(l.size, l.scale, ne)?;
            if (if l.dir == LONG { sub(w, wne)? } else { sub(wne, w)? }) >= band {
                return Some(ne);
            }
            ne = if l.dir == LONG { ne - 1 } else { ne + 1 };
        }
        None
    }

    /// The hash of this account with other coins of its user's and another chain of legs.
    pub fn c_hash(&self, coins: i64, chain: &[u8; 32]) -> [u8; 32] {
        self.c_next(coins, chain, self.a_min_counter, self.b_min_counter)
    }

    /// The same, with another count of legs and another time (where a position has its asks: `docs` 14.2).
    pub fn c_next(&self, coins: i64, chain: &[u8; 32], count: i64, time: i64) -> [u8; 32] {
        let next = Account { a_margin: coins, a_min_counter: count, b_min_counter: time, ..self.clone() };
        blake2b(&[p33(&blake2b(&[self.terms(), p33(chain)].concat())), next.moving()].concat())
    }

    /// As `c_next`, of an account that owes `owes`.
    pub fn c_next_owing(&self, coins: i64, chain: &[u8; 32], count: i64, time: i64, owes: i64) -> [u8; 32] {
        Account { b_margin: owes, ..self.clone() }.c_next(coins, chain, count, time)
    }

    /// `KpfTAddSpot`: this covered account with the token leg `leg` appended, done on an output that landed at
    /// `landed`: a leg more counted, and the time before which nothing is done on it at will. The account owes
    /// nothing and has room for a leg; the leg is a token leg of the token `token` (its market is the token's
    /// name), with an amount, and a discount of the liquidation fee's rate and the tolerance twice at the least.
    pub fn spot_added(&self, leg: &XLeg, token: &TokenOf, landed: i64) -> Result<Account> {
        self.c_check()?;
        self.owes_nothing()?;
        let no = |why: &str| Err(Error::Invalid(format!("no tokens enter this account: {why}")));
        if self.legs.len() >= C_LEGS_MAX {
            return no("it has six legs");
        }
        if leg.dir != DIR_SPOT || leg.size <= 0 || leg.scale <= 0 || leg.price_max <= 0 || worth(leg.size, leg.scale, leg.price_max).is_none() {
            return no("the leg is a token leg with an amount, a scale and a price limit its worth is counted at");
        }
        if leg.market != token.market() {
            return no("the leg's market is not the token's name");
        }
        let least = self.t.liq_fee_bps.checked_add(2 * self.t.g_bps).filter(|x| *x >= 0);
        if least.is_none_or(|x| leg.entry < x) || leg.entry > BPS || self.t.g_bps < 0 || self.t.liq_fee_bps < 0 {
            return no("the discount is the liquidation fee's rate and the tolerance twice at the least, the whole at the most");
        }
        let mut legs = self.legs.clone();
        legs.push(*leg);
        let next = Account { legs, a_min_counter: self.a_min_counter + 1, expiry: self.at_will(landed), ..self.clone() };
        next.c_check()?;
        Ok(next)
    }

    /// Whether this covered account owes its shard nothing: what every script but a step, a sale and a payment
    /// asks of it.
    pub fn owes_nothing(&self) -> Result<()> {
        if self.b_margin != 0 {
            return Err(Error::Invalid("the account owes its shard: until that is paid it is stepped, a token leg of it is sold, or it is paid for, and nothing else".into()));
        }
        Ok(())
    }

    /// `KpfTOwed.pay`: anyone pays `amount` of what this account owes its shard (all of it, or a KAS or more):
    /// the account that owes that much less, and the note that holds the coins.
    pub fn owed_paid(&self, amount: i64) -> Result<(Account, crate::reserve::Owed)> {
        let Some(sid) = self.shard else { return Err(Error::Invalid("an account no shard covers owes no shard".into())) };
        let owes = self.b_margin;
        if owes <= 0 || amount <= 0 || amount > owes || (amount != owes && amount < 100_000_000) {
            return Err(Error::Invalid("what is paid is of what the account owes: all of it, or a KAS or more".into()));
        }
        Ok((Account { b_margin: owes - amount, ..self.clone() }, crate::reserve::Owed { sid, amount }))
    }

    /// The bond of a statement of `kind` on this covered account that says `amount` where a take has its
    /// amount: a sale's (15) is no less than a hundredth of what it pays (review 63, F111).
    pub fn c_bond_for(&self, kind: i64, amount: i64) -> i64 {
        if kind == KIND_C_TOK_SOLD { self.c_bond().max(amount / 100) } else { self.c_bond() }
    }

    /// A covered account of this form: under a shard, of no more than six legs, each a leg, counted; nothing of
    /// a second party's; a bond.
    pub fn c_check(&self) -> Result<()> {
        // (Its futures legs as an account's are; a token leg has a size, a scale and a discount.)
        let (spot, futures): (Vec<XLeg>, Vec<XLeg>) = self.legs.iter().partition(|l| l.dir == DIR_SPOT);
        if !futures.is_empty() {
            Account { legs: futures, ..self.clone() }.check()?;
        }
        if spot.iter().any(|l| l.size <= 0 || l.scale <= 0 || !(1..=BPS).contains(&l.entry) || l.price_max <= 0 || worth(l.size, l.scale, l.price_max).is_none()) {
            return Err(Error::Invalid("a token leg has an amount, a scale, a price limit its worth is counted at, and a discount of 1 to 10000 basis points".into()));
        }
        // (Where a position has side B's margin a covered account says what it owes its shard: docs 16.18.)
        if self.shard.is_none() || self.legs.len() > C_LEGS_MAX || self.a_min_counter != self.legs.len() as i64 || self.b_margin < 0 {
            return Err(Error::Invalid("not a covered account of this form: under a shard, six legs at most and counted, nothing of a second party's, coins enough for a bond".into()));
        }
        Ok(())
    }

    /// `KpfTCEnd`: what the end of a statement's trace makes of this account holding `held`, by the last row's
    /// sums. `None`: the script takes neither verdict (a sum does not fit). `Some(None)`: the statement is not the
    /// rule's. Else what the account becomes and the outputs that say so.
    pub fn c_end(&self, kind: i64, s: &CSaid, claimer: &[u8; 32], held: i64, t0: i64) -> Option<Option<COutcome>> {
        if kind == KIND_C_SPLIT {
            return self.c_split_end(s, claimer, held, t0);
        }
        if kind == KIND_C_MARK {
            return self.c_mark_end(s, claimer, held, t0);
        }
        if kind == KIND_C_TOK_OUT || kind == KIND_C_TOK_SOLD {
            return self.c_tok_end(kind, s, claimer, held, t0);
        }
        let Some(sid) = self.shard else { return Some(None) };
        let n = s.rows.len() as i64;
        // (An account of no leg takes a take, stated with no row.)
        if n < 1 && kind != KIND_C_TAKE {
            return Some(None);
        }
        let zero = CRow::ZERO;
        let row = s.rows.last().unwrap_or(&zero);
        let (gain, w, ws, ls, gl, tk) = (row.gain, row.w, row.ws, row.ls, row.gl, row.tk);
        // (An account that owes takes a step, and nothing else of these kinds.)
        let owes = self.b_margin;
        if !(KIND_C_TAKE..=KIND_C_STEP).contains(&kind) || row.g != self.legs_root() || w < 0 || ws < 0 || ws > w || ls < 0 || tk < 0 || owes < 0 || (owes != 0 && kind != KIND_C_STEP) || !self.c_head_held(s) {
            return Some(None);
        }
        // What is asked: R with the last group closed, of all the legs and of those that stay (docs 17.4, 17.8h).
        let (ra, rs) = row.closed()?;
        // (A note's least is no coin of the account's: it stays with the user where every leg leaves.)
        let coins = self.a_margin;
        let m = sub(coins, NOTE_MIN)?;
        let g = self.t.g_bps;
        // The legs counted, and the time before which the user states nothing again; what a statement of the
        // user's writes there: its own time and twice the longest a statement lives.
        let (count, quiet) = (self.a_min_counter, self.b_min_counter);
        let life = add(self.t.late_daa, ck(self.dispute_daa.checked_mul(2))?)?;
        let next = add(t0, ck(life.checked_mul(2))?)?;
        let when = if kind == KIND_C_STEP { quiet } else { next };
        if kind != KIND_C_STEP && t0 < quiet {
            return Some(None);
        }
        let out = |v: Vec<(Out, bool)>, after: Option<Account>, paid: i64, fee: i64, note: Option<Left>| Outs::with_notes(&v).map(|outs| COutcome { after, paid, fee, note, split: None, mark: None, ticket: None, owed: None, outs });
        if kind == KIND_C_TAKE {
            if s.skip != 0 || *claimer != self.a_key || s.amount <= 0 {
                return Some(None);
            }
            // (No more than the coins: what a token leg counts leaves no coin that is not there.)
            let short = sub(m, s.amount)? < 0;
            if add(sub(m, s.amount)?, tk)? < add(ra, ls)? || short {
                return Some(None);
            }
            let left = sub(coins, s.amount)?;
            let after = Account { a_margin: left, b_min_counter: when, ..self.clone() };
            let v = vec![(Out { key: false, to: self.c_next(left, &self.legs_root(), count, when), value: sub(held, s.amount)? }, false), (Out { key: true, to: *claimer, value: s.amount }, false)];
            return Some(out(v, Some(after), s.amount, 0, None));
        }
        let mut all = s.skip == ALL;
        if s.skip == 0 || s.skip < ALL || s.skip > n || (kind == KIND_C_LEAVE && *claimer != self.a_key) || (kind == KIND_C_STEP && s.skip < 1) {
            return Some(None);
        }
        let wl = sub(w, ws)?;
        let mut fee = 0;
        let mut owing = false;
        if kind == KIND_C_LEAVE {
            if !all {
                // (A leg that lost: the coins pay it, and what stays is at the level of an entry: N65.)
                let stands = if gl >= 0 { add(m, tk)? >= add(rs, ls)? } else { add(m, gl)? >= 0 && add(add(m, gain)?, tk)? >= rs };
                if !stands {
                    return Some(None);
                }
            }
        } else {
            // Due by the prices the rows say: what a step would leave the account is below nothing. What the legs
            // gained to those prices counts less the band where it is above nothing, and no less than nothing
            // (docs 17.8g, D).
            let mut sx = add(gain, row.m)?;
            if sx >= 0 {
                sx = sub(sx, rate_up(w, add(g, g)?)?)?.max(0);
            }
            let e = sub(add(m, tk)?, owes)?;
            if add(e, sx)? >= 0 {
                return Some(None);
            }
            if !all {
                if gl < 0 {
                    // The leg named lost: it leaves alone where the account lacks less after it than before
                    // (docs 17.4), its fee no more than the coins hold beyond what it lost and what the legs
                    // that stay lost (review 47, F73).
                    fee = rate_down(wl, self.t.liq_fee_bps)?.min(sub(add(m, gl)?, ls)?.max(0));
                    // (What it lacks after is counted before the fee: review T13, W2.)
                    let before = sub(sub(ra, e)?, gain)?.max(0);
                    let after = sub(rs, add(e, gain)?)?.max(0);
                    if tk != 0 {
                        // (An account that holds tokens is not taken whole: the leg named leaves alone, and the
                        // account owes what the coins do not pay. Review T13, W1.)
                        owing = add(m, gl)? < 0;
                    } else if after >= before || add(m, gl)? < 0 {
                        all = true;
                    }
                } else {
                    if ls != 0 {
                        return Some(None);
                    }
                    if tk != 0 {
                        fee = rate_down(wl, self.t.liq_fee_bps)?;
                    } else {
                        all = true;
                    }
                }
            }
            if all {
                fee = rate_down(w, self.t.liq_fee_bps)?;
            }
        }
        // (Not every leg while the account holds a token leg: its tokens leave, or are sold, first.)
        if all && tk != 0 {
            return Some(None);
        }
        let g2 = add(g, g)?;
        let (mut pay, mut asked);
        let mut setoff = 0;
        let v;
        let outcome;
        if all {
            let room = m.max(0);
            pay = 0;
            if gain < 0 {
                pay = sub(0, gain)?.min(room);
                asked = sub(0, pay)?;
            } else {
                asked = sub(gain, rate_up(w, g2)?)?.max(0);
                // (What the account owes its shard is set against what its legs ask of it: review 64, F112.)
                asked = sub(asked, asked.min(owes))?;
            }
            fee = fee.min(sub(room, pay)?);
            let paid = sub(sub(held, pay)?, fee)?;
            let note = Left { sid, legs: self.legs.clone(), key: self.a_key, asked };
            let mut o = vec![(Out { key: true, to: self.a_key, value: paid }, false)];
            if fee > 0 {
                o.push((Out { key: true, to: *claimer, value: fee }, false));
            }
            o.push((Out { key: false, to: note.hash_of(&self.legs_root()), value: add(NOTE_MIN, pay)? }, true));
            v = o;
            outcome = (None, paid, Some(note));
        } else {
            pay = 0;
            if gl < 0 {
                pay = sub(0, gl)?;
                asked = gl;
            } else {
                asked = sub(gl, rate_up(wl, g2)?)?.max(0);
                // (And the account owes that much less.)
                setoff = asked.min(owes);
                asked = sub(asked, setoff)?;
            }
            // An account that holds tokens and whose coins do not pay the leg: the coins pay what they can of what
            // it lost, then of the fee, and the account owes the rest. It writes the account's time.
            let room1 = m.max(0);
            let (mut more, mut when) = (sub(0, setoff)?, when);
            if owing {
                more = pay;
                pay = pay.min(room1);
                more = sub(more, pay)?;
                asked = sub(0, pay)?;
                when = when.max(add(add(t0, self.t.late_daa)?, self.dispute_daa)?);
            }
            fee = fee.min(sub(room1, pay)?);
            let owes = add(owes, more)?;
            let left = sub(sub(coins, pay)?, fee)?;
            let k = s.skip as usize - 1;
            let leg = self.legs.get(k).copied();
            let legs: Vec<XLeg> = self.legs.iter().enumerate().filter(|(i, _)| *i != k).map(|(_, l)| *l).collect();
            let after = Account { legs, a_margin: left, b_margin: owes, a_min_counter: sub(count, 1)?, b_min_counter: when, ..self.clone() };
            // (The note names the leg by the chain the statement says: its row holds that to the leg.)
            let note = leg.map(|l| Left { sid, legs: vec![l], key: self.a_key, asked });
            let mut o = vec![(Out { key: false, to: self.c_next_owing(left, &row.h, sub(count, 1)?, when, owes), value: sub(sub(held, pay)?, fee)? }, false)];
            if fee > 0 {
                o.push((Out { key: true, to: *claimer, value: fee }, false));
            }
            o.push((Out { key: false, to: Left { sid, legs: vec![], key: self.a_key, asked }.hash_of(&s.lc), value: add(NOTE_MIN, pay)? }, true));
            v = o;
            outcome = (Some(after), 0, note);
        }
        if s.amount != fee {
            return Some(None);
        }
        Some(out(v, outcome.0, outcome.1, fee, outcome.2))
    }

    /// Whether the head of a statement is this account's (the lines are not held: nothing reads them yet).
    fn c_head_held(&self, s: &CSaid) -> bool {
        let mine = self.c_head();
        (s.head.rate, s.head.tolerance, s.head.t) == (mine.rate, mine.tolerance, mine.t)
    }

    /// `KpfTCSplit`: what the end of a split makes of this account holding `held`: leg `s.skip` leaves it for an
    /// account of its own with `s.amount` of the coins. Each of the two stands by itself at the prices stated,
    /// every leg counted as badly as it can be: the one that stays by the test of a take with what stays; the new
    /// one by its own leg. No note: the shard's books are as they were. `None`, `Some(None)`: as `c_end`.
    pub fn c_split_end(&self, s: &CSaid, claimer: &[u8; 32], held: i64, t0: i64) -> Option<Option<COutcome>> {
        if self.shard.is_none() {
            return Some(None);
        }
        let n = s.rows.len() as i64;
        let zero = CRow::ZERO;
        // (A leg of two or more: the last leg of an account does not leave it for another.)
        let mut ok = n >= 2;
        let row = if ok { s.rows.last().unwrap_or(&zero) } else { &zero };
        let (w, ws, ls, gl) = (row.w, row.ws, row.ls, row.gl);
        ok = ok && row.g == self.legs_root() && w >= 0 && ws >= 0 && ws <= w && ls >= 0 && row.tk >= 0 && self.b_margin == 0 && self.c_head_held(s);
        let (_, rs) = row.closed()?;
        let coins = self.a_margin;
        let (count, quiet) = (self.a_min_counter, self.b_min_counter);
        let when = add(t0, ck(add(self.t.late_daa, ck(self.dispute_daa.checked_mul(2))?)?.checked_mul(2))?)?;
        ok = ok && *claimer == self.a_key && t0 >= quiet && s.skip >= 1 && s.skip <= n && s.amount > 0 && s.amount <= coins;
        if !ok {
            return Some(None);
        }
        // (No more than the coins: what a token leg counts leaves no coin that is not there.)
        let short = sub(sub(coins, s.amount)?, NOTE_MIN)? < 0;
        if add(sub(sub(coins, s.amount)?, NOTE_MIN)?, row.tk)? < add(rs, ls)? || short {
            return Some(None);
        }
        // (The leg that moves, alone: what it can lose at either end of a move of the rate.)
        let lost = if gl < 0 { sub(0, gl)? } else { 0 };
        if sub(s.amount, NOTE_MIN)? < add(row.rl, lost)? {
            return Some(None);
        }
        let k = s.skip as usize - 1;
        let Some(leg) = self.legs.get(k).copied() else { return Some(None) };
        let left = sub(coins, s.amount)?;
        let legs: Vec<XLeg> = self.legs.iter().enumerate().filter(|(i, _)| *i != k).map(|(_, l)| *l).collect();
        let stays = Account { legs, a_margin: left, a_min_counter: sub(count, 1)?, b_min_counter: when, ..self.clone() };
        let new = Account { legs: vec![leg], a_margin: s.amount, a_min_counter: 1, b_min_counter: when, ..self.clone() };
        // (The two are named by the chains the statement says: its rows hold those to the legs.)
        let v = vec![
            (Out { key: false, to: self.c_next(left, &row.h, sub(count, 1)?, when), value: sub(held, s.amount)? }, false),
            (Out { key: false, to: self.c_next(s.amount, &s.lc, 1, when), value: s.amount }, true),
        ];
        Some(Outs::with_notes(&v).map(|outs| COutcome { after: Some(stays), paid: 0, fee: 0, note: None, split: Some(new), mark: None, ticket: None, owed: None, outs }))
    }

    /// `KpfTCMark`: what the end of a mark makes of this account holding `held`: leg `s.skip` is from now entered
    /// at `s.amount`, and what it gained up to there is asked of the shard. The user's, at or after the account's
    /// time, which it writes; what the leg has unrealized beyond its new entry is the tolerance twice of its worth
    /// at the least (the band a leaving takes: taken once over the mark and the leaving); and the account stands
    /// as after a take. `None`, `Some(None)`: as `c_end`.
    pub fn c_mark_end(&self, s: &CSaid, claimer: &[u8; 32], held: i64, t0: i64) -> Option<Option<COutcome>> {
        let Some(sid) = self.shard else { return Some(None) };
        let n = s.rows.len() as i64;
        let zero = CRow::ZERO;
        let row = s.rows.last().unwrap_or(&zero);
        let (kept, w, ws, ls, gl) = (row.gain, row.w, row.ws, row.ls, row.gl);
        let mut ok = n >= 1 && row.g == self.legs_root() && w >= 0 && ws >= 0 && ws <= w && ls >= 0 && row.tk >= 0 && self.b_margin == 0 && self.c_head_held(s);
        let (ra, _) = row.closed()?;
        let coins = self.a_margin;
        let g = self.t.g_bps;
        let quiet = self.b_min_counter;
        let when = add(t0, ck(add(self.t.late_daa, ck(self.dispute_daa.checked_mul(2))?)?.checked_mul(2))?)?;
        ok = ok && *claimer == self.a_key && t0 >= quiet && s.amount > 0 && s.skip >= 1 && s.skip <= n && gl > 0;
        if !ok {
            return Some(None);
        }
        let thin = kept < rate_up(sub(w, ws)?, add(g, g)?)?;
        if add(sub(coins, NOTE_MIN)?, row.tk)? < add(ra, ls)? || thin {
            return Some(None);
        }
        let k = s.skip as usize - 1;
        let Some(leg) = self.legs.get(k).copied() else { return Some(None) };
        let mut legs = self.legs.clone();
        legs[k].entry = s.amount;
        let after = Account { legs, b_min_counter: when, ..self.clone() };
        let note = crate::reserve::MarkNote { sid, leg, entry: s.amount, key: self.a_key, asked: gl };
        // (The account and the note are named by the chains the statement says: its rows hold those to the legs.)
        let v = vec![(Out { key: false, to: self.c_next(coins, &row.h, self.a_min_counter, when), value: held }, false), (Out { key: false, to: note.hash_of(&s.lc), value: NOTE_MIN }, true)];
        Some(Outs::with_notes(&v).map(|outs| COutcome { after: Some(after), paid: 0, fee: 0, note: None, split: None, mark: Some(note), ticket: None, owed: None, outs }))
    }

    /// `KpfTCTok`: what the end of a statement that a token leg leaves (14) or is sold in a step (15) makes of
    /// this account holding `held`. 14: the user's, at or after the account's time, which it writes; the account
    /// passes the test of a take without the leg. 15: anyone's, where a step is due and no step on a futures leg
    /// is; the claimer pays `s.amount`, at least the leg's worth less the liquidation fee's rate, and the coins
    /// grow by it. Either writes the account without the leg, and a ticket. `None`, `Some(None)`: as `c_end`.
    pub fn c_tok_end(&self, kind: i64, s: &CSaid, claimer: &[u8; 32], held: i64, t0: i64) -> Option<Option<COutcome>> {
        if self.shard.is_none() {
            return Some(None);
        }
        let n = s.rows.len() as i64;
        let zero = CRow::ZERO;
        let row = s.rows.last().unwrap_or(&zero);
        let (w, ls, tw, tk) = (row.w, row.ls, row.gl, row.tk);
        let (_, rs) = row.closed()?;
        // (The end is shown the leg the statement names, which hashes to the chain said: with none it takes
        // neither verdict. And it is a token leg. Review 62, F109.)
        let named = self.legs.iter().find(|l| chain_of_one(l) == s.lc)?;
        let owes = self.b_margin;
        let mut ok = n >= 1 && row.g == self.legs_root() && w >= 0 && ls >= 0 && tw >= 0 && tk >= 0 && owes >= 0 && named.dir == DIR_SPOT && self.c_head_held(s);
        let coins = self.a_margin;
        let m = sub(coins, NOTE_MIN)?;
        let liq = self.t.liq_fee_bps;
        let (count, quiet) = (self.a_min_counter, self.b_min_counter);
        let when = add(t0, ck(add(self.t.late_daa, ck(self.dispute_daa.checked_mul(2))?)?.checked_mul(2))?)?;
        ok = ok && s.skip >= 1 && s.skip <= n;
        let (mut after, mut time, mut key, mut to_shard) = (coins, quiet, self.a_key, 0);
        if kind == KIND_C_TOK_OUT {
            ok = ok && *claimer == self.a_key && t0 >= quiet && s.amount == 0 && owes == 0;
            time = when;
            if ok && add(m, tk)? < add(rs, ls)? {
                ok = false;
            }
        } else {
            // A sale: of an account that owes, at or after its time. The shard is paid first, as far as the account
            // owes; the rest is the account's coins.
            key = *claimer;
            ok = ok && owes > 0 && t0 >= quiet && s.amount > 0;
            if ok {
                let enough = s.amount >= sub(tw, rate_down(tw, liq)?)?;
                to_shard = s.amount.min(owes);
                after = sub(add(coins, s.amount)?, to_shard)?;
                ok = enough;
            }
        }
        let k = s.skip as usize;
        if !ok || k < 1 || k > self.legs.len() {
            return Some(None);
        }
        let legs: Vec<XLeg> = self.legs.iter().enumerate().filter(|(i, _)| *i != k - 1).map(|(_, l)| *l).collect();
        let left_owing = sub(owes, to_shard)?;
        let acct = Account { legs, a_margin: after, b_margin: left_owing, a_min_counter: sub(count, 1)?, b_min_counter: time, ..self.clone() };
        let owed = (to_shard > 0).then_some(crate::reserve::Owed { sid: self.shard?, amount: to_shard });
        let ticket = Ticket { chain: s.lc, key };
        // (The account and the ticket are named by the chains the statement says: its rows hold those to the legs.)
        let mut v = vec![(Out { key: false, to: self.c_next_owing(after, &row.h, sub(count, 1)?, time, left_owing), value: add(held, sub(after, coins)?)? }, false), (Out { key: false, to: ticket.hash(), value: NOTE_MIN }, true)];
        if let Some(n) = &owed {
            v.push((Out { key: false, to: n.hash(), value: n.value() }, true));
        }
        Some(Outs::with_notes(&v).map(|outs| COutcome { after: Some(acct), paid: 0, fee: 0, note: None, split: None, mark: None, ticket: Some(ticket), owed, outs }))
    }

    /// What a claimer pays at the least for the token leg `l` sold in a step at the price `a`: its worth less the
    /// liquidation fee's rate.
    pub fn tok_price(&self, l: &XLeg, a: i64) -> Option<i64> {
        let w = worth(l.size, l.scale, a)?;
        sub(w, rate_down(w, self.t.liq_fee_bps)?)
    }

    /// `KpfTCEnd`'s verdict on the end of a statement with the outputs `outs` (`None`: neither).
    pub fn c_end_judged(&self, kind: i64, s: &CSaid, claimer: &[u8; 32], outs: &Outs, held: i64, t0: i64) -> Option<bool> {
        Some(self.c_end(kind, s, claimer, held, t0)?.is_some_and(|o| o.outs == *outs))
    }

    /// The first point of a statement on this covered account that is wrong at the committee's prices `c` (one a
    /// leg), for whoever disputes: 0 the end, 2i - 1 row i, 2i the price of row i. `None`: it is the rule's.
    pub fn c_wrong_point(&self, kind: i64, s: &CSaid, claimer: &[u8; 32], outs: &Outs, held: i64, t0: i64, c: &[i64]) -> Option<i64> {
        if self.c_end_judged(kind, s, claimer, outs, held, t0) == Some(false) {
            return Some(0);
        }
        self.c_rows_wrong_point_by(&s.rows, c, &|i, l, before, row| c_row_judged_of(kind, &before.g, i.checked_sub(1).map(|k| &self.legs[k]), l, before, row, i as i64 + 1, s))
    }

    /// As `Account::rows_wrong_point_by`, of a covered account's rows: the first row that does not carry the chain
    /// of its leg, or that `judge` does not find right, or whose price is not within the tolerance of the
    /// committee's. (A step's second price is its stater's, held to the first by the row: no point of its own.)
    pub fn c_rows_wrong_point_by(&self, rows: &[CRow], c: &[i64], judge: &dyn Fn(usize, &XLeg, &CRow, &CRow) -> Option<bool>) -> Option<i64> {
        let (n, m) = (rows.len(), self.legs.len());
        let mut chain = vec![[0u8; 32]];
        for l in &self.legs {
            chain.push(blake2b(&[&chain[chain.len() - 1][..], &leg_bytes(l)].concat()));
        }
        let own = |k: usize| -> Option<[u8; 32]> { (k + m >= n + 1).then(|| chain[k + m - n]) };
        if let Some(k) = (1..=n).rev().find(|k| own(*k) != Some(rows[k - 1].g) || rows[k - 1].g == [0; 32]) {
            return Some(if rows[k - 1].g == [0; 32] || k == n { 2 * k as i64 - 1 } else { 2 * k as i64 + 1 });
        }
        for (i, l) in self.legs.iter().enumerate() {
            let before = if i == 0 { CRow::ZERO } else { rows[i - 1] };
            if judge(i, l, &before, &rows[i]) != Some(true) {
                return Some(2 * i as i64 + 1);
            }
            let a = rows[i].a;
            if !crate::thin::price_right_at(l, a, c[i], self.t.g_bps) {
                return Some(2 * i as i64 + 2);
            }
        }
        None
    }

    /// The statement of `kind` on this covered account holding `held`, at the prices `a` (one a leg) and the time
    /// `t0`: a take of `amount` (7), a leaving of the leg `skip` or of [`ALL`] (8), a step that names the leg
    /// `skip` (9, by `claimer`).
    #[allow(clippy::too_many_arguments)]
    pub fn c_state(&self, kind: i64, amount: i64, skip: i64, a: &[i64], t0: i64, held: i64, claimer: [u8; 32]) -> Result<(Stated, CSaid, COutcome)> {
        self.c_check()?;
        let no = |why: &str| Error::Invalid(format!("no statement on this covered account: {why}"));
        if a.len() != self.legs.len() || !(0..LOCK_TIME_THRESHOLD).contains(&t0) || a.iter().zip(&self.legs).any(|(p, l)| *p <= 0 || *p > l.price_max) {
            return Err(no("it is stated with one price a leg, within its market's limit, and a time"));
        }
        // (A mark says the leg's new entry where a take has its amount; with none given, the nearest the rule allows.)
        let amount = if kind == KIND_C_MARK && amount == 0 {
            let leg = usize::try_from(skip - 1).ok().and_then(|k| self.legs.get(k)).ok_or_else(|| no("a mark names one leg"))?;
            self.mark_entry(leg, a[skip as usize - 1]).ok_or_else(|| no("the leg has no entry to be marked at within its limits"))?
        } else {
            amount
        };
        // (Tokens sold in a step: with nothing said, the claimer pays the least the rule allows.)
        let tok = kind == KIND_C_TOK_OUT || kind == KIND_C_TOK_SOLD;
        let amount = if kind == KIND_C_TOK_SOLD && amount == 0 {
            let leg = usize::try_from(skip - 1).ok().and_then(|k| self.legs.get(k)).ok_or_else(|| no("tokens sold are one leg's"))?;
            self.tok_price(leg, a[skip as usize - 1]).ok_or_else(|| no("the leg's worth is beyond what is counted"))?
        } else {
            amount
        };
        // (A step is stated by the prices that show it due if any do.)
        let rows = if kind == KIND_C_MARK { self.c_trace_marked(a, skip, amount) } else if tok { self.c_trace_tok(kind, a, skip) } else if kind == KIND_C_STEP { self.c_step_prices(a).and_then(|q| self.c_trace_step(a, &q, skip)) } else { self.c_trace(a, skip) }.ok_or_else(|| no("a sum of its trace is beyond what is counted (or the leg named is no token leg, where tokens leave or are sold)"))?;
        let lc = if skip >= 1 { self.legs.get(skip as usize - 1).map(chain_of_one).unwrap_or([0; 32]) } else { [0; 32] };
        let mut s = CSaid { amount, skip, lc, head: self.c_head(), rows };
        // (A step states its fee: the rule's.)
        if kind == KIND_C_STEP {
            for fee in self.c_fees(&s) {
                s.amount = fee;
                if let Some(Some(o)) = self.c_end(kind, &s, &claimer, held, t0) {
                    return Ok((self.c_stated(kind, &s, &o, t0, held, claimer), s, o));
                }
            }
            return Err(no("the user is not below its maintenance at these prices, or the leg named gained while another lost"));
        }
        let o = self.c_end(kind, &s, &claimer, held, t0).flatten().ok_or_else(|| no("the rule does not allow it at these prices and this time (a take, or what a leg that gained is asked, leaves the coins at the maintenance of the legs that stay and at what they have lost; a leg that lost leaves alone where the coins pay what it lost; the user states it, and not before the time its account says)"))?;
        Ok((self.c_stated(kind, &s, &o, t0, held, claimer), s, o))
    }

    /// The fees a step of this trace can be stated with: of the leg named, of all the legs, and those capped by the
    /// coins (what `c_end` then holds the statement to).
    fn c_fees(&self, s: &CSaid) -> Vec<i64> {
        let Some(row) = s.rows.last() else { return vec![] };
        let one = rate_down(row.w - row.ws, self.t.liq_fee_bps).unwrap_or(0);
        let all = rate_down(row.w, self.t.liq_fee_bps).unwrap_or(0);
        let room = (self.a_margin - NOTE_MIN).max(0);
        let pay = if row.gain < 0 { row.gain.saturating_neg().min(room) } else { 0 };
        // (And of an account that holds tokens: the fee of the leg named, as far as the coins go after its loss.)
        let lost = row.gl.saturating_neg().max(0).min(room);
        // (Of a leg that lost and leaves alone: no more than the coins hold beyond what it lost and what the legs
        // that stay lost.)
        let cap = (self.a_margin - NOTE_MIN + row.gl - row.ls).max(0);
        vec![one, one.min(cap), all.min(room - pay), one.min(cap).min(room - lost), one.min(room - lost), one.min(room)]
    }

    fn c_stated(&self, kind: i64, s: &CSaid, o: &COutcome, t0: i64, held: i64, claimer: [u8; 32]) -> Stated {
        Stated { position: self.hash(), window: self.dispute_daa, late: self.t.late_daa, kind, t0, arg: s.arg(), outs: o.outs, bond: self.c_bond_for(kind, s.amount), claimer, held }
    }
}

// ---------------------------------------------------------------- an entry (kind 10; docs 14.3)

impl Account {
    /// The bond of a statement on a covered account (kinds 7 to 10): the account's, and a note's least at the
    /// least (`KpfTChange`, `KpfTEnterS`: where no age can be asked the bond is the price. Review 56, F100).
    pub fn c_bond(&self) -> i64 {
        // (Of its coins: what a covered account owes its shard, where a position has side B's margin, is no coin
        // of its pot. docs 16.18.)
        let pot = self.a_margin;
        ((pot / BPS) * self.bond_bps + ((pot % BPS) * self.bond_bps) / BPS).max(NOTE_MIN)
    }

    /// `KpfTPass`: this covered account in another user's hands: the key `to` where its user's was, and nothing
    /// else of it changed (`docs/ARCHITECTURE-CLEARING.md` 15.1). Its shard's books are as they were.
    pub fn passed(&self, to: [u8; 32], landed: i64) -> Result<Account> {
        self.c_check()?;
        self.owes_nothing()?;
        Ok(Account { a_key: to, expiry: self.at_will(landed), ..self.clone() })
    }

    /// The time before which nothing is done on a covered account at will (an addition, a hand-over), where a
    /// position has its lease: after such a thing done on an output that landed at `landed`, that time and a
    /// lateness, or the time it had where that is later (review 57, F104).
    pub fn at_will(&self, landed: i64) -> i64 {
        self.expiry.max(landed.saturating_add(self.t.late_daa))
    }

    /// `KpfTJoinC`: this covered account and `other`, of one user under one shard that is not called, as one: the
    /// legs of this then of the other (six at most), the coins of both, the later of the two times (15.2).
    pub fn joined(&self, other: &Account, shard: &crate::reserve::Shard) -> Result<Account> {
        self.c_check()?;
        other.c_check()?;
        self.owes_nothing()?;
        other.owes_nothing()?;
        let same = self.a_key == other.a_key && self.b_key == other.b_key && self.t == other.t && self.dispute_daa == other.dispute_daa && self.bond_bps == other.bond_bps && self.shard == other.shard;
        if !same || self.shard != Some(shard.sid) || self.b_key != shard.owner || shard.called != 0 || self.legs.len() + other.legs.len() > C_LEGS_MAX {
            return Err(Error::Invalid("two accounts are joined where they are one user's under one shard, the same but for their legs, coins and time; the shard is not called; and they have six legs at most between them".into()));
        }
        let coins = add(self.a_margin, other.a_margin).ok_or_else(|| Error::Invalid("the coins of the two are beyond what is counted".into()))?;
        Ok(Account { legs: [self.legs.clone(), other.legs.clone()].concat(), a_margin: coins, a_min_counter: (self.legs.len() + other.legs.len()) as i64, b_min_counter: self.b_min_counter.max(other.b_min_counter), expiry: self.expiry.max(other.expiry), ..self.clone() })
    }
}

/// What is less than this is not paid by an output of its own under a call (`KpfTCStep`, `KpfTCallEnter`): beside
/// a note and the call a block takes no smaller payment. It is whoever steps's.
pub const PAY_MIN: i64 = NOTE_MIN;

/// What a step under a call makes of a covered account (`KpfTCStep`; `docs/ARCHITECTURE-CLEARING.md` 14.6): the
/// call's row of each leg; what the legs gained or lost at the call's prices; what the account pays the shard for
/// it (out of its coins, a note's least staying, which is then the note's own least); the terms' fee of whoever
/// steps (no output: what the transaction leaves is its funder's); what the user is paid (with the bond of a statement it stated, where the
/// step ends one); the bonds of a statement another stated and of its disputer; and the note that the legs left
/// (what it asks is the gain, or below zero what was paid). A payment of less than [`PAY_MIN`] is nothing here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallStep {
    pub at: Vec<usize>,
    pub gain: i64,
    pub pay: i64,
    pub fee: i64,
    pub user: i64,
    pub claimer_bond: i64,
    pub disputer_bond: i64,
    pub note: Left,
    /// What the young legs gained together, less their band (docs 17.8c): a claim of the second class, which the
    /// step's note says beside what it asks (`Left::hash_step`).
    pub young: i64,
    /// The account that goes on, where it holds token legs: with them, what is left of its coins, and what it owes.
    pub after: Option<Account>,
}

impl CallStep {
    /// The hash of the note the step writes: of a kind of its own (tag 9).
    pub fn note_hash(&self) -> [u8; 32] {
        self.note.hash_step(self.young)
    }
}

impl Account {
    /// `KpfTCStep`: this covered account, whose output holds `v` (the bonds of a statement on it apart), stepped
    /// under the call `call` of its shard. `None`: no step of it under this call (another shard's; no leg; a leg
    /// of no row of the call; a sum that does not fit).
    pub fn call_step(&self, call: &crate::reserve::Called, v: i64) -> Option<CallStep> {
        self.call_step_over(call, v, None)
    }

    /// As `call_step`, over a statement that is ended: (its bond, its claimer, whether it is disputed).
    pub fn call_step_over(&self, call: &crate::reserve::Called, v: i64, stood: Option<(i64, [u8; 32], bool)>) -> Option<CallStep> {
        self.call_step_at(call, v, stood, 0)
    }

    /// As `call_step_over`, of an output that landed at `landed`: an account that holds token legs goes on with
    /// them, and where the step makes it owe it says the time of that landing, a lateness and a dispute period.
    pub fn call_step_at(&self, call: &crate::reserve::Called, v: i64, stood: Option<(i64, [u8; 32], bool)>, landed: i64) -> Option<CallStep> {
        let sid = self.shard.filter(|s| *s == call.sid)?;
        if self.legs.is_empty() || self.legs.len() > C_LEGS_MAX || self.a_min_counter != self.legs.len() as i64 {
            return None;
        }
        let (mut gain, mut at) = (0i64, vec![]);
        // (17.9: a leg that entered less than T before the call's time, T twice a lateness and a dispute period,
        // is young. The young legs of an account are counted together: nothing where they gained, paid where
        // they lost.)
        let mut young = 0i64;
        // (The sum of the young legs' larger worths, at the entry or at the call's price: their band's base.)
        let mut ymax = 0i64;
        let ywin = ck(2i64.checked_mul(add(self.t.late_daa, self.dispute_daa)?))?;
        let tokens: Vec<XLeg> = self.legs.iter().filter(|l| l.dir == DIR_SPOT).copied().collect();
        let futures: Vec<XLeg> = self.legs.iter().filter(|l| l.dir != DIR_SPOT).copied().collect();
        if futures.is_empty() || self.b_margin < 0 {
            return None;
        }
        for l in &self.legs {
            // (A token leg is in no row of the call: it stays with the account. Its place among the rows named
            // is read by nothing.)
            if l.dir == DIR_SPOT {
                at.push(0);
                continue;
            }
            if (l.dir != LONG && l.dir != SHORT) || l.scale <= 0 || l.size <= 0 || l.entry <= 0 {
                return None;
            }
            // (The price the call's list has for the leg's market: the market plain, its price's limit, no day.
            // The place is the list's: docs 17.8b, 17.8d.)
            let name = l.market;
            let list = call.list();
            let r = list.iter().position(|e| e.market == name && e.limit == l.price_max && e.day == 0)?;
            let p = list[r].price;
            if p < 0 {
                return None;
            }
            let (wp, we) = (worth(l.size, l.scale, p)?, worth(l.size, l.scale, l.entry)?);
            let d = if l.dir == LONG { sub(wp, we)? } else { sub(we, wp)? };
            if leg_time(l) > sub(call.time, ywin)? {
                young = add(young, d)?;
                ymax = add(ymax, wp.max(we))?;
            } else {
                gain = add(gain, d)?;
            }
            at.push(r);
        }
        if young < 0 {
            gain = add(gain, young)?;
        }
        // (Where the young gained: a claim of the second class, less their band: the tolerance twice of the sum
        // of their larger worths, rounded up once. Nothing of it is set off, paid by or to the account.)
        let yask = if young > 0 { sub(young, rate_up(ymax, ck(2i64.checked_mul(self.t.g_bps))?)?)?.max(0) } else { 0 };
        // (An account that holds token legs goes on: it keeps a KAS of its coins back. Review 64, F113.)
        let keep = if tokens.is_empty() { 0 } else { crate::reserve::OWING_KEEP };
        let room = sub(sub(self.a_margin, NOTE_MIN)?, keep)?.max(0);
        let (pay, asked) = if gain < 0 {
            let pay = sub(0, gain)?.min(room);
            (pay, sub(0, pay)?)
        } else {
            (0, gain)
        };
        // (An output that holds less than its account says pays what it holds: review 54, F89.)
        let (pay, asked) = if pay > v { (v, sub(0, v)?) } else { (pay, asked) };
        if v < 0 {
            return None;
        }
        // (The note's least is the account's own, the one that stays in it while it has a leg: whoever steps adds
        // nothing, but where the output holds less. Review 54, F93.)
        let rest = v - pay;
        let rest = rest - NOTE_MIN.min(rest);
        let fee = self.t.keeper_fee_min.clamp(0, NOTE_MIN).min(rest).min(if keep > 0 { (rest - keep).max(0) } else { rest });
        let mut user = rest - fee;
        let (mut cb, mut db) = (0, 0);
        if let Some((bond, claimer, disputed)) = stood {
            if claimer == self.a_key {
                user = add(user, bond)?;
            } else {
                cb = bond;
            }
            if disputed {
                db = bond;
            }
        }
        // (What the account owes its shard is set against what its legs ask of it: review 64, F112.)
        let setoff = asked.max(0).min(self.b_margin);
        let asked = sub(asked, setoff)?;
        let least = |x: i64| if x < PAY_MIN { 0 } else { x };
        // An account that holds token legs goes on with them: what is left of its coins stays in it, and what its
        // futures legs lost beyond what the coins paid, it owes its shard.
        let after = if tokens.is_empty() {
            None
        } else {
            let more = if gain < 0 { sub(sub(0, gain)?, pay)? } else { sub(0, setoff)? };
            let quiet = if more > 0 { self.b_min_counter.max(add(add(landed, self.t.late_daa)?, self.dispute_daa)?) } else { self.b_min_counter };
            Some(Account { a_margin: user, b_margin: add(self.b_margin, more)?, a_min_counter: tokens.len() as i64, b_min_counter: quiet, legs: tokens, ..self.clone() })
        };
        let user = if after.is_some() { 0 } else { user };
        Some(CallStep { at, gain, pay, fee, user: least(user), claimer_bond: least(cb), disputer_bond: least(db), note: Left { sid, legs: futures, key: self.a_key, asked }, young: yask, after })
    }
}

/// A leg enters a covered account by a statement of its user and of its shard's owner, stated with the shard.
pub const KIND_C_ENTER: i64 = 10;
/// The mark of an account under an entry that is not settled: a push more at the end of its moving part (180
/// bytes: no script that takes an account takes it).
pub const MARK: [u8; 9] = [0x08, 1, 0, 0, 0, 0, 0, 0, 0];
/// The note that a leg did not enter (a note that a leg left, asking nothing, of a kind of its own).
pub const NOTE_UNENTERED: i64 = 8;

/// What an entry says: the coins the user adds, the shard's row the leg takes, the leg whole, the head its rows
/// compute with, the place the leg takes among the legs (the last: docs 17.8h, review T12, E2), and the account's
/// trace with the leg as its last row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CEnterSaid {
    pub add: i64,
    pub r: i64,
    pub leg: XLeg,
    pub head: CHead,
    pub place: i64,
    pub rows: Vec<CRow>,
}

/// Where an entry's rows begin: after two numbers, the leg, the head and the place.
pub const C_ENTER_SAID_AT: usize = 127 + CHEAD_LEN + 8;

impl CEnterSaid {
    pub fn said(&self) -> Vec<u8> {
        [sm8(self.add).to_vec(), sm8(self.r).to_vec(), leg_bytes(&self.leg), self.head.bytes(), sm8(self.place).to_vec(), crows_bytes(&self.rows)].concat()
    }

    pub fn arg(&self) -> [u8; 32] {
        blake2b(&self.said())
    }

    pub fn read(said: &[u8]) -> Option<CEnterSaid> {
        if said.len() < C_ENTER_SAID_AT + CROW_LEN || (said.len() - C_ENTER_SAID_AT) % CROW_LEN != 0 {
            return None;
        }
        let int = |b: &[u8]| crate::state::int8(b);
        let rows = said[C_ENTER_SAID_AT..].chunks(CROW_LEN).map(CRow::read).collect::<Option<Vec<CRow>>>()?;
        Some(CEnterSaid { add: int(&said[..8]), r: int(&said[8..16]), leg: crate::thin::leg_read(&said[16..127])?, head: CHead::read(&said[127..127 + CHEAD_LEN])?, place: int(&said[127 + CHEAD_LEN..C_ENTER_SAID_AT]), rows })
    }
}

/// An account under an entry that is not settled: the account as the entry makes it (the leg at the end of its
/// chain, the coins added, a leg more counted), with the mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marked(pub Account);

impl Marked {
    pub fn moving(&self) -> Vec<u8> {
        [self.0.moving(), MARK.to_vec()].concat()
    }

    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[p33(&self.0.fixed_hash()), self.moving()].concat())
    }

    /// The leg that has not entered, and the chain before it.
    pub fn leg(&self) -> Option<(XLeg, [u8; 32])> {
        let (l, before) = self.0.legs.split_last()?;
        Some((*l, Left::chain(before)))
    }

    /// `KpfTEnterOff`: the account without the leg, unmarked, a leg fewer counted, the coins added staying, less
    /// the terms' least fee of a keeper (as far as the coins go above a note's least); the fee; and the note that
    /// the leg did not enter.
    pub fn undone(&self) -> Option<(Account, i64, Unentered)> {
        let a = &self.0;
        let (l, _) = self.leg()?;
        let fee = a.t.keeper_fee_min.min(sub(a.a_margin, NOTE_MIN)?).max(0);
        let legs = a.legs[..a.legs.len() - 1].to_vec();
        let after = Account { legs, a_margin: sub(a.a_margin, fee)?, a_min_counter: sub(a.a_min_counter, 1)?, ..a.clone() };
        Some((after, fee, Unentered { sid: a.shard?, leg: l, key: a.a_key }))
    }
}

/// The note that a leg did not enter: applied to its shard as a note that the leg left, asking nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unentered {
    pub sid: [u8; 32],
    pub leg: XLeg,
    pub key: [u8; 32],
}

impl Unentered {
    pub fn hash(&self) -> [u8; 32] {
        blake2b(&[i9(12), p33(&self.sid), i9(NOTE_UNENTERED), p33(&chain_of_one(&self.leg)), p33(&self.key), i9(0)].concat())
    }

    pub fn value(&self) -> i64 {
        NOTE_MIN
    }

    /// What it does to the books: what a note that the leg left, asking nothing, does.
    pub fn as_left(&self) -> Left {
        Left { sid: self.sid, legs: vec![self.leg], key: self.key, asked: 0 }
    }
}

fn i9(v: i64) -> Vec<u8> {
    [&[0x08][..], &sm8(v)].concat()
}

/// `KpfTCRow` on a row of an entry: as a row of a covered account's statement in which nothing leaves; and the
/// last row is of the leg said, at its entry.
pub fn c_enter_row_judged(prev: &[u8; 32], leg_before: Option<&XLeg>, l: &XLeg, before: &CRow, row: &CRow, i: i64, s: &CEnterSaid) -> Option<bool> {
    // (A token leg enters by no entry: the last row of one is wrong whatever it sums.)
    if i == s.rows.len() as i64 && l.dir == DIR_SPOT {
        return if row.g != [0; 32] && blake2b(&[&prev[..], &leg_bytes(l)].concat()) != row.g { None } else { Some(false) };
    }
    // (The script finds the row of another leg than the one said wrong before it computes: so does this.)
    let last = i == s.rows.len() as i64;
    let g2 = two_before(&s.rows, i);
    let opened = i <= 1 || leg_before.is_some_and(|b| blake2b(&[&g2[..], &leg_bytes(b)].concat()) == before.g);
    if row.g != [0; 32] && blake2b(&[&prev[..], &leg_bytes(l)].concat()) == row.g && *prev == before.g && opened && last && !(*l == s.leg && l.entry == row.a) {
        return Some(false);
    }
    c_row_judged_with(KIND_C_ENTER, &s.head, 0, 0, &[0; 32], prev, &g2, leg_before, l, before, row, i)
}

impl Account {
    /// `KpfTEnterS`: what stating the entry of `leg` with `add` coins more makes of this account, at the time `t0`:
    /// the marked account. `None` where the script refuses (a statement on it stands elsewhere: the object is then
    /// not the account).
    pub fn c_marked(&self, leg: &XLeg, more: i64, t0: i64) -> Option<Marked> {
        let ok = self.shard.is_some()
            && self.b_margin == 0
            && (0..LOCK_TIME_THRESHOLD).contains(&t0)
            && t0 >= self.b_min_counter
            && (0..C_LEGS_MAX as i64).contains(&self.a_min_counter)
            && more >= 0
            && (leg.dir == LONG || leg.dir == SHORT)
            && leg.scale > 0
            && leg.scale <= 1_000_000_000
            && leg.size > 0
            && leg.size <= crate::cross::MARGIN_MAX
            && leg.price_max > 0
            && leg.entry > 0
            && leg.entry <= leg.price_max
            && leg.size / leg.scale <= crate::cross::MARGIN_MAX / leg.price_max - 1;
        let coins = add(self.a_margin, more)?;
        if !ok || coins > crate::cross::MARGIN_MAX {
            return None;
        }
        let mut legs = self.legs.clone();
        legs.push(*leg);
        Some(Marked(Account { legs, a_margin: coins, a_min_counter: self.a_min_counter + 1, ..self.clone() }))
    }

    /// `KpfTCEnter`: whether the end of an entry's trace is right for the marked account holding `held`: the user
    /// stands, of the net, with the leg; and the one output is the account with the leg, unmarked, with its time.
    /// `None`: a sum does not fit.
    pub fn c_enter_end(marked: &Marked, s: &CEnterSaid, claimer: &[u8; 32], held: i64, t0: i64) -> Option<Option<(Account, Outs)>> {
        let a = &marked.0;
        let Some(row) = s.rows.last() else { return Some(None) };
        let mine = a.c_head();
        if row.g != a.legs_root() || row.w < 0 || row.ls < 0 || row.tk < 0 || a.b_margin != 0 || *claimer != a.a_key || (s.head.rate, s.head.tolerance, s.head.t) != (mine.rate, mine.tolerance, mine.t) || s.place != s.rows.len() as i64 {
            return Some(None);
        }
        let (ra, _) = row.closed()?;
        let life = add(a.t.late_daa, ck(a.dispute_daa.checked_mul(2))?)?;
        let next = add(t0, ck(life.checked_mul(2))?)?;
        if add(add(sub(a.a_margin, NOTE_MIN)?, row.gain)?, row.tk)? < ra {
            return Some(None);
        }
        let after = Account { b_min_counter: next, ..a.clone() };
        let outs = Outs::of(&[Out { key: false, to: after.hash(), value: held }]);
        Some(outs.map(|o| (after, o)))
    }

    /// The first point of a stated entry that is wrong at the committee's prices `c` (one a leg of the marked
    /// account): 0 the end, 2i - 1 row i, 2i the price of row i.
    pub fn c_enter_wrong_point(marked: &Marked, s: &CEnterSaid, claimer: &[u8; 32], outs: &Outs, held: i64, t0: i64, c: &[i64]) -> Option<i64> {
        if Account::c_enter_end(marked, s, claimer, held, t0).is_some_and(|o| !o.is_some_and(|(_, x)| x == *outs)) {
            return Some(0);
        }
        marked.0.c_rows_wrong_point_by(&s.rows, c, &|i, l, before, row| c_enter_row_judged(&before.g, i.checked_sub(1).map(|k| &marked.0.legs[k]), l, before, row, i as i64 + 1, s))
    }

    /// The statement of the entry of `leg` (its entry its price) into this covered account holding `held`, with
    /// `add` coins more, taking the shard's row `r`, at the prices `a` of the legs it has (one each) and the time
    /// `t0`: the statement, what it says, the marked account, and the account it becomes.
    pub fn c_enter(&self, leg: XLeg, more: i64, r: i64, a: &[i64], t0: i64, held: i64) -> Result<(Stated, CEnterSaid, Marked, Account)> {
        self.c_check()?;
        self.owes_nothing()?;
        // (The leg as it stands in the account: it says the time it entered where it had its committee. 17.2.)
        let leg = leg_entered(&leg, t0);
        let no = |why: &str| Error::Invalid(format!("no entry into this covered account: {why}"));
        if a.len() != self.legs.len() || a.iter().zip(&self.legs).any(|(p, l)| *p <= 0 || *p > l.price_max) {
            return Err(no("it is stated with one price a leg the account has, within its market's limit"));
        }
        let marked = self.c_marked(&leg, more, t0).ok_or_else(|| no("the account is at six legs or before its time, or the leg is no futures leg within its bounds, or the coins are beyond what is counted"))?;
        let prices: Vec<i64> = a.iter().copied().chain([leg.entry]).collect();
        let rows = marked.0.c_trace_of(KIND_C_ENTER, &prices, &prices, |_, _| Part::Stays).ok_or_else(|| no("a sum of its trace is beyond what is counted, or two legs of one market next to one another are stated at two prices (a leg that enters next to a leg of its market enters at that leg's price)"))?;
        let s = CEnterSaid { add: more, r, leg, head: marked.0.c_head(), place: rows.len() as i64, rows };
        let held = held.checked_add(more).ok_or_else(|| no("what it holds is beyond what is counted"))?;
        let (after, outs) = Account::c_enter_end(&marked, &s, &self.a_key, held, t0).flatten().ok_or_else(|| no("the user does not stand at its maintenance with the leg, at these prices"))?;
        let st = Stated { position: marked.hash(), window: self.dispute_daa, late: self.t.late_daa, kind: KIND_C_ENTER, t0, arg: s.arg(), outs, bond: marked.0.c_bond(), claimer: self.a_key, held };
        Ok((st, s, marked, after))
    }
}
