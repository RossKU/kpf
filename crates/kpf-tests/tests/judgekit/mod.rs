//! Fixtures for the stake judge (`contracts/KpfStake.sil`, `contracts/stubs/KpfClaimStub.sil`): the compiled templates,
//! the real `Env` of the model, and the builder that turns a model transaction (`kpf_protocol::judge::JTx`) into a
//! rusty-kaspa transaction, so that the model's verdict and the engine's can be compared input by input. The contracts
//! are compiled from source once per test binary (they are not pinned artifacts in this slice).
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

use kaspa_consensus_core::tx::{CovenantBinding, ScriptPublicKey, Transaction, TransactionId, TransactionOutpoint, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::artifacts::{splice, Template};
use kpf_protocol::judge::*;
use kpf_protocol::state::sm8;
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

use crate::common::Spend as Spend_;

use crate::common::{build_ext, compile, int, keypair, p2pk_spk, p2sh, run, Arg, In};

pub struct JKit {
    pub stake: SilAbiArtifact,
    pub stake_t: Template,
    pub claim: SilAbiArtifact,
    pub claim_t: Template,
    pub open_tag: [u8; 4],
    pub resolve_tag: [u8; 4],
}

/// The engine fixtures' judge: rounds of 100 DAA, stake aged 500 before the claim, unbonding 300, mu 1.25, burn 10 %,
/// 10 KAS per stake at least.
pub fn jparams() -> JudgeParams {
    JudgeParams { mu_bps: 12_500, age: 500, unbond: 300, round: 100, burn_bps: 1_000, min_stake: UNIT }
}

pub fn jmarket() -> JudgeMarket {
    JudgeMarket { id: [0x4a; 32], p: jparams() }
}

fn tag(art: &SilAbiArtifact, entry: &str) -> [u8; 4] {
    *art.contracts.values().next().unwrap().entries[entry].dispatch_tag.as_bytes()
}

pub fn jkit() -> &'static JKit {
    static K: OnceLock<JKit> = OnceLock::new();
    K.get_or_init(|| {
        let stake = compile("KpfStake.sil", &Js::free([7; 32], jmarket()).ctor());
        let stake_t = Template::of("KpfStake", &stake);
        let open_tag = tag(&stake, "open");
        let resolve_tag = tag(&stake, "resolve");
        let consts = vec![
            ArtifactValue::Bytes(stake_t.hash.to_vec()),
            int(stake_t.prefix.len() as i64),
            int(stake_t.suffix.len() as i64),
            ArtifactValue::Bytes(open_tag.to_vec()),
            ArtifactValue::Bytes(resolve_tag.to_vec()),
        ];
        let sample = ClaimStub { m: jmarket(), owner: [7; 32], status: CLAIM_PENDING };
        let claim = compile("stubs/KpfClaimStub.sil", &[consts, sample.ctor_state()].concat());
        let claim_t = Template::of("KpfClaimStub", &claim);
        JKit { stake, stake_t, claim, claim_t, open_tag, resolve_tag }
    })
}

pub fn stake_art(s: &Js) -> SilAbiArtifact {
    splice(&jkit().stake, &s.state_bytes())
}
pub fn claim_art(c: &ClaimStub) -> SilAbiArtifact {
    splice(&jkit().claim, &c.state_bytes())
}

/// The script public key of a model script.
pub fn spk_of(s: &Script) -> ScriptPublicKey {
    match s {
        Script::Stake(j) => p2sh(&stake_art(j)),
        Script::Claim(c) => p2sh(&claim_art(c)),
        Script::Key(seed) => p2pk_spk(&keypair(*seed)),
        Script::Burn => kaspa_txscript::pay_to_script_hash_script(&[0x6a]),
    }
}

/// The real environment: blake3 of the versioned script public key bytes, as `blake3(tx.outputs[j].scriptPubKey)`.
pub struct Real;

impl Env for Real {
    fn spk_hash(&self, s: &Script) -> [u8; 32] {
        let spk = spk_of(s);
        let mut b = spk.version().to_be_bytes().to_vec();
        b.extend_from_slice(spk.script());
        *blake3::hash(&b).as_bytes()
    }
}

pub fn b8(v: i64) -> Arg {
    Arg::V(ArtifactValue::Bytes(sm8(v).to_vec()))
}

/// The engine side of a model world: the outpoint of every handle.
#[derive(Default)]
pub struct Eng {
    pub ops: HashMap<u64, TransactionOutpoint>,
}

