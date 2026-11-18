//! Transaction model: inputs and outputs under construction, the unsigned [`Built`] transaction with the per-input
//! data needed to sign, signing, the fee/mass pass and the JSON forms.
//!
//! # Flow
//!
//! 1. A builder (`crate::build`) returns a [`Built`]: a version-1 transaction with empty signature scripts, the UTXO
//!    entry of every input, one [`Plan`] per input (how its signature script is assembled and which keys sign it),
//!    a label for every input and output, and the KPF objects the transaction creates.
//! 2. [`Built::sign`] signs with a [`KeySet`] (Schnorr, SIGHASH_ALL, the only type the contracts accept), runs every
//!    input in the script engine to measure its script units, commits the exact compute budget of each input, sets the
//!    fee from the transaction's mass at the requested rate by moving the change output, re-signs (the sighash commits
//!    to output values, not to budgets or the storage mass) and commits the storage mass.
//! 3. [`crate::verify::verify`] runs the result the way consensus does.
//!
//! Fee rule: the node's relay floor, `rate * max(compute mass, normalized transient mass)` (rusty-kaspa v2.1.0
//! `mining/src/mempool/check_transaction_standard.rs`, 100 sompi per gram at the minimum rate). Storage mass (KIP-9)
//! is committed in the transaction and needs no relay fee. [`FeeMode::Priority`] pays for it too.

use std::collections::BTreeMap;

use kaspa_consensus_core::config::params::{Params, MAINNET_PARAMS};
use kaspa_consensus_core::hashing::covenant_id::covenant_id;
use kaspa_consensus_core::hashing::sighash::{calc_schnorr_signature_hash, SigHashReusedValuesUnsync};
use kaspa_consensus_core::hashing::sighash_type::SIG_HASH_ALL;
use kaspa_consensus_core::mass::{transaction_estimated_serialized_size, ComputeBudget, MassCalculator, ScriptUnits};
use kaspa_consensus_core::subnets::SUBNETWORK_ID_NATIVE;
use kaspa_consensus_core::tx::{
    CovenantBinding, MutableTransaction, PopulatedTransaction, ScriptPublicKey, Transaction, TransactionId, TransactionInput,
    TransactionOutpoint, TransactionOutput, UtxoEntry,
};
use kaspa_consensus_core::Hash;
use kaspa_txscript::opcodes::codes::{OpCheckSig, OpData32};
use kaspa_txscript::script_builder::ScriptBuilder;
use kaspa_txscript::{pay_to_script_hash_script, EngineFlags};
use secp256k1::{Keypair, Secp256k1};
use serde_json::{json, Value};
use silverscript_abi::{encode_contract_entry_sig_script, ArtifactValue};

use crate::artifacts::{contract_name, open_base, open_template, pair_base, pair_template};
use crate::error::{from_hex, hex32, invalid, to_hex, Error, Result};
use crate::state::{Open, Pair};

/// SIGHASH_ALL, the only signature hash type the contracts accept (`SIGHASH_ALL = 0x01`).
pub const SIGHASH_ALL: u8 = 0x01;
/// Covenant bindings need transaction version 1 (`TX_VERSION_TOCCATA`).
pub const TX_VERSION: u16 = 1;
/// Minimum relay fee rate, sompi per gram (rusty-kaspa v2.1.0 `DEFAULT_MINIMUM_RELAY_TRANSACTION_FEE`).
pub const MIN_FEE_RATE: u64 = 100;
/// KIP-9 storage mass parameter `C`.
pub const STORAGE_MASS_PARAMETER: u64 = 1_000_000_000_000;
pub const BLOCK_COMPUTE_LIMIT: u64 = 500_000;
pub const BLOCK_STORAGE_LIMIT: u64 = 500_000;
pub const BLOCK_TRANSIENT_LIMIT: u64 = 1_000_000;
/// Smallest plain output whose storage mass alone fits a block (0.02 KAS).
pub const DUST_OUTPUT_MIN: u64 = 2_000_000;
/// Script units charged per signature operation.
pub const SIGOP_SCRIPT_UNITS: u64 = 100_000;

/// Canonical data push.
pub fn push_data(data: &[u8]) -> Vec<u8> {
    ScriptBuilder::with_flags(EngineFlags::default()).add_data(data).expect("push data").drain()
}

/// P2PK (Schnorr, x-only key) script public key.
pub fn p2pk_spk(key: &[u8; 32]) -> ScriptPublicKey {
    let mut s = vec![OpData32];
    s.extend_from_slice(key);
    s.push(OpCheckSig);
    ScriptPublicKey::new(0, s.into())
}

