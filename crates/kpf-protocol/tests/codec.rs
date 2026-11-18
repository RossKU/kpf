//! State codecs: round trips, strict decoding, validation against the contract bounds, networks and JSON forms.

use kaspa_consensus_core::Hash;
use kpf_protocol::artifacts::*;
use kpf_protocol::fixtures::*;
use kpf_protocol::network::*;
use kpf_protocol::payoff::*;
use kpf_protocol::state::*;
use kpf_protocol::tx::*;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
}

fn random_pair(r: &mut Lcg) -> Pair {
    let mut p = pair(pubkey(1 + (r.next() % 5) as u8), pubkey(6 + (r.next() % 5) as u8));
    p.m.scale = [1, 1_000, 1_000_000, 1_000_000_000][(r.next() % 4) as usize];
    p.size = r.range(p.m.min_size, 1_000_000_000_000);
    p.bs = r.range(2, 1_000_000_000_000);
    p.bl = r.range(0, p.bs - 1);
    p.entry = r.range(0, p.m.price_max);
    p.long_ask = r.range(0, 1_000_000);
    p.short_bid = r.range(0, 1_000_000);
    p.tag_l = Hash::from_bytes([(r.next() & 255) as u8; 32]);
    p.since = r.range(0, 1_000_000_000);
    p
}

#[test]
fn pair_and_open_round_trip() {
    let mut r = Lcg(7);
    for _ in 0..200 {
        let p = random_pair(&mut r);
        assert_eq!(Pair::decode_state(&p.state_bytes()).unwrap(), p);
        assert_eq!(Pair::decode_redeem(&p.redeem()).unwrap(), p);
        assert_eq!(decode_redeem(&p.redeem()).unwrap(), Decoded::Pair(p));
    }
    for o in [long_order(pubkey(3)), short_order(pubkey(4))] {
        assert_eq!(Open::decode_redeem(&o.redeem()).unwrap(), o);
        assert_eq!(decode_redeem(&o.redeem()).unwrap(), Decoded::Open(o));
    }
}

