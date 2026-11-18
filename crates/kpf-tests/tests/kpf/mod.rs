//! Fixtures for the KpfPair and KpfOpen tests: state structs, artifacts, and the q rule.
//!
//! Each template is compiled once (with a sample state); an instance with another state is the template prefix,
//! that state in its canonical push encoding (8-byte ints behind 0x08, 32-byte values behind 0x20) and the
//! suffix. `splice_equals_compile` checks that this is exactly what the compiler emits for the same constructor
//! arguments.
#![allow(dead_code)]

pub mod evidence;
pub mod kob;
pub mod paths;

use std::sync::OnceLock;

use kaspa_consensus_core::Hash;
use secp256k1::Keypair;
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

use crate::common::*;

pub const SCALE: i64 = 1_000_000; // 6 decimals (DESIGN 5 worked example)
pub const C: i64 = 20_000_000; // 0.2 KAS carrier
pub const IDLE: i64 = 77_760_000; // 90 days of DAA
pub const PRICE_MAX: i64 = 100_000_000_000_000; // 10^14
pub const MIN_SIZE: i64 = 1_000_000; // one token
pub const ZERO: Hash = Hash::from_bytes([0; 32]);

pub fn xonly(kp: &Keypair) -> [u8; 32] {
    kp.x_only_public_key().0.serialize()
}

/// q(n, r) rounding down (c = 0) or up (c = scale - 1), in i128 so the reference never overflows.
pub fn q(n: i64, r: i64, scale: i64, c: i64) -> i64 {
    let v = (n / scale) as i128 * r as i128 + (n % scale) as i128 * (r / scale) as i128 + (((n % scale) as i128 * (r % scale) as i128) + c as i128) / scale as i128;
    i64::try_from(v).expect("q fits")
}

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
}

/// The market of the tests: a token of KOB's reference KCC-20 program (`vendor/kob/contracts/kcc20/KCC20Ref.sil`),
/// so that evidence custodies and deliveries run the real token program.
pub fn market() -> Market {
    let (h, pre, suf) = kob::tpl_facts(kob::kcc20_base());
    market_of(Hash::from_bytes(h), pre, suf)
}

pub fn market_of(token_tpl_hash: Hash, tpl_prefix_len: i64, tpl_suffix_len: i64) -> Market {
    Market {
        token_cov_id: Hash::from_bytes([0xc1; 32]),
        token_tpl_hash,
        ext: Hash::from_bytes([0xc3; 32]),
        tpl_prefix_len,
        tpl_suffix_len,
        scale: SCALE,
        mmr_bps: 500,
        k_bps: 10_000,
        min_rest_daa: 600,
        liq_fee_bps: 50,
        keeper_fee_min: 1_000_000,
        min_size: MIN_SIZE,
        carrier: C,
        idle_daa: IDLE,
        price_max: PRICE_MAX,
    }
}

/// A state field: an 8-byte int or a 32-byte value.
enum F {
    I(i64),
    B([u8; 32]),
}

fn encode(fields: &[F]) -> Vec<u8> {
    let mut v = Vec::new();
    for f in fields {
        match f {
            F::I(x) => {
                // Script-number form padded to 8 bytes: magnitude little-endian, sign in the top bit.
                v.push(0x08);
                let mut b = x.unsigned_abs().to_le_bytes();
                if *x < 0 {
                    b[7] |= 0x80;
                }
                v.extend_from_slice(&b);
            }
            F::B(b) => {
                v.push(0x20);
                v.extend_from_slice(b);
            }
        }
    }
    v
}

fn to_args(fields: &[F]) -> Vec<ArtifactValue> {
    fields.iter().map(|f| match f {
        F::I(x) => ArtifactValue::Int(*x),
        F::B(b) => ArtifactValue::Bytes(b.to_vec()),
    }).collect()
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
        ]
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
        ]);
        f
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    /// The constructor arguments: the KOB evidence template constants, then the state.
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        let mut a = kob::kob_consts();
        a.extend(to_args(&self.fields()));
        a
    }
    /// q(s, bs - bl) + C: what every pair output must hold.
    pub fn floor(&self) -> i64 {
        q(self.size, self.bs - self.bl, self.m.scale, 0) + self.m.carrier
    }
    pub fn art(&self) -> SilAbiArtifact {
        splice(pair_base(), &self.state_bytes())
    }
}

