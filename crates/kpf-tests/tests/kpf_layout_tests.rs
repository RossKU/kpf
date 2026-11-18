//! State layout of KpfPair, KpfOpen and KpfPending (DESIGN 4.2, 4.5d): the byte windows the contracts hard-code are
//! where the compiler puts those fields, and an instance spliced from the template equals the compiled one.

mod common;
mod kpf;

use common::*;
use kpf::*;

#[test]
fn pair_state_span_and_windows() {
    let base = pair_base();
    let c = &base.contracts.values().next().unwrap().compiled;
    // The state starts at redeem-script byte 1 and is 492 bytes: market block 261 (207 + the six two-step parameters of
    // DESIGN 4.5d), size..tags 222, since 9.
    assert_eq!(c.state_span.offset, 1);
    assert_eq!(c.state_span.len, 492);
    let p = pair(&keypair(1), &keypair(2));
    let bc = bytecode(base);
    let rd = |at: usize| i64::from_le_bytes(bc[at..at + 8].try_into().unwrap());
    // The two-step parameters at the end of the market block (KpfPair MARKET_END = 262).
    for (i, v) in [p.m.bond_bps, p.m.bond_min, p.m.kc_bps, p.m.g_bps, p.m.challenge_daa, p.m.min_evidence_life].into_iter().enumerate() {
        assert_eq!(rd(208 + 9 * i + 1), v, "two-step parameter {i}");
    }
    // KpfPair constants SIZE_AT = 263, BL_AT = 272, BS_AT = 281, LISTS_AT = 364, LISTS_END = 418.
    assert_eq!(rd(263), p.size);
    assert_eq!(rd(272), p.bl);
    assert_eq!(rd(281), p.bs);
    assert_eq!(rd(290), p.entry);
    assert_eq!(&bc[299..331], &p.long_key);
    assert_eq!(&bc[332..364], &p.short_key);
    assert_eq!(bc[364], 0x08);
    assert_eq!(bc[418], 0x20);
    assert_eq!(&bc[419..451], &p.tag_l.as_bytes());
    assert_eq!(&bc[452..484], &p.tag_s.as_bytes());
    // The since window [484..493) (KpfPair STATE_END = 493).
    assert_eq!(bc[484], 0x08);
    assert_eq!(rd(485), p.since);
}

#[test]
fn open_state_span_and_windows() {
    let base = open_base();
    let c = &base.contracts.values().next().unwrap().compiled;
    assert_eq!(c.state_span.offset, 1);
    assert_eq!(c.state_span.len, 375);
    let o = long_order(&keypair(1));
    let bc = bytecode(base);
    // KpfOpen constants MARKET_AT = 1, MARKET_LEN = 261, LEFT_AT = 305.
    assert_eq!(&bc[1..262], &pair_base_market());
    assert_eq!(i64::from_le_bytes(bc[305..313].try_into().unwrap()), o.size_left);
}

fn pair_base_market() -> Vec<u8> {
    bytecode(pair_base())[1..262].to_vec()
}

