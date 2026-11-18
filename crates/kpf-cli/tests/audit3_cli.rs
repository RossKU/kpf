//! Review 3 (docs/audit/REVIEW-3.md), the `kpf` command line. Offline: no node, nothing submitted. After the fixes of
//! DESIGN 4.5c every finding here is a `holds_*` test asserting the corrected behaviour.

use kpf_cli::args::{fmt_units, parse_amount};
use kpf_cli::files::{funding_utxos, parse_utxos, ObjFile};
use kpf_protocol::fixtures::market;
use serde_json::json;

/// R3-T2, fixed. Token amounts are parsed and printed exactly for any `scale` (DESIGN 4.5c). With scale 400,000 (a
/// market of 400,000 base units per token) `--n 0.5` is 200,000 base units (half a token), and it prints as "0.5";
/// 500,000 base units print as "1.25". A decimal that is not a whole number of base units (0.3 of scale 5) is refused
/// instead of being rounded, and a power-of-ten scale keeps the decimal rule.
#[test]
fn holds_token_amounts_are_exact_for_a_non_decimal_scale() {
    let mut m = market();
    m.scale = 400_000;
    assert!(m.validate().is_ok());
    let n = parse_amount("0.5", m.scale).unwrap();
    assert_eq!(n, 200_000, "0.5 tokens are half a token");
    assert_eq!(fmt_units(n as i128, m.scale as i128), "0.5");
    assert_eq!(fmt_units(500_000, 400_000), "1.25");
    assert_eq!(parse_amount("1.25", 400_000).unwrap(), 500_000);
    assert_eq!(parse_amount("0.4", 5).unwrap(), 2, "scale 5: 0.4 tokens are 2 base units");
    let e = parse_amount("0.5", 5).unwrap_err();
    assert!(e.contains("not a whole number of base units"), "{e}");
    assert_eq!(fmt_units(4, 3), "1 1/3", "a fraction without a finite decimal is printed exactly");
    assert_eq!(parse_amount("1000.5", 1_000_000).unwrap(), 1_000_500_000);
    assert!(parse_amount("0.0000001", 1_000_000).is_err(), "seven places at scale 10^6");
    assert_eq!(parse_amount("7s", 400_000).unwrap(), 7, "exact base units with the s suffix");
}

/// R3-T3, fixed. `kpf utxos --out` writes each UTXO with its `covenantId` (and its script); the simple file shape now
/// reads both back, so `funding_utxos` refuses a covenant-bound UTXO from the tool's own file as the node path does.
#[test]
fn holds_saved_funding_file_keeps_the_covenant_flag() {
    let key = [0x11u8; 32];
    let txid = "aa".repeat(32);
    let cov = "44".repeat(32);
    let saved = json!([{ "txid": txid, "index": 0, "amount": 800_000_000u64, "daa": 5, "covenantId": cov }]);
    let list = parse_utxos(&saved).unwrap();
    assert!(list[0].covenant_id.is_some(), "the covenant id written by `kpf utxos --out` is kept");
    assert!(funding_utxos(&list, &key).is_empty(), "and the covenant UTXO is not funding");
    let mut spk = String::from("000020");
    spk.push_str(&hex(&[0x22u8; 32]));
    spk.push_str("ac");
    let other = json!([{ "txid": txid, "index": 1, "amount": 800_000_000u64, "daa": 5, "covenantId": null, "scriptPublicKey": spk }]);
    let list = parse_utxos(&other).unwrap();
    assert!(funding_utxos(&list, &key).is_empty(), "a saved script of another key is not funding either");
    let node = json!([{ "outpoint": { "transactionId": txid, "index": 0 },
        "utxoEntry": { "amount": "800000000", "scriptPublicKey": spk, "blockDaaScore": "5", "covenantId": cov } }]);
    let list = parse_utxos(&node).unwrap();
    assert!(funding_utxos(&list, &key).is_empty(), "the node shape keeps the filter");
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// R3-T4, fixed. An object or KOB order file without `daa` is read as "unknown", never as DAA 0, and every use that
/// depends on the DAA (a pair, an order, evidence) refuses it with a plain reason. A file with `daa` loads as before.
#[test]
fn holds_object_file_without_daa_is_refused() {
    let p = kpf_protocol::fixtures::pair(kpf_protocol::fixtures::pubkey(1), kpf_protocol::fixtures::pubkey(2));
    let mut v = json!({
        "outpoint": { "txid": "bb".repeat(32), "index": 1 }, "amount": 1_000_000_000u64,
        "covenantId": "e1".repeat(32), "redeem": hex(&p.redeem())
    });
    let o = ObjFile::from_json(&v).unwrap();
    assert_eq!(o.daa, None);
    let e = o.live_pair().unwrap_err();
    assert!(e.contains("no `daa`") && e.contains("not read as 0"), "{e}");
    v["daa"] = json!(null);
    assert!(ObjFile::from_json(&v).unwrap().live_pair().is_err());
    v["daa"] = json!(77);
    assert_eq!(ObjFile::from_json(&v).unwrap().live_pair().unwrap().daa, 77);
}

/// R3-T5 (Windows), fixed. `keygen` now reports what it actually did about the key file's permissions: when `icacls`
/// fails (here: a USERNAME that is not an account) it prints a WARNING that the file keeps its inherited permissions,
/// instead of claiming "mode 600 / current user". The test writes a throw-away key under the build directory and
/// removes it.
#[cfg(windows)]
#[test]
fn holds_keygen_reports_the_permissions_it_set() {
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
    let acl = std::process::Command::new("icacls").arg(&path).output().unwrap();
    let acl = String::from_utf8_lossy(&acl.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{stdout}");
    assert!(!stdout.contains("mode 600 / current user"), "{stdout}");
    assert!(stdout.contains("WARNING") && stdout.contains("inherited"), "{stdout}");
    assert!(acl.contains("(I)"), "inherited access entries remain, and the tool says so:\n{acl}");
}
