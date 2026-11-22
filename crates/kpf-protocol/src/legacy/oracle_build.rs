//! Transaction builders of oracle-only markets (docs/DESIGN-ORACLE-IMPL.md, third slice) on the pinned artifacts
//! (`crate::oracle_artifacts`): every shape of the order, the pair, the pending part, the underwriter's slot and the
//! judge's stakes, including the slot and stake shapes slices A and B1 left without builders. Each builder returns an
//! [`OBuilt`] (unsigned, with a provisional fee) that [`OBuilt::sign`] signs (SIGHASH_ALL), meters (the smallest
//! covering compute budget per input), balances (the fee from the mass, out of the funder's change) and commits; the
//! result passes [`crate::verify::verify`]. Values are the least the contracts accept (`crate::oracle`), so a builder
//! never pays a party less than its covenant requires and never more than the inputs hold.
//!
//! The book market's builder (`crate::tx::Draft`) is not reused: its input plans and created objects are closed enums
//! of the book-market templates. The fee, mass and signing rules are the same functions (`crate::tx`).

use kaspa_consensus_core::hashing::covenant_id::covenant_id;
use kaspa_consensus_core::mass::ComputeBudget;
use kaspa_consensus_core::subnets::SUBNETWORK_ID_NATIVE;
use kaspa_consensus_core::tx::{CovenantBinding, ScriptPublicKey, Transaction, TransactionInput, TransactionOutpoint, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kaspa_txscript::pay_to_script_hash_script;
use silverscript_abi::{encode_contract_entry_sig_script, ArtifactValue, SilAbiArtifact};

use crate::artifacts::contract_name;
use crate::bond::{BindArgs, Bond, Claim, Ctx};
use crate::error::{invalid, Error, Result};
use crate::judge::{Js, NO, STATUS_RESOLVED, UNIT, YES};
use crate::oracle::*;
use crate::oracle_artifacts as pinned;
use crate::payoff::q;
use crate::state::{sm8, KIND_CLOSE, LONG, PLAIN_DUST, SHORT};
use crate::tx::{budget_for_units, masses, p2pk_spk, push_data, sighash, target_fee, Arg, FeeMode, KeySet, KeyUtxo, MassReport, DUST_OUTPUT_MIN, TX_VERSION};

/// Which template an input runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OWhich {
    Open,
    Pair,
    Pending,
    Bond,
    Stake,
    /// A template of the committee family (`crate::committee_build`).
    C(crate::committee_build::CWhich),
    /// A script of the thin family, by its name (`crate::thin_artifacts`).
    T(&'static str),
}

impl OWhich {
    fn base(self) -> &'static SilAbiArtifact {
        match self {
            OWhich::Open => pinned::open_base(),
            OWhich::Pair => pinned::pair_base(),
            OWhich::Pending => pinned::pending_base(),
            OWhich::Bond => pinned::bond_base(),
            OWhich::Stake => pinned::stake_base(),
            OWhich::C(w) => w.base(),
            OWhich::T(n) => crate::thin_artifacts::base(n).expect("a script of the thin family"),
        }
    }
}

/// How an input's signature script is assembled.
#[derive(Clone, Debug)]
pub enum OPlan {
    Key { key: [u8; 32] },
    Contract { which: OWhich, redeem: Vec<u8>, entry: &'static str, args: Vec<Arg> },
}

impl OPlan {
    pub fn signers(&self) -> Vec<[u8; 32]> {
        match self {
            OPlan::Key { key } => vec![*key],
            OPlan::Contract { args, .. } => args.iter().filter_map(|a| if let Arg::Sig(k) = a { Some(*k) } else { None }).collect(),
        }
    }
    pub fn name(&self) -> String {
        match self {
            OPlan::Key { .. } => "p2pk".into(),
            OPlan::Contract { which, entry, .. } => format!("{}.{entry}", contract_name(which.base())),
        }
    }
    fn sigscript(&self, sig_of: &dyn Fn(&[u8; 32]) -> Result<Vec<u8>>) -> Result<Vec<u8>> {
        match self {
            OPlan::Key { key } => Ok(push_data(&sig_of(key)?)),
            OPlan::Contract { which, redeem, entry, args } => {
                let vals = args
                    .iter()
                    .map(|a| match a {
                        Arg::V(v) => Ok(v.clone()),
                        Arg::Sig(k) => Ok(ArtifactValue::Bytes(sig_of(k)?)),
                        Arg::LeaderSig(k) => Ok(ArtifactValue::Bytes([vec![0x00], sig_of(k)?].concat())),
                    })
                    .collect::<Result<Vec<_>>>()?;
                let base = which.base();
                let mut s = encode_contract_entry_sig_script(base, contract_name(base), entry, &vals).map_err(|e| Error::Invalid(format!("{}.{entry}: {e}", contract_name(base))))?;
                s.extend_from_slice(&push_data(redeem));
                Ok(s)
            }
        }
    }
}

/// An object of the oracle family a transaction creates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OObj {
    Order(OOpen),
    Pair(OPair),
    Pending(OPending),
    Slot(Bond),
    Stake(Js),
    /// An object of the committee family (`crate::committee_build`).
    C(crate::committee_build::CObj),
}

impl OObj {
    pub fn redeem(&self) -> Vec<u8> {
        let k = pinned::kit();
        match self {
            OObj::Order(o) => o.redeem(k),
            OObj::Pair(p) => p.redeem(k),
            OObj::Pending(w) => w.redeem(k),
            OObj::Slot(b) => k.bond.redeem(&b.state_bytes()),
            OObj::Stake(s) => k.stake.redeem(&s.state_bytes()),
            OObj::C(c) => c.redeem(),
        }
    }
    pub fn spk(&self) -> ScriptPublicKey {
        pay_to_script_hash_script(&self.redeem())
    }
    pub fn kind(&self) -> &'static str {
        match self {
            OObj::Order(_) => "oracle order",
            OObj::Pair(_) => "oracle pair",
            OObj::Pending(_) => "oracle pending part",
            OObj::Slot(_) => "underwriting slot",
            OObj::Stake(_) => "judge stake",
            OObj::C(c) => c.kind(),
        }
    }
    /// Decodes a redeem script of one of the five pinned templates.
    pub fn decode(redeem: &[u8]) -> Result<OObj> {
        let k = pinned::kit();
        if let Ok(st) = k.pair.state_of(redeem) {
            return Ok(OObj::Pair(OPair::decode_state(st)?));
        }
        if let Ok(st) = k.pending.state_of(redeem) {
            return Ok(OObj::Pending(OPending::decode_state(st)?));
        }
        if let Ok(st) = k.open.state_of(redeem) {
            return Ok(OObj::Order(OOpen::decode_state(st)?));
        }
        if let Ok(st) = k.bond.state_of(redeem) {
            return Ok(OObj::Slot(decode_bond(st)?));
        }
        if let Ok(st) = k.stake.state_of(redeem) {
            return Ok(OObj::Stake(decode_stake(st)?));
        }
        if let Ok(c) = crate::committee_build::CObj::decode(redeem) {
            return Ok(OObj::C(c));
        }
        invalid("not an instance of any pinned oracle-market or committee template")
    }
}

fn rd8(b: &[u8], at: usize) -> Result<i64> {
    let s = b.get(at..at + 9).ok_or_else(|| Error::Decode("state too short".into()))?;
    if s[0] != 0x08 {
        return Err(Error::Decode(format!("byte {at}: not an 8-byte push")));
    }
    let mut m = [0u8; 8];
    m.copy_from_slice(&s[1..9]);
    let neg = m[7] & 0x80 != 0;
    m[7] &= 0x7f;
    let v = u64::from_le_bytes(m) as i64;
    Ok(if neg { -v } else { v })
}
fn rd32(b: &[u8], at: usize) -> Result<[u8; 32]> {
    let s = b.get(at..at + 33).ok_or_else(|| Error::Decode("state too short".into()))?;
    if s[0] != 0x20 {
        return Err(Error::Decode(format!("byte {at}: not a 32-byte push")));
    }
    Ok(s[1..33].try_into().expect("32"))
}

/// A `KpfBond` state from its 236 bytes.
pub fn decode_bond(st: &[u8]) -> Result<Bond> {
    use crate::bond::{BondMarket, BondParams, RING};
    let p = BondParams { rho_bps: rd8(st, 75)?, step: rd8(st, 84)?, term: rd8(st, 93)?, tail: rd8(st, 102)?, rate: rd8(st, 111)?, unbond: rd8(st, 120)? };
    let mut ring = [0i64; RING];
    if st.get(138..140) != Some(&[0x4c, 0x60][..]) {
        return Err(Error::Decode("the ring is not one 96-byte push".into()));
    }
    for (i, b) in ring.iter_mut().enumerate() {
        let mut m = [0u8; 8];
        m.copy_from_slice(&st[140 + 8 * i..148 + 8 * i]);
        let neg = m[7] & 0x80 != 0;
        m[7] &= 0x7f;
        let v = u64::from_le_bytes(m) as i64;
        *b = if neg { -v } else { v };
    }
    Ok(Bond { owner: rd32(st, 0)?, status: rd8(st, 33)?, m: BondMarket { id: rd32(st, 42)?, p }, j0: rd8(st, 129)?, ring })
}

/// A `KpfStake` state from its 270 bytes.
pub fn decode_stake(st: &[u8]) -> Result<Js> {
    use crate::judge::{JudgeMarket, JudgeParams};
    let p = JudgeParams { mu_bps: rd8(st, 75)?, age: rd8(st, 84)?, unbond: rd8(st, 93)?, round: rd8(st, 102)?, burn_bps: rd8(st, 111)?, min_stake: rd8(st, 120)? };
    let f = |i: usize| rd8(st, 162 + 9 * i);
    Ok(Js {
        owner: rd32(st, 0)?,
        status: rd8(st, 33)?,
        m: JudgeMarket { id: rd32(st, 42)?, p },
        spk_hash: rd32(st, 129)?,
        side: f(0)?,
        amt: f(1)?,
        round: f(2)?,
        yes: f(3)?,
        no: f(4)?,
        yes_u: f(5)?,
        no_u: f(6)?,
        anchor: f(7)?,
        start: f(8)?,
        w_left: f(9)?,
        l_left: f(10)?,
        base: f(11)?,
    })
}

/// A live UTXO of the oracle family.
#[derive(Clone, Debug)]
pub struct OLive<T> {
    pub s: T,
    pub value: u64,
    pub id: Option<Hash>,
    pub op: TransactionOutpoint,
    pub daa: u64,
}

impl<T> OLive<T> {
    pub fn idb(&self) -> [u8; 32] {
        self.id.map(|h| h.as_bytes()).unwrap_or([0; 32])
    }
}

fn entry_of(value: u64, spk: ScriptPublicKey, daa: u64, id: Option<Hash>) -> UtxoEntry {
    UtxoEntry::new(value, spk, daa, false, id)
}

// ---------------------------------------------------------------- the draft

struct DIn {
    op: TransactionOutpoint,
    entry: UtxoEntry,
    plan: OPlan,
    label: String,
    seq: u64,
}

struct DOut {
    value: u64,
    spk: ScriptPublicKey,
    binding: Option<(u16, Hash)>,
    /// A genesis output: its covenant id is computed at finish from this authorizing input.
    genesis: Option<usize>,
    /// A genesis output that shares its id with the other shared outputs of the same authorizing input.
    shared: bool,
    label: String,
    obj: Option<OObj>,
}

/// A transaction under construction.
pub struct ODraft {
    ins: Vec<DIn>,
    outs: Vec<DOut>,
    lock: u64,
    pub(crate) change: Option<usize>,
    rate: u64,
    mode: FeeMode,
    /// The transaction's payload (what an opening of the thin family publishes).
    pub(crate) payload: Vec<u8>,
}

/// An unsigned transaction with everything needed to sign it.
#[derive(Clone, Debug)]
pub struct OBuilt {
    pub tx: Transaction,
    pub entries: Vec<UtxoEntry>,
    pub plans: Vec<OPlan>,
    pub input_labels: Vec<String>,
    pub output_labels: Vec<String>,
    /// (output index, object, covenant id) of every object the transaction creates.
    pub created: Vec<(usize, OObj, Hash)>,
    pub change: Option<usize>,
    pub rate: u64,
    pub mode: FeeMode,
}

/// A signed, metered, balanced transaction.
#[derive(Clone, Debug)]
pub struct OSigned {
    pub tx: Transaction,
    pub entries: Vec<UtxoEntry>,
    pub fee: u64,
    pub mass: MassReport,
    pub units: Vec<u64>,
    pub input_labels: Vec<String>,
    pub output_labels: Vec<String>,
    pub created: Vec<(usize, OObj, Hash)>,
}

