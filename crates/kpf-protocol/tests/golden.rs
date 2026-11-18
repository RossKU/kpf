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
    ("create long order", "213001a260156d574b411905e19ec53f2ef4d1a6891244f7813d2a0e809cfac9", "3cfdda821d140b0c87f8577a26e36a11096a5b5acaa7e63b597510e6fde9d22d"),
    ("create short order", "0a5d7b2d79ec63e9eef1e0df3cfbd476fb62656b33fd988d28e262e8ce2eacce", "3c08e26913a7145de6bb86d18f0b085d07c338c2588953aac18e9b6ae408ca10"),
    ("open orders", "0510cc61e51b3e6f5752c2882bee2340084fd6196b218cb1139e1fa5c2931de9", "5214830591432f2ed109ba1234962ac10edf29c411e35cc06b745cb30b3b3d57"),
    ("amend list long", "ae89a2f17f4464e5e5ea15d0ef30336c3584b15e948ff53bbccb9fe08f6600fe", "a2ada652961678915301ee814ebe8b6af769b7e0a54ce81cf4185863ec091486"),
    ("amend list short", "ec92b522be3c8b6cefee4cd0de1975962d413484d1c48409a6f14ff6b708b1e6", "7cb638d044c86e87d7929a3d06d5d0730411dca99fb4fb4ff5ed393a0b93092e"),
    ("mutual close 400", "80ed343d3f5745fcf66d8f087035ff90e6b816bdb851a2d51e86a274657b9576", "1e57588e0d9262269c6c38041182c61f09583b5777ce51f2498494530ef08929"),
    ("add margin", "e789d00738292a27c298042b53782fe4651027eba051001ed484c7b8c4c761cc", "ed0cb364c73c6cb064844949fc4703c5b50dda4de25a821a5c26d233235cfab3"),
    ("idle settle", "2eaa044b82df120362e19c19c39bc4320e0ed9748421850c32c11e6ef1333631", "429835e15b38e19807a7de048bf56234a2c72e02ceab53e734f3fd0eb55d0605"),
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
