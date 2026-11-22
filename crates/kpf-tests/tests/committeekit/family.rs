//! The market family of the committee (`KpfOpenC`, `KpfPairC`, `KpfPendingC`): fixtures and transaction shapes. States
//! and values come from the library model (`kpf_protocol::cmarket`).

use std::sync::OnceLock;

use kaspa_consensus_core::tx::{ScriptPublicKey, TransactionOutpoint, TransactionOutput};
use kaspa_consensus_core::Hash;
use kpf_protocol::artifacts::{splice, Template};
use kpf_protocol::cmarket::*;
use kpf_protocol::committee::*;
use kpf_protocol::oracle::{maker_of, Makers};
use kpf_protocol::payoff::{owed, q};
use kpf_protocol::state::{KIND_CLOSE, KIND_LIQ, KIND_TRIM, LONG, SHORT};
use secp256k1::Keypair;
use silverscript_abi::SilAbiArtifact;

use super::*;

pub struct FK {
    pub kit: CKit,
    pub open: SilAbiArtifact,
    pub pair: SilAbiArtifact,
    pub pending: SilAbiArtifact,
}

/// The fixture market: priced by the fixture committee, an underlying of 6 decimals priced in sompi, a lease of 8,000 DAA,
/// a lapse 1,000 after it, 50 DAA to cure, a claim valid within 100 DAA of its time, 20 DAA of quiet, mmr 5 %, g 2 %,
/// a fee of 0.5 % (at least 0.01 KAS), a carrier of 0.2 KAS.
pub fn cmarket() -> CMarket {
    CMarket {
        committee_id: rid(),
        feed_hash: [0xfe; 32],
        p: CParams {
            term: 8_000,
            scale: 1_000_000,
            price_max: 100_000_000_000_000,
            mmr_bps: 500,
            g_bps: 200,
            liq_fee_bps: 50,
            keeper_fee_min: 1_000_000,
            min_size: 1_000_000,
            carrier: 20_000_000,
            idle_daa: 1_000,
            grace_daa: 50,
            late_daa: 100,
            quiet_daa: 20,
        },
    }
}

pub fn long_kp() -> Keypair {
    keypair(41)
}
pub fn short_kp() -> Keypair {
    keypair(42)
}

/// The worked pair: 1,000 units at 10 KAS, the long's bound at 9, the short's at 12, a lease until 14,000.
pub fn sample_pair(m: &CMarket) -> CPair {
    CPair {
        m: *m,
        size: 1_000_000_000,
        bl: 900_000_000,
        bs: 1_200_000_000,
        entry: 1_000_000_000,
        long_key: x(&long_kp()),
        short_key: x(&short_kp()),
        expiry: 14_000,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers::default(),
    }
}

pub fn sample_order(m: &CMarket, side: i64, owner: [u8; 32]) -> COpen {
    COpen {
        m: *m,
        side,
        owner,
        size_left: 1_000_000_000,
        limit: 1_000_000_000,
        my_rate: if side == LONG { 100_000_000 } else { 200_000_000 },
        min_counter_rate: 0,
        min_fill: 1_000_000,
        tip: 0,
        active_from: 0,
        expiry_daa: 1_000_000,
    }
}

/// A pending state with nothing in it: the sample the pending template is compiled with.
fn sample_pending(sp: &CPair) -> CPending {
    CPending {
        m: sp.m,
        kind: KIND_CLOSE,
        side: LONG,
        n: sp.size,
        bl: sp.bl,
        bs: sp.bs,
        quote: sp.entry,
        t0: 0,
        cure_bound: 0,
        fee: 0,
        refund: 0,
        proposer: sp.long_key,
        entry: sp.entry,
        long_key: sp.long_key,
        short_key: sp.short_key,
        expiry: sp.expiry,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        restore_hash: [0; 32],
        cure_hash: [0; 32],
        forfeit_hash: [0; 32],
        mk_parent: [0; 32],
    }
}

