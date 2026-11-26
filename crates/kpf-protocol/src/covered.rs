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
//! - A leaving is of one leg or of all. Of all, the shard is asked the account's net, once, beyond the band. A leg
//!   that lost leaves alone where the coins pay what it lost.
//! - What a leaving writes is one note to the shard ([`Left`]): the legs, the user's key, and what the shard is
//!   asked; below zero where the account pays the shard, and the note then holds those coins.

use crate::committee::blake2b;
use crate::cross::XLeg;
use crate::error::{Error, Result};
use crate::reserve::{Left, NOTE_MIN};
use crate::state::{sm8, BPS, LOCK_TIME_THRESHOLD, LONG, SHORT};
use crate::thin::{leg_bytes, Account, Out, Outs, Row, Stated, ROW_LEN};

/// The user takes coins out of a covered account.
pub const KIND_C_TAKE: i64 = 7;
/// Legs leave a covered account by its user's statement: one, or all.
pub const KIND_C_LEAVE: i64 = 8;
/// A step of the liquidation of a covered account's user.
pub const KIND_C_STEP: i64 = 9;
/// In the place of the leg that leaves: every leg.
pub const ALL: i64 = -1;

fn p33(h: &[u8; 32]) -> Vec<u8> {
    [&[0x20][..], h].concat()
}

// The engine's integers are 64 bits and checked, and have no least value: these are `None` where a script fails.
fn ck(v: Option<i64>) -> Option<i64> {
    v.filter(|x| *x != i64::MIN)
}
fn add(a: i64, b: i64) -> Option<i64> {
    ck(a.checked_add(b))
}
fn sub(a: i64, b: i64) -> Option<i64> {
    ck(a.checked_sub(b))
}
fn worth(n: i64, sc: i64, p: i64) -> Option<i64> {
    if sc == 0 {
        return None;
    }
    add(add(ck((n / sc).checked_mul(p))?, ck((n % sc).checked_mul(p / sc))?)?, ck((n % sc).checked_mul(p % sc))? / sc)
}
fn rate_up(w: i64, r: i64) -> Option<i64> {
    add(ck((w / BPS).checked_mul(r))?, add(ck((w % BPS).checked_mul(r))?, BPS - 1)? / BPS)
}
fn rate_down(w: i64, r: i64) -> Option<i64> {
    add(ck((w / BPS).checked_mul(r))?, ck((w % BPS).checked_mul(r))? / BPS)
}

/// A row of a covered account's trace. It has a position's row's bytes, and these sums over the legs up to it at
/// the prices stated: `gain`, what all gained or lost; `wa`, what all are worth; `wb`, what the legs that stay are
/// worth; `ea`, what the legs that stay have lost, each counted alone; `eb`, what the legs that leave gained or
/// lost. `g` is the chain of the legs, `h` the chain of the legs that stay (their entries as they are).
pub fn c_row_after(before: &Row, l: &XLeg, a: i64, leaves: bool) -> Option<Row> {
    let w = worth(l.size, l.scale, a)?;
    let d = sub(w, worth(l.size, l.scale, l.entry)?)?;
    let d = if l.dir == LONG { d } else { sub(0, d)? };
    let g = blake2b(&[&before.g[..], &leg_bytes(l)].concat());
    let (gain, wa) = (add(before.gain, d)?, add(before.wa, w)?);
    Some(if leaves {
        Row { a, gain, wa, wb: before.wb, ea: before.ea, eb: add(before.eb, d)?, g, h: before.h }
    } else {
        Row { a, gain, wa, wb: add(before.wb, w)?, ea: add(before.ea, (-d).max(0))?, eb: before.eb, g, h: blake2b(&[&before.h[..], &leg_bytes(l)].concat()) }
    })
}

/// The chain of one leg that leaves alone (what its note names it by).
pub fn chain_of_one(l: &XLeg) -> [u8; 32] {
    Left::chain(&[*l])
}

/// What a statement on a covered account shows: the amount (a take's; a step's fee), the leg that leaves (0 none,
/// [`ALL`] every leg), the chain of the leg that leaves alone, and the trace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CSaid {
    pub amount: i64,
    pub skip: i64,
    pub lc: [u8; 32],
    pub rows: Vec<Row>,
}