impl OSigned {
    /// The live objects once the transaction is accepted at `daa`.
    pub fn created_live(&self, daa: u64) -> Vec<(OObj, OLive<()>)> {
        self.created
            .iter()
            .map(|(i, o, id)| {
                let v = self.tx.outputs[*i].value;
                (o.clone(), OLive { s: (), value: v, id: if *id == Hash::default() { None } else { Some(*id) }, op: TransactionOutpoint { transaction_id: self.tx.id(), index: *i as u32 }, daa })
            })
            .collect()
    }
}

impl ODraft {
    pub fn new(rate: u64, mode: FeeMode) -> ODraft {
        ODraft { ins: vec![], outs: vec![], lock: 0, change: None, rate, mode, payload: vec![] }
    }
    pub fn lock(&mut self, t: u64) {
        self.lock = self.lock.max(t);
    }
    pub(crate) fn input(&mut self, op: TransactionOutpoint, entry: UtxoEntry, plan: OPlan, label: impl Into<String>, seq: u64) -> usize {
        self.ins.push(DIn { op, entry, plan, label: label.into(), seq });
        self.ins.len() - 1
    }
    pub(crate) fn contract<T>(&mut self, l: &OLive<T>, which: OWhich, redeem: Vec<u8>, entry: &'static str, args: Vec<Arg>, label: &str, seq: u64) -> usize {
        let spk = pay_to_script_hash_script(&redeem);
        self.input(l.op, entry_of(l.value, spk, l.daa, l.id), OPlan::Contract { which, redeem, entry, args }, label, seq)
    }
    pub(crate) fn key(&mut self, u: &KeyUtxo, label: &str) -> usize {
        self.input(u.outpoint, u.entry(), OPlan::Key { key: u.key }, label, 0)
    }
    pub(crate) fn out(&mut self, value: u64, spk: ScriptPublicKey, binding: Option<(u16, Hash)>, label: &str, obj: Option<OObj>) -> usize {
        self.outs.push(DOut { value, spk, binding, genesis: None, shared: false, label: label.into(), obj });
        self.outs.len() - 1
    }
    pub(crate) fn obj(&mut self, o: OObj, value: u64, binding: Option<(u16, Hash)>, label: &str) -> usize {
        let spk = o.spk();
        self.out(value, spk, binding, label, Some(o))
    }
    pub(crate) fn genesis(&mut self, o: OObj, value: u64, auth: usize, label: &str) -> usize {
        let i = self.obj(o, value, None, label);
        self.outs[i].genesis = Some(auth);
        i
    }
    /// An output of a script under a covenant id that begins here, authorized by input `auth`: alone under its id, or
    /// (`shared`) under one id with every other shared output of that input (a library of the thin family).
    pub(crate) fn genesis_script(&mut self, spk: ScriptPublicKey, value: u64, auth: usize, shared: bool, label: &str) -> usize {
        let i = self.out(value, spk, None, label, None);
        self.outs[i].genesis = Some(auth);
        self.outs[i].shared = shared;
        i
    }
    pub(crate) fn plain(&mut self, value: i64, key: &[u8; 32], label: &str) -> usize {
        self.out(value.max(DUST_OUTPUT_MIN as i64) as u64, p2pk_spk(key), None, label, None)
    }
    /// A payout at the reverse slot `M - 1 - i` of input 0 (DESIGN 4.4): the last output, which must be at least the
    /// input count; 1 KAS fillers to `filler_to` pad the outputs until it is.
    pub(crate) fn last(&mut self, value: i64, key: &[u8; 32], label: &str, filler_to: &[u8; 32]) -> usize {
        while self.outs.len() < self.ins.len() {
            self.plain(100_000_000, filler_to, "filler");
        }
        self.payout(value, key, label)
    }
    /// As `last`, to any script: an output at the reverse slot of input 0 (the burn of a slash).
    pub(crate) fn last_script(&mut self, value: u64, spk: ScriptPublicKey, label: &str, filler_to: &[u8; 32]) -> usize {
        while self.outs.len() < self.ins.len() {
            self.plain(100_000_000, filler_to, "filler");
        }
        self.out(value, spk, None, label, None)
    }
    /// A payout a contract fixes to an output of its own (`payTo`: at least `value` to `key` at a given index), which no
    /// builder can fold into another output. Below `VIABLE_OUTPUT_MIN` (a value the contracts allow down to 0.02 KAS,
    /// whose storage mass alone is the whole block limit) the funder tops it up to that size, which the contracts accept
    /// (they bound a payout from below only): at most 0.005 KAS more for the payee, instead of a transaction that can
    /// never be mined.
    pub(crate) fn payout(&mut self, value: i64, key: &[u8; 32], label: &str) -> usize {
        self.out(value.max(VIABLE_OUTPUT_MIN as i64) as u64, p2pk_spk(key), None, label, None)
    }
    /// Adds the funder's inputs and the change output (which absorbs the fee).
    pub(crate) fn fund(&mut self, funder: &[KeyUtxo], change_to: &[u8; 32]) -> usize {
        for u in funder {
            self.key(u, "funding");
        }
        let c = self.out(0, p2pk_spk(change_to), None, "change", None);
        self.change = Some(c);
        c
    }

    /// A payment to the underwriter that no contract fixes to an output of its own (the premium of an open or a
    /// renewal: the slot owner's signed bind decides where it goes). An output of its own costs `10^12 / value` grams of
    /// storage mass (KIP-9), so a small one can never be mined (0.02 KAS alone is the whole block limit). It is folded:
    /// into the funder's change when that goes to the underwriter's key (the change absorbs whatever no other output
    /// takes), or into one output with the underwriter's own plain input `extra`; only otherwise is it written alone,
    /// and the storage check of `finish` refuses it if it cannot be mined.
    fn pay_underwriter(&mut self, owner: &[u8; 32], v: i64, change_to: &[u8; 32], extra: Option<&KeyUtxo>, label: &str) -> Result<()> {
        if v <= 0 || change_to == owner {
            return Ok(());
        }
        if let Some(u) = extra {
            if u.key != *owner {
                return invalid("the extra underwriter input must be a plain UTXO of the slot owner's key");
            }
            self.key(u, "the underwriter's own input");
            self.plain(u.amount as i64 + v, owner, &format!("underwriter: its own input and the {label}"));
            return Ok(());
        }
        self.plain(v, owner, label);
        Ok(())
    }

    pub fn finish(self) -> Result<OBuilt> {
        let mut outs: Vec<TransactionOutput> =
            self.outs.iter().map(|o| TransactionOutput { value: o.value, script_public_key: o.spk.clone(), covenant: o.binding.map(|(a, c)| CovenantBinding { authorizing_input: a, covenant_id: c }) }).collect();
        let mut created = vec![];
        for (i, o) in self.outs.iter().enumerate() {
            if let Some(a) = o.genesis {
                let id = if o.shared {
                    let group: Vec<(u32, TransactionOutput)> = self.outs.iter().enumerate().filter(|(_, x)| x.shared && x.genesis == Some(a)).map(|(k, _)| (k as u32, TransactionOutput { covenant: None, ..outs[k].clone() })).collect();
                    covenant_id(self.ins[a].op, group.iter().map(|(k, t)| (*k, t)).collect::<Vec<_>>().into_iter())
                } else {
                    covenant_id(self.ins[a].op, std::iter::once((i as u32, &outs[i])))
                };
                outs[i].covenant = Some(CovenantBinding { authorizing_input: a as u16, covenant_id: id });
            }
        }
        for (i, o) in self.outs.iter().enumerate() {
            if let Some(obj) = &o.obj {
                created.push((i, obj.clone(), outs[i].covenant.map(|c| c.covenant_id).unwrap_or_default()));
            }
        }
        let inputs = self.ins.iter().map(|i| TransactionInput::new_with_compute_budget(i.op, vec![], i.seq, 0)).collect();
        let tx = Transaction::new(TX_VERSION, inputs, outs, self.lock, SUBNETWORK_ID_NATIVE, 0, self.payload.clone());
        let mut b = OBuilt {
            tx,
            entries: self.ins.iter().map(|i| i.entry.clone()).collect(),
            plans: self.ins.iter().map(|i| i.plan.clone()).collect(),
            input_labels: self.ins.iter().map(|i| i.label.clone()).collect(),
            output_labels: self.outs.iter().map(|o| o.label.clone()).collect(),
            created,
            change: self.change,
            rate: self.rate,
            mode: self.mode,
        };
        b.set_change(0)?;
        if let Some(m) = storage_refusal(&b.tx, &b.entries, &b.output_labels) {
            return invalid(m);
        }
        Ok(b)
    }
}

/// A storage refusal of the premium's own output, with what to do instead.
fn premium_advice(e: Error, prem: i64) -> Error {
    match e {
        Error::Invalid(m) if m.contains("(premium,") => Error::Invalid(format!(
            "{m}. The premium ({} KAS) need not have an output of its own: fund the transaction from the underwriter's key (its change then carries the premium) or give one plain UTXO of the underwriter (`--uw-utxo`), which one output returns with the premium",
            crate::error::kas_str(prem as i128)
        )),
        e => e,
    }
}


/// The storage mass (KIP-9) of a draft before signing, and when it is above the block limit the reason in words: the
/// output whose share is largest and the least value at which it would fit, the rest of the transaction unchanged.
/// `None` when the transaction fits. KIP-9 charges `C * p^2 / value` per output (`C` = 10^12, `p` its 100-byte storage
/// units) less a term of the inputs only, so an output's share is the mass it adds over the same transaction with that
/// output made very large.
pub fn storage_refusal(tx: &Transaction, entries: &[UtxoEntry], labels: &[String]) -> Option<String> {
    use crate::tx::BLOCK_STORAGE_LIMIT;
    let total = masses(tx, entries).storage;
    if total <= BLOCK_STORAGE_LIMIT {
        return None;
    }
    let mut worst: Option<(usize, u64, u64)> = None;
    for i in 0..tx.outputs.len() {
        let mut t2 = tx.clone();
        t2.outputs[i].value = 1_000_000_000_000_000_000;
        let rest = masses(&t2, entries).storage;
        let share = total.saturating_sub(rest);
        if worst.map_or(true, |(_, s, _)| share > s) {
            worst = Some((i, share, rest));
        }
    }
    let (i, share, rest) = worst?;
    let v = tx.outputs[i].value;
    let label = labels.get(i).map(String::as_str).unwrap_or("output");
    let kas = |x: u128| crate::error::kas_str(x as i128);
    if rest >= BLOCK_STORAGE_LIMIT {
        return Some(format!("the transaction's storage mass is {total} grams, above the block limit {BLOCK_STORAGE_LIMIT}, and stays above it whatever output {i} ({label}, {} KAS) holds: its outputs are too small for the inputs (KIP-9)", kas(v as u128)));
    }
    let need = (share as u128 * v as u128).div_ceil((BLOCK_STORAGE_LIMIT - rest) as u128);
    Some(format!(
        "output {i} ({label}, {} KAS) is too small to be mined: at least {} KAS (its storage mass is {share} grams and the rest of the transaction's {rest}, above the block limit {BLOCK_STORAGE_LIMIT}; KIP-9 charges 10^12 / value per output)",
        kas(v as u128),
        kas(need)
    ))
}

impl OBuilt {
    /// Review 11 (R11-F6): any SIGHASH_ALL spend of a plain UTXO of a pair's underwriter key vouches every proposal of
    /// that pair in its transaction. So this tool never signs, with an underwriter key, a transaction holding a proposal
    /// it was not asked to vouch: the only transactions with a proposal and a plain input of the pair's underwriter key
    /// it signs are the liquidations [`propose`] builds (the vouch, and the funding when the underwriter funds); beside a
    /// close (which needs no vouch) such an input is refused. The tool builds no transaction for another's signature.
    pub fn vouch_guard(&self) -> Result<()> {
        let mut uw: Vec<([u8; 32], bool)> = vec![];
        for p in &self.plans {
            if let OPlan::Contract { which: OWhich::Pair, redeem, entry: "propose", args } = p {
                let Ok(OObj::Pair(pair)) = OObj::decode(redeem) else { continue };
                let liq = matches!(args.get(1), Some(Arg::V(ArtifactValue::Int(k))) if *k == crate::state::KIND_LIQ);
                uw.push((pair.b.uw_key, liq));
            }
        }
        for (i, p) in self.plans.iter().enumerate() {
            let OPlan::Key { key } = p else { continue };
            for (k, liq) in &uw {
                if k == key && !*liq {
                    return Err(Error::Signing(format!(
                        "input {i} ({}) is a plain UTXO of the underwriter key {} of a pair this transaction proposes a claim on: signing it would vouch that claim (R11-F6). Refused: an underwriter signs only the vouch input of a liquidation it was asked to vouch; keep the vouching funds on a key of their own",
                        self.input_labels[i],
                        crate::error::to_hex(key)
                    )));
                }
            }
        }
        Ok(())
    }

