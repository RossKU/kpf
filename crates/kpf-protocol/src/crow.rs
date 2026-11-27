//! The row of a covered account's trace as `docs/ARCHITECTURE-CLEARING.md` 17.8h has it (after review T12): 232
//! bytes, twenty-one numbers and two chains, and the head of a statement that the rows compute with. The model of
//! the row's arithmetic, with the scripts' integers. Not yet what the scripts read: the row that is built is
//! `thin::Row` (120 bytes) and `covered::c_row_after`; this takes its place with stage 1 of 17.8h.
//!
//! - What an account is asked is what it can lose by a move of the rate, the legs of one market that stand next to
//!   one another counted together (17.4). A leg adds to two sums what it gains from the price `a` to the two ends
//!   of a move of the rate, L and U; a leg of another market than the one before closes the group: R grows by the
//!   larger of nothing, what the sum down lost and what the sum up lost.
//! - The sums are kept twice: of all the legs, and of those that stay.
//! - A step is due by one price `q` a group, within the lower rate of `a`: M is what the legs gain from `a` to it.
//! - Stage 1 has futures legs and token legs: the options' numbers (PA, PS, PO) are nothing.

use crate::committee::blake2b;
use crate::covered::{add, rate_down, rate_up, sub, worth, DIR_SPOT, KIND_C_MARK, KIND_C_STEP};
use crate::cross::XLeg;
use crate::state::{int8, sm8, BPS, LONG, SHORT};
use crate::thin::leg_bytes;

/// The length of a row.
pub const CROW_LEN: usize = 232;
/// The length of what a statement's head says for its rows: the rate, the tolerance, T and the lines.
pub const CHEAD_LEN: usize = 32;

/// What a statement says for its rows to compute with, and its end holds to the account's terms and times: the
/// rate (the maintenance and the tolerance), the tolerance, T (twice a lateness and a dispute period), and the
/// lines of the held options (a bit a leg; nothing in stage 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CHead {
    pub rate: i64,
    pub tolerance: i64,
    pub t: i64,
    pub lines: i64,
}

impl CHead {
    pub fn bytes(&self) -> Vec<u8> {
        [sm8(self.rate), sm8(self.tolerance), sm8(self.t), sm8(self.lines)].concat()
    }

    pub fn read(b: &[u8]) -> Option<CHead> {
        (b.len() == CHEAD_LEN).then(|| CHead { rate: int8(&b[..8]), tolerance: int8(&b[8..16]), t: int8(&b[16..24]), lines: int8(&b[24..32]) })
    }

    /// The rate a step's price is within: the rate less twice the tolerance, or nothing.
    pub fn lower(&self) -> i64 {
        (self.rate - 2 * self.tolerance).max(0)
    }
}

/// A row. `a`: the price stated for the leg's market; `q`: a step's price for it (the same as `a` in any other
/// statement). Over the legs up to it: `gain`, what all gained or lost; `w`, what all are worth; `ws`, the worth
/// of those that stay; `ls`, what those lost, each alone; `gl`, what those that leave gained or lost; `tk`, what
/// the token legs that stay count; `m`, what all gain from `a` to `q`; `ba`, `bl`, the bands of all and of those
/// that leave; (`dda`, `dua`, `ra`) the two sums of the group that is open and R of the groups closed, of all the
/// legs, and (`dds`, `dus`, `rs`) of those that stay; `rl`, what the legs that leave can lose, each alone; `pa`,
/// `ps`, `po`, the options' premiums (17.8h; nothing in stage 1). `g` is the chain of the legs, `h` the chain of
/// those that stay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CRow {
    pub a: i64,
    pub gain: i64,
    pub w: i64,
    pub ws: i64,
    pub ls: i64,
    pub gl: i64,
    pub tk: i64,
    pub q: i64,
    pub m: i64,
    pub ba: i64,
    pub bl: i64,
    pub dda: i64,
    pub dua: i64,
    pub ra: i64,
    pub dds: i64,
    pub dus: i64,
    pub rs: i64,
    pub rl: i64,
    pub pa: i64,
    pub ps: i64,
    pub po: i64,
    pub g: [u8; 32],
    pub h: [u8; 32],
}

