//! Validation through the rusty-kaspa v2.1.0 script engine, the way consensus does it.
//!
//! [`verify`] builds the covenant context from the transaction (an error there rejects the whole transaction), runs
//! every input with the sig-op price of consensus and the input's committed compute budget as the script-unit limit,
//! and checks the transaction-level rules the engine does not see: the storage-mass commitment, outputs not above
//! inputs, the relay fee floor, the block mass limits and the static sig-op bound of each input.

use kaspa_consensus_core::hashing::sighash::SigHashReusedValuesUnsync;
use kaspa_consensus_core::tx::{PopulatedTransaction, Transaction, UtxoEntry};
use kaspa_txscript::caches::Cache;
use kaspa_txscript::covenants::CovenantsContext;
use kaspa_txscript::{EngineCtx, EngineFlags, TxScriptEngine};
use kaspa_txscript_errors::{CovenantsError, TxScriptError};

use crate::tx::{masses, min_fee, MassReport, MIN_FEE_RATE, SIGOP_SCRIPT_UNITS};

/// How script units are metered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meter {
    /// Consensus: each input's committed compute budget is its limit, a signature check costs 100,000 units.
    Consensus,
    /// Consensus sig-op price, no limit (what a measurement needs).
    Unlimited,
    /// No limit and free signature checks (the engine tests' harness).
    Free,
}

/// Per-input engine outcome: the script units used, or the error.
pub type InputOutcome = Result<u64, String>;

/// Runs every input. If the covenant bindings are invalid, every input carries that error.
pub fn run_inputs(tx: &Transaction, entries: &[UtxoEntry], meter: Meter) -> Vec<InputOutcome> {
    if entries.len() != tx.inputs.len() {
        return (0..tx.inputs.len()).map(|_| Err("entry count differs from input count".to_string())).collect();
    }
    match run_raw(tx, entries, meter) {
        Ok(v) => v.into_iter().map(|r| r.map_err(|e| format!("{e:?}"))).collect(),
        Err(e) => (0..tx.inputs.len()).map(|_| Err(format!("covenant context: {e:?}"))).collect(),
    }
}

/// Runs every input with typed errors. `Err` is a refusal of the covenant bindings (one entry per input is required).
pub fn run_raw(tx: &Transaction, entries: &[UtxoEntry], meter: Meter) -> Result<Vec<Result<u64, TxScriptError>>, CovenantsError> {
    assert_eq!(entries.len(), tx.inputs.len(), "one UTXO entry per input");
    let populated = PopulatedTransaction::new(tx, entries.to_vec());
    let cov_ctx = CovenantsContext::from_tx(&populated)?;
    let reused = SigHashReusedValuesUnsync::new();
    let cache = Cache::new(10_000);
    let flags = || EngineFlags { sigop_script_units: if meter == Meter::Free { 0u64 } else { SIGOP_SCRIPT_UNITS }.into() };
    Ok((0..tx.inputs.len())
        .map(|i| {
            let input = tx.inputs[i].clone();
            let limit = if meter == Meter::Consensus { input.compute_commit.allowed_script_units() } else { u64::MAX.into() };
            let mut vm = TxScriptEngine::from_transaction_input_with_script_units_limit(
                &populated,
                &input,
                i,
                &entries[i],
                EngineCtx::new(&cache).with_reused(&reused).with_covenants_ctx(&cov_ctx),
                flags(),
                limit,
            );
            vm.execute().map(|_| vm.used_script_units().0)
        })
        .collect())
}

/// The outcome of [`verify`].
#[derive(Clone, Debug)]
pub struct Verification {
    /// One result per input: script units used, or the engine error.
    pub inputs: Vec<InputOutcome>,
    /// Transaction-level failures (empty when none).
    pub tx_errors: Vec<String>,
    pub mass: MassReport,
    pub fee: u64,
    pub min_fee: u64,
    /// Committed compute budget of each input (units of 10,000 script units).
    pub budgets: Vec<u16>,
}

impl Verification {
    /// Every input passed and no transaction-level rule failed.
    pub fn is_ok(&self) -> bool {
        self.tx_errors.is_empty() && self.inputs.iter().all(|r| r.is_ok())
    }
    pub fn input_ok(&self, i: usize) -> bool {
        self.inputs.get(i).is_some_and(|r| r.is_ok())
    }
    /// Every failure as one line each.
    pub fn errors(&self) -> Vec<String> {
        let mut v: Vec<String> = self.inputs.iter().enumerate().filter_map(|(i, r)| r.as_ref().err().map(|e| format!("input {i}: {e}"))).collect();
        v.extend(self.tx_errors.iter().cloned());
        v
    }
}

