//! The pinned compiled artifacts of the committee family (`contracts/legacy/artifacts/committee/`): the committee `KpfCommittee`,
//! its `KpfEpochTable` and `KpfExit`, and `KpfOpenC`, `KpfPairC`, `KpfPendingC` (docs/DESIGN-COMMITTEE.md), with their own
//! manifest and `SHA256SUMS`. `kpf-artifacts check` (and its test) recompiles them and fails on any difference;
//! regenerate with `CARGO_BUILD_JOBS=2 cargo run -p kpf-artifacts --jobs 2 -- write`.

use std::sync::OnceLock;

use serde::Deserialize;
use silverscript_abi::SilAbiArtifact;

use crate::artifacts::Template;
use crate::cmarket::{CConsts, CKit};

const MANIFEST_JSON: &str = include_str!("../../../../contracts/legacy/artifacts/committee/manifest.json");
const EPOCH_TABLE_JSON: &str = include_str!("../../../../contracts/legacy/artifacts/committee/KpfEpochTable.json");
const EXIT_JSON: &str = include_str!("../../../../contracts/legacy/artifacts/committee/KpfExit.json");
const COMMITTEE_JSON: &str = include_str!("../../../../contracts/legacy/artifacts/committee/KpfCommittee.json");
const PENDING_JSON: &str = include_str!("../../../../contracts/legacy/artifacts/committee/KpfPendingC.json");
const PAIR_JSON: &str = include_str!("../../../../contracts/legacy/artifacts/committee/KpfPairC.json");
const OPEN_JSON: &str = include_str!("../../../../contracts/legacy/artifacts/committee/KpfOpenC.json");
const CHARGE_JSON: &str = include_str!("../../../../contracts/legacy/artifacts/committee/KpfCharge.json");
const PENDX_JSON: &str = include_str!("../../../../contracts/legacy/artifacts/committee/KpfPendingX.json");
const CROSS_JSON: &str = include_str!("../../../../contracts/legacy/artifacts/committee/KpfCrossC.json");

/// The nine artifacts by name, as embedded text (for digest checks).
pub fn artifact_texts() -> [(&'static str, &'static str); 9] {
    [
        ("KpfEpochTable", EPOCH_TABLE_JSON),
        ("KpfExit", EXIT_JSON),
        ("KpfCommittee", COMMITTEE_JSON),
        ("KpfPendingC", PENDING_JSON),
        ("KpfPairC", PAIR_JSON),
        ("KpfOpenC", OPEN_JSON),
        ("KpfCharge", CHARGE_JSON),
        ("KpfPendingX", PENDX_JSON),
        ("KpfCrossC", CROSS_JSON),
    ]
}

pub fn manifest_text() -> &'static str {
    MANIFEST_JSON
}

#[derive(Clone, Debug, Deserialize)]
pub struct CContract {
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
pub struct CTags {
    pub attest: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CManifest {
    pub format: String,
    pub compiler: String,
    pub contracts: std::collections::BTreeMap<String, CContract>,
    pub tags: CTags,
}

pub fn manifest() -> &'static CManifest {
    static M: OnceLock<CManifest> = OnceLock::new();
    M.get_or_init(|| serde_json::from_str(MANIFEST_JSON).expect("contracts/legacy/artifacts/committee/manifest.json"))
}

macro_rules! base {
    ($f:ident, $j:ident, $what:literal) => {
        pub fn $f() -> &'static SilAbiArtifact {
            static A: OnceLock<SilAbiArtifact> = OnceLock::new();
            A.get_or_init(|| serde_json::from_str($j).expect($what))
        }
    };
}
base!(epoch_table_base, EPOCH_TABLE_JSON, "KpfEpochTable.json");
base!(exit_base, EXIT_JSON, "KpfExit.json");
base!(committee_base, COMMITTEE_JSON, "KpfCommittee.json");
base!(pending_base, PENDING_JSON, "KpfPendingC.json");
base!(pair_base, PAIR_JSON, "KpfPairC.json");
base!(open_base, OPEN_JSON, "KpfOpenC.json");
base!(charge_base, CHARGE_JSON, "KpfCharge.json");
base!(pendx_base, PENDX_JSON, "KpfPendingX.json");
base!(cross_base, CROSS_JSON, "KpfCrossC.json");

/// The pinned templates of the cross-margin account and its pending claim, with the account's build constants.
pub fn xkit() -> &'static crate::cross::XKit {
    static K: OnceLock<crate::cross::XKit> = OnceLock::new();
    K.get_or_init(|| {
        let c = &kit().consts;
        let pending = Template::of("KpfPendingX", pendx_base());
        let consts = crate::cross::XConsts { pendx_tpl: pending.hash, epoch_table_tpl: c.epoch_table_tpl, epoch_table_pre: c.epoch_table_pre, epoch_table_suf: c.epoch_table_suf, attest_tag: c.attest_tag };
        crate::cross::XKit { cross: Template::of("KpfCrossC", cross_base()), pending, consts }
    })
}

/// The dispatch tag of `entry` in a pinned artifact (the 4 bytes a signature script pushes before the redeem script).
pub fn entry_tag(a: &SilAbiArtifact, entry: &str) -> [u8; 4] {
    *a.contracts.values().next().expect("one contract").entries[entry].dispatch_tag.as_bytes()
}

/// The pinned templates and the build constants they were compiled with.
pub fn kit() -> &'static CKit {
    static K: OnceLock<CKit> = OnceLock::new();
    K.get_or_init(|| {
        let (epoch_table, exit, committee, pending, pair, open) = (
            Template::of("KpfEpochTable", epoch_table_base()),
            Template::of("KpfExit", exit_base()),
            Template::of("KpfCommittee", committee_base()),
            Template::of("KpfPendingC", pending_base()),
            Template::of("KpfPairC", pair_base()),
            Template::of("KpfOpenC", open_base()),
        );
        let consts = CConsts {
            epoch_table_tpl: epoch_table.hash,
            epoch_table_pre: epoch_table.prefix.len() as i64,
            epoch_table_suf: epoch_table.suffix.len() as i64,
            attest_tag: entry_tag(epoch_table_base(), "attest"),
            exit_tpl: exit.hash,
            pending_tpl: pending.hash,
            pair_tpl: pair.hash,
        };
        let charge = Template::of("KpfCharge", charge_base());
        CKit { open, pair, pending, committee, epoch_table, exit, charge, consts }
    })
}