impl CSaid {
    pub fn said(&self) -> Vec<u8> {
        [sm8(self.amount).to_vec(), sm8(self.skip).to_vec(), self.lc.to_vec(), crate::thin::rows_bytes(&self.rows)].concat()
    }

    pub fn arg(&self) -> [u8; 32] {
        blake2b(&self.said())
    }

    pub fn read(said: &[u8]) -> Option<CSaid> {
        if said.len() < 48 || (said.len() - 48) % ROW_LEN != 0 {
            return None;
        }
        let int = |b: &[u8]| crate::state::int8(b);
        let rows = said[48..].chunks(ROW_LEN).map(|r| Row { a: int(&r[..8]), gain: int(&r[8..16]), wa: int(&r[16..24]), wb: int(&r[24..32]), ea: int(&r[32..40]), eb: int(&r[40..48]), g: r[48..80].try_into().expect("32 bytes"), h: r[80..112].try_into().expect("32 bytes") }).collect();
        Some(CSaid { amount: int(&said[..8]), skip: int(&said[8..16]), lc: said[16..48].try_into().expect("32 bytes"), rows })
    }
}

/// `KpfTCRow`: row `i` (from 1) is opened by a chain `prev` and a leg that hash to its chain (`None` where they do
/// not, or where a sum does not fit: the script then takes neither verdict). It is wrong where `prev` is not the
/// chain of the row before it, where the leg is no futures leg, where its price is not within its market's limit,
/// where the leg leaves alone and is not the one the statement names by its chain; and else right exactly when it
/// follows from the row before and the leg.
pub fn c_row_judged(prev: &[u8; 32], l: &XLeg, before: &Row, row: &Row, i: i64, s: &CSaid) -> Option<bool> {
    if row.g == [0; 32] {
        return Some(false);
    }
    if blake2b(&[&prev[..], &leg_bytes(l)].concat()) != row.g {
        return None;
    }
    let mut ok = *prev == before.g && row.a > 0 && row.a <= l.price_max && (l.dir == LONG || l.dir == SHORT);
    let leaves = s.skip == i || s.skip == ALL;
    if s.skip == i && chain_of_one(l) != s.lc {
        ok = false;
    }
    if !ok {
        return Some(false);
    }
    // (The script computes each sum before it compares.)
    let w = worth(l.size, l.scale, row.a)?;
    let d = sub(w, worth(l.size, l.scale, l.entry)?)?;
    let d = if l.dir == LONG { d } else { sub(0, d)? };
    let (ws, ls, gl, h) = if leaves { (0, 0, d, before.h) } else { (w, if d < 0 { sub(0, d)? } else { 0 }, 0, blake2b(&[&before.h[..], &leg_bytes(l)].concat())) };
    let sums = [add(before.gain, d)?, add(before.wa, w)?, add(before.wb, ws)?, add(before.ea, ls)?, add(before.eb, gl)?];
    Some(sums == [row.gain, row.wa, row.wb, row.ea, row.eb] && h == row.h)
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
    pub outs: Outs,
}

impl Account {
    /// The trace of a statement on this covered account at the prices `a`, one a leg; `skip`: the leg that leaves
    /// (0 none, [`ALL`] every leg). `None` where a sum does not fit.
    pub fn c_trace(&self, a: &[i64], skip: i64) -> Option<Vec<Row>> {
        let mut rows: Vec<Row> = vec![];
        for (i, (l, p)) in self.legs.iter().zip(a).enumerate() {
            rows.push(c_row_after(rows.last().unwrap_or(&Row::ZERO), l, *p, skip == ALL || skip == i as i64 + 1)?);
        }
        Some(rows)
    }

    /// The hash of this account with other coins of its user's and another chain of legs.
    pub fn c_hash(&self, coins: i64, chain: &[u8; 32]) -> [u8; 32] {
        let next = Account { a_margin: coins, ..self.clone() };
        blake2b(&[p33(&blake2b(&[self.terms(), p33(chain)].concat())), next.moving()].concat())
    }

