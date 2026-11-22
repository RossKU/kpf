//! Review C9 (docs/audit/REVIEW-C9.md): the cross-margin account (`KpfCrossC`, `KpfPendingX`) in the script engine,
//! against the library's model at the edges of its arithmetic. The helpers are those of cross_tests.rs.
//!
//! `c9_holds_` what was attacked and held; `c9_bounded_` an accepted behaviour, measured.

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

// ------------------------------------------------------------------------------------------------ review C9

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

/// The engine against the model on 250 accounts at their edges, three signed prices each, both kinds of claim for both sides:
/// `propose` is accepted exactly when the model writes a pending claim, with the model's amounts (the claim's script
/// is the model's bytes); every accepted claim settles as the model says and pays exactly what the UTXO holds; every
/// liquidation is cured with the model's addition, and the cured side is not liquidated at the same prices.
#[test]
fn c9_holds_the_engine_and_the_model_agree_on_accounts_at_their_edges() {
    let mut r = Rng(0xc9c9_1234_5678_9abc);
    let (mut accounts, mut claims, mut accepted_n, mut both, mut unproposable, mut fee_over) = (0, 0, 0, 0, 0, 0);
    let (mut above, mut closes_too) = (0, 0);
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
            let liq = [c.liquidatable(SIDE_A, p) && c.cure_add(SIDE_A, p) > 0, c.liquidatable(SIDE_B, p) && c.cure_add(SIDE_B, p) > 0];
            both += (liq[0] && liq[1]) as u32;
            for (i, side) in [SIDE_A, SIDE_B].into_iter().enumerate() {
                // The trigger without a positive cure: the claim is refused (the model and the engine both).
                unproposable += (c.liquidatable(side, p) && c.cure_add(side, p) <= 0) as u32;
                // A side liquidated at the signed price is below the maintenance at the attested prices themselves, or the
                // pot is (it cannot be owed more than the pot).
                if liq[i] {
                    let v = c.view(p, None);
                    let own = if side == SIDE_A { v.raw } else { c.pot() - v.raw };
                    above += (own >= c.maint(v.notional)) as u32;
                    fee_over += (c.fee_of(side, p) > c.owed_at(side, p)) as u32;
                    // A side that is liquidated does not close at the same time, but where the maintenance at the
                    // prices moved against it rounds to nothing (prices of a sompi or two).
                    if c.close_pay(side, p).is_some() {
                        assert_eq!(c.maint(c.view(p, Some(side != SIDE_A)).notional), 0, "a side that is liquidated closes: {c:?} {p:?}");
                        closes_too += 1;
                    }
                }
                for kind in [KIND_CLOSE, KIND_LIQ] {
                    claims += 1;
                    let cl = if kind == KIND_CLOSE { Claim::close(&c, side, p, 6_500) } else { Claim::liq(&c, side, p, 6_500) };
                    let model = c.pending_of(xkit(), kind, side, p, 6_500, cl.proposer);
                    let t = propose_tx(&lx, &cl, |_| {});
                    assert_eq!(accepted(&t), model.is_some(), "kind {kind} side {side} at {p:?}: {c:?}");
                    let Some(w) = model else { continue };
                    accepted_n += 1;
                    assert!(w.consistent(xkit()));
                    let lw = live(&t, 0, 6_510, &w.made_by(&lx.op));
                    let v = lw.value as i64;
                    match lw.s.settlement(lw.value, 6_510) {
                        XSettlement::Close { a, b } => {
                            assert_eq!(a + b, v);
                            let (mine, rule) = if side == SIDE_A { (a, c.close_pay(side, p).unwrap()) } else { (b, c.close_pay(side, p).unwrap()) };
                            assert!(mine == rule || mine == 0 || mine == v, "the closer is paid its side, nothing below the dust, or all when the rest is dust: {mine} {rule}");
                            assert!(rule >= c.maint(c.view(p, Some(side != SIDE_A)).notional) || !c.liquidatable(side, p));
                        }
                        XSettlement::Forfeit { fee, rest, .. } => {
                            assert_eq!(fee + rest, v);
                            assert!(fee <= c.fee_of(side, p));
                        }
                        XSettlement::Restore { .. } => panic!("a claim that landed in time is restored"),
                    }
                    must("the claim settles as the model says", &finalize_tx(&lw, if kind == KIND_LIQ { c.t.grace_daa as u64 } else { 0 }, |_| {}));
                    if kind == KIND_LIQ {
                        let kp = if side == SIDE_A { a_kp() } else { b_kp() };
                        let (cured, value) = lw.s.cure(lw.value).expect("a liquidation has a cure");
                        assert_eq!(value, v + c.cure_add(side, p));
                        must("the cure", &cure_tx(&lw, &kp, |_| {}));
                        refused("the cure a sompi short", &cure_tx(&lw, &kp, |o| o[0].value -= 1), 0);
                        assert!(!cured.liquidatable(side, p) || cured.cure_add(side, p) <= 0, "a cured side is liquidated at the same prices: {c:?} {p:?}");
                    }
                }
            }
        }
    }
    // The spread margin, per leg: with g below four times the epoch table's spread (199 against 4 x 50) the account takes no
    // epoch table, so no claim; the model's `pending_of` knows no epoch table and writes one.
    let mut c = account();
    c.t.g_bps = 199;
    let cl = Claim::close(&c, SIDE_A, P0, 6_500);
    assert!(c.pending_of(xkit(), KIND_CLOSE, SIDE_A, P0, 6_500, cl.proposer).is_some());
    refused("a claim on an account whose g is under four spreads", &propose_tx(&live_x(&c), &cl, |_| {}), 0);
    println!("C9: {accounts} accounts, {claims} claims, {accepted_n} accepted by the engine and the model alike");
    println!("C9: both sides liquidated at one time: {both}; the trigger met and no cure to pay (refused): {unproposable}; a side liquidated while its raw worth at the attested prices is at or above the maintenance: {above}; the fee above what the liquidated side was owed: {fee_over}");
    println!("C9: a liquidated side that may also close (only where the maintenance against it is 0 sompi): {closes_too}");
    // After the fixes of C9-F1 and C9-F2: no signed price liquidates both sides, and no fee exceeds what the loser is owed.
    assert!(accepted_n > 300 && both == 0 && fee_over == 0, "{accepted_n} {both} {fee_over}");
}

