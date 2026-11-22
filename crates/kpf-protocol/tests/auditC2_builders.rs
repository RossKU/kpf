//! Review C2 of the committee family (docs/audit/REVIEW-C2.md): the tool's own builders
//! (`kpf_protocol::committee_build`) on the pinned artifacts, every transaction signed, metered, balanced and passed
//! through `verify::verify` (the engine with consensus metering, the node's count of signature operations, the fee
//! floor, the storage mass, the block limits).
//!
//! `c2_fixed_*`: what review C2 built and verified with the tool's builders (then `c2_exploit_*`) and the builders now
//! refuse; the old numbers are kept in a comment.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cmarket::*;
use kpf_protocol::committee::*;
use kpf_protocol::committee_build::*;
use kpf_protocol::oracle::Makers;
use kpf_protocol::oracle_build::{OBuilt, OLive, OSigned};
use kpf_protocol::state::{KIND_CLOSE, KIND_LIQ, LONG};
use kpf_protocol::tx::{KeySet, KeyUtxo, MIN_FEE_RATE};
use secp256k1::{Keypair, Secp256k1, SecretKey};

const KAS1: i64 = 100_000_000;
const FEED: i64 = 1_000_000_000;

fn kp(seed: u8) -> Keypair {
    Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&[seed; 32]).expect("a secret key"))
}
fn xk(k: &Keypair) -> [u8; 32] {
    k.x_only_public_key().0.serialize()
}
fn op(n: u8) -> TransactionOutpoint {
    TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: 0 }
}
fn fund(key: [u8; 32], n: u8) -> KeyUtxo {
    KeyUtxo { outpoint: op(n), amount: 1_000 * KAS1 as u64, daa: 0, key }
}
fn signed(what: &str, b: &OBuilt, keys: &KeySet) -> OSigned {
    let sg = b.sign(keys).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
    assert!(v.is_ok(), "{what}: {:?}", v.errors());
    sg
}
fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
    let o = &sg.tx.outputs[i];
    OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
}
fn paid(sg: &OSigned, key: &[u8; 32]) -> i64 {
    let spk = kpf_protocol::tx::p2pk_spk(key);
    sg.tx.outputs.iter().filter(|o| o.script_public_key == spk).map(|o| o.value as i64).sum()
}

