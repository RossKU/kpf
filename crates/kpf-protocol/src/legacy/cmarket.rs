//! Committee markets (`docs/DESIGN-COMMITTEE.md` 3): the market block, and the states and math of `KpfOpenC`,
//! `KpfPairC` and `KpfPendingC`. It is `crate::oracle` without an underwriter: no binding, no cover, no premium, no
//! bond and no contest; a pair carries a lease instead, and a claim names who proposed it.
//!
//! | object | state after the market block | bytes |
//! |---|---|---|
//! | market | id, committee id, feed hash, `termDaa, scale, priceMax, mmrBps, gBps, liqFeeBps, keeperFeeMin, minSize, carrier, idleDaa, graceDaa, lateDaa, quietDaa` | 216 |
//! | order | `side, owner, sizeLeft, limit, myRate, minCounterRate, minFill, tip, activeFrom, expiryDaa` | 330 |
//! | pair | `size, bl, bs, entry, longKey, shortKey, expiry`, the two listings, the five makers | 546 |
//! | pending | `kind, side, n, bl, bs, quote, t0, cureBound, fee, refund, proposer`, the pair's entry, keys, lease and listings, the three return hashes, the maker | 609 |

use kaspa_consensus_core::tx::TransactionOutpoint;
use silverscript_abi::ArtifactValue;

use crate::artifacts::Template;
use crate::committee::blake2b;
use crate::error::{Error, Result};
use crate::oracle::{maker_of, Makers, PART_FLOOR};
use crate::payoff::q;
use crate::state::{encode, to_args, Cursor, F, BPS, KIND_CLOSE, KIND_LIQ, KIND_TRIM, LOCK_TIME_THRESHOLD, LONG, MMR_CAP, PLAIN_DUST, PRICE_CAP, SCALE_MAX, SHORT};

pub const CMARKET_LEN: usize = 216;
pub const COPEN_STATE_LEN: usize = 330;
pub const CPAIR_STATE_LEN: usize = 546;
pub const CPENDING_STATE_LEN: usize = 609;
pub const TERM_MAX: i64 = 77_760_000;
pub const TERM_MIN: i64 = 1_200;
/// A market's tolerance g is at least this many times its committee's spread (`KpfPairC.SPREAD_MARGIN`).
pub const SPREAD_MARGIN: i64 = 4;
pub const WAIT_MAX: i64 = 864_000;
pub const FEE_MIN_MAX: i64 = 1_000_000_000_000;
/// `KpfEpochTable.attest`'s signature script before the redeem script: the six arguments, the dispatch tag, the push header.
pub const ATTEST_HEAD: usize = 1153;

pub type Refusal = &'static str;

fn chk(ok: bool, why: Refusal) -> std::result::Result<(), Refusal> {
    if ok {
        Ok(())
    } else {
        Err(why)
    }
}

// ---------------------------------------------------------------- the market block

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CParams {
    pub term: i64,
    pub scale: i64,
    pub price_max: i64,
    pub mmr_bps: i64,
    pub g_bps: i64,
    pub liq_fee_bps: i64,
    pub keeper_fee_min: i64,
    pub min_size: i64,
    pub carrier: i64,
    pub idle_daa: i64,
    pub grace_daa: i64,
    pub late_daa: i64,
    pub quiet_daa: i64,
}

