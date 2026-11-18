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
    // The state starts at redeem-script byte 1 and is 429 bytes: market block 207, size..tags 222.
    assert_eq!(c.state_span.offset, 1);
    assert_eq!(c.state_span.len, 429);
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
