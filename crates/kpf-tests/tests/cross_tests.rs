//! The cross-margin account in the script engine (docs/DESIGN-COMMITTEE.md 3b): one UTXO between A and B with two legs
//! and one margin, priced by two epoch tables for one time. States and values come from the library model
//! (`kpf_protocol::cross`); every accepted transaction pays exactly what the model says and one sompi more is refused.
//!
//! Fixture: mmr 5 %, g 2 %, fee 0.5 %, a claim valid within 100 DAA of its time, 50 DAA to cure, 20 DAA of quiet, a
//! lease of 8,000 DAA. The account: A is long 1,000 units of market 1 from 10 KAS and short 500 units of market 2 from
//! 20 KAS (10,000 KAS of notional each); A's margin 2,000 KAS, B's 3,000 KAS; accepted at 6,100, lease 14,000.

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

#[test]
fn the_states_are_the_models_bytes() {
    let c = account();
    assert_eq!(c.state_bytes().len(), CROSS_STATE_LEN);
    assert_eq!((xkit().cross.prefix.len(), xkit().cross.state_len), (1, CROSS_STATE_LEN));
    assert_eq!(Cross::decode_state(&c.state_bytes()).unwrap(), c);
    assert_eq!(Cross::decode_state(xkit().cross.state_of(&c.redeem(xkit())).unwrap()).unwrap(), c);
    let w = c.pending_of(xkit(), KIND_LIQ, SIDE_A, [1_000_000_000, 2_400_000_000], 6_500, [9; 32]).unwrap();
    assert_eq!(w.state_bytes().len(), XPENDING_STATE_LEN);
    assert_eq!(xkit().pending.state_len, XPENDING_STATE_LEN);
    assert_eq!(XPending::decode_state(&w.state_bytes()).unwrap(), w);
    assert!(w.consistent(xkit()));
    c.check().unwrap();
}

#[test]
fn a_gain_of_one_leg_pays_for_a_loss_of_the_other() {
    let c = account();
    let lx = live_x(&c);
    // At the entries A is owed its margin; a rise of market 1 by 1 KAS is +1,000 KAS, of market 2 by 1 KAS -500 KAS.
    assert_eq!((c.a_at(P0), c.owed_at(SIDE_B, P0)), (2_000 * KAS1, 3_000 * KAS1));
    assert_eq!(c.a_at([1_100_000_000, 2_100_000_000]), 2_500 * KAS1);
    // Market 2 at 24 alone would cost A 2,000 KAS, its whole margin: with market 1 where it was, A is liquidated.
    let alone = [1_000_000_000, 2_400_000_000];
    assert_eq!(c.a_at(alone), 0);
    assert!(c.liquidatable(SIDE_A, alone));
    must("market 2 at 24, market 1 at 10: A is liquidated", &propose_tx(&lx, &Claim::liq(&c, SIDE_A, alone, 6_500), |_| {}));
    // With market 1 at 12 the long leg has gained the same 2,000 KAS: A is owed its margin and nobody is liquidated.
    let both = [1_200_000_000, 2_400_000_000];
    assert_eq!(c.a_at(both), 2_000 * KAS1);
    assert!(!c.liquidatable(SIDE_A, both) && !c.liquidatable(SIDE_B, both));
    refused("both markets up by a fifth: A is not liquidated", &propose_tx(&lx, &Claim::liq(&c, SIDE_A, both, 6_500), |_| {}), 0);
    refused("nor B", &propose_tx(&lx, &Claim::liq(&c, SIDE_B, both, 6_500), |_| {}), 0);
    // What A is owed never leaves [0, pot]: beyond it the other side is bankrupt.
    assert_eq!(c.a_at([1_300_000_000, 1_600_000_000]), 5_000 * KAS1);
    assert!(c.liquidatable(SIDE_B, [1_300_000_000, 1_600_000_000]));
    must("market 1 up, market 2 down: B is liquidated", &propose_tx(&lx, &Claim::liq(&c, SIDE_B, [1_300_000_000, 1_600_000_000], 6_500), |_| {}));
}

