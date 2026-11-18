//! Minimal engine harness for KPF tests.
//!
//! Why this is a re-implementation and not a path dependency on KOB's helpers: KOB's helpers live in
//! `vendor/kob/crates/kob-tests/tests/common/mod.rs`, a test-only module (the `kob-tests` crate has no library target,
//! `vendor/kob/crates/kob-tests/src/lib.rs`), so no other crate can depend on them. Including that file with
//! `#[path]` is not possible either: it declares `pub mod kron;`, calls `kob_protocol` (`kcc20_33_program`) and
//! resolves every source against KOB's own `contracts/` tree (`repo_root`). What IS reused by path dependency is
//! everything below those helpers: KOB's vendored SilverScript compiler and ABI encoder
//! (`vendor/kob/vendor/silverscript/*`) and the same rusty-kaspa v2.1.0 crates. The functions here follow
//! `vendor/kob/crates/kob-tests/tests/common/mod.rs:102-216` (compile, encode an entry, push the redeem script,
//! execute an input with the covenant context) and the signing helper of
//! `vendor/kob/crates/kob-tests/tests/kob/kob_kron_v2_buy_tests.rs:541-550`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use kaspa_consensus_core::hashing::covenant_id::covenant_id;
use kaspa_consensus_core::hashing::sighash::{calc_schnorr_signature_hash, SigHashReusedValuesUnsync};
use kaspa_consensus_core::hashing::sighash_type::SIG_HASH_ALL;
use kaspa_consensus_core::tx::{
    CovenantBinding, MutableTransaction, PopulatedTransaction, ScriptPublicKey, Transaction, TransactionId, TransactionInput,
    TransactionOutpoint, TransactionOutput, UtxoEntry,
};
use kaspa_consensus_core::Hash;
use kaspa_txscript::caches::Cache;
use kaspa_txscript::covenants::CovenantsContext;
use kaspa_txscript::opcodes::codes::{OpCheckSig, OpData32};
use kaspa_txscript::script_builder::ScriptBuilder;
use kaspa_txscript::{pay_to_script_hash_script, EngineCtx, EngineFlags, TxScriptEngine};
use kaspa_txscript_errors::{CovenantsError, TxScriptError};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use silverscript_abi::{encode_contract_entry_sig_script, ArtifactValue, SilAbiArtifact};
use silverscript_lang::compiler::{compile_to_sil_abi_artifact_with_options, CompileOptions};

/// KPF repository root (this crate lives in `crates/kpf-tests`).
pub fn kpf_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("KPF root")
}

/// Source of `contracts/<rel>` (for example `probes/X1Pair.sil`).
pub fn contract_source(rel: &str) -> String {
    let path = kpf_root().join("contracts").join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).replace("\r\n", "\n")
}

/// Compiles `contracts/<rel>` with the vendored SilverScript compiler (default options, as KOB's tests).
pub fn compile(rel: &str, ctor: &[ArtifactValue]) -> SilAbiArtifact {
    compile_to_sil_abi_artifact_with_options(&contract_source(rel), ctor, CompileOptions::default())
        .unwrap_or_else(|e| panic!("compile {rel}: {e:?}"))
}

fn single_contract_name(art: &SilAbiArtifact) -> &str {
    assert_eq!(art.contracts.len(), 1, "expected exactly one contract");
    art.contracts.keys().next().expect("one contract")
}

pub fn bytecode(art: &SilAbiArtifact) -> Vec<u8> {
    art.contracts.values().next().expect("one contract").compiled.bytecode.clone()
}

/// The P2SH script public key of a compiled contract instance.
pub fn p2sh(art: &SilAbiArtifact) -> ScriptPublicKey {
    pay_to_script_hash_script(&bytecode(art))
}

/// Signature script calling `entry` with `args`, followed by the redeem-script push (P2SH spend).
pub fn entry_sig_script(art: &SilAbiArtifact, entry: &str, args: &[ArtifactValue]) -> Vec<u8> {
    let mut ss = encode_contract_entry_sig_script(art, single_contract_name(art), entry, args).expect("encode entry");
    ss.extend_from_slice(&push(&bytecode(art)));
    ss
}

