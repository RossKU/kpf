//! Regenerates and checks the pinned compiled artifacts of `KpfPair` and `KpfOpen` (`contracts/artifacts/`).
//!
//! This is the only crate that links the SilverScript compiler (KOB's vendored v1.0.0, by path into `vendor/kob`).
//!
//! What is pinned (all under `contracts/artifacts/`):
//! - `KpfPair.json`, `KpfOpen.json`: the compiler's `SilAbiArtifact` of each contract, compiled with a fixed sample
//!   state (the template prefix and suffix, the ABI, the template hash);
//! - `manifest.json`: source and artifact digests, template hashes and cuts, and the `KobAsk` / `KobBid` evidence
//!   templates (hash, prefix and suffix length) that `KpfPair` is built with;
//! - `SHA256SUMS`: digests of the sources and of all of the above.
//!
//! Regenerate after any change to a contract (or the compiler pin) with
//! `CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write`; `check` (and `cargo test -p kpf-artifacts`) fails
//! on any difference between a fresh compile and the committed files.

use std::path::{Path, PathBuf};

use kpf_protocol::artifacts::{template_hash_of, KobConsts};
use kpf_protocol::state::{Market, Open, Pair, Pending, KIND_LIQ, LONG};
use kpf_protocol::tx::pubkey_of;
use serde_json::json;
use sha2::{Digest, Sha256};
use silverscript_abi::{ArtifactValue, SilAbiArtifact};
use silverscript_lang::compiler::{compile_to_sil_abi_artifact_with_options, CompileOptions};

/// The commit of the KOB checkout the compiler and the evidence templates come from (`README.md`).
pub const KOB_COMMIT: &str = "ed16148";

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("KPF root")
}

fn read_norm(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).replace("\r\n", "\n")
}

pub fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn b32(b: [u8; 32]) -> ArtifactValue {
    ArtifactValue::Bytes(b.to_vec())
}
fn int(v: i64) -> ArtifactValue {
    ArtifactValue::Int(v)
}

fn compile(name: &str, src: &str, ctor: &[ArtifactValue]) -> SilAbiArtifact {
    compile_to_sil_abi_artifact_with_options(src, ctor, CompileOptions::default()).unwrap_or_else(|e| panic!("compile {name}: {e:?}"))
}

fn sample_market() -> Market {
    Market {
        token_cov_id: kaspa_hash([0xc1; 32]),
        token_tpl_hash: kaspa_hash([0xc2; 32]),
        ext: kaspa_hash([0xc3; 32]),
        tpl_prefix_len: 1,
        tpl_suffix_len: 1,
        scale: 1_000_000,
        mmr_bps: 500,
        k_bps: 10_000,
        min_rest_daa: 600,
        liq_fee_bps: 50,
        keeper_fee_min: 1_000_000,
        min_size: 1_000_000,
        carrier: 20_000_000,
        idle_daa: 77_760_000,
        price_max: 100_000_000_000_000,
        bond_bps: 300,
        bond_min: 500_000_000,
        kc_bps: 10_000,
        g_bps: 200,
        challenge_daa: 6_000,
        min_evidence_life: 864_000,
    }
}

fn kaspa_hash(b: [u8; 32]) -> kaspa_consensus_core::Hash {
    kaspa_consensus_core::Hash::from_bytes(b)
}

/// The template facts of a compiled artifact: (hash, prefix length, suffix length).
fn facts(a: &SilAbiArtifact) -> ([u8; 32], i64, i64) {
    let c = &a.contracts.values().next().expect("one contract").compiled;
    let (pre, _, suf) = c.script_parts(&c.bytecode).expect("state span");
    assert_eq!(template_hash_of(pre, suf), c.template_hash, "template hash formula");
    (c.template_hash, pre.len() as i64, suf.len() as i64)
}

