//! Review 6 (docs/audit/REVIEW-6.md): the oracle-market family (`KpfOpenO`, `KpfPairO`, `KpfPendingO`, `KpfBond`,
//! `KpfStake`), after the fixes (DESIGN-ORACLE-IMPL 24, "Changes after review 6"). Every transaction here runs every
//! input in the real script engine with the covenant context (`common::run`). `holds_` tests assert that an attack is
//! refused or harmless; the review's `exploit_` tests are kept as `holds_` tests of the same transactions. Worked numbers
//! use the fixture market of `oraclekit` (1,000 units at 10 KAS, long margin 1,000 KAS, short margin 2,000 KAS, cover 6,000
//! KAS, a claim's bond 600 KAS, judge stakes of 10 KAS).
//!
//! The induction the fixes rest on: every UTXO of a pair's or a claim's covenant id was written by a template of the
//! family that a party of that part verified. A lineage starts only where a script of the family checks that no input
//! carries the new id and that exactly one output does (`fresh`: `KpfOpenO.fill` and `take`, `KpfPairO.transfer`,
//! `closeHost`, `closeDonor`, `propose`); every entry spending a UTXO of an id counts that id's outputs exactly and fixes
//! each one's script; and only an input of an id authorizes outputs of it (a genesis cannot reproduce an id). So no UTXO
//! outside the family ever holds a part's or a claim's id, and an object of the family under the id is authentic again.

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use common::*;
use kaspa_consensus_core::tx::{TransactionOutpoint, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::bond::{BindArgs, Claim, Ctx};
use kpf_protocol::judge::{JudgeParams, Js, AGE_MAX, MIN_STAKE_MAX, NO, ROUND_MAX, STATUS_DISPUTE, STATUS_RECEIPT, STATUS_RESOLVED, STATUS_UNBONDING, UNBOND_MAX, UNIT, YES};
use kpf_protocol::oracle::*;
use oraclekit::*;
use secp256k1::Keypair;

const KAS: i64 = 100_000_000;
const FALSE_P: i64 = 920_000_000;

fn liq(side: i64, n: i64, p: i64, t0: i64) -> ClaimSpec {
    ClaimSpec { kind: LIQ, side, n, p, t0, lock: t0 as u64 }
}

fn contest_at(lw: &LPend, seed: u8, v: u64, daa: u64) -> (LPend, LStake, LStake) {
    let st = stake(seed, v, 0, 0x90 + seed);
    let (t, w2, r, d) = contest_tx(lw, &st, seed, daa);
    must("contest", &t);
    (live(&t, 0, daa, &w2), live(&t, 1, daa, &r), live(&t, 2, daa, &d))
}

/// The attacker's own key (never the underwriter's key; its counterparty key in the attacks).
fn attacker() -> Keypair {
    keypair(50)
}

/// A plain P2PK UTXO that carries a covenant id: the attacker's handle on that id.
#[derive(Clone)]
struct Handle {
    op: TransactionOutpoint,
    utxo: UtxoEntry,
}

impl Handle {
    fn id(&self) -> Hash {
        self.utxo.covenant_id.unwrap()
    }
    fn input(&self) -> In {
        In { outpoint: self.op, utxo: self.utxo.clone(), spend: Spend::P2pk(attacker()) }
    }
}

/// Step 0: the attacker makes a covenant id of its own with an ordinary genesis: one P2PK output of its key bound to a
/// fresh id. Consensus accepts it (the genesis id is the hash of the authorizing outpoint and the output).
fn make_handle() -> Handle {
    let ins = vec![key_in(outpoint(0x30), 10_000_000_000, 0, &attacker())];
    let mut outs = vec![out(5_000_000_000, p2pk_spk(&attacker()), None), out(4_000_000_000, p2pk_spk(&attacker()), None)];
    let g = genesis_id(outpoint(0x30), &outs, &[0]);
    outs[0].covenant = bind(0, g);
    let t = build_ext(&ins, outs, 0, &[0]);
    must("the attacker's genesis of a P2PK UTXO with its own covenant id", &t);
    let (op, utxo) = utxo_of(&t.0, 0);
    Handle { op, utxo }
}

/// The next handle: output `j` of `t` (a P2PK continuation of the attacker's id).
fn handle_at(t: &T, j: usize) -> Handle {
    let (op, utxo) = utxo_of(&t.0, j);
    Handle { op, utxo }
}

/// The attacker uses its handle on its id to write any KpfStake state under that id (here: a dispute or a resolved
/// pool); out 0 the forged UTXO, out 1 the handle again. Returns the forged UTXO (at `daa`) and the next handle.
fn forge(h: &Handle, s: &Js, value: u64, daa: u64) -> (LStake, Handle) {
    let id = h.id();
    let ins = vec![h.input(), key_in(outpoint(0x31), 50_000_000_000, 0, &attacker())];
    let outs = vec![out(value, stake_spk(s), bind(0, id)), out(1_000_000_000, p2pk_spk(&attacker()), bind(0, id)), out(40_000_000_000, p2pk_spk(&attacker()), None)];
    let t = build_ext(&ins, outs, 0, &[0, 0]);
    must("the attacker writes a KpfStake state under the id it holds", &t);
    (live(&t, 0, daa, s), handle_at(&t, 1))
}

/// The attacker tries to write the KpfStake state `s` under `id`, an id it does NOT hold, authorizing the output with
/// its handle: the covenant context must refuse (an output of an id needs an input of it; a genesis cannot reproduce
/// an existing id).
fn cannot_write_under(h: &Handle, id: Hash, s: &Js, what: &str) {
    let ins = vec![h.input(), key_in(outpoint(0x36), 50_000_000_000, 0, &attacker())];
    let outs = vec![out(DISPUTE_BASE, stake_spk(s), bind(0, id)), out(40_000_000_000, p2pk_spk(&attacker()), None)];
    let t = build_ext(&ins, outs, 0, &[0, 0]);
    assert!(run(&t.0, &t.1).is_err(), "{what}: written under an id the attacker does not hold");
}

/// A forged dispute naming the contested part `cw`: YES leads, its round started at DAA 0, so it would resolve at once.
fn forged_dispute(cw: &LPend, side: i64) -> Js {
    let jm = cw.s.m.judge_market();
    let base = Js::opened(jm, spk_hash(&pend_spk(&cw.s)), cw.s.clock, 0);
    if side == YES {
        Js { status: STATUS_DISPUTE, side: YES, yes: 10 * UNIT, yes_u: 10, no: 0, no_u: 0, start: 0, ..base }
    } else {
        Js { status: STATUS_DISPUTE, side: NO, no: 10 * UNIT, no_u: 10, yes: 0, yes_u: 0, start: 0, ..base }
    }
}

/// A forged resolved pool for the receipt `r`: `r`'s side the loser, a base equal to all it would hold.
fn forged_pool(r: &LStake) -> Js {
    let jm = r.s.m;
    let fake = Js { status: STATUS_RESOLVED, side: YES, anchor: r.s.anchor, w_left: 0, l_left: r.s.amt, base: DISPUTE_BASE as i64 + r.value as i64, start: 0, ..Js::opened(jm, [0; 32], r.s.anchor, 0) };
    Js { no: r.s.amt, no_u: 0, yes: 0, yes_u: 0, ..fake }
}

/// Every forgery of the review on the contested part `cw` (receipt `r` of the honest opener) fails after the fix:
/// nothing can be written under the claim's id, and what the handle writes under its own id is refused by `decide`
/// (another id than the claim's) and by `settle`/`redeem` (another id than the receipt's).
fn every_forgery_fails(h: &Handle, cw: &LPend, r: &LStake) {
    let cid = cw.id.unwrap();
    assert_ne!(cid, h.id(), "the claim's id is not the handle's");
    cannot_write_under(h, cid, &forged_dispute(cw, YES), "a dispute of the claim");
    cannot_write_under(h, cid, &forged_pool(r), "a pool of the claim");
    cannot_write_under(h, cid, &Js::receipt(x(&attacker()), cw.s.m.judge_market(), YES, 10 * UNIT, 0, r.s.anchor), "a receipt of the claim");
    for side in [YES, NO] {
        let (ld, _) = forge(h, &forged_dispute(cw, side), DISPUTE_BASE, cw.daa + 5);
        let (dt, _, _) = decide_tx(&ld, cw, cw.daa + 10);
        not_accepted("a dispute under the handle's id decides the claim", &dt);
    }
    let (lpool, _) = forge(h, &forged_pool(r), DISPUTE_BASE, cw.daa + 6);
    let (st, _, _) = settle_tx(&lpool, &[r.clone()], cw.daa + 7);
    not_accepted("an honest receipt forfeited into a pool under the handle's id", &st);
}

/// The honest end of a contested false liquidation: the opener's NO leads, the round closes, the part is refuted and
/// restored; the long keeps its margin.
fn honest_refutation(d: &LStake, cw: &LPend) {
    let (dt, _, dw) = decide_tx(d, cw, cw.daa + cw.s.m.judge.round as u64);
    must("the honest dispute resolves onto the contested part", &dt);
    assert_eq!(dw.status, REFUTED);
    let (ft, s) = finalize_tx(&live(&dt, 1, cw.daa + cw.s.m.judge.round as u64, &dw), cw.s.clock, 0);
    must("restore", &ft);
    let Settlement::Restore { part, .. } = s else { panic!("{s:?}") };
    assert_eq!((part.long_key, part.bl, part.bs), (cw.s.long_key, cw.s.bl, cw.s.bs), "the long's position comes back as it was");
}

/// `KpfOpenO.take` with the attacker's handle in the transaction: in 0 the handle (P2PK of id Z, its continuation at out
/// 0), 1 the victim's resting long order (take), 2 the attacker's slot (bind of the part at its expiry), 3 the
/// attacker's funding; out 1 the order's continuation, 2 the part, 3 the slot, 4 change. The short of the part is the
/// attacker's key. `lin` places the part: `Keep` binds it to Z (the review's route 1: the handle authorizes it), `Fresh`
/// a genesis authored by the funding input, `Twin(k)` the same with a twin at output k.
fn take_with_handle(h: &Handle, lo: &LOrder, sl: &LSlot, lock: u64, lin: Lineage) -> (T, OPair) {
    let k = kit();
    let o = &lo.s;
    let n = 1_000_000_000i64;
    let xp = 1_000_000_000i64;
    let bound = 1_200_000_000i64; // the attacker's short bound: bs - x = 2 KAS >= minCounterRate 1.5 KAS
    let b = OBinding { uw_id: sl.idb(), uw_key: sl.s.owner, cover_rate: 600_000_000, expiry: 1_200, premium: 2_000_000 };
    let part = OPair {
        m: o.m,
        size: n,
        bl: xp - o.my_rate,
        bs: bound,
        entry: xp,
        long_key: o.owner,
        short_key: x(&attacker()),
        b,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers { long: maker_of(&lo.op), ..Makers::default() },
    };
    let cover = part.cover_of(n);
    let a = BindArgs::new(lock as i64, b.expiry, 0, &[Claim::output(2, cover)]);
    let ctx = Ctx { lock: lock as i64, seq: 10, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true };
    let b2 = sl.s.bind(&ctx, &a, sl.value as i64).expect("the slot counts the part");
    let take_args = vec![
        iv(2),
        iv(n),
        iv(xp),
        iv(xp),
        iv(bound),
        bv(&x(&attacker())),
        bv(&[0; 32]),
        iv(0),
        iv(0),
        iv(0),
        iv(2),
        iv(0),
        iv(b.expiry),
        iv(b.cover_rate),
        iv(b.premium),
        bv(&k.pair.prefix),
        bv(&k.pair.suffix),
    ];
    let fund = key_in(outpoint(0x32), 900_000_000_000, 0, &attacker());
    let fund_op = fund.outpoint;
    let ins = vec![h.input(), order_in(lo, "take", take_args), slot_in(sl, "bind", bind_args(&a, &uw_kp())), fund];
    let (oc, ov) = order_after(lo, n, o.take_spend(n));
    let id = h.id();
    let mut outs = vec![
        out(1_000_000_000, p2pk_spk(&attacker()), bind(0, id)),
        out(ov, order_spk(&oc.expect("the order rests on")), bind(1, lo.id.unwrap())),
        out(part.floor() as u64, pair_spk(&part), None),
        out(sl.value, bond_spk(&b2), bind(2, sl.id.unwrap())),
        out(600_000_000_000, p2pk_spk(&attacker()), None),
    ];
    match lin {
        Lineage::Keep => outs[2].covenant = bind(0, id),
        _ => place(&mut outs, 2, 3, fund_op, id, lin),
    }
    (build_ext(&ins, outs, lock, &[0, 0, 10, 0]), part)
}

/// `KpfPairO.closeDonor` against a "host" that is the attacker's P2PK handle: in 0 the handle, 1 the donor's listed
/// short (closeDonor), 2 the attacker's funding; out 0 the handle again, 1 the donor's short paid, 2 P3 (the donor's
/// long and the attacker's short), 3 change. `lin` places P3 (`Keep`: under the handle's id, the review's route 2).
fn close_donor_with_handle(lin: Lineage) -> (T, OPair, Handle) {
    let (lp, _, _) = opened();
    let (at, listed) = amend_tx(&lp, S, &short_kp(), 1_000_000_000, 20);
    must("the donor's short lists at 10 KAS", &at);
    let donor = live(&at, 0, 400, &listed);
    let h = make_handle();
    let d = &donor.s;
    let n = d.size;
    let xp = 1_000_000_000i64;
    let mut p3 = d.rest_of(n);
    p3.short_key = x(&attacker());
    (p3.short_bid, p3.short_min, p3.short_tip) = (0, 0, 0);
    p3.mk.donor = maker_of(&donor.op);
    let donor_args = vec![iv(n), iv(xp), iv(d.entry), iv(2), iv(d.bs), bv(&x(&attacker())), iv(0), iv(0), iv(0), iv(d.b.cover_rate), iv(d.b.expiry), iv(d.b.premium), bv(&[0; 32])];
    let spay = kpf_protocol::payoff::owed(n, d.bs - xp, d.short_tip, d.m.p.scale) + donor.value as i64 - d.range_of(n);
    let id = h.id();
    let fund = key_in(outpoint(0x33), 900_000_000_000, 0, &attacker());
    let fund_op = fund.outpoint;
    let ins = vec![h.input(), pair_in(&donor, "closeDonor", donor_args), fund];
    let mut outs = vec![
        out(1_000_000_000, p2pk_spk(&attacker()), bind(0, id)),
        out(spay as u64, key_spk(&d.short_key), None),
        out(p3.floor() as u64, pair_spk(&p3), None),
        out(600_000_000_000, p2pk_spk(&attacker()), None),
    ];
    match lin {
        Lineage::Keep => outs[2].covenant = bind(0, id),
        _ => place(&mut outs, 2, 2, fund_op, id, lin),
    }
    (build_ext(&ins, outs, 0, &[0, seq_quiet(&donor), 0]), p3, h)
}

// ---------------------------------------------------------------- R6-F1: a part under an id someone else holds

/// R6-F1, route 1 (the review's `exploit_take_into_a_held_id_...`): a resting order taken into a part whose covenant id
/// a foreign input already carries (the attacker's P2PK handle authorizes the part as a continuation of its own id).
/// Refused by the order: `take` requires the part's id to be fresh (no input carries it).
#[test]
fn holds_take_into_a_held_id_is_refused_by_the_order() {
    let h = make_handle();
    let lo = order(L, &keypair(46), 0x7a, 0);
    let (t, _) = take_with_handle(&h, &lo, &slot(K), 400, Lineage::Keep);
    refused("an order taken into a part whose id the attacker's handle carries", &t, 1);
}

/// Decision 1: the same take with the part under a fresh id is accepted although the handle is an input of the very
/// transaction; the handle gives no hold on the part: a false liquidation contested in time by an honest staker cannot
/// be decided by anything the handle writes, nothing can be written under the claim's id, and the honest dispute
/// refutes it. The victim's 1,000 KAS margin is safe, as in a pair opened by `fill`.
#[test]
fn holds_a_handle_in_the_take_gives_no_hold_on_the_part_or_its_claim() {
    let h = make_handle();
    let lo = order(L, &keypair(46), 0x7a, 0);
    let (t, part) = take_with_handle(&h, &lo, &slot(K), 400, Lineage::Fresh);
    must("the order enters a fresh part beside the attacker's handle", &t);
    let lp = live(&t, 2, 400, &part);
    let h = handle_at(&t, 0);
    assert_ne!(lp.id, Some(h.id()));
    cannot_write_under(&h, lp.id.unwrap(), &Js::free(x(&attacker()), lp.s.m.judge_market()), "anything under the part's id");
    let lw = proposed(&lp, &liq(L, part.size, FALSE_P, 450), 500);
    assert_ne!(lw.id, lp.id, "the claim has its own fresh id");
    let (cw, r, d) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    every_forgery_fails(&h, &cw, &r);
    honest_refutation(&d, &cw);
    assert_eq!(lp.s.long_at(1_000_000_000), 1_000 * KAS);
}

/// Decision 1, the scan: a second output of the new part's id (a twin made in the same genesis group) is refused by
/// the entering party's script at every entry that starts a lineage: an order's take, a transfer to a signing taker, a
/// pending part's propose, P3 of close-versus-close (both the host and the donor refuse).
#[test]
fn holds_a_second_output_of_a_new_id_is_refused_everywhere() {
    let h = make_handle();
    let lo = order(L, &keypair(46), 0x7a, 0);
    let (t, _) = take_with_handle(&h, &lo, &slot(K), 400, Lineage::Twin(4));
    refused("take with a twin of the part", &t, 1);
    let (lp, sl, _) = opened();
    let (at, listed) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("list the long", &at);
    let ll = live(&at, 0, 400, &listed);
    refused("transfer with a twin of the part", &transfer_tx_with(&ll, L, 1_000_000_000, 900_000_000, &keypair(45), 20, Lineage::Twin(2)).0, 0);
    must("transfer, fresh", &transfer_tx_with(&ll, L, 1_000_000_000, 900_000_000, &keypair(45), 20, Lineage::Fresh).0);
    refused("take with a twin (the pair refuses too)", &take_tx_with(&ll, &order(L, &keypair(46), 0x7a, 0), &sl, 1_000_000_000, 400, true, Lineage::Twin(4)).0, 0);
    refused("take with a twin (the order)", &take_tx_with(&ll, &order(L, &keypair(46), 0x7a, 0), &sl, 1_000_000_000, 400, true, Lineage::Twin(4)).0, 1);
    let c = liq(L, lp.s.size, FALSE_P, 450);
    refused("propose with a twin of the pending part", &propose_tx_with(&lp, &c, &uw_kp(), None, Lineage::Twin(1)).0, 0);
    refused("propose keeping the pair's id", &propose_tx_with(&lp, &c, &uw_kp(), None, Lineage::Keep).0, 0);
    let (host, donor) = two_listed();
    let t = close_vs_close_tx_with(&host, &donor, 1_000_000_000, 1_000_000_000, Lineage::Twin(3)).0;
    refused("close-versus-close with a twin of P3: the host", &t, 0);
    refused("close-versus-close with a twin of P3: the donor", &t, 1);
    let t = close_vs_close_tx_with(&host, &donor, 1_000_000_000, 1_000_000_000, Lineage::Keep).0;
    refused("P3 under the host's id: the host", &t, 0);
    refused("P3 under the host's id: the donor", &t, 1);
    must("close-versus-close, fresh", &close_vs_close_tx_with(&host, &donor, 1_000_000_000, 1_000_000_000, Lineage::Fresh).0);
}

/// Two pairs of one underwriter, the host's long and the donor's short listed at 10 KAS (as `oracle_tests`).
fn two_listed() -> (LPair, LPair) {
    let (lp, _, _) = opened();
    let (at, h) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("host lists its long", &at);
    let host = live(&at, 0, 400, &h);
    let mut d0 = lp.clone();
    d0.op = outpoint(0x5d);
    d0.id = Some(Hash::from_bytes([0x5d; 32]));
    let (at, d) = amend_tx(&d0, S, &short_kp(), 1_000_000_000, 20);
    must("donor lists its short", &at);
    (host, live(&at, 0, 400, &d))
}

/// R6-F1, route 2 (the review's `exploit_close_donor_into_a_held_id_...`): the donor's long moved into P3 under the id
/// of a "host" that is the attacker's P2PK handle. Refused by the donor: P3's id must be fresh.
#[test]
fn holds_close_donor_into_a_held_id_is_refused() {
    let (t, _, _) = close_donor_with_handle(Lineage::Keep);
    refused("closeDonor into the handle's id", &t, 1);
}

/// Decision 1, a host that is a plain-key UTXO: with P3 under a fresh id the donor accepts any host (the host-side
/// checks protect only the host's short, here the attacker's own), and the handle gives no hold on P3 or its claims.
/// The donor's long, which signed nothing, keeps its bound, its entry and the judge.
#[test]
fn holds_a_plain_key_host_gives_no_hold_on_p3() {
    let (t, p3, _) = close_donor_with_handle(Lineage::Fresh);
    must("closeDonor against a plain-key host, P3 fresh", &t);
    let lp3 = live(&t, 2, 450, &p3);
    let h = handle_at(&t, 0);
    assert_ne!(lp3.id, Some(h.id()));
    let lw = proposed(&lp3, &liq(L, p3.size, FALSE_P, 460), 500);
    let (cw, r, d) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    every_forgery_fails(&h, &cw, &r);
    honest_refutation(&d, &cw);
}

/// R6-F1, route 3 (the review's `exploit_an_order_taking_a_genesis_pair_with_a_twin_...`): a look-alike pair written by
/// a genesis together with a P2PK twin of its id. An order taking its listing lands under a FRESH id (the look-alike's
/// own `transfer` and the order both check it), so the twin holds nothing of the part; the old shape, the part kept
/// under the look-alike's id, is refused by the order and by the pair. The order's owner keeps the judge.
#[test]
fn holds_an_order_taking_a_genesis_pair_with_a_twin_lands_under_a_fresh_id() {
    let (lp, sl, _) = opened();
    let mut gp = lp.s.clone();
    (gp.long_ask, gp.mk) = (1_000_000_000, Makers::default());
    let ins = vec![key_in(outpoint(0x35), 900_000_000_000, 0, &attacker())];
    let mut outs = vec![out(lp.value, pair_spk(&gp), None), out(1_000_000_000, p2pk_spk(&attacker()), None), out(500_000_000_000, p2pk_spk(&attacker()), None)];
    let g = genesis_id(outpoint(0x35), &outs, &[0, 1]);
    outs[0].covenant = bind(0, g);
    outs[1].covenant = bind(0, g);
    let t = build_ext(&ins, outs, 0, &[0]);
    must("a genesis of a look-alike pair and a P2PK twin of its id", &t);
    let fake = live(&t, 0, 380, &gp);
    let h = handle_at(&t, 1);
    let lo = order(L, &keypair(46), 0x7a, 0);
    let old = take_tx_with(&fake, &lo, &sl, 1_000_000_000, 400, true, Lineage::Keep).0;
    refused("the part kept under the twin's id: the look-alike's transfer", &old, 0);
    refused("the part kept under the twin's id: the order", &old, 1);
    let (tt, part) = take_tx(&fake, &lo, &sl, 1_000_000_000, 400, true);
    must("the order takes the look-alike's listing into a fresh part", &tt);
    let lpart = live(&tt, 2, 400, &part);
    assert_ne!(lpart.id, Some(h.id()));
    let lw = proposed(&lpart, &liq(L, part.size, FALSE_P, 450), 500);
    let (cw, r, d) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    every_forgery_fails(&h, &cw, &r);
    honest_refutation(&d, &cw);
}

/// R6-F1 against the underwriter (the review's `exploit_a_held_id_refutes_a_true_claim_...`): the attacker holds the id
/// of a pair it wrote itself (a look-alike with a twin, its own short truly past its trigger at 12.50); the honest
/// underwriter vouches the true liquidation and posts its 600 KAS bond; the attacker contests with its own aged stake
/// and forges a NO dispute under the id it holds. `decide` refuses it (the claim has a fresh id of its own); the
/// underwriter defends with 20 KAS on YES and the honest round upholds the claim: its bond comes back.
#[test]
fn holds_a_true_claim_cannot_be_refuted_by_a_forgery() {
    let (lp, _, _) = opened();
    let mut gp = lp.s.clone();
    gp.short_key = x(&attacker());
    let ins = vec![key_in(outpoint(0x37), 900_000_000_000, 0, &attacker())];
    let mut outs = vec![out(lp.value, pair_spk(&gp), None), out(1_000_000_000, p2pk_spk(&attacker()), None), out(500_000_000_000, p2pk_spk(&attacker()), None)];
    let g = genesis_id(outpoint(0x37), &outs, &[0, 1]);
    outs[0].covenant = bind(0, g);
    outs[1].covenant = bind(0, g);
    let t = build_ext(&ins, outs, 0, &[0]);
    must("the attacker's own pair with a twin", &t);
    let own = live(&t, 0, 380, &gp);
    let h = handle_at(&t, 1);
    let truth = 1_250_000_000;
    assert!(own.s.short_liquidatable(truth));
    let lw = proposed(&own, &liq(S, own.s.size, truth, 450), 500);
    assert_ne!(lw.id, Some(h.id()), "the claim's id is fresh even on a lineage the attacker holds");
    let (cw, r, d) = contest_at(&lw, 50, 10 * UNIT as u64, 520);
    every_forgery_fails(&h, &cw, &r);
    let (dt, d2, _) = deposit_tx(&d, YES, &[(stake(31, 20 * UNIT as u64, 0, 0xa1), 31)], 530);
    must("the underwriter defends", &dt);
    let ld = live(&dt, 0, 530, &d2);
    let (dt, _, dw) = decide_tx(&ld, &cw, 530 + cw.s.m.judge.round as u64);
    must("the honest dispute resolves", &dt);
    assert_eq!(dw.status, UPHELD);
    let (ft, s) = finalize_tx(&live(&dt, 1, 640, &dw), cw.s.clock, 0);
    must("the true claim settles", &ft);
    assert!(matches!(s, Settlement::Forfeit { .. }), "{s:?}");
}

/// The same forged dispute cannot reach a part of an honestly opened pair: no UTXO but the lineage's own carries its
/// id, so the forgery cannot be written (the covenant context refuses an output of the id without an input of it).
#[test]
fn holds_a_pair_opened_by_fill_has_no_handle_for_a_forgery() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq(L, lp.s.size, FALSE_P, 450), 500);
    let (cw, _, _) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    let ins = vec![key_in(outpoint(0x34), 10_000_000_000, 0, &attacker())];
    let outs = vec![out(DISPUTE_BASE, stake_spk(&forged_dispute(&cw, YES)), bind(0, cw.id.unwrap()))];
    let t = build_ext(&ins, outs, 0, &[0]);
    assert!(run(&t.0, &t.1).is_err(), "no output of the claim's id without an input of it");
}

