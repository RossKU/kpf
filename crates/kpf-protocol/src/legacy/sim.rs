//! An in-memory UTXO set with the script engine as the chain: the harness of the builder tests and of the command-line
//! integration tests. Nothing here talks to a network.
//!
//! [`Chain::apply`] accepts a transaction the way a node would for the rules that matter to KPF: every input must
//! exist, the lock time must not be in the future, a relative lock (CSV sequence) must have matured, and
//! [`crate::verify::verify`] must pass (scripts with enforced budgets, covenant bindings, mass, fee floor).

use std::collections::HashMap;

use kaspa_consensus_core::tx::{Transaction, TransactionId, TransactionOutpoint, UtxoEntry};
use kaspa_consensus_core::Hash;

use crate::error::{invalid, Error, Result};
use crate::state::{Open, Pair};
use crate::tx::{p2pk_key, KeyUtxo, LiveLock, LiveOpen, LivePair, LivePending, Obj, Signed};
use crate::verify::verify;

#[derive(Default)]
pub struct Chain {
    /// The current DAA score.
    pub daa: u64,
    utxos: HashMap<TransactionOutpoint, UtxoEntry>,
    objs: HashMap<TransactionOutpoint, Obj>,
    mints: u32,
}

impl Chain {
    pub fn new() -> Chain {
        Chain::default()
    }

    pub fn advance(&mut self, daa: u64) {
        self.daa += daa;
    }

    /// Creates a plain P2PK UTXO of `amount` for `key` (a faucet).
    pub fn mint(&mut self, key: &[u8; 32], amount: u64) -> KeyUtxo {
        self.mints += 1;
        let op = TransactionOutpoint { transaction_id: {
            let mut b = [0xf0u8; 32];
            b[..4].copy_from_slice(&self.mints.to_le_bytes());
            TransactionId::from_bytes(b)
        }, index: self.mints };
        let u = KeyUtxo { outpoint: op, amount, daa: self.daa, key: *key };
        self.utxos.insert(op, u.entry());
        u
    }

    /// Puts a UTXO into the set as if an earlier transaction had created it (a KOB order or a token holder that the
    /// tests and the offline runbook place by hand).
    pub fn insert(&mut self, op: TransactionOutpoint, entry: UtxoEntry) {
        self.utxos.insert(op, entry);
    }

    /// Creates a KOB object UTXO (an order or a holder) of `value` sompi, covenant id `id`, created at DAA `daa`.
    pub fn mint_kob(&mut self, obj: crate::kob::KobObj, value: u64, id: Hash, daa: u64) -> KobLive {
        self.mints += 1;
        let mut b = [0xf1u8; 32];
        b[..4].copy_from_slice(&self.mints.to_le_bytes());
        let op = TransactionOutpoint { transaction_id: TransactionId::from_bytes(b), index: self.mints };
        let spk = crate::tx::p2sh_spk(&obj.redeem());
        self.utxos.insert(op, UtxoEntry::new(value, spk, daa, false, Some(id)));
        self.objs.insert(op, Obj::Kob(obj.clone()));
        KobLive { obj, value, id, op, daa }
    }

    /// Creates a KPF object UTXO (a pair, an order, a KOB order or holder) of `value` sompi with covenant id `id`,
    /// created at DAA `daa`, and returns it live.
    pub fn mint_obj(&mut self, obj: Obj, value: u64, id: Hash, daa: u64) -> Live {
        self.mints += 1;
        let mut b = [0xf2u8; 32];
        b[..4].copy_from_slice(&self.mints.to_le_bytes());
        let op = TransactionOutpoint { transaction_id: TransactionId::from_bytes(b), index: self.mints };
        self.utxos.insert(op, UtxoEntry::new(value, crate::tx::p2sh_spk(&obj.redeem()), daa, false, Some(id)));
        self.objs.insert(op, obj.clone());
        match obj {
            Obj::Pair(p) => Live::Pair(LivePair { p, value, id, op, daa }),
            Obj::Open(o) => Live::Open(LiveOpen { o, value, id, op, daa }),
            Obj::Lock(l) => Live::Lock(LiveLock { l, value, op, daa }),
            Obj::Pending(w) => Live::Pending(LivePending { w, value, id, op, daa }),
            Obj::Kob(k) => Live::Kob(KobLive { obj: k, value, id, op, daa }),
        }
    }

