//! The cross-margin account (`KpfCrossC`) and its pending claim (`KpfPendingX`): docs/DESIGN-COMMITTEE.md 3b. One UTXO
//! between two parties, A (the account's owner) and B (its counterparty, an executor), with up to two legs (positions of
//! A in two markets) and one margin: A is owed `clamp(aMargin + pnl_1 + pnl_2, 0, pot)`, so a gain of one leg pays for a
//! loss of the other as it happens and the account is liquidated on its total. This module is the model the contracts
//! are tested against: every amount a contract checks is computed here with the same integer arithmetic.

use kaspa_consensus_core::tx::TransactionOutpoint;
use silverscript_abi::ArtifactValue;

use crate::artifacts::Template;
use crate::cmarket::{FEE_MIN_MAX, TERM_MAX, TERM_MIN, WAIT_MAX};
use crate::committee::blake2b;
use crate::error::{Error, Result};
use crate::oracle::maker_of;
use crate::payoff::q;
use crate::state::{encode, to_args, Cursor, F, BPS, KIND_CLOSE, KIND_LIQ, LOCK_TIME_THRESHOLD, LONG, MMR_CAP, PLAIN_DUST, PRICE_CAP, SCALE_MAX, SHORT};

/// The account's owner.
pub const SIDE_A: i64 = 1;
/// Its counterparty.
pub const SIDE_B: i64 = 2;
pub const CROSS_STATE_LEN: usize = 405;
/// `KpfPendingX`'s state: seven ints, six 32-byte fields, and the account's state behind a 3-byte push header.
pub const XPENDING_STATE_LEN: usize = 7 * 9 + 6 * 33 + 3 + CROSS_STATE_LEN;
/// The most a margin may be (`INT_MAX / 8`), so that no sum of the account overflows.
pub const MARGIN_MAX: i64 = i64::MAX / 8;

fn refuse<T>(why: &str) -> Result<T> {
    Err(Error::Invalid(why.into()))
}

/// The terms two parties agree for an account (the first ten fields of its state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XTerms {
    pub term: i64,
    pub mmr_bps: i64,
    pub g_bps: i64,
    pub liq_fee_bps: i64,
    pub keeper_fee_min: i64,
    pub carrier: i64,
    pub idle_daa: i64,
    pub grace_daa: i64,
    pub late_daa: i64,
    pub quiet_daa: i64,
}

/// One leg: a position of A in one market. `dir` is [`LONG`] (A gains when the price rises) or [`SHORT`]; `size` 0 is
/// no leg (the second leg of an account with one position).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XLeg {
    pub market: [u8; 32],
    pub committee: [u8; 32],
    pub scale: i64,
    pub price_max: i64,
    pub size: i64,
    pub dir: i64,
    pub entry: i64,
}

impl XLeg {
    /// No leg: what the second leg of a one-position account is written as.
    pub fn none() -> XLeg {
        XLeg { market: [0; 32], committee: [0; 32], scale: 1, price_max: 1, size: 0, dir: LONG, entry: 0 }
    }
    fn fields(&self) -> [F; 7] {
        [F::B(self.market), F::B(self.committee), F::I(self.scale), F::I(self.price_max), F::I(self.size), F::I(self.dir), F::I(self.entry)]
    }
    fn read(c: &mut Cursor) -> Result<XLeg> {
        Ok(XLeg { market: c.b32()?, committee: c.b32()?, scale: c.int()?, price_max: c.int()?, size: c.int()?, dir: c.int()?, entry: c.int()? })
    }
    /// What the leg is worth at a price: `floor(size x / scale)`.
    pub fn worth(&self, x: i64) -> i64 {
        q(self.size, x, self.scale, 0)
    }
    /// The leg's gain for A at a price, from its entry.
    pub fn gain(&self, x: i64) -> i64 {
        let d = self.worth(x) - self.worth(self.entry);
        if self.dir == LONG {
            d
        } else {
            -d
        }
    }
    fn check(&self) -> Result<()> {
        if !(self.scale > 0 && self.scale <= SCALE_MAX && self.price_max > 0 && self.price_max <= PRICE_CAP) {
            return refuse("a leg's scale or price limit is out of range");
        }
        if self.size < 0 || self.size / self.scale > (i64::MAX / 8) / self.price_max - 1 {
            return refuse("a leg's size is out of range");
        }
        if (self.dir != LONG && self.dir != SHORT) || self.entry < 0 || self.entry > self.price_max {
            return refuse("a leg's direction or entry is out of range");
        }
        Ok(())
    }
}