/// The payout indices with two pending claims in one transaction (A at the input's index, B at its mirror): each
/// claim's two outputs are its own; one B output for both is refused.
#[test]
fn c9_holds_two_pending_claims_in_one_transaction_pay_each_its_own_outputs() {
    let c1 = account();
    let c2 = Cross { a_margin: 2_500 * KAS1, b_margin: 3_700 * KAS1, ..account() };
    let (lx1, lx2) = (live_x(&c1), Live { op: outpoint(11), id: Some(Hash::from_bytes([0x59; 32])), ..live_x(&c2) });
    let cl = |c: &Cross| Claim::close(c, SIDE_A, P0, 6_500);
    let (w1, w2) = (proposed(&lx1, &cl(&c1), 6_510), proposed(&lx2, &cl(&c2), 6_510));
    let pays = |w: &LW| match w.s.settlement(w.value, 6_510) {
        XSettlement::Close { a, b } => (a, b),
        _ => panic!("a close"),
    };
    let ((a1, b1), (a2, b2)) = (pays(&w1), pays(&w2));
    assert!(a1 != a2 && b1 != b2 && b1 > 0 && b2 > 0);
    let (ak, bk) = (key_spk(&c1.a_key), key_spk(&c1.b_key));
    let tx = |outs: Vec<TransactionOutput>| build_ext(&[pend_in(&w1, "finalize", vec![]), pend_in(&w2, "finalize", vec![]), exec_in(60)], outs, 0, &[0, 0]);
    let good = vec![out(a1 as u64, ak.clone(), None), out(a2 as u64, ak.clone(), None), change(1), out(b2 as u64, bk.clone(), None), out(b1 as u64, bk.clone(), None)];
    must("two closes settled side by side", &tx(good.clone()));
    let mut swapped = good.clone();
    swapped.swap(3, 4);
    assert!(!accepted(&tx(swapped)), "the two B payouts at each other's index");
    let mut swapped = good.clone();
    swapped.swap(0, 1);
    assert!(!accepted(&tx(swapped)), "the two A payouts at each other's index");
    let one = vec![out(a1 as u64, ak.clone(), None), out(a2 as u64, ak.clone(), None), change(1), out(b1.max(b2) as u64, bk.clone(), None)];
    assert!(!accepted(&tx(one)), "one B output for both claims");
    let short = vec![out(a1 as u64, ak.clone(), None), out(a2 as u64, ak, None), out(b2 as u64, bk.clone(), None), out(b1 as u64, bk, None)];
    assert!(!accepted(&tx(short)), "a mirror index among the inputs' own");
}

