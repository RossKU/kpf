//! The pinned compiled artifacts of the thin family (`contracts/artifacts/`): `KpfSlot`, the one script of every
//! object, and the scripts of the operations as a library holds them. The compiler is not a dependency of this crate;
//! `kpf-artifacts` regenerates and checks these files.
//!
//! An object's script is `KpfSlot`'s with two values of 32 bytes in it: the covenant id of the library whose scripts
//! may move it, and the hash of its state. A script of an operation has no value of its own: its bytes are the same
//! in every library, so its address is too.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;
use silverscript_abi::SilAbiArtifact;

const MANIFEST_JSON: &str = include_str!("../../../contracts/artifacts/manifest.json");
const TEXTS: [(&str, &str); 73] = [
    ("KpfSlot", include_str!("../../../contracts/artifacts/KpfSlot.json")),
    ("KpfTAdd", include_str!("../../../contracts/artifacts/KpfTAdd.json")),
    ("KpfTBoth", include_str!("../../../contracts/artifacts/KpfTBoth.json")),
    ("KpfTClose", include_str!("../../../contracts/artifacts/KpfTClose.json")),
    ("KpfTSettle", include_str!("../../../contracts/artifacts/KpfTSettle.json")),
    ("KpfTLate", include_str!("../../../contracts/artifacts/KpfTLate.json")),
    ("KpfTDispute", include_str!("../../../contracts/artifacts/KpfTDispute.json")),
    ("KpfTProve", include_str!("../../../contracts/artifacts/KpfTProve.json")),
    ("KpfTLiq", include_str!("../../../contracts/artifacts/KpfTLiq.json")),
    ("KpfTLiqSettle", include_str!("../../../contracts/artifacts/KpfTLiqSettle.json")),
    ("KpfTCure", include_str!("../../../contracts/artifacts/KpfTCure.json")),
    ("KpfTLiqDispute", include_str!("../../../contracts/artifacts/KpfTLiqDispute.json")),
    ("KpfTLiqProve", include_str!("../../../contracts/artifacts/KpfTLiqProve.json")),
    ("KpfTExit", include_str!("../../../contracts/artifacts/KpfTExit.json")),
    ("KpfTExitEnd", include_str!("../../../contracts/artifacts/KpfTExitEnd.json")),
    ("KpfTExitRefute", include_str!("../../../contracts/artifacts/KpfTExitRefute.json")),
    ("KpfTMatch", include_str!("../../../contracts/artifacts/KpfTMatch.json")),
    ("KpfTCancel", include_str!("../../../contracts/artifacts/KpfTCancel.json")),
    ("KpfTMerge", include_str!("../../../contracts/artifacts/KpfTMerge.json")),
    ("KpfTNovate", include_str!("../../../contracts/artifacts/KpfTNovate.json")),
    ("KpfTPlace", include_str!("../../../contracts/artifacts/KpfTPlace.json")),
    ("KpfTOpen", include_str!("../../../contracts/artifacts/KpfTOpen.json")),
    ("KpfTJoin", include_str!("../../../contracts/artifacts/KpfTJoin.json")),
    ("KpfTTurn", include_str!("../../../contracts/artifacts/KpfTTurn.json")),
    ("KpfTFold", include_str!("../../../contracts/artifacts/KpfTFold.json")),
    ("KpfTLeave", include_str!("../../../contracts/artifacts/KpfTLeave.json")),
    ("KpfTReplace", include_str!("../../../contracts/artifacts/KpfTReplace.json")),
    ("KpfTNote", include_str!("../../../contracts/artifacts/KpfTNote.json")),
    ("KpfTCite", include_str!("../../../contracts/artifacts/KpfTCite.json")),
    ("KpfTChange", include_str!("../../../contracts/artifacts/KpfTChange.json")),
    ("KpfTChangeSettle", include_str!("../../../contracts/artifacts/KpfTChangeSettle.json")),
    ("KpfTChangeDispute", include_str!("../../../contracts/artifacts/KpfTChangeDispute.json")),
    ("KpfTChangeLate", include_str!("../../../contracts/artifacts/KpfTChangeLate.json")),
    ("KpfTTake", include_str!("../../../contracts/artifacts/KpfTTake.json")),
    ("KpfTPoint", include_str!("../../../contracts/artifacts/KpfTPoint.json")),
    ("KpfTStepRow", include_str!("../../../contracts/artifacts/KpfTStepRow.json")),
    ("KpfTStepPrice", include_str!("../../../contracts/artifacts/KpfTStepPrice.json")),
    ("KpfTStepEnd", include_str!("../../../contracts/artifacts/KpfTStepEnd.json")),
    ("KpfTStepLiq", include_str!("../../../contracts/artifacts/KpfTStepLiq.json")),
    ("KpfTKind", include_str!("../../../contracts/artifacts/KpfTKind.json")),
    ("KpfTShardOpen", include_str!("../../../contracts/artifacts/KpfTShardOpen.json")),
    ("KpfTOpenC", include_str!("../../../contracts/artifacts/KpfTOpenC.json")),
    ("KpfTAddC", include_str!("../../../contracts/artifacts/KpfTAddC.json")),
    ("KpfTPass", include_str!("../../../contracts/artifacts/KpfTPass.json")),
    ("KpfTJoinC", include_str!("../../../contracts/artifacts/KpfTJoinC.json")),
    ("KpfTAcctEnd", include_str!("../../../contracts/artifacts/KpfTAcctEnd.json")),
    ("KpfTShardEnd", include_str!("../../../contracts/artifacts/KpfTShardEnd.json")),
    ("KpfTCallEnd", include_str!("../../../contracts/artifacts/KpfTCallEnd.json")),
    ("KpfTCStep", include_str!("../../../contracts/artifacts/KpfTCStep.json")),
    ("KpfTCallPay", include_str!("../../../contracts/artifacts/KpfTCallPay.json")),
    ("KpfTCallEnter", include_str!("../../../contracts/artifacts/KpfTCallEnter.json")),
    ("KpfTShardPrice", include_str!("../../../contracts/artifacts/KpfTShardPrice.json")),
    ("KpfTDrawNote", include_str!("../../../contracts/artifacts/KpfTDrawNote.json")),
    ("KpfTLeft", include_str!("../../../contracts/artifacts/KpfTLeft.json")),
    ("KpfTLeftAll", include_str!("../../../contracts/artifacts/KpfTLeftAll.json")),
    ("KpfTCRow", include_str!("../../../contracts/artifacts/KpfTCRow.json")),
    ("KpfTCEnd", include_str!("../../../contracts/artifacts/KpfTCEnd.json")),
    ("KpfTCSplit", include_str!("../../../contracts/artifacts/KpfTCSplit.json")),
    ("KpfTCMark", include_str!("../../../contracts/artifacts/KpfTCMark.json")),
    ("KpfTCTok", include_str!("../../../contracts/artifacts/KpfTCTok.json")),
    ("KpfTOwed", include_str!("../../../contracts/artifacts/KpfTOwed.json")),
    ("KpfTAddSpot", include_str!("../../../contracts/artifacts/KpfTAddSpot.json")),
    ("KpfTVaultPay", include_str!("../../../contracts/artifacts/KpfTVaultPay.json")),
    ("KpfTMarkNote", include_str!("../../../contracts/artifacts/KpfTMarkNote.json")),
    ("KpfTEnterS", include_str!("../../../contracts/artifacts/KpfTEnterS.json")),
    ("KpfTCEnter", include_str!("../../../contracts/artifacts/KpfTCEnter.json")),
    ("KpfTEnterOff", include_str!("../../../contracts/artifacts/KpfTEnterOff.json")),
    ("KpfTDrawEntry", include_str!("../../../contracts/artifacts/KpfTDrawEntry.json")),
    ("KpfTLeftVoid", include_str!("../../../contracts/artifacts/KpfTLeftVoid.json")),
    ("KpfTSweep", include_str!("../../../contracts/artifacts/KpfTSweep.json")),
    ("KpfTPay", include_str!("../../../contracts/artifacts/KpfTPay.json")),
    ("KpfNote", include_str!("../../../contracts/artifacts/KpfNote.json")),
    ("KpfFeePool", include_str!("../../../contracts/artifacts/KpfFeePool.json")),
];