    fn set_change(&mut self, fee: u64) -> Result<()> {
        let Some(c) = self.change else { return Ok(()) };
        let have: u128 = self.entries.iter().map(|e| e.amount as u128).sum();
        let others: u128 = self.tx.outputs.iter().enumerate().filter(|(k, _)| Some(*k) != self.change).map(|(_, o)| o.value as u128).sum();
        let need = others + fee as u128;
        if have < need {
            return Err(Error::Insufficient { need: need.min(u64::MAX as u128) as u64, have: have.min(u64::MAX as u128) as u64 });
        }
        let v = (have - need) as u64;
        if v < DUST_OUTPUT_MIN {
            return invalid(format!("the funding leaves {v} sompi of change; a plain output needs at least {DUST_OUTPUT_MIN}. Add funding."));
        }
        self.tx.outputs[c].value = v;
        Ok(())
    }

    fn fill(&self, tx: &mut Transaction, keys: &KeySet) -> Result<()> {
        for (i, p) in self.plans.iter().enumerate() {
            let d = sighash(tx, &self.entries, i);
            tx.inputs[i].signature_script = p.sigscript(&|k| keys.sign(k, &d))?;
        }
        Ok(())
    }

    /// Signs (SIGHASH_ALL), meters every input, commits the smallest covering budgets, sets the fee from the mass and
    /// the storage mass, signs again.
    pub fn sign(&self, keys: &KeySet) -> Result<OSigned> {
        self.vouch_guard()?;
        for (i, p) in self.plans.iter().enumerate() {
            for k in p.signers() {
                if !keys.has(&k) {
                    return Err(Error::Signing(format!("input {i} ({}) needs a signature of {} and no such key was given", self.input_labels[i], crate::error::to_hex(&k))));
                }
            }
        }
        let mut b = self.clone();
        let mut tx = b.tx.clone();
        b.fill(&mut tx, keys)?;
        let units = crate::verify::run_inputs(&tx, &b.entries, crate::verify::Meter::Unlimited);
        for (i, u) in units.iter().enumerate() {
            let budget = match u {
                Ok(u) => budget_for_units(*u),
                Err(_) => 2_000,
            };
            tx.inputs[i].compute_commit = ComputeBudget(budget).into();
        }
        b.tx = tx;
        for _ in 0..6 {
            let r = masses(&b.tx, &b.entries);
            let fee = target_fee(&r, b.rate, b.mode);
            let have: u128 = b.entries.iter().map(|e| e.amount as u128).sum();
            let out: u128 = b.tx.outputs.iter().map(|o| o.value as u128).sum();
            if have.checked_sub(out) == Some(fee as u128) {
                break;
            }
            b.set_change(fee)?;
        }
        let mut tx = b.tx.clone();
        b.fill(&mut tx, keys)?;
        let rep = masses(&tx, &b.entries);
        tx.set_storage_mass(rep.storage);
        tx.finalize();
        let mass = masses(&tx, &b.entries);
        if let Some(m) = storage_refusal(&tx, &b.entries, &b.output_labels) {
            return invalid(m);
        }
        let total_in: u64 = b.entries.iter().map(|e| e.amount).sum();
        let total_out: u64 = tx.outputs.iter().map(|o| o.value).sum();
        let units = crate::verify::run_inputs(&tx, &b.entries, crate::verify::Meter::Unlimited).into_iter().map(|r| r.unwrap_or(0)).collect();
        Ok(OSigned { fee: total_in - total_out, tx, entries: b.entries, mass, units, input_labels: b.input_labels, output_labels: b.output_labels, created: b.created })
    }
}

// ---------------------------------------------------------------- arguments

pub(crate) fn iv(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}
pub(crate) fn bv(b: &[u8]) -> Arg {
    Arg::V(ArtifactValue::Bytes(b.to_vec()))
}
pub(crate) fn b8(v: i64) -> Arg {
    Arg::V(ArtifactValue::Bytes(sm8(v).to_vec()))
}

fn bind_args(a: &BindArgs, owner: &[u8; 32]) -> Vec<Arg> {
    let mut v: Vec<Arg> = a.arg_bytes().into_iter().map(|b| Arg::V(ArtifactValue::Bytes(b.to_vec()))).collect();
    v.push(Arg::Sig(*owner));
    v
}

fn slot_ctx(sl: &OLive<Bond>, lock: u64, cov_out: usize) -> Ctx {
    Ctx { lock: lock as i64, seq: sl.s.m.p.rate, v_in: sl.value as i64, cov_in: 1, cov_out, signed: true, has_id: sl.id.is_some() }
}

pub(crate) fn refusal(e: Refusal) -> Error {
    Error::Invalid(e.to_string())
}

/// The ring advance a bind at lock time `lock` needs so that expiry `x` falls inside the ring: the fewest steps that
/// bring `x` into the twelve buckets (always allowed then: the dropped buckets' liability has ended).
pub fn advance_for(b: &Bond, x: i64) -> i64 {
    (x / b.m.p.step - b.j0 - crate::bond::RING as i64).max(0)
}

// ---------------------------------------------------------------- creation (genesis)

/// A new underwriting slot of `owner` worth `value` in market `m`, its ring starting at the step of DAA `daa`: a
/// genesis authored by the first funding input.
pub fn create_slot(m: &OMarket, owner: [u8; 32], value: u64, daa: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    m.check().map_err(refusal)?;
    if funder.is_empty() {
        return invalid("a slot's genesis needs a funding input to author it");
    }
    let j0 = (daa as i64 / m.bond.step - 1).max(0);
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.fund(funder, change_to);
    d.genesis(OObj::Slot(Bond::new(owner, m.bond_market(), j0)), value, 0, "slot");
    d.finish()
}

/// A free judge stake of `owner` worth `value` (at least the market's minimum), no covenant id.
pub fn create_stake(m: &OMarket, owner: [u8; 32], value: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    if (value as i64) < m.judge.min_stake {
        return invalid(format!("a stake needs at least {} sompi", m.judge.min_stake));
    }
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.fund(funder, change_to);
    d.obj(OObj::Stake(Js::free(owner, m.judge_market())), value, None, "stake");
    d.finish()
}

/// A new order: a genesis authored by the first funding input, escrowing `value`.
pub fn create_order(o: &OOpen, value: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    o.m.check().map_err(refusal)?;
    if o.my_rate <= 0 || o.min_counter_rate < 0 || o.tip < 0 || o.prem_max < 0 {
        return invalid("order rates must be positive, counter rate, tip and premium not negative");
    }
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.fund(funder, change_to);
    d.genesis(OObj::Order(o.clone()), value, 0, "order");
    d.finish()
}

// ---------------------------------------------------------------- T1' open

/// The terms of an open.
#[derive(Clone, Copy, Debug)]
pub struct OpenTerms {
    pub n: i64,
    pub x: i64,
    pub cover_rate: i64,
    pub premium: i64,
    pub expiry: i64,
    /// The bind's lock time (a lower bound on the present the slot proves).
    pub lock: u64,
}

/// T1': in 0 the long order, 1 the short order, 2 the slot (bind), then the funding; out 0 / 1 the orders' rest, 2 the
/// pair (a genesis authored by the first funding input), 3 the slot, 4 the premium to the underwriter, then change.
pub fn open(long: &OLive<OOpen>, short: &OLive<OOpen>, slot: &OLive<Bond>, t: &OpenTerms, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, OPair)> {
    open_with(long, short, slot, t, funder, change_to, rate, None)
}

/// `open` with an optional plain UTXO of the underwriter that joins the premium in one output (see `renew_with`).
#[allow(clippy::too_many_arguments)]
pub fn open_with(long: &OLive<OOpen>, short: &OLive<OOpen>, slot: &OLive<Bond>, t: &OpenTerms, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64, uw_extra: Option<&KeyUtxo>) -> Result<(OBuilt, OPair)> {
    let k = pinned::kit();
    let (lo, so) = (&long.s, &short.s);
    if lo.side != LONG || so.side == LONG {
        return invalid("open needs a long order and a short order");
    }
    if lo.m != so.m || lo.m.bond_market() != slot.s.m {
        return invalid("the orders and the slot are not of one market block (one byte apart is another market)");
    }
    let m = lo.m;
    let p = OPair {
        m,
        size: t.n,
        bl: t.x - lo.my_rate,
        bs: t.x + so.my_rate,
        entry: t.x,
        long_key: lo.owner,
        short_key: so.owner,
        b: OBinding { uw_id: slot.idb(), uw_key: slot.s.owner, cover_rate: t.cover_rate, expiry: t.expiry, premium: t.premium },
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers { long: maker_of(&long.op), short: maker_of(&short.op), ..Makers::default() },
    };
    check_pair(&p).map_err(refusal)?;
    if t.premium > lo.prem_max || t.premium > so.prem_max {
        return invalid("the premium is above an order's premMax");
    }
    for o in [long, short] {
        if t.expiry < o.daa as i64 + m.bond.term {
            return invalid("the expiry must be at least one term after each order's DAA score");
        }
        if o.s.uw_accept != [0; 32] && o.s.uw_accept != slot.idb() {
            return invalid("an order does not accept this slot");
        }
    }
    let cover = p.cover_of(t.n);
    let adv = advance_for(&slot.s, t.expiry);
    let a = BindArgs::new(t.lock as i64, t.expiry, adv, &[Claim::output(2, cover)]);
    let b2 = slot.s.bind(&slot_ctx(slot, t.lock, 1), &a, slot.value as i64).map_err(refusal)?;
    let fill = |ck: [u8; 32], ct: [u8; 32]| {
        vec![iv(2), iv(t.n), iv(t.x), iv(p.bl), iv(p.bs), bv(&ck), bv(&ct), iv(2), iv(0), iv(t.expiry), iv(t.cover_rate), iv(t.premium), bv(&k.pair.prefix), bv(&k.pair.suffix)]
    };
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.lock(t.lock.max(lo.active_from.max(0) as u64).max(so.active_from.max(0) as u64));
    d.contract(long, OWhich::Open, lo.redeem(k), "fill", fill(so.owner, maker_of(&short.op)), "long order", 0);
    d.contract(short, OWhich::Open, so.redeem(k), "fill", fill(lo.owner, maker_of(&long.op)), "short order", 0);
    d.contract(slot, OWhich::Bond, k.bond.redeem(&slot.s.state_bytes()), "bind", bind_args(&a, &slot.s.owner), "slot", m.bond.rate as u64);
    // Review 10 (R10-F1): an order pays the pair's carrier only with the fill that empties it (`KpfOpenO.escrow`), and
    // the pair needs one. It is taken from the short order when this fill empties it (the carrier comes back to the
    // short with its side at the pair's end), else from the long order when this fill empties that one, else the funder
    // pays it.
    let payer = if short.s.size_left == t.n {
        Some(1usize)
    } else if long.s.size_left == t.n {
        Some(0)
    } else {
        None
    };
    for (i, o) in [(0usize, long), (1, short)] {
        if !o.s.part_ok(t.n) {
            let least = o.s.least_part().map(|n| format!("its least part is {} units", n as f64 / m.p.scale as f64)).unwrap_or_else(|| "its whole margin is below the floor: it can never be filled".into());
            return invalid(format!(
                "the {} order cannot be filled in {} units: every part must keep the order's minimum fill and the market's minimum size, and hold at least {} KAS of its margin, as must what it leaves (review 10); {least}",
                if i == 0 { "long" } else { "short" },
                t.n as f64 / m.p.scale as f64,
                PART_FLOOR as f64 / 1e8
            ));
        }
        let mut spend = o.s.fill_spend(t.n, t.premium) as u64;
        if payer != Some(i) {
            spend -= o.s.carrier_of(t.n) as u64;
        }
        let left = o.s.size_left - t.n;
        if left < 0 || o.value < spend {
            return invalid("an order cannot fund this fill");
        }
        if left > 0 {
            d.obj(OObj::Order(OOpen { size_left: left, ..o.s.clone() }), o.value - spend, o.id.map(|id| (i as u16, id)), "order rest");
        } else {
            d.payout((o.value - spend) as i64, &o.s.owner, "order refund");
        }
    }
    d.genesis(OObj::Pair(p.clone()), p.floor() as u64, 3, "pair");
    d.obj(OObj::Slot(b2), slot.value, slot.id.map(|id| (2, id)), "slot");
    let prem = q(t.n, t.premium, m.p.scale, 0);
    d.pay_underwriter(&slot.s.owner, prem, change_to, uw_extra, "premium")?;
    d.fund(funder, change_to);
    if funder.is_empty() {
        return invalid("an open needs a funding input (it authors the pair's genesis)");
    }
    Ok((d.finish().map_err(|e| premium_advice(e, prem))?, p))
}

// ---------------------------------------------------------------- claims

