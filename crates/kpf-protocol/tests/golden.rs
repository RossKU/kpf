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
    ("create long order", "34197cb81ead05029bc51078483d0be9d9274eb98633e9495fd7855f6fd4f4e5", "e7b5adc722174cab5f9e5126e35072f0126e31e65f69e2923b528dd6bbd1bd96"),
    ("create short order", "0956ac4e44220463bdf0bee7fd46cb9e957a4106004fe4e4aaa06795103932b5", "0fb7e5415f7e805710f1f20a22ee080a928b98321c3a699f88812d222873c336"),
    ("open orders", "a49bb27aeda22b06aa856dd8375145de68e13990f2a340af25d540b2037fa2e7", "38d09f477f3e24e0f421f1776988ff25cf2062778bae043e064667602bfb1c41"),
    ("amend list long", "edf18ad1bfc45405ef8eafd6f2182757192050bb208984a6412f9651e002c578", "18713ead8412c9f8ef48dca088a31d1b52067d182b2126eb061a09ca8749133a"),
    ("amend list short", "c87341de54b7d5c9f6e08c90f37c27b3547306a1bfeb529cf89b02fd379ec733", "19ee06896f9eaa4482e62610703b132fae382e3deaa671126880ffb768efaa8b"),
    ("mutual close 400", "d7f58eb16c294eae57a781dcdb0837c3b1e91f63058acbdba8c5cf5e584c792b", "8699fe17f64003b8f3b2c959e0878dfdbd57c7630f16237788758c0dae5c7189"),
    ("add margin", "09dd5c09570e136f3f902bb7ba1e0525f74860955042ea03f3af1774729f1025", "dc992c874d039472ed6200c77ba5e3fe1b30fbd81738bb9b978273b5981c885f"),
    ("idle settle", "ec3038eb38efb463508c256ac1e6ded328cd30b688b5c0b9dd36eb61b4a4ec2a", "9c66498cbb4c25a277adad6d5868671575a7c4683b2e29d6b60bd15755222fe9"),
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
