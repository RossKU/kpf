//! The pure payoff math of DESIGN 3.1, 4.5a, 4.5b, 4.6 and 5, as functions.
//!
//! Every function mirrors a function of `KpfPair.sil` or `KpfOpen.sil` (named in each doc) and is tested against the
//! contracts' behaviour in the engine tests: a builder pays each party exactly the least value its covenant accepts.
//! Arithmetic is done in `i128` and checked into `i64`, so a result the contract would overflow on is a panic here
//! (`q`) or an error (`q_checked`), never a wrong number.
//!
//! Evidence clock (DESIGN 4.5c). The output builders copy `since` from the pair they are given into every output
//! that does not change hands (remainders, continuations), so they must be given the pair as [`Pair::clocked`] by
//! its UTXO's DAA score; the parts that change hands (P3, P7, P4's re-paired output) are written with `since = 0`.

use kaspa_consensus_core::Hash;

use crate::state::{Market, Open, Pair, LONG, PLAIN_DUST};

/// `quoteOf(n, r, c)`: n base units at r sompi per whole token (`scale` base units), `c = 0` rounds down,
/// `c = scale - 1` rounds up. The product `n * r` is never formed (DESIGN 5).
pub fn q_checked(n: i64, r: i64, scale: i64, c: i64) -> Option<i64> {
    let (n, r, scale, c) = (n as i128, r as i128, scale as i128, c as i128);
    let v = (n / scale) * r + (n % scale) * (r / scale) + (((n % scale) * (r % scale)) + c) / scale;
    i64::try_from(v).ok()
}

/// [`q_checked`], panicking when the value does not fit 63 bits (the contracts fail the script then).
pub fn q(n: i64, r: i64, scale: i64, c: i64) -> i64 {
    q_checked(n, r, scale, c).expect("q fits 63 bits")
}

/// `KpfPair.owed`: what a leaver of n is owed at price distance r less tip rate t: `q(n, max(r,0)) - q(n, tip)` with
/// the tip capped at the distance and floored at 0.
pub fn owed(n: i64, r: i64, t: i64, scale: i64) -> i64 {
    let rr = r.max(0);
    let tt = t.min(rr).max(0);
    q(n, rr, scale, 0) - q(n, tt, scale, 0)
}

/// Long and short payout of a whole pair at price `p` (DESIGN 3.1), carrier excluded.
pub fn payouts_at(p: &Pair, price: i64) -> (i64, i64) {
    (p.long_at(price), p.short_at(price))
}

/// The keeper fee before its cap, `KpfPair.feeAt`: `max(keeperFeeMin, q(n, floor(quote * liqFeeBps / 10^4)))`.
pub fn fee_of(m: &Market, n: i64, quote: i64) -> i64 {
    q(n, quote * m.liq_fee_bps / 10_000, m.scale, 0).max(m.keeper_fee_min)
}

/// The trigger price T of a loser `side` (DESIGN 4.5b), from the pair's own state:
/// `floor(bl * 10^4 / (10^4 - mmrBps))` for a long, `ceil(bs * 10^4 / (10^4 + mmrBps))` for a short. Not clamped
/// (DESIGN 4.5c): a pair that passes [`Pair::range_ok`] has T strictly inside (bl, bs), and no other pair is liquidatable.
pub fn trigger_of(p: &Pair, side: i64) -> i64 {
    let m = p.m.mmr_bps;
    let t = if side == LONG { p.bl * 10_000 / (10_000 - m) } else { (p.bs * 10_000 + 10_000 + m - 1) / (10_000 + m) };
    t
}

/// Whether the long is liquidatable at an evidence ask `a`: `a * (10^4 - mmrBps) < bl * 10^4` (DESIGN 3.6) and the
/// range is wider than both maintenance bands (DESIGN 4.5c).
pub fn long_liquidatable(p: &Pair, ask: i64) -> bool {
    p.range_ok() && (ask as i128) * ((10_000 - p.m.mmr_bps) as i128) < (p.bl as i128) * 10_000
}

/// Whether the short is liquidatable at an evidence bid `b`: `bs * 10^4 < b * (10^4 + mmrBps)` and the range is
/// wider than both maintenance bands.
pub fn short_liquidatable(p: &Pair, bid: i64) -> bool {
    p.range_ok() && (p.bs as i128) * 10_000 < (bid as i128) * ((10_000 + p.m.mmr_bps) as i128)
}