    pub fn entry(&self, op: &TransactionOutpoint) -> Option<&UtxoEntry> {
        self.utxos.get(op)
    }

    pub fn len(&self) -> usize {
        self.utxos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.utxos.is_empty()
    }

    /// All plain UTXOs of `key`.
    pub fn key_utxos(&self, key: &[u8; 32]) -> Vec<KeyUtxo> {
        let mut v: Vec<KeyUtxo> = self
            .utxos
            .iter()
            .filter(|(_, e)| p2pk_key(&e.script_public_key).as_ref() == Some(key) && e.covenant_id.is_none())
            .map(|(op, e)| KeyUtxo { outpoint: *op, amount: e.amount, daa: e.block_daa_score, key: *key })
            .collect();
        v.sort_by_key(|u| (u.outpoint.index, u.outpoint.transaction_id.as_bytes()));
        v
    }

    /// Total of the plain UTXOs of `key`.
    pub fn balance(&self, key: &[u8; 32]) -> u64 {
        self.key_utxos(key).iter().map(|u| u.amount).sum()
    }

    /// Teaches the chain the object behind a KPF output (what an indexer would know).
    pub fn register(&mut self, op: TransactionOutpoint, obj: Obj) {
        self.objs.insert(op, obj);
    }

    pub fn live_pair(&self, op: &TransactionOutpoint) -> Result<LivePair> {
        let e = self.utxos.get(op).ok_or_else(|| Error::Invalid(format!("no UTXO {}:{}", op.transaction_id, op.index)))?;
        match self.objs.get(op) {
            Some(Obj::Pair(p)) => Ok(LivePair { p: p.clone(), value: e.amount, id: e.covenant_id.unwrap_or_default(), op: *op, daa: e.block_daa_score }),
            _ => invalid("not a known pair"),
        }
    }

    pub fn live_pending(&self, op: &TransactionOutpoint) -> Result<LivePending> {
        let e = self.utxos.get(op).ok_or_else(|| Error::Invalid(format!("no UTXO {}:{}", op.transaction_id, op.index)))?;
        match self.objs.get(op) {
            Some(Obj::Pending(w)) => Ok(LivePending { w: w.clone(), value: e.amount, id: e.covenant_id.unwrap_or_default(), op: *op, daa: e.block_daa_score }),
            _ => invalid("not a known pending part"),
        }
    }

    /// Every live object of the chain (what an indexer of KPF outputs would list).
    pub fn objects(&self) -> Vec<Live> {
        let mut v: Vec<(TransactionOutpoint, Live)> = self
            .objs
            .iter()
            .filter_map(|(op, obj)| {
                let e = self.utxos.get(op)?;
                let (value, id, daa) = (e.amount, e.covenant_id.unwrap_or_default(), e.block_daa_score);
                Some((
                    *op,
                    match obj.clone() {
                        Obj::Pair(p) => Live::Pair(LivePair { p, value, id, op: *op, daa }),
                        Obj::Open(o) => Live::Open(LiveOpen { o, value, id, op: *op, daa }),
                        Obj::Pending(w) => Live::Pending(LivePending { w, value, id, op: *op, daa }),
                        Obj::Lock(l) => Live::Lock(LiveLock { l, value, op: *op, daa }),
                        Obj::Kob(k) => Live::Kob(KobLive { obj: k, value, id, op: *op, daa }),
                    },
                ))
            })
            .collect();
        v.sort_by_key(|(op, _)| (op.transaction_id, op.index));
        v.into_iter().map(|(_, l)| l).collect()
    }

    pub fn live_open(&self, op: &TransactionOutpoint) -> Result<LiveOpen> {
        let e = self.utxos.get(op).ok_or_else(|| Error::Invalid(format!("no UTXO {}:{}", op.transaction_id, op.index)))?;
        match self.objs.get(op) {
            Some(Obj::Open(o)) => Ok(LiveOpen { o: o.clone(), value: e.amount, id: e.covenant_id.unwrap_or_default(), op: *op, daa: e.block_daa_score }),
            _ => invalid("not a known order"),
        }
    }