impl Eng {
    /// The outpoint of handle `h` (a synthetic one for a UTXO that existed before the scenario).
    pub fn op(&self, h: u64) -> TransactionOutpoint {
        self.ops.get(&h).copied().unwrap_or_else(|| {
            let mut b = [0x99u8; 32];
            b[..8].copy_from_slice(&h.to_le_bytes());
            TransactionOutpoint { transaction_id: TransactionId::from_bytes(b), index: (h % 1000) as u32 }
        })
    }

    /// The engine transaction of `t` with its UTXO entries.
    pub fn build(&self, t: &JTx) -> (Transaction, Vec<UtxoEntry>) {
        let ins: Vec<In> = t.ins.iter().map(|i| self.input_of(i)).collect();
        let outs: Vec<TransactionOutput> = t
            .outs
            .iter()
            .map(|o| {
                let cov = o.cov.map(|id| {
                    let a = t.ins.iter().position(|i| i.utxo.cov == Some(id)).unwrap_or(0);
                    CovenantBinding { authorizing_input: a as u16, covenant_id: Hash::from_bytes(id) }
                });
                TransactionOutput { value: o.value as u64, script_public_key: spk_of(&o.script), covenant: cov }
            })
            .collect();
        let seqs: Vec<u64> = t.ins.iter().map(|i| i.seq as u64).collect();
        build_ext(&ins, outs, t.lock as u64, &seqs)
    }

    /// The engine input of a model input (its UTXO entry and signature script plan).
    pub fn input_of(&self, i: &JIn) -> In {
        let daa = if i.daa < 0 { u64::MAX } else { i.daa as u64 };
        let spk = spk_of(&i.utxo.script);
        let utxo = UtxoEntry::new(i.utxo.value as u64, spk, daa, false, i.utxo.cov.map(Hash::from_bytes));
        let sig = Arg::SigType(keypair(i.signer.unwrap_or(254)), i.sighash);
        let spend = match (&i.utxo.script, i.spend) {
            (Script::Key(_), Spend::Key) => Spend_::P2pkType(keypair(i.signer.unwrap_or(254)), i.sighash),
            (Script::Stake(s), sp) => {
                let (entry, args): (&'static str, Vec<Arg>) = match sp {
                    Spend::Open { claim, c_out, d_out } => ("open", vec![b8(claim), b8(c_out), b8(d_out), sig]),
                    Spend::Commit { side, round, anchor } => ("commit", vec![b8(side), b8(round), b8(anchor), sig]),
                    Spend::Unbond => ("unbond", vec![sig]),
                    Spend::Withdraw => ("withdraw", vec![sig]),
                    Spend::Deposit { side, first, k } => ("deposit", vec![b8(side), b8(first), b8(k)]),
                    Spend::Resolve { claim } => ("resolve", vec![b8(claim)]),
                    Spend::Settle { first, k } => ("settle", vec![b8(first), b8(k)]),
                    Spend::Close { j } => ("close", vec![b8(j)]),
                    Spend::Redeem { d } => ("redeem", vec![b8(d)]),
                    other => panic!("no stake entry for {other:?}"),
                };
                Spend_::Signed { art: stake_art(s), entry, args }
            }
            (Script::Claim(c), sp) => {
                let (entry, args): (&'static str, Vec<Arg>) = match sp {
                    Spend::Contest { u, d_out } => ("contest", vec![b8(u), b8(d_out)]),
                    Spend::Decide { d } => ("decide", vec![b8(d)]),
                    Spend::ClaimClose => ("close", vec![sig]),
                    other => panic!("no claim entry for {other:?}"),
                };
                Spend_::Signed { art: claim_art(c), entry, args }
            }
            (s, sp) => panic!("cannot spend {s:?} by {sp:?}"),
        };
        In { outpoint: self.op(i.id), utxo, spend }
    }

    /// The engine's verdict per input (all false when the covenant context refuses the transaction).
    pub fn verdict(&self, t: &JTx) -> Vec<bool> {
        let (tx, e) = self.build(t);
        match run(&tx, &e) {
            Ok(v) => v.iter().map(|r| r.is_ok()).collect(),
            Err(_) => vec![false; t.ins.len()],
        }
    }

    /// Records the outpoints of an applied transaction's outputs under their new handles.
    pub fn applied(&mut self, t: &JTx, handles: &[u64]) {
        let (tx, _) = self.build(t);
        let id = tx.id();
        for (j, h) in handles.iter().enumerate() {
            self.ops.insert(*h, TransactionOutpoint { transaction_id: id, index: j as u32 });
        }
    }
}


/// The model's and the engine's verdicts on `t`, which must agree input by input; returns whether every input passes.
pub fn agree(what: &str, eng: &Eng, t: &JTx) -> bool {
    let model: Vec<bool> = verify(t, &Real).iter().map(|r| r.is_ok()).collect();
    let engine = eng.verdict(t);
    assert_eq!(engine, model, "{what}: engine {engine:?} but model {:?}", verify(t, &Real));
    engine.iter().all(|x| *x)
}