/// What a claim's proposer funds: what the pending part and the remainder (n < s) hold beyond the pair's own value. That
/// is the bond for a whole-pair claim, and the bond plus the carrier of the new part otherwise (the pending part's
/// `refund`). Of a liquidation's, only a cure pays it all back to the underwriter; a valid settlement pays the bond and
/// the keeper fee and leaves the carrier in the forfeited part (the winner's); a void or refuted claim leaves the
/// carrier in the restored part and pays the bond to the defender (and the disputer) (review 9, R9-N1).
pub fn funded_by_proposer(pair: &OLive<OPair>, w: &OPending) -> i64 {
    let p = &pair.s;
    let rest = if w.n < p.size { p.rest_value(pair.value, w.n) } else { 0 };
    p.pending_value(pair.value, w) + rest - pair.value as i64
}

/// A claim to propose.
#[derive(Clone, Copy, Debug)]
pub struct ClaimTerms {
    pub kind: i64,
    pub side: i64,
    pub n: i64,
    pub price: i64,
    pub t0: i64,
}

/// A proposal: in 0 the pair (propose), 1 the underwriter's vouching input (`vouch`, its own P2PK UTXO), then the
/// funding; out 0 the pending part (under a fresh covenant id), 1 the remainder (n < s, the pair's id), then the vouch
/// remainder to its owner, then change. The vouch input only authorises the claim: `KpfPairO` does not constrain where
/// its value goes, so the builder always pays what is left of it to the underwriter in an explicit output.
/// * Liquidation: the proposer is the underwriter and funds everything the claim adds to the contract outputs,
///   `funded_by_proposer` = pending part + remainder - the pair's value: the bond, plus the carrier of the new part when
///   n < s (the pending part's `refund`, which only a cure pays back to the underwriter: a valid settlement pays it the
///   bond and the keeper fee and leaves the carrier in the forfeited part, the winner's; a void or refuted claim leaves
///   the carrier in the restored part and the bond goes to the defender, review 9 R9-N1). So the underwriter pays the
///   carrier of every split it proposes and gets it back only on a cure. The vouch input funds that sum and `vouch.amount - sum`
///   goes back to the underwriter (no output if it is exactly zero; refused if the vouch holds less, or if the rest is
///   above zero but below the plain-output dust). The funder pays only the network fee.
/// * Close: refused here (review 11): a close is a party's own, with no underwriter input ([`propose_close`]).
pub fn propose(pair: &OLive<OPair>, c: &ClaimTerms, vouch: &KeyUtxo, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, OPending)> {
    let k = pinned::kit();
    let p = &pair.s;
    if c.kind == KIND_CLOSE {
        return invalid("a close is the party's own (review 11): it carries no underwriter input; build it with propose_close (kpf oracle-close)");
    }
    if vouch.key != p.b.uw_key {
        return invalid("only the pair's underwriter can vouch a price for it");
    }
    // Review 11 (R11-F3): KpfPairO.propose refuses a liquidation from a pair UTXO accepted too late (or too early) for
    // any claim on it to be valid.
    if !p.liquidatable_from(pair.daa as i64) {
        return invalid(format!(
            "no liquidation of this pair UTXO can be valid: it was accepted at DAA {}, and a claim must be able to land at least the window and one step ({} DAA) before the cover's expiry {} (review 10, 11). Renew first, then claim against the next expiry",
            pair.daa,
            p.m.p.challenge_daa + p.m.bond.step,
            p.b.expiry
        ));
    }
    let id = pair.id.ok_or_else(|| Error::Invalid("the pair has no covenant id".into()))?;
    let (w, fb) = p.pending_of(k, c.n, c.kind, c.side, c.price, c.t0).ok_or_else(|| Error::Invalid("the claim cannot be proposed (no trigger within g, one owner, or another kind)".into()))?;
    // Review 7: the pending part names the pair input (its outpoint) as its maker.
    let w = w.made_by(&pair.op);
    let sig = if c.kind == KIND_CLOSE { Arg::Sig(if c.side == LONG { p.long_key } else { p.short_key }) } else { bv(&[0u8; 65]) };
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.lock(c.t0.max(0) as u64);
    d.contract(pair, OWhich::Pair, p.redeem(k), "propose", vec![iv(c.n), iv(c.kind), iv(c.side), iv(c.price), iv(c.t0), iv(1), sig, iv(fb), iv(0), bv(&k.pending.prefix), bv(&k.pending.suffix)], "pair", 0);
    d.key(vouch, "the underwriter's vouch");
    // Review 6: the pending part starts a fresh covenant id (a genesis authored by the pair input), which propose checks.
    d.genesis(OObj::Pending(w.clone()), p.pending_value(pair.value, &w) as u64, 0, "pending part");
    if c.n < p.size {
        d.obj(OObj::Pair(p.rest_of(p.size - c.n)), p.rest_value(pair.value, c.n) as u64, Some((0, id)), "remainder");
    }
    let adds = funded_by_proposer(pair, &w);
    let what = if c.n < p.size { format!("the claim's bond {} and the carrier of the new part {}", w.bond, adds - w.bond) } else { format!("the claim's bond {}", w.bond) };
    let back = if c.kind == KIND_CLOSE { vouch.amount as i64 } else { vouch.amount as i64 - adds };
    if back < 0 {
        return invalid(format!("the vouch UTXO holds {} sompi, less than the {adds} sompi the claim adds to the contract outputs ({what}): a liquidation's proposer is the underwriter, whose vouch input funds them", vouch.amount));
    }
    // A remainder too small for an output of its own (KIP-9 storage mass) joins the funder's change when that goes to
    // the underwriter's key; otherwise the vouch must leave nothing or at least VIABLE_OUTPUT_MIN.
    let folded = back > 0 && (back as u64) < VIABLE_OUTPUT_MIN && *change_to == vouch.key;
    if back > 0 && (back as u64) < VIABLE_OUTPUT_MIN && !folded {
        return invalid(format!(
            "the vouch UTXO would leave {back} sompi for the underwriter, too small for an output of its own (at least {VIABLE_OUTPUT_MIN} sompi to be mined): vouch with a UTXO of exactly {adds} sompi ({what}) or at least {} sompi, or fund the claim from the underwriter's key (the remainder then joins its change)",
            adds + VIABLE_OUTPUT_MIN as i64
        ));
    }
    if back > 0 && !folded {
        d.plain(back, &vouch.key, "underwriter: the vouch input's remainder back");
    }
    d.fund(funder, change_to);
    Ok((d.finish()?, w))
}

/// Review 11 (R11-F1): a party's own close of `n` units of its `side` at the price it states for `t0`: in 0 the pair
/// (propose kind 1, the closer's signature, sequence quietDaa), then the funding (the closer's own: it posts the bond, and
/// the carrier of the new part when n < s); out 0 the pending part (a fresh covenant id), 1 the remainder (n < s), then
/// change. No underwriter input. The close settles W after it lands at `price` moved by g against the closer, unless it
/// is contested and refuted (the feed definition: true only within g of the feed at t0 and when it lands), which costs
/// the closer its bond. Refused when the closer is below its maintenance at `price`.
pub fn propose_close(pair: &OLive<OPair>, side: i64, n: i64, price: i64, t0: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, OPending)> {
    let k = pinned::kit();
    let p = &pair.s;
    let id = pair.id.ok_or_else(|| Error::Invalid("the pair has no covenant id".into()))?;
    if side != LONG && side != SHORT {
        return invalid("a close names the long or the short");
    }
    if n < p.size && !p.split_ok(n) {
        return invalid(format!("a close of {n} base units would leave a side of a piece with less than {} KAS of value at the entry (the part floor, review 11)", PART_FLOOR as f64 / 1e8));
    }
    let (w, fb) = p.pending_of(k, n, KIND_CLOSE, side, price, t0).ok_or_else(|| {
        Error::Invalid(format!("the {} is below its maintenance at {} sompi: a party may not close at a price at which it would be liquidated (review 11); add margin first", if side == LONG { "long" } else { "short" }, price))
    })?;
    let w = w.made_by(&pair.op);
    let closer = if side == LONG { p.long_key } else { p.short_key };
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.lock(t0.max(0) as u64);
    d.contract(pair, OWhich::Pair, p.redeem(k), "propose", vec![iv(n), iv(KIND_CLOSE), iv(side), iv(price), iv(t0), iv(0), Arg::Sig(closer), iv(fb), iv(0), bv(&k.pending.prefix), bv(&k.pending.suffix)], "pair", p.m.p.quiet_daa as u64);
    d.genesis(OObj::Pending(w.clone()), p.pending_value(pair.value, &w) as u64, 0, "pending part (the close)");
    if n < p.size {
        d.obj(OObj::Pair(p.rest_of(p.size - n)), p.rest_value(pair.value, n) as u64, Some((0, id)), "remainder");
    }
    d.fund(funder, change_to);
    Ok((d.finish()?, w))
}

// ---------------------------------------------------------------- the trace (review 7, R7-F2)

/// What a wallet proved about a claim's origin before committing stake to it or to its dispute (review 7, R7-F2;
/// DESIGN-ORACLE-IMPL 13, 24.2 (a)): the claim's covenant id was born in transaction `birth` as a genesis of the one
/// output `out` (so, by the induction of 24.1, every later UTXO of the id was written by a template of the family), that
/// output is a pinned `KpfPendingO` in status pending, and it names as its maker (review 7) the pair input `pair_in`,
/// which ran the pinned `KpfPairO`'s `propose` and would write exactly this pending state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimTrace {
    pub claim_id: Hash,
    pub birth: kaspa_consensus_core::tx::TransactionId,
    pub out: u32,
    pub pair_in: TransactionOutpoint,
    pub proposed: OPending,
}

/// What the builders of a commitment (`contest`, `deposit`) are told about the claim: traced, or explicitly not.
#[derive(Clone, Debug)]
pub enum Trust<'a> {
    /// The claim (and, for a deposit, its dispute) was traced: `trace_claim` or `trace_dispute`.
    Traced(&'a ClaimTrace),
    /// Not traced; the user acknowledged that a claim or dispute written by a genesis keeps every stake it receives.
    Untraced,
}

/// What a trace does not prove, for the commands to print.
pub const TRACE_LIMITS: &str = "a transaction's id binds its outpoints, outputs and covenant bindings but not its signature scripts: that the pair input ran `propose` is read from the body as given (the pending template and the claim's id are bound by the output's script and the genesis hash)";

fn hash_eq(a: &Hash, b: &Hash) -> bool {
    a.as_bytes() == b.as_bytes()
}

/// The redeem script an input's signature script ends with, if the push before it is `entry`'s dispatch tag of the
/// pinned artifact `art` and the redeem is `len` bytes (a SilverScript entry: the arguments, the tag, the redeem).
fn ran<'a>(sig: &'a [u8], len: usize, art: &SilAbiArtifact, entry: &str) -> Option<&'a [u8]> {
    let tag = pinned::entry_tag(art, entry);
    let n = sig.len();
    if n < len + 8 || sig[n - len - 3] != 0x4d || u16::from_le_bytes([sig[n - len - 2], sig[n - len - 1]]) as usize != len {
        return None;
    }
    let at = n - len - 8;
    if sig[at] != 0x04 || sig[at + 1..at + 5] != tag {
        return None;
    }
    Some(&sig[n - len..])
}

/// Traces the claim of covenant id `claim_id` to its birth (review 7, R7-F2): `birth` is the transaction that created
/// it, `pending_redeem` its redeem script as born (the pending file, or the contest input's script of its opening).
pub fn trace_claim(claim_id: Hash, birth: &Transaction, pending_redeem: &[u8]) -> Result<ClaimTrace> {
    let k = pinned::kit();
    let outs: Vec<usize> = (0..birth.outputs.len()).filter(|&j| birth.outputs[j].covenant.is_some_and(|c| hash_eq(&c.covenant_id, &claim_id))).collect();
    let [o] = outs[..] else {
        return invalid(format!("the claim's id is carried by {} outputs of its birth transaction, not by exactly one", outs.len()));
    };
    let out = &birth.outputs[o];
    let auth = out.covenant.expect("bound").authorizing_input as usize;
    let a = birth.inputs.get(auth).ok_or_else(|| Error::Invalid("the claim's binding names no input".into()))?;
    if !hash_eq(&covenant_id(a.previous_outpoint, std::iter::once((o as u32, out))), &claim_id) {
        return invalid("the claim's id is not the genesis of that one output (a genesis group of several outputs, or a continuation)");
    }
    if pay_to_script_hash_script(pending_redeem) != out.script_public_key {
        return invalid("the pending part's script is not the claim's output script");
    }
    let w = match OObj::decode(pending_redeem)? {
        OObj::Pending(w) => w,
        o => return invalid(format!("the claim is a {}, not a pinned oracle pending part", o.kind())),
    };
    if w.status != PENDING {
        return invalid("the claim was not born pending");
    }
    let pair_len = k.pair.prefix.len() + PAIR_STATE_LEN + k.pair.suffix.len();
    for i in &birth.inputs {
        if maker_of(&i.previous_outpoint) != w.mk_parent {
            continue;
        }
        let Some(rs) = ran(&i.signature_script, pair_len, pinned::pair_base(), "propose") else {
            return invalid("the input the claim names as its maker did not run KpfPairO.propose");
        };
        let OObj::Pair(p) = OObj::decode(rs)? else { return invalid("the claim's maker is not a pinned oracle pair") };
        let (want, _) = p.pending_of(k, w.n, w.kind, w.side, w.quote, w.t0).ok_or_else(|| Error::Invalid("the maker pair cannot propose this claim".into()))?;
        if want.made_by(&i.previous_outpoint) != w {
            return invalid("the claim is not the pending state its maker pair writes");
        }
        return Ok(ClaimTrace { claim_id, birth: birth.id(), out: o as u32, pair_in: i.previous_outpoint, proposed: w });
    }
    invalid("no input of the birth transaction is the claim's maker (it was not proposed by a pair)")
}