#[test]
fn a_close_at_the_attested_prices_settles_at_once() {
    let c = account();
    let lx = live_x(&c);
    // A closes at (11, 21). Against A: market 1 at floor(11 x 0.98) = 10.78 (+780 KAS), market 2 at ceil(21 x 1.02) =
    // 21.42 (-710 KAS): A is paid 2,070 KAS, B the rest.
    let p = [1_100_000_000, 2_100_000_000];
    assert_eq!(c.close_pay(SIDE_A, p), Some(2_070 * KAS1));
    let cl = Claim::close(&c, SIDE_A, p, 6_500);
    let lw = proposed(&lx, &cl, 6_510);
    assert_eq!((lw.s.amount, lw.s.proposer, lw.s.cure_add, lw.s.cure_hash), (2_070 * KAS1, c.a_key, 0, [0; 32]));
    assert_eq!(lw.s.settlement(lw.value, 6_510), XSettlement::Close { a: 2_070 * KAS1, b: 2_930 * KAS1 + 20_000_000 });
    must("finalize at once", &finalize_tx(&lw, 0, |_| {}));
    refused("A one sompi more leaves B short", &finalize_tx(&lw, 0, |o| { o[0].value += 1; o[2].value -= 1; }), 0);
    refused("A one sompi less", &finalize_tx(&lw, 0, |o| o[0].value -= 1), 0);
    refused("A's payout to another key", &finalize_tx(&lw, 0, |o| o[0].script_public_key = key_spk(&x(&exec_kp()))), 0);
    refused("B's payout to another key", &finalize_tx(&lw, 0, |o| o[2].script_public_key = key_spk(&x(&exec_kp()))), 0);
    refused("a valid close restored", &finalize_tx(&lw, 0, |o| *o = vec![out(lw.value, cross_spk(&c), bind(0, lw.id.unwrap())), change(1)]), 0);
    refused("a close is not cured", &cure_tx(&lw, &a_kp(), |_| {}), 0);
    // Landed 101 DAA after its time it is void: the account is restored whole.
    let late = proposed(&lx, &cl, 6_601);
    assert_eq!(late.s.settlement(late.value, 6_601), XSettlement::Restore { account: c, value: lx.value as i64 });
    must("a void close restores the account", &finalize_tx(&late, 0, |_| {}));
    refused("restored with one sompi less", &finalize_tx(&late, 0, |o| o[0].value -= 1), 0);
    refused("restored with another margin", &finalize_tx(&late, 0, |o| o[0].script_public_key = cross_spk(&c.with_margin(SIDE_A, 1))), 0);
    refused("a void close paid", &finalize_tx(&late, 0, |o| *o = vec![out(2_070 * KAS1 as u64, key_spk(&c.a_key), None), change(1), out((2_930 * KAS1 + 20_000_000) as u64, key_spk(&c.b_key), None)]), 0);
    must("valid at t0 + lateDaa", &finalize_tx(&proposed(&lx, &cl, 6_600), 0, |_| {}));
    // B closes at the same prices: paid its side at the prices moved toward A (11.22 and 20.58).
    let b_pay = c.close_pay(SIDE_B, p).unwrap();
    assert_eq!(b_pay, 5_000 * KAS1 - (2_000 * KAS1 + 1_220 * KAS1 - 290 * KAS1));
    let lb = proposed(&lx, &Claim::close(&c, SIDE_B, p, 6_500), 6_510);
    assert_eq!(lb.s.settlement(lb.value, 6_510), XSettlement::Close { a: lx.value as i64 - b_pay, b: b_pay });
    must("B's close", &finalize_tx(&lb, 0, |_| {}));
    refused("B one sompi more", &finalize_tx(&lb, 0, |o| { o[2].value += 1; o[0].value -= 1; }), 0);
}