pub fn push(data: &[u8]) -> Vec<u8> {
    ScriptBuilder::with_flags(EngineFlags::default()).add_data(data).expect("push").drain()
}

pub fn int(v: i64) -> ArtifactValue {
    ArtifactValue::Int(v)
}

pub fn bytes32(h: Hash) -> ArtifactValue {
    ArtifactValue::Bytes(h.as_bytes().to_vec())
}

// ---------------------------------------------------------------- keys and plain outputs

pub fn keypair(seed: u8) -> Keypair {
    let sk = SecretKey::from_slice(&[seed; 32]).expect("secret key");
    Keypair::from_secret_key(&Secp256k1::new(), &sk)
}

/// Standard P2PK script (`OpData32 <x-only key> OpCheckSig`).
pub fn p2pk_spk(kp: &Keypair) -> ScriptPublicKey {
    let mut s = vec![OpData32];
    s.extend_from_slice(&kp.x_only_public_key().0.serialize());
    s.push(OpCheckSig);
    ScriptPublicKey::new(0, s.into())
}

pub fn out(value: u64, spk: ScriptPublicKey, covenant: Option<CovenantBinding>) -> TransactionOutput {
    TransactionOutput { value, script_public_key: spk, covenant }
}

pub fn bind(authorizing_input: u16, covenant_id: Hash) -> Option<CovenantBinding> {
    Some(CovenantBinding { authorizing_input, covenant_id })
}

/// A distinct outpoint for a UTXO that exists before the scenario.
pub fn outpoint(n: u8) -> TransactionOutpoint {
    TransactionOutpoint { transaction_id: TransactionId::from_bytes([n; 32]), index: n as u32 }
}

/// The genesis covenant id consensus expects for the group (authorizing input outpoint, output indices):
/// `vendor/kob` pins rusty-kaspa v2.1.0, where `CovenantsContext::from_tx` recomputes it with
/// `consensus/core/src/hashing/covenant_id.rs:16-30` (outpoint, then index, value and script of each group output;
/// the binding itself is excluded).
pub fn genesis_id(auth_outpoint: TransactionOutpoint, outputs: &[TransactionOutput], group: &[usize]) -> Hash {
    covenant_id(auth_outpoint, group.iter().map(|&i| (i as u32, &outputs[i])))
}

// ---------------------------------------------------------------- transactions