/// KpfPending (DESIGN 4.5d): its state is the pair's market block, the claim, the pair's entry, keys and listings in
/// the pair's own order (one slice of the pair's script, `rs.slice(BS_AT + 8, LISTS_END)`), the clock S, the
/// proposer and the three hashes: 621 bytes, from byte 1. KpfPair.propose writes exactly this sequence, so the pair's
/// [BS_AT + 8, LISTS_END) window must land at pending bytes [352, 481).
#[test]
fn pending_state_span_and_windows() {
    let base = kpf_protocol::artifacts::pending_base();
    let c = &base.contracts.values().next().unwrap().compiled;
    assert_eq!((c.state_span.offset, c.state_span.len), (1, 621));
    let bc = bytecode(base);
    assert_eq!(&bc[1..262], &pair_base_market()[..], "the pending's market block is the pair's");
    let rd = |at: usize| i64::from_le_bytes(bc[at..at + 8].try_into().unwrap());
    // kind, side, n, bl, bs, quote, cureBound, fee, bond, refund (the sample of kpf-artifacts).
    let want = [2, 1, 1_000_000_000, 900_000_000, 1_200_000_000, 940_000_000, 893_000_000, 4_700_000_000, 9_000_000_000, 9_000_000_000];
    for (i, v) in want.into_iter().enumerate() {
        assert_eq!(bc[262 + 9 * i], 0x08);
        assert_eq!(rd(263 + 9 * i), v, "claim field {i}");
    }
    // The pair's entry .. short listing window, byte for byte the shape of the pair's [290 - 1, 418).
    let pair_win = &bytecode(pair_base())[289..418];
    assert_eq!(bc[352], pair_win[0]);
    assert_eq!(bc[352 + 9], 0x20);
    assert_eq!(bc[352 + 129], 0x08, "since follows the window");
    assert_eq!(rd(482), 1, "since of the sample");
    assert_eq!(bc[490], 0x20, "proposer key");
    assert_eq!(&bc[524..556], &[0x2a; 32], "restore hash");
    assert_eq!(&bc[557..589], &[0x2b; 32], "cure hash");
    assert_eq!(&bc[590..622], &[0x2c; 32], "forfeit hash");
}
/// The KobAsk and KobBid state offsets KpfPair.evQuote hard-codes (relative to the state span), checked against
/// the real KOB templates compiled from `vendor/kob` with a distinct value in every field (review 2, F2 added tif
/// and expiryDaa). If KOB's layout changes, this fails. Also the soft-expiry constants KpfPair repeats.
#[test]
fn kob_evidence_offsets_match_the_compiled_templates() {
    use kpf::kob::*;
    let rd = |st: &[u8], at: usize| i64::from_le_bytes(st[at..at + 8].try_into().unwrap());
    let state = |art: &silverscript_abi::SilAbiArtifact| {
        let c = &art.contracts.values().next().unwrap().compiled;
        c.bytecode[c.state_span.offset..c.state_span.offset + c.state_span.len].to_vec()
    };
    let mut a = ask_of(&market(), [9; 32], 1_111, 2_222);
    (a.scale, a.tif, a.active_from, a.expiry_daa, a.interval, a.slope) = (3_333, 4_444, 5_555, 6_666, 7_777, 8_888);
    let st = state(&compile_src("KobAsk", &kob_source("orders/KobAsk.sil"), &a.ctor()));
    assert_eq!(st.len(), 276);
    assert_eq!(&st[34..66], &a.token_cov_id.as_bytes());
    for (at, v) in [(118, a.scale), (136, a.price), (154, a.tif), (163, a.active_from), (172, a.expiry_daa), (190, a.interval), (208, a.slope)] {
        assert_eq!(rd(&st, at), v, "KobAsk offset {at}");
    }
    let mut b = bid_of(&market(), [9; 32], 1_111);
    (b.scale, b.tif, b.active_from, b.expiry_daa, b.interval, b.slope) = (3_333, 4_444, 5_555, 6_666, 7_777, 8_888);
    let st = state(&compile_src("KobBid", &kob_source("orders/KobBid.sil"), &b.ctor()));
    assert_eq!(st.len(), 285);
    assert_eq!(&st[34..66], &b.token_cov_id.as_bytes());
    assert_eq!(&st[118..150], &b.ext.as_bytes());
    for (at, v) in [(151, b.scale), (169, b.price), (187, b.tif), (196, b.active_from), (205, b.expiry_daa), (241, b.interval), (259, b.slope)] {
        assert_eq!(rd(&st, at), v, "KobBid offset {at}");
    }
    for rel in ["orders/KobAsk.sil", "orders/KobBid.sil"] {
        let src = kob_source(rel);
        assert!(src.contains("int constant IOC_LIFE = 600;"), "{rel}: IOC_LIFE");
        assert!(src.contains("int constant MAX_IDLE = 77760000;"), "{rel}: MAX_IDLE");
        assert!(src.contains("int constant TIF_GTC = 0;"), "{rel}: TIF_GTC");
    }
    let pair_src = contract_source("KpfPair.sil");
    assert!(pair_src.contains("int constant IOC_LIFE = 600;") && pair_src.contains("int constant MAX_IDLE = 77760000;"));
}

#[test]
fn splice_equals_compile() {
    let mut p = pair(&keypair(3), &keypair(4));
    p.bl = 0;
    p.entry = -5;
    p.long_ask = 77;
    p.tag_s = ZERO;
    assert_eq!(bytecode(&p.art()), bytecode(&compile("KpfPair.sil", &p.ctor())));
    let mut o = short_order(&keypair(5));
    o.size_left = 123_456_789;
    assert_eq!(bytecode(&o.art()), bytecode(&compile("KpfOpen.sil", &o.ctor())));
    // A pending part as KpfPair.propose writes it (the library's state of a real claim).
    let l = kpf::evidence::live(&pair(&keypair(3), &keypair(4)), 0x11).at(1_000);
    let g = kpf::evidence::PairLeg { l: l.clone(), n: 400_000_000, q: 0, act: kpf::evidence::Act::Liq { side: LONG } };
    let w = kpf::evidence::pend(&g, 940_000_000).w;
    assert_eq!(bytecode(&w.art()), bytecode(&compile("KpfPending.sil", &w.ctor())));
}
