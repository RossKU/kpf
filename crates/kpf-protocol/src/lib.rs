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
//! - [`kob`]: KOB order and token codecs, the evidence check.
//! - [`evidence`]: the builders of the three evidence paths (P5, P6, P7).
//! - [`verify`]: every input through the rusty-kaspa v2.1.0 script engine, with consensus metering.
//!
//! # The evidence paths
//!
//! The three paths of the `onEvidence` entry (P5 unilateral close, P6 liquidation, P7 liquidation by transfer) are built
//! by [`evidence`]: the transaction embeds the fill of a live KOB `KobAsk` / `KobBid`, its counterparty (the executor
//! buys the ask's tokens, or sells its own tokens into the bid) and the token program's inputs and outputs, so that every
//! input passes its own script. KOB's builder (`kob_protocol::build::build_batch`) cannot be used for this: it assembles
//! the transaction in a crate-private draft that holds only KOB's own order and key inputs, not a foreign pair input.
//! [`kob`] reads live orders and token holders from their redeem scripts, checks them against a market (DESIGN 3.3, 4.5b)
//! and says why an order cannot serve. Supported: KOB's reference KCC-20 program (3 / 3 slots) with P2PK owners, plain
//! GTC, IOC/FOK and TWAP/DCA-interval orders at a fixed quote; see `docs/TESTNET.md` for what is not.
//!

pub mod artifacts;
pub mod build;
pub mod error;
pub mod evidence;
pub mod fixtures;
pub mod kob;
pub mod network;
pub mod payoff;
pub mod sim;
pub mod state;
pub mod tx;
pub mod verify;

pub use error::{Error, Result};
