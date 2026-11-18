//! Transaction builders for every KpfPair / KpfOpen path, shared by the listing tests (P3, P4) and the
//! conservation experiment X6.
//!
//! Each builder returns a `Draft`: the inputs and outputs of the shape of DESIGN 4.5 with every party output at
//! the least value its covenant accepts (an executor that keeps everything it can), and the funder's change
//! output, which `build` balances so that the transaction creates no sompi. Tests mutate a draft (an output's
//! value or script, an argument) before building it.

use kaspa_consensus_core::tx::{ScriptPublicKey, Transaction, TransactionId, TransactionOutpoint, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kaspa_txscript::opcodes::codes::{OpCheckSig, OpData32};
use secp256k1::Keypair;
use silverscript_abi::ArtifactValue;

use super::*;
use crate::common::*;

pub const FEE: u64 = 10_000;
/// A plain payout below 0.02 KAS is not created (DESIGN 4.6).
pub const DUST: i64 = kpf_protocol::state::PLAIN_DUST;
pub const IDLE_SEQ: u64 = IDLE as u64;

pub fn iv(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}

pub fn bv(b: &[u8]) -> Arg {
    Arg::V(ArtifactValue::Bytes(b.to_vec()))
}

/// P2PK script of an x-only key.
pub fn p2pk_x(key: &[u8; 32]) -> ScriptPublicKey {
    let mut s = vec![OpData32];
    s.extend_from_slice(key);
    s.push(OpCheckSig);
    ScriptPublicKey::new(0, s.into())
}

/// An outpoint for a funder input, distinct per `n` (genesis ids hash the authorizing outpoint).
pub fn fop(n: u32) -> TransactionOutpoint {
    TransactionOutpoint { transaction_id: TransactionId::from_bytes([0xf0; 32]), index: n }
}

/// A live KpfPair UTXO.
#[derive(Clone, Debug)]
pub struct Live {
    pub p: Pair,
    pub value: u64,
    pub id: Hash,
    pub op: TransactionOutpoint,
    /// The UTXO's DAA score (what `OpTxInputDaaScore` reads; the clock start of a pair with `since = 0`).
    pub daa: u64,
}

impl Live {
    /// The evidence clock start S (DESIGN 4.5c).
    pub fn clock(&self) -> i64 {
        self.p.clock_start(self.daa)
    }
    /// The state with `since = S`: what every output that does not change hands carries.
    pub fn clocked(&self) -> Pair {
        self.p.clocked(self.daa)
    }
    /// The first DAA at which a hand-over (P3, P4) is allowed: S + 4R.
    pub fn handover_from(&self) -> u64 {
        self.p.handover_from(self.daa).max(0) as u64
    }
    /// This pair in a UTXO of DAA score `daa`.
    pub fn at(mut self, daa: u64) -> Live {
        self.daa = daa;
        self
    }
}

/// A live KpfPending UTXO (DESIGN 4.5d).
#[derive(Clone, Debug)]
pub struct LivePend {
    pub w: Pending,
    pub value: u64,
    pub id: Hash,
    pub op: TransactionOutpoint,
    /// The UTXO's DAA score P: the clock of its counter-evidence.
    pub daa: u64,
}

impl LivePend {
    pub fn at(mut self, daa: u64) -> LivePend {
        self.daa = daa;
        self
    }
}

pub fn pending_input(l: &LivePend, entry: &'static str, args: Vec<Arg>) -> In {
    In { outpoint: l.op, utxo: UtxoEntry::new(l.value, p2sh(&l.w.art()), l.daa, false, Some(l.id)), spend: Spend::Signed { art: l.w.art(), entry, args } }
}

/// The pending parts an accepted draft creates, as live UTXOs (DAA score 0; set it with `at`).
pub fn live_pendings(tx: &Transaction, d: &Draft) -> Vec<LivePend> {
    d.pendings
        .iter()
        .map(|(idx, w)| {
            let (op, u) = utxo_of(tx, *idx);
            LivePend { w: w.clone(), value: u.amount, id: u.covenant_id.expect("pending output has an id"), op, daa: u.block_daa_score }
        })
        .collect()
}

/// A live KpfOpen UTXO.
#[derive(Clone, Debug)]
pub struct LiveOpen {
    pub o: Open,
    pub value: u64,
    pub id: Hash,
    pub op: TransactionOutpoint,
}

#[allow(unused_imports)]
pub use kpf_protocol::payoff::owed;

/// A transaction under construction.
#[derive(Clone)]
pub struct Draft {
    pub ins: Vec<In>,
    pub outs: Vec<TransactionOutput>,
    pub lock: u64,
    pub seqs: Vec<u64>,
    /// (output, authorizing input, authorizing outpoint): a genesis binding computed at build time.
    pub genesis: Vec<(usize, u16, TransactionOutpoint)>,
    /// The funder's change output, balanced at build time (inputs - other outputs - FEE).
    pub change: Option<usize>,
    /// The KpfPair states written to outputs (for X6 bookkeeping).
    pub pairs: Vec<(usize, Pair)>,
    /// The KpfPending parts written to outputs (DESIGN 4.5d).
    pub pendings: Vec<(usize, Pending)>,
    /// The KpfOpen continuations written to outputs.
    pub opens: Vec<(usize, Open)>,
}

impl Draft {
    pub fn new(ins: Vec<In>, outs: Vec<TransactionOutput>, change: Option<usize>) -> Draft {
        Draft { ins, outs, lock: 0, seqs: vec![], genesis: vec![], change, pairs: vec![], pendings: vec![], opens: vec![] }
    }

    pub fn build(&self) -> (Transaction, Vec<UtxoEntry>) {
        let mut outs = self.outs.clone();
        if let Some(c) = self.change {
            let ins: u128 = self.ins.iter().map(|i| i.utxo.amount as u128).sum();
            let others: u128 = outs.iter().enumerate().filter(|(k, _)| *k != c).map(|(_, o)| o.value as u128).sum();
            let v = ins.checked_sub(others + FEE as u128).expect("draft is underfunded");
            outs[c].value = u64::try_from(v).expect("change fits");
        }
        for &(idx, auth, op) in &self.genesis {
            outs[idx].covenant = bind(auth, genesis_id(op, &outs, &[idx]));
        }
        build_ext(&self.ins, outs, self.lock, &self.seqs)
    }

    /// Replaces the arguments of the contract input `k`.
    pub fn set_args(&mut self, k: usize, args: Vec<Arg>) {
        if let Spend::Signed { args: a, .. } = &mut self.ins[k].spend {
            *a = args;
        } else {
            panic!("input {k} is not a contract input");
        }
    }

    pub fn args(&self, k: usize) -> Vec<Arg> {
        match &self.ins[k].spend {
            Spend::Signed { args, .. } => args.clone(),
            _ => panic!("input {k} is not a contract input"),
        }
    }

    /// Replaces the pair written to output `idx` (keeping its value and binding).
    pub fn set_pair(&mut self, idx: usize, p: &Pair) {
        self.outs[idx].script_public_key = p2sh(&p.art());
        for e in self.pairs.iter_mut() {
            if e.0 == idx {
                e.1 = p.clone();
            }
        }
    }
}

pub fn pair_input(l: &Live, entry: &'static str, args: Vec<Arg>) -> In {
    In { outpoint: l.op, utxo: UtxoEntry::new(l.value, p2sh(&l.p.art()), l.daa, false, Some(l.id)), spend: Spend::Signed { art: l.p.art(), entry, args } }
}

pub fn open_input(l: &LiveOpen, entry: &'static str, args: Vec<Arg>) -> In {
    In { outpoint: l.op, utxo: UtxoEntry::new(l.value, p2sh(&l.o.art()), 0, false, Some(l.id)), spend: Spend::Signed { art: l.o.art(), entry, args } }
}

pub fn funder(f: &Keypair, n: u32, value: u64) -> In {
    p2pk_in(fop(n), value, None, f)
}

pub const F_FUNDS: u64 = 100_000_000_000_000; // 1,000,000 KAS

#[allow(unused_imports)]
pub use kpf_protocol::payoff::order_spend;

/// The continuation-or-refund a KpfOpen requires for a fill of n, at output `idx`.
pub fn order_out(lo: &LiveOpen, n: i64, idx: u16) -> (TransactionOutput, Option<Open>) {
    let (rest, cont) = kpf_protocol::payoff::order_rest(&lo.o, lo.value, n);
    match cont {
        Some(c) => (out(rest, p2sh(&c.art()), bind(idx, lo.id)), Some(c)),
        None => (out(rest, p2pk_x(&lo.o.owner), None), None),
    }
}

// ---------------------------------------------------------------- T1 open

/// T1: in 0 L fill, 1 S fill, 2 F. out 0 L out, 1 S out, 2 the new pair (genesis by F), 3 change.
pub fn open_draft(lo: &LiveOpen, so: &LiveOpen, n: i64, x: i64, f: &Keypair, fn_: u32) -> Draft {
    let (bl, bs) = (x - lo.o.my_rate, x + so.o.my_rate);
    let mut p = pair(&keypair(1), &keypair(2));
    p.m = lo.o.m.clone();
    (p.size, p.bl, p.bs, p.entry, p.long_key, p.short_key, p.tag_l, p.tag_s) = (n, bl, bs, x, lo.o.owner, so.o.owner, lo.id, so.id);
    let (pre, suf) = parts(pair_base());
    let fa = |ck: &[u8; 32], ct: Hash| vec![iv(2), iv(n), iv(x), iv(bl), iv(bs), bv(ck), bv(&ct.as_bytes()), bv(&pre), bv(&suf)];
    let ins = vec![open_input(lo, "fill", fa(&so.o.owner, so.id)), open_input(so, "fill", fa(&lo.o.owner, lo.id)), funder(f, fn_, F_FUNDS)];
    let (lout, lc) = order_out(lo, n, 0);
    let (sout, sc) = order_out(so, n, 1);
    let outs = vec![lout, sout, out(p.floor() as u64, p2sh(&p.art()), None), out(0, p2pk_spk(f), None)];
    let mut d = Draft::new(ins, outs, Some(3));
    d.genesis.push((2, 2, fop(fn_)));
    d.pairs.push((2, p));
    for (i, c) in [(0, lc), (1, sc)] {
        if let Some(c) = c {
            d.opens.push((i, c));
        }
    }
    d
}

// ---------------------------------------------------------------- P1 amend

/// P1: in 0 pair (signed by `signer`), 1 F. out 0 continuation (value kept), 1 change.
pub fn amend_draft(l: &Live, signer: &Keypair, side: i64, w: (i64, i64, i64), f: &Keypair, fn_: u32) -> Draft {
    let mut n = l.clocked().cleared();
    if side == LONG {
        (n.long_ask, n.long_min, n.long_tip) = w;
    } else {
        (n.short_bid, n.short_min, n.short_tip) = w;
    }
    let ins = vec![pair_input(l, "amend", vec![Arg::Sig(*signer), iv(side), iv(w.0), iv(w.1), iv(w.2)]), funder(f, fn_, F_FUNDS)];
    let outs = vec![out(l.value, p2sh(&n.art()), bind(0, l.id)), out(0, p2pk_spk(f), None)];
    let mut d = Draft::new(ins, outs, Some(1));
    d.pairs.push((0, n));
    d
}

// ---------------------------------------------------------------- P3 transfer

pub use kpf_protocol::payoff::{CloseOut, CloseSpec, MutualOut, TransferOut, TransferSpec};

#[derive(Clone)]
pub enum Entrant {
    /// A KpfOpen of the leaver's side, at input 1, taking with its own rate.
    Open(LiveOpen),
    /// A plain-key taker that funds the whole transaction from its input 1.
    Direct(Keypair),
}

pub fn transfer_out(l: &Live, t: &TransferSpec) -> TransferOut {
    kpf_protocol::payoff::transfer_out(&l.clocked(), l.value, t)
}

/// The hand-over of P3 and P7 (KpfPair.handOver) for a leaver owed `pay`.
pub fn hand_over_out(l: &Live, t: &TransferSpec, pay: i64) -> TransferOut {
    kpf_protocol::payoff::hand_over_out(&l.clocked(), l.value, t, pay)
}

/// P3: in 0 pair, 1 entrant (KpfOpen or direct taker), 2 F (KpfOpen case only). out 0 leaver, 1 entrant's
/// order continuation-or-refund or the direct taker's change, 2 and 3 the part and the remainder (in the order
/// given by k), then F's change.
pub fn transfer_draft(l: &Live, t: &TransferSpec, ent: &Entrant, f: &Keypair, fn_: u32) -> Draft {
    let to = transfer_out(l, t);
    let pa = vec![iv(t.side), iv(t.n), iv(t.x), iv(t.nb), bv(&t.ek), bv(&t.et.as_bytes()), iv(t.k)];
    let mut ins = vec![pair_input(l, "transfer", pa)];
    let mut outs = vec![out(to.pay as u64, p2pk_x(&to.leaver), None)];
    let mut opens = vec![];
    let change;
    match ent {
        Entrant::Open(lo) => {
            let part_out = if to.rest.is_some() && t.k == 1 { 3 } else { 2 };
            let oa = take_args(&to.part, t.side, part_out as i64, t.n, t.x);
            ins.push(open_input(lo, "take", oa));
            ins.push(funder(f, fn_, F_FUNDS));
            let (oo, oc) = order_out(lo, t.n, 1);
            outs.push(oo);
            if let Some(c) = oc {
                opens.push((1, c));
            }
        }
        Entrant::Direct(kp) => {
            ins.push(p2pk_in(fop(fn_), F_FUNDS, None, kp));
            outs.push(out(0, p2pk_spk(kp), None));
        }
    }
    let part_o = out(to.part_value as u64, p2sh(&to.part.art()), bind(0, l.id));
    let mut pairs = vec![];
    match &to.rest {
        None => {
            outs.push(part_o);
            pairs.push((2, to.part.clone()));
        }
        Some((r, rv)) => {
            let rest_o = out(*rv as u64, p2sh(&r.art()), bind(0, l.id));
            if t.k == 0 {
                outs.push(part_o);
                outs.push(rest_o);
                pairs.push((2, to.part.clone()));
                pairs.push((3, r.clone()));
            } else {
                outs.push(rest_o);
                outs.push(part_o);
                pairs.push((2, r.clone()));
                pairs.push((3, to.part.clone()));
            }
        }
    }
    match ent {
        Entrant::Open(_) => {
            outs.push(out(0, p2pk_spk(f), None));
            change = outs.len() - 1;
        }
        Entrant::Direct(_) => change = 1,
    }
    let mut d = Draft::new(ins, outs, Some(change));
    d.pairs = pairs;
    d.opens = opens;
    // The hand-over rate limit (DESIGN 4.5c): the least lock time the pair accepts.
    d.lock = l.handover_from();
    d
}

/// Arguments of KpfOpen.take for the part `part`, taken by an order of `side` for n at x:
/// (partOut, n, x, e, bound, counterKey, ca, cm, ct, cPre, cSuf), with e the part's entry and the stayer's bound,
/// key and listing read from the part.
pub fn take_args(part: &Pair, side: i64, part_out: i64, n: i64, x: i64) -> Vec<Arg> {
    let (bound, ck, w) = if side == LONG { (part.bs, part.short_key, part.short_window()) } else { (part.bl, part.long_key, part.long_window()) };
    let (pre, suf) = parts(pair_base());
    vec![iv(part_out), iv(n), iv(x), iv(part.entry), iv(bound), bv(&ck), iv(w.0), iv(w.1), iv(w.2), bv(&pre), bv(&suf)]
}

// ---------------------------------------------------------------- P4 close-versus-close

pub fn close_out(h: &Live, d: &Live, c: &CloseSpec) -> CloseOut {
    kpf_protocol::payoff::close_out(&h.clocked(), h.value, &d.clocked(), d.value, d.id, c)
}

pub fn host_args(d: &Live, c: &CloseSpec, k: i64) -> Vec<Arg> {
    let w = d.p.long_window();
    vec![iv(c.n), iv(c.x), iv(c.e3), iv(k), iv(d.p.bl), bv(&d.p.long_key), iv(w.0), iv(w.1), iv(w.2), bv(&d.id.as_bytes())]
}

pub fn donor_args(h: &Live, c: &CloseSpec, p3_out: i64) -> Vec<Arg> {
    let w = h.p.short_window();
    vec![iv(c.n), iv(c.x), iv(c.e3), iv(p3_out), iv(h.p.bs), bv(&h.p.short_key), iv(w.0), iv(w.1), iv(w.2)]
}

/// P4: in 0 host (its long leaves), 1 donor (its short leaves), 2 F. out 0 host's long, 1 donor's short, 2 P3
/// (the host's continuation), then the host's remainder and the donor's remainder if any, then F's change.
pub fn close_draft(h: &Live, d: &Live, c: &CloseSpec, f: &Keypair, fn_: u32) -> Draft {
    let co = close_out(h, d, c);
    let ins = vec![pair_input(h, "closeHost", host_args(d, c, 0)), pair_input(d, "closeDonor", donor_args(h, c, 2)), funder(f, fn_, F_FUNDS)];
    let mut outs = vec![
        out(co.host_pay as u64, p2pk_x(&h.p.long_key), None),
        out(co.donor_pay as u64, p2pk_x(&d.p.short_key), None),
        out(co.p3_value as u64, p2sh(&co.p3.art()), bind(0, h.id)),
    ];
    let mut pairs = vec![(2, co.p3.clone())];
    if let Some((r, rv)) = &co.host_rest {
        outs.push(out(*rv as u64, p2sh(&r.art()), bind(0, h.id)));
        pairs.push((outs.len() - 1, r.clone()));
    }
    if let Some((r, rv)) = &co.donor_rest {
        outs.push(out(*rv as u64, p2sh(&r.art()), bind(1, d.id)));
        pairs.push((outs.len() - 1, r.clone()));
    }
    outs.push(out(0, p2pk_spk(f), None));
    let ch = outs.len() - 1;
    let mut dr = Draft::new(ins, outs, Some(ch));
    dr.pairs = pairs;
    dr.lock = h.handover_from().max(d.handover_from());
    dr
}

// ---------------------------------------------------------------- P4 mutual close

pub fn mutual_out(l: &Live, n: i64, x: i64) -> MutualOut {
    kpf_protocol::payoff::mutual_out(&l.clocked(), l.value, n, x)
}

/// T3': in 0 pair, 1 F. out 0 long, then the remainder if any, then F's change, then the short at M - 1.
pub fn mutual_draft(l: &Live, n: i64, x: i64, f: &Keypair, fn_: u32) -> Draft {
    let mo = mutual_out(l, n, x);
    let ins = vec![pair_input(l, "mutualClose", vec![iv(n), iv(x)]), funder(f, fn_, F_FUNDS)];
    let mut outs = vec![out(mo.lp as u64, p2pk_x(&l.p.long_key), None)];
    let mut pairs = vec![];
    if let Some((r, rv)) = &mo.rest {
        outs.push(out(*rv as u64, p2sh(&r.art()), bind(0, l.id)));
        pairs.push((1, r.clone()));
    }
    outs.push(out(0, p2pk_spk(f), None));
    let ch = outs.len() - 1;
    outs.push(out(mo.sp as u64, p2pk_x(&l.p.short_key), None));
    let mut d = Draft::new(ins, outs, Some(ch));
    d.pairs = pairs;
    d
}

// ---------------------------------------------------------------- P8 idle settle

/// (long payout, short payout) of P8, with the dust rule.
pub fn idle_out(l: &Live) -> (i64, i64) {
    kpf_protocol::payoff::idle_out(&l.p, l.value)
}

/// P8: in 0 pair (age idleDaa), 1 F. out 0 long, 1 change, 2 short.
pub fn idle_draft(l: &Live, f: &Keypair, fn_: u32) -> Draft {
    let (lp, sp) = idle_out(l);
    let ins = vec![pair_input(l, "idleSettle", vec![]), funder(f, fn_, F_FUNDS)];
    let outs = vec![out(lp as u64, p2pk_x(&l.p.long_key), None), out(0, p2pk_spk(f), None), out(sp as u64, p2pk_x(&l.p.short_key), None)];
    let mut d = Draft::new(ins, outs, Some(1));
    d.seqs = vec![IDLE_SEQ, 0];
    d
}

/// The UTXOs an accepted draft creates for its pairs, as live pairs.
pub fn live_pairs(tx: &Transaction, d: &Draft) -> Vec<Live> {
    d.pairs
        .iter()
        .map(|(idx, p)| {
            let (op, u) = utxo_of(tx, *idx);
            Live { p: p.clone(), value: u.amount, id: u.covenant_id.expect("pair output has an id"), op, daa: u.block_daa_score }
        })
        .collect()
}

pub fn live_opens(tx: &Transaction, d: &Draft) -> Vec<LiveOpen> {
    d.opens
        .iter()
        .map(|(idx, o)| {
            let (op, u) = utxo_of(tx, *idx);
            LiveOpen { o: o.clone(), value: u.amount, id: u.covenant_id.expect("order output has an id"), op }
        })
        .collect()
}