pub fn fk() -> &'static FK {
    static K: OnceLock<FK> = OnceLock::new();
    K.get_or_init(|| {
        let c = ck();
        let m = cmarket();
        let mut consts = CConsts {
            epoch_table_tpl: c.epoch_table_t.hash,
            epoch_table_pre: c.epoch_table_t.prefix.len() as i64,
            epoch_table_suf: c.epoch_table_t.suffix.len() as i64,
            attest_tag: c.attest_tag,
            exit_tpl: c.exit_t.hash,
            pending_tpl: [0; 32],
            pair_tpl: [0; 32],
        };
        let sp = sample_pair(&m);
        // The pending template pins nothing (its return states are hashes); the pair pins the pending and the epoch table,
        // the order the pair.
        let pending = compile("legacy/KpfPendingC.sil", &sample_pending(&sp).ctor());
        let pending_t = Template::of("KpfPendingC", &pending);
        consts.pending_tpl = pending_t.hash;
        let pair = compile("legacy/KpfPairC.sil", &sp.ctor_with(&consts));
        let pair_t = Template::of("KpfPairC", &pair);
        consts.pair_tpl = pair_t.hash;
        let open = compile("legacy/KpfOpenC.sil", &sample_order(&m, LONG, [3; 32]).ctor_with(&consts));
        let open_t = Template::of("KpfOpenC", &open);
        FK {
            kit: CKit { open: open_t, pair: pair_t, pending: pending_t, committee: c.committee_t.clone(), epoch_table: c.epoch_table_t.clone(), exit: c.exit_t.clone(), charge: c.charge_t.clone(), consts },
            open,
            pair,
            pending,
        }
    })
}

pub fn kit() -> &'static CKit {
    &fk().kit
}

pub fn pair_art(p: &CPair) -> SilAbiArtifact {
    splice(&fk().pair, &p.state_bytes())
}
pub fn pend_art(w: &CPending) -> SilAbiArtifact {
    splice(&fk().pending, &w.state_bytes())
}
pub fn open_art(o: &COpen) -> SilAbiArtifact {
    splice(&fk().open, &o.state_bytes())
}
pub fn pair_spk(p: &CPair) -> ScriptPublicKey {
    p2sh(&pair_art(p))
}
pub fn pend_spk(w: &CPending) -> ScriptPublicKey {
    p2sh(&pend_art(w))
}
pub fn order_spk(o: &COpen) -> ScriptPublicKey {
    p2sh(&open_art(o))
}
pub fn key_spk(key: &[u8; 32]) -> ScriptPublicKey {
    ScriptPublicKey::new(0, [vec![0x20], key.to_vec(), vec![0xac]].concat().into())
}

pub type LPair = Live<CPair>;
pub type LPend = Live<CPending>;
pub type LOrder = Live<COpen>;

pub fn pair_in(l: &LPair, entry: &'static str, args: Vec<Arg>) -> In {
    let art = pair_art(&l.s);
    In { outpoint: l.op, utxo: utxo(l.value, p2sh(&art), l.daa, l.id), spend: Spend::Signed { art, entry, args } }
}
pub fn pend_in(l: &LPend, entry: &'static str, args: Vec<Arg>) -> In {
    let art = pend_art(&l.s);
    In { outpoint: l.op, utxo: utxo(l.value, p2sh(&art), l.daa, l.id), spend: Spend::Signed { art, entry, args } }
}
pub fn order_in(l: &LOrder, entry: &'static str, args: Vec<Arg>) -> In {
    let art = open_art(&l.s);
    In { outpoint: l.op, utxo: utxo(l.value, p2sh(&art), l.daa, l.id), spend: Spend::Signed { art, entry, args } }
}

/// Output `j` becomes the genesis of a fresh covenant id authored by input `auth` (outpoint `op`), the id computed over
/// output `j` alone once every output is final (what the family's `fresh` check requires).
pub fn seal(outs: &mut [TransactionOutput], j: usize, auth: u16, op: TransactionOutpoint) {
    outs[j].covenant = None;
    let g = genesis_id(op, outs, &[j]);
    outs[j].covenant = bind(auth, g);
}

pub const PAIR_ID: Hash = Hash::from_bytes([0x50; 32]);

/// A live pair of the fixture id holding exactly its floor, accepted at `daa`.
pub fn pair(p: &CPair, daa: u64) -> LPair {
    Live { op: outpoint(10), value: p.floor() as u64, daa, id: Some(PAIR_ID), s: p.clone() }
}

/// The epoch table of the epoch [6,000, 7,000) of the fixture table (100 KAS of weight, the three largest members 67).
pub fn epoch_epoch_table() -> LEpochTable {
    epoch_table(&EpochTable { members: table(TOP3), total: 100 * KAS, from: 6_000, until: 7_000, quorum_bps: 6_700, spread_bps: 50 })
}

