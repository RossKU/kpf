//! Golden vectors: fixed TEST-ONLY keys and fixed inputs give fixed transactions. Signing is deterministic (no
//! auxiliary randomness), so the digest of the full RPC JSON (signatures, compute budgets, storage mass included) is
//! pinned as well as the transaction id (which excludes the signature scripts).
//!
//! If a contract, a builder rule or the fee pass changes on purpose, run with `KPF_GOLDEN=print` and update `GOLDEN`.

use kpf_protocol::build::*;
use kpf_protocol::fixtures::*;
use kpf_protocol::sim::{Chain, Live};
use kpf_protocol::state::*;
use kpf_protocol::tx::*;
use sha2::{Digest, Sha256};

const KAS: u64 = 100_000_000;
const GOLDEN: &[(&str, &str, &str)] = &[
    ("create long order", "0b64ee63002fb17eece173ea0f7bda62c2867d366a32152c5d6dd560dd870edf", "befea78ac0ede9e8c2dd7b6d000806d7527e6daded3c1346c2ad0ee947f54a90"),
    ("create short order", "c321ce87115daa76aeff219c2892120966872dee4969b7cc95af3cce3379ef79", "bd44364f7a746b822df43e49ebb36cc2869c0e990a30be0da709198ac197eb6a"),
    ("open orders", "824f0200f72c6d3e6bb7a9438a3af951293899329eef2c94d3dd8c08c607208b", "7f77f2d0d07ec92787b2b3f12d6a1f802a1bb8cabc2a8dcf568b5a93b5fb83a1"),
    ("amend list long", "ab96b5bdb2d230e06e7c4ed84e45adb904d233a6c3626472d50eadff9eebcaf7", "162b2f33e9e9445bd0a79c8201750424baae0a92ce9fdfa71686a608adc8693f"),
    ("amend list short", "b4d8f0b4910ab694ed7264b605acdb0ceec45c594add6742dfdb98e297d709cf", "b00be1a1bbfc4764cd27644aa9ea9a2fb736440f92e69da4ddf84a19b4a9c24c"),
    ("mutual close 400", "c7c542de3da9b64fd5501a025f29c9c411376bb1b95a59e25bce5cb3fb9eecf9", "b04d4ee6bc0b7a3c3c89482b959f6256383f4b86af01a521312f69f681ca8d74"),
    ("add margin", "a601ea459245f362a247ec5b3d107ae68127f535989b16b90e32ee86290dc3a1", "7ddaf5a020eb415cd941ec948c6afacfe65b5d5d883ab073ceb7c7e1b2680bf1"),
    ("idle settle", "948a33881a175353f9cb769af373899184bf125025a772f3ff3bde24cf9e573e", "4c6940c34fca69303ca3e725b1ee3c32bc0b1a1f0d8bfe7d87afe53d81f9a257"),
];

fn sha(v: &serde_json::Value) -> String {
    Sha256::digest(serde_json::to_string(v).unwrap().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn golden_lifecycle() {
    let keys = keyset(&[21, 22, 24]);
    let (l, s, f) = (pubkey(21), pubkey(22), pubkey(24));
    let o = Opts::default();
    let mut chain = Chain::new();
    chain.advance(1_000);
    let mut rows: Vec<(String, String, String)> = vec![];
    let mut step = |name: &str, chain: &mut Chain, b: &Built| -> Vec<Live> {
        let sg = b.sign(&keys).unwrap();
        let out = chain.apply_signed(&sg).unwrap_or_else(|e| panic!("{name}: {e}"));
        rows.push((name.to_string(), sg.txid().to_string(), sha(&rpc_json(&sg.tx))));
        out
    };
    let fund = |chain: &mut Chain, kas: u64| Funding { utxos: vec![chain.mint(&f, kas * KAS)], change_key: f };

    let fu = fund(&mut chain, 3_000);
    let lo = step("create long order", &mut chain, &create_order(&long_order(l), &fu, &o).unwrap()).remove(0).open();
    let fu = fund(&mut chain, 3_000);
    let so = step("create short order", &mut chain, &create_order(&short_order(s), &fu, &o).unwrap()).remove(0).open();
    let fu = fund(&mut chain, 100);
    let out = step("open orders", &mut chain, &open_orders(&lo, &so, 1_000_000_000, 1_000_000_000, &fu, &o).unwrap());
    let pair = out.into_iter().find_map(|x| if let Live::Pair(p) = x { Some(p) } else { None }).unwrap();
    let fu = fund(&mut chain, 100);
    let p = step("amend list long", &mut chain, &amend(&pair, LONG, (1_000_000_000, 0, 0), &fu, &o).unwrap()).remove(0).pair();
    let fu = fund(&mut chain, 100);
    let p = step("amend list short", &mut chain, &amend(&p, SHORT, (1_050_000_000, 0, 0), &fu, &o).unwrap()).remove(0).pair();
    let fu = fund(&mut chain, 100);
    let out = step("mutual close 400", &mut chain, &mutual_close(&p, 400_000_000, 1_020_000_000, &fu, &o).unwrap());
    let rest = out.into_iter().next().unwrap().pair();
    let fu = fund(&mut chain, 2_000);
    let rest = step("add margin", &mut chain, &add_margin(&rest, LONG, 10_000_000, &fu, &o).unwrap()).remove(0).pair();
    chain.advance(77_760_000);
    let fu = fund(&mut chain, 100);
    step("idle settle", &mut chain, &idle_settle(&rest, &fu, &o).unwrap());

    if std::env::var("KPF_GOLDEN").as_deref() == Ok("print") {
        for (n, id, h) in &rows {
            println!("    (\"{n}\", \"{id}\", \"{h}\"),");
        }
        return;
    }
    assert_eq!(rows.len(), GOLDEN.len());
    for ((n, id, h), (gn, gid, gh)) in rows.iter().zip(GOLDEN) {
        assert_eq!(n, gn);
        assert_eq!(id, gid, "{n}: transaction id changed (KPF_GOLDEN=print shows the new values)");
        assert_eq!(h, gh, "{n}: full transaction (signatures, budgets, storage mass) changed");
    }
}
