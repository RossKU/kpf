//! The row of a covered account's trace as `docs/ARCHITECTURE-CLEARING.md` 17.8h and 17.8i have it: 256
//! bytes, twenty-four numbers and two chains, and the head of a statement that the rows compute with. The model of
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
pub const CROW_LEN: usize = 256;
/// What a held option that leaves adds to `ns` (the legs that stay that are not held options are counted below it).
pub const NS_HELD_LEAVES: i64 = 256;
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
/// `ps`, the premiums of all the option legs and of those that stay; `po`, what the option legs pay of their
/// premiums where they leave, all of them (17.8i). `g` is the chain of the legs, `h` the chain of
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
    /// The open group's two sums at the tolerance: what its legs gain from `a` to the price less, and plus, the
    /// tolerance on it (docs 17.8i).
    pub ddb: i64,
    pub dub: i64,
    /// How many of the legs that stay are not held options (a futures leg is one; a token leg none); and, by
    /// [`NS_HELD_LEAVES`], how many held options leave.
    pub ns: i64,
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

/// What a covered account's option leg says (docs 17.2, 17.8i): where a futures leg says its way, 3 a call the
/// user holds, 4 a call it wrote, 5 a put it holds, 6 a put it wrote; and in its 32 bytes, after the time it
/// entered, its day, its strike and its premium a unit. Its entry is the market's price where it entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opt {
    pub call: bool,
    pub held: bool,
    pub day: i64,
    pub strike: i64,
    pub premium: i64,
}

/// The option a leg is (`None`: a futures or a token leg).
pub fn option_of(l: &XLeg) -> Option<Opt> {
    let n = |k: usize| int8(&l.committee[8 * k..8 * k + 8]);
    (3..=6).contains(&l.dir).then(|| Opt { call: l.dir <= 4, held: l.dir % 2 == 1, day: n(1), strike: n(2), premium: n(3) })
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
    pub const ZERO: CRow = CRow { a: 0, gain: 0, w: 0, ws: 0, ls: 0, gl: 0, tk: 0, q: 0, m: 0, ba: 0, bl: 0, dda: 0, dua: 0, ra: 0, dds: 0, dus: 0, rs: 0, rl: 0, pa: 0, ps: 0, po: 0, ddb: 0, dub: 0, ns: 0, g: [0; 32], h: [0; 32] };

    fn numbers(&self) -> [i64; 24] {
        [self.a, self.gain, self.w, self.ws, self.ls, self.gl, self.tk, self.q, self.m, self.ba, self.bl, self.dda, self.dua, self.ra, self.dds, self.dus, self.rs, self.rl, self.pa, self.ps, self.po, self.ddb, self.dub, self.ns]
    }

    /// The 256 bytes: the numbers in their order (today's seven where they are today), then the two chains.
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
            a: n(0), gain: n(1), w: n(2), ws: n(3), ls: n(4), gl: n(5), tk: n(6), q: n(7), m: n(8), ba: n(9), bl: n(10), dda: n(11), dua: n(12), ra: n(13), dds: n(14), dus: n(15), rs: n(16), rl: n(17), pa: n(18), ps: n(19), po: n(20), ddb: n(21), dub: n(22), ns: n(23),
            g: r[192..224].try_into().expect("32 bytes"),
            h: r[224..256].try_into().expect("32 bytes"),
        })
    }

    /// R with the last group closed: of all the legs, and of those that stay (an end reads these).
    /// The band of every leg leaving: each leg's entry's part and what the closed groups lose by a move of the
    /// tolerance (`ba`), and of the open group what the larger of its two sums at the tolerance lost.
    pub fn band(&self) -> Option<i64> {
        add(self.ba, lost(self.ddb, self.dub))
    }

    pub fn closed(&self) -> Option<(i64, i64)> {
        Some((add(self.ra, lost(self.dda, self.dua))?, add(self.rs, lost(self.dds, self.dus))?))
    }
}

