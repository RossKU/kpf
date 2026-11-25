//! State of the two contracts: structs, canonical encoding, decoding and validation.
//!
//! The state of an instance is a fixed sequence of canonical pushes, in the order of the contract's fields
//! (DESIGN 4.2): an integer is `0x08` + 8 bytes (script number, magnitude little-endian, sign in the top bit), a key
//! or hash is `0x20` + 32 bytes. [`Pair::decode_redeem`] and [`Open::decode_redeem`] take a whole redeem script, check
//! it against the pinned template and decode the state; `encode(decode(x)) == x` holds for every accepted script.
//!
//! `validate` applies the bounds the contracts enforce at their entries (`checkPair`, `checkMarket`, DESIGN 5), so a
//! builder refuses a state its contract would refuse. They are necessary, not sufficient: some contract checks depend
//! on the transaction.

use kaspa_consensus_core::tx::ScriptPublicKey;
use kaspa_consensus_core::Hash;
use kaspa_txscript::pay_to_script_hash_script;
use serde_json::{json, Value};
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

use crate::artifacts::{self, kob_consts, lock_template, open_template, pair_template, pending_template};
use crate::error::{invalid, to_hex, Error, Result};
use crate::payoff::q;

pub const LONG: i64 = 1;
pub const SHORT: i64 = 2;

/// Contract bounds (DESIGN 5; the constants of `KpfPair.sil` and `KpfOpen.sil`).
pub const SCALE_MAX: i64 = 1_000_000_000;
pub const PRICE_CAP: i64 = 100_000_000_000_000;
pub const K_MAX: i64 = 100_000;
pub const MMR_CAP: i64 = 5_000;
pub const BPS: i64 = 10_000;
pub const CSV_LIMIT: i64 = 4_294_967_296;
pub const LOCK_TIME_THRESHOLD: i64 = 500_000_000_000;
/// 90 days of DAA at 10 per second: an order untouched this long is refundable by anyone.
pub const MAX_IDLE: u64 = 77_760_000;
/// A plain payout below 0.02 KAS is not created (DESIGN 4.6).
pub const PLAIN_DUST: i64 = 2_000_000;
/// The largest bond floor (`KpfOpen.BOND_MIN_MAX`, 10,000 KAS; DESIGN 4.5e).
pub const BOND_MIN_MAX: i64 = 1_000_000_000_000;
/// The largest lock ratio (`KpfOpen.LOCK_BPS_MAX`, `KpfPending.LOCK_BPS_MAX`; DESIGN 4.5f): 3.
pub const LOCK_BPS_MAX: i64 = 30_000;
/// Kinds of a pending claim (DESIGN 4.5d).
pub const KIND_CLOSE: i64 = 1;
pub const KIND_LIQ: i64 = 2;
/// A trim (committee markets, `KpfPairC.propose` kind 3): a side takes out the margin it has above maintenance.
pub const KIND_TRIM: i64 = 3;

pub const ZERO: Hash = Hash::from_bytes([0; 32]);

/// A state field: an 8-byte int or a 32-byte value.
pub(crate) enum F {
    I(i64),
    B([u8; 32]),
}

/// The number 8 bytes read as (`int(byte[8](..))` of the scripts): little-endian, the top bit the sign.
pub fn int8(b: &[u8]) -> i64 {
    let mut m = [0u8; 8];
    m.copy_from_slice(&b[..8]);
    let neg = m[7] & 0x80 != 0;
    m[7] &= 0x7f;
    let v = i64::from_le_bytes(m);
    if neg { -v } else { v }
}

/// Script-number form of an int padded to 8 bytes (what 8-byte state pushes and `byte[8]` arguments carry).
pub fn sm8(x: i64) -> [u8; 8] {
    let mut b = x.unsigned_abs().to_le_bytes();
    if x < 0 {
        b[7] |= 0x80;
    }
    b
}

pub(crate) fn encode(fields: &[F]) -> Vec<u8> {
    let mut v = Vec::new();
    for f in fields {
        match f {
            F::I(x) => {
                v.push(0x08);
                v.extend_from_slice(&sm8(*x));
            }
            F::B(b) => {
                v.push(0x20);
                v.extend_from_slice(b);
            }
        }
    }
    v
}

pub(crate) fn to_args(fields: &[F]) -> Vec<ArtifactValue> {
    fields
        .iter()
        .map(|f| match f {
            F::I(x) => ArtifactValue::Int(*x),
            F::B(b) => ArtifactValue::Bytes(b.to_vec()),
        })
        .collect()
}

pub(crate) struct Cursor<'a> {
    pub(crate) b: &'a [u8],
    pub(crate) at: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn int(&mut self) -> Result<i64> {
        let s = self.b.get(self.at..self.at + 9).ok_or_else(|| Error::Decode("state ends inside an integer".into()))?;
        if s[0] != 0x08 {
            return Err(Error::Decode(format!("byte {}: expected an 8-byte integer push (0x08), found {:#04x}", self.at, s[0])));
        }
        let mut m = [0u8; 8];
        m.copy_from_slice(&s[1..9]);
        let neg = m[7] & 0x80 != 0;
        m[7] &= 0x7f;
        let mag = u64::from_le_bytes(m);
        if mag > i64::MAX as u64 || (neg && mag == 0) {
            return Err(Error::Decode(format!("byte {}: integer is not in canonical form", self.at)));
        }
        self.at += 9;
        Ok(if neg { -(mag as i64) } else { mag as i64 })
    }
    pub(crate) fn b32(&mut self) -> Result<[u8; 32]> {
        let s = self.b.get(self.at..self.at + 33).ok_or_else(|| Error::Decode("state ends inside a 32-byte value".into()))?;
        if s[0] != 0x20 {
            return Err(Error::Decode(format!("byte {}: expected a 32-byte push (0x20), found {:#04x}", self.at, s[0])));
        }
        self.at += 33;
        Ok(s[1..33].try_into().expect("32 bytes"))
    }
    pub(crate) fn hash(&mut self) -> Result<Hash> {
        Ok(Hash::from_bytes(self.b32()?))
    }
    pub(crate) fn done(&self) -> Result<()> {
        if self.at != self.b.len() {
            return Err(Error::Decode(format!("{} trailing state bytes", self.b.len() - self.at)));
        }
        Ok(())
    }
}

