//! Test support: fixed TEST-ONLY keys, the DESIGN 5 worked-example market, orders and pairs.
//!
//! The secret keys here are well-known test vectors (`[seed; 32]`). They exist so tests and golden vectors are
//! reproducible. Never fund them on a network.

use kaspa_consensus_core::Hash;

use crate::state::{Market, Open, Pair, LONG, SHORT};
use crate::tx::{pubkey_of, KeySet};

pub const SCALE: i64 = 1_000_000; // 6 decimals
pub const C: i64 = 20_000_000; // 0.2 KAS carrier

/// TEST-ONLY secret key `[seed; 32]`.
pub fn secret(seed: u8) -> [u8; 32] {
    [seed; 32]
}

/// x-only public key of the TEST-ONLY key `seed`.
pub fn pubkey(seed: u8) -> [u8; 32] {
    pubkey_of(&secret(seed)).expect("test key")
}

/// A key set of the TEST-ONLY keys.
pub fn keyset(seeds: &[u8]) -> KeySet {
    let mut k = KeySet::new();
    for s in seeds {
        k.add_secret(&secret(*s)).expect("test key");
    }
    k
}

/// The worked-example market of DESIGN 5 (arbitrary fixed token identity; evidence is not exercised here).
pub fn market() -> Market {
    Market {
        token_cov_id: Hash::from_bytes([0xc1; 32]),
        token_tpl_hash: Hash::from_bytes([0xc2; 32]),
        ext: Hash::from_bytes([0xc3; 32]),
        tpl_prefix_len: 1,
        tpl_suffix_len: 1,
        scale: SCALE,
        mmr_bps: 500,
        k_bps: 10_000,
        min_rest_daa: 600,
        liq_fee_bps: 50,
        keeper_fee_min: 1_000_000,
        min_size: 1_000_000,
        carrier: C,
        idle_daa: 77_760_000,
        price_max: 100_000_000_000_000,
    }
}

/// A long order of 2,000 tokens at <= 10.5 KAS, rate 1 KAS, accepting a short rate >= 1.5 KAS.
pub fn long_order(owner: [u8; 32]) -> Open {
    Open {
        m: market(),
        side: LONG,
        owner,
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
pub fn short_order(owner: [u8; 32]) -> Open {
    Open {
        m: market(),
        side: SHORT,
        owner,
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

/// The worked example of DESIGN 5: 1,000 tokens, X = 10 KAS, long rate 1 KAS, short rate 2 KAS, unlisted, no tags.
pub fn pair(long: [u8; 32], short: [u8; 32]) -> Pair {
    Pair {
        m: market(),
        size: 1_000_000_000,
        bl: 900_000_000,
        bs: 1_200_000_000,
        entry: 1_000_000_000,
        long_key: long,
        short_key: short,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        tag_l: Hash::default(),
        tag_s: Hash::default(),
        since: 0,
    }
}

// ---------------------------------------------------------------- KOB evidence fixtures

/// The worked-example market on KOB's reference KCC-20 program: the real template hash and cut of the token, so that
/// evidence custodies and deliveries run the real token script (tokenCovId and the extension commitment are fixed
/// placeholders, as everywhere in the fixtures).
pub fn kob_market() -> Market {
    let t = crate::artifacts::token_template();
    let mut m = market();
    m.token_tpl_hash = Hash::from_bytes(t.hash);
    m.tpl_prefix_len = t.prefix.len() as i64;
    m.tpl_suffix_len = t.suffix.len() as i64;
    m
}

/// A GTC ask of `amount` base units at `price` on the token of `m`, resting since DAA 0 (no decay, no TWAP).
pub fn ask_of(m: &Market, maker: [u8; 32], price: i64, amount: i64) -> crate::kob::AskState {
    crate::kob::AskState {
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

/// A GTC bid at `price` on the token of `m` (minimum fill one whole token, 0.2 KAS delivery carrier).
pub fn bid_of(m: &Market, maker: [u8; 32], price: i64) -> crate::kob::BidState {
    crate::kob::BidState {
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
        delivery_carrier: C,
        interval: 0,
        max_fill: 0,
        slope: 0,
        price_end: 0,
        decay_step: 1,
    }
}

/// KAS a bid of `amount` base units at `price` is funded with: its budget, its delivery carriers for a fill and a
/// continuation, one minimum fill of room and 1 KAS.
pub fn bid_escrow(b: &crate::kob::BidState, amount: i64) -> u64 {
    (b.used(amount) + 2 * b.delivery_carrier + b.used(b.min_fill) + 100_000_000) as u64
}