/// Traces a dispute to its opening and its claim to a propose (review 7, R7-F2): `opening` is the transaction whose
/// `KpfStake.open` opened the dispute on the claim (its contest input spends the claim's birth output and carries the
/// pending part's redeem script), `birth` the claim's birth (a `KpfPairO.propose`). Since the claim's id is born clean,
/// every UTXO of it, the dispute's every continuation included, was written by a template of the family.
pub fn trace_dispute(dispute: &OLive<Js>, opening: &Transaction, birth: &Transaction) -> Result<ClaimTrace> {
    let k = pinned::kit();
    let cid = dispute.id.ok_or_else(|| Error::Invalid("the dispute has no covenant id".into()))?;
    if dispute.s.status != crate::judge::STATUS_DISPUTE {
        return invalid("not a dispute");
    }
    let n_cid = opening.outputs.iter().filter(|x| x.covenant.is_some_and(|c| hash_eq(&c.covenant_id, &cid))).count();
    if n_cid != 3 {
        return invalid("the opening does not write the contested part, the opener's receipt and the dispute under the claim's id");
    }
    let pend_len = k.pending.prefix.len() + PENDING_STATE_LEN + k.pending.suffix.len();
    let c = opening
        .inputs
        .iter()
        .find(|i| i.previous_outpoint.transaction_id == birth.id())
        .ok_or_else(|| Error::Invalid("the opening spends nothing of the claim's birth transaction".into()))?;
    let rs = ran(&c.signature_script, pend_len, pinned::pending_base(), "contest").ok_or_else(|| Error::Invalid("the opening's claim input did not run KpfPendingO.contest".into()))?;
    let t = trace_claim(cid, birth, rs)?;
    if c.previous_outpoint.index != t.out {
        return invalid("the opening contests another output of the birth transaction");
    }
    let hashes: Vec<[u8; 32]> = opening
        .outputs
        .iter()
        .filter(|x| x.covenant.is_some_and(|cv| hash_eq(&cv.covenant_id, &cid)))
        .map(|x| {
            let mut h = x.script_public_key.version().to_be_bytes().to_vec();
            h.extend_from_slice(x.script_public_key.script());
            *blake3::hash(&h).as_bytes()
        })
        .collect();
    if !hashes.contains(&dispute.s.spk_hash) {
        return invalid("the dispute names a contested part its opening did not write");
    }
    Ok(t)
}

fn trusted(t: &Trust, id: Option<Hash>) -> Result<()> {
    match t {
        Trust::Untraced => Ok(()),
        Trust::Traced(c) if id.is_some_and(|i| hash_eq(&i, &c.claim_id)) => Ok(()),
        Trust::Traced(_) => invalid("the trace is of another claim"),
    }
}

/// A contest: in 0 the pending part, 1 the opener's free stake (its owner signs), then the funding (which also funds
/// the dispute's base `base`); out 0 the contested part, 1 the opener's receipt, 2 the dispute, then change. `trust`:
/// the claim traced to its birth (`trace_claim`, whose birth output must be this very UTXO), or explicitly untraced.
pub fn contest(pending: &OLive<OPending>, opener: &OLive<Js>, base: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64, trust: &Trust) -> Result<(OBuilt, OPending, Js)> {
    let k = pinned::kit();
    let w = &pending.s;
    if w.status != PENDING {
        return invalid("only a pending claim can be contested");
    }
    let id = pending.id.ok_or_else(|| Error::Invalid("the part has no covenant id".into()))?;
    trusted(trust, pending.id)?;
    if let Trust::Traced(t) = trust {
        if t.birth != pending.op.transaction_id || t.out != pending.op.index || t.proposed != *w {
            return invalid("the trace is of another UTXO of the claim");
        }
    }
    let jm = w.m.judge_market();
    if opener.s.m != jm || opener.s.status != crate::judge::STATUS_FREE {
        return invalid("the opener is not a free stake of this market's judge");
    }
    if opener.daa as i64 + jm.p.age > pending.daa as i64 {
        return invalid(format!("the opener's stake must be accepted {} DAA before the claim (at {})", jm.p.age, pending.daa));
    }
    let p = pending.daa as i64;
    let w2 = w.contested(p, opener.s.owner);
    let w2spk = pay_to_script_hash_script(&w2.redeem(k));
    let mut h = w2spk.version().to_be_bytes().to_vec();
    h.extend_from_slice(w2spk.script());
    let dsp = Js::opened(jm, *blake3::hash(&h).as_bytes(), p, opener.value as i64);
    let receipt = Js::receipt(opener.s.owner, jm, NO, opener.value as i64, 0, p);
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(pending, OWhich::Pending, w.redeem(k), "contest", vec![b8(1), b8(2)], "pending part", 0);
    d.contract(opener, OWhich::Stake, k.stake.redeem(&opener.s.state_bytes()), "open", vec![b8(0), b8(0), b8(2), Arg::Sig(opener.s.owner)], "opener's stake", 0);
    d.obj(OObj::Pending(w2.clone()), pending.value, Some((0, id)), "contested part");
    d.obj(OObj::Stake(receipt), opener.value, Some((0, id)), "receipt");
    d.obj(OObj::Stake(dsp.clone()), base.max(10_000_000), Some((0, id)), "dispute");
    d.fund(funder, change_to);
    Ok((d.finish()?, w2, dsp))
}

/// A deposit on `side`: in 0 the dispute, 1..=k the stakes (each its owner's signature), then the funding; out 0 the
/// dispute, 1..=k the receipts, then change. `trust`: the dispute traced to its opening and its claim to a propose
/// (`trace_dispute`, of this dispute's id), or explicitly untraced (review 7, R7-F2).
pub fn deposit(dispute: &OLive<Js>, side: i64, stakes: &[OLive<Js>], funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64, trust: &Trust) -> Result<(OBuilt, Js)> {
    let k = pinned::kit();
    let d0 = &dispute.s;
    let id = dispute.id.ok_or_else(|| Error::Invalid("the dispute has no covenant id".into()))?;
    trusted(trust, dispute.id)?;
    if stakes.is_empty() || stakes.len() > 7 {
        return invalid("a deposit carries 1 to 7 stakes");
    }
    if side == d0.side {
        return invalid("only the trailing side deposits");
    }
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(dispute, OWhich::Stake, k.stake.redeem(&d0.state_bytes()), "deposit", vec![b8(side), b8(1), b8(stakes.len() as i64)], "dispute", 0);
    let (mut sum, mut units) = (0i64, 0i64);
    for s in stakes {
        if s.daa as i64 + d0.m.p.age > d0.anchor {
            return invalid("a stake must be accepted M_age before the claim");
        }
        d.contract(s, OWhich::Stake, k.stake.redeem(&s.s.state_bytes()), "commit", vec![b8(side), b8(d0.round), b8(d0.anchor), Arg::Sig(s.s.owner)], "stake", 0);
        sum += s.value as i64;
        units += s.value as i64 / UNIT;
    }
    let mut n = d0.clone();
    let (mine, lead) = if side == YES { (d0.yes + sum, d0.no) } else { (d0.no + sum, d0.yes) };
    if side == YES {
        n.yes += sum;
        n.yes_u += units;
    } else {
        n.no += sum;
        n.no_u += units;
    }
    if mine >= d0.m.p.need(lead).map_err(refusal)? {
        n.side = side;
        n.round += 1;
        n.start = -1;
    } else if d0.start == -1 {
        n.start = dispute.daa as i64;
    }
    d.obj(OObj::Stake(n.clone()), dispute.value, Some((0, id)), "dispute");
    for s in stakes {
        d.obj(OObj::Stake(Js::receipt(s.s.owner, d0.m, side, s.value as i64, d0.round, d0.anchor)), s.value, Some((0, id)), "receipt");
    }
    d.fund(funder, change_to);
    Ok((d.finish()?, n))
}

/// The first DAA at which a dispute (accepted at `daa`) can resolve.
pub fn resolve_from(dispute: &OLive<Js>) -> u64 {
    let st = if dispute.s.start == -1 { dispute.daa as i64 } else { dispute.s.start };
    (st + dispute.s.m.p.round).max(0) as u64
}

/// Resolution and delivery: in 0 the dispute (resolve(1)), 1 the contested part (decide(0)), then the funding; out 0
/// the pool, 1 the decided part, then change.
pub fn decide(dispute: &OLive<Js>, contested: &OLive<OPending>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, OPending, Js)> {
    let k = pinned::kit();
    let id = contested.id.ok_or_else(|| Error::Invalid("the part has no covenant id".into()))?;
    if dispute.id != contested.id {
        return invalid("the dispute is not this part's");
    }
    let d0 = &dispute.s;
    let st = if d0.start == -1 { dispute.daa as i64 } else { d0.start };
    let (wl, ll) = if d0.side == YES { (d0.yes_u, d0.no) } else { (d0.no_u, d0.yes) };
    let pool = Js { status: STATUS_RESOLVED, start: st, w_left: wl, l_left: ll, base: dispute.value as i64, ..d0.clone() };
    let w2 = contested.s.decided(d0.side == YES, contested.daa as i64);
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.lock(resolve_from(dispute));
    d.contract(dispute, OWhich::Stake, k.stake.redeem(&d0.state_bytes()), "resolve", vec![b8(1)], "dispute", 0);
    d.contract(contested, OWhich::Pending, contested.s.redeem(k), "decide", vec![b8(0)], "contested part", 0);
    d.obj(OObj::Stake(pool.clone()), dispute.value, Some((0, id)), "pool");
    d.obj(OObj::Pending(w2.clone()), contested.value, Some((1, id)), "decided part");
    d.fund(funder, change_to);
    Ok((d.finish()?, w2, pool))
}

/// Finalize: in 0 the part, then the funding; the outputs of [`OPending::settlement`]: (close) the long at 0, change,
/// the short last; (forfeit) the underwriter at 0, the forfeited part at 1, change; (restore) the defender (refuted) or
/// the underwriter (void) at 0, the restored part at 1, change, the disputer last.
pub fn finalize(pending: &OLive<OPending>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Settlement)> {
    let k = pinned::kit();
    let w = &pending.s;
    let id = pending.id.ok_or_else(|| Error::Invalid("the part has no covenant id".into()))?;
    let (p, seq) = match w.status {
        PENDING => (pending.daa as i64, w.m.p.challenge_daa as u64),
        UPHELD | REFUTED => (w.clock, 0),
        _ => return invalid("a contested part is decided by its dispute's resolution, not finalized"),
    };
    let s = w.settlement(pending.value, p);
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(pending, OWhich::Pending, w.redeem(k), "finalize", vec![], "pending part", seq);
    let mut last: Option<(i64, [u8; 32], &str)> = None;
    match &s {
        Settlement::Close { long, short } => {
            if *long > 0 {
                d.payout(*long, &w.long_key, "long");
            }
            if *short > 0 {
                last = Some((*short, w.short_key, "short"));
            }
        }
        Settlement::Forfeit { part, value, uw } => {
            d.payout(*uw, &w.b.uw_key, "underwriter: bond and fee");
            d.obj(OObj::Pair(part.clone()), *value as u64, Some((0, id)), "forfeited part");
        }
        Settlement::Restore { part, value, defender, disputer, proposer } => {
            if *defender > 0 {
                d.payout(*defender, &w.defender(), "defender: the bond");
            }
            // Review 11: a void liquidation (one that landed after its last valid DAA) pays the bond back to its proposer.
            if *proposer > 0 {
                d.payout(*proposer, &w.b.uw_key, "underwriter: its bond back (a void claim)");
            }
            d.obj(OObj::Pair(part.clone()), *value as u64, Some((0, id)), "restored part");
            if *disputer > 0 {
                last = Some((*disputer, w.disputer, "disputer's share"));
            }
        }
    }
    d.fund(funder, change_to);
    if let Some((v, key, label)) = last {
        d.last(v, &key, label, change_to);
    }
    Ok((d.finish()?, s))
}

