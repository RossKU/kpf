//! Guaranteed fast settlement (DESIGN 4.5f) as pure functions: what `KpfPending.guarantee` requires of the lock and pays,
//! the lock state it writes, and what `KpfLock.dispute` (the whole cover to the wronged party, DESIGN 4.5g) and
//! `KpfLock.release` pay.
//!
//! Every function mirrors the contracts (named in each doc) and is held to them by the engine tests
//! (`kpf_guarantee_tests`): a builder that pays each party exactly the least value its covenant accepts is accepted, one
//! sompi less is refused.

use kaspa_consensus_core::Hash;

use crate::payoff::q;
use crate::state::{Lock, Pending, BPS, KIND_CLOSE, LONG, PLAIN_DUST, SHORT, STATUS_LOCK};

/// A lock holds at least 0.1 KAS (`KpfPending.LOCK_FLOOR`): above the 0.08 KAS covenant-output minimum, and enough for
/// the dust rules of `KpfLock.dispute` whatever the cover.
pub const LOCK_FLOOR: i64 = 10_000_000;

/// The script public key of the burn output: P2SH of the one-byte redeem script `OP_RETURN` (0x6a), which no spend
/// can satisfy (experiment Z7).
pub fn burn_spk() -> kaspa_consensus_core::tx::ScriptPublicKey {
    kaspa_txscript::pay_to_script_hash_script(&[0x6a])
}

/// The hash of [`burn_spk`] (the constant `BURN` of `KpfLock`).
pub fn burn_hash() -> [u8; 32] {
    burn_spk().script()[2..34].try_into().expect("P2SH script: OpBlake2b, OpData32, 32-byte hash, OpEqual")
}

/// `ceil(v * bps / 10^4)` in the split form of the contracts (`KpfPending.guarantee`), exact for `v` up to the supply.
pub fn ratio_up(v: i64, bps: i64) -> i64 {
    (v / BPS) * bps + ((v % BPS) * bps + BPS - 1) / BPS
}

/// `floor(v * bps / 10^4)` in the split form (`KpfLock.dispute`'s reward).
pub fn ratio_down(v: i64, bps: i64) -> i64 {
    (v / BPS) * bps + ((v % BPS) * bps) / BPS
}

impl Pending {
    /// The cover of a guarantee (`KpfPending.settle`): the value the fast settlement moves away from the party the
    /// claim could wrong. A close: what the closer is paid beyond its refund (its payoff at the clamped quote, or the
    /// whole part when the other party's payout falls below dust); a liquidation: the part's range `q(n, bs - bl)`.
    pub fn guarantee_cover(&self, value: u64) -> i64 {
        if self.kind == KIND_CLOSE {
            let (lp, sp) = self.finalize_close(value);
            (if self.side == LONG { lp } else { sp }) - self.refund
        } else {
            q(self.n, self.bs - self.bl, self.m.scale, 0)
        }
    }

    /// The least value of the lock (`KpfPending.guarantee`): `ceil(cover * lockBps / 10^4) + offer`, at least
    /// [`LOCK_FLOOR`].
    pub fn lock_min(&self, value: u64) -> i64 {
        (ratio_up(self.guarantee_cover(value), self.m.lock_bps) + self.offer).max(LOCK_FLOOR)
    }

    /// A guarantee of a close: (long payout, short payout), the closer's less the offer (`KpfPending.settle`).
    pub fn guarantee_close(&self, value: u64) -> (i64, i64) {
        let (lp, sp) = self.finalize_close(value);
        if self.side == LONG {
            (lp - self.offer, sp)
        } else {
            (lp, sp - self.offer)
        }
    }

    /// A guarantee of a liquidation: (least value of the forfeited pair, the proposer's payout less the offer).
    pub fn guarantee_forfeit(&self, value: u64) -> (i64, i64) {
        (value as i64 - self.bond - self.fee, self.bond + self.fee - self.offer)
    }

    /// The first DAA score at which a pending part of DAA score `utxo_daa` can be guaranteed: at once for a close, after
    /// the cure grace for a liquidation (CSV).
    pub fn guarantee_from(&self, utxo_daa: u64) -> u64 {
        if self.kind == KIND_CLOSE {
            utxo_daa
        } else {
            utxo_daa.saturating_add(self.m.grace_daa.max(0) as u64)
        }
    }

    /// Whether a pending part of DAA score `utxo_daa` can still be guaranteed: it was accepted while its proposal's
    /// evidence was fillable by conforming matchers (`P <= visibleUntil`).
    pub fn guaranteeable_at(&self, utxo_daa: u64) -> bool {
        (utxo_daa as i128) <= self.visible_until as i128
    }

