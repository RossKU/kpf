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
    ("create long order", "906382d90796db9f8ba8611f82ec7bbe26d9feefa2829de8d7a8066d5e7c1284", "e1072c4aeb88199e8903d1f6af37649fa3eba4a6ce9c49caa11862a638f7e147"),
    ("create short order", "0ce7c0601d1f65128b1f3b135b7ff02a415a654c12f9745b6385d2de72ccded5", "bdb40aa3a8b40d00bf1e7ff23198b0848a6b1fdcd27dbf3d1488594c3f9a337d"),
    ("open orders", "3f3c9d5638eb1e9460a99768d5eea57199552c014ac79f352c6030050998ce75", "3d99b049e939878e2362c2beb9e3c5e1050afb9d41214365559ab87ada219cb5"),
    ("amend list long", "2c22c40b87446441a1b052293477f525562cf88b3987b03030df5be059c7b0cb", "090927086670723e8ded6c7ba891c437ec568da1012b296e90e41f37614ccff9"),
    ("amend list short", "8a3a8dcabdabd9c4976d4ded65f56328c6e4984f0c1bb4a552aae54f0d5ba648", "d23392822a991a43712da4310a8df1432b24e3b069192b3ededcd4cbcb8685b6"),
    ("mutual close 400", "76daaf6130bd7f155d8daa0a00f4a8221943935c4fa9e17144250205c9d29a46", "f451d6e007ee5897e243aae39dafe426f535db53ac0f344027dbcc0bb1ee31bf"),
    ("add margin", "8fe4bc4df8b4e3026cb761422af6eb71ba864bfe11932b856ae8fc7ea037bd25", "6b7ccd3135b630acf613b6b48da7ffe805bc7903485ee58619f9f0973a115cd8"),
    ("idle settle", "d359767745e4b9fcbcd3fd133e6c51a47c46553e256e85292c16fff58d00744d", "0834c90bffe4dbf2da56ef16f282d6076f1c4679a6d93e3e5667f17488575a0c"),
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