/// A cure by the loser (its key signs; `funder` is the loser's own funding): in 0 the part, then the funding; out 0 the
/// underwriter's refund, 1 the cured part, then change.
pub fn cure(pending: &OLive<OPending>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, OPair)> {
    let k = pinned::kit();
    let w = &pending.s;
    let id = pending.id.ok_or_else(|| Error::Invalid("the part has no covenant id".into()))?;
    if w.status != PENDING {
        return invalid("only a pending claim can be cured");
    }
    let (c, v) = w.cure(pending.value).ok_or_else(|| Error::Invalid("no cure: a close, or a cured range beyond the cover rate or priceMax".into()))?;
    let loser = if w.side == LONG { w.long_key } else { w.short_key };
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(pending, OWhich::Pending, w.redeem(k), "cure", vec![Arg::Sig(loser)], "pending part", 0);
    d.payout(w.refund, &w.b.uw_key, "underwriter: refund");
    d.obj(OObj::Pair(c.clone()), v as u64, Some((0, id)), "cured part");
    d.fund(funder, change_to);
    Ok((d.finish()?, c))
}

// ---------------------------------------------------------------- the pair's own entries

/// A renewal of up to 7 pairs of one slot in one bind at lock time `lock`: in 0..k the pairs, k the slot, then the
/// funding; out 0..k the renewed pairs, k the slot, k+1 the premiums to the underwriter, then change.
pub fn renew(pairs: &[OLive<OPair>], slot: &OLive<Bond>, lock: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Vec<OPair>)> {
    renew_with(pairs, slot, lock, funder, change_to, rate, None)
}

/// `renew` with an optional plain UTXO of the underwriter that joins the premium in one output (when the funder's change
/// does not go to the underwriter, a premium below the viable output size could not be paid on its own).
pub fn renew_with(pairs: &[OLive<OPair>], slot: &OLive<Bond>, lock: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64, uw_extra: Option<&KeyUtxo>) -> Result<(OBuilt, Vec<OPair>)> {
    let k = pinned::kit();
    if pairs.is_empty() || pairs.len() > 7 {
        return invalid("one bind renews 1 to 7 pairs");
    }
    let x2 = pairs[0].s.b.expiry + pairs[0].s.m.bond.term;
    let mut claims = vec![];
    let mut renewed = vec![];
    let mut prem = 0i64;
    for (i, lp) in pairs.iter().enumerate() {
        if lp.s.b.expiry + lp.s.m.bond.term != x2 || lp.s.b.uw_id != slot.idb() {
            return invalid("one bind renews pairs of its own slot with one expiry");
        }
        if let Some(why) = lp.s.unrenewable_why(lp.daa) {
            return invalid(why);
        }
        let (r, pay) = lp.s.renewed().ok_or_else(|| Error::Invalid("the premium would leave the range no wider than both maintenance bands: the pair cannot be renewed".into()))?;
        claims.push(Claim::input(i, lp.s.cover_of(lp.s.size)));
        prem += pay;
        renewed.push(r);
    }
    let a = BindArgs::new(lock as i64, x2, advance_for(&slot.s, x2), &claims);
    let b2 = slot.s.bind(&slot_ctx(slot, lock, 1), &a, slot.value as i64).map_err(refusal)?;
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.lock(lock);
    let kk = pairs.len();
    for (i, lp) in pairs.iter().enumerate() {
        d.contract(lp, OWhich::Pair, lp.s.redeem(k), "renew", vec![iv(kk as i64), iv(i as i64)], "pair", 0);
    }
    d.contract(slot, OWhich::Bond, k.bond.redeem(&slot.s.state_bytes()), "bind", bind_args(&a, &slot.s.owner), "slot", slot.s.m.p.rate as u64);
    for (i, (lp, r)) in pairs.iter().zip(&renewed).enumerate() {
        let (_, pay) = lp.s.renewed().expect("checked");
        d.obj(OObj::Pair(r.clone()), lp.value - pay as u64, lp.id.map(|id| (i as u16, id)), "renewed pair");
    }
    d.obj(OObj::Slot(b2), slot.value, slot.id.map(|id| (kk as u16, id)), "slot");
    d.pay_underwriter(&slot.s.owner, prem, change_to, uw_extra, "premium")?;
    d.fund(funder, change_to);
    Ok((d.finish().map_err(|e| premium_advice(e, prem))?, renewed))
}

/// A lapse settle (anyone): in 0 the pair, then the funding; out 0 the long, change, the short last.
pub fn lapse_settle(pair: &OLive<OPair>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let p = &pair.s;
    let (l, s) = p.lapse_out(pair.value);
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.lock(p.lapse_from());
    d.contract(pair, OWhich::Pair, p.redeem(k), "lapseSettle", vec![], "pair", 0);
    if l > 0 {
        d.payout(l, &p.long_key, "long");
    }
    d.fund(funder, change_to);
    if s > 0 {
        d.last(s, &p.short_key, "short", change_to);
    }
    d.finish()
}

/// A cooperative close (both owners sign): out 0 the long's `long_pay`, 1 the short's rest less the fee (the short's
/// output absorbs it).
pub fn coop_close(pair: &OLive<OPair>, long_pay: u64, rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let p = &pair.s;
    if long_pay > pair.value {
        return invalid("the long's payout exceeds the pair");
    }
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(pair, OWhich::Pair, p.redeem(k), "coopClose", vec![Arg::Sig(p.long_key), Arg::Sig(p.short_key)], "pair", 0);
    if long_pay > 0 {
        d.out(long_pay, p2pk_spk(&p.long_key), None, "long", None);
    }
    let c = d.out(0, p2pk_spk(&p.short_key), None, "short (absorbs the fee)", None);
    d.change = Some(c);
    d.finish()
}

/// An amend of `side`'s listing (its owner signs; after quietDaa): out 0 the continuation, then change.
pub fn amend(pair: &OLive<OPair>, side: i64, ask: i64, min_part: i64, tip: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, OPair)> {
    let k = pinned::kit();
    let p = &pair.s;
    let id = pair.id.ok_or_else(|| Error::Invalid("the pair has no covenant id".into()))?;
    let mut c = OPair { mk: Makers::default(), ..p.clone() };
    let key = if side == LONG {
        (c.long_ask, c.long_min, c.long_tip) = (ask, min_part, tip);
        p.long_key
    } else {
        (c.short_bid, c.short_min, c.short_tip) = (ask, min_part, tip);
        p.short_key
    };
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(pair, OWhich::Pair, p.redeem(k), "amend", vec![Arg::Sig(key), iv(side), iv(ask), iv(min_part), iv(tip)], "pair", p.m.p.quiet_daa as u64);
    d.obj(OObj::Pair(c.clone()), pair.value, Some((0, id)), "pair");
    d.fund(funder, change_to);
    Ok((d.finish()?, c))
}

/// The bounds `KpfPairO.transfer` (and its `handOver`) puts on a whole transfer of `side` at `x` with the entrant's
/// bound `nb`, checked before a transaction is built so that a refused one is never signed or paid for (live run 1:
/// an `--nb` above the leaver's bankruptcy price gave a bare verification error). Returns the allowed range of `nb`.
pub fn transfer_bounds(p: &OPair, side: i64, x: i64, nb: i64) -> Result<(i64, i64)> {
    use crate::error::kas_str;
    let mp = &p.m.p;
    let bps = 10_000i128;
    let kas = |v: i64| kas_str(v as i128);
    if x < 0 || x > mp.price_max {
        return invalid(format!("the price {} KAS is outside [0, priceMax {} KAS]", kas(x), kas(mp.price_max)));
    }
    let (lo, hi, rule) = if side == LONG {
        if !(p.long_ask > 0 && x >= p.long_ask) {
            return invalid(format!("the long is not listed at or below {} KAS (ask {}, 0 is unlisted): the contract takes a long only at or above its ask", kas(x), kas(p.long_ask)));
        }
        // nb <= bl (no-downgrade: the stayer's counterparty bound may not rise), healthy at x
        // (x * (10^4 - mmr) >= nb * 10^4), nb >= 0, and the new range within the cover rate (bs - nb <= coverRate).
        // Review 9 (R9-F3): below bl only as far as the short's reach leaves room (`shortReach() - nb <= coverRate`);
        // the leaver's own bound is always allowed.
        let healthy = ((x as i128) * (bps - mp.mmr_bps as i128) / bps) as i64;
        let lo = p.growth_limit(LONG).min(p.bl);
        let hi = p.bl.min(healthy);
        let rule = if nb > p.bl {
            format!("no-downgrade: the entrant's long bound may not be above the leaver's bankruptcy price bl {} KAS", kas(p.bl))
        } else if nb > healthy {
            format!("the entrant's long must be healthy at {} KAS: its bound at most {} KAS ({} % maintenance)", kas(x), kas(healthy), mp.mmr_bps as f64 / 100.0)
        } else if nb < lo {
            format!("the entrant's long bound may be at least {} KAS: the range within the cover rate {} KAS per unit, leaving the short its share up to {} KAS (review 9: each side's half of the cover from the entry)", kas(lo), kas(p.b.cover_rate), kas(p.short_reach()))
        } else {
            String::new()
        };
        (lo, hi, rule)
    } else {
        if !(p.short_bid > 0 && x <= p.short_bid) {
            return invalid(format!("the short is not listed at or above {} KAS (bid {}, 0 is unlisted): the contract takes a short only at or below its bid", kas(x), kas(p.short_bid)));
        }
        // nb >= bs (no-downgrade), nb <= priceMax, healthy at x (nb * 10^4 >= x * (10^4 + mmr)), the range within the
        // cover rate (nb - bl <= coverRate).
        let healthy = (((x as i128) * (bps + mp.mmr_bps as i128) + bps - 1) / bps) as i64;
        let lo = p.bs.max(healthy);
        let hi = p.growth_limit(SHORT).max(p.bs);
        let rule = if nb < p.bs {
            format!("no-downgrade: the entrant's short bound may not be below the leaver's bankruptcy price bs {} KAS", kas(p.bs))
        } else if nb < healthy {
            format!("the entrant's short must be healthy at {} KAS: its bound at least {} KAS ({} % maintenance)", kas(x), kas(healthy), mp.mmr_bps as f64 / 100.0)
        } else if nb > hi {
            format!("the entrant's short bound may be at most {} KAS: the range within the cover rate {} KAS per unit, leaving the long its share down to {} KAS (review 9: each side's half of the cover from the entry), and priceMax {} KAS", kas(hi), kas(p.b.cover_rate), kas(p.long_reach()), kas(mp.price_max))
        } else {
            String::new()
        };
        (lo, hi, rule)
    };
    if lo > hi {
        return invalid(format!("no entrant's bound is allowed at {} KAS (it would have to lie in {} .. {} KAS): {}", kas(x), kas(lo), kas(hi), if rule.is_empty() { "the leaver's side is not healthy at this price".to_string() } else { rule }));
    }
    if !rule.is_empty() {
        return invalid(format!("--nb {} KAS refused: {rule}; allowed: {} .. {} KAS", kas(nb), kas(lo), kas(hi)));
    }
    Ok((lo, hi))
}