/// The maintenance slice of n for a loser `side`: `q(n, T - bl)` (long), `q(n, bs - T)` (short).
pub fn slice_of(p: &Pair, n: i64, side: i64) -> i64 {
    let t = trigger_of(p, side);
    if side == LONG {
        q(n, t - p.bl, p.m.scale, 0)
    } else {
        q(n, p.bs - t, p.m.scale, 0)
    }
}

/// The liquidation fee of P6 and P7 (DESIGN 4.5b): [`fee_of`] capped at the maintenance slice.
pub fn liq_fee(p: &Pair, n: i64, quote: i64, side: i64) -> i64 {
    fee_of(&p.m, n, quote).min(slice_of(p, n, side))
}

/// The winner's payoff at the trigger price for n of a loser `side`.
pub fn winner_at_trigger(p: &Pair, n: i64, side: i64) -> i64 {
    let t = trigger_of(p, side);
    if side == LONG {
        q(n, p.bs - t, p.m.scale, 0)
    } else {
        q(n, t - p.bl, p.m.scale, 0)
    }
}

/// The loser's bankruptcy price: where P6 settles (the loser is owed 0).
pub fn loser_price(p: &Pair, side: i64) -> i64 {
    if side == LONG {
        p.bl
    } else {
        p.bs
    }
}

/// An evidence quote clamped to the pair's range.
pub fn clamp_price(p: &Pair, x: i64) -> i64 {
    x.clamp(p.bl, p.bs)
}

/// The winner's key of a loser `side`.
pub fn winner_key(p: &Pair, side: i64) -> [u8; 32] {
    if side == LONG {
        p.short_key
    } else {
        p.long_key
    }
}

/// `KpfPair.closeAt`: (long payout, short payout, remainder and its least value) of closing n at price c in [bl, bs],
/// `side` being the party owed exactly its distance, the other party taking the rest less `fee`.
pub fn close_at(p: &Pair, value: u64, n: i64, c: i64, side: i64, fee: i64) -> (i64, i64, Option<(Pair, i64)>) {
    let v = value as i64;
    let part = p.range_of(n);
    let (mut keep, avail) = if n == p.size { (0, v) } else { (v - part, part) };
    let mut owe = if side == LONG { q(n, c - p.bl, p.m.scale, 0) } else { q(n, p.bs - c, p.m.scale, 0) };
    let mut other = (avail - owe - fee).max(0);
    let rest = if n == p.size {
        if owe < PLAIN_DUST {
            other += owe;
            owe = 0;
        }
        if other < PLAIN_DUST {
            owe += other;
            other = 0;
        }
        if owe < PLAIN_DUST {
            owe = 0;
        }
        None
    } else {
        if owe < PLAIN_DUST {
            keep += owe;
            owe = 0;
        }
        if other < PLAIN_DUST {
            keep += other;
            other = 0;
        }
        let r = p.rest_of(p.size - n);
        let rv = (r.range_of(r.size) + p.m.carrier).max(keep);
        Some((r, rv))
    };
    if side == LONG {
        (owe, other, rest)
    } else {
        (other, owe, rest)
    }
}

// ---------------------------------------------------------------- orders

/// What a KpfOpen fill or take may spend for n (`spend` in the contract).
pub fn order_spend(o: &Open, n: i64) -> u64 {
    o.spend_for(n) as u64
}

/// The value and state a KpfOpen of `value` sompi must leave after a fill of n: its continuation (n < sizeLeft) or the
/// refund to its owner (n = sizeLeft, `None`).
pub fn order_rest(o: &Open, value: u64, n: i64) -> (u64, Option<Open>) {
    let left = o.size_left - n;
    let rest = value - order_spend(o, n);
    if left > 0 {
        let mut c = o.clone();
        c.size_left = left;
        (rest, Some(c))
    } else {
        (rest, None)
    }
}

// ---------------------------------------------------------------- hand-over (P3, P7)

#[derive(Clone, Debug)]
pub struct TransferSpec {
    pub side: i64,
    pub n: i64,
    pub x: i64,
    /// The entrant's new bound: bl' for a long, bs' for a short.
    pub nb: i64,
    pub ek: [u8; 32],
    pub et: Hash,
    /// Ordinal of the part among this pair's two continuations.
    pub k: i64,
}