/// The six KOB evidence constants as constructor arguments (`KpfPair` and `KpfPending` both start with them).
fn kob_args(k: &crate::artifacts::KobConsts) -> Vec<ArtifactValue> {
    vec![
        ArtifactValue::Bytes(k.ask_tpl.to_vec()),
        ArtifactValue::Int(k.ask_pre),
        ArtifactValue::Int(k.ask_suf),
        ArtifactValue::Bytes(k.bid_tpl.to_vec()),
        ArtifactValue::Int(k.bid_pre),
        ArtifactValue::Int(k.bid_suf),
    ]
}

fn check_key(key: &[u8; 32], what: &str) -> Result<()> {
    secp256k1::XOnlyPublicKey::from_slice(key).map(|_| ()).map_err(|_| Error::Invalid(format!("{what} is not a valid x-only public key")))
}

// ---------------------------------------------------------------- market

/// The market block (state bytes 0..207), identical in every pair and open order of one market (DESIGN 4.2).
#[derive(Clone, Debug, PartialEq)]
pub struct Market {
    pub token_cov_id: Hash,
    pub token_tpl_hash: Hash,
    pub ext: Hash,
    pub tpl_prefix_len: i64,
    pub tpl_suffix_len: i64,
    pub scale: i64,
    pub mmr_bps: i64,
    pub k_bps: i64,
    pub min_rest_daa: i64,
    pub liq_fee_bps: i64,
    pub keeper_fee_min: i64,
    pub min_size: i64,
    pub carrier: i64,
    pub idle_daa: i64,
    pub price_max: i64,
    /// Two-step evidence (DESIGN 4.5d): bond rate on the part's range, bond floor, counter-evidence multiple,
    /// contradiction margin, window W (earliest finalize) and the evidence life L.
    pub bond_bps: i64,
    pub bond_min: i64,
    pub kc_bps: i64,
    pub g_bps: i64,
    pub challenge_daa: i64,
    pub min_evidence_life: i64,
    /// Guaranteed fast settlement (DESIGN 4.5f): the lock ratio lambda (a lock holds at least `lockBps / 10^4` times
    /// the value it covers), the dispute window D after a fast settlement (also the release age of its lock), and the
    /// cure grace G a pending liquidation reaches before it can be fast-settled.
    pub lock_bps: i64,
    pub dispute_daa: i64,
    pub grace_daa: i64,
}

impl Market {
    fn fields(&self) -> Vec<F> {
        vec![
            F::B(self.token_cov_id.as_bytes()),
            F::B(self.token_tpl_hash.as_bytes()),
            F::B(self.ext.as_bytes()),
            F::I(self.tpl_prefix_len),
            F::I(self.tpl_suffix_len),
            F::I(self.scale),
            F::I(self.mmr_bps),
            F::I(self.k_bps),
            F::I(self.min_rest_daa),
            F::I(self.liq_fee_bps),
            F::I(self.keeper_fee_min),
            F::I(self.min_size),
            F::I(self.carrier),
            F::I(self.idle_daa),
            F::I(self.price_max),
            F::I(self.bond_bps),
            F::I(self.bond_min),
            F::I(self.kc_bps),
            F::I(self.g_bps),
            F::I(self.challenge_daa),
            F::I(self.min_evidence_life),
            F::I(self.lock_bps),
            F::I(self.dispute_daa),
            F::I(self.grace_daa),
        ]
    }

    fn read(c: &mut Cursor) -> Result<Market> {
        Ok(Market {
            token_cov_id: c.hash()?,
            token_tpl_hash: c.hash()?,
            ext: c.hash()?,
            tpl_prefix_len: c.int()?,
            tpl_suffix_len: c.int()?,
            scale: c.int()?,
            mmr_bps: c.int()?,
            k_bps: c.int()?,
            min_rest_daa: c.int()?,
            liq_fee_bps: c.int()?,
            keeper_fee_min: c.int()?,
            min_size: c.int()?,
            carrier: c.int()?,
            idle_daa: c.int()?,
            price_max: c.int()?,
            bond_bps: c.int()?,
            bond_min: c.int()?,
            kc_bps: c.int()?,
            g_bps: c.int()?,
            challenge_daa: c.int()?,
            min_evidence_life: c.int()?,
            lock_bps: c.int()?,
            dispute_daa: c.int()?,
            grace_daa: c.int()?,
        })
    }