/// The row after `before` with the leg `l`, in a statement of `kind` whose head is `head`, at the price `a` and a
/// step's price `q`. `leg_before` is the leg of the row before (none for the first row): a leg of another market
/// closes the group; a leg of the same market says the same two prices.
/// `t0` is the statement's time and `i` the leg's place from 0 (an option leg reads them: its day, and the head's
/// bit of its line).
/// `None`: a sum does not fit (the script takes neither verdict). `Some(None)`: no right row follows (a price out
/// of its range, two prices in one group, a mark's entry out of range, a token's discount out of range, an option
/// that is ended, or named by a leaving or a mark).
#[allow(clippy::too_many_arguments)]
pub fn crow_after(kind: i64, head: &CHead, t0: i64, i: usize, before: &CRow, leg_before: Option<&XLeg>, l: &XLeg, a: i64, q: i64, part: Part) -> Option<Option<CRow>> {
    if a <= 0 || a > l.price_max || (l.dir != LONG && l.dir != SHORT && l.dir != DIR_SPOT && option_of(l).is_none()) {
        return Some(None);
    }
    // (A head whose rates are no rates: no right row. The end holds the head to the terms.)
    if !(0..=BPS).contains(&head.rate) || !(0..=BPS).contains(&head.tolerance) || head.lines < 0 {
        return Some(None);
    }
    if kind == KIND_C_STEP {
        // (The script takes the two differences before it compares: a price whose distance does not fit is
        // answered by neither verdict. Review T13, W5.)
        let (over, under) = (sub(q, a)?, sub(a, q)?);
        let dq = rate_up(a, head.lower())?;
        if q < 0 || over > dq || under > dq {
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
            r.ba = add(r.ba, lost(r.ddb, r.dub))?;
            (r.dda, r.dua, r.dds, r.dus, r.ddb, r.dub) = (0, 0, 0, 0, 0, 0);
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
    if let Some(o) = option_of(l) {
        // An option leg (docs 17.8i; review T14's list). Wrong before anything: a strike out of its market's
        // range, a premium below nothing, a day at or before the statement's time (an ended leg leaves by the
        // day's transaction), and where a leaving or a mark names it (nobody exercises before the day).
        if o.strike <= 0 || o.strike > l.price_max || o.premium < 0 || o.day <= t0 || ((kind == crate::covered::KIND_C_LEAVE || kind == KIND_C_MARK) && part != Part::Stays) || matches!(part, Part::Marked(_)) {
            return Some(None);
        }
        let wk = worth(l.size, l.scale, o.strike)?;
        // What it is exercised at a price, and what it is worth exercised there to the user.
        let ex = |p: i64| -> Option<i64> {
            let wp = worth(l.size, l.scale, p)?;
            if o.call { sub(wp, wk) } else { sub(wk, wp) }
        };
        let v = |p: i64| -> Option<i64> {
            let e = ex(p)?.max(0);
            if o.held { Some(e) } else { sub(0, e) }
        };
        let between = |lo: i64, up: i64| o.held && lo < o.strike && o.strike < up;
        let near = sub(o.day, t0)? <= head.t;
        let va = v(a)?;
        let d = sub(va, v(l.entry)?)?;
        let (lo, up) = ends(a, head.rate)?;
        // The two sums: exact; a held option whose strike is between the ends, along its line (in a step the
        // line of nothing, whatever the head says); within T of its day, each no more than nothing.
        let (mut dl, mut du) = if between(lo, up) {
            if kind != KIND_C_STEP && (head.lines >> i) & 1 == 1 { (sub(ex(lo)?, va)?, sub(ex(up)?, va)?) } else { (sub(0, va)?, sub(0, va)?) }
        } else {
            (sub(v(lo)?, va)?, sub(v(up)?, va)?)
        };
        let mut mq = sub(v(q)?, va)?;
        if near {
            (dl, du, mq) = (dl.min(0), du.min(0), mq.min(0));
        }
        // Its entry's part: what it is worth exercised changes by where its entry is a tolerance off for the user.
        let (el, eu) = ends(l.entry, head.tolerance)?;
        let ve = v(l.entry)?;
        let e = 0.max(sub(v(el)?, ve)?).max(sub(v(eu)?, ve)?);
        // The band's ends: what it gains to them; the line of nothing where its strike is between them.
        let (lob, upb) = ends(a, head.tolerance)?;
        let (bl, bu) = if between(lob, upb) { (sub(0, va)?, sub(0, va)?) } else { (sub(v(lob)?, va)?, sub(v(upb)?, va)?) };
        let premium = worth(l.size, l.scale, o.premium)?;
        r.m = add(r.m, mq)?;
        r.ba = add(r.ba, e)?;
        r.ddb = add(r.ddb, bl)?;
        r.dub = add(r.dub, bu)?;
        r.dda = add(r.dda, dl)?;
        r.dua = add(r.dua, du)?;
        r.pa = add(r.pa, premium)?;
        // (What it pays of its premium where it leaves: a holder's, whole; a writer's, less what it lost. Of every
        // option leg: a step that names one leg may take every leg.)
        r.po = add(r.po, if o.held { premium } else { sub(premium, (-d).max(0))?.max(0) })?;
        match part {
            Part::Stays => {
                if kind != KIND_C_MARK {
                    r.gain = add(r.gain, d)?;
                }
                r.ls = add(r.ls, (-d).max(0))?;
                r.dds = add(r.dds, dl)?;
                r.dus = add(r.dus, du)?;
                r.ps = add(r.ps, premium)?;
                if !o.held {
                    r.ns = add(r.ns, 1)?;
                }
                r.h = with(l);
            }
            _ => {
                r.gain = add(r.gain, d)?;
                r.gl = add(r.gl, d)?;
                r.rl = add(r.rl, lost(dl, du))?;
                // (Its band where it leaves alone: its entry's part and what it loses by a move of the tolerance.)
                r.bl = add(r.bl, add(e, 0.max(sub(va, v(lob)?)?).max(sub(va, v(upb)?)?))?)?;
                // (A held option that leaves is counted in the same number, by 256: an end is not shown the leg
                // named, and a step's rule reads whether it is a held option.)
                if o.held {
                    r.ns = add(r.ns, NS_HELD_LEAVES)?;
                }
            }
        }
        return Some(Some(r));
    }
    let s = |x: i64| if l.dir == LONG { Some(x) } else { sub(0, x) };
    let (lo, up) = ends(a, head.rate)?;
    let d = s(sub(w, worth(l.size, l.scale, l.entry)?)?)?;
    let dl = s(sub(worth(l.size, l.scale, lo)?, w)?)?;
    let du = s(sub(worth(l.size, l.scale, up)?, w)?)?;
    let mq = s(sub(worth(l.size, l.scale, q)?, w)?)?;
    // (Its band where it leaves alone: twice the tolerance of its worth. Its entry's part: the tolerance of its
    // worth. And what it gains to the two ends of a move of the tolerance.)
    let b = rate_up(w, 2 * head.tolerance)?;
    let (lob, upb) = ends(a, head.tolerance)?;
    r.w = add(r.w, w)?;
    r.ba = add(r.ba, rate_up(w, head.tolerance)?)?;
    r.ddb = add(r.ddb, s(sub(worth(l.size, l.scale, lob)?, w)?)?)?;
    r.dub = add(r.dub, s(sub(worth(l.size, l.scale, upb)?, w)?)?)?;
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
            r.ns = add(r.ns, 1)?;
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

/// The trace of the legs at the prices `a` and a step's prices `q` (a leg's each), stated at the time `t0`, each
/// leg doing what `part` says of it; `None` where no right trace is.
pub fn ctrace(kind: i64, head: &CHead, t0: i64, legs: &[XLeg], a: &[i64], q: &[i64], part: impl Fn(usize) -> Part) -> Option<Vec<CRow>> {
    let mut rows: Vec<CRow> = vec![];
    for (i, l) in legs.iter().enumerate() {
        let before = rows.last().copied().unwrap_or(CRow::ZERO);
        rows.push(crow_after(kind, head, t0, i, &before, i.checked_sub(1).map(|k| &legs[k]), l, *a.get(i)?, *q.get(i)?, part(i))??);
    }
    Some(rows)
}
