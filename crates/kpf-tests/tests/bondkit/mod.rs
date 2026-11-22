//! Fixtures and transaction builders for `KpfBond` and its stand-in pair and order (`contracts/legacy/KpfBond.sil`,
//! `contracts/legacy/stubs/`), on the model types of `kpf_protocol::bond`. The contracts are compiled from source once per
//! test binary (they are not pinned artifacts in this slice).
#![allow(dead_code)]

use std::sync::OnceLock;

use kaspa_consensus_core::tx::{CovenantBinding, Transaction, TransactionOutpoint, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::artifacts::{splice, Template};
use kpf_protocol::bond::*;
use kpf_protocol::state::sm8;
use secp256k1::Keypair;
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

use crate::common::*;

pub struct Kit {
    pub bond: SilAbiArtifact,
    pub bond_t: Template,
    pub pair: SilAbiArtifact,
    pub pair_t: Template,
    pub open: SilAbiArtifact,
    pub open_t: Template,
    /// The dispatch tag of `KpfBond.bind` (a build constant of the stand-ins).
    pub tag: [u8; 4],
}

/// The engine fixtures' market: small DAA numbers (a step of 100, a term of 8 steps, a tail of 2 steps: the ring's
/// bound `term + tail + 2 step <= 12 steps` exactly), `a` = 10, `U` = 50, rho 1.
pub fn params() -> BondParams {
    BondParams { rho_bps: 10_000, step: 100, term: 800, tail: 200, rate: 10, unbond: 50 }
}

pub fn market() -> BondMarket {
    BondMarket { id: [0x4d; 32], p: params() }
}

pub fn market_with(p: BondParams) -> BondMarket {
    BondMarket { id: [0x4d; 32], p }
}

pub fn xkey(kp: &Keypair) -> [u8; 32] {
    kp.x_only_public_key().0.serialize()
}

pub fn kit() -> &'static Kit {
    static K: OnceLock<Kit> = OnceLock::new();
    K.get_or_init(|| {
        let sample = Bond::new(xkey(&keypair(31)), market(), 0);
        let bond = compile("legacy/KpfBond.sil", &sample.ctor());
        let bond_t = Template::of("KpfBond", &bond);
        let entry = &bond.contracts.values().next().unwrap().entries["bind"];
        let tag: [u8; 4] = *entry.dispatch_tag.as_bytes();
        let consts = vec![
            ArtifactValue::Bytes(bond_t.hash.to_vec()),
            int(bond_t.prefix.len() as i64),
            int(bond_t.suffix.len() as i64),
            ArtifactValue::Bytes(tag.to_vec()),
        ];
        let sp = StubPair { m: market(), uw_id: [1; 32], uw_key: [2; 32], owner: [3; 32], cover: 1, expiry: 800 };
        let pair = compile("legacy/stubs/KpfPairStub.sil", &[consts.clone(), sp.ctor_state()].concat());
        let pair_t = Template::of("KpfPairStub", &pair);
        let so = StubOpen { m: market(), uw_accept: [0; 32], owner: [3; 32], cover: 1 };
        let mut oc = vec![ArtifactValue::Bytes(pair_t.hash.to_vec())];
        oc.extend(consts);
        let open = compile("legacy/stubs/KpfOpenStub.sil", &[oc, so.ctor_state()].concat());
        let open_t = Template::of("KpfOpenStub", &open);
        Kit { bond, bond_t, pair, pair_t, open, open_t, tag }
    })
}

pub fn bond_art(b: &Bond) -> SilAbiArtifact {
    splice(&kit().bond, &b.state_bytes())
}
pub fn pair_art(p: &StubPair) -> SilAbiArtifact {
    splice(&kit().pair, &p.state_bytes())
}
pub fn open_art(o: &StubOpen) -> SilAbiArtifact {
    splice(&kit().open, &o.state_bytes())
}

/// An 8-byte argument (`byte[8]`), as the ABI pushes it.
pub fn b8(v: i64) -> Arg {
    Arg::V(ArtifactValue::Bytes(sm8(v).to_vec()))
}

pub fn iv(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}

