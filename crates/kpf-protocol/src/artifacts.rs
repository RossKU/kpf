//! The pinned compiled artifacts of `KpfPair`, `KpfOpen`, `KpfPending` and `KpfLock`.
//!
//! The compiler is not a dependency of this crate. `contracts/artifacts/` holds the compiler's output (the
//! `SilAbiArtifact` JSON of each contract compiled with a fixed sample state), a manifest with source and artifact
//! digests, the template prefix and suffix lengths and the template hashes, and `SHA256SUMS`. They are embedded here.
//! `kpf-artifacts check` (and its test) recompiles the sources with the vendored compiler and fails if one byte
//! differs. Regenerate with `cargo run -p kpf-artifacts -- write` (see `README.md`).
//!
//! An instance of a template is `prefix || state || suffix`. The state is a fixed sequence of canonical pushes
//! (`0x08` + 8-byte script number, `0x20` + 32 bytes), see [`crate::state`].

use std::sync::OnceLock;

use serde::Deserialize;
use silverscript_abi::SilAbiArtifact;

use crate::error::{Error, Result};

const MANIFEST_JSON: &str = include_str!("../../../contracts/artifacts/manifest.json");
const PAIR_JSON: &str = include_str!("../../../contracts/artifacts/KpfPair.json");
const OPEN_JSON: &str = include_str!("../../../contracts/artifacts/KpfOpen.json");
const PENDING_JSON: &str = include_str!("../../../contracts/artifacts/KpfPending.json");
const LOCK_JSON: &str = include_str!("../../../contracts/artifacts/KpfLock.json");
// KOB's published artifacts (the read-only checkout `vendor/kob`, never copied): the evidence orders and the reference
// KCC-20 token program. Their ABIs build the signature scripts of the KOB and token inputs of an evidence transaction;
// their templates decode live orders. `tests/pinned.rs` pins their digests and checks the template hashes against the
// constants `KpfPair` was built with.
const KOB_ASK_JSON: &str = include_str!("../../../vendor/kob/contracts/artifacts/KobAsk.json");
const KOB_BID_JSON: &str = include_str!("../../../vendor/kob/contracts/artifacts/KobBid.json");
const KCC20_JSON: &str = include_str!("../../../vendor/kob/contracts/artifacts/KCC20Ref.json");

/// KOB's `KobAsk` artifact as embedded, as text (for digest checks).
pub fn kob_ask_artifact_text() -> &'static str {
    KOB_ASK_JSON
}
/// KOB's `KobBid` artifact as embedded, as text.
pub fn kob_bid_artifact_text() -> &'static str {
    KOB_BID_JSON
}
/// KOB's reference KCC-20 (3 / 3 slots) artifact as embedded, as text.
pub fn kcc20_artifact_text() -> &'static str {
    KCC20_JSON
}

/// The embedded manifest, as text (for digest checks).
pub fn manifest_text() -> &'static str {
    MANIFEST_JSON
}
/// The embedded pair artifact, as text.
pub fn pair_artifact_text() -> &'static str {
    PAIR_JSON
}
/// The embedded order artifact, as text.
pub fn open_artifact_text() -> &'static str {
    OPEN_JSON
}
/// The embedded pending artifact, as text.
pub fn pending_artifact_text() -> &'static str {
    PENDING_JSON
}

/// The pinned `KpfPending` artifact (DESIGN 4.5d).
pub fn pending_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| serde_json::from_str(PENDING_JSON).expect("contracts/artifacts/KpfPending.json"))
}

/// The `KpfPending` template (its hash is the `PENDING_TPL` constant of `KpfPair`).
pub fn pending_template() -> &'static Template {
    static T: OnceLock<Template> = OnceLock::new();
    T.get_or_init(|| Template::of("KpfPending", pending_base()))
}