/// The x-only key of a P2PK script, if it is one.
pub fn p2pk_key(spk: &ScriptPublicKey) -> Option<[u8; 32]> {
    let s = spk.script();
    (spk.version() == 0 && s.len() == 34 && s[0] == OpData32 && s[33] == OpCheckSig).then(|| s[1..33].try_into().expect("32 bytes"))
}

// ---------------------------------------------------------------- UTXOs

/// A plain P2PK UTXO of `key` (a funding input).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyUtxo {
    pub outpoint: TransactionOutpoint,
    pub amount: u64,
    pub daa: u64,
    pub key: [u8; 32],
}

impl KeyUtxo {
    pub fn entry(&self) -> UtxoEntry {
        UtxoEntry::new(self.amount, p2pk_spk(&self.key), self.daa, false, None)
    }
}

/// A live `KpfPair` UTXO.
#[derive(Clone, Debug)]
pub struct LivePair {
    pub p: Pair,
    pub value: u64,
    pub id: Hash,
    pub op: TransactionOutpoint,
    pub daa: u64,
}

impl LivePair {
    pub fn entry(&self) -> UtxoEntry {
        UtxoEntry::new(self.value, self.p.spk(), self.daa, false, Some(self.id))
    }
}

/// A live `KpfOpen` UTXO.
#[derive(Clone, Debug)]
pub struct LiveOpen {
    pub o: Open,
    pub value: u64,
    pub id: Hash,
    pub op: TransactionOutpoint,
    pub daa: u64,
}

impl LiveOpen {
    pub fn entry(&self) -> UtxoEntry {
        UtxoEntry::new(self.value, self.o.spk(), self.daa, false, Some(self.id))
    }
}

/// A KPF object a transaction creates.
#[derive(Clone, Debug, PartialEq)]
pub enum Obj {
    Pair(Pair),
    Open(Open),
}

impl Obj {
    pub fn redeem(&self) -> Vec<u8> {
        match self {
            Obj::Pair(p) => p.redeem(),
            Obj::Open(o) => o.redeem(),
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Obj::Pair(_) => "pair",
            Obj::Open(_) => "order",
        }
    }
}

// ---------------------------------------------------------------- keys

/// Secret keys by x-only public key.
#[derive(Default)]
pub struct KeySet {
    keys: BTreeMap<[u8; 32], Keypair>,
}

impl KeySet {
    pub fn new() -> KeySet {
        KeySet::default()
    }
    /// Adds a secret key; returns its x-only public key.
    pub fn add_secret(&mut self, secret: &[u8; 32]) -> Result<[u8; 32]> {
        let kp = Keypair::from_seckey_slice(&Secp256k1::new(), secret).map_err(|e| Error::Signing(format!("secret key: {e}")))?;
        Ok(self.add_keypair(kp))
    }
    pub fn add_keypair(&mut self, kp: Keypair) -> [u8; 32] {
        let k = kp.x_only_public_key().0.serialize();
        self.keys.insert(k, kp);
        k
    }
    pub fn has(&self, key: &[u8; 32]) -> bool {
        self.keys.contains_key(key)
    }
    /// Deterministic Schnorr signature (no auxiliary randomness) of a digest, with the SIGHASH_ALL byte appended.
    pub fn sign(&self, key: &[u8; 32], digest: &[u8; 32]) -> Result<Vec<u8>> {
        let kp = self.keys.get(key).ok_or_else(|| Error::Signing(format!("no secret key for public key {}", to_hex(key))))?;
        let msg = secp256k1::Message::from_digest_slice(digest).expect("32-byte digest");
        let mut s = Secp256k1::new().sign_schnorr_no_aux_rand(&msg, kp).as_ref().to_vec();
        s.push(SIGHASH_ALL);
        Ok(s)
    }
}

/// The public key of a secret key.
pub fn pubkey_of(secret: &[u8; 32]) -> Result<[u8; 32]> {
    let kp = Keypair::from_seckey_slice(&Secp256k1::new(), secret).map_err(|e| Error::Signing(format!("secret key: {e}")))?;
    Ok(kp.x_only_public_key().0.serialize())
}

// ---------------------------------------------------------------- input plans

/// A contract argument: a value, or a SIGHASH_ALL signature by the key over the transaction being built.
#[derive(Clone, Debug)]
pub enum Arg {
    V(ArtifactValue),
    Sig([u8; 32]),
}

