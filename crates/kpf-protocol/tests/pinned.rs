//! The pinned artifacts agree with the sources, the manifest, `SHA256SUMS` and the codecs. (That they equal a fresh
//! compile is checked by `cargo test -p kpf-artifacts`, the only crate that links the compiler.)

use kpf_protocol::artifacts::*;
use kpf_protocol::fixtures::*;
use kpf_protocol::state::*;
use sha2::{Digest, Sha256};

fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn norm(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn contracts() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts")
}

fn source(name: &str) -> String {
    norm(&std::fs::read_to_string(contracts().join(name)).unwrap())
}

#[test]
fn manifest_matches_sources_and_embedded_artifacts() {
    let m = manifest();
    assert_eq!(m.format, "kpf-artifacts/1");
    assert_eq!(m.kpf_pair.source_sha256, sha(source("KpfPair.sil").as_bytes()), "contracts/KpfPair.sil changed: regenerate the artifacts");
    assert_eq!(m.kpf_open.source_sha256, sha(source("KpfOpen.sil").as_bytes()), "contracts/KpfOpen.sil changed: regenerate the artifacts");
    assert_eq!(m.kpf_pair.artifact_sha256, sha(norm(pair_artifact_text()).as_bytes()));
    assert_eq!(m.kpf_open.artifact_sha256, sha(norm(open_artifact_text()).as_bytes()));
}

#[test]
fn sha256sums_cover_every_pinned_file() {
    let sums = std::fs::read_to_string(contracts().join("artifacts/SHA256SUMS")).unwrap();
    let mut seen = 0;
    for line in sums.lines() {
        let (h, path) = line.split_once("  ").expect("sha256sum line");
        let text = norm(&std::fs::read_to_string(contracts().join(path)).unwrap());
        assert_eq!(h, sha(text.as_bytes()), "{path}");
        seen += 1;
    }
    assert_eq!(seen, 5, "KpfPair.sil, KpfOpen.sil, the two artifacts and the manifest");
    assert!(sums.contains("artifacts/manifest.json"));
}

#[test]
fn templates_and_hashes_agree_with_the_manifest() {
    let m = manifest();
    for (t, c) in [(pair_template(), &m.kpf_pair), (open_template(), &m.kpf_open)] {
        assert_eq!(t.prefix.len(), c.prefix_len);
        assert_eq!(t.suffix.len(), c.suffix_len);
        assert_eq!(t.state_len, c.state_len);
        assert_eq!(template_hash_of(&t.prefix, &t.suffix).iter().map(|b| format!("{b:02x}")).collect::<String>(), c.template_hash);
        assert_eq!(t.hash, template_hash_of(&t.prefix, &t.suffix));
    }
    assert_ne!(pair_template().hash, open_template().hash);
    // The KOB evidence templates are the ones KOB itself pins in KobCondAsk (vendor/kob/contracts/orders/KobCondAsk.ctor.json).
    let k = kob_consts();
    assert_eq!(k.ask_tpl[..4], [7, 11, 179, 178]);
    assert_eq!((k.ask_pre, k.ask_suf, k.bid_pre, k.bid_suf), (1, 1407, 1, 1343));
}

#[test]
fn state_lengths_are_the_contracts() {
    let p = pair(pubkey(1), pubkey(2));
    assert_eq!(p.state_bytes().len(), 429);
    assert_eq!(p.state_bytes().len(), pair_template().state_len);
    let o = long_order(pubkey(1));
    assert_eq!(o.state_bytes().len(), 321);
    assert_eq!(o.state_bytes().len(), open_template().state_len);
    // The market block is the first 207 bytes of both, byte for byte (the contracts compare them).
    assert_eq!(p.state_bytes()[..207], o.state_bytes()[..207]);
}

#[test]
fn the_pinned_sample_instance_decodes_and_re_encodes() {
    for base in [pair_base(), open_base()] {
        let c = &base.contracts.values().next().unwrap().compiled;
        let d = decode_redeem(&c.bytecode).expect("sample decodes");
        let again = match d {
            Decoded::Pair(p) => p.redeem(),
            Decoded::Open(o) => o.redeem(),
        };
        assert_eq!(again, c.bytecode);
    }
}