// ---------------------------------------------------------------- R6-F2: two claims of one split, one anchor

struct TwoPools {
    pool_a: LStake,
    pool_b: LStake,
    /// A's receipts: the opener's NO, the attacker's YES (the loser), the honest NO answer.
    a: Vec<LStake>,
    /// B's receipts: the opener's NO (the loser), the honest YES defence (the winner).
    b: Vec<LStake>,
}

/// Two pair UTXOs of one lineage (a part restored after a void claim and the remainder) and two claims proposed on
/// them in two independent transactions that land in the same chain block: both pending parts have the same DAA score
/// P = 700, so their disputes have the same anchor; after the fix each claim has its own covenant id.
fn two_disputes_one_anchor() -> TwoPools {
    let (lp, _, _) = opened();
    let half = lp.s.size / 2;
    let (t, w) = propose_tx(&lp, &liq(L, half, FALSE_P, 349), &uw_kp(), None);
    must("a void claim on half", &t);
    let lw = live(&t, 0, 450, &w);
    let rest = live(&t, 1, 450, &lp.s.rest_of(lp.s.size - half));
    let (ft, s) = finalize_tx(&lw, 450, 200);
    must("void: restored", &ft);
    let Settlement::Restore { part, .. } = s else { panic!() };
    let a = live(&ft, 1, 650, &part);
    let wa = proposed(&a, &liq(L, half, FALSE_P, 690), 700);
    let wb = proposed(&rest, &liq(L, half, 900_000_000, 690), 700);
    assert_eq!((wa.daa, wb.daa), (700, 700));
    assert_ne!(wa.id, wb.id, "two claims of one split, one DAA score: two ids");
    let (ca, ra, da) = contest_at(&wa, 70, 10 * UNIT as u64, 720);
    let (cb, rb, db) = contest_at(&wb, 71, 10 * UNIT as u64, 720);
    assert_eq!((ra.s.anchor, rb.s.anchor), (700, 700));
    let (t, da2, ry) = deposit_tx(&da, YES, &[(stake(80, 20 * UNIT as u64, 0, 0xb0), 80)], 730);
    must("the attacker's YES in A", &t);
    let ry = live(&t, 1, 730, &ry[0]);
    let (t, da3, rn) = deposit_tx(&live(&t, 0, 730, &da2), NO, &[(stake(72, 25 * UNIT as u64, 0, 0xb1), 72)], 740);
    must("honest NO in A", &t);
    let rn = live(&t, 1, 740, &rn[0]);
    let (dta, pool_a, _) = decide_tx(&live(&t, 0, 740, &da3), &ca, 840);
    must("A resolves NO", &dta);
    let (t, db2, rw) = deposit_tx(&db, YES, &[(stake(81, 20 * UNIT as u64, 0, 0xb2), 81)], 730);
    must("honest YES in B", &t);
    let rw = live(&t, 1, 730, &rw[0]);
    let (dtb, pool_b, _) = decide_tx(&live(&t, 0, 730, &db2), &cb, 830);
    must("B resolves YES", &dtb);
    TwoPools { pool_a: live(&dta, 0, 840, &pool_a), pool_b: live(&dtb, 0, 830, &pool_b), a: vec![ra, ry, rn], b: vec![rb, rw] }
}