/// Integer argument.
pub fn iv(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}
/// Byte-string argument.
pub fn bv(b: &[u8]) -> Arg {
    Arg::V(ArtifactValue::Bytes(b.to_vec()))
}

/// How an input's signature script is assembled.
#[derive(Clone, Debug)]
pub enum Plan {
    /// A plain P2PK input signed by `key`.
    P2pk { key: [u8; 32] },
    /// A `KpfPair` input calling `entry(args)`.
    Pair { redeem: Vec<u8>, entry: &'static str, args: Vec<Arg> },
    /// A `KpfOpen` input calling `entry(args)`.
    Open { redeem: Vec<u8>, entry: &'static str, args: Vec<Arg> },
}

impl Plan {
    /// The keys that sign this input.
    pub fn signers(&self) -> Vec<[u8; 32]> {
        match self {
            Plan::P2pk { key } => vec![*key],
            Plan::Pair { args, .. } | Plan::Open { args, .. } => {
                args.iter().filter_map(|a| if let Arg::Sig(k) = a { Some(*k) } else { None }).collect()
            }
        }
    }

    pub fn entry_name(&self) -> String {
        match self {
            Plan::P2pk { .. } => "p2pk".into(),
            Plan::Pair { entry, .. } => format!("KpfPair.{entry}"),
            Plan::Open { entry, .. } => format!("KpfOpen.{entry}"),
        }
    }

    /// The signature script, given the signature of each signing key (65 bytes each).
    pub fn sigscript(&self, sig_of: &dyn Fn(&[u8; 32]) -> Result<Vec<u8>>) -> Result<Vec<u8>> {
        let enc = |base, name: &str, redeem: &[u8], entry: &str, args: &[Arg]| -> Result<Vec<u8>> {
            let vals = args
                .iter()
                .map(|a| match a {
                    Arg::V(v) => Ok(v.clone()),
                    Arg::Sig(k) => Ok(ArtifactValue::Bytes(sig_of(k)?)),
                })
                .collect::<Result<Vec<_>>>()?;
            let mut s = encode_contract_entry_sig_script(base, name, entry, &vals).map_err(|e| Error::Invalid(format!("{name}.{entry}: {e}")))?;
            s.extend_from_slice(&push_data(redeem));
            Ok(s)
        };
        match self {
            Plan::P2pk { key } => Ok(push_data(&sig_of(key)?)),
            Plan::Pair { redeem, entry, args } => enc(pair_base(), contract_name(pair_base()), redeem, entry, args),
            Plan::Open { redeem, entry, args } => enc(open_base(), contract_name(open_base()), redeem, entry, args),
        }
    }

    /// The signature script with all-zero 65-byte placeholder signatures (for sizes and provisional fees).
    pub fn placeholder_sigscript(&self) -> Result<Vec<u8>> {
        self.sigscript(&|_| Ok(vec![0u8; 65]))
    }
}

// ---------------------------------------------------------------- masses

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FeeMode {
    /// The relay floor: `rate * max(compute, normalized transient)`.
    #[default]
    Relay,
    /// Also pays for the storage mass: `rate * max(compute, normalized transient, storage)`.
    Priority,
}

/// Masses of a transaction (grams).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MassReport {
    pub size: u64,
    pub compute: u64,
    pub transient: u64,
    pub transient_normalized: u64,
    pub storage: u64,
    /// max(compute, normalized transient): what the relay floor is charged on.
    pub fee_mass: u64,
    /// max(fee_mass, storage).
    pub priority_mass: u64,
}

impl MassReport {
    pub fn within_block_limits(&self) -> bool {
        self.compute <= BLOCK_COMPUTE_LIMIT && self.storage <= BLOCK_STORAGE_LIMIT && self.transient <= BLOCK_TRANSIENT_LIMIT
    }
}

/// The consensus parameters masses are computed with: rusty-kaspa v2.1.0 mainnet. Testnet-10 has the same mass
/// parameters and block mass limits.
pub fn mass_params() -> &'static Params {
    &MAINNET_PARAMS
}

