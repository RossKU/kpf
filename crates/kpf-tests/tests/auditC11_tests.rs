//! Review C11 (docs/audit/REVIEW-C11.md): what an account is when its output carries no covenant id (the object a
//! `cosign` printout shows is named by its script alone), and the close rule of review C10 in the engine. Helpers:
//! cross_tests.rs and auditC9_tests.rs.

#![allow(dead_code, unused_imports)]

mod committeekit;
mod common;

use committeekit::*;
use common::*;
use kaspa_consensus_core::tx::{ScriptPublicKey, TransactionOutput};
use kaspa_consensus_core::Hash;
use kpf_protocol::artifacts::splice;
use kpf_protocol::committee::*;
use kpf_protocol::committee_artifacts::{cross_base, pendx_base, xkit};
use kpf_protocol::cross::*;
use kpf_protocol::state::{KIND_CLOSE, KIND_LIQ, LONG, SHORT};
use secp256k1::Keypair;

const KAS1: i64 = 100_000_000;
const XID: Hash = Hash::from_bytes([0x58; 32]);
const M2: [u8; 32] = [0x77; 32];

type LX = Live<Cross>;
type LW = Live<XPending>;

fn account() -> Cross {
    Cross {
        t: XTerms { term: 8_000, mmr_bps: 500, g_bps: 200, liq_fee_bps: 50, keeper_fee_min: 1_000_000, carrier: 20_000_000, idle_daa: 1_000, grace_daa: 50, late_daa: 100, quiet_daa: 20 },
        legs: [
            XLeg { market: cmarket().id(), committee: rid(), scale: 1_000_000, price_max: 100_000_000_000_000, size: 1_000_000_000, dir: LONG, entry: 1_000_000_000 },
            XLeg { market: M2, committee: rid(), scale: 1_000_000, price_max: 100_000_000_000_000, size: 500_000_000, dir: SHORT, entry: 2_000_000_000 },
        ],
        a_key: x(&long_kp()),
        b_key: x(&short_kp()),
        a_margin: 2_000 * KAS1,
        b_margin: 3_000 * KAS1,
        expiry: 14_000,
    }
}

fn a_kp() -> Keypair {
    long_kp()
}
fn b_kp() -> Keypair {
    short_kp()
}

fn cross_spk(c: &Cross) -> ScriptPublicKey {
    p2sh(&splice(cross_base(), &c.state_bytes()))
}
fn pend_x_spk(w: &XPending) -> ScriptPublicKey {
    p2sh(&splice(pendx_base(), &w.state_bytes()))
}
fn live_x(c: &Cross) -> LX {
    Live { op: outpoint(10), value: c.floor() as u64, daa: 6_100, id: Some(XID), s: *c }
}
fn cross_in(l: &LX, entry: &'static str, args: Vec<Arg>) -> In {
    let art = splice(cross_base(), &l.s.state_bytes());
    In { outpoint: l.op, utxo: kaspa_consensus_core::tx::UtxoEntry::new(l.value, p2sh(&art), l.daa, false, l.id), spend: Spend::Signed { art, entry, args } }
}
fn pend_in(l: &LW, entry: &'static str, args: Vec<Arg>) -> In {
    let art = splice(pendx_base(), &l.s.state_bytes());
    In { outpoint: l.op, utxo: kaspa_consensus_core::tx::UtxoEntry::new(l.value, p2sh(&art), l.daa, false, l.id), spend: Spend::Signed { art, entry, args } }
}

/// The two epoch tables of the epoch [6,000, 7,000): one a leg.
fn epoch_tables() -> [LEpochTable; 2] {
    let t = epoch_epoch_table();
    [t.clone(), Live { op: outpoint(3), ..t }]
}
/// The three largest members sign `p` for the time `t0` of `market`.
fn att_of(market: &[u8; 32], t0: i64, p: i64) -> Attestation {
    let t = epoch_epoch_table().s;
    let s: Vec<SignedPrice> = (0..3).map(|i| sign_price(&member_kp(i), &rid(), market, t0, p)).collect();
    Attestation::collect(&t, &rid(), market, t0, &s).expect("a quorum")
}

