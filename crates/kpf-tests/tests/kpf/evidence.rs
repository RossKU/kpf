//! Transaction builders for the two-step evidence paths (DESIGN 4.5d): a block of `KpfPair.propose` and
//! `KpfPending.challenge` inputs acting on one KOB evidence fill, with the evidence order and its counterparty
//! executed by their real scripts, and the evidence-free `KpfPending.cure` and `KpfPending.finalize`.
//!
//! Layout of an evidence transaction (DESIGN 4.4; indices are what the builders use, the covenants accept any order
//! with the same rules):
//!   inputs   [0, B) the block (pairs that propose, pending parts that are challenged), B the evidence order (KobAsk
//!            settle / KobBid fill), B + 1 the token input of the evidence (the ask's custody, leader of its own
//!            transfer; or the seller's tokens to the bid, leader signed by the seller), then the executor F;
//!   outputs  one positional output per input (member i: its pending part, or its restored pair; the ask's maker KAS
//!            or the bid's token delivery at B; the buyer's tokens or the seller's KAS at B + 1; F's change), then the
//!            pair remainders and the bid's continuation.
//! Every party output holds the least its covenant accepts; F's change takes the rest (less FEE): the bonds of the
//! parts a challenge refutes land there.

use kaspa_consensus_core::tx::{TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use secp256k1::Keypair;
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

use super::kob::*;
use super::paths::*;
use super::*;
use crate::common::*;

#[allow(unused_imports)]
pub use kpf_protocol::pending::{bond_of, claim_fee, cure_bound_of, forfeit_bound_of, propose_out, refund_of, ProposeOut};
#[allow(unused_imports)]
pub use kpf_protocol::state::{Pending, KIND_CLOSE, KIND_LIQ};

/// The lock time of every evidence transaction (what `tx.daa >= x` proves).
pub const NOW: u64 = 10_000_000;

pub fn rest_daa() -> u64 {
    market().min_rest_daa as u64
}

/// The window W of the test market (DESIGN 4.5d): the sequence a finalize needs.
pub fn window() -> u64 {
    market().challenge_daa as u64
}

/// The default proposer of a liquidation (its payout key).
pub fn proposer_kp() -> Keypair {
    keypair(77)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Ask,
    Bid,
}

/// One evidence fill: the order, its token input and its counterparty.
#[derive(Clone)]
pub struct Evidence {
    pub kind: Kind,
    pub ask: Ask,
    pub bid: Bid,
    /// A template that replaces the order's (a look-alike).
    pub art: Option<SilAbiArtifact>,
    pub id: Hash,
    pub value: u64,
    pub daa: u64,
    /// n_ev, the fill. An ask is filled out (`amountLeft = fill`).
    pub fill: i64,
    pub tok_daa: u64,
    pub tok_value: u64,
    /// Covenant id and extension commitment of the token UTXOs of the evidence.
    pub tok_cov: Hash,
    pub tok_ext: Hash,
    /// Owner of the ask's custody (the ask's own covenant id when None).
    pub custody_owner: Option<Hash>,
    pub maker: Keypair,
    /// The buyer of an ask, the seller to a bid.
    pub counter: Keypair,
}

const ORDER_CARRIER: u64 = 1_000_000_000; // 10 KAS, as KOB's fixtures
const TOKEN_CARRIER: u64 = 20_000_000;

impl Evidence {
    pub fn ask(price: i64, fill: i64) -> Evidence {
        let m = market();
        let maker = keypair(41);
        Evidence {
            kind: Kind::Ask,
            ask: ask_of(&m, xonly(&maker), price, fill),
            bid: bid_of(&m, xonly(&maker), price),
            art: None,
            id: Hash::from_bytes([0xe1; 32]),
            value: ORDER_CARRIER,
            daa: NOW - rest_daa() - 50,
            fill,
            tok_daa: NOW - rest_daa() - 50,
            tok_value: TOKEN_CARRIER,
            tok_cov: m.token_cov_id,
            tok_ext: m.ext,
            custody_owner: None,
            maker,
            counter: keypair(42),
        }
    }

    pub fn bid(price: i64, fill: i64) -> Evidence {
        let mut e = Evidence::ask(price, fill);
        e.kind = Kind::Bid;
        e.id = Hash::from_bytes([0xe2; 32]);
        e.ask.amount_left = fill;
        let b = &e.bid;
        let used = q(fill, b.price + b.tip, b.scale, b.scale - 1);
        let room = q(b.min_fill, b.price + b.tip, b.scale, b.scale - 1);
        e.value = (used + 2 * b.delivery_carrier + room + 100_000_000) as u64;
        e
    }

    /// A bid of time in force `tif` (1 IOC, 2 FOK) funded for exactly this fill, so the fill ends it.
    pub fn ending_bid(price: i64, fill: i64, tif: i64) -> Evidence {
        let mut e = Evidence::bid(price, fill);
        e.bid.tif = tif;
        let b = &e.bid;
        e.value = (q(fill, b.price + b.tip, b.scale, b.scale - 1) + b.delivery_carrier) as u64;
        e
    }

    pub fn price(&self) -> i64 {
        match self.kind {
            Kind::Ask => self.ask.price,
            Kind::Bid => self.bid.price,
        }
    }

    /// The last DAA at which conforming matchers fill this order (`KpfPair.propose` writes it as the pending part's
    /// visibleUntil, DESIGN 4.5f): the earliest of its expiryDaa, its IOC/FOK kill and its 90-day idle limit.
    pub fn visible_until(&self) -> i64 {
        let (exp, tif, act) = match self.kind {
            Kind::Ask => (self.ask.expiry_daa, self.ask.tif, self.ask.active_from),
            Kind::Bid => (self.bid.expiry_daa, self.bid.tif, self.bid.active_from),
        };
        let ud = self.daa as i64;
        let mut v = exp.min(ud + 77_760_000);
        if tif != 0 {
            v = v.min(ud.max(act) + 600);
        }
        v
    }

    /// This order with another covenant id, outpoint tag and maker: a second, independent order.
    pub fn other(mut self, tag: u8) -> Evidence {
        self.id = Hash::from_bytes([tag; 32]);
        self
    }

    /// The order and its token input placed at DAA `daa`.
    pub fn placed(mut self, daa: u64) -> Evidence {
        self.daa = daa;
        self.tok_daa = daa;
        self
    }

    fn order_art(&self) -> SilAbiArtifact {
        match (&self.art, self.kind) {
            (Some(a), Kind::Ask) => splice(a, &self.ask.state_bytes()),
            (Some(a), Kind::Bid) => splice(a, &self.bid.state_bytes()),
            (None, Kind::Ask) => self.ask.art(),
            (None, Kind::Bid) => self.bid.art(),
        }
    }

    /// The decay time argument t: the origin of a decaying order (its price then is `price`).
    fn t(&self) -> i64 {
        match self.kind {
            Kind::Ask => self.ask.active_from,
            Kind::Bid => self.bid.active_from,
        }
    }

    fn op_tag(&self) -> u8 {
        self.id.as_bytes()[0]
    }

    /// The two inputs (order at `ev`, token input at `ev + 1`), the positional outputs at `ev` and `ev + 1`, and the
    /// extra outputs (the bid's continuation).
    fn leg(&self, ev: usize) -> (Vec<In>, Vec<TransactionOutput>, Vec<TransactionOutput>) {
        let tk = ev + 1;
        let art = self.order_art();
        let nb = ArtifactValue::Bytes(sm8(self.fill).to_vec());
        let tok_base = |t: &Tok, value: u64| UtxoEntry::new(value, p2sh(&t.art()), self.tok_daa, false, Some(self.tok_cov));
        let tag = self.op_tag();
        match self.kind {
            Kind::Ask => {
                let a = &self.ask;
                let order = In {
                    outpoint: outpoint(tag.wrapping_add(0x10)),
                    utxo: UtxoEntry::new(self.value, p2sh(&art), self.daa, false, Some(self.id)),
                    spend: Spend::Signed { art, entry: "settle", args: vec![Arg::V(nb), iv(tk as i64), iv(tk as i64), iv(self.t())] },
                };
                let custody = Tok::covid(a.amount_left, self.custody_owner.unwrap_or(self.id), self.tok_ext);
                let bought = Tok::p2pk(self.fill, xonly(&self.counter), self.tok_ext);
                let tok_in = In {
                    outpoint: outpoint(tag.wrapping_add(0x11)),
                    utxo: tok_base(&custody, self.tok_value),
                    spend: Spend::Signed {
                        art: custody.art(),
                        entry: "transfer",
                        args: vec![Arg::V(ArtifactValue::Array(vec![bought.value()])), Arg::V(ArtifactValue::Bytes(vec![0x00]))],
                    },
                };
                let all_in = q(self.fill, a.price - a.tip, a.scale, a.scale - 1) as u64;
                let maker_out = out(all_in + self.value + self.tok_value, p2pk_spk(&self.maker), None);
                let tok_out = out(self.tok_value, p2sh(&bought.art()), bind(tk as u16, self.tok_cov));
                (vec![order, tok_in], vec![maker_out, tok_out], vec![])
            }
            Kind::Bid => {
                let b = &self.bid;
                let order = In {
                    outpoint: outpoint(tag.wrapping_add(0x12)),
                    utxo: UtxoEntry::new(self.value, p2sh(&art), self.daa, false, Some(self.id)),
                    spend: Spend::Signed { art: art.clone(), entry: "fill", args: vec![Arg::V(nb), iv(tk as i64), iv(self.t())] },
                };
                let sold = Tok::p2pk(self.fill, xonly(&self.counter), self.tok_ext);
                let delivered = Tok::p2pk(self.fill, b.maker, b.ext);
                let tok_in = In {
                    outpoint: outpoint(tag.wrapping_add(0x13)),
                    utxo: tok_base(&sold, self.tok_value),
                    spend: Spend::Signed {
                        art: sold.art(),
                        entry: "transfer",
                        args: vec![Arg::V(ArtifactValue::Array(vec![delivered.value()])), Arg::LeaderSig(self.counter)],
                    },
                };
                let used = q(self.fill, b.price + b.tip, b.scale, b.scale - 1);
                let all_in = q(self.fill, b.price + b.tip, b.scale, 0);
                let left = self.value as i64 - used;
                let delivery = out((b.delivery_carrier + used - all_in) as u64, p2sh(&delivered.art()), bind(tk as u16, self.tok_cov));
                let seller = out(all_in as u64, p2pk_spk(&self.counter), None);
                if b.tif != 0 {
                    // An IOC/FOK bid ends with its fill (KobBid: no continuation); built with `ending_bid`, so the
                    // KAS left beyond the delivery is below KOB's refund minimum and rides on the delivery.
                    let delivery = out((self.value as i64 - all_in) as u64, p2sh(&delivered.art()), bind(tk as u16, self.tok_cov));
                    return (vec![order, tok_in], vec![delivery, seller], vec![]);
                }
                let cont = out((left - b.delivery_carrier) as u64, p2sh(&art), bind(ev as u16, self.id));
                (vec![order, tok_in], vec![delivery, seller], vec![cont])
            }
        }
    }
}

/// What a pair of a proposal block claims.
#[derive(Clone)]
pub enum Act {
    /// P5, kind 1, by the owner of `side` (signed by `signer`).
    Close { side: i64, signer: Keypair },
    /// P6, kind 2, of the loser `side`; the proposer is `proposer_kp()`.
    Liq { side: i64 },
    /// P6 with an explicit proposer key.
    LiqBy { side: i64, proposer: [u8; 32] },
}

impl Act {
    pub fn kind_side(&self) -> (i64, i64) {
        match self {
            Act::Close { side, .. } => (KIND_CLOSE, *side),
            Act::Liq { side } | Act::LiqBy { side, .. } => (KIND_LIQ, *side),
        }
    }
    pub fn proposer(&self) -> [u8; 32] {
        match self {
            Act::LiqBy { proposer, .. } => *proposer,
            _ => xonly(&proposer_kp()),
        }
    }
}

#[derive(Clone)]
pub struct PairLeg {
    pub l: Live,
    pub n: i64,
    /// The block start this pair names.
    pub q: i64,
    pub act: Act,
}

/// A pending part of a challenge block.
#[derive(Clone)]
pub struct PendLeg {
    pub w: LivePend,
    /// The block start this part names.
    pub q: i64,
}

/// A member of an evidence block.
#[derive(Clone)]
pub enum Member {
    P(PairLeg),
    W(PendLeg),
    /// A lock that is disputed (DESIGN 4.5f), naming block start `q`.
    L(LiveLk, i64),
}

#[allow(unused_imports)]
pub use kpf_protocol::payoff::{fee_of, liq_fee, slice_of, trigger_of, winner_at_trigger, winner_key};

/// The settlement price of an evidence quote for a pair: clamped to [bl, bs].
pub fn clamp(p: &Pair, x: i64) -> i64 {
    kpf_protocol::payoff::clamp_price(p, x)
}

/// The empty signature argument of a liquidation proposal.
pub fn no_sig() -> Arg {
    bv(&[])
}

/// Arguments of KpfPair.propose: (nb, qb, kind, s, side, ev, tk, fb, pk, k, offer, pPre, pSuf), with no offer.
#[allow(clippy::too_many_arguments)]
pub fn propose_args(n: i64, qq: i64, kind: i64, s: Arg, side: i64, ev: i64, tk: i64, fb: i64, pk: &[u8; 32], k: i64) -> Vec<Arg> {
    let t = kpf_protocol::artifacts::pending_template();
    vec![bv(&sm8(n)), bv(&sm8(qq)), iv(kind), s, iv(side), iv(ev), iv(tk), iv(fb), bv(pk), iv(k), iv(0), bv(&t.prefix), bv(&t.suffix)]
}

/// Arguments of KpfPending.challenge: (nb, qb, ev, tk).
pub fn challenge_args(n: i64, qq: i64, ev: i64, tk: i64) -> Vec<Arg> {
    vec![bv(&sm8(n)), bv(&sm8(qq)), iv(ev), iv(tk)]
}

/// Index of the arguments of `propose_args` (for mutations).
pub const ARG_KIND: usize = 2;
pub const ARG_SIDE: usize = 4;
pub const ARG_FB: usize = 7;
pub const ARG_PK: usize = 8;
pub const ARG_K: usize = 9;
pub const ARG_OFFER: usize = 10;
pub const ARG_PRE: usize = 11;

/// The pending part (with its least value) and remainder a proposal leg writes, on the evidence fixtures' visibility
/// (a GTC order of DAA `NOW - R - 50`: visible until that DAA + 90 days) and with no offer.
pub fn propose_of(g: &PairLeg, quote: i64) -> ProposeOut {
    propose_of_on(g, quote, fixture_visible_until())
}

/// [`propose_of`] on evidence visible until `vis` (what `KpfPair.propose` writes as visibleUntil).
pub fn propose_of_on(g: &PairLeg, quote: i64, vis: i64) -> ProposeOut {
    let (kind, side) = g.act.kind_side();
    propose_out(&g.l.clocked(), g.l.value, g.n, kind, side, quote, g.act.proposer(), vis, 0)
}

/// The evidence transaction: the pair block `legs` at inputs [0, B), the evidence at B.
pub fn ev_draft(legs: &[PairLeg], e: &Evidence, f: &Keypair, fn_: u32) -> Draft {
    let m: Vec<Member> = legs.iter().cloned().map(Member::P).collect();
    blk_draft_at(&m, m.len(), e, f, fn_)
}

/// As `ev_draft` with the evidence at input `ev_at`: legs [0, ev_at) before it, the others after its token input.
pub fn ev_draft_at(legs: &[PairLeg], ev_at: usize, e: &Evidence, f: &Keypair, fn_: u32) -> Draft {
    let m: Vec<Member> = legs.iter().cloned().map(Member::P).collect();
    blk_draft_at(&m, ev_at, e, f, fn_)
}

/// A challenge of the pending parts `ws` (each naming block start 0) on the counter-evidence `e`.
pub fn ch_draft(ws: &[LivePend], e: &Evidence, f: &Keypair, fn_: u32) -> Draft {
    let m: Vec<Member> = ws.iter().map(|w| Member::W(PendLeg { w: w.clone(), q: 0 })).collect();
    blk_draft_at(&m, m.len(), e, f, fn_)
}

/// Any block of proposals and challenges on one evidence.
pub fn blk_draft(members: &[Member], e: &Evidence, f: &Keypair, fn_: u32) -> Draft {
    blk_draft_at(members, members.len(), e, f, fn_)
}

pub fn blk_draft_at(members: &[Member], ev_at: usize, e: &Evidence, f: &Keypair, fn_: u32) -> Draft {
    let b = members.len();
    let ev = ev_at;
    let tk = ev_at + 1;
    let tk_arg = if e.kind == Kind::Ask { tk as i64 } else { -1 };
    let fi = b + 2;
    let n_in = fi + 1;
    let (ev_ins, ev_pos, ev_extra) = e.leg(ev);
    let p = e.price();
    let at = |j: usize| if j < ev_at { j } else { j + 2 };

    let mut ins: Vec<In> = vec![];
    let mut pos: Vec<TransactionOutput> = vec![];
    let mut extra: Vec<TransactionOutput> = vec![];
    // Reverse slots (by input index): the burn of a disputed lock lands at M - 1 - i.
    let mut tail: Vec<Option<TransactionOutput>> = vec![];
    let mut pairs = vec![];
    let mut pendings = vec![];
    for (j, g) in members.iter().enumerate() {
        let i = at(j);
        if j == ev_at {
            ins.extend(ev_ins.clone());
            pos.extend(ev_pos.clone());
        }
        match g {
            Member::P(g) => {
                let (kind, side) = g.act.kind_side();
                let s = match &g.act {
                    Act::Close { signer, .. } => Arg::Sig(*signer),
                    _ => no_sig(),
                };
                let po = propose_of_on(g, p, e.visible_until());
                let a = propose_args(g.n, g.q, kind, s, side, ev as i64, tk_arg, po.fb, &g.act.proposer(), 0);
                ins.push(pair_input(&g.l, "propose", a));
                pendings.push((i, po.pending.clone()));
                // A malformed pair (a negative n, a bound beyond priceMax) gives nonsense values; they are capped so the
                // draft can be built at all (the pair input fails on its own).
                pos.push(out(cap(po.pending_value, g.l.value), p2sh(&po.pending.art()), bind(i as u16, g.l.id)));
                if let Some((r, rv)) = po.rest {
                    pairs.push((n_in + extra.len(), r.clone()));
                    extra.push(out(cap(rv, g.l.value), p2sh(&r.art()), bind(i as u16, g.l.id)));
                }
            }
            Member::W(g) => {
                let r = g.w.w.restore_pair().expect("restore state matches its hash");
                ins.push(pending_input(&g.w, "challenge", challenge_args(g.w.w.n, g.q, ev as i64, tk_arg)));
                pairs.push((i, r.clone()));
                pos.push(out(g.w.w.restore_value(g.w.value) as u64, p2sh(&r.art()), bind(i as u16, g.w.id)));
            }
            Member::L(l, q) => {
                ins.push(lock_input(l, "dispute", challenge_args(l.l.n, *q, ev as i64, tk_arg)));
                let pay = l.l.victim_pay(p);
                pos.push(if pay > 0 { out(pay as u64, p2pk_x(&l.l.victim), None) } else { out(1_000_000, p2pk_spk(f), None) });
                let burn = l.l.burn(l.value, p);
                if burn > 0 {
                    while tail.len() <= i {
                        tail.push(None);
                    }
                    tail[i] = Some(out(burn as u64, kpf_protocol::lock::burn_spk(), None));
                }
            }
        }
    }
    if ev_at >= b {
        ins.extend(ev_ins);
        pos.extend(ev_pos);
    }
    ins.push(funder(f, fn_, F_FUNDS));
    pos.push(out(0, p2pk_spk(f), None));
    assert_eq!(ins.len(), n_in);
    let mut outs = pos;
    outs.extend(extra);
    outs.extend(ev_extra);
    if !tail.is_empty() {
        while outs.len() < n_in {
            outs.push(out(1_000_000, p2pk_spk(f), None));
        }
        for t in tail.into_iter().rev() {
            outs.push(t.unwrap_or_else(|| out(1_000_000, p2pk_spk(f), None)));
        }
    }
    let mut d = Draft::new(ins, outs, Some(fi));
    d.lock = NOW;
    d.pairs = pairs;
    d.pendings = pendings;
    d
}

fn cap(v: i64, pair_value: u64) -> u64 {
    v.clamp(0, (pair_value + F_FUNDS / 4).min(i64::MAX as u64) as i64) as u64
}

/// A live pair for the evidence tests (the DESIGN 5 worked example: 1,000 tokens, bl 9, bs 12 KAS).
pub fn live(p: &Pair, tag: u8) -> Live {
    Live { p: p.clone(), value: p.floor() as u64, id: Hash::from_bytes([tag; 32]), op: outpoint(tag), daa: 0 }
}

/// The pending part a proposal leg creates, as a live UTXO of DAA score `daa` (the clock P of its challenges).
pub fn pend_of(g: &PairLeg, quote: i64, daa: u64) -> LivePend {
    let po = propose_of(g, quote);
    LivePend { w: po.pending, value: po.pending_value as u64, id: g.l.id, op: outpoint(g.l.op.index as u8 ^ 0x80), daa }
}

/// The pending part of a proposal leg at the DAA a challenge at `NOW` can count a counter order placed now from:
/// P = NOW - R - 10 (its counter-evidence fixtures rest from before P).
pub fn pend(g: &PairLeg, quote: i64) -> LivePend {
    pend_of(g, quote, NOW - rest_daa() - 10)
}

// ---------------------------------------------------------------- cure and finalize

/// Cure: in 0 the pending part (signed by `signer`), 1 F. out 0 the proposer's refund, 1 the cured pair, 2 change.
pub fn cure_draft(w: &LivePend, signer: &Keypair, f: &Keypair, fn_: u32) -> Draft {
    let c = w.w.cure_pair().expect("cure state matches its hash");
    let ins = vec![pending_input(w, "cure", vec![Arg::Sig(*signer)]), funder(f, fn_, F_FUNDS)];
    let outs = vec![
        out(w.w.refund as u64, p2pk_x(&w.w.proposer_key), None),
        out(w.w.cure_value(w.value) as u64, p2sh(&c.art()), bind(0, w.id)),
        out(0, p2pk_spk(f), None),
    ];
    let mut d = Draft::new(ins, outs, Some(2));
    d.pairs.push((1, c));
    d
}

/// Finalize at age W (the sequence). A close: out 0 the long, then change, then the short at M - 1. A liquidation:
/// out 0 the proposer, 1 the forfeited pair, 2 change.
pub fn finalize_draft(w: &LivePend, f: &Keypair, fn_: u32) -> Draft {
    let ins = vec![pending_input(w, "finalize", vec![]), funder(f, fn_, F_FUNDS)];
    let mut d;
    if w.w.kind == KIND_CLOSE {
        let (lp, sp) = w.w.finalize_close(w.value);
        let mut outs = vec![out(lp as u64, p2pk_x(&w.w.long_key), None), out(0, p2pk_spk(f), None)];
        if sp > 0 {
            outs.push(out(sp as u64, p2pk_x(&w.w.short_key), None));
        }
        d = Draft::new(ins, outs, Some(1));
    } else {
        let fp = w.w.forfeit_pair().expect("forfeit state matches its hash");
        let (fv, pay) = w.w.finalize_forfeit(w.value);
        let outs = vec![out(pay as u64, p2pk_x(&w.w.proposer_key), None), out(fv as u64, p2sh(&fp.art()), bind(0, w.id)), out(0, p2pk_spk(f), None)];
        d = Draft::new(ins, outs, Some(2));
        d.pairs.push((1, fp));
    }
    d.seqs = vec![window(), 0];
    d
}

/// The visibleUntil of the default evidence fixtures (a GTC order of DAA `NOW - R - 50`, 90 days idle).
pub fn fixture_visible_until() -> i64 {
    Evidence::ask(1, 1).visible_until()
}

// ---------------------------------------------------------------- guaranteed fast settlement (DESIGN 4.5f)

/// The default guarantor (owner of the lock).
pub fn guarantor_kp() -> Keypair {
    keypair(88)
}

/// The cure grace G of the test market: the sequence a guarantee of a liquidation needs.
pub fn grace() -> u64 {
    market().grace_daa as u64
}

/// The dispute window D of the test market: the sequence a release needs.
pub fn dispute_window() -> u64 {
    market().dispute_daa as u64
}

/// Arguments of KpfPending.guarantee: (j, gk, lPre, lSuf).
pub fn guarantee_args(j: i64, gk: &[u8; 32]) -> Vec<Arg> {
    let t = kpf_protocol::artifacts::lock_template();
    vec![iv(j), bv(gk), bv(&t.prefix), bv(&t.suffix)]
}

/// Guarantee of the pending part `w` by `gk`, the lock holding exactly its least value. A close: out 0 the long, 1 the
/// lock, 2 change, then the short at M - 1. A liquidation: out 0 the proposer, 1 the forfeited pair, 2 the lock, 3
/// change. A liquidation's input carries the grace as its sequence.
pub fn guarantee_draft(w: &LivePend, gk: &[u8; 32], f: &Keypair, fn_: u32) -> Draft {
    let lock = w.w.lock_of(*gk, w.id, w.daa, w.value);
    let lv = w.w.lock_min(w.value) as u64;
    let mut d;
    if w.w.kind == KIND_CLOSE {
        let (lp, sp) = w.w.guarantee_close(w.value);
        let ins = vec![pending_input(w, "guarantee", guarantee_args(1, gk)), funder(f, fn_, F_FUNDS)];
        let mut outs = vec![out(lp.max(0) as u64, p2pk_x(&w.w.long_key), None), out(lv, p2sh(&lock.art()), None), out(0, p2pk_spk(f), None)];
        if sp > 0 {
            outs.push(out(sp as u64, p2pk_x(&w.w.short_key), None));
        }
        d = Draft::new(ins, outs, Some(2));
        d.locks.push((1, lock));
    } else {
        let fp = w.w.forfeit_pair().expect("forfeit state matches its hash");
        let (fv, pay) = w.w.guarantee_forfeit(w.value);
        let ins = vec![pending_input(w, "guarantee", guarantee_args(2, gk)), funder(f, fn_, F_FUNDS)];
        let outs = vec![
            out(pay as u64, p2pk_x(&w.w.proposer_key), None),
            out(fv as u64, p2sh(&fp.art()), bind(0, w.id)),
            out(lv, p2sh(&lock.art()), None),
            out(0, p2pk_spk(f), None),
        ];
        d = Draft::new(ins, outs, Some(3));
        d.pairs.push((1, fp));
        d.locks.push((2, lock));
        d.seqs = vec![grace(), 0];
    }
    d
}

/// The disputes of the locks `ls` (each naming block start 0) on the counter-evidence `e`.
pub fn dp_draft(ls: &[LiveLk], e: &Evidence, f: &Keypair, fn_: u32) -> Draft {
    let m: Vec<Member> = ls.iter().map(|l| Member::L(l.clone(), 0)).collect();
    blk_draft_at(&m, m.len(), e, f, fn_)
}

/// Release at age D: in 0 the lock, 1 F. out 0 the owner (the whole value), 1 change.
pub fn release_draft(l: &LiveLk, f: &Keypair, fn_: u32) -> Draft {
    let ins = vec![lock_input(l, "release", vec![]), funder(f, fn_, F_FUNDS)];
    let outs = vec![out(l.value, p2pk_x(&l.l.owner), None), out(0, p2pk_spk(f), None)];
    let mut d = Draft::new(ins, outs, Some(1));
    d.seqs = vec![dispute_window(), 0];
    d
}

/// The lock a guarantee of `w` writes, as a live UTXO of DAA score `daa` (F).
pub fn lock_of(w: &LivePend, gk: &[u8; 32], daa: u64) -> LiveLk {
    LiveLk { l: w.w.lock_of(*gk, w.id, w.daa, w.value), value: w.w.lock_min(w.value) as u64, op: outpoint(w.op.index as u8 ^ 0x40), daa }
}