/// Every mass dimension of a transaction, with the rusty-kaspa calculator and the node's mempool cofactors.
pub fn masses(tx: &Transaction, entries: &[UtxoEntry]) -> MassReport {
    let params = mass_params();
    let calc = MassCalculator::new_with_consensus_params(params);
    let non = calc.calc_non_contextual_masses(tx);
    let storage = if tx.inputs.is_empty() || entries.len() != tx.inputs.len() || tx.outputs.iter().any(|o| o.value == 0) {
        u64::MAX
    } else {
        let populated = PopulatedTransaction::new(tx, entries.to_vec());
        calc.calc_contextual_masses(&populated).map(|c| c.storage_mass).unwrap_or(u64::MAX)
    };
    let transient_normalized = non.normalized_transient(&params.block_mass_cofactors());
    let fee_mass = non.compute_mass.max(transient_normalized);
    MassReport {
        size: transaction_estimated_serialized_size(tx),
        compute: non.compute_mass,
        transient: non.transient_mass,
        transient_normalized,
        storage,
        fee_mass,
        priority_mass: fee_mass.max(storage),
    }
}

/// The node's minimum fee for `mass` grams at `rate` sompi per gram.
pub fn relay_fee_for_mass(mass: u64, rate: u64) -> u64 {
    let per_kg = rate.saturating_mul(1000);
    let fee = mass.saturating_mul(per_kg) / 1000;
    (if fee == 0 { per_kg } else { fee }).min(kaspa_consensus_core::constants::MAX_SOMPI)
}

pub fn min_fee(r: &MassReport, rate: u64) -> u64 {
    relay_fee_for_mass(r.fee_mass, rate)
}

pub fn target_fee(r: &MassReport, rate: u64, mode: FeeMode) -> u64 {
    match mode {
        FeeMode::Relay => min_fee(r, rate),
        FeeMode::Priority => relay_fee_for_mass(r.priority_mass, rate).max(min_fee(r, rate)),
    }
}

/// Smallest compute budget whose allowance covers `units` script units.
pub fn budget_for_units(units: u64) -> u16 {
    ComputeBudget::checked_covering_script_units(ScriptUnits(units)).map(|b| b.value()).unwrap_or(u16::MAX)
}

// ---------------------------------------------------------------- the draft (builders' workspace)

pub(crate) struct DIn {
    pub outpoint: TransactionOutpoint,
    pub entry: UtxoEntry,
    pub plan: Plan,
    pub label: String,
    pub sequence: u64,
}

pub(crate) struct DOut {
    pub value: u64,
    pub spk: ScriptPublicKey,
    pub binding: Option<(u16, Hash)>,
    pub label: String,
    pub obj: Option<Obj>,
}

/// A transaction under construction. Builders add inputs and outputs, then call [`Draft::finish`].
pub struct Draft {
    pub(crate) ins: Vec<DIn>,
    pub(crate) outs: Vec<DOut>,
    pub(crate) lock: u64,
    pub(crate) change: Option<usize>,
    /// (output, authorizing input): a genesis binding computed at `finish`.
    pub(crate) genesis: Vec<(usize, usize)>,
    pub(crate) rate: u64,
    pub(crate) mode: FeeMode,
}

/// The object a transaction creates, with its output index.
#[derive(Clone, Debug)]
pub struct Created {
    pub index: usize,
    pub obj: Obj,
    pub covenant_id: Hash,
}

/// An unsigned transaction with everything needed to sign and verify it.
#[derive(Clone, Debug)]
pub struct Built {
    pub tx: Transaction,
    pub entries: Vec<UtxoEntry>,
    pub plans: Vec<Plan>,
    pub input_labels: Vec<String>,
    pub output_labels: Vec<String>,
    pub created: Vec<Created>,
    /// The output that absorbs the fee (the funder's change, or the payout of a cancel or cooperative close).
    pub change: Option<usize>,
    pub fee_rate: u64,
    pub fee_mode: FeeMode,
}

impl Draft {
    pub fn new(rate: u64, mode: FeeMode) -> Draft {
        Draft { ins: vec![], outs: vec![], lock: 0, change: None, genesis: vec![], rate, mode }
    }

    pub(crate) fn add_in(&mut self, outpoint: TransactionOutpoint, entry: UtxoEntry, plan: Plan, label: impl Into<String>) -> usize {
        self.ins.push(DIn { outpoint, entry, plan, label: label.into(), sequence: 0 });
        self.ins.len() - 1
    }

    pub(crate) fn add_pair_in(&mut self, l: &LivePair, entry: &'static str, args: Vec<Arg>, label: impl Into<String>) -> usize {
        self.add_in(l.op, l.entry(), Plan::Pair { redeem: l.p.redeem(), entry, args }, label)
    }