    /// `KpfTCEnd`: what the end of a statement's trace makes of this account holding `held`, by the last row's
    /// sums. `None`: the script takes neither verdict (a sum does not fit). `Some(None)`: the statement is not the
    /// rule's. Else what the account becomes and the outputs that say so.
    pub fn c_end(&self, kind: i64, s: &CSaid, claimer: &[u8; 32], held: i64) -> Option<Option<COutcome>> {
        let Some(sid) = self.shard else { return Some(None) };
        let n = s.rows.len() as i64;
        let Some(row) = s.rows.last() else { return Some(None) };
        let (gain, w, ws, ls, gl) = (row.gain, row.wa, row.wb, row.ea, row.eb);
        if !(KIND_C_TAKE..=KIND_C_STEP).contains(&kind) || row.g != self.legs_root() || w < 0 || ws < 0 || ws > w || ls < 0 || self.b_margin != 0 {
            return Some(None);
        }
        // (A note's least is no coin of the account's: it stays with the user where every leg leaves.)
        let coins = self.a_margin;
        let m = sub(coins, NOTE_MIN)?;
        let g = self.t.g_bps;
        let up = add(self.t.mmr_bps, g)?;
        let out = |v: Vec<(Out, bool)>, after: Option<Account>, paid: i64, fee: i64, note: Option<Left>| Outs::with_notes(&v).map(|outs| COutcome { after, paid, fee, note, outs });
        if kind == KIND_C_TAKE {
            if s.skip != 0 || *claimer != self.a_key || s.amount <= 0 {
                return Some(None);
            }
            if sub(m, s.amount)? < add(rate_up(w, up)?, ls)? {
                return Some(None);
            }
            let left = sub(coins, s.amount)?;
            let after = Account { a_margin: left, ..self.clone() };
            let v = vec![(Out { key: false, to: self.c_hash(left, &self.legs_root()), value: sub(held, s.amount)? }, false), (Out { key: true, to: *claimer, value: s.amount }, false)];
            return Some(out(v, Some(after), s.amount, 0, None));
        }
        let mut all = s.skip == ALL || row.h == [0; 32];
        if s.skip == 0 || s.skip < ALL || s.skip > n || (kind == KIND_C_LEAVE && *claimer != self.a_key) || (kind == KIND_C_STEP && s.skip < 1) {
            return Some(None);
        }
        let wl = sub(w, ws)?;
        let mut fee = 0;
        if kind == KIND_C_LEAVE {
            if !all {
                let stands = if gl >= 0 { m >= add(rate_up(ws, up)?, ls)? } else { add(m, gl)? >= 0 };
                if !stands {
                    return Some(None);
                }
            }
        } else {
            let lo = sub(sub(up, g)?, g)?.max(0);
            if add(m, gain)? >= rate_up(w, lo)? {
                return Some(None);
            }
            if !all {
                if gl < 0 {
                    fee = rate_down(wl, self.t.liq_fee_bps)?;
                    // (The fee is not taken before what the legs that stay have lost: review 47, F73.)
                    if sub(sub(add(m, gl)?, fee)?, ls)? < 0 {
                        all = true;
                    }
                } else {
                    if ls != 0 {
                        return Some(None);
                    }
                    all = true;
                }
            }
            if all {
                fee = rate_down(w, self.t.liq_fee_bps)?;
            }
        }
        let g2 = add(g, g)?;
        let (mut pay, asked);
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
            }
            let left = sub(sub(coins, pay)?, fee)?;
            let k = s.skip as usize - 1;
            let leg = self.legs.get(k).copied();
            let legs: Vec<XLeg> = self.legs.iter().enumerate().filter(|(i, _)| *i != k).map(|(_, l)| *l).collect();
            let after = Account { legs, a_margin: left, ..self.clone() };
            // (The note names the leg by the chain the statement says: its row holds that to the leg.)
            let note = leg.map(|l| Left { sid, legs: vec![l], key: self.a_key, asked });
            let mut o = vec![(Out { key: false, to: self.c_hash(left, &row.h), value: sub(sub(held, pay)?, fee)? }, false)];
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