/// C9-F1, as fixed. The maintenance is on the notional at the present prices and the pot is fixed. An account whose
/// two legs offset (A long and short the same size) owes A the same at every price, and both sides fall below the
/// maintenance when the prices have risen enough. Before the fix either party was liquidated at such a signed price and the
/// first claim took the whole account; now a side is liquidated only if the other is not below the maintenance
/// itself, so a pot smaller than two maintenances liquidates nobody.
#[test]
fn c9_fixed_a_pot_smaller_than_two_maintenances_liquidates_nobody() {
    // A long 1,000 units and short 1,000 units of two markets at 10, 1,500 KAS of margin each (15 % of 20,000).
    let mut c = account();
    c.legs[1] = XLeg { size: 1_000_000_000, entry: 1_000_000_000, ..c.legs[1] };
    c.a_margin = 1_500 * KAS1;
    c.b_margin = 1_500 * KAS1;
    let at = |x: i64| [x * KAS1, x * KAS1];
    for x in [10, 20, 24, 30] {
        assert_eq!((c.a_at(at(x)), c.liquidatable(SIDE_A, at(x)), c.liquidatable(SIDE_B, at(x))), (1_500 * KAS1, false, false), "at {x}");
    }
    // At 30 (the notional 60,000, its maintenance 3,000: the pot) neither side has the maintenance and neither lost a
    // sompi: no claim liquidates either.
    let p = at(30);
    let lx = live_x(&c);
    for side in [SIDE_A, SIDE_B] {
        refused("a liquidation where both sides are below the maintenance", &propose_tx(&lx, &Claim::liq(&c, side, p, 6_500), |_| {}), 0);
    }
    // Either may close there (after review C10: a close is refused only where it would escape a due liquidation).
    assert!(c.close_pay(SIDE_A, p).is_some() && c.close_pay(SIDE_B, p).is_some());
}