    /// The UTXO entries of a transaction's inputs, or the first missing outpoint.
    pub fn entries_of(&self, tx: &Transaction) -> Result<Vec<UtxoEntry>> {
        tx.inputs
            .iter()
            .map(|i| {
                self.utxos.get(&i.previous_outpoint).cloned().ok_or_else(|| {
                    Error::Invalid(format!("input {}:{} is not in the UTXO set", i.previous_outpoint.transaction_id, i.previous_outpoint.index))
                })
            })
            .collect()
    }

    /// Accepts `tx`: checks time rules and runs [`verify`]; on success spends its inputs and creates its outputs at the
    /// current DAA, registering `created` objects (output index, object). Returns the txid.
    pub fn apply(&mut self, tx: &Transaction, created: &[(usize, Obj)]) -> Result<TransactionId> {
        let entries = self.entries_of(tx)?;
        if tx.lock_time > self.daa {
            return invalid(format!("lock time {} is in the future (DAA {})", tx.lock_time, self.daa));
        }
        for (i, (inp, e)) in tx.inputs.iter().zip(&entries).enumerate() {
            if inp.sequence != 0 && self.daa.saturating_sub(e.block_daa_score) < inp.sequence {
                return invalid(format!("input {i}: relative lock {} has not matured (age {})", inp.sequence, self.daa.saturating_sub(e.block_daa_score)));
            }
        }
        let v = verify(tx, &entries);
        if !v.is_ok() {
            return Err(Error::Engine(v.errors().join("; ")));
        }
        let id = tx.id();
        for i in &tx.inputs {
            self.utxos.remove(&i.previous_outpoint);
            self.objs.remove(&i.previous_outpoint);
        }
        for (k, o) in tx.outputs.iter().enumerate() {
            let op = TransactionOutpoint { transaction_id: id, index: k as u32 };
            self.utxos.insert(op, UtxoEntry::new(o.value, o.script_public_key.clone(), self.daa, false, o.covenant.map(|b| b.covenant_id)));
        }
        for (k, obj) in created {
            self.objs.insert(TransactionOutpoint { transaction_id: id, index: *k as u32 }, obj.clone());
        }
        Ok(id)
    }

    /// [`Chain::apply`] for a signed transaction of the builders; returns the live objects it created.
    pub fn apply_signed(&mut self, s: &Signed) -> Result<Vec<Live>> {
        let created: Vec<(usize, Obj)> = s.created.iter().map(|c| (c.index, c.obj.clone())).collect();
        let id = self.apply(&s.tx, &created)?;
        Ok(s.created
            .iter()
            .map(|c| {
                let op = TransactionOutpoint { transaction_id: id, index: c.index as u32 };
                let e = &self.utxos[&op];
                let (value, cid, daa) = (e.amount, e.covenant_id.unwrap_or_default(), e.block_daa_score);
                match &c.obj {
                    Obj::Pair(p) => Live::Pair(LivePair { p: p.clone(), value, id: cid, op, daa }),
                    Obj::Open(o) => Live::Open(LiveOpen { o: o.clone(), value, id: cid, op, daa }),
                    Obj::Pending(w) => Live::Pending(LivePending { w: w.clone(), value, id: cid, op, daa }),
                    Obj::Lock(l) => Live::Lock(LiveLock { l: l.clone(), value, op, daa }),
                    Obj::Kob(k) => Live::Kob(KobLive { obj: k.clone(), value, id: cid, op, daa }),
                }
            })
            .collect())
    }
}

/// A live KPF object.
#[derive(Clone, Debug)]
pub enum Live {
    Pair(LivePair),
    Open(LiveOpen),
    Pending(LivePending),
    /// A guarantor's lock (DESIGN 4.5f).
    Lock(LiveLock),
    Kob(KobLive),
}

/// A live KOB order or token holder created by an evidence transaction.
#[derive(Clone, Debug)]
pub struct KobLive {
    pub obj: crate::kob::KobObj,
    pub value: u64,
    pub id: Hash,
    pub op: TransactionOutpoint,
    pub daa: u64,
}