/// A whole transfer of a listed `side` at `x` to a signing taker `entrant` posting bound `nb` (the taker funds it):
/// out 0 the leaver, 1 the new pair (under a fresh covenant id), then change.
pub fn transfer(pair: &OLive<OPair>, side: i64, x: i64, nb: i64, entrant: [u8; 32], funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, OPair)> {
    let k = pinned::kit();
    let p = &pair.s;
    let _id = pair.id.ok_or_else(|| Error::Invalid("the pair has no covenant id".into()))?;
    if entrant == p.b.uw_key {
        return invalid("the underwriter cannot be a party");
    }
    transfer_bounds(p, side, x, nb)?;
    let sc = p.m.p.scale;
    let mut part = p.rest_of(p.size);
    let (pay, leaver) = if side == LONG {
        part.bl = nb;
        part.long_key = entrant;
        (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
        (crate::payoff::owed(p.size, x - p.bl, p.long_tip, sc), p.long_key)
    } else {
        part.bs = nb;
        part.short_key = entrant;
        (part.short_bid, part.short_min, part.short_tip) = (0, 0, 0);
        (crate::payoff::owed(p.size, p.bs - x, p.short_tip, sc), p.short_key)
    };
    // Review 7: the part names the pair input as its parent (a signing taker is no family input: no entrant maker).
    part.mk.parent = maker_of(&pair.op);
    let keep = pair.value as i64 - p.range_of(p.size);
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(pair, OWhich::Pair, p.redeem(k), "transfer", vec![iv(side), iv(p.size), iv(x), iv(nb), bv(&entrant), bv(&[0; 32]), iv(if pay >= PLAIN_DUST { 1 } else { 0 })], "pair", p.m.p.quiet_daa as u64);
    if pay >= PLAIN_DUST {
        d.payout(pay, &leaver, "leaver");
    }
    // Review 6: the entrant's pair starts a fresh covenant id (a genesis authored by the pair input), which transfer checks.
    d.genesis(OObj::Pair(part.clone()), (part.range_of(p.size) + keep + if pay < PLAIN_DUST { pay } else { 0 }) as u64, 0, "pair");
    d.fund(funder, change_to);
    Ok((d.finish()?, part))
}

/// A mutual close of the whole pair at `x` (both listings cross; anyone): out 0 the long, change, the short last.
pub fn mutual_close(pair: &OLive<OPair>, x: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let p = &pair.s;
    let sc = p.m.p.scale;
    let mut lp = crate::payoff::owed(p.size, x - p.bl, p.long_tip, sc);
    let mut sp = pair.value as i64 - lp;
    if lp < PLAIN_DUST {
        sp += lp;
        lp = 0;
    }
    if sp < PLAIN_DUST {
        lp += sp;
        sp = 0;
    }
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(pair, OWhich::Pair, p.redeem(k), "mutualClose", vec![iv(p.size), iv(x)], "pair", p.m.p.quiet_daa as u64);
    if lp > 0 {
        d.payout(lp, &p.long_key, "long");
    }
    d.fund(funder, change_to);
    if sp > 0 {
        d.last(sp, &p.short_key, "short", change_to);
    }
    d.finish()
}

// ---------------------------------------------------------------- the slot and the stakes (slices A and B1)

/// `KpfBond.advance` (anyone) by `steps` at lock time `lock`.
pub fn slot_advance(slot: &OLive<Bond>, steps: i64, lock: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Bond)> {
    let k = pinned::kit();
    let b2 = slot.s.advance(&Ctx { seq: 0, ..slot_ctx(slot, lock, 1) }, steps, slot.value as i64).map_err(refusal)?;
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.lock(lock);
    d.contract(slot, OWhich::Bond, k.bond.redeem(&slot.s.state_bytes()), "advance", vec![b8(steps)], "slot", 0);
    d.obj(OObj::Slot(b2.clone()), slot.value, slot.id.map(|id| (0, id)), "slot");
    d.fund(funder, change_to);
    Ok((d.finish()?, b2))
}

/// `KpfBond.split` (owner): child A with ring `a` and value `va`, child B the rest.
pub fn slot_split(slot: &OLive<Bond>, a: &[i64; 12], va: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Bond, Bond)> {
    let k = pinned::kit();
    let vb = slot.value.checked_sub(va).ok_or_else(|| Error::Invalid("child A is worth more than the slot".into()))?;
    let (ba, bb) = slot.s.split(&slot_ctx(slot, 0, 2), a, va as i64, vb as i64).map_err(refusal)?;
    let id = slot.id.ok_or_else(|| Error::Invalid("the slot has no covenant id".into()))?;
    let mut args: Vec<Arg> = a.iter().map(|v| b8(*v)).collect();
    args.push(Arg::Sig(slot.s.owner));
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(slot, OWhich::Bond, k.bond.redeem(&slot.s.state_bytes()), "split", args, "slot", slot.s.m.p.rate as u64);
    d.obj(OObj::Slot(ba.clone()), va, Some((0, id)), "child A");
    d.obj(OObj::Slot(bb.clone()), vb, Some((0, id)), "child B");
    d.fund(funder, change_to);
    Ok((d.finish()?, ba, bb))
}

/// `KpfBond.unbond` (owner) after advancing `steps` at lock time `lock`.
pub fn slot_unbond(slot: &OLive<Bond>, steps: i64, lock: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Bond)> {
    let k = pinned::kit();
    let b2 = slot.s.unbond(&slot_ctx(slot, lock, 1), steps, slot.value as i64).map_err(refusal)?;
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.lock(lock);
    d.contract(slot, OWhich::Bond, k.bond.redeem(&slot.s.state_bytes()), "unbond", vec![b8(steps), Arg::Sig(slot.s.owner)], "slot", slot.s.m.p.rate as u64);
    d.obj(OObj::Slot(b2.clone()), slot.value, slot.id.map(|id| (0, id)), "unbonding slot");
    d.fund(funder, change_to);
    Ok((d.finish()?, b2))
}

/// `KpfBond.withdraw` (owner, unbonding and U old): the whole value to `to`, less the fee.
pub fn slot_withdraw(slot: &OLive<Bond>, to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(slot, OWhich::Bond, k.bond.redeem(&slot.s.state_bytes()), "withdraw", vec![Arg::Sig(slot.s.owner)], "slot", slot.s.m.p.unbond as u64);
    let c = d.out(0, p2pk_spk(to), None, "withdrawal (absorbs the fee)", None);
    d.change = Some(c);
    d.finish()
}

/// `KpfStake.unbond` (owner): the stake starts leaving.
pub fn stake_unbond(stake: &OLive<Js>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Js)> {
    let k = pinned::kit();
    let s2 = Js { status: crate::judge::STATUS_UNBONDING, ..stake.s.clone() };
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(stake, OWhich::Stake, k.stake.redeem(&stake.s.state_bytes()), "unbond", vec![Arg::Sig(stake.s.owner)], "stake", 0);
    d.obj(OObj::Stake(s2.clone()), stake.value, stake.id.map(|id| (0, id)), "unbonding stake");
    d.fund(funder, change_to);
    Ok((d.finish()?, s2))
}

/// `KpfStake.withdraw` (owner, U old): the whole value to `to`, less the fee.
pub fn stake_withdraw(stake: &OLive<Js>, to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(stake, OWhich::Stake, k.stake.redeem(&stake.s.state_bytes()), "withdraw", vec![Arg::Sig(stake.s.owner)], "stake", stake.s.m.p.unbond as u64);
    let c = d.out(0, p2pk_spk(to), None, "withdrawal (absorbs the fee)", None);
    d.change = Some(c);
    d.finish()
}

/// `KpfStake.settle` of up to 7 receipts with their pool (anyone): out 0 the pool, the winners' free stakes at their own
/// indices; the first loser's index carries the change (a loser is forfeited).
pub fn pool_settle(pool: &OLive<Js>, receipts: &[OLive<Js>], funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Js)> {
    let k = pinned::kit();
    let p = &pool.s;
    let id = pool.id.ok_or_else(|| Error::Invalid("the pool has no covenant id".into()))?;
    if receipts.is_empty() || receipts.len() > 7 {
        return invalid("one settlement carries 1 to 7 receipts");
    }
    let (wu, l) = if p.side == YES { (p.yes_u, p.no) } else { (p.no_u, p.yes) };
    let lnet = l - p.m.p.burn(l).map_err(refusal)?;
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(pool, OWhich::Stake, k.stake.redeem(&p.state_bytes()), "settle", vec![b8(1), b8(receipts.len() as i64)], "pool", 0);
    for r in receipts {
        if r.id != pool.id || r.s.anchor != p.anchor {
            return invalid("a receipt of another dispute");
        }
        d.contract(r, OWhich::Stake, k.stake.redeem(&r.s.state_bytes()), "redeem", vec![b8(0)], "receipt", 0);
    }
    for u in funder {
        d.key(u, "funding");
    }
    d.obj(OObj::Stake(p.clone()), 0, Some((0, id)), "pool");
    let (mut wl, mut ll, mut gained, mut paid) = (p.w_left, p.l_left, 0i64, 0i64);
    let mut change = None;
    for r in receipts {
        if r.s.side == p.side {
            let u = r.s.amt / UNIT;
            let sh = crate::judge::share(u, lnet, wu).map_err(refusal)?;
            d.obj(OObj::Stake(Js::free(r.s.owner, p.m)), r.value + sh as u64, None, "winner's stake");
            wl -= u;
            paid += sh;
        } else {
            ll -= r.s.amt;
            gained += r.value as i64;
            if change.is_none() {
                change = Some(d.out(0, p2pk_spk(change_to), None, "change", None));
            } else {
                d.plain(100_000_000, change_to, "filler");
            }
        }
    }
    let p2 = Js { w_left: wl, l_left: ll, ..p.clone() };
    let pv = pool.value as i64 + gained - paid;
    if pv < 0 {
        return invalid("the pool cannot pay these winners before the losers are forfeited");
    }
    d.outs[0].value = pv as u64;
    d.outs[0].spk = OObj::Stake(p2.clone()).spk();
    d.outs[0].obj = Some(OObj::Stake(p2.clone()));
    let c = match change {
        Some(c) => c,
        None => d.out(0, p2pk_spk(change_to), None, "change", None),
    };
    d.change = Some(c);
    Ok((d.finish()?, p2))
}

/// `KpfStake.close` of a settled pool (anyone): its base back to `change_to` (absorbing the fee), the rest burned.
pub fn pool_close(pool: &OLive<Js>, change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let b = pool.value as i64 - pool.s.base;
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(pool, OWhich::Stake, k.stake.redeem(&pool.s.state_bytes()), "close", vec![b8(1)], "pool", 0);
    let c = d.out(0, p2pk_spk(change_to), None, "the pool's base (absorbs the fee)", None);
    d.change = Some(c);
    if b >= crate::judge::BURN_DUST {
        // A burn the contract fixes at its own output (at least b): below VIABLE_OUTPUT_MIN the closer's base tops it up.
        d.out((b as u64).max(VIABLE_OUTPUT_MIN), crate::lock::burn_spk(), None, "burn", None);
    }
    d.finish()
}

// ---------------------------------------------------------------- signing on two machines

/// What a plain key input's script costs to run (one Schnorr check): the budget committed for an input its owner
/// signs later, on another machine.
pub const KEY_INPUT_UNITS: u64 = 100_001;

/// A transaction signed by every party but the owners of some plain key inputs (an executor's funding): its outputs,
/// budgets and fee are final, so the signatures it carries hold, and each open input needs only its owner's signature.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OPartial {
    pub tx: Transaction,
    pub entries: Vec<UtxoEntry>,
    /// The inputs still to be signed, with the key each needs.
    pub open: Vec<(usize, [u8; 32])>,
    pub input_labels: Vec<String>,
    pub output_labels: Vec<String>,
    /// (output index, redeem script, covenant id) of every object the transaction creates.
    pub created: Vec<(usize, Vec<u8>, Hash)>,
    /// The signatures still to be made that a contract's script checks (`sign_open`): the input, where in its
    /// signature script the 65 bytes are, and the key.
    #[serde(default)]
    pub open_sigs: Vec<(usize, usize, [u8; 32])>,
}

/// What stands in a signature script where a signature of another machine's key will be: 65 bytes no signature is
/// (they end as one does), told apart by the input and by which of its signatures it is.
fn sig_mark(i: usize, j: usize) -> Vec<u8> {
    let mut m = vec![0xA5u8; 65];
    (m[0], m[1], m[2], m[64]) = (0xC3, i as u8, j as u8, 0x01);
    m
}

impl OBuilt {
    /// Signs with the keys at hand and leaves open the plain inputs of other keys AND the signatures of other keys
    /// that a contract's script checks (the thin family's entries that two users sign). A script that waits for a
    /// signature cannot be run here, so its budget cannot be measured here: `units` are every input's script units
    /// as measured on a transaction of the same shape in which every key was at hand (the caller builds it with
    /// keys of its own in the places of the missing ones). A budget too small is found by whoever signs last, whose
    /// tool runs every script before it sends anything.
    pub fn sign_open(&self, keys: &KeySet, units: &[u64]) -> Result<OPartial> {
        self.vouch_guard()?;
        if units.len() != self.plans.len() {
            return invalid("the measured units are not of this transaction's inputs");
        }
        let open: Vec<(usize, [u8; 32])> = self.plans.iter().enumerate().filter_map(|(i, p)| if let OPlan::Key { key } = p { (!keys.has(key)).then_some((i, *key)) } else { None }).collect();
        let is_open = |i: usize| open.iter().any(|(j, _)| *j == i);
        // Every signature a script takes, in the order its signature script is written; those of keys not at hand
        // are noted when `waits` is given.
        let fill = |b: &OBuilt, tx: &mut Transaction, sized: bool, waits: Option<&std::cell::RefCell<Vec<(usize, usize, [u8; 32])>>>| -> Result<()> {
            for (i, p) in b.plans.iter().enumerate() {
                if is_open(i) {
                    tx.inputs[i].signature_script = if sized { push_data(&[0u8; 65]) } else { vec![] };
                    continue;
                }
                let d = sighash(tx, &b.entries, i);
                let n = std::cell::Cell::new(0usize);
                tx.inputs[i].signature_script = p.sigscript(&|k| {
                    let j = n.get();
                    n.set(j + 1);
                    if keys.has(k) {
                        return keys.sign(k, &d);
                    }
                    if let Some(w) = waits {
                        w.borrow_mut().push((i, j, *k));
                    }
                    Ok(sig_mark(i, j))
                })?;
            }
            Ok(())
        };
        let mut b = self.clone();
        for (i, u) in units.iter().enumerate() {
            b.tx.inputs[i].compute_commit = ComputeBudget(budget_for_units(if is_open(i) { KEY_INPUT_UNITS.max(*u) } else { *u })).into();
        }
        for _ in 0..6 {
            let mut sized = b.tx.clone();
            fill(&b, &mut sized, true, None)?;
            let r = masses(&sized, &b.entries);
            let fee = target_fee(&r, b.rate, b.mode);
            let have: u128 = b.entries.iter().map(|e| e.amount as u128).sum();
            let out: u128 = b.tx.outputs.iter().map(|o| o.value as u128).sum();
            if have.checked_sub(out) == Some(fee as u128) {
                break;
            }
            b.set_change(fee)?;
        }
        let mut sized = b.tx.clone();
        fill(&b, &mut sized, true, None)?;
        let rep = masses(&sized, &b.entries);
        let mut tx = b.tx.clone();
        tx.set_storage_mass(rep.storage);
        let waits = std::cell::RefCell::new(vec![]);
        fill(&b, &mut tx, false, Some(&waits))?;
        tx.finalize();
        if let Some(m) = storage_refusal(&tx, &b.entries, &b.output_labels) {
            return invalid(m);
        }
        let mut open_sigs = vec![];
        for (i, j, k) in waits.into_inner() {
            let (s, m) = (&tx.inputs[i].signature_script, sig_mark(i, j));
            let at: Vec<usize> = (0..s.len().saturating_sub(64)).filter(|a| s[*a..*a + 65] == m[..]).collect();
            if at.len() != 1 {
                return invalid(format!("input {i}: the place of its signature {j} is not found once in its signature script"));
            }
            open_sigs.push((i, at[0], k));
        }
        if open.is_empty() && open_sigs.is_empty() {
            return invalid("every signer's key is here: nothing is left to a co-signer");
        }
        let res = crate::verify::run_inputs(&tx, &b.entries, crate::verify::Meter::Unlimited);
        for (i, r) in res.iter().enumerate() {
            if !is_open(i) && !open_sigs.iter().any(|(j, _, _)| *j == i) && r.is_err() {
                return invalid(format!("input {i} ({}) fails locally: {r:?}", b.input_labels[i]));
            }
        }
        Ok(OPartial { tx, entries: b.entries, open, input_labels: b.input_labels, output_labels: b.output_labels, created: b.created.iter().map(|(i, o, id)| (*i, o.redeem(), *id)).collect(), open_sigs })
    }

