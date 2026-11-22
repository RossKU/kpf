//! Review C10 (docs/audit/REVIEW-C10.md): the fixes of review C9 on the cross-margin account (a liquidation needs the
//! other party at its maintenance; the fee is no more than the liquidated side is owed), what the first of them
//! leaves, and the close the coordinator plans, judged on the model. Helpers: cross_tests.rs and auditC9_tests.rs.
//!
//! `c10_holds_` what was attacked and held; `c10_bounded_` an accepted behaviour, measured.

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

// ------------------------------------------------------------------------------------------------ review C10

/// The two fixes of review C9 on 250 accounts at their edges, three signed prices each: a side is liquidated only while the
/// other is at its maintenance at the same prices, so never both at one time; the fee is no more than the liquidated
/// side is owed at the attested prices, so the winner is paid at least what it is owed there and a side owed nothing
/// pays no proposer. The engine accepts exactly the liquidations the model writes.
#[test]
fn c10_holds_a_liquidation_needs_a_healthy_winner_and_its_fee_is_the_losers_money() {
    let mut r = Rng(0xc10c_10c1_0c10_c10c);
    let (mut accounts, mut liqs, mut free, mut nobody) = (0, 0, 0, 0);
    while accounts < 250 {
        let c = random_account(&mut r);
        if c.check().is_err() {
            continue;
        }
        accounts += 1;
        let lx = live_x(&c);
        for _ in 0..3 {
            let mut p = [0i64; 2];
            for i in 0..2 {
                let e = c.legs[i].entry.max(1_000_000_000);
                p[i] = r.pick(&[1, e / 2, e * 9 / 10, e * 99 / 100, e, e * 101 / 100, e * 11 / 10, e * 3 / 2, e * 3, c.legs[i].price_max, c.legs[i].price_max + 1]).clamp(1, 100_000_000_000_000);
            }
            let mut n = 0;
            for side in [SIDE_A, SIDE_B] {
                let cl = Claim::liq(&c, side, p, 6_500);
                let model = c.pending_of(xkit(), KIND_LIQ, side, p, 6_500, cl.proposer);
                let t = propose_tx(&lx, &cl, |_| {});
                assert_eq!(accepted(&t), model.is_some(), "side {side} at {p:?}: {c:?}");
                let Some(w) = model else { continue };
                n += 1;
                liqs += 1;
                // The other party is at its maintenance at the trigger's prices (those moved toward the side's gain).
                let v = c.view(p, Some(side == SIDE_A));
                let a = v.raw.clamp(0, c.pot());
                let (mine, theirs) = if side == SIDE_A { (a, c.pot() - a) } else { (c.pot() - a, a) };
                assert!(mine < c.maint(v.notional) && theirs >= c.maint(v.notional), "{c:?} {p:?}");
                // The fee is the loser's: no more than it is owed at the attested prices.
                let owed = c.owed_at(side, p);
                assert!(w.amount <= owed && w.amount == c.fee_of(side, p), "the fee {} against {owed} owed: {c:?} {p:?}", w.amount);
                free += (w.amount == 0) as u32;
                let lw = live(&t, 0, 6_510, &w.made_by(&lx.op));
                let XSettlement::Forfeit { fee, rest, .. } = lw.s.settlement(lw.value, 6_510) else { panic!("a liquidation") };
                assert!(fee <= owed && fee + rest == lw.value as i64);
                assert!(rest >= c.pot() - owed + c.t.carrier, "the winner is paid less than it is owed at the attested prices");
                must("the liquidation settles", &finalize_tx(&lw, c.t.grace_daa as u64, |_| {}));
            }
            assert!(n < 2, "both sides liquidated at one time: {c:?} {p:?}");
            // Where neither is liquidated though one is below the maintenance at its trigger's prices.
            let below = |side: i64| {
                let v = c.view(p, Some(side == SIDE_A));
                let a = v.raw.clamp(0, c.pot());
                (if side == SIDE_A { a } else { c.pot() - a }) < c.maint(v.notional)
            };
            nobody += (n == 0 && (below(SIDE_A) || below(SIDE_B))) as u32;
        }
    }
    println!("C10: {accounts} accounts, 750 signed prices: {liqs} liquidations accepted, {free} of them with no fee (the loser is owed nothing), never two at one time; at {nobody} signed prices a side is below the maintenance and nobody is liquidated");
    assert!(liqs > 60 && free > 0 && nobody > 0, "{liqs} {free} {nobody}");
}