/// The three largest members sign `p` for the time `t0` of the fixture market.
pub fn att(t: &EpochTable, t0: i64, p: i64) -> Attestation {
    let m = cmarket().id();
    let s: Vec<SignedPrice> = (0..3).map(|i| sign_price(&member_kp(i), &rid(), &m, t0, p)).collect();
    Attestation::collect(t, &rid(), &m, t0, &s).expect("a quorum")
}

pub fn attest_in(lt: &LEpochTable, a: &Attestation) -> In {
    epoch_table_in(lt, "attest", a.args().into_iter().map(Arg::V).collect())
}

fn tmpl(t: &Template) -> [Arg; 2] {
    [bv(&t.prefix), bv(&t.suffix)]
}

// ---------------------------------------------------------------- orders and the open

pub fn order(side: i64, kp: &Keypair, n: u8, daa: u64) -> LOrder {
    let o = sample_order(&cmarket(), side, x(kp));
    Live { op: outpoint(n), value: (o.spend(o.size_left) + KAS) as u64, daa, id: Some(Hash::from_bytes([n; 32])), s: o }
}

/// The pair two orders open: n units at x, the lease xe.
pub fn opened_pair(lo: &LOrder, so: &LOrder, n: i64, px: i64, xe: i64) -> CPair {
    CPair {
        m: lo.s.m,
        size: n,
        bl: px - lo.s.my_rate,
        bs: px + so.s.my_rate,
        entry: px,
        long_key: lo.s.owner,
        short_key: so.s.owner,
        expiry: xe,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers { long: maker_of(&lo.op), short: maker_of(&so.op), ..Makers::default() },
    }
}

/// Order x order: inputs the long order, the short order, the executor; outputs each order's rest or refund at its own
/// index, the pair (a fresh id authored by the executor's input), the change.
pub fn open_tx(lo: &LOrder, so: &LOrder, n: i64, px: i64, xe: i64, lock: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let p = opened_pair(lo, so, n, px, xe);
    let rest = |o: &LOrder, i: u16| {
        if n < o.s.size_left {
            out(o.value - o.s.spend(n) as u64, order_spk(&o.s.after(n)), bind(i, o.id.unwrap()))
        } else {
            out(o.value - o.s.spend(n) as u64, key_spk(&o.s.owner), None)
        }
    };
    let mut outs = vec![rest(lo, 0), rest(so, 1), out(p.floor() as u64, pair_spk(&p), None), change(1)];
    tweak(&mut outs);
    seal(&mut outs, 2, 2, outpoint(60));
    let [pre, suf] = tmpl(&kit().pair);
    let args = |other: &LOrder| vec![iv(2), iv(n), iv(px), iv(p.bl), iv(p.bs), bv(&other.s.owner), bv(&maker_of(&other.op)), iv(xe), pre.clone(), suf.clone()];
    build_ext(&[order_in(lo, "fill", args(so)), order_in(so, "fill", args(lo)), exec_in(60)], outs, lock, &[])
}

/// The worked pair, opened at DAA 6,100 by two orders accepted at 6,000, with the lease 14,000.
pub fn opened() -> LPair {
    let (lo, so) = (order(LONG, &long_kp(), 11, 6_000), order(SHORT, &short_kp(), 12, 6_000));
    let t = open_tx(&lo, &so, 1_000_000_000, 1_000_000_000, 14_000, 6_100, |_| {});
    must("the open", &t);
    live(&t, 2, 6_100, &opened_pair(&lo, &so, 1_000_000_000, 1_000_000_000, 14_000))
}

// ---------------------------------------------------------------- claims

#[derive(Clone, Copy, Debug)]
pub struct Claim {
    pub kind: i64,
    pub side: i64,
    pub n: i64,
    pub p: i64,
    pub t0: i64,
}