#[test]
fn a_close_needs_its_owner_the_quiet_time_the_epoch_tables_and_its_maintenance() {
    let c = account();
    let lx = live_x(&c);
    let p = [1_100_000_000, 2_100_000_000];
    let good = Claim::close(&c, SIDE_A, p, 6_500);
    must("A's close", &propose_tx(&lx, &good, |_| {}));
    refused("signed by B", &propose_tx(&lx, &Claim { signer: Some(b_kp()), ..good.clone() }, |_| {}), 0);
    refused("signed by a stranger", &propose_tx(&lx, &Claim { signer: Some(exec_kp()), ..good.clone() }, |_| {}), 0);
    refused("before the quiet time", &propose_tx(&lx, &Claim { seq: 19, ..good.clone() }, |_| {}), 0);
    refused("a pending claim of one sompi less", &propose_tx(&lx, &good, |o| o[0].value -= 1), 0);
    // The prices are the epoch tables': a claim that names another price for either leg is refused.
    refused("another price for leg 1", &propose_tx(&lx, &Claim { p: [1_110_000_000, p[1]], ..good.clone() }, |_| {}), 0);
    refused("another price for leg 2", &propose_tx(&lx, &Claim { p: [p[0], 2_090_000_000], ..good.clone() }, |_| {}), 0);
    // Each leg its own market: the two attestations swapped, or one epoch table named for both legs.
    refused("the attestations swapped", &propose_tx(&lx, &Claim { atts: [good.atts[1].clone(), good.atts[0].clone()], ..good.clone() }, |_| {}), 0);
    refused("one epoch table for both legs", &propose_tx(&lx, &Claim { u: [1, 1], ..good.clone() }, |_| {}), 0);
    refused("the account named as an epoch table", &propose_tx(&lx, &Claim { u: [1, 0], ..good.clone() }, |_| {}), 0);
    // One signed price for both legs: a second leg attested for another signed price is refused (its epoch table's own script accepts it).
    let other_price = Claim { atts: [good.atts[0].clone(), att_of(&M2, 6_490, p[1])], ..good.clone() };
    refused("the second leg priced at another signed price", &propose_tx(&lx, &other_price, |_| {}), 0);
    // A close that would leave the closer below the maintenance at the prices it is paid at is refused: A's exit is
    // then the liquidation. At (10, 22.9) A is owed 372 KAS against A, the maintenance is 1,073.9 KAS.
    let low = [1_000_000_000, 2_290_000_000];
    assert_eq!(c.close_pay(SIDE_A, low), None);
    refused("a close below the closer's maintenance", &propose_tx(&lx, &Claim::close(&c, SIDE_A, low, 6_500), |_| {}), 0);
    // A signed price more than lateDaa older than the account's UTXO is refused: for an account accepted at 6,700 the signed price
    // 6,500 is too old and 6,600 is the oldest.
    let young = Live { daa: 6_700, ..lx.clone() };
    refused("a signed price older than the account by more than lateDaa", &propose_tx(&young, &good, |_| {}), 0);
    must("a signed price at the edge", &propose_tx(&young, &Claim::close(&c, SIDE_A, p, 6_600), |_| {}));
}

