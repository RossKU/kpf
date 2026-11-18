//! The pinned compiled artifacts of the oracle-market family (`contracts/artifacts/oracle/`): the slot `KpfBond`, the
//! judge `KpfStake`, and `KpfOpenO`, `KpfPairO`, `KpfPendingO` (docs/DESIGN-ORACLE-IMPL.md, third slice), with their own
//! manifest and `SHA256SUMS` beside the book market's. `kpf-artifacts check` (and its test) recompiles them and fails on
//! any difference; regenerate with `CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write`.

use std::sync::OnceLock;

use serde::Deserialize;
use silverscript_abi::SilAbiArtifact;

use crate::artifacts::Template;
use crate::oracle::{OConsts, OKit};

const MANIFEST_JSON: &str = include_str!("../../../contracts/artifacts/oracle/manifest.json");
const BOND_JSON: &str = include_str!("../../../contracts/artifacts/oracle/KpfBond.json");
const STAKE_JSON: &str = include_str!("../../../contracts/artifacts/oracle/KpfStake.json");
const PENDING_JSON: &str = include_str!("../../../contracts/artifacts/oracle/KpfPendingO.json");
const PAIR_JSON: &str = include_str!("../../../contracts/artifacts/oracle/KpfPairO.json");
const OPEN_JSON: &str = include_str!("../../../contracts/artifacts/oracle/KpfOpenO.json");

/// The five artifacts by name, as embedded text (for digest checks).
pub fn artifact_texts() -> [(&'static str, &'static str); 5] {
    [("KpfBond", BOND_JSON), ("KpfStake", STAKE_JSON), ("KpfPendingO", PENDING_JSON), ("KpfPairO", PAIR_JSON), ("KpfOpenO", OPEN_JSON)]
}

pub fn manifest_text() -> &'static str {
    MANIFEST_JSON
}

#[derive(Clone, Debug, Deserialize)]
pub struct OContract {
    pub source: String,
    pub source_sha256: String,
    pub artifact: String,
    pub artifact_sha256: String,
    pub template_hash: String,
    pub prefix_len: usize,
    pub suffix_len: usize,
    pub state_len: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OTags {
    pub bind: String,
    pub open: String,
    pub resolve: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OManifest {
    pub format: String,
    pub compiler: String,
    pub contracts: std::collections::BTreeMap<String, OContract>,
    pub tags: OTags,
}

pub fn manifest() -> &'static OManifest {
    static M: OnceLock<OManifest> = OnceLock::new();
    M.get_or_init(|| serde_json::from_str(MANIFEST_JSON).expect("contracts/artifacts/oracle/manifest.json"))
}

macro_rules! base {
    ($f:ident, $j:ident, $what:literal) => {
        pub fn $f() -> &'static SilAbiArtifact {
            static A: OnceLock<SilAbiArtifact> = OnceLock::new();
            A.get_or_init(|| serde_json::from_str($j).expect($what))
        }
    };
}
base!(bond_base, BOND_JSON, "KpfBond.json");
base!(stake_base, STAKE_JSON, "KpfStake.json");
base!(pending_base, PENDING_JSON, "KpfPendingO.json");
base!(pair_base, PAIR_JSON, "KpfPairO.json");
base!(open_base, OPEN_JSON, "KpfOpenO.json");

fn tag(a: &SilAbiArtifact, entry: &str) -> [u8; 4] {
    *a.contracts.values().next().expect("one contract").entries[entry].dispatch_tag.as_bytes()
}

/// The pinned templates and the build constants they were compiled with.
pub fn kit() -> &'static OKit {
    static K: OnceLock<OKit> = OnceLock::new();
    K.get_or_init(|| {
        let (bond, stake, pending, pair, open) = (
            Template::of("KpfBond", bond_base()),
            Template::of("KpfStake", stake_base()),
            Template::of("KpfPendingO", pending_base()),
            Template::of("KpfPairO", pair_base()),
            Template::of("KpfOpenO", open_base()),
        );
        let consts = OConsts {
            bond_tpl: bond.hash,
            bond_pre: bond.prefix.len() as i64,
            bond_suf: bond.suffix.len() as i64,
            bind_tag: tag(bond_base(), "bind"),
            judge_tpl: stake.hash,
            judge_pre: stake.prefix.len() as i64,
            judge_suf: stake.suffix.len() as i64,
            open_tag: tag(stake_base(), "open"),
            resolve_tag: tag(stake_base(), "resolve"),
            pending_tpl: pending.hash,
            pair_tpl: pair.hash,
        };
        OKit { open, pair, pending, bond, stake, consts }
    })
}