/// R6-F2 (the review's `exploit_receipts_of_two_disputes_with_one_anchor_...`): two claims on parts of one split with
/// equal DAA scores. A's losing YES receipt can no longer be settled as a winner in B's pool (another id: both the pool
/// and the receipt refuse), and each pool pays its own receipts exactly: B's honest winner its 20 KAS and B's share,
/// the attacker's receipt forfeited in A.
#[test]
fn holds_receipts_of_two_claims_with_one_anchor_never_settle_in_each_others_pool() {
    let p = two_disputes_one_anchor();
    assert_ne!(p.pool_a.id, p.pool_b.id);
    let (ry, rw) = (p.a[1].clone(), p.b[1].clone());
    let t = settle_tx(&p.pool_b, &[p.b[0].clone(), ry.clone()], 900).0;
    refused("A's receipt in B's pool: the pool", &t, 0);
    refused("A's receipt in B's pool: the receipt", &t, 2);
    refused("B's winner forfeited in A's pool", &settle_tx(&p.pool_a, &[rw.clone()], 902).0, 0);
    let (st, _, pays) = settle_tx(&p.pool_b, &p.b, 900);
    must("B pays its own", &st);
    assert_eq!(pays[1], rw.value + 9 * UNIT as u64, "B's winner: 20 KAS back and B's whole share of 9 KAS");
    let (st, _, pays) = settle_tx(&p.pool_a, &p.a, 902);
    must("A pays its own", &st);
    assert_eq!(pays[1], 0, "the attacker's losing receipt is forfeited in A");
}