impl CParams {
    fn fields(&self) -> Vec<F> {
        [self.term, self.scale, self.price_max, self.mmr_bps, self.g_bps, self.liq_fee_bps, self.keeper_fee_min, self.min_size, self.carrier, self.idle_daa, self.grace_daa, self.late_daa, self.quiet_daa]
            .map(F::I)
            .into_iter()
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CMarket {
    pub committee_id: [u8; 32],
    pub feed_hash: [u8; 32],
    pub p: CParams,
}

impl CMarket {
    fn body_fields(&self) -> Vec<F> {
        let mut f = vec![F::B(self.committee_id), F::B(self.feed_hash)];
        f.extend(self.p.fields());
        f
    }
    /// What the id is the blake3 of: the committee id, the feed hash and the parameters, as their state pushes.
    pub fn body(&self) -> Vec<u8> {
        encode(&self.body_fields())
    }
    pub fn id(&self) -> [u8; 32] {
        *blake3::hash(&self.body()).as_bytes()
    }
    fn fields(&self) -> Vec<F> {
        let mut f = vec![F::B(self.id())];
        f.extend(self.body_fields());
        f
    }
    /// The 216 bytes every order, pair and pending part of the market starts with.
    pub fn bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    /// `checkMarket`.
    pub fn check(&self) -> std::result::Result<(), Refusal> {
        let p = &self.p;
        chk(self.committee_id != [0; 32], "no committee")?;
        chk(p.term >= TERM_MIN && p.term <= TERM_MAX, "termDaa out of range")?;
        chk(p.scale > 0 && p.scale <= SCALE_MAX, "scale out of range")?;
        chk(p.price_max > 0 && p.price_max <= PRICE_CAP, "priceMax out of range")?;
        chk(p.mmr_bps >= 0 && p.mmr_bps < MMR_CAP, "mmrBps out of range")?;
        chk(p.g_bps > 0 && p.g_bps < BPS, "gBps out of range")?;
        chk(p.liq_fee_bps >= 0 && p.liq_fee_bps <= BPS, "liqFeeBps out of range")?;
        chk(p.keeper_fee_min >= 0 && p.keeper_fee_min <= FEE_MIN_MAX, "keeperFeeMin out of range")?;
        chk(p.min_size > 0, "minSize must be positive")?;
        chk(p.carrier >= 0, "negative carrier")?;
        chk(p.idle_daa >= 0 && p.idle_daa <= TERM_MAX, "idleDaa out of range")?;
        chk(p.grace_daa >= 0 && p.grace_daa <= WAIT_MAX, "graceDaa out of range")?;
        chk(p.late_daa > 0 && p.late_daa <= WAIT_MAX, "lateDaa out of range")?;
        chk(p.quiet_daa >= 0 && p.quiet_daa <= WAIT_MAX, "quietDaa out of range")
    }
    /// `KpfPairC.priced` (review C2): an epoch table prices this market only if the market's g leaves its spread a margin.
    pub fn spread_ok(&self, spread_bps: i64) -> bool {
        spread_bps >= 0 && spread_bps * SPREAD_MARGIN <= self.p.g_bps
    }
    /// The testnet defaults (docs/DESIGN-COMMITTEE.md 6): an underlying of 8 decimals priced in sompi, a lease of 7
    /// days, a lapse 1 day after it, 10 s to cure and for a claim to land, 2 s of quiet.
    pub fn testnet(committee_id: [u8; 32], feed_hash: [u8; 32]) -> CMarket {
        CMarket {
            committee_id,
            feed_hash,
            p: CParams {
                term: 6_048_000,
                scale: 100_000_000,
                price_max: PRICE_CAP,
                mmr_bps: 500,
                g_bps: 50,
                liq_fee_bps: 50,
                keeper_fee_min: 10_000_000,
                min_size: 1_000_000,
                carrier: 20_000_000,
                idle_daa: 864_000,
                grace_daa: 100,
                late_daa: 100,
                quiet_daa: 20,
            },
        }
    }
    fn read(c: &mut Cursor) -> Result<CMarket> {
        let id = c.b32()?;
        let m = CMarket {
            committee_id: c.b32()?,
            feed_hash: c.b32()?,
            p: CParams {
                term: c.int()?,
                scale: c.int()?,
                price_max: c.int()?,
                mmr_bps: c.int()?,
                g_bps: c.int()?,
                liq_fee_bps: c.int()?,
                keeper_fee_min: c.int()?,
                min_size: c.int()?,
                carrier: c.int()?,
                idle_daa: c.int()?,
                grace_daa: c.int()?,
                late_daa: c.int()?,
                quiet_daa: c.int()?,
            },
        };
        if m.id() != id {
            return Err(Error::Decode("the market id is not blake3 of the market body".into()));
        }
        Ok(m)
    }
    pub fn decode(b: &[u8]) -> Result<CMarket> {
        let mut c = Cursor { b, at: 0 };
        let m = CMarket::read(&mut c)?;
        c.done()?;
        Ok(m)
    }
}

// ---------------------------------------------------------------- build constants and templates

/// What the family's templates are compiled with: each other's template hashes and the epoch table's entry tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CConsts {
    pub epoch_table_tpl: [u8; 32],
    pub epoch_table_pre: i64,
    pub epoch_table_suf: i64,
    pub attest_tag: [u8; 4],
    pub exit_tpl: [u8; 32],
    pub pending_tpl: [u8; 32],
    pub pair_tpl: [u8; 32],
}

impl CConsts {
    /// `KpfPairC`'s build constants.
    pub fn pair_args(&self) -> Vec<ArtifactValue> {
        vec![
            ArtifactValue::Bytes(self.pending_tpl.to_vec()),
            ArtifactValue::Bytes(self.epoch_table_tpl.to_vec()),
            ArtifactValue::Int(self.epoch_table_pre),
            ArtifactValue::Int(self.epoch_table_suf),
            ArtifactValue::Bytes(self.attest_tag.to_vec()),
        ]
    }
    /// `KpfOpenC`'s build constants.
    pub fn open_args(&self) -> Vec<ArtifactValue> {
        vec![ArtifactValue::Bytes(self.pair_tpl.to_vec())]
    }
}

/// The templates of the committee family (a fresh compile in the tests, the pinned ones once pinned).
#[derive(Clone, Debug)]
pub struct CKit {
    pub open: Template,
    pub pair: Template,
    pub pending: Template,
    pub committee: Template,
    pub epoch_table: Template,
    pub exit: Template,
    pub charge: Template,
    pub consts: CConsts,
}

// ---------------------------------------------------------------- the order

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct COpen {
    pub m: CMarket,
    pub side: i64,
    pub owner: [u8; 32],
    pub size_left: i64,
    pub limit: i64,
    pub my_rate: i64,
    pub min_counter_rate: i64,
    pub min_fill: i64,
    pub tip: i64,
    pub active_from: i64,
    pub expiry_daa: i64,
}

impl COpen {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend([F::I(self.side), F::B(self.owner)]);
        f.extend([self.size_left, self.limit, self.my_rate, self.min_counter_rate, self.min_fill, self.tip, self.active_from, self.expiry_daa].map(F::I));
        f
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    pub fn ctor_with(&self, c: &CConsts) -> Vec<ArtifactValue> {
        let mut a = c.open_args();
        a.extend(to_args(&self.fields()));
        a
    }
    pub fn redeem(&self, k: &CKit) -> Vec<u8> {
        k.open.redeem(&self.state_bytes())
    }
    pub fn decode_state(b: &[u8]) -> Result<COpen> {
        let mut c = Cursor { b, at: 0 };
        let m = CMarket::read(&mut c)?;
        let o = COpen {
            m,
            side: c.int()?,
            owner: c.b32()?,
            size_left: c.int()?,
            limit: c.int()?,
            my_rate: c.int()?,
            min_counter_rate: c.int()?,
            min_fill: c.int()?,
            tip: c.int()?,
            active_from: c.int()?,
            expiry_daa: c.int()?,
        };
        c.done()?;
        Ok(o)
    }
    /// The carrier a part of n takes from this order: one carrier with the part that empties the order.
    pub fn carrier_of(&self, n: i64) -> i64 {
        if n == self.size_left {
            self.m.p.carrier
        } else {
            0
        }
    }
    /// What a fill or take of n takes from the escrow: `q(n, myRate + tip)`, and the carrier when it empties the order.
    pub fn spend(&self, n: i64) -> i64 {
        q(n, self.my_rate + self.tip, self.m.p.scale, 0) + self.carrier_of(n)
    }
    /// This order's margin in a part of n: `q(n, myRate)`.
    pub fn margin_of(&self, n: i64) -> i64 {
        q(n, self.my_rate, self.m.p.scale, 0)
    }
    /// `KpfOpenC.amount`.
    pub fn part_ok(&self, n: i64) -> bool {
        let left = self.size_left - n;
        n > 0
            && n <= self.size_left
            && n >= self.m.p.min_size
            && (n >= self.min_fill || n == self.size_left)
            && (left == 0 || left >= self.m.p.min_size)
            && n / self.m.p.scale <= (i64::MAX - self.m.p.carrier) / self.m.p.price_max - 1
            && self.margin_of(n) >= PART_FLOOR
            && (left == 0 || self.margin_of(left) >= PART_FLOOR)
    }
    /// What whoever refunds this order (escrow `value`) may keep: `keeperFeeMin`, at most a twentieth of the escrow.
    pub fn refund_fee(&self, value: u64) -> i64 {
        self.m.p.keeper_fee_min.min(value as i64 / 20)
    }
    /// The order after a part of n left it.
    pub fn after(&self, n: i64) -> COpen {
        COpen { size_left: self.size_left - n, ..self.clone() }
    }
    /// The lease a fill may give a pair: at least a term after this order was accepted at `born`.
    pub fn least_lease(&self, born: i64) -> i64 {
        born + self.m.p.term
    }
}

// ---------------------------------------------------------------- the pair

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CPair {
    pub m: CMarket,
    pub size: i64,
    pub bl: i64,
    pub bs: i64,
    pub entry: i64,
    pub long_key: [u8; 32],
    pub short_key: [u8; 32],
    pub expiry: i64,
    pub long_ask: i64,
    pub long_min: i64,
    pub long_tip: i64,
    pub short_bid: i64,
    pub short_min: i64,
    pub short_tip: i64,
    pub mk: Makers,
}

impl CPair {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend([F::I(self.size), F::I(self.bl), F::I(self.bs), F::I(self.entry), F::B(self.long_key), F::B(self.short_key), F::I(self.expiry)]);
        f.extend([self.long_ask, self.long_min, self.long_tip, self.short_bid, self.short_min, self.short_tip].map(F::I));
        f.extend([F::B(self.mk.long), F::B(self.mk.short), F::B(self.mk.parent), F::B(self.mk.host), F::B(self.mk.donor)]);
        f
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    pub fn ctor_with(&self, c: &CConsts) -> Vec<ArtifactValue> {
        let mut a = c.pair_args();
        a.extend(to_args(&self.fields()));
        a
    }
    pub fn redeem(&self, k: &CKit) -> Vec<u8> {
        k.pair.redeem(&self.state_bytes())
    }
    /// The P2SH script hash of this state (what a pending part pays to with `new ScriptPubKeyP2SH(hash)`).
    pub fn script_hash(&self, k: &CKit) -> [u8; 32] {
        blake2b(&self.redeem(k))
    }
    pub fn decode_state(b: &[u8]) -> Result<CPair> {
        let mut c = Cursor { b, at: 0 };
        let m = CMarket::read(&mut c)?;
        let p = CPair {
            m,
            size: c.int()?,
            bl: c.int()?,
            bs: c.int()?,
            entry: c.int()?,
            long_key: c.b32()?,
            short_key: c.b32()?,
            expiry: c.int()?,
            long_ask: c.int()?,
            long_min: c.int()?,
            long_tip: c.int()?,
            short_bid: c.int()?,
            short_min: c.int()?,
            short_tip: c.int()?,
            mk: Makers { long: c.b32()?, short: c.b32()?, parent: c.b32()?, host: c.b32()?, donor: c.b32()? },
        };
        c.done()?;
        Ok(p)
    }
    pub fn range_of(&self, n: i64) -> i64 {
        q(n, self.bs - self.bl, self.m.p.scale, 0)
    }
    /// The least value of the pair: `q(s, bs - bl) + carrier`.
    pub fn floor(&self) -> i64 {
        self.range_of(self.size) + self.m.p.carrier
    }
    /// The range wider than both maintenance bands (`bl * (10^4 + m) < bs * (10^4 - m)`).
    pub fn range_ok(&self) -> bool {
        let m = self.m.p.mmr_bps as i128;
        (self.bl as i128) * (BPS as i128 + m) < (self.bs as i128) * (BPS as i128 - m)
    }
    /// The part n of this pair (keys, lease, listings kept), makers cleared.
    pub fn rest_of(&self, n: i64) -> CPair {
        CPair { size: n, mk: Makers::default(), ..self.clone() }
    }
    fn unlisted_part(&self, n: i64) -> CPair {
        CPair { long_ask: 0, long_min: 0, long_tip: 0, short_bid: 0, short_min: 0, short_tip: 0, ..self.rest_of(n) }
    }
    /// The long's and the short's payoff at a price (carrier excluded).
    pub fn long_at(&self, p: i64) -> i64 {
        q(self.size, p.clamp(self.bl, self.bs) - self.bl, self.m.p.scale, 0)
    }
    pub fn short_at(&self, p: i64) -> i64 {
        q(self.size, self.bs - p.clamp(self.bl, self.bs), self.m.p.scale, 0)
    }
    /// Whether an attested price `p` liquidates the long (`KpfPairC.propose`):
    /// `ceil(p * (10^4 + g) / 10^4) * (10^4 - m) < bl * 10^4`, and the range is wider than both bands.
    pub fn long_liquidatable(&self, p: i64) -> bool {
        let (b, g, m) = (BPS as i128, self.m.p.g_bps as i128, self.m.p.mmr_bps as i128);
        let pt = (p as i128 * (b + g) + b - 1) / b;
        self.range_ok() && pt * (b - m) < self.bl as i128 * b
    }
    /// Whether an attested price `p` liquidates the short: `bs * 10^4 < floor(p * (10^4 - g) / 10^4) * (10^4 + m)`.
    pub fn short_liquidatable(&self, p: i64) -> bool {
        let (b, g, m) = (BPS as i128, self.m.p.g_bps as i128, self.m.p.mmr_bps as i128);
        let pt = p as i128 * (b - g) / b;
        self.range_ok() && (self.bs as i128) * b < pt * (b + m)
    }
    /// The trigger price T of a loser `side` from the pair's own state (as KpfPair).
    pub fn trigger_of(&self, side: i64) -> i64 {
        let m = self.m.p.mmr_bps;
        if side == LONG {
            self.bl * BPS / (BPS - m)
        } else {
            (self.bs * BPS + BPS + m - 1) / (BPS + m)
        }
    }
    /// The proposer's fee of a liquidation of n at price p: `min(max(keeperFeeMin, q(n, floor(p * liqFeeBps / 10^4))),
    /// maintenance slice)`.
    pub fn fee_of(&self, n: i64, side: i64, p: i64) -> i64 {
        let sc = self.m.p.scale;
        let f = q(n, p * self.m.p.liq_fee_bps / BPS, sc, 0).max(self.m.p.keeper_fee_min);
        let t = self.trigger_of(side);
        let slice = if side == LONG { q(n, t - self.bl, sc, 0) } else { q(n, self.bs - t, sc, 0) };
        f.min(slice)
    }
    /// The cure bound of a liquidation at p (as KpfPair): `None` when the short's cured bound would exceed priceMax.
    pub fn cure_bound_of(&self, side: i64, p: i64) -> Option<i64> {
        let (b, g, m) = (BPS as i128, self.m.p.g_bps as i128, self.m.p.mmr_bps as i128);
        let pp = p as i128;
        let cb = if side == LONG {
            let pg = (pp * (b - g) + b - 1) / b;
            pg * (b - m) / b
        } else {
            let pg = pp * (b + g) / b;
            (pg * (b + m) + b - 1) / b
        };
        (side == LONG || cb <= self.m.p.price_max as i128).then_some(cb as i64)
    }
    /// The forfeit bound (as KpfPair): the loser's bound moved inward to the nearest bound at which the narrowed range
    /// plus the fee still fits the part.
    pub fn forfeit_bound_of(&self, n: i64, side: i64, fee: i64) -> i64 {
        let sc = self.m.p.scale;
        let part = self.range_of(n);
        let fits = |w: i64| q(n, w, sc, 0) + fee <= part;
        let (mut lo, mut hi) = (1i64, self.bs - self.bl);
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
            self.bs - lo
        } else {
            self.bl + lo
        }
    }
    /// `keepAlive` with a signed price at `t0`: the pair with its lease at `t0 + termDaa`; `None` when that is not at least a
    /// quarter of a term later than the lease it has.
    pub fn kept_alive(&self, t0: i64) -> Option<CPair> {
        let x2 = t0 + self.m.p.term;
        (t0 >= 0 && t0 < LOCK_TIME_THRESHOLD && x2 >= self.expiry + self.m.p.term / 4).then(|| CPair { expiry: x2, mk: Makers::default(), ..self.clone() })
    }
    /// The first signed price that can renew this pair's lease.
    pub fn keep_alive_from(&self) -> i64 {
        self.expiry + self.m.p.term / 4 - self.m.p.term
    }
    /// The first DAA at which anyone settles the pair at its entry (`lapseSettle`).
    pub fn lapse_from(&self) -> u64 {
        (self.expiry + self.m.p.idle_daa).max(0) as u64
    }
    /// `lapseSettle` (as KpfPair's idle settle): (long, short) of a pair of `value`.
    pub fn lapse_out(&self, value: u64) -> (i64, i64) {
        let e = self.entry.clamp(self.bl, self.bs);
        let v = value as i64;
        let lp = q(self.size, e - self.bl, self.m.p.scale, 0);
        let sp = v - lp;
        if lp < PLAIN_DUST {
            (0, v)
        } else if sp < PLAIN_DUST {
            (v, 0)
        } else {
            (lp, sp)
        }
    }
    /// The restore state of a claim on the part n: keys, bounds, entry, lease and listings unchanged, makers 0.
    pub fn restore_state(&self, n: i64) -> CPair {
        self.rest_of(n)
    }
    /// The cure state: the loser's bound at `cb`.
    pub fn cure_state(&self, n: i64, side: i64, cb: i64) -> CPair {
        let mut c = self.unlisted_part(n);
        if side == LONG {
            c.bl = cb;
        } else {
            c.bs = cb;
        }
        c
    }
    /// The forfeit state: both keys the winner's, the loser's bound at `fb`.
    pub fn forfeit_state(&self, n: i64, side: i64, fb: i64) -> CPair {
        let mut f = self.unlisted_part(n);
        let w = if side == LONG { self.short_key } else { self.long_key };
        f.long_key = w;
        f.short_key = w;
        if side == LONG {
            f.bl = fb;
        } else {
            f.bs = fb;
        }
        f
    }
    /// `KpfPairC.propose` kind 1: a party may close its own side at `p` only if it is not below its maintenance at the
    /// close price (p moved by g against it).
    pub fn close_ok(&self, side: i64, p: i64) -> bool {
        let (b, g, m) = (BPS as i128, self.m.p.g_bps as i128, self.m.p.mmr_bps as i128);
        let p = p as i128;
        if side == LONG {
            (p * (b - g) / b) * (b - m) >= self.bl as i128 * b
        } else {
            self.bs as i128 * b >= ((p * (b + g) + b - 1) / b) * (b + m)
        }
    }
    /// The pair with `side`'s bound moved to `nb` (a trim; keys, entry, lease and listings kept, makers cleared).
    pub fn trim_state(&self, side: i64, nb: i64) -> CPair {
        let mut t = self.rest_of(self.size);
        if side == LONG {
            t.bl = nb;
        } else {
            t.bs = nb;
        }
        t
    }
    /// What a trim of `side` to the bound `nb` frees: `q(s, bs - bl) - q(s, the new range)`.
    pub fn trim_take(&self, side: i64, nb: i64) -> i64 {
        self.range_of(self.size) - self.trim_state(side, nb).range_of(self.size)
    }
    /// `KpfPairC.propose` kind 3: whether the owner of `side` may trim its bound inward to `nb` at the attested price
    /// `p`: the bound moves inward, the side is not below its maintenance at p moved by g against it measured from
    /// `nb`, the trimmed range is still wider than both maintenance bands, and at least `PLAIN_DUST` leaves.
    pub fn trim_ok(&self, side: i64, p: i64, nb: i64) -> bool {
        if (side != LONG && side != SHORT) || !self.split_ok(self.size) || nb < 0 || nb > self.m.p.price_max {
            return false;
        }
        let t = self.trim_state(side, nb);
        let inward = if side == LONG { nb > self.bl } else { nb < self.bs };
        inward && t.range_ok() && t.close_ok(side, p) && self.trim_take(side, nb) >= PLAIN_DUST
    }
    /// The nearest bound a trim of `side` at the attested price `p` may ask for (the one that frees the most); `None`
    /// when the side has nothing to trim at `p`.
    pub fn trim_bound_of(&self, side: i64, p: i64) -> Option<i64> {
        let (b, g, m) = (BPS as i128, self.m.p.g_bps as i128, self.m.p.mmr_bps as i128);
        let px = p.min(self.m.p.price_max);
        let pp = px as i128;
        let nb = if side == LONG {
            let healthy = (pp * (b - g) / b) * (b - m) / b;
            let wide = (self.bs as i128 * (b - m) + (b + m) - 1) / (b + m) - 1;
            healthy.min(wide)
        } else {
            let healthy = (((pp * (b + g) + b - 1) / b) * (b + m) + b - 1) / b;
            let wide = self.bl as i128 * (b + m) / (b - m) + 1;
            healthy.max(wide)
        };
        let nb = i64::try_from(nb).ok()?;
        self.trim_ok(side, px, nb).then_some(nb)
    }
    /// The bound a trim of `side` asks for to free at least `amount`: the nearest to the present bound that does.
    /// `None` when no bound frees that much ([`CPair::trim_ok`] says whether the price allows it).
    pub fn trim_bound_for(&self, side: i64, amount: i64) -> Option<i64> {
        let width = self.bs - self.bl;
        let all = self.range_of(self.size);
        if amount <= 0 || all < amount {
            return None;
        }
        // The least move d with q(s, width) - q(s, width - d) >= amount (q is monotone in the range).
        let (mut lo, mut hi) = (1i64, width);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if all - q(self.size, width - mid, self.m.p.scale, 0) >= amount {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        Some(if side == LONG { self.bl + lo } else { self.bs - lo })
    }
    /// The pending part `propose` writes for a trim of `side` to `nb` at the attested price `a` (counted as
    /// `min(a, priceMax)`) of the time t0: the whole pair, the claim's `cure_bound` the bound asked for and its
    /// `cure_hash` the trimmed pair. `None` when the trim cannot be proposed ([`CPair::trim_ok`]).
    pub fn trim_pending(&self, k: &CKit, side: i64, a: i64, t0: i64, nb: i64) -> Option<CPending> {
        if a < 0 {
            return None;
        }
        let p = a.min(self.m.p.price_max);
        if !self.trim_ok(side, p, nb) {
            return None;
        }
        Some(CPending {
            m: self.m,
            kind: KIND_TRIM,
            side,
            n: self.size,
            bl: self.bl,
            bs: self.bs,
            quote: p,
            t0,
            cure_bound: nb,
            fee: 0,
            refund: 0,
            proposer: if side == LONG { self.long_key } else { self.short_key },
            entry: self.entry,
            long_key: self.long_key,
            short_key: self.short_key,
            expiry: self.expiry,
            long_ask: self.long_ask,
            long_min: self.long_min,
            long_tip: self.long_tip,
            short_bid: self.short_bid,
            short_min: self.short_min,
            short_tip: self.short_tip,
            restore_hash: self.restore_state(self.size).script_hash(k),
            cure_hash: self.trim_state(side, nb).script_hash(k),
            forfeit_hash: [0; 32],
            mk_parent: [0; 32],
        })
    }
    /// `KpfPairC.checkPart`: the sizes, and each side with value at the entry keeps at least `PART_FLOOR` of it in the
    /// smaller piece of a split.
    pub fn split_ok(&self, n: i64) -> bool {
        if n <= 0 || n > self.size || n < self.m.p.min_size {
            return false;
        }
        if n == self.size {
            return true;
        }
        if self.size - n < self.m.p.min_size {
            return false;
        }
        let e = self.entry.clamp(self.bl, self.bs);
        let m = n.min(self.size - n);
        [e - self.bl, self.bs - e].iter().all(|&r| r == 0 || q(m, r, self.m.p.scale, 0) >= PART_FLOOR)
    }
    /// The pending part `propose` writes for a claim (kind, side, the attested price `a` at t0, counted as
    /// `min(a, priceMax)`, proposed by `proposer`) on the part n, and the forfeit bound argument `fb`. `None` when the claim cannot be proposed: a liquidation that does not
    /// trigger, of a part whose keys are equal, or whose range is not wider than both bands; a close at a price below
    /// the closer's maintenance. The epoch table and the quiet time of a close are the caller's.
    pub fn pending_of(&self, k: &CKit, n: i64, kind: i64, side: i64, a: i64, t0: i64, proposer: [u8; 32]) -> Option<(CPending, i64)> {
        if !self.split_ok(n) || a < 0 {
            return None;
        }
        let p = a.min(self.m.p.price_max);
        let restore = self.restore_state(n);
        let (mut cure_bound, mut cure_hash, mut forfeit_hash, mut fee, mut fb) = (0, [0u8; 32], [0u8; 32], 0, if side == LONG { self.bl } else { self.bs });
        let mut who = proposer;
        if kind == KIND_CLOSE {
            if !self.close_ok(side, p) {
                return None;
            }
            who = if side == LONG { self.long_key } else { self.short_key };
        } else if kind == KIND_LIQ {
            if self.long_key == self.short_key {
                return None;
            }
            let trig = if side == LONG { self.long_liquidatable(p) } else { self.short_liquidatable(p) };
            if !trig {
                return None;
            }
            fee = self.fee_of(n, side, p);
            fb = self.forfeit_bound_of(n, side, fee);
            forfeit_hash = self.forfeit_state(n, side, fb).script_hash(k);
            if let Some(cb) = self.cure_bound_of(side, p) {
                cure_bound = cb;
                cure_hash = self.cure_state(n, side, cb).script_hash(k);
            }
        } else {
            return None;
        }
        Some((
            CPending {
                m: self.m,
                kind,
                side,
                n,
                bl: self.bl,
                bs: self.bs,
                quote: p,
                t0,
                cure_bound,
                fee,
                refund: if n < self.size { self.m.p.carrier } else { 0 },
                proposer: who,
                entry: self.entry,
                long_key: self.long_key,
                short_key: self.short_key,
                expiry: self.expiry,
                long_ask: self.long_ask,
                long_min: self.long_min,
                long_tip: self.long_tip,
                short_bid: self.short_bid,
                short_min: self.short_min,
                short_tip: self.short_tip,
                restore_hash: restore.script_hash(k),
                cure_hash,
                forfeit_hash,
                mk_parent: [0; 32],
            },
            fb,
        ))
    }
    /// `KpfPairC.propose` (review C1): a claim whose signed price is more than `lateDaa` older than the pair UTXO (accepted at
    /// `born`) is certainly void and is refused.
    pub fn price_fresh(&self, t0: i64, born: u64) -> bool {
        t0 as i128 + self.m.p.late_daa as i128 >= born as i128
    }
    /// The least value of a pending part `propose` writes from a pair of `value`: `value` for the whole pair,
    /// `q(n, bs - bl) + refund` for a part (the proposer funds the part's carrier).
    pub fn pending_value(&self, value: u64, w: &CPending) -> i64 {
        if w.n == self.size {
            value as i64
        } else {
            self.range_of(w.n) + w.refund
        }
    }
    /// The least value of the remainder of a split of n from a pair of `value`.
    pub fn rest_value(&self, value: u64, n: i64) -> i64 {
        (q(self.size - n, self.bs - self.bl, self.m.p.scale, 0) + self.m.p.carrier).max(value as i64 - self.range_of(n))
    }
}

// ---------------------------------------------------------------- the pending part

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CPending {
    pub m: CMarket,
    pub kind: i64,
    pub side: i64,
    pub n: i64,
    pub bl: i64,
    pub bs: i64,
    pub quote: i64,
    pub t0: i64,
    pub cure_bound: i64,
    pub fee: i64,
    pub refund: i64,
    pub proposer: [u8; 32],
    pub entry: i64,
    pub long_key: [u8; 32],
    pub short_key: [u8; 32],
    pub expiry: i64,
    pub long_ask: i64,
    pub long_min: i64,
    pub long_tip: i64,
    pub short_bid: i64,
    pub short_min: i64,
    pub short_tip: i64,
    pub restore_hash: [u8; 32],
    pub cure_hash: [u8; 32],
    pub forfeit_hash: [u8; 32],
    pub mk_parent: [u8; 32],
}

/// What `KpfPendingC.finalize` does with a part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CSettlement {
    /// A valid close: the two payouts (0: none).
    Close { long: i64, short: i64 },
    /// A valid liquidation: the forfeited part with at least `value`, and the proposer's fee.
    Forfeit { part: CPair, value: i64, fee: i64 },
    /// A void claim: the part restored with at least `value`.
    Restore { part: CPair, value: i64 },
    /// A valid trim: the pair with the side's bound moved, holding at least `value`, and `take` to the side's owner.
    Trim { part: CPair, value: i64, take: i64 },
}