/// The worked example of DESIGN 5: 1,000 tokens, X = 10 KAS, long rate 1 KAS, short rate 2 KAS.
pub fn pair(long: &Keypair, short: &Keypair) -> Pair {
    Pair {
        m: market(),
        size: 1_000_000_000,
        bl: 900_000_000,
        bs: 1_200_000_000,
        entry: 1_000_000_000,
        long_key: xonly(long),
        short_key: xonly(short),
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        tag_l: Hash::from_bytes([0x1a; 32]),
        tag_s: Hash::from_bytes([0x1b; 32]),
    }
}

pub fn pair_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| compile("KpfPair.sil", &pair(&keypair(1), &keypair(2)).ctor()))
}

/// The template prefix and suffix of a compiled artifact (around its state span).
pub fn parts(art: &SilAbiArtifact) -> (Vec<u8>, Vec<u8>) {
    let c = &art.contracts.values().next().unwrap().compiled;
    let bc = &c.bytecode;
    let (pre, _, suf) = c.script_parts(bc).expect("parts");
    (pre.to_vec(), suf.to_vec())
}

pub fn template_hash(art: &SilAbiArtifact) -> [u8; 32] {
    art.contracts.values().next().unwrap().compiled.template_hash
}

pub fn splice(base: &SilAbiArtifact, state: &[u8]) -> SilAbiArtifact {
    let (pre, suf) = parts(base);
    let mut a = base.clone();
    let c = a.contracts.values_mut().next().unwrap();
    assert_eq!(state.len(), c.compiled.state_span.len, "state length");
    c.compiled.bytecode = [pre, state.to_vec(), suf].concat();
    a
}

// ---------------------------------------------------------------- KpfOpen

pub const LONG: i64 = 1;
pub const SHORT: i64 = 2;

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
    pub fn state_bytes(&self) -> Vec<u8> {
        encode(&self.fields())
    }
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        let mut a = vec![ArtifactValue::Bytes(template_hash(pair_base()).to_vec())];
        a.extend(to_args(&self.fields()));
        a
    }
    pub fn art(&self) -> SilAbiArtifact {
        splice(open_base(), &self.state_bytes())
    }
    /// Escrow of DESIGN 4.2: q(sizeLeft, myRate + tip) + C + carrier, rounded up.
    pub fn escrow(&self) -> i64 {
        q(self.size_left, self.my_rate + self.tip, self.m.scale, self.m.scale - 1) + 2 * self.m.carrier
    }
}

/// A long order of 2,000 tokens at <= 10.5 KAS, rate 1 KAS, accepting a short rate >= 1.5 KAS.
pub fn long_order(owner: &Keypair) -> Open {
    Open {
        m: market(),
        side: LONG,
        owner: xonly(owner),
        size_left: 2_000_000_000,
        limit: 1_050_000_000,
        my_rate: 100_000_000,
        min_counter_rate: 150_000_000,
        min_fill: 100_000_000,
        tip: 1_000_000,
        active_from: 0,
        expiry_daa: 1_000_000_000,
    }
}

/// A short order of 1,000 tokens at >= 9.5 KAS, rate 2 KAS, accepting a long rate >= 0.5 KAS.
pub fn short_order(owner: &Keypair) -> Open {
    Open {
        m: market(),
        side: SHORT,
        owner: xonly(owner),
        size_left: 1_000_000_000,
        limit: 950_000_000,
        my_rate: 200_000_000,
        min_counter_rate: 50_000_000,
        min_fill: 100_000_000,
        tip: 1_000_000,
        active_from: 0,
        expiry_daa: 1_000_000_000,
    }
}

pub fn open_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| compile("KpfOpen.sil", &long_order(&keypair(1)).ctor()))
}

pub fn args(v: &[ArtifactValue]) -> Vec<Arg> {
    v.iter().cloned().map(Arg::V).collect()
}

pub fn bytes(b: &[u8]) -> ArtifactValue {
    ArtifactValue::Bytes(b.to_vec())
}