pub fn bind_args(a: &BindArgs, kp: &Keypair) -> Vec<Arg> {
    let mut v: Vec<Arg> = a.arg_bytes().into_iter().map(|b| Arg::V(ArtifactValue::Bytes(b.to_vec()))).collect();
    v.push(Arg::Sig(*kp));
    v
}

pub fn split_args(a: &[i64; RING], kp: &Keypair) -> Vec<Arg> {
    let mut v: Vec<Arg> = a.iter().map(|x| b8(*x)).collect();
    v.push(Arg::Sig(*kp));
    v
}

/// A slot UTXO spent on `entry(args)`.
pub fn bond_in(op: TransactionOutpoint, value: u64, id: Option<Hash>, daa: u64, b: &Bond, entry: &'static str, args: Vec<Arg>) -> In {
    let art = bond_art(b);
    In { outpoint: op, utxo: UtxoEntry::new(value, p2sh(&art), daa, false, id), spend: Spend::Signed { art, entry, args } }
}

pub fn pair_in(op: TransactionOutpoint, value: u64, id: Option<Hash>, daa: u64, p: &StubPair, entry: &'static str, args: Vec<Arg>) -> In {
    let art = pair_art(p);
    In { outpoint: op, utxo: UtxoEntry::new(value, p2sh(&art), daa, false, id), spend: Spend::Signed { art, entry, args } }
}

pub fn open_in(op: TransactionOutpoint, value: u64, id: Option<Hash>, daa: u64, o: &StubOpen, pair_out: usize, u: usize, k: i64, x: i64) -> In {
    let art = open_art(o);
    let t = &kit().pair_t;
    let args = vec![
        iv(pair_out as i64),
        iv(u as i64),
        iv(k),
        iv(x),
        Arg::V(ArtifactValue::Bytes(t.prefix.clone())),
        Arg::V(ArtifactValue::Bytes(t.suffix.clone())),
    ];
    In { outpoint: op, utxo: UtxoEntry::new(value, p2sh(&art), daa, false, id), spend: Spend::Signed { art, entry: "fill", args } }
}

pub fn bond_out(value: u64, b: &Bond, cov: Option<CovenantBinding>) -> TransactionOutput {
    out(value, p2sh(&bond_art(b)), cov)
}
pub fn pair_out(value: u64, p: &StubPair, cov: Option<CovenantBinding>) -> TransactionOutput {
    out(value, p2sh(&pair_art(p)), cov)
}

pub fn h(b: [u8; 32]) -> Hash {
    Hash::from_bytes(b)
}

/// Runs a transaction and reports, per input, whether it passed (the covenant context must accept the bindings).
pub fn passes(tx: &Transaction, e: &[UtxoEntry]) -> Vec<bool> {
    run(tx, e).unwrap_or_else(|err| panic!("covenant context refused the transaction: {err:?}")).iter().map(|r| r.is_ok()).collect()
}

/// Every input passes (and the bindings are valid).
pub fn ok(tx: &Transaction, e: &[UtxoEntry]) -> bool {
    match run(tx, e) {
        Ok(v) => v.iter().all(|r| r.is_ok()),
        Err(_) => false,
    }
}

/// A plain funding input of key 60 (the executor's), worth 10 KAS.
pub fn funder(op: u8) -> In {
    p2pk_in(outpoint(op), 1_000_000_000, None, &keypair(60))
}

pub fn change(v: u64) -> TransactionOutput {
    out(v, p2pk_spk(&keypair(60)), None)
}

// ---------------------------------------------------------------- live UTXOs and transaction shapes

/// A slot UTXO.
#[derive(Clone, Debug)]
pub struct LSlot {
    pub op: TransactionOutpoint,
    pub value: u64,
    pub daa: u64,
    pub id: Option<Hash>,
    pub b: Bond,
}

/// A stand-in pair UTXO.
#[derive(Clone, Debug)]
pub struct LPair {
    pub op: TransactionOutpoint,
    pub value: u64,
    pub daa: u64,
    pub id: Hash,
    pub p: StubPair,
}

