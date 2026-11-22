//! Fixtures for the KpfPair and KpfOpen tests: the market, sample pairs and orders, the pinned artifacts.
//!
//! The state structs, their encoding, the `q` rule and the artifact splicing live in `kpf-protocol`
//! (`crates/kpf-protocol`); this module re-exports them and adds only what is specific to the engine tests (a market
//! on KOB's reference token program, pairs and orders keyed by `secp256k1::Keypair`). `splice_equals_compile` (in
//! `kpf_layout_tests`) still checks that a spliced instance is exactly what the compiler emits for the same
//! constructor arguments, so the library's codec is held to the compiler on every run.
#![allow(dead_code)]

pub mod evidence;
pub mod kob;
pub mod paths;

use kaspa_consensus_core::Hash;
use secp256k1::Keypair;
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

use crate::common::*;

pub use kpf_protocol::artifacts::splice;
pub use kpf_protocol::payoff::q;
#[allow(unused_imports)]
pub use kpf_protocol::state::{Lock, Market, Open, Pair, Pending, LONG, SHORT, ZERO};

pub const SCALE: i64 = 1_000_000; // 6 decimals (DESIGN 5 worked example)
pub const C: i64 = 20_000_000; // 0.2 KAS carrier
pub const IDLE: i64 = 77_760_000; // 90 days of DAA
pub const PRICE_MAX: i64 = 100_000_000_000_000; // 10^14
pub const MIN_SIZE: i64 = 1_000_000; // one token

pub fn xonly(kp: &Keypair) -> [u8; 32] {
    kp.x_only_public_key().0.serialize()
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
        bond_bps: 300,
        bond_min: 500_000_000,
        kc_bps: 10_000,
        g_bps: 200,
        challenge_daa: 6_000,
        min_evidence_life: 864_000,
        lock_bps: 11_000,
        dispute_daa: 6_600,
        grace_daa: 1_200,
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
        since: 0,
    }
}

/// The pinned `KpfPair` artifact (`contracts/legacy/artifacts`); equal to a fresh compile (`cargo test -p kpf-artifacts`).
pub fn pair_base() -> &'static SilAbiArtifact {
    kpf_protocol::artifacts::pair_base()
}

/// The pinned `KpfOpen` artifact.
pub fn open_base() -> &'static SilAbiArtifact {
    kpf_protocol::artifacts::open_base()
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

/// A long order of 2,000 tokens at <= 10.5 KAS, rate 1 KAS, accepting a short rate >= 1.5 KAS.
pub fn long_order(owner: &Keypair) -> Open {
    kpf_protocol::fixtures::long_order(xonly(owner)).with_market(market())
}

/// A short order of 1,000 tokens at >= 9.5 KAS, rate 2 KAS, accepting a long rate >= 0.5 KAS.
pub fn short_order(owner: &Keypair) -> Open {
    kpf_protocol::fixtures::short_order(xonly(owner)).with_market(market())
}

pub fn args(v: &[ArtifactValue]) -> Vec<Arg> {
    v.iter().cloned().map(Arg::V).collect()
}

pub fn bytes(b: &[u8]) -> ArtifactValue {
    ArtifactValue::Bytes(b.to_vec())
}