/// The build constants of `KpfCrossC`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XConsts {
    pub pendx_tpl: [u8; 32],
    pub epoch_table_tpl: [u8; 32],
    pub epoch_table_pre: i64,
    pub epoch_table_suf: i64,
    pub attest_tag: [u8; 4],
}

impl XConsts {
    pub fn cross_args(&self) -> Vec<ArtifactValue> {
        vec![
            ArtifactValue::Bytes(self.pendx_tpl.to_vec()),
            ArtifactValue::Bytes(self.epoch_table_tpl.to_vec()),
            ArtifactValue::Int(self.epoch_table_pre),
            ArtifactValue::Int(self.epoch_table_suf),
            ArtifactValue::Bytes(self.attest_tag.to_vec()),
        ]
    }
}

/// The templates of the account and its pending claim.
#[derive(Clone, Debug)]
pub struct XKit {
    pub cross: Template,
    pub pending: Template,
    pub consts: XConsts,
}

/// A cross-margin account (`KpfCrossC`'s state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cross {
    pub t: XTerms,
    pub legs: [XLeg; 2],
    pub a_key: [u8; 32],
    pub b_key: [u8; 32],
    pub a_margin: i64,
    pub b_margin: i64,
    pub expiry: i64,
}

/// The prices of the two legs as a claim counts them, each moved by g toward A's gain (`Some(true)`), against A
/// (`Some(false)`) or not at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XView {
    /// `aMargin + gain_1 + gain_2`, before the clamp.
    pub raw: i64,
    /// The notional `q_1 + q_2` at the prices used.
    pub notional: i64,
}