#[test]
fn c2_fixed_the_tools_builders_refuse_a_claim_and_a_renewal_when_the_spread_exceeds_a_quarter_of_g() {
    // Review C2 (c2_exploit_the_tools_builders_take_a_healthy_side_with_honest_prices_when_the_spread_exceeds_g): with the
    // signed prices below presented at 9.0909091 the builders built, and `verify` passed, the forfeit of a long with a full
    // 10 % of margin, and at 11.1111111 the long's close for 205.56 KAS (a side worth 100 at the feed).
    // A committee inside `checkParams` with a spread of 10 % (`kpf committee-create --spread-bps 1000`), the testnet market on
    // it (g 0.5 %; `kpf c-check` prints two OKs for the pair of them: run by hand in review C2). Three members, every one
    // honest, sign the feed, 10 KAS, for the time 6,500.
    let rid = Hash::from_bytes([0x52; 32]);
    let rp = CommitteeParams { spread_bps: 1_000, ..CommitteeParams::testnet() };
    let m = CMarket::testnet(rid.as_bytes(), [0xfe; 32]);
    assert!(rp.check().is_ok() && m.check().is_ok());
    let members = [kp(101), kp(102), kp(103)];
    let mut table = EMPTY_TABLE;
    for (i, w) in [30i64, 22, 15].iter().enumerate() {
        table[i] = Member { key: xk(&members[i]), weight: w * KAS1 };
    }
    let epoch_table = OLive { s: EpochTable { members: table, total: 67 * KAS1, from: 6_000, until: 9_000, quorum_bps: rp.quorum_bps, spread_bps: rp.spread_bps }, value: KAS1 as u64, id: Some(rid), op: op(2), daa: 6_000 };
    let signed_prices: Vec<(usize, SignedPrice)> = members.iter().enumerate().map(|(i, k)| (i, sign_price(k, &rid.as_bytes(), &m.id(), 6_500, FEED))).collect();
    let (long, short, keeper) = (kp(41), kp(42), kp(77));
    let mut keys = KeySet::new();
    for k in [long, short, keeper] {
        keys.add_keypair(k);
    }
    // 100 units at 10 KAS, the long's bound at 9 (100 KAS of margin), the short's at 12.
    let p = CPair {
        m,
        size: 100 * m.p.scale,
        bl: 900_000_000,
        bs: 1_200_000_000,
        entry: FEED,
        long_key: xk(&long),
        short_key: xk(&short),
        expiry: 6_054_100,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers::default(),
    };
    let pair = OLive { value: p.floor() as u64, id: Some(Hash::from_bytes([0x50; 32])), op: op(10), daa: 6_100, s: p.clone() };
    assert_eq!(p.long_at(FEED), 100 * KAS1);

    // What the tool's collector claims from these signed prices: the feed. It liquidates nobody.
    let honest_prices: Vec<SignedPrice> = signed_prices.iter().map(|t| t.1).collect();
    let honest = Attestation::collect(&epoch_table.s, &rid.as_bytes(), &m.id(), 6_500, &honest_prices).expect("a quorum");
    assert_eq!(honest.p, FEED);
    assert!(propose(&pair, &epoch_table, &honest, KIND_LIQ, LONG, p.size, xk(&keeper), &[fund(xk(&keeper), 60)], &xk(&keeper), MIN_FEE_RATE).is_err());

    // The steps that used to succeed: the same signed prices presented with the lowest and the highest price within their
    // spread, 10 / 1.1 and 10 / 0.9. The epoch table's rule still accepts both attestations; the builders refuse the claims.
    assert!(!m.spread_ok(rp.spread_bps));
    let low = Attestation::of(&epoch_table.s, &m.id(), 6_500, 909_090_910, &signed_prices);
    let high = Attestation::of(&epoch_table.s, &m.id(), 6_500, 1_111_111_111, &signed_prices);
    assert!(low.check(&epoch_table.s, &rid.as_bytes(), 6_500).is_ok() && high.check(&epoch_table.s, &rid.as_bytes(), 6_500).is_ok());
    assert!(propose(&pair, &epoch_table, &low, KIND_LIQ, LONG, p.size, xk(&keeper), &[fund(xk(&keeper), 60)], &xk(&keeper), MIN_FEE_RATE).is_err());
    assert!(propose(&pair, &epoch_table, &high, KIND_CLOSE, LONG, p.size, xk(&long), &[fund(xk(&long), 62)], &xk(&long), MIN_FEE_RATE).is_err());
    assert!(propose(&pair, &epoch_table, &honest, KIND_CLOSE, LONG, p.size, xk(&long), &[fund(xk(&long), 62)], &xk(&long), MIN_FEE_RATE).is_err());
    let renew_at = Attestation::of(&epoch_table.s, &m.id(), 8_999, FEED, &members.iter().enumerate().map(|(i, k)| (i, sign_price(k, &rid.as_bytes(), &m.id(), 8_999, FEED))).collect::<Vec<_>>());
    let soon = OLive { s: CPair { expiry: 7_000, ..p.clone() }, ..pair.clone() };
    assert!(keep_alive(&soon, &epoch_table, &renew_at, &[fund(xk(&keeper), 64)], &xk(&keeper), MIN_FEE_RATE).is_err());

    // On the testnet committee (a spread of 0.1 %, a fifth of g) the same builders build and verify the honest close.
    let narrow = OLive { s: EpochTable { spread_bps: CommitteeParams::testnet().spread_bps, ..epoch_table.s }, ..epoch_table.clone() };
    assert!(m.spread_ok(narrow.s.spread_bps));
    let at = Attestation::collect(&narrow.s, &rid.as_bytes(), &m.id(), 6_500, &honest_prices).expect("a quorum");
    let (b, w) = propose(&pair, &narrow, &at, KIND_CLOSE, LONG, p.size, xk(&long), &[fund(xk(&long), 62)], &xk(&long), MIN_FEE_RATE).expect("the close on the testnet committee");
    let sg = signed("the long's close at the feed", &b, &keys);
    let pending = nth(&sg, 0, w, 6_510);
    let (b, s) = finalize(&pending, &[fund(xk(&keeper), 63)], &xk(&keeper), MIN_FEE_RATE).expect("the finalize");
    let fin = signed("its settlement", &b, &keys);
    let CSettlement::Close { long: long_paid, .. } = s else { panic!("a close") };
    // floor(10 x 0.995) = 9.95: 95 KAS for a side worth 100 at the feed.
    assert_eq!((long_paid, paid(&fin, &xk(&long))), (95 * KAS1, 95 * KAS1));
    // The widest claim left on that committee: 10 / 0.999, paid 9.95995995: a fifth of g back to the closer.
    let edge = Attestation::of(&narrow.s, &m.id(), 6_500, 1_001_001_001, &signed_prices);
    let (_, w) = propose(&pair, &narrow, &edge, KIND_CLOSE, LONG, p.size, xk(&long), &[fund(xk(&long), 62)], &xk(&long), MIN_FEE_RATE).expect("the close at the band's edge");
    assert_eq!(w.close_price(), 995_995_995);
}