#[test]
fn a_liquidation_is_decided_on_the_total_and_forfeits_after_the_grace() {
    let c = account();
    let lx = live_x(&c);
    // At (10, 23), toward A: market 1 at 10.2 (+200), market 2 at 22.54 (-1,270): A is owed 930 KAS, below the
    // maintenance 1,073.5 KAS of the 21,470 KAS notional; B has its own.
    let p = [1_000_000_000, 2_300_000_000];
    assert!(c.liquidatable(SIDE_A, p));
    // The trigger on market 2 with market 1 at its entry: the least price that liquidates, to the sompi.
    let first = (2_000_000_000..2_400_000_000i64).step_by(10_000).find(|&x| c.liquidatable(SIDE_A, [P0[0], x])).unwrap();
    assert!(!c.liquidatable(SIDE_A, [P0[0], first - 10_000]));
    must("the first liquidating price", &propose_tx(&lx, &Claim::liq(&c, SIDE_A, [P0[0], first], 6_500), |_| {}));
    refused("the last one that does not", &propose_tx(&lx, &Claim::liq(&c, SIDE_A, [P0[0], first - 10_000], 6_500), |_| {}), 0);
    refused("B is not the one liquidated", &propose_tx(&lx, &Claim::liq(&c, SIDE_B, p, 6_500), |_| {}), 0);
    let cl = Claim::liq(&c, SIDE_A, p, 6_500);
    let lw = proposed(&lx, &cl, 6_510);
    // The fee: 0.5 % of the 21,500 KAS at the attested prices, 107.5 KAS (A is still owed 500 KAS there). The cure:
    // against A market 1 at 9.8 (-200), market 2 at 23.46 (-1,730): A's side is 70 KAS and the maintenance 1,076.5
    // KAS, so 1,006.5 KAS.
    let fee = 107 * KAS1 + 50_000_000;
    assert_eq!((lw.s.amount, lw.s.cure_add, lw.s.proposer), (fee, 1_006 * KAS1 + 50_000_000, x(&exec_kp())));
    assert_eq!(lw.s.settlement(lw.value, 6_510), XSettlement::Forfeit { fee, winner: c.b_key, rest: lx.value as i64 - fee });
    refused("finalize before the grace", &finalize_tx(&lw, 49, |_| {}), 0);
    must("finalize after the grace", &finalize_tx(&lw, 50, |_| {}));
    refused("the fee one sompi short", &finalize_tx(&lw, 50, |o| o[0].value -= 1), 0);
    refused("the fee to another key", &finalize_tx(&lw, 50, |o| o[0].script_public_key = key_spk(&c.a_key)), 0);
    refused("the winner one sompi short", &finalize_tx(&lw, 50, |o| o[2].value -= 1), 0);
    refused("the rest to the loser", &finalize_tx(&lw, 50, |o| o[2].script_public_key = key_spk(&c.a_key)), 0);
    refused("a valid liquidation restored", &finalize_tx(&lw, 50, |o| *o = vec![out(lw.value, cross_spk(&c), bind(0, lw.id.unwrap())), change(1)]), 0);
    // The loser cures: its margin grows by 1,006.5 KAS, and no claim within g of these prices reopens the account.
    let (cured, value) = lw.s.cure(lw.value).unwrap();
    assert_eq!((cured.a_margin, value), (3_006 * KAS1 + 50_000_000, lx.value as i64 + 1_006 * KAS1 + 50_000_000));
    assert!(!cured.liquidatable(SIDE_A, p) && cured.close_pay(SIDE_A, p).is_some());
    must("A cures", &cure_tx(&lw, &a_kp(), |_| {}));
    refused("B cannot cure A", &cure_tx(&lw, &b_kp(), |_| {}), 0);
    refused("one sompi less than the cure", &cure_tx(&lw, &a_kp(), |o| o[0].value -= 1), 0);
    refused("a cure that keeps the old margin", &cure_tx(&lw, &a_kp(), |o| o[0].script_public_key = cross_spk(&c)), 0);
    // A void liquidation restores the account, after the grace too, and pays nobody.
    let late = proposed(&lx, &cl, 6_601);
    must("a void liquidation restores", &finalize_tx(&late, 0, |_| {}));
    refused("a void liquidation paid as a forfeit", &finalize_tx(&late, 50, |o| *o = vec![out(fee as u64, key_spk(&x(&exec_kp())), None), change(1), out(lx.value - fee as u64, key_spk(&c.b_key), None)]), 0);
    // An account with one owner on both sides is not liquidated.
    let own = Cross { b_key: c.a_key, ..c };
    refused("one owner", &propose_tx(&live_x(&own), &Claim::liq(&own, SIDE_A, p, 6_500), |_| {}), 0);
}

