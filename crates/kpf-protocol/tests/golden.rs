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
    ("create long order", "a7bb25dc3dde28c1050a8bbba6ba0858e2b60678887aaa30fa51661f23eb94ca", "b7432e93d6537a8a4988e49e39a5e6e2bd887c58a9bae9a9ed5ab62af83dc368"),
    ("create short order", "077c16b500e6b3b9ec5f3cd46fc2ebdd965f6c7ecc5fc8eae3163cb20864df4a", "841dbef8cf442e8b87eb85fa234d70f7f789fc475f22f5c1e155144804d9c020"),
    ("open orders", "63459b08e42f7a437325968ac361197818fcf5dec3a0114036b62ba66547228f", "797eecc3d3783ffa02c642e4b69d37bfb0fd3679fb05e42451d043946e25f995"),
    ("amend list long", "ad825551050c5d0bfd8aff511da141402d596eb35daa146d5265e92d10cf9df1", "45efa75ed2c68b08e2f29d5cc684b44451c6ce4ad1c6bd21cc54114eaa502616"),
    ("amend list short", "ff0d77f43d1b1bbeb9c56c9dbe04c54b611dcbf133a4b8a049ecf1d7b60dfc31", "1c6216ef8c3407529f7faa546dd94d6ea975c201e4ad0a4b3c118a69fd784686"),
    ("mutual close 400", "87ef47ba3418b7ab44f17cf249c33341b7658f9a2dc4e67cd29cd52c8465325d", "ced4ec46eec668a14fbf57f778aad539433832c60fef61e29d54b386defbf239"),
    ("add margin", "bd616d42807ababcd46fb95b8e5b4bfd3966570c08718b76fdf30d6aec46d8ca", "248ef8ee12c61cea914abbb20fae739d9cc7f50d702592daa27dbffba6214430"),
    ("idle settle", "00aaa1c50cd8c9b640740fccff9e1615df316481e801d1301181d053fbc24db8", "435a1e46e683fc8a5720780cabe08cec34348e981ba687dbb1e7eaff2ea975fc"),
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