    /// The bounds `KpfOpen.checkMarket` and `KpfPair.checkPair` / `propose` enforce on the market block.
    pub fn validate(&self) -> Result<()> {
        let req = |ok: bool, what: &str| if ok { Ok(()) } else { invalid(format!("market: {what}")) };
        req(self.scale > 0 && self.scale <= SCALE_MAX, "scale must be in 1..=10^9")?;
        req(self.price_max > 0 && self.price_max <= PRICE_CAP, "priceMax must be in 1..=10^14")?;
        req(self.carrier >= 0, "carrier must be >= 0")?;
        req(self.min_size > 0, "minSize must be > 0")?;
        req(self.idle_daa >= 0 && self.idle_daa < CSV_LIMIT, "idleDaa must be in 0..2^32")?;
        req(self.k_bps > 0 && self.k_bps <= K_MAX, "kBps must be in 1..=10^5")?;
        req(self.mmr_bps >= 0 && self.mmr_bps < MMR_CAP, "mmrBps must be in 0..5000")?;
        req(self.liq_fee_bps >= 0 && self.liq_fee_bps <= BPS, "liqFeeBps must be in 0..=10^4")?;
        req(self.keeper_fee_min >= 0, "keeperFeeMin must be >= 0")?;
        req(self.min_rest_daa >= 0 && self.min_rest_daa < LOCK_TIME_THRESHOLD, "minRestDaa must be in 0..5*10^11")?;
        req(self.min_rest_daa.saturating_mul(4) < LOCK_TIME_THRESHOLD, "4 * minRestDaa must be below 5*10^11 (the hand-over limit S + 4R is a lock time, DESIGN 4.5c)")?;
        req(self.tpl_prefix_len >= 0 && self.tpl_suffix_len >= 0, "token template cut must be >= 0")?;
        // The two-step evidence parameters (DESIGN 4.5d; KpfOpen.checkMarket, KpfPair.propose).
        req(self.bond_bps >= 0 && self.bond_bps <= BPS, "bondBps must be in 0..=10^4")?;
        req(self.bond_min >= 0 && self.bond_min <= BOND_MIN_MAX, "bondMin must be in 0..=10^12 (10,000 KAS)")?;
        req(self.kc_bps > 0 && self.kc_bps <= K_MAX, "kcBps must be in 1..=10^5")?;
        req(self.g_bps > 0 && self.g_bps < BPS, "gBps must be in 1..10^4")?;
        req(
            self.challenge_daa >= self.min_rest_daa.saturating_mul(2) && self.challenge_daa < CSV_LIMIT,
            "challengeDaa must be in 2 * minRestDaa..2^32 (the window leaves the defender at least R after the earliest challenge, DESIGN 4.5e)",
        )?;
        req(
            self.min_evidence_life >= 0 && self.min_evidence_life.saturating_add(self.min_rest_daa) <= MAX_IDLE as i64,
            "minEvidenceLife must be >= 0 with minEvidenceLife + minRestDaa <= 77,760,000",
        )?;
        // Guaranteed fast settlement (DESIGN 4.5f; KpfOpen.checkMarket, KpfPending.guarantee).
        req(self.lock_bps >= BPS && self.lock_bps <= LOCK_BPS_MAX, "lockBps must be in 10^4..=3*10^4 (a lock ratio of 1 to 3)")?;
        req(
            self.dispute_daa >= self.min_rest_daa.saturating_mul(2) && self.dispute_daa < CSV_LIMIT,
            "disputeDaa must be in 2 * minRestDaa..2^32 (a disputer whose evidence rested in time has R to land, DESIGN 4.5f)",
        )?;
        req(self.grace_daa >= 0 && self.grace_daa <= self.challenge_daa, "graceDaa must be in 0..=challengeDaa")?;
        Ok(())
    }

    /// The testnet defaults of the two-step parameters (DESIGN 4.5d): bond 300 bps of the part's range with a 5 KAS
    /// floor, `k_c = k`, `g` 200 bps, window 600 DAA (60 s), evidence life 864,000 DAA (one day).
    pub fn with_two_step_defaults(mut self) -> Market {
        self.bond_bps = 300;
        self.bond_min = 500_000_000;
        self.kc_bps = self.k_bps;
        self.g_bps = 200;
        self.challenge_daa = 600;
        self.min_evidence_life = 864_000;
        self.with_guarantee_defaults()
    }

    /// The testnet defaults of guaranteed fast settlement (DESIGN 4.5f), from the window W and the rest R already set:
    /// lambda 1.1 (11,000 bps; DESIGN-ORACLE 6, the smallest first version: a refuted claim then costs its guarantor
    /// 1.1 times what it moved, more than the 3 % bond the slow path risks), the dispute window D = W + R (counter-evidence
    /// must have rested by F + D - R >= P + W, so a victim has at least the slow path's window W from P), and the cure
    /// grace G = 2R (an online loser has 2R to cure before its liquidation can be fast-settled; at most W).
    pub fn with_guarantee_defaults(mut self) -> Market {
        self.lock_bps = 11_000;
        self.dispute_daa = self.challenge_daa.saturating_add(self.min_rest_daa);
        self.grace_daa = self.min_rest_daa.saturating_mul(2).min(self.challenge_daa);
        self
    }

    /// Reads the market block from its JSON form (the keys of [`Market::to_json`]).
    pub fn from_json(v: &Value) -> Result<Market> {
        let int = |k: &str| -> Result<i64> {
            v.get(k).and_then(Value::as_i64).ok_or_else(|| Error::Invalid(format!("market: `{k}` is missing or not an integer")))
        };
        let hash = |k: &str| -> Result<Hash> {
            let s = v.get(k).and_then(Value::as_str).ok_or_else(|| Error::Invalid(format!("market: `{k}` is missing or not a hex string")))?;
            Ok(Hash::from_bytes(crate::error::hex32(s).map_err(|e| Error::Invalid(format!("market: `{k}`: {e}")))?))
        };
        let m = Market {
            token_cov_id: hash("tokenCovId")?,
            token_tpl_hash: hash("tokenTplHash")?,
            ext: hash("extensionCommitment")?,
            tpl_prefix_len: int("tplPrefixLen")?,
            tpl_suffix_len: int("tplSuffixLen")?,
            scale: int("scale")?,
            mmr_bps: int("mmrBps")?,
            k_bps: int("kBps")?,
            min_rest_daa: int("minRestDaa")?,
            liq_fee_bps: int("liqFeeBps")?,
            keeper_fee_min: int("keeperFeeMin")?,
            min_size: int("minSize")?,
            carrier: int("carrier")?,
            idle_daa: int("idleDaa")?,
            price_max: int("priceMax")?,
            bond_bps: int("bondBps")?,
            bond_min: int("bondMin")?,
            kc_bps: int("kcBps")?,
            g_bps: int("gBps")?,
            challenge_daa: int("challengeDaa")?,
            min_evidence_life: int("minEvidenceLife")?,
            lock_bps: int("lockBps")?,
            dispute_daa: int("disputeDaa")?,
            grace_daa: int("graceDaa")?,
        };
        m.validate()?;
        Ok(m)
    }