/// The part and remainder a transfer writes, at the least values the pair accepts, and the leaver's payout.
pub struct TransferOut {
    pub pay: i64,
    pub leaver: [u8; 32],
    pub part: Pair,
    pub part_value: i64,
    pub rest: Option<(Pair, i64)>,
}

/// P3 (`KpfPair.transfer`): the leaver is paid `owed(n, x - bl, longTip)` (long) or `owed(n, bs - x, shortTip)`.
pub fn transfer_out(p: &Pair, value: u64, t: &TransferSpec) -> TransferOut {
    let sc = p.m.scale;
    let pay = if t.side == LONG { owed(t.n, t.x - p.bl, p.long_tip, sc) } else { owed(t.n, p.bs - t.x, p.short_tip, sc) };
    hand_over_out(p, value, t, pay)
}

/// `KpfPair.handOver` (P3 and P7) for a payee owed `pay`.
pub fn hand_over_out(p: &Pair, value: u64, t: &TransferSpec, pay: i64) -> TransferOut {
    let mut part = p.rest_of(t.n);
    // The part changes hands: its evidence clock restarts at its own UTXO (DESIGN 4.5c).
    part.since = 0;
    let mut pay = pay;
    let leaver = if t.side == LONG {
        part.bl = t.nb;
        part.long_key = t.ek;
        part.tag_l = t.et;
        (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
        p.long_key
    } else {
        part.bs = t.nb;
        part.short_key = t.ek;
        part.tag_s = t.et;
        (part.short_bid, part.short_min, part.short_tip) = (0, 0, 0);
        p.short_key
    };
    let mut keep = value as i64 - p.range_of(t.n);
    if pay < PLAIN_DUST {
        keep += pay;
        pay = 0;
    }
    let pr = part.range_of(t.n);
    if t.n == p.size {
        TransferOut { pay, leaver, part_value: (pr + p.m.carrier).max(pr + keep), part, rest: None }
    } else {
        let r = p.rest_of(p.size - t.n);
        let rv = (r.range_of(r.size) + p.m.carrier).max(keep);
        TransferOut { pay, leaver, part_value: pr + p.m.carrier, part, rest: Some((r, rv)) }
    }
}

// ---------------------------------------------------------------- close-versus-close (P4)

#[derive(Clone, Debug)]
pub struct CloseSpec {
    pub n: i64,
    pub x: i64,
    /// The entry price written into P3: E(donor) <= e3 <= E(host) (DESIGN 4.5a).
    pub e3: i64,
}

pub struct CloseOut {
    pub host_pay: i64,
    pub donor_pay: i64,
    pub p3: Pair,
    pub p3_value: i64,
    pub host_rest: Option<(Pair, i64)>,
    pub donor_rest: Option<(Pair, i64)>,
}

/// P4 close-versus-close: the host's long leaves, the donor's short leaves, the host's short and the donor's long form P3.
/// `h`/`d` are the pairs with their UTXO values and covenant id of the donor (the tag of P3's long).
pub fn close_out(h: &Pair, hvalue: u64, d: &Pair, dvalue: u64, did: Hash, c: &CloseSpec) -> CloseOut {
    let sc = h.m.scale;
    let mut p3 = h.rest_of(c.n);
    p3.bl = d.bl;
    p3.entry = c.e3;
    p3.long_key = d.long_key;
    (p3.long_ask, p3.long_min, p3.long_tip) = d.long_window();
    p3.tag_l = did;
    // The re-paired output restarts its evidence clock (DESIGN 4.5c).
    p3.since = 0;
    let mut hpay = owed(c.n, c.x - h.bl, h.long_tip, sc);
    let mut hkeep = hvalue as i64 - h.range_of(c.n);
    if hpay < PLAIN_DUST {
        hkeep += hpay;
        hpay = 0;
    }
    let r3 = p3.range_of(c.n);
    let (p3_value, host_rest) = if c.n == h.size {
        ((r3 + h.m.carrier).max(r3 + hkeep), None)
    } else {
        let r = h.rest_of(h.size - c.n);
        let rv = (r.range_of(r.size) + h.m.carrier).max(hkeep);
        (r3 + h.m.carrier, Some((r, rv)))
    };
    let mut dpay = owed(c.n, d.bs - c.x, d.short_tip, sc);
    let mut dkeep = dvalue as i64 - d.range_of(c.n);
    let donor_rest = if c.n == d.size {
        dpay += dkeep;
        if dpay < PLAIN_DUST {
            dpay = 0;
        }
        None
    } else {
        if dpay < PLAIN_DUST {
            dkeep += dpay;
            dpay = 0;
        }
        let r = d.rest_of(d.size - c.n);
        let rv = (r.range_of(r.size) + d.m.carrier).max(dkeep);
        Some((r, rv))
    };
    CloseOut { host_pay: hpay, donor_pay: dpay, p3, p3_value, host_rest, donor_rest }
}

// ---------------------------------------------------------------- mutual close (P4') and idle settle (P8)

pub struct MutualOut {
    pub lp: i64,
    pub sp: i64,
    pub rest: Option<(Pair, i64)>,
}

/// T3' mutual close of n at x: both listings cross.
pub fn mutual_out(p: &Pair, value: u64, n: i64, x: i64) -> MutualOut {
    let sc = p.m.scale;
    let mut lp = owed(n, x - p.bl, p.long_tip, sc);
    let mut sp = owed(n, p.bs - x, p.short_tip, sc);
    let mut keep = value as i64 - p.range_of(n);
    if n == p.size {
        sp += keep;
        if lp < PLAIN_DUST {
            sp += lp;
            lp = 0;
        }
        if sp < PLAIN_DUST {
            lp += sp;
            sp = 0;
        }
        if lp < PLAIN_DUST {
            lp = 0;
        }
        MutualOut { lp, sp, rest: None }
    } else {
        if lp < PLAIN_DUST {
            keep += lp;
            lp = 0;
        }
        if sp < PLAIN_DUST {
            keep += sp;
            sp = 0;
        }
        let r = p.rest_of(p.size - n);
        let rv = (r.range_of(r.size) + p.m.carrier).max(keep);
        MutualOut { lp, sp, rest: Some((r, rv)) }
    }
}

/// (long payout, short payout) of P8 idle settle at the entry price clamped to [bl, bs], with the dust rule.
pub fn idle_out(p: &Pair, value: u64) -> (i64, i64) {
    let e = p.entry.clamp(p.bl, p.bs);
    let v = value as i64;
    let lp = q(p.size, e - p.bl, p.m.scale, 0);
    let sp = v - lp;
    if lp < PLAIN_DUST {
        (0, v)
    } else if sp < PLAIN_DUST {
        (v, 0)
    } else {
        (lp, sp)
    }
}

// ---------------------------------------------------------------- margin and no-downgrade helpers

/// The value P2 add margin must add: `ceil(q(s, delta))`.
pub fn margin_cost(p: &Pair, delta: i64) -> i64 {
    q(p.size, delta, p.m.scale, p.m.scale - 1)
}

/// What a leaver of the long side receives for n at x less its tip (the figure a transfer or close pays), for display.
pub fn leaver_pay(p: &Pair, side: i64, n: i64, x: i64) -> i64 {
    if side == LONG {
        owed(n, x - p.bl, p.long_tip, p.m.scale)
    } else {
        owed(n, p.bs - x, p.short_tip, p.m.scale)
    }
}

/// One row of the quote table of a pair at a price.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteRow {
    pub price: i64,
    pub long: i64,
    pub short: i64,
    pub long_liquidatable: bool,
    pub short_liquidatable: bool,
}

/// The payoff table of a pair at prices: payouts (carrier excluded) and whether each side is liquidatable at that
/// evidence quote (a long at an ask, a short at a bid of that price).
pub fn quote_table(p: &Pair, prices: &[i64]) -> Vec<QuoteRow> {
    prices
        .iter()
        .map(|&price| QuoteRow {
            price,
            long: p.long_at(price),
            short: p.short_at(price),
            long_liquidatable: price >= 0 && long_liquidatable(p, price),
            short_liquidatable: short_liquidatable(p, price),
        })
        .collect()
}

/// The winner's payout of P7 before the dust rule (DESIGN 4.5b): the entrant's payment `q(n, x - bl)` (long loser) or
/// `q(n, bs - x)` (short loser) less the fee at the evidence `quote`, never below 0.
pub fn p7_pay(p: &Pair, n: i64, side: i64, x: i64, quote: i64) -> i64 {
    let o = if side == LONG { owed(n, x - p.bl, 0, p.m.scale) } else { owed(n, p.bs - x, 0, p.m.scale) };
    (o - liq_fee(p, n, quote, side)).max(0)
}