/// The pending part a claim writes whether or not the contract would accept it (the model's `pending_of` without its
/// refusals), so that a refused claim can be built and shown to be refused.
pub fn pending_forced(p: &CPair, c: &Claim, proposer: [u8; 32]) -> (CPending, i64) {
    if let Some(r) = p.pending_of(kit(), c.n, c.kind, c.side, c.p, c.t0, proposer) {
        return r;
    }
    let k = kit();
    let (mut fee, mut fb, mut cure_bound, mut cure_hash, mut forfeit_hash) = (0, if c.side == LONG { p.bl } else { p.bs }, 0, [0u8; 32], [0u8; 32]);
    let mut who = proposer;
    if c.kind == KIND_LIQ {
        fee = p.fee_of(c.n, c.side, c.p);
        fb = p.forfeit_bound_of(c.n, c.side, fee);
        forfeit_hash = p.forfeit_state(c.n, c.side, fb).script_hash(k);
        if let Some(cb) = p.cure_bound_of(c.side, c.p) {
            cure_bound = cb;
            cure_hash = p.cure_state(c.n, c.side, cb).script_hash(k);
        }
    } else {
        who = if c.side == LONG { p.long_key } else { p.short_key };
    }
    (
        CPending {
            m: p.m,
            kind: c.kind,
            side: c.side,
            n: c.n,
            bl: p.bl,
            bs: p.bs,
            quote: c.p,
            t0: c.t0,
            cure_bound,
            fee,
            refund: if c.n < p.size { p.m.p.carrier } else { 0 },
            proposer: who,
            entry: p.entry,
            long_key: p.long_key,
            short_key: p.short_key,
            expiry: p.expiry,
            long_ask: p.long_ask,
            long_min: p.long_min,
            long_tip: p.long_tip,
            short_bid: p.short_bid,
            short_min: p.short_min,
            short_tip: p.short_tip,
            restore_hash: p.restore_state(c.n).script_hash(k),
            cure_hash,
            forfeit_hash,
            mk_parent: [0; 32],
        },
        fb,
    )
}

/// How a propose is assembled, for the negatives: who signs a close, the proposer named, the attestation presented,
/// the lock time and the pair input's sequence.
#[derive(Clone)]
pub struct Propose {
    pub c: Claim,
    pub a: Attestation,
    pub signer: Option<Keypair>,
    pub proposer: [u8; 32],
    pub lock: u64,
    pub seq: u64,
}

impl Propose {
    /// A liquidation of `side` at `p` for the time `t0`, proposed by the executor.
    pub fn liq(lt: &LEpochTable, side: i64, n: i64, p: i64, t0: i64) -> Propose {
        Propose { c: Claim { kind: KIND_LIQ, side, n, p, t0 }, a: att(&lt.s, t0, p), signer: None, proposer: x(&exec_kp()), lock: t0 as u64, seq: 0 }
    }
    /// A close of `side` at `p` for the time `t0`, signed by `kp` after the quiet time.
    pub fn close(lt: &LEpochTable, side: i64, n: i64, p: i64, t0: i64, kp: &Keypair) -> Propose {
        Propose { c: Claim { kind: KIND_CLOSE, side, n, p, t0 }, a: att(&lt.s, t0, p), signer: Some(*kp), proposer: x(kp), lock: t0 as u64, seq: cmarket().p.quiet_daa as u64 }
    }
}

/// Inputs: the pair (propose), the epoch table (attest), the executor. Outputs: the pending part (a fresh id authored by the
/// pair), the epoch table, the remainder of a split, the change.
pub fn propose_tx(lp: &LPair, lt: &LEpochTable, pr: &Propose, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let (w, fb) = pending_forced(&lp.s, &pr.c, pr.proposer);
    let w = w.made_by(&lp.op);
    let whole = pr.c.n == lp.s.size;
    let pv = if whole { lp.value as i64 } else { lp.s.range_of(pr.c.n) + w.refund };
    let mut outs = vec![out(pv.max(1) as u64, pend_spk(&w), None), out(lt.value, epoch_table_spk(&lt.s), bind(1, RID))];
    if !whole && pr.c.n > 0 && pr.c.n < lp.s.size {
        outs.push(out(lp.s.rest_value(lp.value, pr.c.n) as u64, pair_spk(&lp.s.rest_of(lp.s.size - pr.c.n)), bind(0, lp.id.unwrap())));
    }
    outs.push(change(1));
    tweak(&mut outs);
    seal(&mut outs, 0, 0, lp.op);
    let [pre, suf] = tmpl(&kit().pending);
    let s = match &pr.signer {
        Some(kp) => Arg::Sig(*kp),
        None => bv(&[0u8; 65]),
    };
    let args = vec![iv(pr.c.n), iv(pr.c.kind), iv(pr.c.side), iv(pr.c.p), iv(pr.c.t0), iv(1), s, iv(fb), iv(0), bv(&pr.proposer), pre, suf];
    build_ext(&[pair_in(lp, "propose", args), attest_in(lt, &pr.a), exec_in(60)], outs, pr.lock, &[pr.seq])
}