impl LSlot {
    pub fn ctx(&self, lock: u64, seq: u64) -> Ctx {
        Ctx { lock: lock as i64, seq: seq as i64, v_in: self.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: self.id.is_some() }
    }
    pub fn id_bytes(&self) -> [u8; 32] {
        self.id.map(|h| h.as_bytes()).unwrap_or([0; 32])
    }
}

/// A bind by the slot alone: in 0 the slot, 1 the funder; out 0 the continuation `b2` of value `v_out`, 1 change.
pub fn bind_alone(s: &LSlot, a: &BindArgs, b2: &Bond, v_out: u64, lock: u64, seq: u64, signer: &Keypair) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [bond_in(s.op, s.value, s.id, s.daa, &s.b, "bind", bind_args(a, signer)), funder(200)];
    let outs = vec![bond_out(v_out, b2, s.id.and_then(|id| bind(0, id))), change(900_000_000)];
    build_ext(&ins, outs, lock, &[seq, 0])
}

/// A renewal of `pairs` (each at input i, renewing with claim `ks[i]`) by the slot at input `pairs.len()`, then the
/// funder; out: the pairs' continuations `p2s`, the slot's `b2`, change.
#[allow(clippy::too_many_arguments)]
pub fn renew_tx(pairs: &[LPair], ks: &[i64], p2s: &[StubPair], s: &LSlot, a: &BindArgs, b2: &Bond, lock: u64, seq: u64, signer: &Keypair) -> (Transaction, Vec<UtxoEntry>) {
    let u = pairs.len();
    let mut ins: Vec<In> = pairs.iter().zip(ks).map(|(p, k)| pair_in(p.op, p.value, Some(p.id), p.daa, &p.p, "renew", vec![iv(u as i64), iv(*k)])).collect();
    ins.push(bond_in(s.op, s.value, s.id, s.daa, &s.b, "bind", bind_args(a, signer)));
    ins.push(funder(201));
    let mut outs: Vec<TransactionOutput> = pairs.iter().zip(p2s).enumerate().map(|(i, (p, p2))| pair_out(p.value, p2, bind(i as u16, p.id))).collect();
    outs.push(bond_out(s.value, b2, s.id.and_then(|id| bind(u as u16, id))));
    outs.push(change(900_000_000));
    let mut seqs = vec![0u64; u];
    seqs.push(seq);
    seqs.push(0);
    build_ext(&ins, outs, lock, &seqs)
}

/// An open: in 0 the order, 1 the slot (bind), 2 the funder; out 0 the slot's `b2`, 1 the pair (a genesis authored by
/// the funder, with `twins` more outputs in its group), 2 change, then `extra` plain outputs.
#[allow(clippy::too_many_arguments)]
pub fn open_tx(o: &StubOpen, order_id: Hash, k: i64, s: &LSlot, a: &BindArgs, b2: &Bond, p: &StubPair, lock: u64, seq: u64, twins: usize, extra: usize) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [open_in(outpoint(150), 200_000_000, Some(order_id), 0, o, 1, 1, k, p.expiry), bond_in(s.op, s.value, s.id, s.daa, &s.b, "bind", bind_args(a, &keypair(31))), funder(202)];
    let mut outs: Vec<TransactionOutput> = vec![bond_out(s.value, b2, s.id.and_then(|id| bind(1, id))), pair_out(200_000_000, p, None), change(800_000_000)];
    for _ in 0..twins {
        outs.push(pair_out(1_000_000, p, None));
    }
    for _ in 0..extra {
        outs.push(out(1_000_000, p2pk_spk(&keypair(61)), None));
    }
    let mut group = vec![1usize];
    group.extend(3..3 + twins);
    let g = genesis_id(outpoint(202), &outs, &group);
    for i in group {
        outs[i].covenant = bind(2, g);
    }
    build_ext(&ins, outs, lock, &[0, seq, 0])
}

/// An advance by anyone (no signature): in 0 the slot, 1 the funder; out 0 `b2`, 1 change.
pub fn advance_tx(s: &LSlot, d: i64, b2: &Bond, v_out: u64, lock: u64) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [bond_in(s.op, s.value, s.id, s.daa, &s.b, "advance", vec![b8(d)]), funder(203)];
    build_ext(&ins, vec![bond_out(v_out, b2, s.id.and_then(|id| bind(0, id))), change(900_000_000)], lock, &[0, 0])
}