    pub(crate) fn add_open_in(&mut self, l: &LiveOpen, entry: &'static str, args: Vec<Arg>, label: impl Into<String>) -> usize {
        self.add_in(l.op, l.entry(), Plan::Open { redeem: l.o.redeem(), entry, args }, label)
    }

    pub(crate) fn add_out(&mut self, value: u64, spk: ScriptPublicKey, binding: Option<(u16, Hash)>, label: impl Into<String>) -> usize {
        self.outs.push(DOut { value, spk, binding, label: label.into(), obj: None });
        self.outs.len() - 1
    }

    pub(crate) fn add_plain(&mut self, value: u64, key: &[u8; 32], label: impl Into<String>) -> usize {
        self.add_out(value, p2pk_spk(key), None, label)
    }

    pub(crate) fn add_pair_out(&mut self, p: &Pair, value: u64, binding: Option<(u16, Hash)>, label: impl Into<String>) -> usize {
        let i = self.add_out(value, p.spk(), binding, label);
        self.outs[i].obj = Some(Obj::Pair(p.clone()));
        i
    }

    pub(crate) fn add_open_out(&mut self, o: &Open, value: u64, binding: Option<(u16, Hash)>, label: impl Into<String>) -> usize {
        let i = self.add_out(value, o.spk(), binding, label);
        self.outs[i].obj = Some(Obj::Open(o.clone()));
        i
    }

    /// Marks output `idx` as the one that absorbs the fee (no separate funder).
    pub(crate) fn fee_from(&mut self, idx: usize) {
        self.change = Some(idx);
    }

    /// Applies genesis bindings, balances the fee-absorbing output with a provisional fee and returns the unsigned
    /// transaction.
    pub fn finish(mut self) -> Result<Built> {
        // Genesis bindings: the id is a hash of the authorizing outpoint and the group's outputs (value and script).
        let mut seen = std::collections::BTreeSet::new();
        for i in &self.ins {
            if !seen.insert((i.outpoint.transaction_id, i.outpoint.index)) {
                return invalid(format!("UTXO {}:{} is spent twice", i.outpoint.transaction_id, i.outpoint.index));
            }
        }
        let mut tx_outs: Vec<TransactionOutput> = self
            .outs
            .iter()
            .map(|o| TransactionOutput { value: o.value, script_public_key: o.spk.clone(), covenant: o.binding.map(|(a, c)| CovenantBinding { authorizing_input: a, covenant_id: c }) })
            .collect();
        for &(out, auth) in &self.genesis {
            let id = covenant_id(self.ins[auth].outpoint, std::iter::once((out as u32, &tx_outs[out])));
            tx_outs[out].covenant = Some(CovenantBinding { authorizing_input: auth as u16, covenant_id: id });
            self.outs[out].binding = Some((auth as u16, id));
        }
        let inputs: Vec<TransactionInput> =
            self.ins.iter().map(|i| TransactionInput::new_with_compute_budget(i.outpoint, vec![], i.sequence, 0)).collect();
        let entries: Vec<UtxoEntry> = self.ins.iter().map(|i| i.entry.clone()).collect();
        let plans: Vec<Plan> = self.ins.iter().map(|i| i.plan.clone()).collect();
        let tx = Transaction::new(TX_VERSION, inputs, tx_outs, self.lock, SUBNETWORK_ID_NATIVE, 0, vec![]);
        let created = self
            .outs
            .iter()
            .enumerate()
            .filter_map(|(i, o)| o.obj.clone().map(|obj| Created { index: i, obj, covenant_id: o.binding.map(|b| b.1).unwrap_or_default() }))
            .collect();
        let mut b = Built {
            tx,
            entries,
            plans,
            input_labels: self.ins.iter().map(|i| i.label.clone()).collect(),
            output_labels: self.outs.iter().map(|o| o.label.clone()).collect(),
            created,
            change: self.change,
            fee_rate: self.rate,
            fee_mode: self.mode,
        };
        for (k, i) in self.ins.iter().enumerate() {
            b.tx.inputs[k].sequence = i.sequence;
        }
        b.balance_provisional()?;
        Ok(b)
    }
}

// ---------------------------------------------------------------- Built, signing

/// A signed transaction.
#[derive(Clone, Debug)]
pub struct Signed {
    pub tx: Transaction,
    pub entries: Vec<UtxoEntry>,
    pub fee: u64,
    pub mass: MassReport,
    /// Script units each input used.
    pub units: Vec<u64>,
    pub input_labels: Vec<String>,
    pub output_labels: Vec<String>,
    pub created: Vec<Created>,
    pub change: Option<usize>,
}