/// The pending part and the remainder a propose leaves, the part accepted at DAA `p`.
pub fn proposed(lp: &LPair, lt: &LEpochTable, pr: &Propose, p: u64) -> (LPend, Option<LPair>) {
    let t = propose_tx(lp, lt, pr, |_| {});
    must("the propose", &t);
    let (w, _) = lp.s.pending_of(kit(), pr.c.n, pr.c.kind, pr.c.side, pr.c.p, pr.c.t0, pr.proposer).expect("the model accepts what the engine accepted");
    let rest = (pr.c.n < lp.s.size).then(|| live(&t, 2, p, &lp.s.rest_of(lp.s.size - pr.c.n)));
    (live(&t, 0, p, &w.made_by(&lp.op)), rest)
}

/// Finalize at a pending part: outputs by the model's settlement. A close pays the long at output 0 and the short last;
/// a liquidation pays the proposer's fee at output 0 and writes the forfeited part at output 1; a void claim writes the
/// restored part at output 0.
pub fn finalize_tx(lw: &LPend, seq: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let id = lw.id.unwrap();
    let mut outs = match lw.s.settlement(lw.value, lw.daa as i64) {
        CSettlement::Close { long, short } => {
            let mut o = vec![if long > 0 { out(long as u64, key_spk(&lw.s.long_key), None) } else { change(1) }, change(1)];
            if short > 0 {
                o.push(out(short as u64, key_spk(&lw.s.short_key), None));
            }
            o
        }
        CSettlement::Forfeit { part, value, fee } => {
            vec![out(fee as u64, key_spk(&lw.s.proposer), None), out(value as u64, pair_spk(&part), bind(0, id)), change(1)]
        }
        CSettlement::Restore { part, value } => vec![out(value as u64, pair_spk(&part), bind(0, id)), change(1)],
        CSettlement::Trim { part, value, take } => vec![out(take as u64, key_spk(&lw.s.proposer), None), out(value as u64, pair_spk(&part), bind(0, id)), change(1)],
    };
    tweak(&mut outs);
    build_ext(&[pend_in(lw, "finalize", vec![]), exec_in(60)], outs, 0, &[seq])
}

/// A trim (`propose` kind 3) as it is assembled, for the negatives: the side, the part named (the whole pair in every
/// accepted trim), the bound asked for, the attestation, who signs, the lock time and the pair input's sequence.
#[derive(Clone)]
pub struct Trim {
    pub side: i64,
    pub n: i64,
    pub nb: i64,
    pub p: i64,
    pub t0: i64,
    pub a: Attestation,
    pub signer: Keypair,
    pub lock: u64,
    pub seq: u64,
}

impl Trim {
    /// A trim of `side` of the worked pair to `nb` at `p` for the time `t0`, signed by `kp` after the quiet time.
    pub fn of(lt: &LEpochTable, side: i64, nb: i64, p: i64, t0: i64, kp: &Keypair) -> Trim {
        Trim { side, n: 1_000_000_000, nb, p, t0, a: att(&lt.s, t0, p), signer: *kp, lock: t0 as u64, seq: cmarket().p.quiet_daa as u64 }
    }
}

/// The pending part a trim writes whether or not the contract would accept it.
pub fn trim_forced(p: &CPair, tr: &Trim) -> CPending {
    if tr.n == p.size {
        if let Some(w) = p.trim_pending(kit(), tr.side, tr.p, tr.t0, tr.nb) {
            return w;
        }
    }
    let (mut w, _) = pending_forced(p, &Claim { kind: KIND_TRIM, side: tr.side, n: tr.n, p: tr.p, t0: tr.t0 }, [0; 32]);
    w.cure_bound = tr.nb;
    w.cure_hash = CPair { size: tr.n, ..p.trim_state(tr.side, tr.nb) }.script_hash(kit());
    w
}