// ---------------------------------------------------------------- R6-F3: the lapse settle cannot be blocked

/// R6-F3 (the review's `exploit_a_party_blocks_the_lapse_settle_forever_by_amending`): the short amends a lapsed pair
/// again and again (quietDaa old only); the lapse settle is due from X + tail + idleDaa by the calendar (DAA 2,400 on
/// the fixture), however young the latest UTXO is: an amend one DAA before does not postpone it.
#[test]
fn holds_a_party_cannot_block_the_lapse_settle_by_amending() {
    let (lp, _, _) = opened();
    let due = lp.s.lapse_from();
    assert_eq!(due, 1_200 + 200 + 1_000);
    let mut cur = lp.clone();
    for daa in [1_400u64, 1_900, due - 1] {
        let (t, c) = amend_tx(&cur, S, &short_kp(), 0, cur.s.m.p.quiet_daa as u64);
        must("the short amends a lapsed pair", &t);
        cur = live(&t, 0, daa, &c);
    }
    refused("one DAA before the calendar's end", &lapse_tx(&cur, due - 1, 0), 0);
    must("the lapse settle on a UTXO one DAA old", &lapse_tx(&cur, due, 0));
}

/// A lapsed pair has no underwriter, so no claim can end it.
#[test]
fn holds_no_claim_is_valid_on_a_lapsed_pair() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq(L, lp.s.size, FALSE_P, 1_390), 1_400);
    let (t, s) = finalize_tx(&lw, 1_400, 200);
    must("void", &t);
    assert!(matches!(s, Settlement::Restore { .. }));
}