/// What the rule leaves when both sides are below the maintenance (two legs that offset, both markets up from 10 to
/// 30): nobody is liquidated and nobody closes. The party that first adds its shortfall is at its maintenance, the
/// other is not, and is liquidated: 1,500 KAS that lost nothing are forfeited to whoever topped up first (300 of them
/// the proposer's), unless the other adds 2,700 KAS within the grace. The addition comes back with the account.
#[test]
fn c10_bounded_under_two_maintenances_the_first_to_add_its_shortfall_liquidates_the_other() {
    let mut c = account();
    c.legs[1] = XLeg { size: 1_000_000_000, entry: 1_000_000_000, ..c.legs[1] };
    c.a_margin = 1_500 * KAS1;
    c.b_margin = 1_500 * KAS1;
    let p = [30 * KAS1, 30 * KAS1];
    let lx = live_x(&c);
    for side in [SIDE_A, SIDE_B] {
        // (After this round the close is built: where neither side is at its maintenance nobody is liquidated and
        // either party closes, paid its side at the prices moved by g against it.)
        assert!(!c.liquidatable(side, p) && c.close_pay(side, p).is_some());
        refused("a liquidation where neither side is at its maintenance", &propose_tx(&lx, &Claim::liq(&c, side, p, 6_500), |_| {}), 0);
        must("a close where neither side is at its maintenance", &propose_tx(&lx, &Claim::close(&c, side, p, 6_500), |_| {}));
    }
    // A adds what puts it at the maintenance at the prices moved against it: 2,700 KAS.
    let add = c.cure_add(SIDE_A, p);
    assert_eq!(add, 2_700 * KAS1);
    let t = add_margin_tx(&lx, SIDE_A, add, &a_kp(), 20, |_| {});
    must("A adds its shortfall", &t);
    let c2 = c.with_margin(SIDE_A, add);
    let lx2 = live(&t, 0, 6_480, &c2);
    assert!(c2.liquidatable(SIDE_B, p) && !c2.liquidatable(SIDE_A, p));
    // No quiet time is asked of a liquidation: the claim follows the addition at once.
    let lw = proposed(&lx2, &Claim::liq(&c2, SIDE_B, p, 6_500), 6_510);
    let XSettlement::Forfeit { fee, winner, rest } = lw.s.settlement(lw.value, 6_510) else { panic!("a liquidation") };
    assert_eq!((winner, fee, rest), (c.a_key, 300 * KAS1, lx2.value as i64 - 300 * KAS1));
    assert_eq!(rest - add - c.a_margin - c.t.carrier, 1_200 * KAS1, "A's addition and margin back, and 1,200 KAS of B's");
    must("the liquidation of the party that did not add", &finalize_tx(&lw, c.t.grace_daa as u64, |_| {}));
    assert_eq!(c2.cure_add(SIDE_B, p), 2_700 * KAS1);
    must("B's cure", &cure_tx(&lw, &b_kp(), |_| {}));
    // B, meanwhile, could not have closed: its close needs the quiet time of an account A has just respent, and is
    // refused in any case while it is below the maintenance.
    assert!(c2.close_pay(SIDE_B, p).is_none());
}

/// The hole the coordinator named. A is owed nothing (one leg long from 10, the price at 9, 200 KAS of margin each)
/// and the pot (400) is under the maintenance of the notional (459 at the prices moved toward A): B, owed all of it,
/// is "below the maintenance", so A is not liquidated, and B does not close. Left alone the account lapses and pays A
/// its margin of 200 KAS. B's way out today: add 59 KAS and more (they come back), then liquidate; the fee is 0, so
/// nobody else will.
#[test]
fn c10_fixed_the_party_owed_everything_closes_when_the_pot_is_under_the_maintenance() {
    let mut c = account();
    c.legs[1] = XLeg::none();
    c.a_margin = 200 * KAS1;
    c.b_margin = 200 * KAS1;
    let p = [9 * KAS1, 1];
    assert_eq!((c.a_at(p), c.owed_at(SIDE_B, p)), (0, 400 * KAS1));
    let lx = live_x(&c);
    // (As fixed after this round: A is still not liquidated, and B, owed everything, closes and takes the pot; before,
    // its close was refused too and the lapse below was its only end.)
    assert!(!c.liquidatable(SIDE_A, p) && c.close_pay(SIDE_B, p) == Some(400 * KAS1) && c.close_pay(SIDE_A, p) == Some(0));
    refused("the liquidation of a side owed nothing, the pot under the maintenance", &propose_tx(&lx, &Claim::liq(&c, SIDE_A, p, 6_500), |_| {}), 0);
    must("the close of the party owed everything", &propose_tx(&lx, &Claim::close(&c, SIDE_B, p, 6_500), |_| {}));
    // The lapse: the entries. A, owed nothing at 9, would be paid its margin if B let the lease lapse.
    assert_eq!(c.lapse_out(lx.value), (200 * KAS1, 200 * KAS1 + c.t.carrier));
    must("the lapse pays A its margin", &lapse_tx(&lx, c.lapse_from(), |_| {}));
    // B adds 100 KAS: the pot is over the maintenance, A is liquidated, the fee is A's money (nothing), B takes all.
    let t = add_margin_tx(&lx, SIDE_B, 100 * KAS1, &b_kp(), 20, |_| {});
    must("B adds margin", &t);
    let c2 = c.with_margin(SIDE_B, 100 * KAS1);
    let lx2 = live(&t, 0, 6_480, &c2);
    assert!(c2.liquidatable(SIDE_A, p));
    assert_eq!(c2.fee_of(SIDE_A, p), 0);
    let lw = proposed(&lx2, &Claim::liq(&c2, SIDE_A, p, 6_500), 6_510);
    let XSettlement::Forfeit { fee, winner, rest } = lw.s.settlement(lw.value, 6_510) else { panic!("a liquidation") };
    assert_eq!((fee, winner, rest), (0, c.b_key, lx2.value as i64));
    must("B takes the account", &finalize_tx(&lw, c.t.grace_daa as u64, |_| {}));
    // With 58 KAS (a pot of 458 against 459) it is still not.
    assert!(!c.with_margin(SIDE_B, 58 * KAS1).liquidatable(SIDE_A, p) && c.with_margin(SIDE_B, 59 * KAS1).liquidatable(SIDE_A, p));
}