/// Inputs: the pair (propose, kind 3), the epoch table (attest), the executor. Outputs: the pending trim (a fresh id
/// authored by the pair, the pair's whole value), the epoch table, the change.
pub fn trim_tx(lp: &LPair, lt: &LEpochTable, tr: &Trim, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let w = trim_forced(&lp.s, tr).made_by(&lp.op);
    let whole = tr.n == lp.s.size;
    let pv = if whole { lp.value as i64 } else { lp.s.range_of(tr.n) + w.refund };
    let mut outs = vec![out(pv.max(1) as u64, pend_spk(&w), None), out(lt.value, epoch_table_spk(&lt.s), bind(1, RID))];
    if !whole && tr.n > 0 && tr.n < lp.s.size {
        outs.push(out(lp.s.rest_value(lp.value, tr.n) as u64, pair_spk(&lp.s.rest_of(lp.s.size - tr.n)), bind(0, lp.id.unwrap())));
    }
    outs.push(change(1));
    tweak(&mut outs);
    seal(&mut outs, 0, 0, lp.op);
    let [pre, suf] = tmpl(&kit().pending);
    let args = vec![iv(tr.n), iv(KIND_TRIM), iv(tr.side), iv(tr.p), iv(tr.t0), iv(1), Arg::Sig(tr.signer), iv(tr.nb), iv(0), bv(&x(&tr.signer)), pre, suf];
    build_ext(&[pair_in(lp, "propose", args), attest_in(lt, &tr.a), exec_in(60)], outs, tr.lock, &[tr.seq])
}

/// The pending trim, accepted at DAA `p`.
pub fn trimmed(lp: &LPair, lt: &LEpochTable, tr: &Trim, p: u64) -> LPend {
    let t = trim_tx(lp, lt, tr, |_| {});
    must("the trim", &t);
    let w = lp.s.trim_pending(kit(), tr.side, tr.p, tr.t0, tr.nb).expect("the model accepts what the engine accepted");
    live(&t, 0, p, &w.made_by(&lp.op))
}

/// Cure: the proposer's refund at output 0 (the change when there is none), the cured part at output 1.
pub fn cure_tx(lw: &LPend, signer: &Keypair, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let (part, value) = lw.s.cure(lw.value).unwrap_or((lw.s.pair_part(), lw.value as i64));
    let mut outs = vec![
        if lw.s.refund > 0 { out(lw.s.refund as u64, key_spk(&lw.s.proposer), None) } else { change(1) },
        out(value as u64, pair_spk(&part), bind(0, lw.id.unwrap())),
        change(1),
    ];
    tweak(&mut outs);
    build(&[pend_in(lw, "cure", vec![Arg::Sig(*signer)]), exec_in(60)], outs)
}

// ---------------------------------------------------------------- the lease, hand-overs, price-free entries

pub fn keep_alive_tx(lp: &LPair, lt: &LEpochTable, a: &Attestation, lock: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let next = CPair { expiry: a.t0 + lp.s.m.p.term, mk: Makers::default(), ..lp.s.clone() };
    let mut outs = vec![out(lp.value, pair_spk(&next), bind(0, lp.id.unwrap())), out(lt.value, epoch_table_spk(&lt.s), bind(1, RID)), change(1)];
    tweak(&mut outs);
    build_ext(&[pair_in(lp, "keepAlive", vec![iv(1), iv(a.t0)]), attest_in(lt, a), exec_in(60)], outs, lock, &[])
}

pub fn lapse_tx(lp: &LPair, lock: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let (long, short) = lp.s.lapse_out(lp.value);
    let mut outs = vec![if long > 0 { out(long as u64, key_spk(&lp.s.long_key), None) } else { change(1) }, change(1)];
    if short > 0 {
        outs.push(out(short as u64, key_spk(&lp.s.short_key), None));
    }
    tweak(&mut outs);
    build_ext(&[pair_in(lp, "lapseSettle", vec![]), exec_in(60)], outs, lock, &[])
}

