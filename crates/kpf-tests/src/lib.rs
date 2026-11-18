//! Integration tests live in `tests/`; this crate has no library code.
//!
//! The tests compile the `.sil` sources under `contracts/` with KOB's vendored SilverScript compiler and execute the
//! resulting scripts in rusty-kaspa's `TxScriptEngine` with the covenant context of the transaction.