    /// The largest `size / scale` the contracts accept: `(INT_MAX - carrier) / priceMax - 1` whole tokens.
    pub fn max_whole_tokens(&self) -> i64 {
        (i64::MAX - self.carrier) / self.price_max - 1
    }

    pub fn to_json(&self) -> Value {
        json!({
            "tokenCovId": to_hex(&self.token_cov_id.as_bytes()), "tokenTplHash": to_hex(&self.token_tpl_hash.as_bytes()),
            "extensionCommitment": to_hex(&self.ext.as_bytes()), "tplPrefixLen": self.tpl_prefix_len, "tplSuffixLen": self.tpl_suffix_len,
            "scale": self.scale, "mmrBps": self.mmr_bps, "kBps": self.k_bps, "minRestDaa": self.min_rest_daa,
            "liqFeeBps": self.liq_fee_bps, "keeperFeeMin": self.keeper_fee_min, "minSize": self.min_size,
            "carrier": self.carrier, "idleDaa": self.idle_daa, "priceMax": self.price_max,
            "bondBps": self.bond_bps, "bondMin": self.bond_min, "kcBps": self.kc_bps, "gBps": self.g_bps,
            "challengeDaa": self.challenge_daa, "minEvidenceLife": self.min_evidence_life,
            "lockBps": self.lock_bps, "disputeDaa": self.dispute_daa, "graceDaa": self.grace_daa,
        })
    }
}

// ---------------------------------------------------------------- health rules of review 3

/// The range condition of DESIGN 4.5c: `bl * (10^4 + mmrBps) < bs * (10^4 - mmrBps)`.
pub fn range_ok(m: &Market, bl: i64, bs: i64) -> bool {
    (bl as i128) * ((BPS + m.mmr_bps) as i128) < (bs as i128) * ((BPS - m.mmr_bps) as i128)
}

/// Health at entry of DESIGN 4.5c: a long with bound `bl` is not liquidatable at price x
/// (`x * (10^4 - mmrBps) >= bl * 10^4`).
pub fn long_healthy_at(m: &Market, bl: i64, x: i64) -> bool {
    (x as i128) * ((BPS - m.mmr_bps) as i128) >= (bl as i128) * (BPS as i128)
}

/// Health at entry of a short with bound `bs` at price x (`bs * 10^4 >= x * (10^4 + mmrBps)`).
pub fn short_healthy_at(m: &Market, bs: i64, x: i64) -> bool {
    (bs as i128) * (BPS as i128) >= (x as i128) * ((BPS + m.mmr_bps) as i128)
}

/// Health at entry of a side with bound `b` (bl for a long, bs for a short) at price x.
pub fn healthy_at(m: &Market, side: i64, b: i64, x: i64) -> bool {
    if side == LONG {
        long_healthy_at(m, b, x)
    } else {
        short_healthy_at(m, b, x)
    }
}

// ---------------------------------------------------------------- KpfPair

#[derive(Clone, Debug, PartialEq)]
pub struct Pair {
    pub m: Market,
    pub size: i64,
    pub bl: i64,
    pub bs: i64,
    pub entry: i64,
    pub long_key: [u8; 32],
    pub short_key: [u8; 32],
    pub long_ask: i64,
    pub long_min: i64,
    pub long_tip: i64,
    pub short_bid: i64,
    pub short_min: i64,
    pub short_tip: i64,
    pub tag_l: Hash,
    pub tag_s: Hash,
    /// Evidence clock start (DESIGN 4.5c): 0 means the DAA score of the pair's own UTXO.
    pub since: i64,
}