/// The object's script.
pub const SLOT: &str = "KpfSlot";
/// Where a library's oracle fees gather: an object under the library's own id, written at its genesis.
pub const FEE_POOL: &str = "KpfFeePool";
/// A price the committee signed, written once under the library's id by `KpfTNote` and cited by `KpfTCite`.
pub const NOTE: &str = "KpfNote";
/// The scripts that take no oracle fees of their own use: they collect them.
pub const COLLECTORS: [&str; 2] = ["KpfTSweep", "KpfTPay"];
/// The scripts of a library, in the order its genesis writes them.
pub const SCRIPTS: [&str; 70] = ["KpfTAdd", "KpfTBoth", "KpfTClose", "KpfTSettle", "KpfTLate", "KpfTDispute", "KpfTProve", "KpfTLiq", "KpfTLiqSettle", "KpfTCure", "KpfTLiqDispute", "KpfTLiqProve", "KpfTExit", "KpfTExitEnd", "KpfTExitRefute", "KpfTMatch", "KpfTCancel", "KpfTMerge", "KpfTNovate", "KpfTPlace", "KpfTOpen", "KpfTJoin", "KpfTTurn", "KpfTFold", "KpfTLeave", "KpfTReplace", "KpfTNote", "KpfTCite", "KpfTChange", "KpfTChangeSettle", "KpfTChangeDispute", "KpfTChangeLate", "KpfTTake", "KpfTKind", "KpfTPoint", "KpfTStepRow", "KpfTStepPrice", "KpfTStepEnd", "KpfTStepLiq", "KpfTShardOpen", "KpfTOpenC", "KpfTAddC", "KpfTPass", "KpfTJoinC", "KpfTAcctEnd", "KpfTShardEnd", "KpfTCallEnd", "KpfTCStep", "KpfTCallPay", "KpfTCallEnter", "KpfTShardPrice", "KpfTDrawNote", "KpfTLeft", "KpfTLeftAll", "KpfTLeftVoid", "KpfTCRow", "KpfTCEnd", "KpfTCSplit", "KpfTCMark", "KpfTCTok", "KpfTOwed", "KpfTAddSpot", "KpfTVaultPay", "KpfTMarkNote", "KpfTEnterS", "KpfTCEnter", "KpfTEnterOff", "KpfTDrawEntry", "KpfTSweep", "KpfTPay"];