impl Signed {
    pub fn txid(&self) -> TransactionId {
        self.tx.id()
    }

    /// The UTXOs of the KPF objects this transaction creates, once it is accepted: outpoint, entry, object.
    pub fn created_utxos(&self) -> Vec<(TransactionOutpoint, UtxoEntry, Obj)> {
        self.created
            .iter()
            .map(|c| {
                let o = &self.tx.outputs[c.index];
                (
                    TransactionOutpoint { transaction_id: self.tx.id(), index: c.index as u32 },
                    UtxoEntry::new(o.value, o.script_public_key.clone(), 0, false, o.covenant.map(|b| b.covenant_id)),
                    c.obj.clone(),
                )
            })
            .collect()
    }
}

impl Built {
    fn input_sum(&self) -> u128 {
        self.entries.iter().map(|e| e.amount as u128).sum()
    }

    fn others_sum(&self) -> u128 {
        self.tx.outputs.iter().enumerate().filter(|(k, _)| Some(*k) != self.change).map(|(_, o)| o.value as u128).sum()
    }

    fn set_change(&mut self, fee: u64) -> Result<()> {
        let Some(c) = self.change else {
            return Ok(());
        };
        let need = self.others_sum() + fee as u128;
        let have = self.input_sum();
        if have < need {
            return Err(Error::Insufficient { need: need.min(u64::MAX as u128) as u64, have: have.min(u64::MAX as u128) as u64 });
        }
        let v = (have - need) as u64;
        if v < DUST_OUTPUT_MIN {
            return invalid(format!(
                "the funding leaves {v} sompi for the {} output; a plain output needs at least {DUST_OUTPUT_MIN} (0.02 KAS). Add funding.",
                self.output_labels[c]
            ));
        }
        self.tx.outputs[c].value = v;
        Ok(())
    }

    /// A provisional fee from placeholder signature scripts and a flat budget per contract input.
    fn balance_provisional(&mut self) -> Result<()> {
        // A change of zero would make the storage mass infinite: start from everything left over.
        self.set_change(0)?;
        let mut t = self.tx.clone();
        for (i, p) in self.plans.iter().enumerate() {
            t.inputs[i].signature_script = p.placeholder_sigscript()?;
            if !matches!(p, Plan::P2pk { .. }) {
                t.inputs[i].compute_commit = ComputeBudget(300).into();
            }
        }
        let r = masses(&t, &self.entries);
        let fee = target_fee(&r, self.fee_rate, self.fee_mode);
        self.set_change(fee)?;
        for (i, inp) in self.tx.inputs.iter_mut().enumerate() {
            inp.compute_commit = t.inputs[i].compute_commit;
        }
        Ok(())
    }

    /// The digest input `idx` signs (SIGHASH_ALL); the signature scripts do not take part.
    pub fn sighash(&self, tx: &Transaction, idx: usize) -> [u8; 32] {
        sighash(tx, &self.entries, idx)
    }

    fn fill_signatures(&self, tx: &mut Transaction, keys: &KeySet) -> Result<()> {
        for (i, plan) in self.plans.iter().enumerate() {
            let digest = sighash(tx, &self.entries, i);
            tx.inputs[i].signature_script = plan.sigscript(&|k| keys.sign(k, &digest))?;
        }
        Ok(())
    }