    /// The lock a guarantee of this part (covenant id `claim_id`, DAA score `utxo_daa`, value `value`) by `owner` writes.
    pub fn lock_of(&self, owner: [u8; 32], claim_id: Hash, utxo_daa: u64, value: u64) -> Lock {
        Lock {
            owner,
            status: STATUS_LOCK,
            m: self.m.clone(),
            claim_id,
            kind: self.kind,
            side: self.side,
            n: self.n,
            bl: self.bl,
            bs: self.bs,
            quote: self.quote,
            clock: utxo_daa as i64,
            cover: self.guarantee_cover(value),
            victim: self.defender_key(),
        }
    }
}

impl Lock {
    /// The counter-evidence a dispute needs: `true` an ask, `false` a bid (as `KpfPending.challenge`).
    pub fn counter_is_ask(&self) -> bool {
        if self.kind == KIND_CLOSE {
            self.side == LONG
        } else {
            self.side == SHORT
        }
    }

    fn clamp(&self, x: i64) -> i64 {
        x.clamp(self.bl, self.bs)
    }

    /// Whether a counter quote `c` contradicts the claim by the margin g (`KpfLock.dispute`, as `KpfPending.challenge`).
    pub fn contradicts(&self, c: i64) -> bool {
        if c < 0 || c > self.m.price_max {
            return false;
        }
        let (b, g, mm) = (BPS as i128, self.m.g_bps as i128, self.m.mmr_bps as i128);
        if self.kind == KIND_CLOSE {
            let (cc, cq) = (self.clamp(c) as i128, self.clamp(self.quote) as i128);
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

    /// The counter quote at which a dispute starts to contradict: the highest ask or the lowest bid that still does.
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

    /// What the fast settlement took from the wronged party if the true price is `c`: what its side would have been
    /// worth at `c` less what the settlement paid it. A close: the closer's payoff at the claim's quote less its payoff
    /// at `c(c)` (the cover less the latter); a liquidation: the loser's payoff at `c(c)`. Never above the cover; 0 when
    /// the claim was right or in the wronged party's favour at `c`. (Before review 5 this was the restitution, priced at
    /// the disputing order's quote; DESIGN 4.5g pays the cover instead, which is never less.)
    pub fn taken_at(&self, c: i64) -> i64 {
        let cc = self.clamp(c);
        let sc = self.m.scale;
        let h = if self.kind == KIND_CLOSE {
            self.cover - if self.side == LONG { q(self.n, cc - self.bl, sc, 0) } else { q(self.n, self.bs - cc, sc, 0) }
        } else if self.side == LONG {
            q(self.n, cc - self.bl, sc, 0)
        } else {
            q(self.n, self.bs - cc, sc, 0)
        };
        h.max(0)
    }

    /// The restitution H of any successful dispute (`KpfLock.dispute`, DESIGN 4.5g), before the dust rule: the whole
    /// cover, whatever the counter order and whoever disputes. It is never below [`Self::taken_at`] at any price.
    pub fn restitution(&self) -> i64 {
        self.cover.max(0)
    }

    /// What the victim is paid by a dispute: [`Self::restitution`], raised to the plain dust bound when it is positive
    /// and below it.
    pub fn victim_pay(&self) -> i64 {
        let h = self.restitution();
        if h > 0 && h < PLAIN_DUST {
            PLAIN_DUST
        } else {
            h
        }
    }

    /// The reward of a dispute's executor: `floor(cover * (lockBps - 10^4) / 10^4)`.
    pub fn reward(&self) -> i64 {
        ratio_down(self.cover, self.m.lock_bps - BPS)
    }

    /// The least burn of a dispute of a lock of `value` (0: no burn output, it is below dust): the lock less the
    /// restitution and the reward, i.e. the offer and the rounding of the lock ratio for a lock of the least value.
    pub fn burn(&self, value: u64) -> i64 {
        let b = value as i64 - self.victim_pay() - self.reward();
        if b >= PLAIN_DUST {
            b
        } else {
            0
        }
    }

    /// The last rest end a dispute's counter-evidence may have, for a lock of DAA score `lock_daa` (F):
    /// `F + disputeDaa - minRestDaa`.
    pub fn rest_deadline(&self, lock_daa: u64) -> i64 {
        lock_daa as i64 + self.m.dispute_daa - self.m.min_rest_daa
    }

    /// The first DAA score at which a lock of DAA score `lock_daa` can be released (CSV `disputeDaa`).
    pub fn release_from(&self, lock_daa: u64) -> u64 {
        lock_daa.saturating_add(self.m.dispute_daa.max(0) as u64)
    }

    /// Whether this lock is the one a guarantee of the pending part `w` (covenant id `id`, DAA score `daa`, value
    /// `value`) writes for its owner.
    pub fn is_lock_of(&self, w: &Pending, id: Hash, daa: u64, value: u64) -> bool {
        *self == w.lock_of(self.owner, id, daa, value)
    }
}
