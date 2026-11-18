//! State layout of KpfPair and KpfOpen (DESIGN 4.2): the byte windows the contracts hard-code are where the
//! compiler puts those fields, and an instance spliced from the template equals the compiled one.

mod common;
mod kpf;

use common::*;
use kpf::*;

#[test]
fn pair_state_span_and_windows() {
    let base = pair_base();
    let c = &base.contracts.values().next().unwrap().compiled;
    // The state starts at redeem-script byte 1 and is 438 bytes: market block 207, size..tags 222, since 9 (review 3).
    assert_eq!(c.state_span.offset, 1);
    assert_eq!(c.state_span.len, 438);
    let p = pair(&keypair(1), &keypair(2));
    let bc = bytecode(base);
    let rd = |at: usize| i64::from_le_bytes(bc[at..at + 8].try_into().unwrap());
    // KpfPair constants BL_AT = 218, BS_AT = 227, LISTS_AT = 310, LISTS_END = 364.
    assert_eq!(rd(209), p.size);
    assert_eq!(rd(218), p.bl);
    assert_eq!(rd(227), p.bs);
    assert_eq!(rd(236), p.entry);
    assert_eq!(&bc[245..277], &p.long_key);
    assert_eq!(&bc[278..310], &p.short_key);
    assert_eq!(bc[310], 0x08);
    assert_eq!(bc[364], 0x20);
    assert_eq!(&bc[365..397], &p.tag_l.as_bytes());
    assert_eq!(&bc[398..430], &p.tag_s.as_bytes());
    // The since window [430..439) (KpfPair STATE_END = 439).
    assert_eq!(bc[430], 0x08);
    assert_eq!(rd(431), p.since);
}

#[test]
fn open_state_span_and_windows() {
    let base = open_base();
    let c = &base.contracts.values().next().unwrap().compiled;
    assert_eq!(c.state_span.offset, 1);
    assert_eq!(c.state_span.len, 321);
    let o = long_order(&keypair(1));
    let bc = bytecode(base);
    // KpfOpen constants MARKET_AT = 1, MARKET_LEN = 207, LEFT_AT = 251.
    assert_eq!(&bc[1..208], &pair_base_market());
    assert_eq!(i64::from_le_bytes(bc[251..259].try_into().unwrap()), o.size_left);
}

fn pair_base_market() -> Vec<u8> {
    bytecode(pair_base())[1..208].to_vec()
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
}