/// A claim as it is assembled, for the negatives.
#[derive(Clone)]
struct Claim {
    kind: i64,
    side: i64,
    p: [i64; 2],
    t0: i64,
    atts: [Attestation; 2],
    signer: Option<Keypair>,
    proposer: [u8; 32],
    lock: u64,
    seq: u64,
    u: [i64; 2],
}

impl Claim {
    fn close(c: &Cross, side: i64, p: [i64; 2], t0: i64) -> Claim {
        let kp = if side == SIDE_A { a_kp() } else { b_kp() };
        Claim { kind: KIND_CLOSE, side, p, t0, atts: [att_of(&c.legs[0].market, t0, p[0]), att_of(&c.legs[1].market, t0, p[1])], signer: Some(kp), proposer: x(&kp), lock: t0 as u64, seq: 20, u: [1, 2] }
    }
    fn liq(c: &Cross, side: i64, p: [i64; 2], t0: i64) -> Claim {
        Claim { kind: KIND_LIQ, side, p, t0, atts: [att_of(&c.legs[0].market, t0, p[0]), att_of(&c.legs[1].market, t0, p[1])], signer: None, proposer: x(&exec_kp()), lock: t0 as u64, seq: 0, u: [1, 2] }
    }
}

/// The pending claim whether or not the contract would accept it.
fn pending_forced(c: &Cross, cl: &Claim) -> XPending {
    if let Some(w) = c.pending_of(xkit(), cl.kind, cl.side, cl.p, cl.t0, cl.proposer) {
        return w;
    }
    let who = if cl.kind == KIND_CLOSE { if cl.side == SIDE_A { c.a_key } else { c.b_key } } else { cl.proposer };
    let (amount, add, ch) = if cl.kind == KIND_CLOSE {
        (c.owed_at(cl.side, cl.p), 0, [0u8; 32])
    } else {
        let add = c.cure_add(cl.side, cl.p).max(1);
        (c.fee_of(cl.side, cl.p), add, c.with_margin(cl.side, add).script_hash(xkit()))
    };
    XPending { late_daa: c.t.late_daa, grace_daa: c.t.grace_daa, kind: cl.kind, side: cl.side, t0: cl.t0, amount, cure_add: add, a_key: c.a_key, b_key: c.b_key, proposer: who, restore_hash: c.script_hash(xkit()), cure_hash: ch, mk_parent: [0; 32], account: *c }
}

/// Inputs: the account (propose), an epoch table a leg (attest), the executor. Outputs: the pending claim (a fresh id authored
/// by the account), the two epoch tables, the change.
fn propose_tx(lx: &LX, cl: &Claim, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let w = pending_forced(&lx.s, cl).made_by(&lx.op);
    let ts = epoch_tables();
    let two = lx.s.legs[1].size > 0;
    let mut outs = vec![out(lx.value, pend_x_spk(&w), None), out(ts[0].value, epoch_table_spk(&ts[0].s), bind(1, RID))];
    if two {
        outs.push(out(ts[1].value, epoch_table_spk(&ts[1].s), bind(2, RID)));
    }
    outs.push(change(1));
    tweak(&mut outs);
    seal(&mut outs, 0, 0, lx.op);
    let k = xkit();
    let s = match &cl.signer {
        Some(kp) => Arg::Sig(*kp),
        None => bv(&[0u8; 65]),
    };
    let args = vec![iv(cl.kind), iv(cl.side), iv(cl.p[0]), iv(cl.p[1]), iv(cl.t0), iv(cl.u[0]), iv(cl.u[1]), s, iv(0), bv(&cl.proposer), bv(&k.pending.prefix), bv(&k.pending.suffix)];
    let mut ins = vec![cross_in(lx, "propose", args), attest_in(&ts[0], &cl.atts[0])];
    if two {
        ins.push(attest_in(&ts[1], &cl.atts[1]));
    }
    ins.push(exec_in(60));
    build_ext(&ins, outs, cl.lock, &[cl.seq])
}