// ---------------------------------------------------------------- R6-F4: the judge's bounds

/// A market whose judge is `j`, with the fixture's other parameters.
fn market_with_judge(j: JudgeParams) -> OMarket {
    OMarket { judge: j, ..omarket() }
}

/// An open on market `m` (both orders and the slot of `m`).
fn open_on(m: &OMarket) -> T {
    let mut lo = order(L, &long_kp(), 0x71, 0);
    let mut so = order(S, &short_kp(), 0x72, 0);
    lo.s.m = *m;
    so.s.m = *m;
    let mut sl = slot(K);
    sl.s.m = m.bond_market();
    open_tx(&lo, &so, &sl, &open_spec(), &|_, _| {}).0
}

/// An unbond of a free stake of the judge of `m`.
fn unbond_on(m: &OMarket) -> T {
    let st = Live { op: outpoint(0x94), value: m.judge.min_stake.max(10 * UNIT) as u64, daa: 0, id: None, s: Js::free(x(&keypair(70)), m.judge_market()) };
    let un = Js { status: STATUS_UNBONDING, ..st.s.clone() };
    let ins = vec![stake_in(&st, "unbond", vec![Arg::Sig(keypair(70))]), exec_in(0x95)];
    build_ext(&ins, vec![out(st.value, stake_spk(&un), None), change(90_000_000_000)], 0, &[0, 0])
}