impl Cross {
    fn fields(&self) -> Vec<F> {
        let t = &self.t;
        let mut f: Vec<F> = [t.term, t.mmr_bps, t.g_bps, t.liq_fee_bps, t.keeper_fee_min, t.carrier, t.idle_daa, t.grace_daa, t.late_daa, t.quiet_daa].map(F::I).into_iter().collect();
        f.extend(self.legs[0].fields());
        f.extend(self.legs[1].fields());
        f.extend([F::B(self.a_key), F::B(self.b_key), F::I(self.a_margin), F::I(self.b_margin), F::I(self.expiry)]);
        f
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    pub fn ctor_with(&self, c: &XConsts) -> Vec<ArtifactValue> {
        let mut a = c.cross_args();
        a.extend(to_args(&self.fields()));
        a
    }
    pub fn redeem(&self, k: &XKit) -> Vec<u8> {
        k.cross.redeem(&self.state_bytes())
    }
    /// The P2SH script hash of this state (what a pending claim restores or cures to).
    pub fn script_hash(&self, k: &XKit) -> [u8; 32] {
        blake2b(&self.redeem(k))
    }
    pub fn decode_state(b: &[u8]) -> Result<Cross> {
        let mut c = Cursor { b, at: 0 };
        let t = XTerms {
            term: c.int()?,
            mmr_bps: c.int()?,
            g_bps: c.int()?,
            liq_fee_bps: c.int()?,
            keeper_fee_min: c.int()?,
            carrier: c.int()?,
            idle_daa: c.int()?,
            grace_daa: c.int()?,
            late_daa: c.int()?,
            quiet_daa: c.int()?,
        };
        let legs = [XLeg::read(&mut c)?, XLeg::read(&mut c)?];
        let x = Cross { t, legs, a_key: c.b32()?, b_key: c.b32()?, a_margin: c.int()?, b_margin: c.int()?, expiry: c.int()? };
        c.done()?;
        Ok(x)
    }
    /// `KpfCrossC.checkCross`: what every entry requires of the state.
    pub fn check(&self) -> Result<()> {
        let t = &self.t;
        let ok = t.term >= TERM_MIN
            && t.term <= TERM_MAX
            && t.mmr_bps >= 0
            && t.mmr_bps < MMR_CAP
            && t.g_bps > 0
            && t.g_bps < BPS
            && t.liq_fee_bps >= 0
            && t.liq_fee_bps <= BPS
            && (0..=FEE_MIN_MAX).contains(&t.keeper_fee_min)
            && (0..=FEE_MIN_MAX).contains(&t.carrier)
            && (0..=TERM_MAX).contains(&t.idle_daa)
            && (0..=WAIT_MAX).contains(&t.grace_daa)
            && t.late_daa > 0
            && t.late_daa <= WAIT_MAX
            && (0..=WAIT_MAX).contains(&t.quiet_daa);
        if !ok {
            return refuse("the account's terms are out of range");
        }
        self.legs[0].check()?;
        self.legs[1].check()?;
        if self.legs[0].size <= 0 {
            return refuse("the first leg is empty");
        }
        if !(0..=MARGIN_MAX).contains(&self.a_margin) || !(0..=MARGIN_MAX).contains(&self.b_margin) {
            return refuse("a margin is out of range");
        }
        Ok(())
    }
    /// What the account holds for its parties: `aMargin + bMargin`.
    pub fn pot(&self) -> i64 {
        self.a_margin + self.b_margin
    }
    /// The least value of the account's UTXO.
    pub fn floor(&self) -> i64 {
        self.pot() + self.t.carrier
    }
    /// A price as a claim counts it: at most the leg's price limit.
    pub fn counted(&self, p: [i64; 2]) -> [i64; 2] {
        [p[0].min(self.legs[0].price_max), p[1].min(self.legs[1].price_max)]
    }
    /// The counted price of leg `i` moved by g toward A's gain or against it (`KpfCrossC.moved`): a rise is
    /// `ceil(p (10^4 + g) / 10^4)`, at most the price limit; a fall `floor(p (10^4 - g) / 10^4)`.
    pub fn moved(&self, i: usize, p: i64, toward_a: bool) -> i64 {
        let (l, b, g) = (&self.legs[i], BPS as i128, self.t.g_bps as i128);
        if (l.dir == LONG) == toward_a {
            (((p as i128) * (b + g) + b - 1) / b).min(l.price_max as i128) as i64
        } else {
            ((p as i128) * (b - g) / b) as i64
        }
    }
    /// The account at the counted prices `p`, moved by g toward A's gain, against A, or as they are.
    pub fn view(&self, p: [i64; 2], toward_a: Option<bool>) -> XView {
        let p = self.counted(p);
        let x: Vec<i64> = (0..2).map(|i| toward_a.map_or(p[i], |f| self.moved(i, p[i], f))).collect();
        XView { raw: self.a_margin + self.legs[0].gain(x[0]) + self.legs[1].gain(x[1]), notional: self.legs[0].worth(x[0]) + self.legs[1].worth(x[1]) }
    }
    /// The maintenance of a notional: `ceil(n mmr / 10^4)`.
    pub fn maint(&self, n: i64) -> i64 {
        (n / BPS) * self.t.mmr_bps + ((n % BPS) * self.t.mmr_bps + BPS - 1) / BPS
    }
    /// What A is owed at the prices `p` as they are: `clamp(raw, 0, pot)`. B is owed the rest of the pot.
    pub fn a_at(&self, p: [i64; 2]) -> i64 {
        self.view(p, None).raw.clamp(0, self.pot())
    }
    /// What `side` is owed at the prices `p` as they are.
    pub fn owed_at(&self, side: i64, p: [i64; 2]) -> i64 {
        if side == SIDE_A {
            self.a_at(p)
        } else {
            self.pot() - self.a_at(p)
        }
    }
    /// (what `side` is owed, the maintenance) at the prices moved by g toward its gain (`true`) or against it.
    fn health(&self, side: i64, p: [i64; 2], toward_side: bool) -> (i64, i64) {
        // B's gain is A's loss: toward B is against A.
        let v = self.view(p, Some(if side == SIDE_A { toward_side } else { !toward_side }));
        let a = v.raw.clamp(0, self.pot());
        (if side == SIDE_A { a } else { self.pot() - a }, self.maint(v.notional))
    }
    /// Whether the attested prices liquidate `side`: it is below the maintenance even at the prices moved by g toward
    /// its gain (the tolerance inside the trigger), the other party is not below it there (review C9, C9-F1: a pot
    /// smaller than two maintenances liquidates nobody), and the two keys differ.
    pub fn liquidatable(&self, side: i64, p: [i64; 2]) -> bool {
        let (v, m) = self.health(side, p, true);
        self.a_key != self.b_key && v < m && self.pot() - v >= m
    }
    /// What a close pays `side` at the attested prices (its side at the prices moved by g against it); `None` where
    /// the close would escape a due liquidation: the closer is below the maintenance there and the other party has
    /// its own at the liquidation's prices. Where both lack it nobody is liquidated and either party closes.
    pub fn close_pay(&self, side: i64, p: [i64; 2]) -> Option<i64> {
        let (v, m) = self.health(side, p, false);
        let (vt, mt) = self.health(side, p, true);
        (v >= m || self.pot() - vt < mt).then_some(v)
    }
    /// What a cure of a liquidation of `side` at `p` adds to its margin: what puts it at the maintenance at the prices
    /// moved by g against it.
    pub fn cure_add(&self, side: i64, p: [i64; 2]) -> i64 {
        let v = self.view(p, Some(side != SIDE_A));
        let raw = if side == SIDE_A { v.raw } else { self.pot() - v.raw };
        self.maint(v.notional) - raw
    }
    /// What a cure of a liquidation of `side` adds at the prices `p` moved by g toward the side (`Some(true)`),
    /// against it (`Some(false)`: `cure_add`), or as they are: what puts the side at the maintenance there.
    pub fn cure_at(&self, side: i64, p: [i64; 2], toward_side: Option<bool>) -> i64 {
        let v = self.view(p, toward_side.map(|t| if side == SIDE_A { t } else { !t }));
        self.maint(v.notional) - if side == SIDE_A { v.raw } else { self.pot() - v.raw }
    }
    /// The proposer's fee of a liquidation of `side` at `p`: `max(keeperFeeMin, floor(N liqFeeBps / 10^4))` at the
    /// attested prices, at most what the loser is still owed at them (review C9, C9-F2).
    pub fn fee_of(&self, side: i64, p: [i64; 2]) -> i64 {
        let n = self.view(p, None).notional;
        let f = ((n / BPS) * self.t.liq_fee_bps + ((n % BPS) * self.t.liq_fee_bps) / BPS).max(self.t.keeper_fee_min);
        f.min(self.owed_at(side, p))
    }
    /// The account with `side`'s margin grown by `add`.
    pub fn with_margin(&self, side: i64, add: i64) -> Cross {
        let mut c = *self;
        if side == SIDE_A {
            c.a_margin += add;
        } else {
            c.b_margin += add;
        }
        c
    }
    /// `addMargin`: `None` when the margin would leave its range.
    pub fn added(&self, side: i64, delta: i64) -> Option<Cross> {
        let c = self.with_margin(side, delta);
        (delta > 0 && delta <= MARGIN_MAX && (side == SIDE_A || side == SIDE_B) && c.a_margin <= MARGIN_MAX && c.b_margin <= MARGIN_MAX).then_some(c)
    }
    /// `keepAlive` with a signed price at `t0`: the lease at `t0 + term`; `None` when that is not a quarter of a term later.
    pub fn kept_alive(&self, t0: i64) -> Option<Cross> {
        let x2 = t0 + self.t.term;
        (t0 >= 0 && t0 < LOCK_TIME_THRESHOLD && x2 >= self.expiry + self.t.term / 4).then(|| Cross { expiry: x2, ..*self })
    }
    /// The first signed price that can renew the lease.
    pub fn keep_alive_from(&self) -> i64 {
        self.expiry + self.t.term / 4 - self.t.term
    }
    /// The first DAA at which anyone settles the account at its entries.
    pub fn lapse_from(&self) -> u64 {
        (self.expiry + self.t.idle_daa).max(0) as u64
    }
    /// `lapseSettle`: (A, B) of an account of `value`: A its margin, B the rest; a payout below dust goes to the other.
    pub fn lapse_out(&self, value: u64) -> (i64, i64) {
        let v = value as i64;
        let a = self.a_margin.min(v);
        if a < PLAIN_DUST {
            (0, v)
        } else if v - a < PLAIN_DUST {
            (v, 0)
        } else {
            (a, v - a)
        }
    }
    /// `propose`: a claim whose signed price is more than `lateDaa` older than the account's UTXO is refused.
    pub fn price_fresh(&self, t0: i64, born: u64) -> bool {
        t0 as i128 + self.t.late_daa as i128 >= born as i128
    }
    /// The pending claim `propose` writes for (kind, side) at the attested prices `a` of the time t0, proposed by
    /// `proposer`. `None` when the contract refuses it: a close below the closer's maintenance, a liquidation that
    /// does not trigger or of an account with one owner. The epoch tables and the quiet time are the caller's.
    pub fn pending_of(&self, k: &XKit, kind: i64, side: i64, a: [i64; 2], t0: i64, proposer: [u8; 32]) -> Option<XPending> {
        if self.check().is_err() || a[0] < 0 || (self.legs[1].size > 0 && a[1] < 0) || (side != SIDE_A && side != SIDE_B) {
            return None;
        }
        let a = if self.legs[1].size > 0 { a } else { [a[0], 0] };
        let (amount, cure_add, cure_hash, who) = if kind == KIND_CLOSE {
            (self.close_pay(side, a)?, 0, [0u8; 32], if side == SIDE_A { self.a_key } else { self.b_key })
        } else if kind == KIND_LIQ {
            if !self.liquidatable(side, a) {
                return None;
            }
            let add = self.cure_add(side, a);
            let cured = self.with_margin(side, add);
            if add <= 0 || cured.a_margin > MARGIN_MAX || cured.b_margin > MARGIN_MAX {
                return None;
            }
            (self.fee_of(side, a), add, cured.script_hash(k), proposer)
        } else {
            return None;
        };
        Some(XPending {
            late_daa: self.t.late_daa,
            grace_daa: self.t.grace_daa,
            kind,
            side,
            t0,
            amount,
            cure_add,
            a_key: self.a_key,
            b_key: self.b_key,
            proposer: who,
            restore_hash: self.script_hash(k),
            cure_hash,
            mk_parent: [0; 32],
            account: *self,
        })
    }
}

/// A pending claim on an account (`KpfPendingX`'s state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XPending {
    pub late_daa: i64,
    pub grace_daa: i64,
    pub kind: i64,
    pub side: i64,
    pub t0: i64,
    pub amount: i64,
    pub cure_add: i64,
    pub a_key: [u8; 32],
    pub b_key: [u8; 32],
    pub proposer: [u8; 32],
    pub restore_hash: [u8; 32],
    pub cure_hash: [u8; 32],
    pub mk_parent: [u8; 32],
    /// The account as it was when the claim was proposed.
    pub account: Cross,
}

