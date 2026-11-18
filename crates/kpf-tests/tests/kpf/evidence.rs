//! Transaction builders for the evidence paths of KpfPair (DESIGN 4.5 T4, T6, T7): a block of pair inputs acting
//! on one KOB evidence fill, with the evidence order and its counterparty executed by their real scripts.
//!
//! Layout (DESIGN 4.4; indices are what the builders use, the covenants accept any order with the same rules):
//!   inputs   [0, B) the block of pairs, B the evidence order (KobAsk settle / KobBid fill), B + 1 the token input of
//!            the evidence (the ask's custody, leader of its own transfer; or the seller's tokens to the bid, leader
//!            signed by the seller), then the KpfOpen entrants of P7, then the executor F;
//!   outputs  one positional output per input (pair i: its long or leaver at i; the ask's maker KAS or the bid's
//!            token delivery at B; the buyer's tokens or the seller's KAS at B + 1; each KpfOpen's continuation or
//!            refund; F's change), then the pair continuations, the bid's continuation, and last the short payouts
//!            of the block at M - 1 - i.
//! Every party output holds the least its covenant accepts; F's change takes the rest (less FEE).

use kaspa_consensus_core::tx::{TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use secp256k1::Keypair;
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

use super::kob::*;
use super::paths::*;
use super::*;
use crate::common::*;

/// The lock time of every evidence transaction (what `tx.daa >= x` proves).
pub const NOW: u64 = 10_000_000;

pub fn rest_daa() -> u64 {
    market().min_rest_daa as u64
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

    /// The two inputs (order at `ev`, token input at `ev + 1`), the positional outputs at `ev` and `ev + 1`, and the
    /// extra outputs (the bid's continuation).
    fn leg(&self, ev: usize) -> (Vec<In>, Vec<TransactionOutput>, Vec<TransactionOutput>) {
        let tk = ev + 1;
        let art = self.order_art();
        let nb = ArtifactValue::Bytes(sm8(self.fill).to_vec());
        let tok_base = |t: &Tok, value: u64| UtxoEntry::new(value, p2sh(&t.art()), self.tok_daa, false, Some(self.tok_cov));
        match self.kind {
            Kind::Ask => {
                let a = &self.ask;
                let order = In {
                    outpoint: outpoint(0xa0),
                    utxo: UtxoEntry::new(self.value, p2sh(&art), self.daa, false, Some(self.id)),
                    spend: Spend::Signed { art, entry: "settle", args: vec![Arg::V(nb), iv(tk as i64), iv(tk as i64), iv(self.t())] },
                };
                let custody = Tok::covid(a.amount_left, self.custody_owner.unwrap_or(self.id), self.tok_ext);
                let bought = Tok::p2pk(self.fill, xonly(&self.counter), self.tok_ext);
                let tok_in = In {
                    outpoint: outpoint(0xa1),
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
                    outpoint: outpoint(0xa2),
                    utxo: UtxoEntry::new(self.value, p2sh(&art), self.daa, false, Some(self.id)),
                    spend: Spend::Signed { art: art.clone(), entry: "fill", args: vec![Arg::V(nb), iv(tk as i64), iv(self.t())] },
                };
                let sold = Tok::p2pk(self.fill, xonly(&self.counter), self.tok_ext);
                let delivered = Tok::p2pk(self.fill, b.maker, b.ext);
                let tok_in = In {
                    outpoint: outpoint(0xa3),
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

/// What a pair of the block does.
#[derive(Clone)]
pub enum Act {
    /// P5 by the owner of `side` (signed by `signer`).
    Close { side: i64, signer: Keypair },
    /// P6 of the loser `side`.
    Liq { side: i64 },
    /// P7 of the loser `side`: its n goes to the KpfOpen `open` at x; the part is ordinal k of the continuations.
    LiqTransfer { side: i64, x: i64, open: LiveOpen, k: i64 },
}

#[derive(Clone)]
pub struct PairLeg {
    pub l: Live,
    pub n: i64,
    /// The block start this pair names.
    pub q: i64,
    pub act: Act,
}

/// The keeper fee before its cap: max(keeperFeeMin, q(n, floor(p * liqFeeBps / 10^4))).
pub fn fee_of(m: &Market, n: i64, p: i64) -> i64 {
    q(n, p * m.liq_fee_bps / 10_000, m.scale, 0).max(m.keeper_fee_min)
}

/// The trigger price T of a loser `side` (DESIGN 4.5b), from the pair's own state, clamped to [bl, bs]:
/// floor(bl * 10^4 / (10^4 - mmrBps)) for a long, ceil(bs * 10^4 / (10^4 + mmrBps)) for a short.
pub fn trigger_of(p: &Pair, side: i64) -> i64 {
    let m = p.m.mmr_bps;
    let t = if side == LONG { p.bl * 10_000 / (10_000 - m) } else { (p.bs * 10_000 + 10_000 + m - 1) / (10_000 + m) };
    t.clamp(p.bl, p.bs)
}

/// The maintenance slice of n for a loser `side`: q(n, T - bl) (long), q(n, bs - T) (short).
pub fn slice_of(p: &Pair, n: i64, side: i64) -> i64 {
    let t = trigger_of(p, side);
    if side == LONG { q(n, t - p.bl, p.m.scale, 0) } else { q(n, p.bs - t, p.m.scale, 0) }
}

/// The liquidation fee of P6 and P7 (DESIGN 4.5b): fee_of capped at the maintenance slice.
pub fn liq_fee(p: &Pair, n: i64, quote: i64, side: i64) -> i64 {
    fee_of(&p.m, n, quote).min(slice_of(p, n, side))
}

/// The winner's payoff at the trigger price for n of a loser `side`: q(n, bs - T) for a long loser, q(n, T - bl)
/// for a short loser.
pub fn winner_at_trigger(p: &Pair, n: i64, side: i64) -> i64 {
    let t = trigger_of(p, side);
    if side == LONG { q(n, p.bs - t, p.m.scale, 0) } else { q(n, t - p.bl, p.m.scale, 0) }
}

/// The loser's bankruptcy price: where P6 settles (the loser is owed 0).
pub fn loser_price(p: &Pair, side: i64) -> i64 {
    if side == LONG { p.bl } else { p.bs }
}

/// KpfPair.closeAt: (long payout, short payout, remainder and its least value).
pub fn close_at(l: &Live, n: i64, c: i64, side: i64, fee: i64) -> (i64, i64, Option<(Pair, i64)>) {
    let p = &l.p;
    let v = l.value as i64;
    let part = p.range_of(n);
    let (mut keep, avail) = if n == p.size { (0, v) } else { (v - part, part) };
    let mut owe = if side == LONG { q(n, c - p.bl, p.m.scale, 0) } else { q(n, p.bs - c, p.m.scale, 0) };
    // The fee is capped at the other party's rest: the owed party (the winner of P6) never pays it.
    let mut other = (avail - owe - fee).max(0);
    let rest = if n == p.size {
        if owe < DUST {
            other += owe;
            owe = 0;
        }
        if other < DUST {
            owe += other;
            other = 0;
        }
        if owe < DUST {
            owe = 0;
        }
        None
    } else {
        if owe < DUST {
            keep += owe;
            owe = 0;
        }
        if other < DUST {
            keep += other;
            other = 0;
        }
        let r = p.rest_of(p.size - n);
        let rv = (r.range_of(r.size) + p.m.carrier).max(keep);
        Some((r, rv))
    };
    if side == LONG {
        (owe, other, rest)
    } else {
        (other, owe, rest)
    }
}

/// The settlement price of an evidence quote for a pair: clamped to [bl, bs].
pub fn clamp(p: &Pair, x: i64) -> i64 {
    x.clamp(p.bl, p.bs)
}

/// The entrant of P7 for a loser `side` sold at x: an order of that side with its own rate.
pub fn p7_spec(n: i64, side: i64, x: i64, open: &LiveOpen, k: i64) -> TransferSpec {
    let nb = if side == LONG { x - open.o.my_rate } else { x + open.o.my_rate };
    TransferSpec { side, n, x, nb, ek: open.o.owner, et: open.id, k }
}

/// The winner's payout of P7 before the dust rule (DESIGN 4.5b): the entrant's payment q(n, x - bl) (long loser)
/// or q(n, bs - x) (short loser) less the fee, never below 0.
pub fn p7_pay(l: &Live, n: i64, side: i64, x: i64, p: i64) -> i64 {
    let pr = &l.p;
    let o = if side == LONG { owed(n, x - pr.bl, 0, pr.m.scale) } else { owed(n, pr.bs - x, 0, pr.m.scale) };
    (o - liq_fee(pr, n, p, side)).max(0)
}

/// The winner's key of a loser `side`.
pub fn winner_key(p: &Pair, side: i64) -> [u8; 32] {
    if side == LONG { p.short_key } else { p.long_key }
}

fn nbq(n: i64, qq: i64) -> [Arg; 2] {
    [bv(&sm8(n)), bv(&sm8(qq))]
}

/// The empty signature argument of the paths that read none (P6, P7).
pub fn no_sig() -> Arg {
    bv(&[])
}

/// Arguments of KpfPair.onEvidence: (nb, qb, path, s, side, ev, tk, x, nbd, ek, et, k).
#[allow(clippy::too_many_arguments)]
pub fn ev_args(n: i64, qq: i64, path: i64, s: Arg, side: i64, ev: i64, tk: i64, x: i64, nbd: i64, ek: &[u8; 32], et: Hash, k: i64) -> Vec<Arg> {
    let [a0, a1] = nbq(n, qq);
    vec![a0, a1, iv(path), s, iv(side), iv(ev), iv(tk), iv(x), iv(nbd), bv(ek), bv(&et.as_bytes()), iv(k)]
}

/// Index of the argument x, nbd and path in `ev_args` (for mutations).
pub const ARG_PATH: usize = 2;
pub const ARG_X: usize = 7;
pub const ARG_NBD: usize = 8;

/// The evidence transaction: the block `legs` at inputs [0, B), the evidence at B.
pub fn ev_draft(legs: &[PairLeg], e: &Evidence, f: &Keypair, fn_: u32) -> Draft {
    ev_draft_at(legs, legs.len(), e, f, fn_)
}

/// As `ev_draft` with the evidence at input `ev_at`: legs [0, ev_at) before it, the others after its token input.
pub fn ev_draft_at(legs: &[PairLeg], ev_at: usize, e: &Evidence, f: &Keypair, fn_: u32) -> Draft {
    let b = legs.len();
    let ev = ev_at;
    let tk = ev_at + 1;
    let tk_arg = if e.kind == Kind::Ask { tk as i64 } else { -1 };
    let n_opens = legs.iter().filter(|g| matches!(g.act, Act::LiqTransfer { .. })).count();
    let fi = b + 2 + n_opens;
    let n_in = fi + 1;
    let (ev_ins, ev_pos, ev_extra) = e.leg(ev);
    let p = e.price();
    let at = |j: usize| if j < ev_at { j } else { j + 2 };

    let mut ins: Vec<In> = vec![];
    let mut pos: Vec<TransactionOutput> = vec![];
    let mut extra: Vec<TransactionOutput> = ev_extra;
    let last_pair = if b == 0 { 0 } else { at(b - 1) + 1 };
    let mut tail: Vec<TransactionOutput> = (0..last_pair).map(|_| out(0, p2pk_spk(f), None)).collect();
    let mut open_ins: Vec<In> = vec![];
    let mut open_pos: Vec<TransactionOutput> = vec![];
    let mut pairs = vec![];
    let mut opens = vec![];
    let mut oi = b + 2;
    for (j, g) in legs.iter().enumerate() {
        let i = at(j);
        if j == ev_at {
            ins.extend(ev_ins.clone());
            pos.extend(ev_pos.clone());
        }
        let lp = &g.l.p;
        let z = [0u8; 32];
        match &g.act {
            Act::Close { side, signer } => {
                let (l_pay, s_pay, rest) = close_at(&g.l, g.n, clamp(lp, p), *side, 0);
                let a = ev_args(g.n, g.q, 5, Arg::Sig(*signer), *side, ev as i64, tk_arg, 0, 0, &z, ZERO, 0);
                ins.push(pair_input(&g.l, "onEvidence", a));
                pos.push(out(l_pay as u64, p2pk_x(&lp.long_key), None));
                tail[i] = out(s_pay as u64, p2pk_x(&lp.short_key), None);
                if let Some((r, rv)) = rest {
                    pairs.push((n_in + extra.len(), r.clone()));
                    extra.push(out(rv as u64, p2sh(&r.art()), bind(i as u16, g.l.id)));
                }
            }
            Act::Liq { side } => {
                let (l_pay, s_pay, rest) = close_at(&g.l, g.n, loser_price(lp, *side), *side, liq_fee(lp, g.n, p, *side));
                ins.push(pair_input(&g.l, "onEvidence", ev_args(g.n, g.q, 6, no_sig(), *side, ev as i64, tk_arg, 0, 0, &z, ZERO, 0)));
                pos.push(out(l_pay as u64, p2pk_x(&lp.long_key), None));
                tail[i] = out(s_pay as u64, p2pk_x(&lp.short_key), None);
                if let Some((r, rv)) = rest {
                    pairs.push((n_in + extra.len(), r.clone()));
                    extra.push(out(rv as u64, p2sh(&r.art()), bind(i as u16, g.l.id)));
                }
            }
            Act::LiqTransfer { side, x, open, k } => {
                let t = p7_spec(g.n, *side, *x, open, *k);
                let to = hand_over_out(&g.l, &t, p7_pay(&g.l, g.n, *side, *x, p));
                ins.push(pair_input(&g.l, "onEvidence", ev_args(g.n, g.q, 7, no_sig(), *side, ev as i64, tk_arg, *x, t.nb, &t.ek, t.et, *k)));
                pos.push(out(to.pay as u64, p2pk_x(&winner_key(lp, *side)), None));
                let part_o = out(to.part_value as u64, p2sh(&to.part.art()), bind(i as u16, g.l.id));
                let part_at;
                match &to.rest {
                    None => {
                        part_at = n_in + extra.len();
                        extra.push(part_o);
                    }
                    Some((r, rv)) => {
                        let rest_o = out(*rv as u64, p2sh(&r.art()), bind(i as u16, g.l.id));
                        if *k == 0 {
                            part_at = n_in + extra.len();
                            extra.push(part_o);
                            pairs.push((n_in + extra.len(), r.clone()));
                            extra.push(rest_o);
                        } else {
                            pairs.push((n_in + extra.len(), r.clone()));
                            extra.push(rest_o);
                            part_at = n_in + extra.len();
                            extra.push(part_o);
                        }
                    }
                }
                pairs.push((part_at, to.part.clone()));
                let oa = take_args(&to.part, *side, part_at as i64, g.n, *x);
                open_ins.push(open_input(open, "take", oa));
                let (oo, oc) = order_out(open, g.n, oi as u16);
                open_pos.push(oo);
                if let Some(c) = oc {
                    opens.push((oi, c));
                }
                oi += 1;
            }
        }
    }
    let any_tail = legs.iter().any(|g| !matches!(g.act, Act::LiqTransfer { .. }));
    if ev_at >= b {
        ins.extend(ev_ins);
        pos.extend(ev_pos);
    }
    ins.extend(open_ins);
    pos.extend(open_pos);
    ins.push(funder(f, fn_, F_FUNDS));
    pos.push(out(0, p2pk_spk(f), None));
    assert_eq!(ins.len(), n_in);
    let mut outs = pos;
    outs.extend(extra);
    if any_tail {
        tail.reverse();
        outs.extend(tail);
    }
    let mut d = Draft::new(ins, outs, Some(fi));
    d.lock = NOW;
    d.pairs = pairs;
    d.opens = opens;
    d
}

/// A live pair for the evidence tests (the DESIGN 5 worked example: 1,000 tokens, bl 9, bs 12 KAS).
pub fn live(p: &Pair, tag: u8) -> Live {
    Live { p: p.clone(), value: p.floor() as u64, id: Hash::from_bytes([tag; 32]), op: outpoint(tag) }
}

/// The output index of the short payout of block pair i.
pub fn short_slot(tx_outputs: usize, i: usize) -> usize {
    tx_outputs - 1 - i
}