/// The close the coordinator plans (a close is refused only where the closer's own liquidation is due), judged on the
/// model before it is built: `closes(side) = at its maintenance at the prices against it, OR the other party below
/// its maintenance at the prices toward the closer`. Over the same accounts and signed prices: where it differs from "the
/// closer is not liquidated", and that it never lets a side that is liquidated at the signed price close.
#[test]
fn c10_bounded_the_planned_close_against_the_liquidation_on_the_model() {
    let mut r = Rng(0xc10c_10c1_0c10_c10c);
    let (mut accounts, mut signed_prices, mut newly, mut band, mut both_ways, mut stuck_now, mut stuck_then) = (0, 0, 0, 0, 0, 0, 0);
    while accounts < 2_000 {
        let c = random_account(&mut r);
        if c.check().is_err() {
            continue;
        }
        accounts += 1;
        for _ in 0..3 {
            let mut p = [0i64; 2];
            for i in 0..2 {
                let e = c.legs[i].entry.max(1_000_000_000);
                p[i] = r.pick(&[1, e / 2, e * 9 / 10, e * 99 / 100, e, e * 101 / 100, e * 11 / 10, e * 3 / 2, e * 3, c.legs[i].price_max, c.legs[i].price_max + 1]).clamp(1, 100_000_000_000_000);
            }
            signed_prices += 1;
            // (own worth, the other's, the maintenance) at the prices moved toward `side`'s gain or against it.
            let at = |side: i64, toward: bool| {
                let v = c.view(p, Some((side == SIDE_A) == toward));
                let a = v.raw.clamp(0, c.pot());
                let m = c.maint(v.notional);
                if side == SIDE_A { (a, c.pot() - a, m) } else { (c.pot() - a, a, m) }
            };
            let mut exits = 0;
            let mut exits_now = 0;
            for side in [SIDE_A, SIDE_B] {
                let (own_g, _, m_g) = at(side, false);
                let (_, other_f, m_f) = at(side, true);
                let planned = own_g >= m_g || other_f < m_f;
                let now = c.close_pay(side, p).is_some();
                assert!(planned || !now, "the planned close refuses what closes today");
                let liq = c.liquidatable(side, p) && c.cure_add(side, p) > 0;
                newly += (planned && !now) as u32;
                // Refused, and not liquidated: the band of the tolerance (at its maintenance toward its gain, below it
                // against), the other party healthy.
                band += (!planned && !liq) as u32;
                if planned && liq {
                    // Only where the maintenance against the side rounds to nothing (review C9, C9-N2).
                    assert_eq!(m_g, 0, "a side closes and is liquidated at one time: {c:?} {p:?}");
                    both_ways += 1;
                }
                exits += (planned || c.liquidatable(SIDE_A, p) || c.liquidatable(SIDE_B, p)) as u32;
                exits_now += (now || c.liquidatable(SIDE_A, p) || c.liquidatable(SIDE_B, p)) as u32;
            }
            stuck_then += (exits == 0) as u32;
            stuck_now += (exits_now == 0) as u32;
        }
    }
    println!("C10 (planned close, model): {signed_prices} signed prices; a close the plan allows and today's rule refuses: {newly}; refused though the closer is not liquidated (the tolerance band): {band}; closes and is liquidated at one time (maintenance of 0 sompi only): {both_ways}");
    println!("C10 (planned close, model): signed prices at which no claim of either kind is proposable: {stuck_now} today, {stuck_then} with the planned close");
    // The planned close is built now: the model's `close_pay` is the plan, and no signed price is without a claim.
    assert!(newly == 0 && stuck_then == 0 && stuck_now == 0, "{newly} {stuck_then} {stuck_now}");
}
