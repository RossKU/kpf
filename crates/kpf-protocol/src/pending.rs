//! The two-step evidence mechanism of DESIGN 4.5d as pure functions: what `KpfPair.propose` writes into a `KpfPending`,
//! the pair states the pending part may become, and what `challenge`, `cure` and `finalize` must pay.
//!
//! Every function mirrors the contracts (named in each doc) and is held to them by the engine tests: a builder that
//! pays each party exactly the least value its covenant accepts is accepted, one sompi less is refused.

use crate::payoff::{fee_of, q, slice_of};
use crate::state::{Market, Pair, Pending, BPS, KIND_CLOSE, KIND_LIQ, LONG, PLAIN_DUST, SHORT};

/// `KpfPair.propose`: the bond of a part `n`, `max(bondMin, 0.02 KAS, q(n, floor((bs - bl) * bondBps / 10^4)))`.
pub fn bond_of(p: &Pair, n: i64) -> i64 {
    let r = ((p.bs - p.bl) as i128 * p.m.bond_bps as i128 / BPS as i128) as i64;
    q(n, r, p.m.scale, 0).max(p.m.bond_min).max(PLAIN_DUST)
}

/// The refund of a proposal: the bond, plus the carrier the proposer funds when the part is smaller than the pair.
pub fn refund_of(p: &Pair, n: i64) -> i64 {
    bond_of(p, n) + if n < p.size { p.m.carrier } else { 0 }
}

/// The liquidation fee of a claim (DESIGN 4.5b, unchanged): the fee at the quote capped at the maintenance slice.
pub fn claim_fee(p: &Pair, n: i64, side: i64, quote: i64) -> i64 {
    fee_of(&p.m, n, quote).min(slice_of(p, n, side))
}

/// The farthest quote within the contradiction margin g of a liquidation claim at `quote` (DESIGN 4.5e): for a long
/// loser (ask evidence) the lowest ask with `a * 10^4 >= quote * (10^4 - g)`, `ceil(quote * (10^4 - g) / 10^4)`; for a
/// short loser the highest bid with `b * 10^4 <= quote * (10^4 + g)`, `floor(quote * (10^4 + g) / 10^4)`.
pub fn cure_reach_of(m: &Market, side: i64, quote: i64) -> i64 {
    let (qq, g, b) = (quote as i128, m.g_bps as i128, BPS as i128);
    if side == LONG {
        ((qq * (b - g) + b - 1) / b) as i64
    } else {
        (qq * (b + g) / b) as i64
    }
}

/// The cure bound of a liquidation claim at `quote` (`KpfPair.propose`, DESIGN 4.5e): the nearest bound at which no
/// quote within g of the claim triggers, with `pg` = [`cure_reach_of`]: a long loser's `floor(pg * (10^4 - mmrBps) /
/// 10^4)`, a short loser's `ceil(pg * (10^4 + mmrBps) / 10^4)`; `None` when the short's exceeds priceMax (no cure).
pub fn cure_bound_of(m: &Market, side: i64, quote: i64) -> Option<i64> {
    let (pg, mm, b) = (cure_reach_of(m, side, quote) as i128, m.mmr_bps as i128, BPS as i128);
    if side == LONG {
        Some((pg * (b - mm) / b) as i64)
    } else {
        let c = (pg * (b + mm) + b - 1) / b;
        (c <= m.price_max as i128).then_some(c as i64)
    }
}

