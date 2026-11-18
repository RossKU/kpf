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
    ("create long order", "43f1cb6c05aa88104182f5aaba672f2e65bc0cf4af68c070790c9e871400a640", "f95a319fd4ee4db864781b6fd3dcac426d573a4fd253b751fcd19b82922d2089"),
    ("create short order", "bfd229bff01e119b6f6075b6b8bd8ac23207eaef2f6310b32b4448e964e2ad6f", "140f49fa696a696ec048922374003aa4f9405c47c4da3fca90a0c50b23e00734"),
    ("open orders", "eb31c8e507a30ea2ee91dbb4c659a739f4e6ea2be25bccd1fa42e7693c4358e8", "6f93fb62ebf16f38a932a8354d5b748fdbe083be1f46c5fb25568474db680235"),
    ("amend list long", "f06c7b56d382f184847aed49f7590facee74f111997f66a8914803bd8892938f", "6616c8bb7fd5547f5589e23161306c77d7a50bc0531286f1b88de1f7fad6324a"),
    ("amend list short", "ef2bee97e2d5a5031149ca78eaee365d7e3cc19d3135d53658c84c43dca35461", "42435336593e4cffc32a7236771d2975eabc3ab0808c0c76d7a8e9e980d23110"),
    ("mutual close 400", "f219852f629e8584244dc66f0b775cceeecfe04a51ee951a52aade2b2a115c42", "97bb8a67835b227207532e7ff3a0374e79ed77e53e54e53f0a24281fce853290"),
    ("add margin", "7f399024b6569168e720e1ad049d7b3630f43de23235ec3f8fc0b4ea743bb618", "e63bd4a7aa9903f6e9b9c3f5788cc5b1835d79b7eeb790d35d4b8acdebb445ea"),
    ("idle settle", "5f3ab201214fd2c1cad745f0160c8bb480335b97670cb28d13d74bd80a6d41cd", "49a059350e205ba0ffeeca11ddc4d80d8e0b52c76db6b2559bae79a3b4334e9c"),
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