/// The larger of nothing, what a sum down lost and what a sum up lost.
pub fn lost(dd: i64, du: i64) -> i64 {
    0.max(dd.checked_neg().unwrap_or(i64::MAX)).max(du.checked_neg().unwrap_or(i64::MAX))
}

/// The two ends of a move of the rate from a price: the rate on the price rounded up, the lower end not below
/// nothing.
pub fn ends(a: i64, rate: i64) -> Option<(i64, i64)> {
    let d = rate_up(a, rate)?;
    Some((sub(a, d)?.max(0), add(a, d)?))
}

/// What a leg does in a statement: it stays, it leaves, or it is the leg a mark names (it stays, entered from now
/// at the price said).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Stays,
    Leaves,
    Marked(i64),
}

impl CRow {
    /// What stands before the first row.
    pub const ZERO: CRow = CRow { a: 0, gain: 0, w: 0, ws: 0, ls: 0, gl: 0, tk: 0, q: 0, m: 0, ba: 0, bl: 0, dda: 0, dua: 0, ra: 0, dds: 0, dus: 0, rs: 0, rl: 0, pa: 0, ps: 0, po: 0, g: [0; 32], h: [0; 32] };

    fn numbers(&self) -> [i64; 21] {
        [self.a, self.gain, self.w, self.ws, self.ls, self.gl, self.tk, self.q, self.m, self.ba, self.bl, self.dda, self.dua, self.ra, self.dds, self.dus, self.rs, self.rl, self.pa, self.ps, self.po]
    }

    /// The 232 bytes: the numbers in their order (today's seven where they are today), then the two chains.
    pub fn bytes(&self) -> Vec<u8> {
        let mut b: Vec<u8> = self.numbers().iter().flat_map(|x| sm8(*x)).collect();
        b.extend_from_slice(&self.g);
        b.extend_from_slice(&self.h);
        b
    }

    pub fn read(r: &[u8]) -> Option<CRow> {
        if r.len() != CROW_LEN {
            return None;
        }
        let n = |i: usize| int8(&r[8 * i..8 * i + 8]);
        Some(CRow {
            a: n(0), gain: n(1), w: n(2), ws: n(3), ls: n(4), gl: n(5), tk: n(6), q: n(7), m: n(8), ba: n(9), bl: n(10), dda: n(11), dua: n(12), ra: n(13), dds: n(14), dus: n(15), rs: n(16), rl: n(17), pa: n(18), ps: n(19), po: n(20),
            g: r[168..200].try_into().expect("32 bytes"),
            h: r[200..232].try_into().expect("32 bytes"),
        })
    }

    /// R with the last group closed: of all the legs, and of those that stay (an end reads these).
    pub fn closed(&self) -> Option<(i64, i64)> {
        Some((add(self.ra, lost(self.dda, self.dua))?, add(self.rs, lost(self.dds, self.dus))?))
    }
}