/// R6-F4: the judge's upper bounds hold in the order (fill), and in the judge's own stake, at each bound and one
/// beyond: the least stake (10,000 KAS), the round (3 days), the stake age (30 days), the unbonding delay (90 days),
/// at 10 DAA/s. The library refuses exactly the same markets.
#[test]
fn holds_markets_at_each_judge_bound_open_and_beyond_are_refused() {
    let j = omarket().judge;
    let cases = [
        ("least stake", JudgeParams { min_stake: MIN_STAKE_MAX, ..j }, JudgeParams { min_stake: MIN_STAKE_MAX + 1, ..j }),
        ("round", JudgeParams { round: ROUND_MAX, age: ROUND_MAX, unbond: ROUND_MAX, ..j }, JudgeParams { round: ROUND_MAX + 1, age: AGE_MAX, unbond: UNBOND_MAX, ..j }),
        ("stake age", JudgeParams { age: AGE_MAX, ..j }, JudgeParams { age: AGE_MAX + 1, ..j }),
        ("unbonding", JudgeParams { unbond: UNBOND_MAX, ..j }, JudgeParams { unbond: UNBOND_MAX + 1, ..j }),
    ];
    for (what, at, beyond) in cases {
        let (ma, mb) = (market_with_judge(at), market_with_judge(beyond));
        assert!(ma.check().is_ok(), "{what}: the library admits the bound");
        assert!(mb.check().is_err(), "{what}: the library refuses beyond it");
        must(&format!("{what}: an open at the bound"), &open_on(&ma));
        refused(&format!("{what}: an open beyond the bound (the long order)"), &open_on(&mb), 0);
        must(&format!("{what}: a stake of the judge at the bound"), &unbond_on(&ma));
        refused(&format!("{what}: a stake of the judge beyond the bound"), &unbond_on(&mb), 0);
    }
    let _ = STATUS_RECEIPT;
}