    /// Signs with the keys at hand and leaves the plain key inputs of other keys open (`OPartial`). Only plain inputs
    /// may be left: a signature a contract's script checks is made here. An open input is given the budget of a key
    /// input and, for the fee, the size its signature will have.
    pub fn sign_partial(&self, keys: &KeySet) -> Result<OPartial> {
        self.vouch_guard()?;
        let mut open: Vec<(usize, [u8; 32])> = vec![];
        for (i, p) in self.plans.iter().enumerate() {
            match p {
                OPlan::Key { key } if !keys.has(key) => open.push((i, *key)),
                _ => {
                    for k in p.signers() {
                        if !keys.has(&k) {
                            return Err(Error::Signing(format!(
                                "input {i} ({}) needs a signature of {} inside its script and no such key was given: only plain funding inputs are left to a co-signer",
                                self.input_labels[i],
                                crate::error::to_hex(&k)
                            )));
                        }
                    }
                }
            }
        }
        if open.is_empty() {
            return invalid("every signer's key is here: nothing is left to a co-signer");
        }
        let is_open = |i: usize| open.iter().any(|(j, _)| *j == i);
        let fill = |b: &OBuilt, tx: &mut Transaction, placeholder: bool| -> Result<()> {
            for (i, p) in b.plans.iter().enumerate() {
                if is_open(i) {
                    // The size of the signature to come (65 bytes behind a one-byte push), or nothing.
                    tx.inputs[i].signature_script = if placeholder { push_data(&[0u8; 65]) } else { vec![] };
                    continue;
                }
                let d = sighash(tx, &b.entries, i);
                tx.inputs[i].signature_script = p.sigscript(&|k| keys.sign(k, &d))?;
            }
            Ok(())
        };
        let mut b = self.clone();
        let mut tx = b.tx.clone();
        fill(&b, &mut tx, true)?;
        let units = crate::verify::run_inputs(&tx, &b.entries, crate::verify::Meter::Unlimited);
        for (i, u) in units.iter().enumerate() {
            let budget = if is_open(i) {
                budget_for_units(KEY_INPUT_UNITS)
            } else {
                match u {
                    Ok(u) => budget_for_units(*u),
                    Err(_) => 2_000,
                }
            };
            tx.inputs[i].compute_commit = ComputeBudget(budget).into();
        }
        b.tx = tx;
        for _ in 0..6 {
            let r = masses(&b.tx, &b.entries);
            let fee = target_fee(&r, b.rate, b.mode);
            let have: u128 = b.entries.iter().map(|e| e.amount as u128).sum();
            let out: u128 = b.tx.outputs.iter().map(|o| o.value as u128).sum();
            if have.checked_sub(out) == Some(fee as u128) {
                break;
            }
            b.set_change(fee)?;
        }
        // The storage mass and the id are those of the finished transaction: computed with the placeholders in place.
        let mut sized = b.tx.clone();
        fill(&b, &mut sized, true)?;
        let rep = masses(&sized, &b.entries);
        let mut tx = b.tx.clone();
        tx.set_storage_mass(rep.storage);
        fill(&b, &mut tx, false)?;
        tx.finalize();
        if let Some(m) = storage_refusal(&tx, &b.entries, &b.output_labels) {
            return invalid(m);
        }
        let res = crate::verify::run_inputs(&tx, &b.entries, crate::verify::Meter::Unlimited);
        for (i, r) in res.iter().enumerate() {
            if !is_open(i) && r.is_err() {
                return invalid(format!("input {i} ({}) fails locally: {r:?}", b.input_labels[i]));
            }
        }
        Ok(OPartial { tx, entries: b.entries, open, input_labels: b.input_labels, output_labels: b.output_labels, created: b.created.iter().map(|(i, o, id)| (*i, o.redeem(), *id)).collect(), open_sigs: vec![] })
    }
}

impl OPartial {
    /// Every input the file says is signed passes its script as the transaction stands (the open inputs' signatures
    /// do not enter another input's check): what a co-signer verifies before it signs anything.
    pub fn check_signed(&self) -> Result<()> {
        if self.tx.inputs.len() != self.entries.len() {
            return invalid("the file's inputs and their UTXO entries do not match");
        }
        let res = crate::verify::run_inputs(&self.tx, &self.entries, crate::verify::Meter::Unlimited);
        for (i, r) in res.iter().enumerate() {
            // A script that still waits for a signature cannot pass yet: it is run when the last one is in.
            if !self.open.iter().any(|(j, _)| *j == i) && !self.open_sigs.iter().any(|(j, _, _)| *j == i) && r.is_err() {
                return invalid(format!("input {i}, which the other party should have signed, fails its script: {r:?}"));
            }
        }
        Ok(())
    }

    /// The covenant id output `i` carries, if this transaction gives it one: the id of its authorizing input's UTXO
    /// (a continuation), or the genesis id of the outputs that input writes under it. `None` for an output without a
    /// binding, or with an id the transaction does not give (review C11, C11-F1: an object is its script and its id).
    pub fn bound(&self, i: usize) -> Option<Hash> {
        let b = self.tx.outputs.get(i)?.covenant?;
        let auth = b.authorizing_input as usize;
        let entry = self.entries.get(auth)?;
        if b.covenant_id == Hash::default() {
            return None;
        }
        // A continuation: the authorizing input's UTXO carries the id. Not an open input: that is a plain coin of
        // the co-signer's and carries none, whatever the file says of it (review C12, C12-N1).
        if !self.open.iter().any(|(j, _)| *j == auth) && entry.covenant_id == Some(b.covenant_id) {
            return Some(b.covenant_id);
        }
        let group: Vec<(u32, &TransactionOutput)> = self.tx.outputs.iter().enumerate().filter(|(_, o)| o.covenant == Some(b)).map(|(j, o)| (j as u32, o)).collect();
        (covenant_id(self.tx.inputs.get(auth)?.previous_outpoint, group.into_iter()) == b.covenant_id).then_some(b.covenant_id)
    }

    /// Signs whatever is open and of a key at hand: plain inputs, and signatures a script checks. What is still open
    /// is in the file returned; with nothing open it is `finished`. A signature holds for this transaction's inputs
    /// and outputs only, whatever else the file says.
    pub fn sign_more(&self, keys: &KeySet) -> Result<OPartial> {
        let mut p = self.clone();
        if p.tx.inputs.len() != p.entries.len() {
            return invalid("the file's inputs and their UTXO entries do not match");
        }
        for (i, key) in &self.open {
            if *i >= p.tx.inputs.len() || p.entries[*i].script_public_key != p2pk_spk(key) {
                return invalid(format!("input {i} is not a plain output of the key it is left to"));
            }
            if keys.has(key) {
                let d = sighash(&p.tx, &p.entries, *i);
                p.tx.inputs[*i].signature_script = push_data(&keys.sign(key, &d)?);
            }
        }
        for (i, at, key) in &self.open_sigs {
            if *i >= p.tx.inputs.len() || self.open.iter().any(|(j, _)| j == i) || p.tx.inputs[*i].signature_script.len() < at + 65 {
                return invalid(format!("input {i} has no place for the signature the file says it waits for"));
            }
            // A signature inside a script is a script's: a plain coin is left to its key in `open`, where whoever
            // signs is told it spends a coin (review T1b, F19).
            if crate::tx::p2pk_key(&p.entries[*i].script_public_key).is_some() {
                return invalid(format!("input {i} is a plain coin, which the file lists among the signatures a script waits for"));
            }
            if keys.has(key) {
                let d = sighash(&p.tx, &p.entries, *i);
                let sig = keys.sign(key, &d)?;
                p.tx.inputs[*i].signature_script[*at..*at + 65].copy_from_slice(&sig);
            }
        }
        p.open.retain(|(_, k)| !keys.has(k));
        p.open_sigs.retain(|(_, _, k)| !keys.has(k));
        Ok(p)
    }

    /// The finished transaction of a file in which nothing is open. Not a verdict on it: an input whose script fails
    /// is given 0 units here, and whoever sends it runs `verify` first (the command line does, and sends nothing
    /// that fails).
    pub fn finished(&self) -> Result<OSigned> {
        if !self.open.is_empty() || !self.open_sigs.is_empty() {
            return invalid("signatures are still missing");
        }
        if self.tx.inputs.len() != self.entries.len() {
            return invalid("the file's inputs and their UTXO entries do not match");
        }
        let mut tx = self.tx.clone();
        tx.finalize();
        let mass = masses(&tx, &self.entries);
        let total_in: u64 = self.entries.iter().map(|e| e.amount).sum();
        let total_out: u64 = tx.outputs.iter().map(|o| o.value).sum();
        if total_out > total_in {
            return invalid("the outputs exceed the inputs");
        }
        let units = crate::verify::run_inputs(&tx, &self.entries, crate::verify::Meter::Unlimited).into_iter().map(|r| r.unwrap_or(0)).collect();
        Ok(OSigned { fee: total_in - total_out, tx, entries: self.entries.clone(), mass, units, input_labels: self.input_labels.clone(), output_labels: self.output_labels.clone(), created: vec![] })
    }

    /// The open inputs' owner signs them. Every open input must be a plain output of its key, and every key at hand.
    pub fn cosign(&self, keys: &KeySet) -> Result<OSigned> {
        if !self.open_sigs.is_empty() {
            return invalid("this transaction waits for signatures inside a script: it is signed with `sign_more`");
        }
        let mut tx = self.tx.clone();
        if tx.inputs.len() != self.entries.len() {
            return invalid("the file's inputs and their UTXO entries do not match");
        }
        for (i, key) in &self.open {
            if *i >= tx.inputs.len() || self.entries[*i].script_public_key != p2pk_spk(key) {
                return invalid(format!("input {i} is not a plain output of the key it is left to"));
            }
            if !keys.has(key) {
                return Err(Error::Signing(format!("input {i} needs a signature of {} and no such key was given", crate::error::to_hex(key))));
            }
        }
        for (i, key) in &self.open {
            let d = sighash(&tx, &self.entries, *i);
            tx.inputs[*i].signature_script = push_data(&keys.sign(key, &d)?);
        }
        tx.finalize();
        let mass = masses(&tx, &self.entries);
        let total_in: u64 = self.entries.iter().map(|e| e.amount).sum();
        let total_out: u64 = tx.outputs.iter().map(|o| o.value).sum();
        if total_out > total_in {
            return invalid("the outputs exceed the inputs");
        }
        let units = crate::verify::run_inputs(&tx, &self.entries, crate::verify::Meter::Unlimited).into_iter().map(|r| r.unwrap_or(0)).collect();
        let created = self.created.iter().map(|(i, rs, id)| Ok((*i, OObj::decode(rs)?, *id))).collect::<Result<Vec<_>>>()?;
        Ok(OSigned { fee: total_in - total_out, tx, entries: self.entries.clone(), mass, units, input_labels: self.input_labels.clone(), output_labels: self.output_labels.clone(), created })
    }
}