/// The row after `before` with the leg `l`, in a statement of `kind` whose head is `head`, at the price `a` and a
/// step's price `q`. `leg_before` is the leg of the row before (none for the first row): a leg of another market
/// closes the group; a leg of the same market says the same two prices.
/// `None`: a sum does not fit (the script takes neither verdict). `Some(None)`: no right row follows (a price out
/// of its range, two prices in one group, a mark's entry out of range, a token's discount out of range).
pub fn crow_after(kind: i64, head: &CHead, before: &CRow, leg_before: Option<&XLeg>, l: &XLeg, a: i64, q: i64, part: Part) -> Option<Option<CRow>> {
    if a <= 0 || a > l.price_max || (l.dir != LONG && l.dir != SHORT && l.dir != DIR_SPOT) {
        return Some(None);
    }
    if kind == KIND_C_STEP {
        if q < 0 || sub(q, a)?.abs() > rate_up(a, head.lower())? {
            return Some(None);
        }
    } else if q != a {
        return Some(None);
    }
    let mut r = CRow { a, q, ..*before };
    match leg_before {
        Some(b) if b.market != l.market => {
            r.ra = add(r.ra, lost(r.dda, r.dua))?;
            r.rs = add(r.rs, lost(r.dds, r.dus))?;
            (r.dda, r.dua, r.dds, r.dus) = (0, 0, 0, 0);
        }
        Some(_) if before.a != a || before.q != q => return Some(None),
        _ => {}
    }
    let w = worth(l.size, l.scale, a)?;
    r.g = blake2b(&[&before.g[..], &leg_bytes(l)].concat());
    let with = |x: &XLeg| blake2b(&[&before.h[..], &leg_bytes(x)].concat());
    if l.dir == DIR_SPOT {
        // A token leg: nothing for the legs' worth, for what they gained or for what is asked. One that stays
        // counts, as coins, its worth less its discount (where a futures leg has its entry), rounded down, and a
        // sompi; the one named (kinds 14, 15) leaves the chain, its worth whole the fifth sum.
        match part {
            Part::Stays => {
                if !(0..=BPS).contains(&l.entry) {
                    return Some(None);
                }
                r.tk = add(r.tk, add(rate_down(w, BPS - l.entry)?, 1)?)?;
                r.h = with(l);
            }
            Part::Leaves => r.gl = add(r.gl, w)?,
            Part::Marked(_) => return Some(None),
        }
        return Some(Some(r));
    }
    let s = |x: i64| if l.dir == LONG { Some(x) } else { sub(0, x) };
    let (lo, up) = ends(a, head.rate)?;
    let d = s(sub(w, worth(l.size, l.scale, l.entry)?)?)?;
    let dl = s(sub(worth(l.size, l.scale, lo)?, w)?)?;
    let du = s(sub(worth(l.size, l.scale, up)?, w)?)?;
    let mq = s(sub(worth(l.size, l.scale, q)?, w)?)?;
    let b = rate_up(w, 2 * head.tolerance)?;
    r.w = add(r.w, w)?;
    r.ba = add(r.ba, b)?;
    r.dda = add(r.dda, dl)?;
    r.dua = add(r.dua, du)?;
    match part {
        Part::Stays => {
            // (Of a mark, the legs that are not named add nothing to the first sum: it is the marked leg's.)
            if kind != KIND_C_MARK {
                r.gain = add(r.gain, d)?;
            }
            r.m = add(r.m, mq)?;
            r.ws = add(r.ws, w)?;
            r.ls = add(r.ls, (-d).max(0))?;
            r.dds = add(r.dds, dl)?;
            r.dus = add(r.dus, du)?;
            r.h = with(l);
        }
        Part::Leaves => {
            r.gain = add(r.gain, d)?;
            r.m = add(r.m, mq)?;
            r.gl = add(r.gl, d)?;
            r.bl = add(r.bl, b)?;
            r.rl = add(r.rl, lost(dl, du))?;
        }
        Part::Marked(ne) => {
            if ne <= 0 || ne > l.price_max {
                return Some(None);
            }
            let wne = worth(l.size, l.scale, ne)?;
            r.gain = add(r.gain, s(sub(w, wne)?)?)?;
            r.gl = add(r.gl, s(sub(wne, worth(l.size, l.scale, l.entry)?)?)?)?;
            r.bl = add(r.bl, b)?;
            r.h = with(&XLeg { entry: ne, ..*l });
        }
    }
    Some(Some(r))
}

/// The trace of the legs at the prices `a` and a step's prices `q` (a leg's each), each leg doing what `part`
/// says of it; `None` where no right trace is.
pub fn ctrace(kind: i64, head: &CHead, legs: &[XLeg], a: &[i64], q: &[i64], part: impl Fn(usize) -> Part) -> Option<Vec<CRow>> {
    let mut rows: Vec<CRow> = vec![];
    for (i, l) in legs.iter().enumerate() {
        let before = rows.last().copied().unwrap_or(CRow::ZERO);
        rows.push(crow_after(kind, head, &before, i.checked_sub(1).map(|k| &legs[k]), l, *a.get(i)?, *q.get(i)?, part(i))??);
    }
    Some(rows)
}
