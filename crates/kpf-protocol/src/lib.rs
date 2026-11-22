//! KPF protocol core (testnet-10 research prototype).
//!
//! Everything that encodes, builds, signs or checks a transaction of the four KPF contracts, shared by the
//! command line (`kpf-cli`) and the engine tests (`kpf-tests`).
//!
//! The family being built is the thin one: [`thin`] (the model of its objects) and [`thin_artifacts`] (its pinned
//! scripts, `contracts/artifacts/`). Everything below is the three earlier families, in [`legacy`].
//!
//! - [`artifacts`]: the pinned compiled contracts (`contracts/legacy/artifacts/`), templates, template hashes.
//! - [`state`]: state structs and codecs of `KpfPair`, `KpfOpen`, `KpfPending` and `KpfLock`, validation against the contracts' bounds.
//! - [`payoff`]: the pure payoff math (DESIGN 3.1, 4.5, 5): `q`, payouts, trigger prices, fees, floor values.
//! - [`network`]: testnet-10 addresses (mainnet is refused).
//! - [`tx`]: the transaction model, signing (Schnorr, SIGHASH_ALL), fee and mass, JSON forms.
//! - [`build`]: the builders of every evidence-free path.
//! - [`kob`]: KOB order and token codecs, the evidence check.
//! - [`evidence`]: the builders of the two-step evidence paths (DESIGN 4.5d): propose (P5, P6), challenge, cure, finalize; and of
//!   guaranteed fast settlement (DESIGN 4.5f): guarantee, dispute, release.
//! - [`pending`]: the pure math of the two-step paths: bond, cure and forfeit bounds, the pending state, what each branch pays.
//! - [`lock`]: guaranteed fast settlement (DESIGN 4.5f): the cover, the lock a guarantee needs, what a dispute and a release pay.
//! - [`verify`]: every input through the rusty-kaspa v2.1.0 script engine, with consensus metering.
//! - [`bond`]: the underwriting slot of oracle-only markets (`docs/DESIGN-ORACLE-IMPL.md`) as a pure model of
//!   `KpfBond` and of its stand-in pair and order: the ring, the bounds, the acceptance rule of every entry.
//! - [`judge`]: the stake judge (`docs/DESIGN-ORACLE-IMPL.md`, second slice) as a pure model of `KpfStake` and of its
//!   stand-in claim: whole transactions, the acceptance rule of every input, the transaction shapes, a UTXO world.
//! - [`oracle`]: oracle-only markets, the integration slice (`docs/DESIGN-ORACLE-IMPL.md`, third slice): the oracle market
//!   block, the states and math of `KpfOpenO`, `KpfPairO` and `KpfPendingO`, the feed-definition check.
//!
//! # The evidence paths
//!
//! A claim on price evidence is two transactions (DESIGN 4.5d). `KpfPair.propose` (P5 unilateral close, P6 liquidation)
//! moves the part into a `KpfPending` with a bond; `KpfPending.challenge` refutes it with counter-evidence, `cure` lets the
//! threatened side add the margin it needs, `finalize` settles it after the window (a close at its quote, a liquidation as
//! forfeit and keep). Proposals and challenges are built by [`evidence`]: the transaction embeds the fill of a live KOB
//! `KobAsk` / `KobBid`, its counterparty (the executor buys the ask's tokens, or sells its own tokens into the bid) and the
//! token program's inputs and outputs, so that every input passes its own script. KOB's builder
//! (`kob_protocol::build::build_batch`) cannot be used for this: it assembles the transaction in a crate-private draft that
//! holds only KOB's own order and key inputs, not a foreign pair input.
//! [`kob`] reads live orders and token holders from their redeem scripts, checks them against a market (DESIGN 3.3, 4.5b)
//! and says why an order cannot serve. Supported: KOB's reference KCC-20 program (3 / 3 slots) with P2PK owners, plain
//! GTC and TWAP/DCA-interval orders at a fixed quote (IOC/FOK only in a market whose evidence life L is below 600); see `docs/TESTNET.md` for what is not.
//!

pub mod legacy;
pub use legacy::artifacts;
pub use legacy::bond;
pub use legacy::build;
pub use legacy::cmarket;
pub use legacy::committee;
pub use legacy::committee_artifacts;
pub use legacy::committee_build;
pub use legacy::cross;
pub use legacy::evidence;
pub use legacy::fixtures;
pub use legacy::judge;
pub use legacy::kob;
pub use legacy::lock;
pub use legacy::oracle;
pub use legacy::oracle_artifacts;
pub use legacy::oracle_build;
pub use legacy::payoff;
pub use legacy::pending;
pub use legacy::sim;
pub use legacy::state;
pub mod error;
pub mod network;
pub mod thin;
pub mod thin_artifacts;
pub mod thin_build;
pub mod thin_read;
pub mod tx;
pub mod verify;

pub use error::{Error, Result};