/// (name, the artifact's text) of the pinned artifacts.
pub fn artifact_texts() -> &'static [(&'static str, &'static str)] {
    &TEXTS
}

pub fn manifest_text() -> &'static str {
    MANIFEST_JSON
}

#[derive(Clone, Debug, Deserialize)]
pub struct TContract {
    pub source: String,
    pub source_sha256: String,
    pub artifact: String,
    pub artifact_sha256: String,
    pub script_len: usize,
}

/// Where the two values of an object's script are.
#[derive(Clone, Debug, Deserialize)]
pub struct TSlot {
    pub lib_at: usize,
    pub state_at: usize,
    pub sample_lib: String,
    pub sample_state: String,
}

/// Where the writer's key is in a price note's script (the state's hash is where an object's is).
#[derive(Clone, Debug, Deserialize)]
pub struct TNote {
    pub owner_at: usize,
    pub sample_owner: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TManifest {
    pub format: String,
    pub compiler: String,
    pub slot: TSlot,
    pub note: TNote,
    pub contracts: BTreeMap<String, TContract>,
}

pub fn manifest() -> &'static TManifest {
    static M: OnceLock<TManifest> = OnceLock::new();
    M.get_or_init(|| serde_json::from_str(MANIFEST_JSON).expect("contracts/artifacts/manifest.json"))
}

fn bases() -> &'static BTreeMap<&'static str, SilAbiArtifact> {
    static A: OnceLock<BTreeMap<&'static str, SilAbiArtifact>> = OnceLock::new();
    A.get_or_init(|| TEXTS.iter().map(|(n, t)| (*n, serde_json::from_str(t).unwrap_or_else(|e| panic!("contracts/artifacts/{n}.json: {e}")))).collect())
}

/// The pinned artifact of a contract of the family (its ABI encodes the signature script of an entry).
pub fn base(name: &str) -> Option<&'static SilAbiArtifact> {
    bases().get(name)
}

fn code(a: &SilAbiArtifact) -> Vec<u8> {
    a.contracts.values().next().expect("one contract").compiled.bytecode.clone()
}