/// What a hand-over of `side` of the part n at x to `ek` with the bound nb writes: the leaver's payout, the part with
/// its makers, the part's value and what the remainder keeps.
pub fn handed(lp: &LPair, side: i64, n: i64, px: i64, nb: i64, ek: [u8; 32], et: [u8; 32], tip: i64) -> (i64, CPair, i64, i64) {
    let p = &lp.s;
    let sc = p.m.p.scale;
    let mut pay = owed(n, if side == LONG { px - p.bl } else { p.bs - px }, tip, sc);
    let mut keep = lp.value as i64 - q(n, p.bs - p.bl, sc, 0);
    if pay < 2_000_000 {
        keep += pay;
        pay = 0;
    }
    let mut part = p.rest_of(n);
    part.mk = Makers { parent: maker_of(&lp.op), ..Makers::default() };
    if side == LONG {
        part.bl = nb;
        part.long_key = ek;
        part.long_ask = 0;
        part.long_min = 0;
        part.long_tip = 0;
        part.mk.long = et;
    } else {
        part.bs = nb;
        part.short_key = ek;
        part.short_bid = 0;
        part.short_min = 0;
        part.short_tip = 0;
        part.mk.short = et;
    }
    let range = q(n, (part.bs - part.bl).max(0), sc, 0);
    let pv = if n == p.size { range + keep.max(p.m.p.carrier) } else { range + p.m.p.carrier };
    (pay, part, pv, keep)
}

fn hand_over_outs(lp: &LPair, side: i64, n: i64, pay: i64, part: &CPair, pv: i64, keep: i64) -> Vec<TransactionOutput> {
    let leaver = if side == LONG { lp.s.long_key } else { lp.s.short_key };
    let mut outs = vec![if pay > 0 { out(pay as u64, key_spk(&leaver), None) } else { change(1) }, out(pv.max(1) as u64, pair_spk(part), None)];
    if n > 0 && n < lp.s.size {
        let rest = lp.s.rest_of(lp.s.size - n);
        outs.push(out(keep.max(rest.floor()) as u64, pair_spk(&rest), bind(0, lp.id.unwrap())));
    }
    outs.push(change(1));
    outs
}

/// Assign: inputs the pair (the leaver signs), the entrant (the executor's key funds). Outputs: the leaver's payout at
/// 0, the part (a fresh id authored by the pair) at 1, the remainder of a split, the change.
pub fn assign_tx(lp: &LPair, side: i64, n: i64, px: i64, nb: i64, signer: &Keypair, seq: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let ek = x(&exec_kp());
    let (pay, part, pv, keep) = handed(lp, side, n, px, nb, ek, [0; 32], 0);
    let mut outs = hand_over_outs(lp, side, n, pay, &part, pv, keep);
    tweak(&mut outs);
    seal(&mut outs, 1, 0, lp.op);
    build_ext(&[pair_in(lp, "assign", vec![Arg::Sig(*signer), iv(side), iv(n), iv(px), iv(nb), bv(&ek), iv(1)]), exec_in(60)], outs, 0, &[seq])
}

/// Transfer of a listed side to a signing taker (the executor's key).
pub fn transfer_tx(lp: &LPair, side: i64, n: i64, px: i64, nb: i64, seq: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let ek = x(&exec_kp());
    let tip = if side == LONG { lp.s.long_tip } else { lp.s.short_tip };
    let (pay, part, pv, keep) = handed(lp, side, n, px, nb, ek, [0; 32], tip);
    let mut outs = hand_over_outs(lp, side, n, pay, &part, pv, keep);
    tweak(&mut outs);
    seal(&mut outs, 1, 0, lp.op);
    build_ext(&[pair_in(lp, "transfer", vec![iv(side), iv(n), iv(px), iv(nb), bv(&ek), bv(&[0; 32]), iv(1)]), exec_in(60)], outs, 0, &[seq])
}

pub fn amend_tx(lp: &LPair, side: i64, ask: i64, min: i64, tip: i64, signer: &Keypair, seq: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let mut next = lp.s.rest_of(lp.s.size);
    if side == LONG {
        (next.long_ask, next.long_min, next.long_tip) = (ask, min, tip);
    } else {
        (next.short_bid, next.short_min, next.short_tip) = (ask, min, tip);
    }
    let mut outs = vec![out(lp.value, pair_spk(&next), bind(0, lp.id.unwrap())), change(1)];
    tweak(&mut outs);
    build_ext(&[pair_in(lp, "amend", vec![Arg::Sig(*signer), iv(side), iv(ask), iv(min), iv(tip)]), exec_in(60)], outs, 0, &[seq])
}

