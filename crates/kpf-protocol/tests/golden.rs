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
    ("create long order", "f04ba49a8d9e8365023b741ea1f9a2827d0602477be046a488e67187f44fad7d", "ebfd44c92a6c9d6be5f363023944f5f2712d76edffd1ceaaeaa9aa5065e5a6cd"),
    ("create short order", "d74c4f3c4737ab3788a82655e855733805e54567621f276cfc658737ba2035b2", "5c38f1786a31dfcce1014f4dc0da5cc1a290ef5b966c486ce57ec1bb1674bb22"),
    ("open orders", "57810be553371ae64acbc3e9ace14f0685d49e1d37d2a390ddc70b6a00ee09cd", "367c230489d361e444bdd7f448a207b2f73a1051a8c054b134c264fb40af838a"),
    ("amend list long", "4a563899edb9efabe19264b6fb8b99883a1560fcce0fb82cb82cd87b2cee9066", "2baab57c1f91cb8380d0e755a2230ae96d5863fdbd777534932c4c2ce9671a71"),
    ("amend list short", "d5a73302c2499f826fb936bb0f64d74bbc46a36b6368974a0c17fe8e386a1d4e", "9fe582db95ec15f67899b858e0de3d95b3157a9a49b706e78c0be80d3871859b"),
    ("mutual close 400", "7c580d714d6977276e24fa3a2ec71e4b77b8f4f2e98a9e9bf6a989b3c6e9ed77", "5b52073f6e120ef5b9bf5011c553c6cc6bbb8a43ad26c7eadc38bb80873ce3ea"),
    ("add margin", "ed81f023410d26971cd7f309dbe097ae2a6953c41df1fc006dc42cd94d8fa20b", "1481c7d69ebac7bcde308213ecb3f0ac4ab99e4c8ab9235c79b32004dfa77c8b"),
    ("idle settle", "40cb677e50526cb3cf0b9d67100e2bf19a1fe02c00c36125c7588809822c193f", "709196957baaf37ce8137baa45a15460ca17b85df1feb2ff8d5663624722b6ba"),
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