/// The scripts of the family as bytes.
#[derive(Clone, Debug)]
pub struct TKit {
    /// `KpfSlot` with the manifest's sample values.
    slot: Vec<u8>,
    lib_at: usize,
    state_at: usize,
    scripts: BTreeMap<&'static str, Vec<u8>>,
    /// `KpfFeePool` with the manifest's sample state.
    fee_pool: Vec<u8>,
    /// `KpfNote` with the manifest's sample state and writer.
    note: Vec<u8>,
    owner_at: usize,
}

pub fn kit() -> &'static TKit {
    static K: OnceLock<TKit> = OnceLock::new();
    K.get_or_init(|| {
        let m = manifest();
        let slot = code(&bases()[SLOT]);
        assert!(m.slot.lib_at + 32 <= slot.len() && m.slot.state_at + 32 <= slot.len() && (m.slot.lib_at >= m.slot.state_at + 32 || m.slot.state_at >= m.slot.lib_at + 32), "the slot's two values");
        TKit { slot, lib_at: m.slot.lib_at, state_at: m.slot.state_at, scripts: SCRIPTS.iter().map(|n| (*n, code(&bases()[n]))).collect(), fee_pool: code(&bases()[FEE_POOL]), note: code(&bases()[NOTE]), owner_at: m.note.owner_at }
    })
}

impl TKit {
    /// The script of an object of the library `lib` whose state hashes to `state`.
    pub fn object(&self, lib: &[u8; 32], state: &[u8; 32]) -> Vec<u8> {
        let mut s = self.slot.clone();
        s[self.lib_at..self.lib_at + 32].copy_from_slice(lib);
        s[self.state_at..self.state_at + 32].copy_from_slice(state);
        s
    }

    /// (the library's id, the state's hash) of a script, if it is an object's.
    pub fn read(&self, script: &[u8]) -> Option<([u8; 32], [u8; 32])> {
        if script.len() != self.slot.len() {
            return None;
        }
        let lib: [u8; 32] = script[self.lib_at..self.lib_at + 32].try_into().ok()?;
        let state: [u8; 32] = script[self.state_at..self.state_at + 32].try_into().ok()?;
        (self.object(&lib, &state) == script).then_some((lib, state))
    }

    /// The script of a library's fee pool whose state hashes to `state` (the same in every library: it holds no id).
    pub fn fee_pool(&self, state: &[u8; 32]) -> Vec<u8> {
        let mut s = self.fee_pool.clone();
        s[self.state_at..self.state_at + 32].copy_from_slice(state);
        s
    }

    /// The script of a price note whose state hashes to `state`, written by `owner` (the same in every library).
    pub fn note(&self, state: &[u8; 32], owner: &[u8; 32]) -> Vec<u8> {
        let mut s = self.note.clone();
        s[self.state_at..self.state_at + 32].copy_from_slice(state);
        s[self.owner_at..self.owner_at + 32].copy_from_slice(owner);
        s
    }

    /// (the state's hash, the writer's key) of a script, if it is a price note's.
    pub fn read_note(&self, script: &[u8]) -> Option<([u8; 32], [u8; 32])> {
        if script.len() != self.note.len() {
            return None;
        }
        let state: [u8; 32] = script[self.state_at..self.state_at + 32].try_into().ok()?;
        let owner: [u8; 32] = script[self.owner_at..self.owner_at + 32].try_into().ok()?;
        (self.note(&state, &owner) == script).then_some((state, owner))
    }

    /// The state's hash of a script, if it is a fee pool's.
    pub fn read_fee_pool(&self, script: &[u8]) -> Option<[u8; 32]> {
        if script.len() != self.fee_pool.len() {
            return None;
        }
        let state: [u8; 32] = script[self.state_at..self.state_at + 32].try_into().ok()?;
        (self.fee_pool(&state) == script).then_some(state)
    }

    /// The length of every object's script (what a script of the library is told to read of an input).
    pub fn object_len(&self) -> usize {
        self.slot.len()
    }

    /// The bytes of a script of the library.
    pub fn script(&self, name: &str) -> Option<&[u8]> {
        self.scripts.get(name).map(|v| v.as_slice())
    }
}