#[test]
fn negative_numbers_use_the_script_number_form() {
    let mut p = pair(pubkey(1), pubkey(2));
    p.entry = -5;
    let s = p.state_bytes();
    assert_eq!(Pair::decode_state(&s).unwrap().entry, -5);
    assert_eq!(sm8(-5), [5, 0, 0, 0, 0, 0, 0, 0x80]);
    assert_eq!(sm8(300), [0x2c, 1, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn decoding_is_strict() {
    let p = pair(pubkey(1), pubkey(2));
    let s = p.state_bytes();
    assert!(Pair::decode_state(&s[..s.len() - 1]).is_err(), "truncated");
    let mut long = s.clone();
    long.push(0);
    assert!(Pair::decode_state(&long).is_err(), "trailing byte");
    let mut bad = s.clone();
    bad[0] = 0x07; // not the 32-byte push
    assert!(Pair::decode_state(&bad).is_err());
    let mut bad = s.clone();
    bad[3 * 33] = 0x09; // an integer push of the wrong width
    assert!(Pair::decode_state(&bad).is_err());
    // Negative zero is not canonical.
    let mut nz = s.clone();
    nz[371] = 0x80; // longAsk = 0 with the sign bit: "negative zero"
    assert!(Pair::decode_state(&nz).is_err());
    // A redeem script of another template, or of the wrong length.
    let mut rs = p.redeem();
    rs[0] ^= 1;
    assert!(Pair::decode_redeem(&rs).is_err());
    assert!(decode_redeem(&rs).is_err());
    assert!(Pair::decode_redeem(&rs[..rs.len() - 1]).is_err());
    assert!(Open::decode_redeem(&p.redeem()).is_err(), "a pair is not an order");
    let mut rs = p.redeem();
    let n = rs.len();
    rs[n - 1] ^= 1;
    assert!(Pair::decode_redeem(&rs).is_err(), "suffix must match the pinned template");
}

#[test]
fn validation_applies_the_contract_bounds() {
    let good = pair(pubkey(1), pubkey(2));
    assert!(good.validate().is_ok());
    let bad = |f: &dyn Fn(&mut Pair)| {
        let mut p = good.clone();
        f(&mut p);
        assert!(p.validate().is_err());
    };
    bad(&|p| p.m.scale = 0);
    bad(&|p| p.m.scale = SCALE_MAX + 1);
    bad(&|p| p.m.price_max = PRICE_CAP + 1);
    bad(&|p| p.m.price_max = 0);
    bad(&|p| p.m.carrier = -1);
    bad(&|p| p.m.min_size = 0);
    bad(&|p| p.m.idle_daa = CSV_LIMIT);
    bad(&|p| p.m.k_bps = 0);
    bad(&|p| p.m.k_bps = K_MAX + 1);
    bad(&|p| p.m.mmr_bps = MMR_CAP);
    bad(&|p| p.m.liq_fee_bps = BPS + 1);
    bad(&|p| p.m.min_rest_daa = LOCK_TIME_THRESHOLD);
    bad(&|p| p.size = 0);
    bad(&|p| p.size = i64::MAX / 2);
    bad(&|p| p.bl = p.bs);
    bad(&|p| p.bl = -1);
    bad(&|p| p.bs = p.m.price_max + 1);
    bad(&|p| p.long_key = [0xff; 32]); // not a curve point
    bad(&|p| p.long_tip = -1);
    bad(&|p| p.short_bid = p.m.price_max + 1);
    // The size bound: (INT_MAX - carrier) / priceMax - 1 whole tokens.
    let mut p = good.clone();
    p.m.scale = 1;
    p.size = p.m.max_whole_tokens();
    assert!(p.validate().is_ok());
    p.size += 1;
    assert!(p.validate().is_err());
    // Orders.
    let o = long_order(pubkey(3));
    assert!(o.validate().is_ok());
    let mut b = o.clone();
    b.side = 3;
    assert!(b.validate().is_err());
    let mut b = o.clone();
    b.my_rate = 0;
    assert!(b.validate().is_err());
    let mut b = o.clone();
    b.size_left = b.m.min_size - 1;
    assert!(b.validate().is_err());
}

#[test]
fn escrow_and_floor_follow_design() {
    // DESIGN 5 worked example: pair value q(1e9, 3e8) = 3e11 sompi + carrier.
    let p = pair(pubkey(1), pubkey(2));
    assert_eq!(p.floor(), 300_000_000_000 + C);
    // Long order of 2,000 tokens, rate 1 KAS + tip 0.01 KAS, two carriers.
    let o = long_order(pubkey(3));
    assert_eq!(o.escrow(), 2_000 * 101_000_000 + 2 * C);
    assert_eq!(o.spend_for(1_000_000_000), 1_000 * 101_000_000 + C);
}

#[test]
fn tx_json_round_trips() {
    let tx = {
        let mut chain = kpf_protocol::sim::Chain::new();
        let k = pubkey(24);
        let f = Funding_of(&mut chain, k);
        let b = kpf_protocol::build::create_order(&long_order(pubkey(21)), &f, &Default::default()).unwrap();
        b.sign(&keyset(&[24])).unwrap().tx
    };
    let v = rpc_json(&tx);
    let back = tx_from_rpc_json(&v).unwrap();
    assert_eq!(back.id(), tx.id());
    assert_eq!(back.storage_mass(), tx.storage_mass());
    assert_eq!(rpc_json(&back), v);
    assert_eq!(tx_hex(&back), tx_hex(&tx));
    assert!(tx.version == 1 && tx.inputs.iter().all(|i| i.compute_commit.compute_budget().is_some()));
}

#[allow(non_snake_case)]
fn Funding_of(chain: &mut kpf_protocol::sim::Chain, k: [u8; 32]) -> kpf_protocol::build::Funding {
    kpf_protocol::build::Funding { utxos: vec![chain.mint(&k, 5_000 * 100_000_000)], change_key: k }
}

#[test]
fn networks_and_addresses() {
    assert_eq!(Network::parse("testnet-10").unwrap(), Network::Testnet10);
    assert_eq!(Network::parse("kaspa-testnet-10").unwrap(), Network::Testnet10);
    for bad in ["mainnet", "Mainnet", "kaspa", "simnet", "devnet", "testnet-11"] {
        let e = Network::parse(bad).unwrap_err().to_string();
        assert!(e.contains("KPF is a testnet-only research prototype"), "{bad}: {e}");
    }
    let key = pubkey(1);
    let a = p2pk_address(&key, Network::Testnet10);
    assert!(a.starts_with("kaspatest:"), "{a}");
    assert_eq!(pubkey_of_address(&a, Network::Testnet10).unwrap(), key);
    // A mainnet address is refused with the message.
    let main = a.replacen("kaspatest:", "kaspa:", 1);
    let e = parse_address(&main, Network::Testnet10).unwrap_err().to_string();
    assert!(e.contains("testnet-only") && e.contains("mainnet address"), "{e}");
    assert_eq!(MAINNET_PREFIX, "kaspa");
    // P2SH addresses of both contracts, and the script they stand for.
    let p = pair(key, pubkey(2));
    let pa = p2sh_address(&p.redeem(), Network::Testnet10);
    assert!(pa.starts_with("kaspatest:"));
    assert_eq!(address_spk(&pa, Network::Testnet10).unwrap(), p.spk());
    let o = long_order(key);
    assert_eq!(address_spk(&p2sh_address(&o.redeem(), Network::Testnet10), Network::Testnet10).unwrap(), o.spk());
    assert_ne!(pa, p2sh_address(&o.redeem(), Network::Testnet10));
}

#[test]
fn covenant_id_is_a_function_of_outpoint_and_output() {
    use kaspa_consensus_core::hashing::covenant_id::covenant_id;
    use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint, TransactionOutput};
    let op = TransactionOutpoint { transaction_id: TransactionId::from_bytes([9; 32]), index: 1 };
    let out = TransactionOutput { value: 5, script_public_key: p2pk_spk(&pubkey(1)), covenant: None };
    let a = covenant_id(op, std::iter::once((0u32, &out)));
    let b = covenant_id(op, std::iter::once((0u32, &out)));
    assert_eq!(a, b);
    let mut out2 = out.clone();
    out2.value = 6;
    assert_ne!(a, covenant_id(op, std::iter::once((0u32, &out2))));
    let _ = pair_template();
}

#[test]
fn q_matches_a_wide_reference_and_is_monotone() {
    let mut r = Lcg(99);
    for _ in 0..2000 {
        let scale = [1i64, 10, 1_000, 1_000_000, 1_000_000_000][(r.next() % 5) as usize];
        let n = r.range(0, 4_000_000_000_000);
        let rate = r.range(0, 1_000_000_000_000);
        let reference = (n as i128 * rate as i128) / scale as i128;
        let up = (n as i128 * rate as i128 + scale as i128 - 1) / scale as i128;
        if reference > i64::MAX as i128 {
            assert!(q_checked(n, rate, scale, 0).is_none());
            continue;
        }
        assert_eq!(q(n, rate, scale, 0) as i128, reference, "n={n} r={rate} scale={scale}");
        if up <= i64::MAX as i128 {
            assert_eq!(q(n, rate, scale, scale - 1) as i128, up);
        }
        // Superadditive in the rate: the parts never exceed the whole (rounding down).
        let (a, b) = (rate / 3, rate - rate / 3);
        assert!(q(n, a, scale, 0) + q(n, b, scale, 0) <= q(n, rate, scale, 0));
    }
    assert!(q_checked(i64::MAX, i64::MAX, 1, 0).is_none());
}

#[test]
fn quote_table_matches_the_design_worked_example() {
    let p = pair(pubkey(1), pubkey(2));
    // Price 9.0 and below: the long is wiped out, the short owns the whole range; 10.0 splits 1,000 / 2,000 KAS.
    let rows = quote_table(&p, &[800_000_000, 900_000_000, 1_000_000_000, 1_200_000_000, 1_300_000_000]);
    let kas = |x: i64| x / 100_000_000;
    assert_eq!((kas(rows[0].long), kas(rows[0].short)), (0, 3_000));
    assert_eq!((kas(rows[2].long), kas(rows[2].short)), (1_000, 2_000));
    assert_eq!((kas(rows[3].long), kas(rows[3].short)), (3_000, 0));
    assert_eq!((rows[4].long, rows[4].short), (rows[3].long, rows[3].short), "clamped above bs");
    for r in &rows {
        assert_eq!(r.long + r.short, 300_000_000_000);
    }
    // DESIGN 5: with mmrBps = 500 the long is liquidatable at an ask of at most 9.4737 KAS (947,368,421 sompi), the short
    // at a bid of at least 11.4286 KAS (1,142,857,143).
    assert!(long_liquidatable(&p, 947_368_421) && !long_liquidatable(&p, 947_368_422));
    assert!(short_liquidatable(&p, 1_142_857_143) && !short_liquidatable(&p, 1_142_857_142));
    assert_eq!(trigger_of(&p, LONG), 947_368_421);
    assert_eq!(trigger_of(&p, SHORT), 1_142_857_143);
    // At a = 9.4 KAS the fee is 47 KAS: q(1000 tokens, 9.4e8 * 50 / 10^4) = 1000 * 4,700,000 sompi.
    assert_eq!(fee_of(&p.m, 1_000_000_000, 940_000_000), 1_000 * 4_700_000);
    // The fee is capped at the maintenance slice q(n, T - bl).
    assert_eq!(liq_fee(&p, 1_000_000_000, 940_000_000, LONG), 1_000 * 4_700_000);
    assert_eq!(slice_of(&p, 1_000_000_000, LONG), 1_000 * 47_368_421);
    assert_eq!(winner_at_trigger(&p, 1_000_000_000, LONG), 1_000 * (1_200_000_000 - 947_368_421));
}
