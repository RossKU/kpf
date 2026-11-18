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
use crate::state::{sm8, KIND_CLOSE, LONG, PLAIN_DUST};
use crate::tx::{budget_for_units, masses, p2pk_spk, push_data, sighash, target_fee, Arg, FeeMode, KeySet, KeyUtxo, MassReport, DUST_OUTPUT_MIN, TX_VERSION};

/// Which template an input runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OWhich {
    Open,
    Pair,
    Pending,
    Bond,
    Stake,
}

impl OWhich {
    fn base(self) -> &'static SilAbiArtifact {
        match self {
            OWhich::Open => pinned::open_base(),
            OWhich::Pair => pinned::pair_base(),
            OWhich::Pending => pinned::pending_base(),
            OWhich::Bond => pinned::bond_base(),
            OWhich::Stake => pinned::stake_base(),
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
        invalid("not an instance of any pinned oracle-market template")
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
    label: String,
    obj: Option<OObj>,
}

/// A transaction under construction.
pub struct ODraft {
    ins: Vec<DIn>,
    outs: Vec<DOut>,
    lock: u64,
    change: Option<usize>,
    rate: u64,
    mode: FeeMode,
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
        ODraft { ins: vec![], outs: vec![], lock: 0, change: None, rate, mode }
    }
    pub fn lock(&mut self, t: u64) {
        self.lock = self.lock.max(t);
    }
    fn input(&mut self, op: TransactionOutpoint, entry: UtxoEntry, plan: OPlan, label: impl Into<String>, seq: u64) -> usize {
        self.ins.push(DIn { op, entry, plan, label: label.into(), seq });
        self.ins.len() - 1
    }
    fn contract<T>(&mut self, l: &OLive<T>, which: OWhich, redeem: Vec<u8>, entry: &'static str, args: Vec<Arg>, label: &str, seq: u64) -> usize {
        let spk = pay_to_script_hash_script(&redeem);
        self.input(l.op, entry_of(l.value, spk, l.daa, l.id), OPlan::Contract { which, redeem, entry, args }, label, seq)
    }
    fn key(&mut self, u: &KeyUtxo, label: &str) -> usize {
        self.input(u.outpoint, u.entry(), OPlan::Key { key: u.key }, label, 0)
    }
    fn out(&mut self, value: u64, spk: ScriptPublicKey, binding: Option<(u16, Hash)>, label: &str, obj: Option<OObj>) -> usize {
        self.outs.push(DOut { value, spk, binding, genesis: None, label: label.into(), obj });
        self.outs.len() - 1
    }
    fn obj(&mut self, o: OObj, value: u64, binding: Option<(u16, Hash)>, label: &str) -> usize {
        let spk = o.spk();
        self.out(value, spk, binding, label, Some(o))
    }
    fn genesis(&mut self, o: OObj, value: u64, auth: usize, label: &str) -> usize {
        let i = self.obj(o, value, None, label);
        self.outs[i].genesis = Some(auth);
        i
    }
    fn plain(&mut self, value: i64, key: &[u8; 32], label: &str) -> usize {
        self.out(value.max(DUST_OUTPUT_MIN as i64) as u64, p2pk_spk(key), None, label, None)
    }
    /// A payout at the reverse slot `M - 1 - i` of input 0 (DESIGN 4.4): the last output, which must be at least the
    /// input count; 1 KAS fillers to `filler_to` pad the outputs until it is.
    fn last(&mut self, value: i64, key: &[u8; 32], label: &str, filler_to: &[u8; 32]) -> usize {
        while self.outs.len() < self.ins.len() {
            self.plain(100_000_000, filler_to, "filler");
        }
        self.plain(value, key, label)
    }
    /// Adds the funder's inputs and the change output (which absorbs the fee).
    fn fund(&mut self, funder: &[KeyUtxo], change_to: &[u8; 32]) -> usize {
        for u in funder {
            self.key(u, "funding");
        }
        let c = self.out(0, p2pk_spk(change_to), None, "change", None);
        self.change = Some(c);
        c
    }

    pub fn finish(self) -> Result<OBuilt> {
        let mut outs: Vec<TransactionOutput> =
            self.outs.iter().map(|o| TransactionOutput { value: o.value, script_public_key: o.spk.clone(), covenant: o.binding.map(|(a, c)| CovenantBinding { authorizing_input: a, covenant_id: c }) }).collect();
        let mut created = vec![];
        for (i, o) in self.outs.iter().enumerate() {
            if let Some(a) = o.genesis {
                let id = covenant_id(self.ins[a].op, std::iter::once((i as u32, &outs[i])));
                outs[i].covenant = Some(CovenantBinding { authorizing_input: a as u16, covenant_id: id });
            }
        }
        for (i, o) in self.outs.iter().enumerate() {
            if let Some(obj) = &o.obj {
                created.push((i, obj.clone(), outs[i].covenant.map(|c| c.covenant_id).unwrap_or_default()));
            }
        }
        let inputs = self.ins.iter().map(|i| TransactionInput::new_with_compute_budget(i.op, vec![], i.seq, 0)).collect();
        let tx = Transaction::new(TX_VERSION, inputs, outs, self.lock, SUBNETWORK_ID_NATIVE, 0, vec![]);
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
        Ok(b)
    }
}