/// A split: in 0 the slot, 1 the funder; out 0 child A, 1 child B, 2 change.
#[allow(clippy::too_many_arguments)]
pub fn split_tx(s: &LSlot, a: &[i64; RING], ba: &Bond, va: u64, bb: &Bond, vb: u64, lock: u64, seq: u64, signer: &Keypair) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [bond_in(s.op, s.value, s.id, s.daa, &s.b, "split", split_args(a, signer)), funder(204)];
    let id = s.id;
    let outs = vec![bond_out(va, ba, id.and_then(|i| bind(0, i))), bond_out(vb, bb, id.and_then(|i| bind(0, i))), change(900_000_000)];
    build_ext(&ins, outs, lock, &[seq, 0])
}

/// An unbond: in 0 the slot, 1 the funder; out 0 `b2`, 1 change.
pub fn unbond_tx(s: &LSlot, d: i64, b2: &Bond, lock: u64, seq: u64, signer: &Keypair) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [bond_in(s.op, s.value, s.id, s.daa, &s.b, "unbond", vec![b8(d), Arg::Sig(*signer)]), funder(205)];
    build_ext(&ins, vec![bond_out(s.value, b2, s.id.and_then(|id| bind(0, id))), change(900_000_000)], lock, &[seq, 0])
}

/// A withdraw: in 0 the slot, 1 the funder; out 0 the owner's payout (and `keep` an output of the id).
pub fn withdraw_tx(s: &LSlot, seq: u64, signer: &Keypair, keep: bool) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [bond_in(s.op, s.value, s.id, s.daa, &s.b, "withdraw", vec![Arg::Sig(*signer)]), funder(206)];
    let mut outs = vec![out(s.value, p2pk_spk(&keypair(31)), None), change(900_000_000)];
    if keep {
        outs[0] = bond_out(s.value, &s.b, s.id.and_then(|id| bind(0, id)));
    }
    build_ext(&ins, outs, 0, &[seq, 0])
}

/// A rely of the pair at DAA `daa` (its own UTXO's score).
pub fn rely_tx(p: &LPair) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [pair_in(p.op, p.value, Some(p.id), p.daa, &p.p, "rely", vec![]), funder(207)];
    build_ext(&ins, vec![pair_out(p.value, &p.p, bind(0, p.id)), change(900_000_000)], 0, &[0, 0])
}

/// The model's verdict and the engine's agree; returns the verdict.
pub fn agree<T: std::fmt::Debug>(what: &str, tx: &(Transaction, Vec<UtxoEntry>), model: &Result<T, Refusal>) -> bool {
    let engine = ok(&tx.0, &tx.1);
    assert_eq!(engine, model.is_ok(), "{what}: engine {} but model {:?} (inputs {:?})", engine, model, run(&tx.0, &tx.1).map(|v| v.iter().map(|r| r.is_ok()).collect::<Vec<_>>()));
    engine
}

/// A minimal UTXO set: enough to show that two transactions spending one slot conflict.
#[derive(Default)]
pub struct Utxos {
    pub set: std::collections::HashMap<TransactionOutpoint, UtxoEntry>,
}

impl Utxos {
    pub fn add(&mut self, op: TransactionOutpoint, e: UtxoEntry) {
        self.set.insert(op, e);
    }
    /// Applies an accepted transaction at DAA `daa`: every input must be unspent.
    pub fn apply(&mut self, tx: &Transaction, daa: u64) -> Result<(), String> {
        for i in &tx.inputs {
            if !self.set.contains_key(&i.previous_outpoint) {
                return Err(format!("input {} is spent or unknown", i.previous_outpoint));
            }
        }
        for i in &tx.inputs {
            self.set.remove(&i.previous_outpoint);
        }
        for (k, o) in tx.outputs.iter().enumerate() {
            self.set.insert(
                TransactionOutpoint { transaction_id: tx.id(), index: k as u32 },
                UtxoEntry::new(o.value, o.script_public_key.clone(), daa, false, o.covenant.map(|c| c.covenant_id)),
            );
        }
        Ok(())
    }
}