impl Pair {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend([
            F::I(self.size),
            F::I(self.bl),
            F::I(self.bs),
            F::I(self.entry),
            F::B(self.long_key),
            F::B(self.short_key),
            F::I(self.long_ask),
            F::I(self.long_min),
            F::I(self.long_tip),
            F::I(self.short_bid),
            F::I(self.short_min),
            F::I(self.short_tip),
            F::B(self.tag_l.as_bytes()),
            F::B(self.tag_s.as_bytes()),
            F::I(self.since),
        ]);
        f
    }

    /// The canonical encoded state (438 bytes).
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }

    /// The constructor arguments the compiler takes: the pinned KOB evidence constants, the `KpfPending` template
    /// hash, then the state.
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        self.ctor_with(&kob_consts(), pending_template().hash)
    }

    /// [`Pair::ctor`] with explicit KOB constants and pending template hash (the artifact generator, before any are
    /// pinned).
    pub fn ctor_with(&self, k: &crate::artifacts::KobConsts, pending_tpl: [u8; 32]) -> Vec<ArtifactValue> {
        let mut a = kob_args(k);
        a.push(ArtifactValue::Bytes(pending_tpl.to_vec()));
        a.extend(to_args(&self.fields()));
        a
    }

    /// The redeem script of this instance.
    pub fn redeem(&self) -> Vec<u8> {
        pair_template().redeem(&self.state_bytes())
    }

    /// The artifact of this instance (the pinned artifact with this state spliced in).
    pub fn art(&self) -> SilAbiArtifact {
        artifacts::splice(artifacts::pair_base(), &self.state_bytes())
    }

    /// The P2SH script public key of this instance.
    pub fn spk(&self) -> ScriptPublicKey {
        pay_to_script_hash_script(&self.redeem())
    }

    /// Decodes the state of a `KpfPair` redeem script (checked against the pinned template).
    pub fn decode_redeem(redeem: &[u8]) -> Result<Pair> {
        Pair::decode_state(pair_template().state_of(redeem)?)
    }

    /// Decodes an encoded state. Rejects a state that does not re-encode to the same bytes.
    pub fn decode_state(state: &[u8]) -> Result<Pair> {
        let mut c = Cursor { b: state, at: 0 };
        let m = Market::read(&mut c)?;
        let p = Pair {
            m,
            size: c.int()?,
            bl: c.int()?,
            bs: c.int()?,
            entry: c.int()?,
            long_key: c.b32()?,
            short_key: c.b32()?,
            long_ask: c.int()?,
            long_min: c.int()?,
            long_tip: c.int()?,
            short_bid: c.int()?,
            short_min: c.int()?,
            short_tip: c.int()?,
            tag_l: c.hash()?,
            tag_s: c.hash()?,
            since: c.int()?,
        };
        c.done()?;
        Ok(p)
    }

    /// `checkPair` and the field bounds of the entries (DESIGN 5), plus valid keys.
    pub fn validate(&self) -> Result<()> {
        self.m.validate()?;
        let req = |ok: bool, what: &str| if ok { Ok(()) } else { invalid(format!("pair: {what}")) };
        req(self.size > 0, "size must be > 0")?;
        req(self.size / self.m.scale <= self.m.max_whole_tokens(), "size exceeds the 63-bit bound of the contracts")?;
        req(self.bl >= 0 && self.bl < self.bs && self.bs <= self.m.price_max, "need 0 <= bl < bs <= priceMax")?;
        req(self.entry >= 0 && self.entry <= self.m.price_max, "entry must be in 0..=priceMax")?;
        req(self.since >= 0 && self.since < LOCK_TIME_THRESHOLD, "since must be in 0..5*10^11")?;
        for (n, v) in [("longAsk", self.long_ask), ("shortBid", self.short_bid)] {
            req(v >= 0 && v <= self.m.price_max, &format!("{n} must be in 0..=priceMax"))?;
        }
        for (n, v) in [("longMin", self.long_min), ("longTip", self.long_tip), ("shortMin", self.short_min), ("shortTip", self.short_tip)] {
            req(v >= 0, &format!("{n} must be >= 0"))?;
        }
        check_key(&self.long_key, "longKey")?;
        check_key(&self.short_key, "shortKey")?;
        Ok(())
    }

    /// q(s, bs - bl) + C: what every output of this pair must hold.
    pub fn floor(&self) -> i64 {
        q(self.size, self.bs - self.bl, self.m.scale, 0) + self.m.carrier
    }

    /// The evidence clock start S of this pair in a UTXO of DAA score `utxo_daa` (DESIGN 4.5c, `KpfPair.clockStart`):
    /// `since` when non-zero, else the UTXO's own DAA score.
    pub fn clock_start(&self, utxo_daa: u64) -> i64 {
        if self.since != 0 {
            self.since
        } else {
            utxo_daa as i64
        }
    }

    /// This state with `since` set to its clock start: the state every pair output that does not change hands
    /// carries (the continuation of amend and add margin, every remainder). The payoff functions build their outputs
    /// from a clocked pair.
    pub fn clocked(&self, utxo_daa: u64) -> Pair {
        let mut p = self.clone();
        p.since = self.clock_start(utxo_daa);
        p
    }

    /// The first DAA at which a hand-over (transfer, close-versus-close) of this pair is allowed:
    /// `S + 4 * minRestDaa` (DESIGN 4.5c).
    pub fn handover_from(&self, utxo_daa: u64) -> i64 {
        self.clock_start(utxo_daa).saturating_add(self.m.min_rest_daa.saturating_mul(4))
    }

    /// Whether the range is wider than both maintenance bands (DESIGN 4.5c, review 3 F2):
    /// `bl * (10^4 + mmrBps) < bs * (10^4 - mmrBps)`. A pair that fails it can never be liquidated.
    pub fn range_ok(&self) -> bool {
        range_ok(&self.m, self.bl, self.bs)
    }

    pub fn with_size(&self, s: i64) -> Pair {
        let mut p = self.clone();
        p.size = s;
        p
    }
    /// This state with both tags cleared: what every continuation that hands over no side carries (DESIGN 4.4).
    pub fn cleared(&self) -> Pair {
        let mut p = self.clone();
        p.tag_l = Hash::default();
        p.tag_s = Hash::default();
        p
    }
    /// The remainder of a split: size s, tags cleared, every other field kept.
    pub fn rest_of(&self, s: i64) -> Pair {
        self.with_size(s).cleared()
    }
    /// q(n, bs - bl), rounded down.
    pub fn range_of(&self, n: i64) -> i64 {
        q(n, self.bs - self.bl, self.m.scale, 0)
    }
    /// Long payout at price p (DESIGN 3.1), without the carrier.
    pub fn long_at(&self, p: i64) -> i64 {
        let c = p.clamp(self.bl, self.bs);
        q(self.size, c - self.bl, self.m.scale, 0)
    }
    pub fn short_at(&self, p: i64) -> i64 {
        let c = p.clamp(self.bl, self.bs);
        q(self.size, self.bs - c, self.m.scale, 0)
    }
    pub fn long_window(&self) -> (i64, i64, i64) {
        (self.long_ask, self.long_min, self.long_tip)
    }
    pub fn short_window(&self) -> (i64, i64, i64) {
        (self.short_bid, self.short_min, self.short_tip)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "contract": "KpfPair", "market": self.m.to_json(), "size": self.size, "bl": self.bl, "bs": self.bs, "entry": self.entry,
            "longKey": to_hex(&self.long_key), "shortKey": to_hex(&self.short_key),
            "longListing": {"ask": self.long_ask, "minPart": self.long_min, "tip": self.long_tip},
            "shortListing": {"bid": self.short_bid, "minPart": self.short_min, "tip": self.short_tip},
            "tagL": to_hex(&self.tag_l.as_bytes()), "tagS": to_hex(&self.tag_s.as_bytes()), "since": self.since,
            "floorValue": self.floor(),
        })
    }
}