impl CPending {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend([self.kind, self.side, self.n, self.bl, self.bs, self.quote, self.t0, self.cure_bound, self.fee, self.refund].map(F::I));
        f.extend([F::B(self.proposer), F::I(self.entry), F::B(self.long_key), F::B(self.short_key), F::I(self.expiry)]);
        f.extend([self.long_ask, self.long_min, self.long_tip, self.short_bid, self.short_min, self.short_tip].map(F::I));
        f.extend([F::B(self.restore_hash), F::B(self.cure_hash), F::B(self.forfeit_hash), F::B(self.mk_parent)]);
        f
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    /// `KpfPendingC` has no build constants.
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        to_args(&self.fields())
    }
    pub fn redeem(&self, k: &CKit) -> Vec<u8> {
        k.pending.redeem(&self.state_bytes())
    }
    pub fn decode_state(b: &[u8]) -> Result<CPending> {
        let mut c = Cursor { b, at: 0 };
        let m = CMarket::read(&mut c)?;
        let w = CPending {
            m,
            kind: c.int()?,
            side: c.int()?,
            n: c.int()?,
            bl: c.int()?,
            bs: c.int()?,
            quote: c.int()?,
            t0: c.int()?,
            cure_bound: c.int()?,
            fee: c.int()?,
            refund: c.int()?,
            proposer: c.b32()?,
            entry: c.int()?,
            long_key: c.b32()?,
            short_key: c.b32()?,
            expiry: c.int()?,
            long_ask: c.int()?,
            long_min: c.int()?,
            long_tip: c.int()?,
            short_bid: c.int()?,
            short_min: c.int()?,
            short_tip: c.int()?,
            restore_hash: c.b32()?,
            cure_hash: c.b32()?,
            forfeit_hash: c.b32()?,
            mk_parent: c.b32()?,
        };
        c.done()?;
        Ok(w)
    }
    /// The pair state the part came from (a part of size n, with the listings it had).
    pub fn pair_part(&self) -> CPair {
        CPair {
            m: self.m,
            size: self.n,
            bl: self.bl,
            bs: self.bs,
            entry: self.entry,
            long_key: self.long_key,
            short_key: self.short_key,
            expiry: self.expiry,
            long_ask: self.long_ask,
            long_min: self.long_min,
            long_tip: self.long_tip,
            short_bid: self.short_bid,
            short_min: self.short_min,
            short_tip: self.short_tip,
            mk: Makers::default(),
        }
    }
    /// This pending part as `propose` writes it from the pair input at outpoint `op` (its maker field).
    pub fn made_by(&self, op: &TransactionOutpoint) -> CPending {
        CPending { mk_parent: maker_of(op), ..self.clone() }
    }
    /// The claim's validity at P (the part's own DAA score): it landed within `lateDaa` of its time.
    pub fn valid_at(&self, p: i64) -> bool {
        p as i128 - self.t0 as i128 <= self.m.p.late_daa as i128
    }
    /// The last DAA at which this claim can land and be valid.
    pub fn last_valid_landing(&self) -> i64 {
        self.t0 + self.m.p.late_daa
    }
    /// The close price of a close claim: the attested price moved by g against the closer, clamped.
    pub fn close_price(&self) -> i64 {
        let (b, g) = (BPS as i128, self.m.p.g_bps as i128);
        let pc = if self.side == LONG { self.quote as i128 * (b - g) / b } else { (self.quote as i128 * (b + g) + b - 1) / b };
        (pc as i64).clamp(self.bl, self.bs)
    }
    /// The age this part needs before finalize when it landed at `p`: `graceDaa` for a valid liquidation, none for a
    /// close, a trim or a void claim.
    pub fn finalize_age(&self, p: i64) -> u64 {
        if self.kind != KIND_LIQ || !self.valid_at(p) {
            0
        } else {
            self.m.p.grace_daa.max(0) as u64
        }
    }
    /// What finalize does with a part of `value` whose P is `p` (`KpfPendingC.finalize`).
    pub fn settlement(&self, value: u64, p: i64) -> CSettlement {
        let v = value as i64;
        let part = self.pair_part();
        if !self.valid_at(p) {
            return CSettlement::Restore { part: part.restore_state(self.n), value: v };
        }
        if self.kind == KIND_CLOSE {
            let c = self.close_price();
            let sc = self.m.p.scale;
            let owe = if self.side == LONG { q(self.n, c - self.bl, sc, 0) } else { q(self.n, self.bs - c, sc, 0) };
            let (mut me, mut other) = (owe + self.refund, v - owe - self.refund);
            if other < PLAIN_DUST {
                me = v;
                other = 0;
            }
            if self.side == LONG {
                CSettlement::Close { long: me, short: other }
            } else {
                CSettlement::Close { long: other, short: me }
            }
        } else if self.kind == KIND_TRIM {
            let take = part.trim_take(self.side, self.cure_bound);
            CSettlement::Trim { part: part.trim_state(self.side, self.cure_bound), value: v - take, take }
        } else {
            let fb = part.forfeit_bound_of(self.n, self.side, self.fee);
            CSettlement::Forfeit { part: part.forfeit_state(self.n, self.side, fb), value: v - self.fee, fee: self.fee }
        }
    }
    /// The cure (`KpfPendingC.cure`): the cured part and its least value from a part of `value`; `None` when there is
    /// no cure.
    pub fn cure(&self, value: u64) -> Option<(CPair, i64)> {
        if self.kind != KIND_LIQ || self.cure_hash == [0; 32] {
            return None;
        }
        let part = self.pair_part();
        let sc = self.m.p.scale;
        let (c, add) = if self.side == LONG {
            (part.cure_state(self.n, LONG, self.cure_bound), q(self.n, self.bl - self.cure_bound, sc, sc - 1))
        } else {
            (part.cure_state(self.n, SHORT, self.cure_bound), q(self.n, self.cure_bound - self.bs, sc, sc - 1))
        };
        let floor = q(self.n, c.bs - c.bl, sc, 0) + self.m.p.carrier;
        Some((c, floor.max(value as i64 - self.refund + add)))
    }
}

