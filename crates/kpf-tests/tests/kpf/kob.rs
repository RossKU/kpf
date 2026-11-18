//! KOB's real contracts, compiled from their place in the read-only checkout `vendor/kob/contracts`: the plain
//! orders `KobAsk` and `KobBid` (price evidence, DESIGN 3.3) and the reference KCC-20 program `KCC20Ref` (the
//! token of the market, its custody and its deliveries). Each template is compiled once with a sample state; an
//! instance with another state is the template prefix, that state in its canonical push encoding and the suffix,
//! as for the KPF templates (`splice`).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use kaspa_consensus_core::Hash;
use silverscript_abi::{ArtifactValue, SilAbiArtifact};
use silverscript_lang::compiler::{compile_to_sil_abi_artifact_with_options, CompileOptions};

use super::*;
use crate::common::*;

/// The evidence template constants KOB itself pins in `KobCondAsk` (`vendor/kob/contracts/orders/KobCondAsk.ctor.json`:
/// ASK_TPL, ASK_PRE, ASK_SUF, BID_TPL, BID_PRE, BID_SUF). The templates compiled here must hash to them.
pub const KOB_ASK_TPL: [u8; 32] = [
    7, 11, 179, 178, 66, 92, 200, 0, 192, 46, 46, 148, 101, 36, 26, 139, 148, 171, 220, 233, 62, 113, 47, 211, 186, 241, 148, 116, 182, 59, 136, 124,
];
pub const KOB_ASK_PRE: i64 = 1;
pub const KOB_ASK_SUF: i64 = 1407;
pub const KOB_BID_TPL: [u8; 32] = [
    223, 205, 120, 241, 92, 218, 220, 203, 125, 182, 248, 9, 76, 180, 36, 33, 164, 9, 100, 42, 139, 40, 4, 21, 24, 166, 189, 46, 197, 75, 213, 239,
];
pub const KOB_BID_PRE: i64 = 1;
pub const KOB_BID_SUF: i64 = 1343;

/// Source of `vendor/kob/contracts/<rel>`.
pub fn kob_source(rel: &str) -> String {
    let path = kpf_root().join("vendor/kob/contracts").join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).replace("\r\n", "\n")
}

pub fn compile_src(name: &str, src: &str, ctor: &[ArtifactValue]) -> SilAbiArtifact {
    compile_to_sil_abi_artifact_with_options(src, ctor, CompileOptions::default()).unwrap_or_else(|e| panic!("compile {name}: {e:?}"))
}

fn b32(b: &[u8; 32]) -> ArtifactValue {
    ArtifactValue::Bytes(b.to_vec())
}

/// Script-number form of an int padded to 8 bytes (what `byte[8]` arguments and 8-byte state pushes carry).
pub fn sm8(x: i64) -> [u8; 8] {
    let mut b = x.unsigned_abs().to_le_bytes();
    if x < 0 {
        b[7] |= 0x80;
    }
    b
}

fn push_i(v: &mut Vec<u8>, x: i64) {
    v.push(0x08);
    v.extend_from_slice(&sm8(x));
}

fn push_b(v: &mut Vec<u8>, b: &[u8; 32]) {
    v.push(0x20);
    v.extend_from_slice(b);
}

// ---------------------------------------------------------------- KCC-20 (KCC20Ref)

/// A KCC-20 holder state (112 bytes encoded).
#[derive(Clone, Debug, PartialEq)]
pub struct Tok {
    pub amount: i64,
    pub owner: [u8; 32],
    pub scheme: u8,
    pub borrow: u8,
    pub guard: [u8; 32],
    pub ext: Hash,
}

pub const SCHEME_P2PK: u8 = 0x00;
pub const SCHEME_COVID: u8 = 0x04;