// ---------------------------------------------------------------- KpfOpen

#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    pub m: Market,
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

impl Open {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend([
            F::I(self.side),
            F::B(self.owner),
            F::I(self.size_left),
            F::I(self.limit),
            F::I(self.my_rate),
            F::I(self.min_counter_rate),
            F::I(self.min_fill),
            F::I(self.tip),
            F::I(self.active_from),
            F::I(self.expiry_daa),
        ]);
        f
    }

    /// The canonical encoded state (321 bytes; unchanged by review 3).
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }

    /// The constructor arguments: the pair template hash, then the state.
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        self.ctor_with(pair_template().hash)
    }

    /// [`Open::ctor`] with an explicit pair template hash.
    pub fn ctor_with(&self, pair_tpl: [u8; 32]) -> Vec<ArtifactValue> {
        let mut a = vec![ArtifactValue::Bytes(pair_tpl.to_vec())];
        a.extend(to_args(&self.fields()));
        a
    }

    pub fn redeem(&self) -> Vec<u8> {
        open_template().redeem(&self.state_bytes())
    }

    pub fn art(&self) -> SilAbiArtifact {
        artifacts::splice(artifacts::open_base(), &self.state_bytes())
    }

    pub fn spk(&self) -> ScriptPublicKey {
        pay_to_script_hash_script(&self.redeem())
    }

    pub fn decode_redeem(redeem: &[u8]) -> Result<Open> {
        Open::decode_state(open_template().state_of(redeem)?)
    }

    pub fn decode_state(state: &[u8]) -> Result<Open> {
        let mut c = Cursor { b: state, at: 0 };
        let m = Market::read(&mut c)?;
        let o = Open {
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

    /// `checkMarket` and the order bounds a fill or take relies on.
    pub fn validate(&self) -> Result<()> {
        self.m.validate()?;
        let req = |ok: bool, what: &str| if ok { Ok(()) } else { invalid(format!("order: {what}")) };
        req(self.side == LONG || self.side == SHORT, "side must be 1 (long) or 2 (short)")?;
        check_key(&self.owner, "owner")?;
        req(self.my_rate > 0, "myRate must be > 0")?;
        req(self.min_counter_rate >= 0, "minCounterRate must be >= 0")?;
        req(self.tip >= 0, "tip must be >= 0")?;
        req(self.min_fill >= 0, "minFill must be >= 0")?;
        req(self.size_left >= self.m.min_size, "sizeLeft must be >= minSize")?;
        req(self.size_left / self.m.scale <= self.m.max_whole_tokens(), "sizeLeft exceeds the 63-bit bound of the contracts")?;
        req(self.limit >= 0 && self.limit <= self.m.price_max, "limit must be in 0..=priceMax")?;
        req(self.my_rate <= self.m.price_max, "myRate must be <= priceMax")?;
        req(self.active_from >= 0 && self.expiry_daa >= 0, "activeFrom and expiryDaa must be >= 0")?;
        Ok(())
    }

    /// This order in another market.
    pub fn with_market(mut self, m: Market) -> Open {
        self.m = m;
        self
    }

    /// Escrow of DESIGN 4.2: q(sizeLeft, myRate + tip) rounded up, plus the pair carrier a fill funds and the order's
    /// own carrier.
    pub fn escrow(&self) -> i64 {
        q(self.size_left, self.my_rate + self.tip, self.m.scale, self.m.scale - 1) + 2 * self.m.carrier
    }

    /// What a fill or take of `n` may spend from the escrow: its margin and tip for n (rounded down) and one carrier.
    pub fn spend_for(&self, n: i64) -> i64 {
        q(n, self.my_rate + self.tip, self.m.scale, 0) + self.m.carrier
    }

    pub fn to_json(&self) -> Value {
        json!({
            "contract": "KpfOpen", "market": self.m.to_json(), "side": if self.side == LONG { "long" } else { "short" },
            "owner": to_hex(&self.owner), "sizeLeft": self.size_left, "limit": self.limit, "myRate": self.my_rate,
            "minCounterRate": self.min_counter_rate, "minFill": self.min_fill, "tip": self.tip,
            "activeFrom": self.active_from, "expiryDaa": self.expiry_daa, "escrowValue": self.escrow(),
        })
    }
}

// ---------------------------------------------------------------- KpfPending

/// A part against which a claim on price evidence is pending (DESIGN 4.5d), written by `KpfPair.propose` and immutable.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub m: Market,
    /// [`KIND_CLOSE`] (P5) or [`KIND_LIQ`] (P6).
    pub kind: i64,
    /// The closer (kind 1) or the loser (kind 2).
    pub side: i64,
    pub n: i64,
    pub bl: i64,
    pub bs: i64,
    /// The proposal's evidence quote.
    pub quote: i64,
    /// Kind 2: the loser's bound after a cure (0 with a zero cure hash: no cure).
    pub cure_bound: i64,
    /// Kind 2: the keeper fee, to the proposer at finalize.
    pub fee: i64,
    pub bond: i64,
    /// `bond`, plus the carrier the proposer funded when the part is smaller than the pair.
    pub refund: i64,
    /// Guaranteed fast settlement (DESIGN 4.5f): the last DAA at which the proposal's evidence order was fillable by
    /// conforming matchers (`KpfPending.guarantee` requires the part's DAA score P to be at most this), and the part of
    /// its own payout the proposer gives a guarantor (0 ..= bond - 0.02 KAS).
    pub visible_until: i64,
    pub offer: i64,
    /// The pair's entry, keys, listings and clock start S when the claim was proposed (self-describing state; only
    /// the keys are read by the contract).
    pub entry: i64,
    pub long_key: [u8; 32],
    pub short_key: [u8; 32],
    pub long_ask: i64,
    pub long_min: i64,
    pub long_tip: i64,
    pub short_bid: i64,
    pub short_min: i64,
    pub short_tip: i64,
    pub since: i64,
    pub proposer_key: [u8; 32],
    /// P2SH script hashes of the pair states this part may become (restore, cure, forfeit; zero when none).
    pub restore_hash: [u8; 32],
    pub cure_hash: [u8; 32],
    pub forfeit_hash: [u8; 32],
}

