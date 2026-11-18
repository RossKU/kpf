//! Review 3 (docs/audit/REVIEW-3.md), the `kpf` command line. Offline: no node, nothing submitted.

use kpf_cli::args::{fmt_units, parse_amount};
use kpf_cli::files::{funding_utxos, parse_utxos, ObjFile};
use kpf_protocol::fixtures::market;
use serde_json::json;

/// R3-T2. Token amounts are parsed and printed with round(log10(scale)) decimal places, but `Market::validate`
/// accepts any scale in 1..=10^9, and the scale comes from the market or pair file someone else hands the user.
/// With scale 400,000 (a market of 400,000 base units per token) `--size 0.5` or `--n 0.5` becomes 500,000 base
/// units (1.25 tokens), and the summary the command prints for it reads "1.1". With scale 5, "0.5" is a whole
/// token. Who loses: the user commits margin and exposure for 2.5 times the size it typed, and the printed summary
/// shows a third number.
#[test]
fn exploit_token_amounts_for_a_non_decimal_scale() {
    let mut m = market();
    m.scale = 400_000;
    assert!(m.validate().is_ok(), "the tool accepts the market");
    let n = parse_amount("0.5", m.scale).unwrap();
    assert_eq!(n, 500_000, "0.5 tokens are parsed as 1.25 tokens");
    assert_ne!(n, 200_000);
    assert_eq!(fmt_units(n as i128, m.scale as i128), "1.1", "and printed as 1.1 tokens");
    assert_eq!(fmt_units(200_000, 400_000), "0.2", "a real half token is printed as 0.2");
    assert_eq!(parse_amount("0.5", 5).unwrap(), 5, "scale 5: half a token becomes one token");
}

/// R3-T3. `kpf utxos --out` writes each UTXO with its `covenantId` "usable as --funding", but the simple file shape
/// is read back without it (and without the script), so `funding_utxos` admits a covenant-bound UTXO that the node
/// path refuses. The same entry in the node's own shape is refused.
#[test]
fn exploit_saved_funding_file_readmits_covenant_utxos() {
    let key = [0x11u8; 32];
    let txid = "aa".repeat(32);
    let cov = "44".repeat(32);
    let saved = json!([{ "txid": txid, "index": 0, "amount": 800_000_000u64, "daa": 5, "covenantId": cov }]);
    let list = parse_utxos(&saved).unwrap();
    assert!(list[0].covenant_id.is_none(), "the covenant id written by `kpf utxos --out` is dropped");
    assert_eq!(funding_utxos(&list, &key).len(), 1, "and the covenant UTXO becomes funding");
    let mut spk = String::from("000020");
    spk.push_str(&hex(&key));
    spk.push_str("ac");
    let node = json!([{ "outpoint": { "transactionId": txid, "index": 0 },
        "utxoEntry": { "amount": "800000000", "scriptPublicKey": spk, "blockDaaScore": "5", "covenantId": cov } }]);
    let list = parse_utxos(&node).unwrap();
    assert!(funding_utxos(&list, &key).is_empty(), "the node shape keeps the filter");
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// R3-T4. An object or KOB order file without `daa` is read as DAA 0, silently, and the `evidence` command and the
/// evidence builders then compute the rest window from DAA 0: a fresh order is reported as rested and the local
/// engine check passes, while the node (which reads the real UTXO DAA) refuses the transaction.
#[test]
fn exploit_object_file_without_daa_is_read_as_daa_zero() {
    let o = ObjFile::from_json(&json!({
        "outpoint": { "txid": "bb".repeat(32), "index": 1 }, "amount": 1_000_000_000u64,
        "covenantId": "e1".repeat(32), "redeem": "00"
    }))
    .unwrap();
    assert_eq!(o.daa, 0);
}

/// R3-T5 (Windows). The key file is written with the inherited permissions first, and `icacls` is then run on a
/// best-effort basis with its result ignored; when it fails (here: a USERNAME that is not an account) the file
/// keeps every inherited access entry while `keygen` still prints "mode 600 / current user". The test writes a
/// throw-away key under the build directory and removes it.
#[cfg(windows)]
#[test]
fn exploit_keygen_claims_private_permissions_it_did_not_set() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/audit3-keygen");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("k.json");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_kpf"))
        .args(["keygen", "--out", path.to_str().unwrap(), "--allow-in-repo"])
        .env("USERNAME", "no-such-account-r3")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("mode 600 / current user"));
    let acl = std::process::Command::new("icacls").arg(&path).output().unwrap();
    let acl = String::from_utf8_lossy(&acl.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(acl.contains("(I)"), "inherited access entries remain:\n{acl}");
}