/// The data-signature opcodes (`OpCheckSigFromStack` 0xd7, its ECDSA form 0xd8) of the last push of a signature
/// script, the redeem script of a P2SH input; 0 when the signature script has no such push.
pub fn data_sig_ops(signature_script: &[u8]) -> u32 {
    fn pushes(s: &[u8]) -> Option<Vec<&[u8]>> {
        let (mut i, mut v) = (0usize, vec![]);
        while i < s.len() {
            let op = s[i];
            let (len, head) = match op {
                0x01..=0x4b => (op as usize, 1),
                0x4c => (*s.get(i + 1)? as usize, 2),
                0x4d => (u16::from_le_bytes([*s.get(i + 1)?, *s.get(i + 2)?]) as usize, 3),
                0x4e => (u32::from_le_bytes([*s.get(i + 1)?, *s.get(i + 2)?, *s.get(i + 3)?, *s.get(i + 4)?]) as usize, 5),
                _ => (0, 1),
            };
            let data = s.get(i + head..i + head + len)?;
            if (0x01..=0x4e).contains(&op) {
                v.push(data);
            }
            i += head + len;
        }
        Some(v)
    }
    let Some(redeem) = pushes(signature_script).and_then(|v| v.last().copied()) else { return 0 };
    // Walk the redeem script's opcodes, skipping push data.
    let (mut i, mut n) = (0usize, 0u32);
    while i < redeem.len() {
        let op = redeem[i];
        let skip = match op {
            0x01..=0x4b => 1 + op as usize,
            0x4c => 2 + redeem.get(i + 1).copied().unwrap_or(0) as usize,
            0x4d => 3 + redeem.get(i + 1..i + 3).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).unwrap_or(0),
            0x4e => 5 + redeem.get(i + 1..i + 5).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).unwrap_or(0),
            _ => 1,
        };
        if op == 0xd7 || op == 0xd8 {
            n += 1;
        }
        i += skip;
    }
    n
}

/// Full validation of a signed transaction with its UTXO entries.
pub fn verify(tx: &Transaction, entries: &[UtxoEntry]) -> Verification {
    let inputs = run_inputs(tx, entries, Meter::Consensus);
    let mass = masses(tx, entries);
    let mut tx_errors = vec![];
    if entries.len() == tx.inputs.len() {
        if tx.storage_mass() != mass.storage {
            tx_errors.push(format!("storage mass commitment {} != computed {}", tx.storage_mass(), mass.storage));
        }
        for (i, input) in tx.inputs.iter().enumerate() {
            let so = kaspa_txscript::get_sig_op_count_upper_bound::<PopulatedTransaction, SigHashReusedValuesUnsync>(
                &input.signature_script,
                &entries[i].script_public_key,
            ) as u32;
            // Live run 3: a testnet-10 node also counts every data signature (`OpCheckSigFromStack`) of the input's
            // redeem script, whatever entry runs, which the v2.1.0 function above does not.
            let so = so + data_sig_ops(&input.signature_script);
            if so > 15 {
                tx_errors.push(format!("input {i}: {so} signature operations in its script, more than the 15 a node admits"));
            }
        }
    }
    let total_in: u128 = entries.iter().map(|e| e.amount as u128).sum();
    let total_out: u128 = tx.outputs.iter().map(|o| o.value as u128).sum();
    let fee = total_in.saturating_sub(total_out) as u64;
    if total_out > total_in {
        tx_errors.push(format!("outputs {total_out} exceed inputs {total_in}"));
    }
    let floor = min_fee(&mass, MIN_FEE_RATE);
    if fee < floor {
        tx_errors.push(format!("fee {fee} below the relay floor {floor}"));
    }
    if !mass.within_block_limits() {
        tx_errors.push(format!("mass exceeds block limits: {mass:?}"));
    }
    let budgets = tx.inputs.iter().map(|i| i.compute_commit.compute_budget().unwrap_or(0)).collect();
    Verification { inputs, tx_errors, mass, fee, min_fee: floor, budgets }
}