    /// Signs, measures, commits the compute budgets, sets the exact fee and the storage mass (see the module docs).
    pub fn sign(&self, keys: &KeySet) -> Result<Signed> {
        // Missing keys first, with the label of the input that needs them.
        for (i, p) in self.plans.iter().enumerate() {
            for k in p.signers() {
                if !keys.has(&k) {
                    return Err(Error::Signing(format!("input {i} ({}) needs a signature of {} and no such key was given", self.input_labels[i], to_hex(&k))));
                }
            }
        }
        let mut b = self.clone();
        // Pass 1: signatures over the provisional outputs, measure the script units of every input.
        let mut tx = b.tx.clone();
        b.fill_signatures(&mut tx, keys)?;
        let units = crate::verify::run_inputs(&tx, &b.entries, crate::verify::Meter::Unlimited);
        for (i, u) in units.iter().enumerate() {
            let budget = match u {
                Ok(u) => budget_for_units(*u),
                // A failing input cannot be measured; `verify` reports why. Commit a generous budget.
                Err(_) => 2_000,
            };
            tx.inputs[i].compute_commit = ComputeBudget(budget).into();
        }
        // The fee from the mass at the committed budgets; the change absorbs it. In priority mode the storage mass
        // depends on the change value, so iterate until the fee is stable.
        b.tx = tx;
        for _ in 0..6 {
            let r = masses(&b.tx, &b.entries);
            let fee = target_fee(&r, b.fee_rate, b.fee_mode);
            let cur = b.input_sum() - b.tx.outputs.iter().map(|o| o.value as u128).sum::<u128>();
            if cur == fee as u128 {
                break;
            }
            b.set_change(fee)?;
        }
        // Pass 2: the outputs changed, so sign again.
        let mut tx = b.tx.clone();
        b.fill_signatures(&mut tx, keys)?;
        let report = masses(&tx, &b.entries);
        tx.set_storage_mass(report.storage);
        tx.finalize();
        let mass = masses(&tx, &b.entries);
        let total_in: u64 = b.entries.iter().map(|e| e.amount).sum();
        let total_out: u64 = tx.outputs.iter().map(|o| o.value).sum();
        let units: Vec<u64> = crate::verify::run_inputs(&tx, &b.entries, crate::verify::Meter::Unlimited)
            .into_iter()
            .map(|r| r.unwrap_or(0))
            .collect();
        Ok(Signed {
            fee: total_in - total_out,
            tx,
            entries: b.entries.clone(),
            mass,
            units,
            input_labels: b.input_labels.clone(),
            output_labels: b.output_labels.clone(),
            created: b.created.clone(),
            change: b.change,
        })
    }
}

/// Schnorr SIGHASH_ALL digest of input `idx`.
pub fn sighash(tx: &Transaction, entries: &[UtxoEntry], idx: usize) -> [u8; 32] {
    let mt = MutableTransaction::with_entries(tx.clone(), entries.to_vec());
    let reused = SigHashReusedValuesUnsync::new();
    let h = calc_schnorr_signature_hash(&mt.as_verifiable(), idx, SIG_HASH_ALL, &reused);
    h.as_bytes()
}

// ---------------------------------------------------------------- JSON

/// `RpcTransaction` JSON (camelCase) as accepted by `submitTransaction` over wRPC.
pub fn rpc_json(tx: &Transaction) -> Value {
    let inputs: Vec<Value> = tx
        .inputs
        .iter()
        .map(|i| {
            json!({
                "previousOutpoint": { "transactionId": i.previous_outpoint.transaction_id.to_string(), "index": i.previous_outpoint.index },
                "signatureScript": to_hex(&i.signature_script),
                "sequence": i.sequence,
                "sigOpCount": i.compute_commit.sig_op_count().unwrap_or(0),
                "computeBudget": i.compute_commit.compute_budget().unwrap_or(0),
                "verboseData": Value::Null,
            })
        })
        .collect();
    let outputs: Vec<Value> = tx
        .outputs
        .iter()
        .map(|o| {
            json!({
                "value": o.value,
                "scriptPublicKey": spk_hex(&o.script_public_key),
                "verboseData": Value::Null,
                "covenant": o.covenant.map(|c| json!({ "authorizingInput": c.authorizing_input, "covenantId": c.covenant_id.to_string() })),
            })
        })
        .collect();
    json!({
        "version": tx.version,
        "inputs": inputs,
        "outputs": outputs,
        "lockTime": tx.lock_time,
        "subnetworkId": to_hex(tx.subnetwork_id.as_ref()),
        "gas": tx.gas,
        "payload": to_hex(&tx.payload),
        "storageMass": tx.storage_mass(),
        "mass": tx.storage_mass(),
        "verboseData": Value::Null,
    })
}

/// `version (u16 BE) || script` as hex.
pub fn spk_hex(spk: &ScriptPublicKey) -> String {
    format!("{:04x}{}", spk.version(), to_hex(spk.script()))
}

pub fn spk_from_hex(s: &str) -> Result<ScriptPublicKey> {
    let b = from_hex(s)?;
    if b.len() < 2 {
        return invalid("script public key shorter than its version");
    }
    Ok(ScriptPublicKey::new(u16::from_be_bytes([b[0], b[1]]), b[2..].to_vec().into()))
}

fn u64_of(v: &Value, what: &str) -> Result<u64> {
    match v {
        Value::Number(n) => n.as_u64().ok_or_else(|| Error::Invalid(format!("{what} out of range"))),
        Value::String(s) => s.parse().map_err(|_| Error::Invalid(format!("{what} is not an integer"))),
        _ => invalid(format!("{what} is not an integer")),
    }
}