#[test]
fn the_counterparty_is_liquidated_and_cures_the_same_way() {
    let c = account();
    let lx = live_x(&c);
    // At (13, 16) A's legs have gained 3,000 and 2,000 KAS: more than B's 3,000 KAS of margin.
    let p = [1_300_000_000, 1_600_000_000];
    let cl = Claim::liq(&c, SIDE_B, p, 6_500);
    let lw = proposed(&lx, &cl, 6_510);
    assert_eq!(lw.s.cure_add, c.cure_add(SIDE_B, p));
    let XSettlement::Forfeit { fee, winner, rest } = lw.s.settlement(lw.value, 6_510) else { panic!("a forfeit") };
    assert_eq!((winner, fee + rest), (c.a_key, lx.value as i64));
    must("B is forfeited after the grace", &finalize_tx(&lw, 50, |_| {}));
    refused("the rest to B", &finalize_tx(&lw, 50, |o| o[2].script_public_key = key_spk(&c.b_key)), 0);
    let (cured, _) = lw.s.cure(lw.value).unwrap();
    assert!(cured.b_margin > c.b_margin && cured.a_margin == c.a_margin && !cured.liquidatable(SIDE_B, p));
    must("B cures", &cure_tx(&lw, &b_kp(), |_| {}));
    refused("A cannot cure B", &cure_tx(&lw, &a_kp(), |_| {}), 0);
}

#[test]
fn margin_is_added_the_lease_renewed_and_the_account_closed_by_both_or_lapsed() {
    let c = account();
    let lx = live_x(&c);
    must("A adds 100 KAS", &add_margin_tx(&lx, SIDE_A, 100 * KAS1, &a_kp(), 20, |_| {}));
    must("B adds 100 KAS", &add_margin_tx(&lx, SIDE_B, 100 * KAS1, &b_kp(), 20, |_| {}));
    refused("B signs for A's margin", &add_margin_tx(&lx, SIDE_A, 100 * KAS1, &b_kp(), 20, |_| {}), 0);
    refused("before the quiet time", &add_margin_tx(&lx, SIDE_A, 100 * KAS1, &a_kp(), 19, |_| {}), 0);
    refused("one sompi less than the margin added", &add_margin_tx(&lx, SIDE_A, 100 * KAS1, &a_kp(), 20, |o| o[0].value -= 1), 0);
    refused("a margin taken out", &add_margin_tx(&lx, SIDE_A, -100 * KAS1, &a_kp(), 20, |o| o[0].value = lx.value - 100 * KAS1 as u64), 0);
    refused("the other side's margin written", &add_margin_tx(&lx, SIDE_A, 100 * KAS1, &a_kp(), 20, |o| o[0].script_public_key = cross_spk(&c.with_margin(SIDE_B, 100 * KAS1))), 0);
    // An added margin moves the liquidation: with 1,000 KAS more A is not liquidated at (10, 24).
    assert!(!c.with_margin(SIDE_A, 1_000 * KAS1).liquidatable(SIDE_A, [1_000_000_000, 2_400_000_000]));
    // The lease (14,000, a term of 8,000): a signed price of either leg's market from 8,000 on renews it.
    assert_eq!(c.keep_alive_from(), 8_000);
    refused("a signed price that moves the lease by less than a quarter of a term", &keep_alive_tx(&lx, 1, &c.legs[0].market, 1_000_000_000, 6_500, |_| {}), 0);
    // An account whose lease ends at 12,000 is renewed by a signed price of 6,500, of either leg's market and no other.
    let soon = live_x(&Cross { expiry: 12_000, ..c });
    assert_eq!(soon.s.kept_alive(6_500).unwrap().expiry, 14_500);
    must("a signed price of market 1 renews the lease", &keep_alive_tx(&soon, 1, &c.legs[0].market, 1_000_000_000, 6_500, |_| {}));
    must("a signed price of market 2 renews it", &keep_alive_tx(&soon, 2, &M2, 2_000_000_000, 6_500, |_| {}));
    refused("a signed price of market 1 named for leg 2", &keep_alive_tx(&soon, 2, &c.legs[0].market, 1_000_000_000, 6_500, |_| {}), 0);
    refused("a third leg", &keep_alive_tx(&soon, 3, &M2, 2_000_000_000, 6_500, |_| {}), 0);
    refused("a renewal that takes a sompi", &keep_alive_tx(&soon, 1, &c.legs[0].market, 1_000_000_000, 6_500, |o| o[0].value -= 1), 0);
    refused("a renewal that changes a margin", &keep_alive_tx(&soon, 1, &c.legs[0].market, 1_000_000_000, 6_500, |o| o[0].script_public_key = cross_spk(&Cross { expiry: 14_500, ..soon.s.with_margin(SIDE_A, 1) })), 0);
    // Anyone settles a lapsed account at its entries: A its margin, B the rest.
    assert_eq!(c.lapse_out(lx.value), (2_000 * KAS1, 3_000 * KAS1 + 20_000_000));
    must("the lapse", &lapse_tx(&lx, 15_000, |_| {}));
    refused("before the lease and its idle time", &lapse_tx(&lx, 14_999, |_| {}), 0);
    refused("A one sompi more at the lapse", &lapse_tx(&lx, 15_000, |o| { o[0].value += 1; o[2].value -= 1; }), 0);
    refused("B's share to another key", &lapse_tx(&lx, 15_000, |o| o[2].script_public_key = key_spk(&x(&exec_kp()))), 0);
    // Both parties end the account as they agree.
    let coop = |a: &Keypair, b: &Keypair| build_ext(&[cross_in(&lx, "coopClose", vec![Arg::Sig(*a), Arg::Sig(*b)]), exec_in(60)], vec![change(1)], 0, &[]);
    must("both sign", &coop(&a_kp(), &b_kp()));
    refused("A alone", &coop(&a_kp(), &a_kp()), 0);
    refused("B alone", &coop(&b_kp(), &b_kp()), 0);
}