/// What `KpfPendingX.finalize` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XSettlement {
    /// A valid close: the two payouts (0: none).
    Close { a: i64, b: i64 },
    /// A valid liquidation: the proposer's fee (0: none) and the rest to the winner.
    Forfeit { fee: i64, winner: [u8; 32], rest: i64 },
    /// A void claim: the account restored with `value`.
    Restore { account: Cross, value: i64 },
}

impl XPending {
    fn fields(&self) -> Vec<F> {
        let mut f: Vec<F> = [self.late_daa, self.grace_daa, self.kind, self.side, self.t0, self.amount, self.cure_add].map(F::I).into_iter().collect();
        f.extend([self.a_key, self.b_key, self.proposer, self.restore_hash, self.cure_hash, self.mk_parent].map(F::B));
        f
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut v = encode(&self.fields());
        // The account's state: a 405-byte push (OP_PUSHDATA2, the length little-endian).
        v.extend([0x4d, (CROSS_STATE_LEN & 0xff) as u8, (CROSS_STATE_LEN >> 8) as u8]);
        v.extend(self.account.state_bytes());
        v
    }
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        let mut a = to_args(&self.fields());
        a.push(ArtifactValue::Bytes(self.account.state_bytes()));
        a
    }
    pub fn redeem(&self, k: &XKit) -> Vec<u8> {
        k.pending.redeem(&self.state_bytes())
    }
    pub fn decode_state(b: &[u8]) -> Result<XPending> {
        let head = 7 * 9 + 6 * 33;
        if b.len() != XPENDING_STATE_LEN || b[head..head + 3] != [0x4d, (CROSS_STATE_LEN & 0xff) as u8, (CROSS_STATE_LEN >> 8) as u8] {
            return Err(Error::Decode("not the state of a pending account claim".into()));
        }
        let mut c = Cursor { b: &b[..head], at: 0 };
        let w = XPending {
            late_daa: c.int()?,
            grace_daa: c.int()?,
            kind: c.int()?,
            side: c.int()?,
            t0: c.int()?,
            amount: c.int()?,
            cure_add: c.int()?,
            a_key: c.b32()?,
            b_key: c.b32()?,
            proposer: c.b32()?,
            restore_hash: c.b32()?,
            cure_hash: c.b32()?,
            mk_parent: c.b32()?,
            account: Cross::decode_state(&b[head + 3..])?,
        };
        c.done()?;
        Ok(w)
    }
    /// This claim as `propose` writes it from the account input at outpoint `op`.
    pub fn made_by(&self, op: &TransactionOutpoint) -> XPending {
        XPending { mk_parent: maker_of(op), ..*self }
    }
    /// Whether the state the claim carries is the account its hashes bind (a wallet's check of a pending claim it
    /// did not write): the restore hash is the carried account's, the cure hash the carried account's with the
    /// loser's margin grown by `cure_add`.
    pub fn consistent(&self, k: &XKit) -> bool {
        self.restore_hash == self.account.script_hash(k) && (self.kind != KIND_LIQ || self.cure_hash == self.account.with_margin(self.side, self.cure_add).script_hash(k))
    }
    /// The claim's validity at P (the claim's own DAA score): it landed within `lateDaa` of its time.
    pub fn valid_at(&self, p: i64) -> bool {
        p as i128 - self.t0 as i128 <= self.late_daa as i128
    }
    pub fn last_valid_landing(&self) -> i64 {
        self.t0 + self.late_daa
    }
    /// The age the claim needs before finalize when it landed at `p`: `graceDaa` for a valid liquidation.
    pub fn finalize_age(&self, p: i64) -> u64 {
        if self.kind == KIND_LIQ && self.valid_at(p) {
            self.grace_daa.max(0) as u64
        } else {
            0
        }
    }
    /// What finalize does with a claim of `value` whose P is `p` (`KpfPendingX.finalize`).
    pub fn settlement(&self, value: u64, p: i64) -> XSettlement {
        let v = value as i64;
        if !self.valid_at(p) {
            return XSettlement::Restore { account: self.account, value: v };
        }
        if self.kind == KIND_CLOSE {
            let mut mine = self.amount.min(v);
            let mut other = v - mine;
            if other < PLAIN_DUST {
                (mine, other) = (v, 0);
            } else if mine < PLAIN_DUST {
                (mine, other) = (0, v);
            }
            if self.side == SIDE_A {
                XSettlement::Close { a: mine, b: other }
            } else {
                XSettlement::Close { a: other, b: mine }
            }
        } else {
            let mut fee = self.amount.min(v);
            if fee < PLAIN_DUST {
                fee = 0;
            }
            XSettlement::Forfeit { fee, winner: if self.side == SIDE_A { self.b_key } else { self.a_key }, rest: v - fee }
        }
    }
    /// The cure (`KpfPendingX.cure`): the cured account and its least value from a claim of `value`; `None` when the
    /// claim is not a liquidation.
    pub fn cure(&self, value: u64) -> Option<(Cross, i64)> {
        (self.kind == KIND_LIQ && self.cure_add > 0).then(|| (self.account.with_margin(self.side, self.cure_add), value as i64 + self.cure_add))
    }
}