/// One pinned contract of the manifest.
#[derive(Clone, Debug, Deserialize)]
pub struct ManifestContract {
    pub source: String,
    pub source_sha256: String,
    pub artifact: String,
    pub artifact_sha256: String,
    pub template_hash: String,
    pub prefix_len: usize,
    pub suffix_len: usize,
    pub state_len: usize,
}

/// A KOB evidence template (`KobAsk` or `KobBid`) the pair pins as build constants.
#[derive(Clone, Debug, Deserialize)]
pub struct ManifestKob {
    pub source: String,
    pub source_sha256: String,
    pub template_hash: String,
    pub prefix_len: i64,
    pub suffix_len: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub compiler: String,
    pub kob_commit: String,
    pub kob_ask: ManifestKob,
    pub kob_bid: ManifestKob,
    pub kpf_pair: ManifestContract,
    pub kpf_open: ManifestContract,
    pub kpf_pending: ManifestContract,
    pub kpf_lock: ManifestContract,
}

/// The parsed manifest.
pub fn manifest() -> &'static Manifest {
    static M: OnceLock<Manifest> = OnceLock::new();
    M.get_or_init(|| serde_json::from_str(MANIFEST_JSON).expect("contracts/artifacts/manifest.json"))
}

/// The six build constants of `KpfPair`: the `KobAsk` and `KobBid` templates (hash, prefix length, suffix length).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KobConsts {
    pub ask_tpl: [u8; 32],
    pub ask_pre: i64,
    pub ask_suf: i64,
    pub bid_tpl: [u8; 32],
    pub bid_pre: i64,
    pub bid_suf: i64,
}

/// The pinned KOB evidence constants.
pub fn kob_consts() -> KobConsts {
    let m = manifest();
    KobConsts {
        ask_tpl: crate::error::hex32(&m.kob_ask.template_hash).expect("ask template hash"),
        ask_pre: m.kob_ask.prefix_len,
        ask_suf: m.kob_ask.suffix_len,
        bid_tpl: crate::error::hex32(&m.kob_bid.template_hash).expect("bid template hash"),
        bid_pre: m.kob_bid.prefix_len,
        bid_suf: m.kob_bid.suffix_len,
    }
}

/// The pinned `KpfPair` artifact (compiled with the manifest's sample state).
pub fn pair_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| serde_json::from_str(PAIR_JSON).expect("contracts/artifacts/KpfPair.json"))
}

/// The pinned `KpfOpen` artifact.
pub fn open_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| serde_json::from_str(OPEN_JSON).expect("contracts/artifacts/KpfOpen.json"))
}

/// The template hash of a prefix and suffix, as the contracts compute it:
/// `blake3(len(prefix) as 8 bytes || prefix || len(suffix) as 8 bytes || suffix)`, lengths little-endian.
pub fn template_hash_of(prefix: &[u8], suffix: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(&(prefix.len() as u64).to_le_bytes());
    h.update(prefix);
    h.update(&(suffix.len() as u64).to_le_bytes());
    h.update(suffix);
    *h.finalize().as_bytes()
}

/// A contract template: what surrounds the state.
#[derive(Clone, Debug)]
pub struct Template {
    pub name: &'static str,
    pub prefix: Vec<u8>,
    pub suffix: Vec<u8>,
    pub state_len: usize,
    pub hash: [u8; 32],
}

impl Template {
    /// The template of the single contract of an artifact (its compiled sample instance gives the cut).
    pub fn of(name: &'static str, art: &SilAbiArtifact) -> Template {
        let c = &art.contracts.values().next().expect("one contract").compiled;
        let (pre, _, suf) = c.script_parts(&c.bytecode).expect("state span");
        Template { name, prefix: pre.to_vec(), suffix: suf.to_vec(), state_len: c.state_span.len, hash: c.template_hash }
    }