/// The pending claim a propose leaves, accepted at DAA `p`.
fn proposed(lx: &LX, cl: &Claim, p: u64) -> LW {
    let t = propose_tx(lx, cl, |_| {});
    must("the propose", &t);
    let w = lx.s.pending_of(xkit(), cl.kind, cl.side, cl.p, cl.t0, cl.proposer).expect("the model accepts what the engine accepted");
    live(&t, 0, p, &w.made_by(&lx.op))
}

/// Finalize: a close pays A at output 0 and B last; a liquidation the proposer's fee at 0 and the winner last; a void
/// claim writes the restored account at output 0.
fn finalize_tx(lw: &LW, seq: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let pay = |v: i64, key: &[u8; 32]| if v > 0 { out(v as u64, key_spk(key), None) } else { change(1) };
    let mut outs = match lw.s.settlement(lw.value, lw.daa as i64) {
        XSettlement::Close { a, b } => {
            let mut o = vec![pay(a, &lw.s.a_key), change(1)];
            if b > 0 {
                o.push(pay(b, &lw.s.b_key));
            }
            o
        }
        XSettlement::Forfeit { fee, winner, rest } => vec![pay(fee, &lw.s.proposer), change(1), pay(rest, &winner)],
        XSettlement::Restore { account, value } => vec![out(value as u64, cross_spk(&account), bind(0, lw.id.unwrap())), change(1)],
    };
    tweak(&mut outs);
    build_ext(&[pend_in(lw, "finalize", vec![]), exec_in(60)], outs, 0, &[seq])
}

fn cure_tx(lw: &LW, signer: &Keypair, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let (acct, value) = lw.s.cure(lw.value).unwrap_or((lw.s.account, lw.value as i64));
    let mut outs = vec![out(value as u64, cross_spk(&acct), bind(0, lw.id.unwrap())), change(1)];
    tweak(&mut outs);
    build_ext(&[pend_in(lw, "cure", vec![Arg::Sig(*signer)]), exec_in(60)], outs, 0, &[])
}

fn add_margin_tx(lx: &LX, side: i64, delta: i64, signer: &Keypair, seq: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let next = lx.s.with_margin(side, delta);
    let mut outs = vec![out((lx.value as i64 + delta.max(0)) as u64, cross_spk(&next), bind(0, XID)), change(1)];
    tweak(&mut outs);
    build_ext(&[cross_in(lx, "addMargin", vec![Arg::Sig(*signer), iv(side), iv(delta)]), exec_in(60)], outs, 0, &[seq])
}

fn keep_alive_tx(lx: &LX, leg: i64, market: &[u8; 32], p: i64, t0: i64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let ts = epoch_tables();
    let next = Cross { expiry: t0 + lx.s.t.term, ..lx.s };
    let mut outs = vec![out(lx.value, cross_spk(&next), bind(0, XID)), out(ts[0].value, epoch_table_spk(&ts[0].s), bind(1, RID)), change(1)];
    tweak(&mut outs);
    build_ext(&[cross_in(lx, "keepAlive", vec![iv(leg), iv(1), iv(p), iv(t0)]), attest_in(&ts[0], &att_of(market, t0, p)), exec_in(60)], outs, t0 as u64, &[])
}

fn lapse_tx(lx: &LX, lock: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let (a, b) = lx.s.lapse_out(lx.value);
    let mut outs = vec![if a > 0 { out(a as u64, key_spk(&lx.s.a_key), None) } else { change(1) }, change(1)];
    if b > 0 {
        outs.push(out(b as u64, key_spk(&lx.s.b_key), None));
    }
    tweak(&mut outs);
    build_ext(&[cross_in(lx, "lapseSettle", vec![]), exec_in(60)], outs, lock, &[])
}

const P0: [i64; 2] = [1_000_000_000, 2_000_000_000];


struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick(&mut self, v: &[i64]) -> i64 {
        v[(self.next() % v.len() as u64) as usize]
    }
    fn one_in(&mut self, n: u64) -> bool {
        self.next() % n == 0
    }
}

/// An account at the edges: sizes of one unit and of 250,000 whole units, entries at 0 and at the price limit, a price
/// limit near the prices, margins from 0 to the model's maximum, mmr 0, g from four spreads to all but one point, a fee
/// of nothing and of everything, a second leg that is none, both legs one market, the same direction twice.
fn random_account(r: &mut Rng) -> Cross {
    let mut c = account();
    for i in 0..2 {
        let pm = r.pick(&[100_000_000_000_000, 3_000_000_000, 1_500_000_000]);
        c.legs[i].price_max = pm;
        c.legs[i].size = r.pick(&[1, 999_999, 1_000_000, 37_000_001, 1_000_000_000, 250_000_000_000]);
        c.legs[i].dir = if r.one_in(2) { LONG } else { SHORT };
        c.legs[i].entry = r.pick(&[0, 1, 500_000_000, 1_000_000_000, 2_000_000_000, pm]).min(pm);
    }
    if r.one_in(5) {
        c.legs[1] = XLeg::none();
    } else if r.one_in(5) {
        c.legs[1].market = c.legs[0].market;
    }
    let margins = [0, 1, 20_000_000, KAS1, 300 * KAS1, 5_000 * KAS1, 2_000_000 * KAS1, MARGIN_MAX];
    c.a_margin = r.pick(&margins);
    c.b_margin = r.pick(&margins);
    c.t.mmr_bps = r.pick(&[0, 500, 500, 2_500]);
    // (g is at least four times the epoch table's spread of 50 bps: below that the account refuses the epoch table, which the
    // model's `pending_of` does not see; see the last lines of the test.)
    c.t.g_bps = r.pick(&[200, 200, 2_000, 9_999]);
    c.t.liq_fee_bps = r.pick(&[0, 50, 50, 10_000]);
    c.t.keeper_fee_min = r.pick(&[0, 1_000_000]);
    c
}

// ------------------------------------------------------------------------------------------------ review C11

/// C11-F1, the engine's half. An account's script on an output that carries no covenant id is spent by nothing: both
/// parties signing together, the lapse, a claim and a top-up are all refused, beside whatever other inputs. With the
/// id the same transactions are accepted. No contract writes an account's first output, so nothing but its builder
/// and its co-signer's reading puts the id there.
#[test]
fn c11_exploit_an_account_script_without_a_covenant_id_is_spent_by_nothing() {
    let c = account();
    let with_id = live_x(&c);
    let without = Live { id: None, ..live_x(&c) };
    let (a, b) = (a_kp(), b_kp());
    let coop = |lx: &LX, other: In| build_ext(&[cross_in(lx, "coopClose", vec![Arg::Sig(a), Arg::Sig(b)]), other], vec![change(1)], 0, &[]);
    must("control: both sign, the account under its id", &coop(&with_id, exec_in(60)));
    must("control: the lapse, the account under its id", &lapse_tx(&with_id, c.lapse_from(), |_| {}));
    refused("both sign, no id", &coop(&without, exec_in(60)), 0);
    refused("the lapse, no id", &lapse_tx(&without, c.lapse_from(), |_| {}), 0);
    refused("a close, no id", &propose_tx(&without, &Claim::close(&c, SIDE_A, P0, 6_500), |_| {}), 0);
    let next = c.with_margin(SIDE_A, KAS1);
    let outs = vec![out(without.value + KAS1 as u64, cross_spk(&next), None), change(1)];
    refused("a top-up, no id", &build_ext(&[cross_in(&without, "addMargin", vec![Arg::Sig(a), iv(SIDE_A), iv(KAS1)]), exec_in(60)], outs, 0, &[20]), 0);
    // Beside an input that carries an id of its own and no other input without one.
    let mut funded = exec_in(61);
    funded.utxo = kaspa_consensus_core::tx::UtxoEntry::new(funded.utxo.amount, funded.utxo.script_public_key.clone(), 0, false, Some(Hash::from_bytes([0x77; 32])));
    refused("both sign, no id, the only input without one", &coop(&without, funded.clone()), 0);
    let (la, lb) = c.lapse_out(without.value);
    let outs = vec![out(la as u64, key_spk(&c.a_key), None), change(1), out(lb as u64, key_spk(&c.b_key), None)];
    refused("the lapse, no id, the only input without one", &build_ext(&[cross_in(&without, "lapseSettle", vec![]), funded], outs, c.lapse_from(), &[]), 0);
}