/// A table: an arena on the engine market with one stand-in claim, and the engine outpoints.
pub struct JTable {
    pub a: Arena,
    pub eng: Eng,
    pub claim: u64,
    pub dispute: Option<u64>,
    pub receipts: Vec<u64>,
    pub now: i64,
}

/// The claim's DAA in the engine fixtures (the anchor of its dispute).
pub const ANCHOR: i64 = 10_000;
pub const CLAIM_ID: [u8; 32] = [0xc1; 32];
pub const FUNDER: u8 = 60;

impl JTable {
    pub fn new() -> JTable {
        let mut a = Arena::new(jmarket(), FUNDER);
        let claim = a.claim(CLAIM_ID, 9, ANCHOR);
        JTable { a, eng: Eng::default(), claim, dispute: None, receipts: vec![], now: ANCHOR }
    }
    pub fn d(&self) -> Js {
        self.a.js(self.dispute.expect("open"))
    }
    /// An aged free stake of key `seed` (accepted exactly `age` before the anchor, or `late` DAA later).
    pub fn stake(&mut self, seed: u8, v: i64, late: i64) -> u64 {
        let daa = ANCHOR - self.a.m.p.age + late;
        self.a.stake(seed, v, daa)
    }
    /// Runs `t` (the model and the engine must agree) and applies it at `daa` if every input passes.
    pub fn run(&mut self, what: &str, t: &JTx, daa: i64) -> Option<Vec<u64>> {
        if agree(what, &self.eng, t) {
            let hs = self.a.apply(t, daa.max(self.now));
            self.now = daa.max(self.now);
            self.eng.applied(t, &hs);
            Some(hs)
        } else {
            None
        }
    }
    pub fn open(&mut self, stake: u64, seed: u8, daa: i64) -> bool {
        let t = self.a.open_tx(self.claim, stake, seed, 50_000_000, &Real);
        match self.run("open", &t, daa) {
            Some(o) => {
                self.claim = o[0];
                self.receipts.push(o[1]);
                self.dispute = Some(o[2]);
                true
            }
            None => false,
        }
    }
    pub fn deposit(&mut self, side: i64, stakes: &[(u64, u8)], daa: i64) -> bool {
        let t = self.a.deposit_tx(self.dispute.unwrap(), side, stakes, SIGHASH_SINGLE_ACP);
        match self.run("deposit", &t, daa) {
            Some(o) => {
                self.dispute = Some(o[0]);
                self.receipts.extend(&o[1..=stakes.len()]);
                true
            }
            None => false,
        }
    }
    pub fn window_end(&self) -> i64 {
        let d = self.d();
        let st = if d.start == -1 { self.a.w.utxos[&self.dispute.unwrap()].daa } else { d.start };
        st + self.a.m.p.round
    }
    pub fn resolve(&mut self, lock: i64) -> bool {
        let t = self.a.resolve_tx(self.dispute.unwrap(), self.claim, lock);
        match self.run("resolve", &t, lock) {
            Some(o) => {
                self.dispute = Some(o[0]);
                self.claim = o[1];
                true
            }
            None => false,
        }
    }
    /// Settles `rs` (winners first in the transaction); returns the payout per receipt handle if accepted.
    pub fn settle(&mut self, rs: &[u64]) -> Option<Vec<(u64, i64)>> {
        let pool = self.dispute.unwrap();
        let w = self.a.js(pool).side;
        let t = self.a.settle_tx(pool, rs);
        let order: Vec<u64> = t.ins[1..t.ins.len() - 1].iter().map(|i| i.id).collect();
        let sides: Vec<i64> = order.iter().map(|h| self.a.js(*h).side).collect();
        let now = self.now + 1;
        let o = self.run("settle", &t, now)?;
        self.dispute = Some(o[0]);
        let mut j = 1;
        let mut res = vec![];
        for (h, s) in order.iter().zip(sides) {
            if s == w {
                res.push((*h, t.outs[j].value));
                j += 1;
            } else {
                res.push((*h, 0));
            }
        }
        self.receipts.retain(|r| !order.contains(r));
        Some(res)
    }
    pub fn close(&mut self) -> Option<JTx> {
        let t = self.a.close_tx(self.dispute.unwrap());
        let now = self.now + 1;
        self.run("close", &t, now).map(|_| t)
    }
    pub fn claim_status(&self) -> i64 {
        match &self.a.w.utxos[&self.claim].out.script {
            Script::Claim(c) => c.status,
            s => panic!("not a claim: {s:?}"),
        }
    }
}