impl Tok {
    pub fn p2pk(amount: i64, owner: [u8; 32], ext: Hash) -> Tok {
        Tok { amount, owner, scheme: SCHEME_P2PK, borrow: 0, guard: [0; 32], ext }
    }
    pub fn covid(amount: i64, owner: Hash, ext: Hash) -> Tok {
        Tok { amount, owner: owner.as_bytes(), scheme: SCHEME_COVID, borrow: 0, guard: [0; 32], ext }
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut v = vec![];
        push_i(&mut v, self.amount);
        push_b(&mut v, &self.owner);
        v.extend_from_slice(&[0x01, self.scheme, 0x01, self.borrow]);
        push_b(&mut v, &self.guard);
        push_b(&mut v, &self.ext.as_bytes());
        v
    }
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        vec![
            ArtifactValue::Int(self.amount),
            b32(&self.owner),
            ArtifactValue::Byte(self.scheme),
            ArtifactValue::Byte(self.borrow),
            b32(&self.guard),
            b32(&self.ext.as_bytes()),
        ]
    }
    /// A `next_states` element of the KCC-20 leader's `transfer`.
    pub fn value(&self) -> ArtifactValue {
        let mut m = BTreeMap::new();
        m.insert("amount".to_string(), ArtifactValue::Int(self.amount));
        m.insert("owner".to_string(), b32(&self.owner));
        m.insert("owner_scheme".to_string(), ArtifactValue::Byte(self.scheme));
        m.insert("borrow_scheme".to_string(), ArtifactValue::Byte(self.borrow));
        m.insert("borrow_guard".to_string(), b32(&self.guard));
        m.insert("extension_commitment".to_string(), b32(&self.ext.as_bytes()));
        ArtifactValue::Object(m)
    }
    pub fn art(&self) -> SilAbiArtifact {
        splice(kcc20_base(), &self.state_bytes())
    }
}

pub fn kcc20_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| {
        let t = Tok::covid(1000, Hash::from_bytes([3; 32]), Hash::from_bytes([0xee; 32]));
        compile_src("KCC20Ref", &kob_source("kcc20/KCC20Ref.sil"), &t.ctor())
    })
}

// ---------------------------------------------------------------- KobAsk

#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub maker: [u8; 32],
    pub token_cov_id: Hash,
    pub token_tpl_hash: Hash,
    pub pre: i64,
    pub suf: i64,
    pub scale: i64,
    pub min_fill: i64,
    pub price: i64,
    pub tip: i64,
    pub tif: i64,
    pub active_from: i64,
    pub expiry_daa: i64,
    pub refund_tip: i64,
    pub interval: i64,
    pub max_fill: i64,
    pub slope: i64,
    pub price_end: i64,
    pub decay_step: i64,
    pub amount_left: i64,
    pub ext: Hash,
}

impl Ask {
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut v = vec![];
        push_b(&mut v, &self.maker);
        push_b(&mut v, &self.token_cov_id.as_bytes());
        push_b(&mut v, &self.token_tpl_hash.as_bytes());
        for x in [
            self.pre,
            self.suf,
            self.scale,
            self.min_fill,
            self.price,
            self.tip,
            self.tif,
            self.active_from,
            self.expiry_daa,
            self.refund_tip,
            self.interval,
            self.max_fill,
            self.slope,
            self.price_end,
            self.decay_step,
            self.amount_left,
        ] {
            push_i(&mut v, x);
        }
        push_b(&mut v, &self.ext.as_bytes());
        v
    }
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        let mut a = vec![b32(&self.maker), b32(&self.token_cov_id.as_bytes()), b32(&self.token_tpl_hash.as_bytes())];
        for x in [
            self.pre,
            self.suf,
            self.scale,
            self.min_fill,
            self.price,
            self.tip,
            self.tif,
            self.active_from,
            self.expiry_daa,
            self.refund_tip,
            self.interval,
            self.max_fill,
            self.slope,
            self.price_end,
            self.decay_step,
            self.amount_left,
        ] {
            a.push(ArtifactValue::Int(x));
        }
        a.push(b32(&self.ext.as_bytes()));
        a
    }
    pub fn art(&self) -> SilAbiArtifact {
        splice(ask_base(), &self.state_bytes())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bid {
    pub maker: [u8; 32],
    pub token_cov_id: Hash,
    pub token_tpl_hash: Hash,
    pub pre: i64,
    pub suf: i64,
    pub ext: Hash,
    pub scale: i64,
    pub min_fill: i64,
    pub price: i64,
    pub tip: i64,
    pub tif: i64,
    pub active_from: i64,
    pub expiry_daa: i64,
    pub refund_tip: i64,
    pub reserve: i64,
    pub delivery_carrier: i64,
    pub interval: i64,
    pub max_fill: i64,
    pub slope: i64,
    pub price_end: i64,
    pub decay_step: i64,
}

impl Bid {
    fn ints(&self) -> [i64; 15] {
        [
            self.scale,
            self.min_fill,
            self.price,
            self.tip,
            self.tif,
            self.active_from,
            self.expiry_daa,
            self.refund_tip,
            self.reserve,
            self.delivery_carrier,
            self.interval,
            self.max_fill,
            self.slope,
            self.price_end,
            self.decay_step,
        ]
    }
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut v = vec![];
        push_b(&mut v, &self.maker);
        push_b(&mut v, &self.token_cov_id.as_bytes());
        push_b(&mut v, &self.token_tpl_hash.as_bytes());
        push_i(&mut v, self.pre);
        push_i(&mut v, self.suf);
        push_b(&mut v, &self.ext.as_bytes());
        for x in self.ints() {
            push_i(&mut v, x);
        }
        v
    }
    pub fn ctor(&self) -> Vec<ArtifactValue> {
        let mut a = vec![
            b32(&self.maker),
            b32(&self.token_cov_id.as_bytes()),
            b32(&self.token_tpl_hash.as_bytes()),
            ArtifactValue::Int(self.pre),
            ArtifactValue::Int(self.suf),
            b32(&self.ext.as_bytes()),
        ];
        for x in self.ints() {
            a.push(ArtifactValue::Int(x));
        }
        a
    }
    pub fn art(&self) -> SilAbiArtifact {
        splice(bid_base(), &self.state_bytes())
    }
}