impl KobLive {
    pub fn ask(&self) -> crate::kob::LiveAsk {
        match &self.obj {
            crate::kob::KobObj::Ask(a) => crate::kob::LiveAsk { a: a.clone(), value: self.value, id: self.id, op: self.op, daa: self.daa },
            _ => panic!("expected an ask"),
        }
    }
    pub fn bid(&self) -> crate::kob::LiveBid {
        match &self.obj {
            crate::kob::KobObj::Bid(b) => crate::kob::LiveBid { b: b.clone(), value: self.value, id: self.id, op: self.op, daa: self.daa },
            _ => panic!("expected a bid"),
        }
    }
    pub fn token(&self) -> crate::kob::LiveToken {
        match &self.obj {
            crate::kob::KobObj::Token(t) => crate::kob::LiveToken { t: t.clone(), value: self.value, id: self.id, op: self.op, daa: self.daa },
            _ => panic!("expected a token holder"),
        }
    }
}

impl Live {
    pub fn pair(self) -> LivePair {
        match self {
            Live::Pair(p) => p,
            _ => panic!("expected a pair"),
        }
    }
    pub fn open(self) -> LiveOpen {
        match self {
            Live::Open(o) => o,
            _ => panic!("expected an order"),
        }
    }
    pub fn pending(self) -> LivePending {
        match self {
            Live::Pending(w) => w,
            _ => panic!("expected a pending part"),
        }
    }
    pub fn lock(self) -> LiveLock {
        match self {
            Live::Lock(l) => l,
            _ => panic!("expected a lock"),
        }
    }
    pub fn kob(self) -> KobLive {
        match self {
            Live::Kob(k) => k,
            _ => panic!("expected a KOB object"),
        }
    }
}

/// Decodes the pair a spent input revealed, for an indexer-like view of a signed transaction: the redeem script is the
/// last push of the signature script.
pub fn redeem_of_input(sig_script: &[u8]) -> Option<Vec<u8>> {
    // The last push: parse pushes forward.
    let mut i = 0;
    let mut last = None;
    while i < sig_script.len() {
        let op = sig_script[i];
        let (len, hdr) = match op {
            1..=75 => (op as usize, 1),
            76 => (*sig_script.get(i + 1)? as usize, 2),
            77 => (u16::from_le_bytes([*sig_script.get(i + 1)?, *sig_script.get(i + 2)?]) as usize, 3),
            _ => (0, 1),
        };
        let end = i + hdr + len;
        if end > sig_script.len() {
            return None;
        }
        if len > 0 {
            last = Some(sig_script[i + hdr..end].to_vec());
        }
        i = end;
    }
    last
}

/// Decodes the pair spent by input `idx` of a transaction, from its signature script.
pub fn spent_pair(tx: &Transaction, idx: usize) -> Option<Pair> {
    Pair::decode_redeem(&redeem_of_input(&tx.inputs[idx].signature_script)?).ok()
}

/// Decodes the order spent by input `idx` of a transaction, from its signature script.
pub fn spent_open(tx: &Transaction, idx: usize) -> Option<Open> {
    Open::decode_redeem(&redeem_of_input(&tx.inputs[idx].signature_script)?).ok()
}

/// The covenant id of a UTXO of the chain.
pub fn covenant_of(chain: &Chain, op: &TransactionOutpoint) -> Option<Hash> {
    chain.entry(op).and_then(|e| e.covenant_id)
}

/// Decodes the pending part spent by input `idx` of a transaction, from its signature script.
pub fn spent_pending(tx: &Transaction, idx: usize) -> Option<crate::state::Pending> {
    crate::state::Pending::decode_redeem(&redeem_of_input(&tx.inputs[idx].signature_script)?).ok()
}

/// Decodes the lock spent by input `idx` of a transaction, from its signature script (DESIGN 4.5f).
pub fn spent_lock(tx: &Transaction, idx: usize) -> Option<crate::state::Lock> {
    crate::state::Lock::decode_redeem(&redeem_of_input(&tx.inputs[idx].signature_script)?).ok()
}

impl Chain {
    pub fn live_lock(&self, op: &TransactionOutpoint) -> Result<LiveLock> {
        let e = self.utxos.get(op).ok_or_else(|| Error::Invalid(format!("no UTXO {}:{}", op.transaction_id, op.index)))?;
        match self.objs.get(op) {
            Some(Obj::Lock(l)) => Ok(LiveLock { l: l.clone(), value: e.amount, op: *op, daa: e.block_daa_score }),
            _ => invalid("not a known lock"),
        }
    }
}