/// A UTXO that holds more than the pot and the carrier, in each path: a close pays the closer its side and the other
/// party all the rest; a liquidation pays the winner; a cure and a restore keep it in the account; the lapse pays A
/// its margin and B the rest.
#[test]
fn c9_bounded_what_an_account_holds_beyond_its_pot_goes_to_the_party_that_did_not_claim() {
    let c = account();
    let extra = 7 * KAS1;
    let lx = Live { value: live_x(&c).value + extra as u64, ..live_x(&c) };
    for side in [SIDE_A, SIDE_B] {
        let lw = proposed(&lx, &Claim::close(&c, side, P0, 6_500), 6_510);
        let XSettlement::Close { a, b } = lw.s.settlement(lw.value, 6_510) else { panic!("a close") };
        let rule = c.close_pay(side, P0).unwrap();
        assert_eq!(if side == SIDE_A { (a, b) } else { (b, a) }, (rule, c.pot() - rule + c.t.carrier + extra));
        must("the close of an account that holds more", &finalize_tx(&lw, 0, |_| {}));
        // Void, it is restored with all it held.
        let late = Live { daa: 6_601, ..lw.clone() };
        assert!(matches!(late.s.settlement(late.value, 6_601), XSettlement::Restore { value, .. } if value == lx.value as i64));
        must("the restore", &finalize_tx(&late, 0, |_| {}));
        refused("the restore a sompi short", &finalize_tx(&late, 0, |o| o[0].value -= 1), 0);
    }
    assert_eq!(c.lapse_out(lx.value), (c.a_margin, c.b_margin + c.t.carrier + extra));
    must("the lapse", &lapse_tx(&lx, c.lapse_from(), |_| {}));
    // An account that holds less than its pot: every claim still pays no more than there is.
    let thin = Live { value: (c.a_margin / 2) as u64, ..live_x(&c) };
    let lw = proposed(&thin, &Claim::close(&c, SIDE_A, P0, 6_500), 6_510);
    let XSettlement::Close { a, b } = lw.s.settlement(lw.value, 6_510) else { panic!("a close") };
    assert_eq!((a, b), (thin.value as i64, 0));
    must("the close of an account that holds less than it says", &finalize_tx(&lw, 0, |_| {}));
}

/// The epoch table's change (`OpCovOutputCount == OpCovInputCount` for an attest): what the old "one input, one output"
/// refused and the family's own test does not show. An attest beside an epoch table that retires (two inputs of the id, one
/// output, and with a made-up second output two), and an attest beside the committee's top-up: refused by both.
#[test]
fn c9_holds_an_attest_beside_a_retire_or_the_committee_mints_nothing() {
    let lt = epoch_epoch_table();
    let a = att(&lt.s, 6_500, 1_000_000_000);
    let args = || a.args().into_iter().map(Arg::V).collect::<Vec<_>>();
    let old = Live { op: outpoint(4), ..epoch_table(&EpochTable { from: 4_000, until: 5_000, ..lt.s }) };
    let both_fail = |what: &str, t: &T| {
        let v = run(&t.0, &t.1).expect("bindings");
        assert!(v[0].is_err() && v[1].is_err(), "{what}: {:?} {:?}", v[0], v[1]);
    };
    let ins = [epoch_table_in(&lt, "attest", args()), epoch_table_in(&old, "retire", vec![]), exec_in(60)];
    both_fail("an attest beside a retire", &build_ext(&ins, vec![out(lt.value, epoch_table_spk(&lt.s), bind(0, RID)), change(1), change(1)], 8_000, &[]));
    // The count made equal with an output of the id nobody wrote: the retire still wants none and to be alone.
    let mut forged = EMPTY_TABLE;
    forged[0] = Member { key: x(&keypair(77)), weight: 100 * KAS1 };
    let fake = EpochTable { members: forged, total: 100 * KAS1, from: 6_000, until: 7_000, quorum_bps: 6_700, spread_bps: 50 };
    let outs = vec![out(lt.value, epoch_table_spk(&lt.s), bind(0, RID)), out(KAS1 as u64, epoch_table_spk(&fake), bind(1, RID)), change(1)];
    let t = build_ext(&ins, outs, 8_000, &[]);
    let v = run(&t.0, &t.1).expect("bindings");
    assert!(v[1].is_err(), "a retire that authors an epoch table: {:?}", v[1]);
    assert!(!accepted(&t));
    // Two attests whose second output is another epoch table than its own.
    let lt2 = Live { op: outpoint(5), ..lt.clone() };
    let two = [epoch_table_in(&lt, "attest", args()), epoch_table_in(&lt2, "attest", args()), exec_in(60)];
    let outs = vec![out(lt.value, epoch_table_spk(&lt.s), bind(0, RID)), out(lt.value, epoch_table_spk(&fake), bind(1, RID)), change(1)];
    let t = build_ext(&two, outs, 6_500, &[]);
    let v = run(&t.0, &t.1).expect("bindings");
    assert!(v[1].is_err() && v[0].is_ok(), "{:?} {:?}", v[0], v[1]);
}