    /// `KpfTCEnd`'s verdict on the end of a statement with the outputs `outs` (`None`: neither).
    pub fn c_end_judged(&self, kind: i64, s: &CSaid, claimer: &[u8; 32], outs: &Outs, held: i64) -> Option<bool> {
        Some(self.c_end(kind, s, claimer, held)?.is_some_and(|o| o.outs == *outs))
    }

    /// The first point of a statement on this covered account that is wrong at the committee's prices `c` (one a
    /// leg), for whoever disputes: 0 the end, 2i - 1 row i, 2i the price of row i. `None`: it is the rule's.
    pub fn c_wrong_point(&self, kind: i64, s: &CSaid, claimer: &[u8; 32], outs: &Outs, held: i64, c: &[i64]) -> Option<i64> {
        if self.c_end_judged(kind, s, claimer, outs, held) == Some(false) {
            return Some(0);
        }
        self.rows_wrong_point_by(&s.rows, c, &|i, l, before, row| c_row_judged(&before.g, l, before, row, i as i64 + 1, s))
    }

    /// The statement of `kind` on this covered account holding `held`, at the prices `a` (one a leg) and the time
    /// `t0`: a take of `amount` (7), a leaving of the leg `skip` or of [`ALL`] (8), a step that names the leg
    /// `skip` (9, by `claimer`).
    #[allow(clippy::too_many_arguments)]
    pub fn c_state(&self, kind: i64, amount: i64, skip: i64, a: &[i64], t0: i64, held: i64, claimer: [u8; 32]) -> Result<(Stated, CSaid, COutcome)> {
        self.check()?;
        let no = |why: &str| Error::Invalid(format!("no statement on this covered account: {why}"));
        if a.len() != self.legs.len() || !(0..LOCK_TIME_THRESHOLD).contains(&t0) || a.iter().zip(&self.legs).any(|(p, l)| *p <= 0 || *p > l.price_max) {
            return Err(no("it is stated with one price a leg, within its market's limit, and a time"));
        }
        let rows = self.c_trace(a, skip).ok_or_else(|| no("a sum of its trace is beyond what is counted"))?;
        let lc = if skip >= 1 { self.legs.get(skip as usize - 1).map(chain_of_one).unwrap_or([0; 32]) } else { [0; 32] };
        let mut s = CSaid { amount, skip, lc, rows };
        // (A step states its fee: the rule's.)
        if kind == KIND_C_STEP {
            for fee in self.c_fees(&s) {
                s.amount = fee;
                if let Some(Some(o)) = self.c_end(kind, &s, &claimer, held) {
                    return Ok((self.c_stated(kind, &s, &o, t0, held, claimer), s, o));
                }
            }
            return Err(no("the user is not below its maintenance at these prices, or the leg named gained while another lost"));
        }
        let o = self.c_end(kind, &s, &claimer, held).flatten().ok_or_else(|| no("the rule does not allow it at these prices (a take, or what a leg that gained is asked, leaves the coins at the maintenance of the legs that stay and at what they have lost; a leg that lost leaves alone where the coins pay what it lost; the user states it)"))?;
        Ok((self.c_stated(kind, &s, &o, t0, held, claimer), s, o))
    }

    /// The fees a step of this trace can be stated with: of the leg named, of all the legs, and those capped by the
    /// coins (what `c_end` then holds the statement to).
    fn c_fees(&self, s: &CSaid) -> Vec<i64> {
        let Some(row) = s.rows.last() else { return vec![] };
        let one = rate_down(row.wa - row.wb, self.t.liq_fee_bps).unwrap_or(0);
        let all = rate_down(row.wa, self.t.liq_fee_bps).unwrap_or(0);
        let room = (self.a_margin - NOTE_MIN).max(0);
        let pay = if row.gain < 0 { row.gain.saturating_neg().min(room) } else { 0 };
        vec![one, all.min(room - pay)]
    }

    fn c_stated(&self, kind: i64, s: &CSaid, o: &COutcome, t0: i64, held: i64, claimer: [u8; 32]) -> Stated {
        Stated { position: self.hash(), window: self.dispute_daa, late: self.t.late_daa, kind, t0, arg: s.arg(), outs: o.outs, bond: self.bond(), claimer, held }
    }
}