#[derive(Clone)]
pub enum Spend {
    /// A P2SH contract input calling `entry(args)`.
    Contract { art: SilAbiArtifact, entry: &'static str, args: Vec<ArtifactValue> },
    /// An ordinary P2PK input signed with SIGHASH_ALL.
    P2pk(Keypair),
    /// A P2SH contract input whose arguments may include SIGHASH_ALL Schnorr signatures over this transaction.
    Signed { art: SilAbiArtifact, entry: &'static str, args: Vec<Arg> },
}

/// A contract argument: a value, or a SIGHASH_ALL signature of `kp` over the transaction being built (signature
/// scripts are not part of the sighash, so it is computed before they are filled).
#[derive(Clone)]
pub enum Arg {
    V(ArtifactValue),
    Sig(Keypair),
    /// A KCC-20 leader witness of the normal path for a P2PK owner: `0x00 || signature`
    /// (`vendor/kob/crates/kob-protocol/src/script.rs`, `leader_witness`).
    LeaderSig(Keypair),
}

#[derive(Clone)]
pub struct In {
    pub outpoint: TransactionOutpoint,
    pub utxo: UtxoEntry,
    pub spend: Spend,
}

/// A contract UTXO (P2SH of `art`) with an optional covenant id.
pub fn contract_in(outpoint: TransactionOutpoint, value: u64, cov: Option<Hash>, art: &SilAbiArtifact, entry: &'static str, args: Vec<ArtifactValue>) -> In {
    In { outpoint, utxo: UtxoEntry::new(value, p2sh(art), 0, false, cov), spend: Spend::Contract { art: art.clone(), entry, args } }
}

/// A P2PK UTXO of `kp` with an optional covenant id (a P2PK UTXO can carry one: `CovenantBinding` is per output and
/// independent of the script, `consensus/core/src/tx.rs:190-203`).
pub fn p2pk_in(outpoint: TransactionOutpoint, value: u64, cov: Option<Hash>, kp: &Keypair) -> In {
    In { outpoint, utxo: UtxoEntry::new(value, p2pk_spk(kp), 0, false, cov), spend: Spend::P2pk(*kp) }
}

/// Builds a version-1 transaction (covenant bindings are refused in version 0,
/// `consensus/src/processes/transaction_validator/tx_validation_in_isolation.rs:208-209`), fills every signature
/// script and returns it with its UTXO entries.
pub fn build(ins: &[In], outputs: Vec<TransactionOutput>) -> (Transaction, Vec<UtxoEntry>) {
    build_ext(ins, outputs, 0, &[])
}

/// As `build`, with the transaction lock time (what `tx.daa >= x` proves by CLTV) and per-input sequence numbers
/// (what `this.ageDaa >= x` proves by CSV; a missing entry is 0).
pub fn build_ext(ins: &[In], outputs: Vec<TransactionOutput>, lock_time: u64, sequences: &[u64]) -> (Transaction, Vec<UtxoEntry>) {
    let entries: Vec<UtxoEntry> = ins.iter().map(|i| i.utxo.clone()).collect();
    let inputs: Vec<TransactionInput> = ins
        .iter()
        .enumerate()
        .map(|(k, i)| TransactionInput::new(i.outpoint, vec![], sequences.get(k).copied().unwrap_or(0), 0))
        .collect();
    let unsigned = Transaction::new(1, inputs, outputs, lock_time, Default::default(), 0, vec![]);
    let mut tx = unsigned.clone();
    for (k, i) in ins.iter().enumerate() {
        tx.inputs[k].signature_script = match &i.spend {
            Spend::Contract { art, entry, args } => entry_sig_script(art, entry, args),
            Spend::P2pk(kp) => push(&sign(&unsigned, &entries, k, kp)),
            Spend::Signed { art, entry, args } => {
                let vals: Vec<ArtifactValue> = args
                    .iter()
                    .map(|a| match a {
                        Arg::V(v) => v.clone(),
                        Arg::Sig(kp) => ArtifactValue::Bytes(sign(&unsigned, &entries, k, kp)),
                        Arg::LeaderSig(kp) => ArtifactValue::Bytes([vec![0x00], sign(&unsigned, &entries, k, kp)].concat()),
                    })
                    .collect();
                entry_sig_script(art, entry, &vals)
            }
        };
    }
    tx.finalize();
    (tx, entries)
}

fn sign(tx: &Transaction, entries: &[UtxoEntry], idx: usize, kp: &Keypair) -> Vec<u8> {
    let mt = MutableTransaction::with_entries(tx.clone(), entries.to_vec());
    let reused = SigHashReusedValuesUnsync::new();
    let h = calc_schnorr_signature_hash(&mt.as_verifiable(), idx, SIG_HASH_ALL, &reused);
    let msg = secp256k1::Message::from_digest_slice(h.as_bytes().as_slice()).expect("msg");
    let mut s = kp.sign_schnorr(msg).as_ref().to_vec();
    s.push(SIG_HASH_ALL.to_u8());
    s
}

/// The UTXO that output `idx` of an accepted `tx` becomes: its covenant id is the output's binding id
/// (`consensus/core/src/utxo/utxo_diff.rs:239`), fixed for the life of the UTXO.
pub fn utxo_of(tx: &Transaction, idx: usize) -> (TransactionOutpoint, UtxoEntry) {
    let o = &tx.outputs[idx];
    (
        TransactionOutpoint { transaction_id: tx.id(), index: idx as u32 },
        UtxoEntry::new(o.value, o.script_public_key.clone(), 0, false, o.covenant.map(|c| c.covenant_id)),
    )
}

/// Validates a transaction the way `tx_validation_in_utxo_context.rs:58-62` does for scripts: first the covenant
/// bindings (`CovenantsContext::from_tx`, `crypto/txscript/src/covenants.rs:101-163`; an error here rejects the whole
/// transaction), then every input's script with that context. Returns one result per input.
pub fn run(tx: &Transaction, entries: &[UtxoEntry]) -> Result<Vec<Result<(), TxScriptError>>, CovenantsError> {
    let populated = PopulatedTransaction::new(tx, entries.to_vec());
    let cov_ctx = CovenantsContext::from_tx(&populated)?;
    let reused = SigHashReusedValuesUnsync::new();
    let cache = Cache::new(10_000);
    Ok((0..tx.inputs.len())
        .map(|i| {
            let input = tx.inputs[i].clone();
            let mut vm = TxScriptEngine::from_transaction_input(
                &populated,
                &input,
                i,
                &entries[i],
                EngineCtx::new(&cache).with_reused(&reused).with_covenants_ctx(&cov_ctx),
                EngineFlags { sigop_script_units: 0.into() },
            );
            vm.execute()
        })
        .collect())
}

/// One input measured with consensus engine flags (`EngineFlags::default()`, which also charges signature checks
/// in script units): the result, the script units used, and the largest combined main + alt stack the engine held
/// before any opcode (read from its opcode log).
pub struct Metered {
    pub result: Result<(), TxScriptError>,
    pub script_units: u64,
    pub max_stack: usize,
}

pub fn run_metered(tx: &Transaction, entries: &[UtxoEntry]) -> Vec<Metered> {
    let populated = PopulatedTransaction::new(tx, entries.to_vec());
    let cov_ctx = CovenantsContext::from_tx(&populated).expect("covenant bindings");
    let reused = SigHashReusedValuesUnsync::new();
    let cache = Cache::new(10_000);
    (0..tx.inputs.len())
        .map(|i| {
            let input = tx.inputs[i].clone();
            let mut log: Vec<u8> = Vec::new();
            let (result, script_units) = {
                let mut vm = TxScriptEngine::from_transaction_input(
                    &populated,
                    &input,
                    i,
                    &entries[i],
                    EngineCtx::new(&cache).with_reused(&reused).with_covenants_ctx(&cov_ctx),
                    EngineFlags::default(),
                )
                .with_opcode_execution_log_buffer(&mut log);
                let r = vm.execute();
                (r, vm.used_script_units().0)
            };
            let text = String::from_utf8_lossy(&log);
            let max_stack = text.lines().map(|l| l.matches("\"0x").count()).max().unwrap_or(0);
            Metered { result, script_units, max_stack }
        })
        .collect()
}

/// Every input passes.
pub fn assert_accepted(tx: &Transaction, entries: &[UtxoEntry]) {
    let res = run(tx, entries).unwrap_or_else(|e| panic!("covenant context refused the transaction: {e:?}"));
    for (i, r) in res.iter().enumerate() {
        assert!(r.is_ok(), "input {i} failed: {r:?}");
    }
}

/// The covenant bindings are valid and input `idx` fails as a script `require` failure.
pub fn assert_input_refused(tx: &Transaction, entries: &[UtxoEntry], idx: usize) -> TxScriptError {
    let res = run(tx, entries).unwrap_or_else(|e| panic!("covenant context refused the transaction: {e:?}"));
    let err = res[idx].clone().expect_err("input expected to fail");
    assert!(matches!(err, TxScriptError::VerifyError | TxScriptError::EvalFalse), "expected a failed require, got {err:?}");
    err
}

/// The covenant bindings are valid and input `idx` fails for any script reason (a lock-time refusal, an overflow).
pub fn assert_input_fails(tx: &Transaction, entries: &[UtxoEntry], idx: usize) -> TxScriptError {
    let res = run(tx, entries).unwrap_or_else(|e| panic!("covenant context refused the transaction: {e:?}"));
    res[idx].clone().expect_err("input expected to fail")
}