/// The inverse of [`rpc_json`].
pub fn tx_from_rpc_json(v: &Value) -> Result<Transaction> {
    let get = |o: &Value, k: &str| o.get(k).cloned().ok_or_else(|| Error::Invalid(format!("transaction JSON: missing `{k}`")));
    let inputs = get(v, "inputs")?
        .as_array()
        .ok_or_else(|| Error::Invalid("inputs is not an array".into()))?
        .iter()
        .map(|i| {
            let op = get(i, "previousOutpoint")?;
            let txid = hex32(get(&op, "transactionId")?.as_str().unwrap_or(""))?;
            let budget = u64_of(&get(i, "computeBudget")?, "computeBudget")?;
            Ok(TransactionInput::new_with_compute_budget(
                TransactionOutpoint { transaction_id: TransactionId::from_bytes(txid), index: u64_of(&get(&op, "index")?, "index")? as u32 },
                from_hex(get(i, "signatureScript")?.as_str().unwrap_or(""))?,
                u64_of(&get(i, "sequence")?, "sequence")?,
                budget as u16,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let outputs = get(v, "outputs")?
        .as_array()
        .ok_or_else(|| Error::Invalid("outputs is not an array".into()))?
        .iter()
        .map(|o| {
            let cov = match o.get("covenant") {
                None | Some(Value::Null) => None,
                Some(c) => Some(CovenantBinding {
                    authorizing_input: u64_of(&get(c, "authorizingInput")?, "authorizingInput")? as u16,
                    covenant_id: Hash::from_bytes(hex32(get(c, "covenantId")?.as_str().unwrap_or(""))?),
                }),
            };
            Ok(TransactionOutput {
                value: u64_of(&get(o, "value")?, "value")?,
                script_public_key: spk_from_hex(get(o, "scriptPublicKey")?.as_str().unwrap_or(""))?,
                covenant: cov,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut tx = Transaction::new(
        u64_of(&get(v, "version")?, "version")? as u16,
        inputs,
        outputs,
        u64_of(&get(v, "lockTime")?, "lockTime")?,
        SUBNETWORK_ID_NATIVE,
        u64_of(&get(v, "gas")?, "gas")?,
        from_hex(get(v, "payload")?.as_str().unwrap_or(""))?,
    );
    tx.set_storage_mass(u64_of(&get(v, "storageMass")?, "storageMass")?);
    tx.finalize();
    Ok(tx)
}

/// JSON of a UTXO entry: `{amount, scriptPublicKey, blockDaaScore, covenantId}`.
pub fn entry_json(e: &UtxoEntry) -> Value {
    json!({ "amount": e.amount, "scriptPublicKey": spk_hex(&e.script_public_key), "blockDaaScore": e.block_daa_score, "covenantId": e.covenant_id.map(|c| c.to_string()) })
}

pub fn entry_from_json(v: &Value) -> Result<UtxoEntry> {
    let g = |k: &str| v.get(k).ok_or_else(|| Error::Invalid(format!("UTXO entry JSON: missing `{k}`")));
    let cov = match v.get("covenantId") {
        None | Some(Value::Null) => None,
        Some(c) => Some(Hash::from_bytes(hex32(c.as_str().unwrap_or(""))?)),
    };
    Ok(UtxoEntry::new(
        u64_of(g("amount")?, "amount")?,
        spk_from_hex(g("scriptPublicKey")?.as_str().unwrap_or(""))?,
        v.get("blockDaaScore").map(|d| u64_of(d, "blockDaaScore")).transpose()?.unwrap_or(0),
        false,
        cov,
    ))
}

/// The Borsh encoding of a transaction (kaspa-consensus-core's own), as hex.
pub fn tx_hex(tx: &Transaction) -> String {
    to_hex(&borsh::to_vec(tx).expect("borsh"))
}

/// The P2SH script public key of a redeem script.
pub fn p2sh_spk(redeem: &[u8]) -> ScriptPublicKey {
    pay_to_script_hash_script(redeem)
}

/// The redeem script of `KpfPair`/`KpfOpen` instances of a state: shortcut used by displays.
pub fn pair_redeem_len() -> usize {
    let t = pair_template();
    t.prefix.len() + t.state_len + t.suffix.len()
}

pub fn open_redeem_len() -> usize {
    let t = open_template();
    t.prefix.len() + t.state_len + t.suffix.len()
}
