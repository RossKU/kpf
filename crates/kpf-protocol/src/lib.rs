//! KPF protocol core (testnet-10 research prototype).
//!
//! Everything that encodes, builds, signs or checks a transaction of the two KPF contracts, shared by the
//! command line (`kpf-cli`) and the engine tests (`kpf-tests`).
//!
//! - [`artifacts`]: the pinned compiled contracts (`contracts/artifacts/`), templates, template hashes.
//! - [`state`]: state structs and codecs of `KpfPair` and `KpfOpen`, validation against the contracts' bounds.
//! - [`payoff`]: the pure payoff math (DESIGN 3.1, 4.5, 5): `q`, payouts, trigger prices, fees, floor values.
//! - [`network`]: testnet-10 addresses (mainnet is refused).
//! - [`tx`]: the transaction model, signing (Schnorr, SIGHASH_ALL), fee and mass, JSON forms.
//! - [`build`]: the builders of every evidence-free path.
//! - [`verify`]: every input through the rusty-kaspa v2.1.0 script engine, with consensus metering.
//!
//! # What is not here
//!
//! The evidence paths of `KpfPair.onEvidence` (P5 unilateral close, P6 liquidation, P7 liquidation by transfer) are not
//! built. Their transaction embeds the fill of a live KOB `KobAsk`/`KobBid` and its token custody: the signature
//! script and outputs of those inputs are KOB's and the token program's (a KCC-20 leader witness, per-program slot
//! limits, deliveries). `kob-protocol` builds them only inside its own crate-private draft, which cannot take foreign
//! inputs, and a KPF evidence transaction needs such an order to exist on the network, which requires KOB's indexer.
//! The engine tests (`crates/kpf-tests/tests/kpf/evidence.rs`) exercise these paths against KOB's real contracts.

pub mod artifacts;
pub mod build;
pub mod error;
pub mod fixtures;
pub mod network;
pub mod payoff;
pub mod sim;
pub mod state;
pub mod tx;
pub mod verify;

pub use error::{Error, Result};