/// The sample states the pinned artifacts are compiled with (a template does not depend on its state).
pub mod samples {
    use super::*;

    pub fn terms() -> XTerms {
        XTerms { term: 6_048_000, mmr_bps: 500, g_bps: 50, liq_fee_bps: 50, keeper_fee_min: 10_000_000, carrier: 20_000_000, idle_daa: 864_000, grace_daa: 100, late_daa: 100, quiet_daa: 20 }
    }
    pub fn cross() -> Cross {
        Cross {
            t: terms(),
            legs: [
                XLeg { market: [0x11; 32], committee: [0x22; 32], scale: 100_000_000, price_max: PRICE_CAP, size: 1_000_000_000, dir: LONG, entry: 1_000_000_000 },
                XLeg { market: [0x33; 32], committee: [0x22; 32], scale: 100_000_000, price_max: PRICE_CAP, size: 500_000_000, dir: SHORT, entry: 2_000_000_000 },
            ],
            a_key: [1; 32],
            b_key: [2; 32],
            a_margin: 2_000_000_000,
            b_margin: 3_000_000_000,
            expiry: 6_048_000,
        }
    }
    pub fn pending(x: &Cross) -> XPending {
        XPending {
            late_daa: x.t.late_daa,
            grace_daa: x.t.grace_daa,
            kind: KIND_CLOSE,
            side: SIDE_A,
            t0: 1,
            amount: 1,
            cure_add: 0,
            a_key: x.a_key,
            b_key: x.b_key,
            proposer: x.a_key,
            restore_hash: [3; 32],
            cure_hash: [0; 32],
            mk_parent: [0; 32],
            account: *x,
        }
    }
}