// ---------------------------------------------------------------- item 9: the fix attacked

/// A part entered under a fresh id keeps everything that referred to it: its cover is bound to the slot by the
/// transaction's input index, never by the pair's id, so it renews; it can be claimed (under yet another fresh id),
/// handed over again, and lapse-settled by the calendar. The slot's count does not change with the id (nothing to
/// release or to count twice: the bucket is the expiry's).
#[test]
fn holds_a_part_under_a_new_id_renews_claims_and_lapses() {
    let (lp, sl, _) = opened();
    let (at, listed) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("list", &at);
    let ll = live(&at, 0, 400, &listed);
    let (tt, part) = transfer_tx(&ll, L, 1_000_000_000, 900_000_000, &keypair(45), 20);
    must("hand-over to a signing taker", &tt);
    let np = live(&tt, 1, 450, &part);
    assert_ne!(np.id, lp.id);
    let counted = sl.s.counted_at(np.s.b.expiry);
    assert!(counted >= np.s.cover_of(np.s.size), "the part's cover is still counted at its expiry");
    let (rt, r, _) = renew_tx(&np, &sl, 1_100, 8);
    must("the part renews under its new id", &rt);
    let rp = live(&rt, 0, 1_100, &r);
    assert_eq!(rp.id, np.id, "a renewal keeps the id (nobody enters)");
    let lw = proposed(&np, &liq(L, np.s.size, FALSE_P, 450), 500);
    assert!(lw.id != np.id && lw.id != lp.id);
    must("the part lapse-settles by the calendar", &lapse_tx(&np, np.s.lapse_from(), 0));
}