impl OBuilt {
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
            if have - out == fee as u128 {
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
        let total_in: u64 = b.entries.iter().map(|e| e.amount).sum();
        let total_out: u64 = tx.outputs.iter().map(|o| o.value).sum();
        let units = crate::verify::run_inputs(&tx, &b.entries, crate::verify::Meter::Unlimited).into_iter().map(|r| r.unwrap_or(0)).collect();
        Ok(OSigned { fee: total_in - total_out, tx, entries: b.entries, mass, units, input_labels: b.input_labels, output_labels: b.output_labels, created: b.created })
    }
}

// ---------------------------------------------------------------- arguments

fn iv(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}
fn bv(b: &[u8]) -> Arg {
    Arg::V(ArtifactValue::Bytes(b.to_vec()))
}
fn b8(v: i64) -> Arg {
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

fn refusal(e: Refusal) -> Error {
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
        tag_l: long.idb(),
        tag_s: short.idb(),
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
    d.contract(long, OWhich::Open, lo.redeem(k), "fill", fill(so.owner, short.idb()), "long order", 0);
    d.contract(short, OWhich::Open, so.redeem(k), "fill", fill(lo.owner, long.idb()), "short order", 0);
    d.contract(slot, OWhich::Bond, k.bond.redeem(&slot.s.state_bytes()), "bind", bind_args(&a, &slot.s.owner), "slot", m.bond.rate as u64);
    for (i, o) in [(0usize, long), (1, short)] {
        let spend = o.s.fill_spend(t.n, t.premium) as u64;
        let left = o.s.size_left - t.n;
        if left < 0 || o.value < spend {
            return invalid("an order cannot fund this fill");
        }
        if left > 0 {
            d.obj(OObj::Order(OOpen { size_left: left, ..o.s.clone() }), o.value - spend, o.id.map(|id| (i as u16, id)), "order rest");
        } else {
            d.plain((o.value - spend) as i64, &o.s.owner, "order refund");
        }
    }
    d.genesis(OObj::Pair(p.clone()), p.floor() as u64, 3, "pair");
    d.obj(OObj::Slot(b2), slot.value, slot.id.map(|id| (2, id)), "slot");
    d.plain(q(t.n, t.premium, m.p.scale, 0), &slot.s.owner, "premium");
    d.fund(funder, change_to);
    if funder.is_empty() {
        return invalid("an open needs a funding input (it authors the pair's genesis)");
    }
    Ok((d.finish()?, p))
}

// ---------------------------------------------------------------- claims

/// A claim to propose.
#[derive(Clone, Copy, Debug)]
pub struct ClaimTerms {
    pub kind: i64,
    pub side: i64,
    pub n: i64,
    pub price: i64,
    pub t0: i64,
}

/// A proposal: in 0 the pair (propose), 1 the underwriter's vouching input (`vouch`, its own P2PK UTXO, which also
/// funds the bond unless the closer's funding does), then the funding; out 0 the pending part, 1 the remainder (n < s),
/// then change. A close carries the closer's signature (`KeySet` must hold it).
pub fn propose(pair: &OLive<OPair>, c: &ClaimTerms, vouch: &KeyUtxo, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, OPending)> {
    let k = pinned::kit();
    let p = &pair.s;
    if vouch.key != p.b.uw_key {
        return invalid("only the pair's underwriter can vouch a price for it");
    }
    let id = pair.id.ok_or_else(|| Error::Invalid("the pair has no covenant id".into()))?;
    let (w, fb) = p.pending_of(k, c.n, c.kind, c.side, c.price, c.t0).ok_or_else(|| Error::Invalid("the claim cannot be proposed (no trigger within g, one owner, or another kind)".into()))?;
    let sig = if c.kind == KIND_CLOSE { Arg::Sig(if c.side == LONG { p.long_key } else { p.short_key }) } else { bv(&[0u8; 65]) };
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.lock(c.t0.max(0) as u64);
    d.contract(pair, OWhich::Pair, p.redeem(k), "propose", vec![iv(c.n), iv(c.kind), iv(c.side), iv(c.price), iv(c.t0), iv(1), sig, iv(fb), iv(0), bv(&k.pending.prefix), bv(&k.pending.suffix)], "pair", 0);
    d.key(vouch, "the underwriter's vouch");
    d.obj(OObj::Pending(w.clone()), p.pending_value(pair.value, &w) as u64, Some((0, id)), "pending part");
    if c.n < p.size {
        d.obj(OObj::Pair(p.rest_of(p.size - c.n)), p.rest_value(pair.value, c.n) as u64, Some((0, id)), "remainder");
    }
    d.fund(funder, change_to);
    Ok((d.finish()?, w))
}

/// A contest: in 0 the pending part, 1 the opener's free stake (its owner signs), then the funding (which also funds
/// the dispute's base `base`); out 0 the contested part, 1 the opener's receipt, 2 the dispute, then change.
pub fn contest(pending: &OLive<OPending>, opener: &OLive<Js>, base: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, OPending, Js)> {
    let k = pinned::kit();
    let w = &pending.s;
    if w.status != PENDING {
        return invalid("only a pending claim can be contested");
    }
    let id = pending.id.ok_or_else(|| Error::Invalid("the part has no covenant id".into()))?;
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
/// dispute, 1..=k the receipts, then change.
pub fn deposit(dispute: &OLive<Js>, side: i64, stakes: &[OLive<Js>], funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Js)> {
    let k = pinned::kit();
    let d0 = &dispute.s;
    let id = dispute.id.ok_or_else(|| Error::Invalid("the dispute has no covenant id".into()))?;
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
/// the short last; (forfeit) the underwriter at 0, the forfeited part at 1, change; (restore) the defender at 0, the
/// restored part at 1, change, the disputer last.
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
                d.plain(*long, &w.long_key, "long");
            }
            if *short > 0 {
                last = Some((*short, w.short_key, "short"));
            }
        }
        Settlement::Forfeit { part, value, uw } => {
            d.plain(*uw, &w.b.uw_key, "underwriter: bond and fee");
            d.obj(OObj::Pair(part.clone()), *value as u64, Some((0, id)), "forfeited part");
        }
        Settlement::Restore { part, value, defender, disputer } => {
            if *defender > 0 {
                d.plain(*defender, &w.defender(), "defender: the bond");
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
    d.plain(w.refund, &w.b.uw_key, "underwriter: refund");
    d.obj(OObj::Pair(c.clone()), v as u64, Some((0, id)), "cured part");
    d.fund(funder, change_to);
    Ok((d.finish()?, c))
}

// ---------------------------------------------------------------- the pair's own entries

/// A renewal of up to 7 pairs of one slot in one bind at lock time `lock`: in 0..k the pairs, k the slot, then the
/// funding; out 0..k the renewed pairs, k the slot, k+1 the premiums to the underwriter, then change.
pub fn renew(pairs: &[OLive<OPair>], slot: &OLive<Bond>, lock: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Vec<OPair>)> {
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
    d.plain(prem, &slot.s.owner, "premium");
    d.fund(funder, change_to);
    Ok((d.finish()?, renewed))
}

/// A lapse settle (anyone): in 0 the pair, then the funding; out 0 the long, change, the short last.
pub fn lapse_settle(pair: &OLive<OPair>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let p = &pair.s;
    let (l, s) = p.lapse_out(pair.value);
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.lock((p.b.expiry + p.m.bond.tail).max(0) as u64);
    d.contract(pair, OWhich::Pair, p.redeem(k), "lapseSettle", vec![], "pair", p.m.p.idle_daa as u64);
    if l > 0 {
        d.plain(l, &p.long_key, "long");
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
    let mut c = OPair { tag_l: [0; 32], tag_s: [0; 32], ..p.clone() };
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

/// A whole transfer of a listed `side` at `x` to a signing taker `entrant` posting bound `nb` (the taker funds it):
/// out 0 the leaver, 1 the new pair, then change.
pub fn transfer(pair: &OLive<OPair>, side: i64, x: i64, nb: i64, entrant: [u8; 32], funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, OPair)> {
    let k = pinned::kit();
    let p = &pair.s;
    let id = pair.id.ok_or_else(|| Error::Invalid("the pair has no covenant id".into()))?;
    if entrant == p.b.uw_key {
        return invalid("the underwriter cannot be a party");
    }
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
    let keep = pair.value as i64 - p.range_of(p.size);
    let mut d = ODraft::new(rate, FeeMode::Relay);
    d.contract(pair, OWhich::Pair, p.redeem(k), "transfer", vec![iv(side), iv(p.size), iv(x), iv(nb), bv(&entrant), bv(&[0; 32]), iv(0)], "pair", p.m.p.quiet_daa as u64);
    if pay >= PLAIN_DUST {
        d.plain(pay, &leaver, "leaver");
    }
    d.obj(OObj::Pair(part.clone()), (part.range_of(p.size) + keep + if pay < PLAIN_DUST { pay } else { 0 }) as u64, Some((0, id)), "pair");
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
        d.plain(lp, &p.long_key, "long");
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
        d.out(b as u64, crate::lock::burn_spk(), None, "burn", None);
    }
    d.finish()
}