impl Pending {
    fn fields(&self) -> Vec<F> {
        let mut f = self.m.fields();
        f.extend([
            F::I(self.kind),
            F::I(self.side),
            F::I(self.n),
            F::I(self.bl),
            F::I(self.bs),
            F::I(self.quote),
            F::I(self.cure_bound),
            F::I(self.fee),
            F::I(self.bond),
            F::I(self.refund),
            F::I(self.visible_until),
            F::I(self.offer),
            F::I(self.entry),
            F::B(self.long_key),
            F::B(self.short_key),
            F::I(self.long_ask),
            F::I(self.long_min),
            F::I(self.long_tip),
            F::I(self.short_bid),
            F::I(self.short_min),
            F::I(self.short_tip),
            F::I(self.since),
            F::B(self.proposer_key),
            F::B(self.restore_hash),
            F::B(self.cure_hash),
            F::B(self.forfeit_hash),
        ]);
        f
    }

    /// The canonical encoded state (666 bytes).
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }

    /// The constructor arguments: the pinned KOB evidence constants, then the state.
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        self.ctor_with(&kob_consts(), lock_template().hash)
    }

    /// [`Pending::ctor`] with explicit KOB constants and `KpfLock` template hash (the artifact generator).
    pub fn ctor_with(&self, k: &crate::artifacts::KobConsts, lock_tpl: [u8; 32]) -> Vec<ArtifactValue> {
        let mut a = kob_args(k);
        a.push(ArtifactValue::Bytes(lock_tpl.to_vec()));
        a.extend(to_args(&self.fields()));
        a
    }

    pub fn redeem(&self) -> Vec<u8> {
        pending_template().redeem(&self.state_bytes())
    }

    pub fn art(&self) -> SilAbiArtifact {
        artifacts::splice(artifacts::pending_base(), &self.state_bytes())
    }

    pub fn spk(&self) -> ScriptPublicKey {
        pay_to_script_hash_script(&self.redeem())
    }

    pub fn decode_redeem(redeem: &[u8]) -> Result<Pending> {
        Pending::decode_state(pending_template().state_of(redeem)?)
    }

    pub fn decode_state(state: &[u8]) -> Result<Pending> {
        let mut c = Cursor { b: state, at: 0 };
        let m = Market::read(&mut c)?;
        let p = Pending {
            m,
            kind: c.int()?,
            side: c.int()?,
            n: c.int()?,
            bl: c.int()?,
            bs: c.int()?,
            quote: c.int()?,
            cure_bound: c.int()?,
            fee: c.int()?,
            bond: c.int()?,
            refund: c.int()?,
            visible_until: c.int()?,
            offer: c.int()?,
            entry: c.int()?,
            long_key: c.b32()?,
            short_key: c.b32()?,
            long_ask: c.int()?,
            long_min: c.int()?,
            long_tip: c.int()?,
            short_bid: c.int()?,
            short_min: c.int()?,
            short_tip: c.int()?,
            since: c.int()?,
            proposer_key: c.b32()?,
            restore_hash: c.b32()?,
            cure_hash: c.b32()?,
            forfeit_hash: c.b32()?,
        };
        c.done()?;
        Ok(p)
    }

    /// Whether a cure exists (kind 2 with a non-zero cure hash).
    pub fn can_cure(&self) -> bool {
        self.kind == KIND_LIQ && self.cure_hash != [0u8; 32]
    }

    /// The owner of the threatened side: the loser (kind 2) or the party that did not close (kind 1).
    pub fn defender_key(&self) -> [u8; 32] {
        let other = if self.kind == KIND_CLOSE { if self.side == LONG { SHORT } else { LONG } } else { self.side };
        if other == LONG {
            self.long_key
        } else {
            self.short_key
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "contract": "KpfPending", "market": self.m.to_json(),
            "kind": if self.kind == KIND_CLOSE { "close" } else { "liquidate" },
            "side": if self.side == LONG { "long" } else { "short" },
            "n": self.n, "bl": self.bl, "bs": self.bs, "quote": self.quote, "cureBound": self.cure_bound,
            "fee": self.fee, "bond": self.bond, "refund": self.refund, "visibleUntil": self.visible_until, "offer": self.offer,
            "longKey": to_hex(&self.long_key), "shortKey": to_hex(&self.short_key), "proposerKey": to_hex(&self.proposer_key),
            "restoreHash": to_hex(&self.restore_hash), "cureHash": to_hex(&self.cure_hash), "forfeitHash": to_hex(&self.forfeit_hash),
            "challengeDaa": self.m.challenge_daa,
        })
    }
}