/// Everything `write` puts on disk, as (path relative to `contracts/`, content).
pub fn generate() -> Vec<(String, String)> {
    let r = root();
    let kob = r.join("vendor/kob/contracts/orders");
    let ask_src = read_norm(&kob.join("KobAsk.sil"));
    let bid_src = read_norm(&kob.join("KobBid.sil"));
    let pair_src = read_norm(&r.join("contracts/KpfPair.sil"));
    let open_src = read_norm(&r.join("contracts/KpfOpen.sil"));
    let pending_src = read_norm(&r.join("contracts/KpfPending.sil"));
    let m = sample_market();

    // KobAsk / KobBid: any valid constructor; the template does not depend on the state.
    let ask = compile(
        "KobAsk",
        &ask_src,
        &[
            vec![b32([3; 32]), b32(m.token_cov_id.as_bytes()), b32(m.token_tpl_hash.as_bytes())],
            [1, 1, m.scale, 1, 250_000_000, 0, 0, 0, 1_000_000_000_000, 0, 0, 0, 0, 0, 1, 10].map(int).to_vec(),
            vec![b32(m.ext.as_bytes())],
        ]
        .concat(),
    );
    let bid = compile(
        "KobBid",
        &bid_src,
        &[
            vec![b32([3; 32]), b32(m.token_cov_id.as_bytes()), b32(m.token_tpl_hash.as_bytes()), int(1), int(1), b32(m.ext.as_bytes())],
            [m.scale, 1_000_000, 250_000_000, 0, 0, 0, 1_000_000_000_000, 0, 0, 20_000_000, 0, 0, 0, 0, 1].map(int).to_vec(),
        ]
        .concat(),
    );
    let (at, ap, asf) = facts(&ask);
    let (bt, bp, bsf) = facts(&bid);
    let kc = KobConsts { ask_tpl: at, ask_pre: ap, ask_suf: asf, bid_tpl: bt, bid_pre: bp, bid_suf: bsf };

    let (k1, k2, k3) = (pubkey_of(&[1; 32]).unwrap(), pubkey_of(&[2; 32]).unwrap(), pubkey_of(&[3; 32]).unwrap());
    // KpfPending first: its template hash is a build constant of KpfPair (DESIGN 4.5d; no cycle, it needs no pair
    // template).
    let sample_pending = Pending {
        m: m.clone(),
        kind: KIND_LIQ,
        side: LONG,
        n: 1_000_000_000,
        bl: 900_000_000,
        bs: 1_200_000_000,
        quote: 940_000_000,
        cure_bound: 893_000_000,
        fee: 4_700_000_000,
        bond: 9_000_000_000,
        refund: 9_000_000_000,
        entry: 1_000_000_000,
        long_key: k1,
        short_key: k2,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        since: 1,
        proposer_key: k3,
        restore_hash: [0x2a; 32],
        cure_hash: [0x2b; 32],
        forfeit_hash: [0x2c; 32],
    };
    let pending = compile("KpfPending", &pending_src, &sample_pending.ctor_with(&kc));
    let (wt, _, _) = facts(&pending);
    let sample_pair = Pair {
        m: m.clone(),
        size: 1_000_000_000,
        bl: 900_000_000,
        bs: 1_200_000_000,
        entry: 1_000_000_000,
        long_key: k1,
        short_key: k2,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        tag_l: kaspa_hash([0x1a; 32]),
        tag_s: kaspa_hash([0x1b; 32]),
        since: 0,
    };
    let pair = compile("KpfPair", &pair_src, &sample_pair.ctor_with(&kc, wt));
    let (pt, _, _) = facts(&pair);
    let sample_open = Open {
        m: m.clone(),
        side: LONG,
        owner: k1,
        size_left: 2_000_000_000,
        limit: 1_050_000_000,
        my_rate: 100_000_000,
        min_counter_rate: 150_000_000,
        min_fill: 100_000_000,
        tip: 1_000_000,
        active_from: 0,
        expiry_daa: 1_000_000_000,
    };
    let open = compile("KpfOpen", &open_src, &sample_open.ctor_with(pt));

    let json = |a: &SilAbiArtifact| format!("{}\n", serde_json::to_string(a).expect("artifact json"));
    let (pair_json, open_json, pending_json) = (json(&pair), json(&open), json(&pending));
    let contract = |src: &str, file: &str, a: &SilAbiArtifact, art_json: &str| {
        let c = &a.contracts.values().next().expect("one contract").compiled;
        let (hash, pre, suf) = facts(a);
        let text = match file {
            "KpfPair.json" => &pair_src,
            "KpfOpen.json" => &open_src,
            _ => &pending_src,
        };
        json!({
            "source": src, "source_sha256": sha256_hex(text.as_bytes()),
            "artifact": file, "artifact_sha256": sha256_hex(art_json.as_bytes()),
            "template_hash": hex(&hash), "prefix_len": pre, "suffix_len": suf, "state_len": c.state_span.len,
        })
    };
    let kobc = |src: &str, text: &str, f: ([u8; 32], i64, i64)| {
        json!({ "source": src, "source_sha256": sha256_hex(text.as_bytes()), "template_hash": hex(&f.0), "prefix_len": f.1, "suffix_len": f.2 })
    };
    let manifest = json!({
        "format": "kpf-artifacts/1",
        "compiler": format!("silverscript-lang (vendor/kob @ {KOB_COMMIT}, SilverScript v1.0.0), CompileOptions::default(); {}", pair.compiler_version),
        "regenerate": "CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write",
        "kob_commit": KOB_COMMIT,
        "kob_ask": kobc("contracts/orders/KobAsk.sil", &ask_src, (at, ap, asf)),
        "kob_bid": kobc("contracts/orders/KobBid.sil", &bid_src, (bt, bp, bsf)),
        "kpf_pair": contract("contracts/KpfPair.sil", "KpfPair.json", &pair, &pair_json),
        "kpf_open": contract("contracts/KpfOpen.sil", "KpfOpen.json", &open, &open_json),
        "kpf_pending": contract("contracts/KpfPending.sil", "KpfPending.json", &pending, &pending_json),
    });
    let manifest_text = format!("{}\n", serde_json::to_string_pretty(&manifest).expect("manifest"));
    let sums = [
        ("KpfPair.sil", pair_src.clone()),
        ("KpfOpen.sil", open_src.clone()),
        ("KpfPending.sil", pending_src.clone()),
        ("artifacts/KpfPair.json", pair_json.clone()),
        ("artifacts/KpfOpen.json", open_json.clone()),
        ("artifacts/KpfPending.json", pending_json.clone()),
        ("artifacts/manifest.json", manifest_text.clone()),
    ]
    .iter()
    .map(|(p, t)| format!("{}  {p}\n", sha256_hex(t.as_bytes())))
    .collect::<String>();
    vec![
        ("artifacts/KpfPair.json".into(), pair_json),
        ("artifacts/KpfOpen.json".into(), open_json),
        ("artifacts/KpfPending.json".into(), pending_json),
        ("artifacts/manifest.json".into(), manifest_text),
        ("artifacts/SHA256SUMS".into(), sums),
    ]
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Writes the generated files under `contracts/`.
pub fn write() -> Vec<String> {
    let r = root().join("contracts");
    generate()
        .into_iter()
        .map(|(rel, text)| {
            std::fs::write(r.join(&rel), text).unwrap_or_else(|e| panic!("{rel}: {e}"));
            rel
        })
        .collect()
}

/// The differences between a fresh compile and the committed files (empty when they agree).
pub fn check() -> Vec<String> {
    let r = root().join("contracts");
    generate()
        .into_iter()
        .filter_map(|(rel, text)| match std::fs::read_to_string(r.join(&rel)) {
            Ok(have) if have.replace("\r\n", "\n") == text => None,
            Ok(_) => Some(format!("contracts/{rel} differs from a fresh compile")),
            Err(e) => Some(format!("contracts/{rel}: {e}")),
        })
        .collect()
}