/// The forfeit bound (`KpfPair.propose`): the loser's bound moved inward to the nearest bound at which the narrowed
/// range plus the fee still fits the part, `q(n, narrowed) + fee <= q(n, bs - bl)`.
pub fn forfeit_bound_of(p: &Pair, n: i64, side: i64, fee: i64) -> i64 {
    let sc = p.m.scale;
    let part = q(n, p.bs - p.bl, sc, 0);
    let fits = |w: i64| q(n, w, sc, 0) + fee <= part;
    // The widest range w in (0, bs - bl] that fits; the forfeit range is monotone in w.
    let (mut lo, mut hi) = (1i64, p.bs - p.bl);
    if fits(hi) {
        lo = hi;
    } else {
        while lo < hi {
            let mid = lo + (hi - lo + 1) / 2;
            if fits(mid) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
    }
    if side == LONG {
        p.bs - lo
    } else {
        p.bl + lo
    }
}

/// The part `n` of `p` with both tags 0 and both sides unlisted: what every return state starts from (DESIGN 4.5e).
fn unlisted_part(p: &Pair, n: i64) -> Pair {
    let mut f = p.rest_of(n);
    (f.long_ask, f.long_min, f.long_tip, f.short_bid, f.short_min, f.short_tip) = (0, 0, 0, 0, 0, 0);
    f
}

/// The restore state (`KpfPair.propose`): the part `n` of `p` with its keys, bounds and entry unchanged, tags 0, both
/// sides unlisted (DESIGN 4.5e) and `since = S` (`p` clocked, i.e. [`Pair::clocked`]): a refuted claim neither restarts
/// nor moves the pair's clock (DESIGN 4.5d), and a listing live at the proposal is not live again without its owner.
pub fn restore_state(p: &Pair, n: i64) -> Pair {
    unlisted_part(p, n)
}

/// The cure state: the part with the loser's bound moved to `cb`, both sides unlisted and `since = S` (`s` the pair's
/// clock start).
pub fn cure_state(p: &Pair, n: i64, side: i64, cb: i64, s: i64) -> Pair {
    let mut c = unlisted_part(p, n);
    if side == LONG {
        c.bl = cb;
    } else {
        c.bs = cb;
    }
    c.since = s;
    c
}

/// The forfeit state: the part with both keys the winner's, both sides unlisted, `since = 0` and the loser's bound at
/// `fb`.
pub fn forfeit_state(p: &Pair, n: i64, side: i64, fb: i64) -> Pair {
    let mut f = unlisted_part(p, n);
    let w = if side == LONG { p.short_key } else { p.long_key };
    f.long_key = w;
    f.short_key = w;
    if side == LONG {
        f.bl = fb;
    } else {
        f.bs = fb;
    }
    f.since = 0;
    f
}

/// The P2SH script hash of a pair state (what `KpfPending` pays to with `new ScriptPubKeyP2SH(hash)`).
pub fn script_hash(p: &Pair) -> [u8; 32] {
    let spk = p.spk();
    let s = spk.script();
    s[2..34].try_into().expect("P2SH script: OpBlake2b, OpData32, 32-byte hash, OpEqual")
}

/// The pending state `KpfPair.propose` writes for a claim on the part `n` of `p` (the pair with `since` = its clock
/// start S, i.e. [`Pair::clocked`]). `kind` 1 close (side = the closer; the proposer is the closer), 2 liquidation
/// (side = the loser). The claim has `visibleUntil` 0 and no offer, so it can never be guaranteed (DESIGN 4.5f): a
/// builder of a real proposal uses [`pending_with`] with the evidence order's [`crate::kob::Evidence::live_until`].
pub fn pending_of(p: &Pair, n: i64, kind: i64, side: i64, quote: i64, proposer: [u8; 32]) -> Pending {
    pending_with(p, n, kind, side, quote, proposer, 0, 0)
}

/// [`pending_of`] with the two fields of guaranteed fast settlement `KpfPair.propose` writes (DESIGN 4.5f): the last DAA
/// at which the evidence order is fillable by conforming matchers, and the proposer's offer to a guarantor.
#[allow(clippy::too_many_arguments)]
pub fn pending_with(p: &Pair, n: i64, kind: i64, side: i64, quote: i64, proposer: [u8; 32], visible_until: i64, offer: i64) -> Pending {
    let s = p.since;
    let restore = restore_state(p, n);
    let (mut cure_bound, mut cure_hash, mut forfeit_hash, mut fee) = (0, [0u8; 32], [0u8; 32], 0);
    let proposer_key = if kind == KIND_CLOSE {
        if side == LONG {
            p.long_key
        } else {
            p.short_key
        }
    } else {
        proposer
    };
    if kind == KIND_LIQ {
        fee = claim_fee(p, n, side, quote);
        let fb = forfeit_bound_of(p, n, side, fee);
        forfeit_hash = script_hash(&forfeit_state(p, n, side, fb));
        if let Some(cb) = cure_bound_of(&p.m, side, quote) {
            cure_bound = cb;
            cure_hash = script_hash(&cure_state(p, n, side, cb, s));
        }
    }
    let bond = bond_of(p, n);
    Pending {
        m: p.m.clone(),
        kind,
        side,
        n,
        bl: p.bl,
        bs: p.bs,
        quote,
        cure_bound,
        fee,
        bond,
        refund: refund_of(p, n),
        visible_until,
        offer,
        entry: p.entry,
        long_key: p.long_key,
        short_key: p.short_key,
        long_ask: p.long_ask,
        long_min: p.long_min,
        long_tip: p.long_tip,
        short_bid: p.short_bid,
        short_min: p.short_min,
        short_tip: p.short_tip,
        since: s,
        proposer_key,
        restore_hash: script_hash(&restore),
        cure_hash,
        forfeit_hash,
    }
}

/// What a proposal writes: the pending part with its least value and, when `n < s`, the remainder with its least value.
#[derive(Clone, Debug)]
pub struct ProposeOut {
    pub pending: Pending,
    pub pending_value: i64,
    pub rest: Option<(Pair, i64)>,
    /// The forfeit bound argument `fb` of `KpfPair.propose` (the loser's own bound for a close).
    pub fb: i64,
}

/// `KpfPair.propose` on the part `n` of `p` (clocked) in a UTXO of `value` sompi, on an evidence order fillable by
/// conforming matchers until `visible_until`, with the proposer's guarantee offer `offer`.
#[allow(clippy::too_many_arguments)]
pub fn propose_out(p: &Pair, value: u64, n: i64, kind: i64, side: i64, quote: i64, proposer: [u8; 32], visible_until: i64, offer: i64) -> ProposeOut {
    let w = pending_with(p, n, kind, side, quote, proposer, visible_until, offer);
    let v = value as i64;
    let part = p.range_of(n);
    let fb = if kind == KIND_LIQ { forfeit_bound_of(p, n, side, w.fee) } else if side == LONG { p.bl } else { p.bs };
    if n == p.size {
        ProposeOut { pending_value: v + w.bond, pending: w, rest: None, fb }
    } else {
        let r = p.rest_of(p.size - n);
        let rv = (r.range_of(r.size) + p.m.carrier).max(v - part);
        ProposeOut { pending_value: part + w.refund, pending: w, rest: Some((r, rv)), fb }
    }
}

impl Pending {
    /// The pair this part was cut from, rebuilt from the self-describing state (size n, tags 0, `since` = S).
    pub fn pair_view(&self) -> Pair {
        Pair {
            m: self.m.clone(),
            size: self.n,
            bl: self.bl,
            bs: self.bs,
            entry: self.entry,
            long_key: self.long_key,
            short_key: self.short_key,
            long_ask: self.long_ask,
            long_min: self.long_min,
            long_tip: self.long_tip,
            short_bid: self.short_bid,
            short_min: self.short_min,
            short_tip: self.short_tip,
            tag_l: Default::default(),
            tag_s: Default::default(),
            since: self.since,
        }
    }

    /// The restore state, checked against `restoreHash`.
    pub fn restore_pair(&self) -> Option<Pair> {
        let r = restore_state(&self.pair_view(), self.n);
        (script_hash(&r) == self.restore_hash).then_some(r)
    }

    /// The cure state, checked against `cureHash` (None: no cure, or the state does not match).
    pub fn cure_pair(&self) -> Option<Pair> {
        if !self.can_cure() {
            return None;
        }
        let c = cure_state(&self.pair_view(), self.n, self.side, self.cure_bound, self.since);
        (script_hash(&c) == self.cure_hash).then_some(c)
    }

    /// The forfeit state, checked against `forfeitHash`.
    pub fn forfeit_pair(&self) -> Option<Pair> {
        if self.kind != KIND_LIQ {
            return None;
        }
        let v = self.pair_view();
        let f = forfeit_state(&v, self.n, self.side, forfeit_bound_of(&v, self.n, self.side, self.fee));
        (script_hash(&f) == self.forfeit_hash).then_some(f)
    }

    /// Whether the three pair states rebuilt from this state match its hashes (a genuine proposal of the pinned pair
    /// template always does; a look-alike may not).
    pub fn hashes_match(&self) -> bool {
        self.restore_pair().is_some()
            && (self.kind == KIND_CLOSE || self.forfeit_pair().is_some())
            && (!self.can_cure() || self.cure_pair().is_some())
    }

    /// The counter-evidence a challenge needs: `true` an ask, `false` a bid.
    pub fn counter_is_ask(&self) -> bool {
        if self.kind == KIND_CLOSE {
            self.side == LONG
        } else {
            self.side == SHORT
        }
    }

    /// Whether a counter quote `c` contradicts the claim by the margin g (`KpfPending.challenge`).
    pub fn contradicts(&self, c: i64) -> bool {
        if c < 0 || c > self.m.price_max {
            return false;
        }
        let (b, g, mm) = (BPS as i128, self.m.g_bps as i128, self.m.mmr_bps as i128);
        let clamp = |x: i64| x.clamp(self.bl, self.bs) as i128;
        if self.kind == KIND_CLOSE {
            let (cc, cq) = (clamp(c), clamp(self.quote));
            if self.side == LONG {
                cc * b <= cq * (b - g)
            } else {
                cc * b >= cq * (b + g)
            }
        } else if self.side == LONG {
            c as i128 * (b - mm) >= self.bl as i128 * (b + g)
        } else {
            c as i128 * (b + mm) <= self.bs as i128 * (b - g)
        }
    }

    /// The counter quote at which a challenge starts to contradict: the highest ask (kind 1 long, kind 2 short) or the
    /// lowest bid (otherwise) that still does. `None` when no quote in `[0, priceMax]` does.
    pub fn contradiction_bound(&self) -> Option<i64> {
        let (mut lo, mut hi) = (0i64, self.m.price_max);
        if self.counter_is_ask() {
            if !self.contradicts(0) {
                return None;
            }
            while lo < hi {
                let mid = lo + (hi - lo + 1) / 2;
                if self.contradicts(mid) {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            Some(lo)
        } else {
            if !self.contradicts(hi) {
                return None;
            }
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if self.contradicts(mid) {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            Some(lo)
        }
    }

    /// The least value every genuine pending part of this claim holds: its range `q(n, bs - bl)` and the refund
    /// (`KpfPair.propose`; review 4, F4).
    pub fn least_value(&self) -> i64 {
        q(self.n, self.bs - self.bl, self.m.scale, 0).saturating_add(self.refund)
    }

    /// What a cure costs its funder in all, besides the network fee: the refund to the proposer and the cured part's
    /// value less this UTXO's value (the added margin, and the cured part's carrier when the curer funds it).
    pub fn cure_total(&self, value: u64) -> i64 {
        self.refund + self.cure_value(value) - value as i64
    }

    /// The least value of the restored pair after a challenge: this UTXO's value less the bond.
    pub fn restore_value(&self, value: u64) -> i64 {
        value as i64 - self.bond
    }

    /// The value the loser moves to cure (`KpfPending.cure`): ceil(q(n, moved distance)).
    pub fn cure_add(&self) -> i64 {
        let sc = self.m.scale;
        if self.side == LONG {
            q(self.n, self.bl - self.cure_bound, sc, sc - 1)
        } else {
            q(self.n, self.cure_bound - self.bs, sc, sc - 1)
        }
    }

    /// The least value of the cured pair: its range floor, and this UTXO's value less the refund plus [`Self::cure_add`].
    pub fn cure_value(&self, value: u64) -> i64 {
        let (b0, b1) = if self.side == LONG { (self.cure_bound, self.bs) } else { (self.bl, self.cure_bound) };
        let floor = q(self.n, b1 - b0, self.m.scale, 0) + self.m.carrier;
        floor.max(value as i64 - self.refund + self.cure_add())
    }

    /// Finalize of a close (kind 1): (long payout, short payout) with the dust rule (`KpfPending.finalize`).
    pub fn finalize_close(&self, value: u64) -> (i64, i64) {
        let c = self.quote.clamp(self.bl, self.bs);
        let sc = self.m.scale;
        let owe = if self.side == LONG { q(self.n, c - self.bl, sc, 0) } else { q(self.n, self.bs - c, sc, 0) };
        let v = value as i64;
        let mut me = owe + self.refund;
        let mut other = v - me;
        if other < PLAIN_DUST {
            me = v;
            other = 0;
        }
        if self.side == LONG {
            (me, other)
        } else {
            (other, me)
        }
    }

    /// Finalize of a liquidation (kind 2): (least value of the forfeited pair, the proposer's payout).
    pub fn finalize_forfeit(&self, value: u64) -> (i64, i64) {
        (value as i64 - self.bond - self.fee, self.bond + self.fee)
    }

    /// The first DAA score at which this UTXO (of DAA score `utxo_daa`) can be finalized.
    pub fn finalize_from(&self, utxo_daa: u64) -> u64 {
        utxo_daa.saturating_add(self.m.challenge_daa.max(0) as u64)
    }

    /// The winner's key of a liquidation claim, the non-actor's of a close.
    pub fn winner_key(&self) -> [u8; 32] {
        if self.side == LONG {
            self.short_key
        } else {
            self.long_key
        }
    }
}