// ---------------------------------------------------------------- KpfLock

/// The bond status of a lock (the only one `KpfPending.guarantee` writes, DESIGN 4.5f).
pub const STATUS_LOCK: i64 = 1;

/// The guarantor's lock of a guaranteed fast settlement (DESIGN 4.5f), written by `KpfPending.guarantee` and immutable.
#[derive(Clone, Debug, PartialEq)]
pub struct Lock {
    /// The guarantor: a release pays it.
    pub owner: [u8; 32],
    /// [`STATUS_LOCK`].
    pub status: i64,
    /// The pending part's market block.
    pub m: Market,
    /// The covenant id of the pending part the lock settled.
    pub claim_id: Hash,
    pub kind: i64,
    pub side: i64,
    pub n: i64,
    pub bl: i64,
    pub bs: i64,
    pub quote: i64,
    /// P, the pending part's DAA score: the clock of the counter-evidence.
    pub clock: i64,
    /// V, the value the fast settlement moved away from the victim.
    pub cover: i64,
    /// The party the claim could wrong: the loser of a liquidation, the other party of a close.
    pub victim: [u8; 32],
}

impl Lock {
    fn fields(&self) -> Vec<F> {
        let mut f = vec![F::B(self.owner), F::I(self.status)];
        f.extend(self.m.fields());
        f.extend([
            F::B(self.claim_id.as_bytes()),
            F::I(self.kind),
            F::I(self.side),
            F::I(self.n),
            F::I(self.bl),
            F::I(self.bs),
            F::I(self.quote),
            F::I(self.clock),
            F::I(self.cover),
            F::B(self.victim),
        ]);
        f
    }

    /// The canonical encoded state (468 bytes).
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }

    /// The constructor arguments: the pinned KOB evidence constants, then the state.
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        self.ctor_with(&kob_consts())
    }

    pub fn ctor_with(&self, k: &crate::artifacts::KobConsts) -> Vec<ArtifactValue> {
        let mut a = kob_args(k);
        a.extend(to_args(&self.fields()));
        a
    }

    pub fn redeem(&self) -> Vec<u8> {
        lock_template().redeem(&self.state_bytes())
    }

    pub fn art(&self) -> SilAbiArtifact {
        artifacts::splice(artifacts::lock_base(), &self.state_bytes())
    }

    pub fn spk(&self) -> ScriptPublicKey {
        pay_to_script_hash_script(&self.redeem())
    }

    pub fn decode_redeem(redeem: &[u8]) -> Result<Lock> {
        Lock::decode_state(lock_template().state_of(redeem)?)
    }

    pub fn decode_state(state: &[u8]) -> Result<Lock> {
        let mut c = Cursor { b: state, at: 0 };
        let owner = c.b32()?;
        let status = c.int()?;
        let m = Market::read(&mut c)?;
        let l = Lock {
            owner,
            status,
            m,
            claim_id: c.hash()?,
            kind: c.int()?,
            side: c.int()?,
            n: c.int()?,
            bl: c.int()?,
            bs: c.int()?,
            quote: c.int()?,
            clock: c.int()?,
            cover: c.int()?,
            victim: c.b32()?,
        };
        c.done()?;
        Ok(l)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "contract": "KpfLock", "owner": to_hex(&self.owner), "status": self.status, "market": self.m.to_json(),
            "claimId": to_hex(&self.claim_id.as_bytes()),
            "kind": if self.kind == KIND_CLOSE { "close" } else { "liquidate" },
            "side": if self.side == LONG { "long" } else { "short" },
            "n": self.n, "bl": self.bl, "bs": self.bs, "quote": self.quote, "clock": self.clock, "cover": self.cover,
            "victim": to_hex(&self.victim), "disputeDaa": self.m.dispute_daa, "lockBps": self.m.lock_bps,
        })
    }
}

/// Which contract a redeem script is an instance of, decoded.
#[derive(Clone, Debug, PartialEq)]
pub enum Decoded {
    Pair(Pair),
    Open(Open),
    Pending(Pending),
    /// A guarantor's lock (DESIGN 4.5f).
    Lock(Lock),
}

/// Decodes a redeem script as a pair, an order, a pending part or a lock.
pub fn decode_redeem(redeem: &[u8]) -> Result<Decoded> {
    let pe = match Pair::decode_redeem(redeem) {
        Ok(p) => return Ok(Decoded::Pair(p)),
        Err(e) => e,
    };
    let oe = match Open::decode_redeem(redeem) {
        Ok(o) => return Ok(Decoded::Open(o)),
        Err(e) => e,
    };
    let we = match Pending::decode_redeem(redeem) {
        Ok(p) => return Ok(Decoded::Pending(p)),
        Err(e) => e,
    };
    match Lock::decode_redeem(redeem) {
        Ok(l) => Ok(Decoded::Lock(l)),
        Err(le) => Err(Error::Decode(format!("not a KpfPair ({pe}), not a KpfOpen ({oe}), not a KpfPending ({we}) and not a KpfLock ({le})"))),
    }
}