/// A hand-over while a claim is pending or contested: the claim lives under its own id, so handing over the remainder
/// (which starts yet another id) neither voids the claim nor lets anyone escape or redirect its dispute; the
/// contested part is refuted and restored under the claim's id, and the restored part lives on (amend, claim again).
#[test]
fn holds_a_hand_over_of_the_remainder_leaves_a_claim_and_its_dispute_intact() {
    let (lp, _, _) = opened();
    let half = lp.s.size / 2;
    let (t, w) = propose_tx(&lp, &liq(L, half, FALSE_P, 450), &uw_kp(), None);
    must("a false claim on half", &t);
    let lw = live(&t, 0, 500, &w);
    let rest = live(&t, 1, 500, &lp.s.rest_of(lp.s.size - half));
    let (cw, _r, d) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    let (at, listed) = amend_tx(&rest, L, &long_kp(), 1_000_000_000, 20);
    must("the remainder lists", &at);
    let ll = live(&at, 0, 540, &listed);
    let (tt, part) = transfer_tx(&ll, L, 1_000_000_000, 900_000_000, &keypair(45), 20);
    must("the remainder is handed over", &tt);
    let moved = live(&tt, 1, 560, &part);
    assert!(moved.id != cw.id && moved.id != lp.id);
    let (dt, _, dw) = decide_tx(&d, &cw, 620);
    must("the dispute resolves onto its own contested part, after the hand-over", &dt);
    assert_eq!(dw.status, REFUTED);
    let (ft, s) = finalize_tx(&live(&dt, 1, 620, &dw), cw.s.clock, 0);
    must("restore", &ft);
    let Settlement::Restore { part: back, .. } = s else { panic!("{s:?}") };
    let back = live(&ft, 1, 640, &back);
    assert_eq!(back.id, cw.id, "the restored part continues the claim's id (nobody enters)");
    let (at, _) = amend_tx(&back, S, &short_kp(), 0, 20);
    must("the restored part lives on", &at);
    let again = proposed(&back, &liq(L, back.s.size, FALSE_P, 650), 700);
    assert!(again.id != back.id, "a second claim on the restored part has its own id");
}

/// The ids are carried nowhere else: an output that is not a family template never carries a part's or a claim's id
/// in any shape above (checked in every step of the conservation run, `oracle_conservation_tests`), and a `TransactionOutput`
/// written by the kit for a hand-over is a genesis of exactly one output.
#[test]
fn holds_every_new_lineage_is_a_genesis_of_one_output() {
    let (lp, _, _) = opened();
    let (at, listed) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("list", &at);
    let ll = live(&at, 0, 400, &listed);
    let (tt, _) = transfer_tx(&ll, L, 1_000_000_000, 900_000_000, &keypair(45), 20);
    let ids: Vec<Hash> = tt.0.outputs.iter().filter_map(|o: &TransactionOutput| o.covenant.map(|c| c.covenant_id)).collect();
    assert_eq!(ids.len(), 1);
    assert!(tt.1.iter().all(|e| e.covenant_id != Some(ids[0])), "no input carries the new id");
}