#[test]
fn an_account_with_one_leg_needs_one_epoch_table() {
    let mut c = account();
    c.legs[1] = XLeg::none();
    c.check().unwrap();
    let lx = live_x(&c);
    // One leg: A long 1,000 units from 10 with 2,000 KAS: a pair in the account's form.
    assert_eq!(c.a_at([1_100_000_000, 0]), 3_000 * KAS1);
    let cl = Claim::close(&Cross { legs: [c.legs[0], account().legs[1]], ..c }, SIDE_A, [1_100_000_000, 0], 6_500);
    let lw = proposed(&lx, &cl, 6_510);
    assert_eq!(lw.s.amount, c.close_pay(SIDE_A, [1_100_000_000, 0]).unwrap());
    must("the close of a one-leg account", &finalize_tx(&lw, 0, |_| {}));
    assert!(c.liquidatable(SIDE_A, [800_000_000, 0]));
    must("its liquidation", &propose_tx(&lx, &Claim::liq(&Cross { legs: [c.legs[0], account().legs[1]], ..c }, SIDE_A, [800_000_000, 0], 6_500), |_| {}));
}

#[test]
fn a_pot_smaller_than_two_maintenances_liquidates_nobody_and_a_fee_is_the_losers_money() {
    // Review C9 (C9-F1). Two legs of equal size and opposite direction: at equal prices A is owed its margin whatever
    // the price. With 1,500 KAS a side and both markets at 30, the notional is 60,000 KAS and its maintenance 3,000:
    // neither side has it, nobody lost anything, and neither is liquidated by whoever claims first.
    let mut c = account();
    c.legs[1].size = 1_000_000_000;
    c.legs[1].entry = 1_000_000_000;
    (c.a_margin, c.b_margin) = (1_500 * KAS1, 1_500 * KAS1);
    let lx = live_x(&c);
    let high = [3_000_000_000, 3_000_000_000];
    assert_eq!(c.a_at(high), 1_500 * KAS1);
    assert!(!c.liquidatable(SIDE_A, high) && !c.liquidatable(SIDE_B, high));
    refused("A is not liquidated", &propose_tx(&lx, &Claim::liq(&c, SIDE_A, high, 6_500), |_| {}), 0);
    refused("nor B", &propose_tx(&lx, &Claim::liq(&c, SIDE_B, high, 6_500), |_| {}), 0);
    // Either party leaves with its side at the prices moved by g against it (-600 KAS of the 1,500 here: g is 2 %
    // of two legs of 30,000 KAS): nobody forfeits and nobody is held until the lapse.
    assert_eq!((c.close_pay(SIDE_A, high), c.close_pay(SIDE_B, high)), (Some(300 * KAS1), Some(300 * KAS1)));
    for side in [SIDE_A, SIDE_B] {
        let lw = proposed(&lx, &Claim::close(&c, side, high, 6_500), 6_510);
        must("a close where both sides lack the maintenance", &finalize_tx(&lw, 0, |_| {}));
    }
    // A bankrupt owner cannot hold its counterparty: market 2 alone at 100 costs A 90,000 KAS against a pot of 3,000
    // (below the maintenance of the notional), A is owed nothing, and B closes and takes the pot (the carrier, 0.2
    // KAS, is the other party's in a close).
    let gone = [1_000_000_000, 10_000_000_000];
    assert_eq!((c.a_at(gone), c.liquidatable(SIDE_A, gone), c.close_pay(SIDE_B, gone)), (0, false, Some(3_000 * KAS1)));
    let lw = proposed(&lx, &Claim::close(&c, SIDE_B, gone, 6_500), 6_510);
    assert_eq!(lw.s.settlement(lw.value, 6_510), XSettlement::Close { a: 20_000_000, b: 3_000 * KAS1 });
    must("the counterparty of a bankrupt owner closes and takes the pot", &finalize_tx(&lw, 0, |_| {}));
    // Once the other party has its maintenance, a side below its own is liquidated as ever, and does not close.
    let funded = c.with_margin(SIDE_B, 3_000 * KAS1);
    assert!(funded.liquidatable(SIDE_A, high) && !funded.liquidatable(SIDE_B, high) && funded.close_pay(SIDE_A, high).is_none());
    refused("a liquidated side does not close", &propose_tx(&live_x(&funded), &Claim::close(&funded, SIDE_A, high, 6_500), |_| {}), 0);
    must("A is liquidated once B has its maintenance", &propose_tx(&live_x(&funded), &Claim::liq(&funded, SIDE_A, high, 6_500), |_| {}));
    // C9-F2. The fee is at most what the loser is still owed at the attested prices: at (13, 16) of the worked account B is
    // owed nothing, so the proposer is paid nothing and the winner takes the whole account.
    let w = account();
    let p = [1_300_000_000, 1_600_000_000];
    assert_eq!(w.fee_of(SIDE_B, p), 0);
    let lw = proposed(&live_x(&w), &Claim::liq(&w, SIDE_B, p, 6_500), 6_510);
    assert_eq!(lw.s.settlement(lw.value, 6_510), XSettlement::Forfeit { fee: 0, winner: w.a_key, rest: lw.value as i64 });
    must("the winner takes the whole account", &finalize_tx(&lw, 50, |_| {}));
    refused("a fee out of the winner's money", &finalize_tx(&lw, 50, |o| { o[0] = out(KAS1 as u64, key_spk(&x(&exec_kp())), None); o[2].value -= KAS1 as u64; }), 0);
}