pub fn add_margin_tx(lp: &LPair, side: i64, delta: i64, signer: &Keypair, seq: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let mut next = lp.s.rest_of(lp.s.size);
    if side == LONG {
        next.bl -= delta;
    } else {
        next.bs += delta;
    }
    let add = q(lp.s.size, delta.max(0), lp.s.m.p.scale, lp.s.m.p.scale - 1);
    let mut outs = vec![out((lp.value as i64 + add) as u64, pair_spk(&next), bind(0, lp.id.unwrap())), change(1)];
    tweak(&mut outs);
    build_ext(&[pair_in(lp, "addMargin", vec![Arg::Sig(*signer), iv(side), iv(delta)]), exec_in(60)], outs, 0, &[seq])
}

/// Mutual close of the whole pair at x: the long at output 0, the short (with what the pair holds beyond its range) last.
pub fn mutual_tx(lp: &LPair, px: i64, seq: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let p = &lp.s;
    let sc = p.m.p.scale;
    let lpay = owed(p.size, px - p.bl, p.long_tip, sc);
    let spay = owed(p.size, p.bs - px, p.short_tip, sc) + lp.value as i64 - q(p.size, p.bs - p.bl, sc, 0);
    let mut outs = vec![out(lpay.max(1) as u64, key_spk(&p.long_key), None), change(1), out(spay.max(1) as u64, key_spk(&p.short_key), None)];
    tweak(&mut outs);
    build_ext(&[pair_in(lp, "mutualClose", vec![iv(p.size), iv(px)]), exec_in(60)], outs, 0, &[seq])
}

pub fn coop_tx(lp: &LPair, long: &Keypair, short: &Keypair, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let mut outs = vec![out(lp.value / 2, key_spk(&lp.s.long_key), None), out(lp.value / 2, key_spk(&lp.s.short_key), None)];
    tweak(&mut outs);
    build(&[pair_in(lp, "coopClose", vec![Arg::Sig(*long), Arg::Sig(*short)]), exec_in(60)], outs)
}

/// An order takes a listed side: inputs the pair (transfer), the order (take), the executor. Outputs: the leaver's
/// payout at 0, the order's rest or refund at 1, the part (a fresh id authored by the pair) at 2, the remainder, the
/// change.
pub fn take_tx(lp: &LPair, lo: &LOrder, n: i64, px: i64, lock: u64, seq: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let side = lo.s.side;
    let nb = if side == LONG { px - lo.s.my_rate } else { px + lo.s.my_rate };
    let tip = if side == LONG { lp.s.long_tip } else { lp.s.short_tip };
    let (pay, part, pv, keep) = handed(lp, side, n, px, nb, lo.s.owner, maker_of(&lo.op), tip);
    let leaver = if side == LONG { lp.s.long_key } else { lp.s.short_key };
    let order_out = if n < lo.s.size_left {
        out(lo.value - lo.s.spend(n) as u64, order_spk(&lo.s.after(n)), bind(1, lo.id.unwrap()))
    } else {
        out(lo.value - lo.s.spend(n) as u64, key_spk(&lo.s.owner), None)
    };
    let mut outs = vec![if pay > 0 { out(pay as u64, key_spk(&leaver), None) } else { change(1) }, order_out, out(pv as u64, pair_spk(&part), None)];
    if n < lp.s.size {
        let rest = lp.s.rest_of(lp.s.size - n);
        outs.push(out(keep.max(rest.floor()) as u64, pair_spk(&rest), bind(0, lp.id.unwrap())));
    }
    outs.push(change(1));
    tweak(&mut outs);
    seal(&mut outs, 2, 0, lp.op);
    let [pre, suf] = tmpl(&kit().pair);
    let (stayer, ca, cm, ct, bound) = if side == LONG {
        (lp.s.short_key, lp.s.short_bid, lp.s.short_min, lp.s.short_tip, lp.s.bs)
    } else {
        (lp.s.long_key, lp.s.long_ask, lp.s.long_min, lp.s.long_tip, lp.s.bl)
    };
    let take = vec![iv(2), iv(n), iv(px), iv(lp.s.entry), iv(bound), bv(&stayer), bv(&maker_of(&lp.op)), iv(ca), iv(cm), iv(ct), iv(lp.s.expiry), pre, suf];
    let transfer = vec![iv(side), iv(n), iv(px), iv(nb), bv(&lo.s.owner), bv(&maker_of(&lo.op)), iv(2)];
    build_ext(&[pair_in(lp, "transfer", transfer), order_in(lo, "take", take), exec_in(60)], outs, lock, &[seq])
}