/// A GTC ask of `amount` base units at `price` on the token of `m`, resting since forever (activeFrom 0).
pub fn ask_of(m: &Market, maker: [u8; 32], price: i64, amount: i64) -> Ask {
    Ask {
        maker,
        token_cov_id: m.token_cov_id,
        token_tpl_hash: m.token_tpl_hash,
        pre: m.tpl_prefix_len,
        suf: m.tpl_suffix_len,
        scale: m.scale,
        min_fill: 1,
        price,
        tip: 0,
        tif: 0,
        active_from: 0,
        expiry_daa: 1_000_000_000_000,
        refund_tip: 0,
        interval: 0,
        max_fill: 0,
        slope: 0,
        price_end: 0,
        decay_step: 1,
        amount_left: amount,
        ext: m.ext,
    }
}

/// A GTC bid at `price` on the token of `m`.
pub fn bid_of(m: &Market, maker: [u8; 32], price: i64) -> Bid {
    Bid {
        maker,
        token_cov_id: m.token_cov_id,
        token_tpl_hash: m.token_tpl_hash,
        pre: m.tpl_prefix_len,
        suf: m.tpl_suffix_len,
        ext: m.ext,
        scale: m.scale,
        min_fill: 1_000_000,
        price,
        tip: 0,
        tif: 0,
        active_from: 0,
        expiry_daa: 1_000_000_000_000,
        refund_tip: 0,
        reserve: 0,
        delivery_carrier: 20_000_000,
        interval: 0,
        max_fill: 0,
        slope: 0,
        price_end: 0,
        decay_step: 1,
    }
}

fn sample_market() -> Market {
    let mut m = market_of(Hash::from_bytes([0xc2; 32]), 1, 1);
    m.ext = Hash::from_bytes([0xee; 32]);
    m
}

pub fn ask_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| compile_src("KobAsk", &kob_source("orders/KobAsk.sil"), &ask_of(&sample_market(), [3; 32], 250_000_000, 10).ctor()))
}

pub fn bid_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| compile_src("KobBid", &kob_source("orders/KobBid.sil"), &bid_of(&sample_market(), [3; 32], 250_000_000).ctor()))
}

/// A look-alike of KobAsk: the same source with one constant changed (the IOC kill), hence the same state layout
/// and behaviour for a GTC fill, but another template hash.
pub fn fake_ask_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| {
        let src = kob_source("orders/KobAsk.sil");
        let fake = src.replace("int constant IOC_LIFE = 600;", "int constant IOC_LIFE = 601;");
        assert_ne!(src, fake);
        compile_src("KobAsk look-alike", &fake, &ask_of(&sample_market(), [3; 32], 250_000_000, 10).ctor())
    })
}

/// (template hash, prefix length, suffix length) of a compiled template.
pub fn tpl_facts(art: &SilAbiArtifact) -> ([u8; 32], i64, i64) {
    let (pre, suf) = parts(art);
    (template_hash(art), pre.len() as i64, suf.len() as i64)
}

/// The KpfPair build constants: the KobAsk and KobBid templates.
pub fn kob_consts() -> Vec<ArtifactValue> {
    let (at, ap, asf) = tpl_facts(ask_base());
    let (bt, bp, bsf) = tpl_facts(bid_base());
    vec![b32(&at), ArtifactValue::Int(ap), ArtifactValue::Int(asf), b32(&bt), ArtifactValue::Int(bp), ArtifactValue::Int(bsf)]
}