    /// The redeem script of an instance with this encoded state.
    pub fn redeem(&self, state: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.prefix.len() + state.len() + self.suffix.len());
        v.extend_from_slice(&self.prefix);
        v.extend_from_slice(state);
        v.extend_from_slice(&self.suffix);
        v
    }

    /// The encoded state of a redeem script of this template, or why it is not an instance.
    pub fn state_of<'a>(&self, redeem: &'a [u8]) -> Result<&'a [u8]> {
        let want = self.prefix.len() + self.state_len + self.suffix.len();
        if redeem.len() != want {
            return Err(Error::Decode(format!("{}: script is {} bytes, an instance is {want}", self.name, redeem.len())));
        }
        if !redeem.starts_with(&self.prefix) || !redeem.ends_with(&self.suffix) {
            return Err(Error::Decode(format!("{}: script does not match the pinned template", self.name)));
        }
        Ok(&redeem[self.prefix.len()..self.prefix.len() + self.state_len])
    }
}

/// The `KpfPair` template.
pub fn pair_template() -> &'static Template {
    static T: OnceLock<Template> = OnceLock::new();
    T.get_or_init(|| Template::of("KpfPair", pair_base()))
}

/// The `KpfOpen` template.
pub fn open_template() -> &'static Template {
    static T: OnceLock<Template> = OnceLock::new();
    T.get_or_init(|| Template::of("KpfOpen", open_base()))
}

/// The artifact of an instance: `base` with its bytecode replaced by prefix + `state` + suffix.
pub fn splice(base: &SilAbiArtifact, state: &[u8]) -> SilAbiArtifact {
    let c = &base.contracts.values().next().expect("one contract").compiled;
    let (pre, _, suf) = c.script_parts(&c.bytecode).expect("state span");
    let mut a = base.clone();
    let cc = a.contracts.values_mut().next().expect("one contract");
    assert_eq!(state.len(), cc.compiled.state_span.len, "state length");
    cc.compiled.bytecode = [pre, state, suf].concat();
    a
}

/// The name of the single contract of an artifact.
pub fn contract_name(art: &SilAbiArtifact) -> &str {
    art.contracts.keys().next().expect("one contract")
}

/// KOB's `KobAsk` artifact.
pub fn ask_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| serde_json::from_str(KOB_ASK_JSON).expect("KobAsk.json"))
}

/// KOB's `KobBid` artifact.
pub fn bid_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| serde_json::from_str(KOB_BID_JSON).expect("KobBid.json"))
}

/// KOB's reference KCC-20 token program (3 / 3 slots, 112-byte state).
pub fn token_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| serde_json::from_str(KCC20_JSON).expect("KCC20Ref.json"))
}

/// The `KobAsk` template.
pub fn ask_template() -> &'static Template {
    static T: OnceLock<Template> = OnceLock::new();
    T.get_or_init(|| Template::of("KobAsk", ask_base()))
}

/// The `KobBid` template.
pub fn bid_template() -> &'static Template {
    static T: OnceLock<Template> = OnceLock::new();
    T.get_or_init(|| Template::of("KobBid", bid_base()))
}

/// The reference KCC-20 template.
pub fn token_template() -> &'static Template {
    static T: OnceLock<Template> = OnceLock::new();
    T.get_or_init(|| Template::of("KCC20Ref", token_base()))
}

/// The embedded lock artifact, as text (DESIGN 4.5f).
pub fn lock_artifact_text() -> &'static str {
    LOCK_JSON
}

/// The pinned `KpfLock` artifact (DESIGN 4.5f).
pub fn lock_base() -> &'static SilAbiArtifact {
    static A: OnceLock<SilAbiArtifact> = OnceLock::new();
    A.get_or_init(|| serde_json::from_str(LOCK_JSON).expect("contracts/artifacts/KpfLock.json"))
}

/// The `KpfLock` template (its hash is the `LOCK_TPL` constant of `KpfPending`).
pub fn lock_template() -> &'static Template {
    static T: OnceLock<Template> = OnceLock::new();
    T.get_or_init(|| Template::of("KpfLock", lock_base()))
}