/// The byte offsets of `KpfEpochTable.attest`'s arguments in its signature script (what `KpfPairC.priced` reads): the
/// market push `[0..33)`, t0 `[33..42)`, p `[42..51)`, bits `[51..60)`, the prices `[60..182)`, the signatures
/// `[182..1145)`, the dispatch tag `[1145..1150)`, the redeem script's push header `[1150..1153)`.
pub const ATTEST_OFFSETS: [usize; 8] = [0, 33, 42, 51, 60, 182, 1145, 1150];

/// The sample states the pinned artifacts are compiled with (a template does not depend on its state).
pub mod samples {
    use super::*;
    use crate::committee::{Charge, Exit, Committee, CommitteeParams, EpochTable, EMPTY_TABLE};

    pub fn committee_params() -> CommitteeParams {
        CommitteeParams::testnet()
    }
    pub fn epoch_table() -> EpochTable {
        let p = committee_params();
        EpochTable { members: EMPTY_TABLE, total: 0, from: 0, until: p.epoch, quorum_bps: p.quorum_bps, spread_bps: p.spread_bps }
    }
    pub fn exit() -> Exit {
        Exit::of([7; 32], 1, 0, &committee_params())
    }
    pub fn charge() -> Charge {
        Charge::new([1; 32], [7; 32], [8; 32], [2; 32], 0, 1, 1, committee_params().charge)
    }
    pub fn committee() -> Committee {
        Committee::empty(committee_params(), 0)
    }
    pub fn market() -> CMarket {
        CMarket::testnet([0x52; 32], [0xfe; 32])
    }
    pub fn pair(m: &CMarket) -> CPair {
        CPair {
            m: *m,
            size: 100_000_000,
            bl: 900_000_000,
            bs: 1_200_000_000,
            entry: 1_000_000_000,
            long_key: [1; 32],
            short_key: [2; 32],
            expiry: 6_048_000,
            long_ask: 0,
            long_min: 0,
            long_tip: 0,
            short_bid: 0,
            short_min: 0,
            short_tip: 0,
            mk: Makers::default(),
        }
    }
    pub fn pending(p: &CPair) -> CPending {
        CPending {
            m: p.m,
            kind: KIND_CLOSE,
            side: LONG,
            n: p.size,
            bl: p.bl,
            bs: p.bs,
            quote: p.entry,
            t0: 0,
            cure_bound: 0,
            fee: 0,
            refund: 0,
            proposer: p.long_key,
            entry: p.entry,
            long_key: p.long_key,
            short_key: p.short_key,
            expiry: p.expiry,
            long_ask: 0,
            long_min: 0,
            long_tip: 0,
            short_bid: 0,
            short_min: 0,
            short_tip: 0,
            restore_hash: [0; 32],
            cure_hash: [0; 32],
            forfeit_hash: [0; 32],
            mk_parent: [0; 32],
        }
    }
    pub fn order(m: &CMarket, side: i64, owner: [u8; 32]) -> COpen {
        COpen { m: *m, side, owner, size_left: 100_000_000, limit: 1_000_000_000, my_rate: 100_000_000, min_counter_rate: 0, min_fill: 1_000_000, tip: 0, active_from: 0, expiry_daa: 1_000_000 }
    }
}
