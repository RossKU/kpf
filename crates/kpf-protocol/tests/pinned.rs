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

/// KOB's artifacts, embedded from the read-only checkout `vendor/kob`: digests pinned here, so that a different checkout
/// is noticed, and their templates are the ones `KpfPair` was built with.
#[test]
fn the_embedded_kob_artifacts_are_the_pinned_ones() {
    let digests = [
        ("KobAsk", kob_ask_artifact_text(), "08f35a7babcd0f5790b130be80342ee7b5b065a49f92f3d23b5c00f433a4b0cf"),
        ("KobBid", kob_bid_artifact_text(), "9e0d2613700b80a9d657107e6921dd68cec541ab32dfaa5abc47287658e4be43"),
        ("KCC20Ref", kcc20_artifact_text(), "0149cff3d8b10236eab2c0d2a92e88beb89b726dd9044e4ee1a0a3d4cf6b956e"),
    ];
    for (name, text, want) in digests {
        assert_eq!(sha(norm(text).as_bytes()), want, "{name}: vendor/kob/contracts/artifacts/{name}.json is not the checkout KPF was built against");
    }
    let m = manifest();
    let k = kob_consts();
    for (t, c) in [(ask_template(), &m.kob_ask), (bid_template(), &m.kob_bid)] {
        assert_eq!(t.hash.iter().map(|b| format!("{b:02x}")).collect::<String>(), c.template_hash, "{}", t.name);
        assert_eq!((t.prefix.len() as i64, t.suffix.len() as i64), (c.prefix_len, c.suffix_len), "{}", t.name);
    }
    assert_eq!(ask_template().hash, k.ask_tpl);
    assert_eq!(bid_template().hash, k.bid_tpl);
    assert_eq!((ask_template().state_len, bid_template().state_len, token_template().state_len), (276, 285, 112));
    // The reference token program is the one KOB's registry lists as `kcc20-ref-3x3`.
    assert_eq!(token_template().hash.iter().map(|b| format!("{b:02x}")).collect::<String>(), "173ca6a796c2c05f171c31b9a73aaca161a3226a9e8f57d2f3d833ff18dbe41b");
    assert_eq!((token_template().prefix.len(), token_template().suffix.len()), (1, 2802));
    // The market of the fixtures names exactly this program.
    kpf_protocol::kob::supports_token_program(&kob_market()).unwrap();
}
