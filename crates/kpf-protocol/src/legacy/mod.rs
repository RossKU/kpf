//! The three earlier families: the KOB-evidence market (`KpfPair`, `KpfOpen`, `KpfPending`, `KpfLock`), the oracle
//! market (`KpfOpenO`, `KpfPairO`, `KpfPendingO`, `KpfBond`, `KpfStake`) and the committee family (`KpfCommittee`,
//! `KpfEpochTable`, `KpfExit`, `KpfCharge`, `KpfOpenC`, `KpfPairC`, `KpfPendingC`, `KpfCrossC`, `KpfPendingX`), with their
//! contracts under `contracts/legacy/`. They work and are tested; the family being built is the thin one
//! (`contracts/*.sil`, `docs/DESIGN-COMMITTEE.md` 8a), which uses the committee's committee and epoch table until it has its
//! own. The crate root re-exports every module here under its old path.

pub mod artifacts;
pub mod bond;
pub mod build;
pub mod cmarket;
pub mod committee;
pub mod committee_artifacts;
pub mod committee_build;
pub mod cross;
pub mod evidence;
pub mod fixtures;
pub mod judge;
pub mod kob;
pub mod lock;
pub mod oracle;
pub mod oracle_artifacts;
pub mod oracle_build;
pub mod payoff;
pub mod pending;
pub mod sim;
pub mod state;
