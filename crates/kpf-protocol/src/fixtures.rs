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
    }
}