/// The close rule of review C10, in the engine. Both sides below the maintenance (two legs that offset, both markets
/// at 30): either closes, paid its side at the prices moved by g against it, and neither is liquidated. A side owed
/// nothing, the pot under the maintenance: the party owed everything closes and takes it. A side whose liquidation is
/// due does not close.
#[test]
fn c11_holds_the_close_of_review_c10_in_the_engine() {
    let mut c = account();
    c.legs[1] = XLeg { size: 1_000_000_000, entry: 1_000_000_000, ..c.legs[1] };
    c.a_margin = 1_500 * KAS1;
    c.b_margin = 1_500 * KAS1;
    let p = [30 * KAS1, 30 * KAS1];
    let lx = live_x(&c);
    for side in [SIDE_A, SIDE_B] {
        assert!(!c.liquidatable(side, p));
        refused("a liquidation where neither side is at its maintenance", &propose_tx(&lx, &Claim::liq(&c, side, p, 6_500), |_| {}), 0);
        // Its side at the prices against it: 1,500 less 2 % of 60,000.
        assert_eq!(c.close_pay(side, p), Some(300 * KAS1));
        let lw = proposed(&lx, &Claim::close(&c, side, p, 6_500), 6_510);
        let XSettlement::Close { a, b } = lw.s.settlement(lw.value, 6_510) else { panic!("a close") };
        assert_eq!(if side == SIDE_A { (a, b) } else { (b, a) }, (300 * KAS1, 2_700 * KAS1 + c.t.carrier));
        must("the close where both lack the maintenance", &finalize_tx(&lw, 0, |_| {}));
        refused("the closer paid a sompi more", &finalize_tx(&lw, 0, |o| { let n = o.len(); if side == SIDE_A { o[0].value += 1; o[n - 1].value -= 1; } else { o[n - 1].value += 1; o[0].value -= 1; } }), 0);
    }
    // A owed nothing, the pot (400) under the maintenance (459): B closes and takes all; A's close pays it nothing.
    let mut d = account();
    d.legs[1] = XLeg::none();
    d.a_margin = 200 * KAS1;
    d.b_margin = 200 * KAS1;
    let q = [9 * KAS1, 1];
    let ld = live_x(&d);
    assert!(!d.liquidatable(SIDE_A, q));
    assert_eq!(d.close_pay(SIDE_B, q), Some(400 * KAS1));
    let lw = proposed(&ld, &Claim::close(&d, SIDE_B, q, 6_500), 6_510);
    // (The carrier goes to the party that did not claim: review C9, C9-N4.)
    assert_eq!(lw.s.settlement(lw.value, 6_510), XSettlement::Close { a: d.t.carrier, b: 400 * KAS1 });
    must("the party owed everything closes", &finalize_tx(&lw, 0, |_| {}));
    // With 100 KAS more of B's margin A's liquidation is due: A does not close, and B (at its maintenance) does.
    let e = d.with_margin(SIDE_B, 100 * KAS1);
    let le = live_x(&e);
    assert!(e.liquidatable(SIDE_A, q) && e.close_pay(SIDE_A, q).is_none() && e.close_pay(SIDE_B, q).is_some());
    refused("a side whose liquidation is due closes", &propose_tx(&le, &Claim::close(&e, SIDE_A, q, 6_500), |_| {}), 0);
    must("it is liquidated", &propose_tx(&le, &Claim::liq(&e, SIDE_A, q, 6_500), |_| {}));
}
