//! Review 10 (docs/audit/REVIEW-10.md): the oracle-market family at commit `c1b0b1d` (the fix of review 9). Every
//! transaction here runs every input in the real script engine with the covenant context (`common::run`), except the
//! replay of the live runs (`r10_holds_the_live_runs_replay_...`), which builds every transaction with the tool's own
//! builders (`kpf_protocol::oracle_build`, the pinned artifacts) and checks it with `verify::verify` (engine with
//! consensus metering, fee floor, storage mass, block limits).
//!
//! `r10_exploit_` tests execute an attack that works, `r10_holds_` tests an attack that is refused (or a shape that keeps
//! its rule), `r10_bounded_` tests a residual that is measured. Worked numbers use the fixture market of `oraclekit`
//! (term 800, step 100, tail 200, idle 1,000, W 200, quiet 20, mmr 5 %, g 2 %, carrier 0.2 KAS, scale 10^6) unless a
//! test says it uses the testnet defaults (`OMarket::testnet`: scale 10^8, minimum size 0.01 unit).
//!
//! The harness pieces copied from `audit9_tests.rs` (take without a pair, the renewal burst, the top-up, close-versus-close
//! and the seeded walk) are at the end of the file; the walk gained one hostile step (`shred`).

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use common::*;
use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::bond::{BindArgs, Bond, Claim, Ctx};
use kpf_protocol::judge::{NO, UNIT, YES};
use kpf_protocol::oracle::*;
use oraclekit::*;
use secp256k1::Keypair;

const KAS: i64 = 100_000_000;

/// A distinct outpoint per (tag, n), for scenarios with more UTXOs than `common::outpoint` names.
fn uop(tag: u8, n: u32) -> TransactionOutpoint {
    let mut b = [tag; 32];
    b[..4].copy_from_slice(&n.to_le_bytes());
    TransactionOutpoint { transaction_id: TransactionId::from_bytes(b), index: n }
}

/// `KpfOpenO.take` of `n` units of `lo` with no pair in the transaction (as `take_alone`, for a part of the order): in 0
/// the order, 1 the slot (bind, claim OUT_REF + 1 at `xe`), 2 the counterparty's funding (P2PK of `ck`, it authors the
/// part's genesis); out 0 the order's continuation (its refund when `n` is all it has left), 1 the part, 2 the slot, 3
/// the counterparty's change. Returns the transaction, the part, the slot's continuation and the order's.
#[allow(clippy::too_many_arguments)]
fn take_piece(lo: &LOrder, n: i64, sl: &LSlot, ck: &Keypair, xp: i64, e: i64, bound: i64, xe: i64, cr: i64, prem: i64, lock: u64, fop: TransactionOutpoint) -> (T, OPair, Bond, Option<OOpen>) {
    let k = kit();
    let o = &lo.s;
    let sc = o.m.p.scale;
    let (bl, bs, lk, sk) = if o.side == L { (xp - o.my_rate, bound, o.owner, x(ck)) } else { (bound, xp + o.my_rate, x(ck), o.owner) };
    let mut part = OPair {
        m: o.m,
        size: n,
        bl,
        bs,
        entry: e,
        long_key: lk,
        short_key: sk,
        b: OBinding { uw_id: sl.idb(), uw_key: sl.s.owner, cover_rate: cr, expiry: xe, premium: prem },
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers::default(),
    };
    if o.side == L {
        part.mk.long = maker_of(&lo.op);
    } else {
        part.mk.short = maker_of(&lo.op);
    }
    let a = BindArgs::new(lock as i64, xe, 0, &[Claim::output(1, q(n, cr, sc))]);
    let b2 = sl.s.bind(&Ctx { seq: sl.s.m.p.rate, ..slot_ctx(sl, lock) }, &a, sl.value as i64).unwrap_or_else(|_| sl.s.clone());
    let args = vec![
        iv(1),
        iv(n),
        iv(xp),
        iv(e),
        iv(bound),
        bv(&x(ck)),
        bv(&[0; 32]),
        iv(0),
        iv(0),
        iv(0),
        iv(1),
        iv(0),
        iv(xe),
        iv(cr),
        iv(prem),
        bv(&k.pair.prefix),
        bv(&k.pair.suffix),
    ];
    let fv = 1_000_000_000_000i64;
    let ins = vec![order_in(lo, "take", args), slot_in(sl, "bind", bind_args(&a, &uw_kp())), key_in(fop, fv as u64, 0, ck)];
    let spend = o.take_spend(n);
    let pv = part.floor();
    let left = o.size_left - n;
    let rest = (lo.value as i64 - spend).max(0);
    let (first, cont) = if left > 0 {
        let c = OOpen { size_left: left, ..o.clone() };
        (out(rest as u64, order_spk(&c), bind(0, lo.id.unwrap())), Some(c))
    } else {
        (out(rest as u64, key_spk(&o.owner), None), None)
    };
    let mut outs = vec![
        first,
        out(pv as u64, pair_spk(&part), None),
        out(sl.value, bond_spk(&b2), bind(1, sl.id.unwrap())),
        out((fv + (lo.value as i64 - rest) - pv) as u64, p2pk_spk(ck), None),
    ];
    seal(&mut outs, 1, 2, fop);
    (build_ext(&ins, outs, lock, &[0, sl.s.m.p.rate as u64, 0]), part, b2, cont)
}

/// A lapse settle built by the counterparty `fk` with its own funding: in 0 the pair, 1 the funding; out 0 the long's
/// payout when the contract owes one (otherwise the funder's own output: the contract fixes nothing there), 1 the
/// funder's change, 2 the short's payout (M - 1 - 0). Returns the transaction and the (long, short) payouts.
fn lapse_by(lp: &LPair, lock: u64, fk: &Keypair, fop: TransactionOutpoint) -> (T, i64, i64) {
    let (l, s) = lp.s.lapse_out(lp.value);
    let ins = vec![pair_in(lp, "lapseSettle", vec![]), key_in(fop, 10 * KAS as u64, 0, fk)];
    let first = if l > 0 { out(l as u64, key_spk(&lp.s.long_key), None) } else { out(5 * KAS as u64, p2pk_spk(fk), None) };
    let outs = vec![first, out(4 * KAS as u64, p2pk_spk(fk), None), out(s as u64, key_spk(&lp.s.short_key), None)];
    (build_ext(&ins, outs, lock, &[0, 0]), l, s)
}

/// A slot of the attacker's (owner `uw_kp`), written by a genesis at ring position 0 in market `m`.
fn own_slot(m: &OMarket, tag: u8) -> LSlot {
    Live { op: uop(tag, 0), value: 1_000 * KAS as u64, daa: 0, id: Some(Hash::from_bytes([tag; 32])), s: Bond::new(x(&uw_kp()), m.bond_market(), 0) }
}

// ---------------------------------------------------------------- R10-F1: a resting long order taken in small parts

/// R10-F1, with the testnet defaults (scale 10^8, minimum size 0.01 unit, carrier 0.2 KAS). The victim is the order
/// `kpf oracle-order` writes for live run 2's long: 2 units, limit 10.50, margin rate 1 KAS, `--any-slot`, no tip,
/// `--prem-max` 0, and the command's defaults `min-fill` = the market's minimum size and escrow = a full fill + 1 KAS
/// (3.2 KAS). The attacker holds a slot (a genesis at ring position 0: the order accepts any slot of its market) and a
/// second key A. Each round, at DAA 20,000,000:
///   take: in 0 the order (take, n = 0.01 unit, x = e = 10, the short's bound 10.50, cover rate 1.50, expiry 3,024,000,
///   premium 0), 1 the slot (bind), 2 A's funding; out 0 the order's continuation (0.21 KAS less), 1 the part
///   (0.215 KAS: long 9 .. short 10.50 and the carrier), 2 the slot, 3 A's change. Signed by the slot's owner and A.
///   lapse settle (due: 3,024,000 + tail + idle = 11,880,000): the long is owed q(0.01, 10 - 9) = 0.01 KAS, below the
///   0.02 KAS dust floor, so `lapseSettle` pays the whole part to the short, A.
/// The order pays 0.21 KAS a round and gets nothing back; A puts in 0.005 KAS a round and takes 0.215 KAS. Fifteen
/// rounds take 3.15 KAS of the 3.2 KAS escrow (all but what no further round can use). DESIGN-ORACLE-IMPL 27.4 bounds a
/// take into the past by the order's tip and one carrier (0.2 KAS here), for the whole order.
#[test]
fn r10_exploit_small_takes_without_a_pair_drain_a_resting_long_orders_whole_escrow() {
    let m = OMarket::testnet(feed_hash_of(b"review 10: a testnet-default market"));
    assert!(m.check().is_ok());
    let sc = m.p.scale;
    let now = 20_000_000u64;
    let o = OOpen {
        m,
        side: L,
        owner: x(&long_kp()),
        size_left: 2 * sc,
        limit: 1_050_000_000,
        my_rate: 100_000_000,
        min_counter_rate: 0,
        min_fill: m.p.min_size,
        tip: 0,
        active_from: 0,
        expiry_daa: 1_000_000_000,
        uw_accept: [0; 32],
        prem_max: 0,
    };
    let escrow = (o.fill_spend(o.size_left, o.prem_max) + KAS) as u64;
    assert_eq!(escrow, 320_000_000, "the command's default escrow: a full fill and 1 KAS");
    let mut lo = Live { op: uop(0x71, 0), value: escrow, daa: now - 1_000_000, id: Some(Hash::from_bytes([0x71; 32])), s: o.clone() };
    let mut sl = own_slot(&m, 0x5c);
    let att = keypair(77);
    let n = m.p.min_size;
    let (xp, bs, cr, xe) = (1_000_000_000i64, 1_050_000_000i64, 150_000_000i64, m.bond.step);
    let (mut paid, mut back, mut put_in, mut took, mut rounds) = (0i64, 0i64, 0i64, 0i64, 0u32);
    while lo.value as i64 >= o.take_spend(n) {
        let (t, part, b2, cont) = take_piece(&lo, n, &sl, &att, xp, xp, bs, xe, cr, 0, now, uop(0x91, rounds));
        must(&format!("round {rounds}: the take of 0.01 unit with no pair"), &t);
        let spend = lo.value as i64 - t.0.outputs[0].value as i64;
        assert_eq!(spend, q(n, o.my_rate, sc) + m.p.carrier, "the order pays its margin and a carrier");
        paid += spend;
        put_in += part.floor() - spend;
        let lp = live(&t, 1, now, &part);
        assert!(lp.s.lapse_from() <= now);
        let (lt, l, s) = lapse_by(&lp, now, &att, uop(0x92, rounds));
        must(&format!("round {rounds}: the immediate lapse settle"), &lt);
        assert_eq!(l, 0, "the long's 0.01 KAS is below the dust floor: the short gets the whole part");
        if rounds == 0 {
            // Both transactions can be mined: KIP-9's storage mass is at most C * sum(1 / output value), far below the
            // block limit (the smallest output is the 0.215 KAS part).
            let upper = |tx: &kaspa_consensus_core::tx::Transaction| tx.outputs.iter().map(|o| 1_000_000_000_000u64 / o.value.max(1)).sum::<u64>();
            let (st, sl_) = (upper(&t.0), upper(&lt.0));
            assert!(st <= kpf_protocol::tx::BLOCK_STORAGE_LIMIT && sl_ <= kpf_protocol::tx::BLOCK_STORAGE_LIMIT, "{st} {sl_}");
            println!("R10-F1 round 0: storage mass at most {st} (take) and {sl_} (lapse settle) of {}", kpf_protocol::tx::BLOCK_STORAGE_LIMIT);
        }
        back += l;
        took += s;
        lo = live(&t, 0, now, &cont.expect("the order continues"));
        sl = live(&t, 2, now, &b2);
        rounds += 1;
    }
    println!("R10-F1 (testnet defaults): {rounds} rounds; the order paid {paid} sompi and got {back} back; the attacker put in {put_in} and took {took}");
    assert_eq!(rounds, 15);
    assert_eq!((paid, back), (315_000_000, 0), "3.15 KAS of the 3.2 KAS escrow, nothing back");
    assert_eq!(took - put_in, paid, "everything the order paid went to the attacker");
    let documented = q(o.size_left, o.tip, sc) + m.p.carrier;
    assert!(paid - back > 15 * documented, "fifteen times the documented bound of 27.4");
}

/// R10-F1 above the dust floor (the fixture market, 1-unit parts of the fixture order with the command's default
/// `min-fill` = the minimum size and a tip of 0.01 KAS): each part's lapse settle returns the margin, and each part
/// still costs the order one carrier and its tip, which the attacker keeps (the carrier at the lapse settle, as the
/// short; the tip in its change at the take). Ten parts of one unit: 2.1 KAS, against the documented bound for ten
/// units taken into the past (their tip and one carrier, 0.3 KAS). The number of parts is bounded only by
/// `size / max(minFill, minSize)` and by the escrow.
#[test]
fn r10_exploit_each_take_without_a_pair_costs_a_long_order_another_carrier() {
    let now = 30_000u64;
    let m = omarket();
    let lo0 = order(L, &long_kp(), 0x71, now - 100);
    let o = OOpen { min_fill: m.p.min_size, tip: 1_000_000, ..lo0.s.clone() };
    let mut lo = Live { s: o.clone(), ..lo0 };
    let mut sl = slot(K);
    let att = keypair(77);
    let sc = m.p.scale;
    let n = m.p.min_size;
    let (mut loss, mut took, mut put_in) = (0i64, 0i64, 0i64);
    for r in 0..10u32 {
        let (t, part, b2, cont) = take_piece(&lo, n, &sl, &att, 1_000_000_000, 1_000_000_000, 1_220_000_000, 1_200, 320_000_000, o.prem_max, now, uop(0x93, r));
        must("a one-unit take with no pair into the past", &t);
        let spend = lo.value as i64 - t.0.outputs[0].value as i64;
        put_in += part.floor() - (spend - q(n, o.tip, sc));
        let lp = live(&t, 1, now, &part);
        let (lt, l, s) = lapse_by(&lp, now, &att, uop(0x94, r));
        must("its lapse settle", &lt);
        assert_eq!(l, q(n, o.my_rate, sc), "above the dust floor the margin comes back");
        loss += spend - l;
        took += s;
        lo = live(&t, 0, now, &cont.unwrap());
        sl = live(&t, 2, now, &b2);
    }
    assert_eq!(loss, 10 * (q(n, o.tip, sc) + m.p.carrier), "one carrier and the tip per part");
    assert_eq!(loss, 210_000_000);
    assert_eq!(took - put_in, 10 * m.p.carrier, "the attacker keeps the carriers (and the tips, in its change at each take)");
    let documented = q(10 * n, o.tip, sc) + m.p.carrier;
    assert!(loss >= 7 * documented, "seven times the documented bound for the units taken");
    println!("R10-F1 (fixture): 10 parts cost the order {loss} sompi; 27.4 bounds a take of those units by {documented}");
}

/// R10-F1, the counter-check: a short order taken the same way loses only its tip (its carrier comes back with its side
/// at the lapse settle, which pays the short the rest of the part), and a long order whose minimum fill is its whole
/// size can be taken only once (the documented bound).
#[test]
fn r10_holds_a_short_order_or_an_order_filled_whole_loses_only_tip_and_one_carrier() {
    let now = 30_000u64;
    let m = omarket();
    let sc = m.p.scale;
    let so0 = order(S, &short_kp(), 0x72, now - 100);
    let so = Live { s: OOpen { min_fill: m.p.min_size, tip: 1_000_000, ..so0.s.clone() }, ..so0 };
    let n = m.p.min_size;
    // The short order: x >= its limit 9.50, the short's bound x + 2, the long's (the attacker's) at least 0.5 below x.
    let (t, part, _, _) = take_piece(&so, n, &slot(K), &keypair(77), 1_000_000_000, 1_000_000_000, 900_000_000, 1_200, 300_000_000, 0, now, uop(0x95, 0));
    must("a one-unit take of the short order", &t);
    let spend = so.value as i64 - t.0.outputs[0].value as i64;
    let lp = live(&t, 1, now, &part);
    let (lt, _, s) = lapse_by(&lp, now, &keypair(77), uop(0x96, 0));
    must("its lapse settle", &lt);
    assert_eq!(spend - s, q(n, so.s.tip, sc), "the short order loses its tip only");
    // A long order with minFill = its size: a part of it is refused.
    let lo = order(L, &long_kp(), 0x73, now - 100);
    let whole = Live { s: OOpen { min_fill: lo.s.size_left, ..lo.s.clone() }, ..lo };
    let (t, _, _, _) = take_piece(&whole, n, &slot(K), &keypair(77), 1_000_000_000, 1_000_000_000, 1_220_000_000, 1_200, 320_000_000, 0, now, uop(0x97, 0));
    refused("a part of an order whose minimum fill is its size", &t, 0);
}

// ---------------------------------------------------------------- R10-F2: a cure after the expiry

/// R10-F2. A valid liquidation of the long (the fixture pair, X = 1,200; vouched at 9.20 with P = 1,000, so P + W =
/// 1,200 <= X) is pending through the renewal window [1,100, 1,200]: a pending part cannot be renewed. From 1,200 anyone
/// can finalize it, which forfeits the long's part to the short. If the long's cure lands first and after X (at 1,201:
/// one block of ordering, or nobody finalized during [P + W, X]), the cured pair is a state written after its expiry,
/// which since review 9 is never renewed: the underwriter's renewal is refused, the cover lapses, and anyone settles the
/// pair at its entry 10 at 2,400, whatever the price. The long, liquidatable at 9.20 (its equity there 200 KAS), gets
/// its whole margin and its cure payment back (1,434.8 KAS for 434.8 KAS paid in); the short gets 2,000.2 KAS instead
/// of the forfeited part (2,954.2 KAS: the pair less the 46 KAS keeper fee). Before review 9 the underwriter renewed the cured pair late and the cover went
/// on. Control: the same cure landing at 1,199 is renewed.
#[test]
fn r10_exploit_a_cure_after_the_expiry_turns_a_valid_liquidation_into_a_lapse_at_the_entry() {
    let (lp, sl, _) = opened();
    let p = 920_000_000i64;
    assert!(lp.s.long_liquidatable(p));
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p, t0: 1_000, lock: 1_000 }, 1_000);
    assert!(lw.s.cure_hash != [0; 32], "the claim offers a cure");
    let (ft, s) = finalize_tx(&lw, 1_000, 200);
    must("the finalize from 1,200 (valid: P + W = X)", &ft);
    let Settlement::Forfeit { value: forfeit, part: fpart, .. } = s else { panic!("{s:?}") };
    assert_eq!((fpart.long_key, fpart.short_key), (lp.s.short_key, lp.s.short_key));
    let (ct, c) = cure_tx(&lw, &long_kp());
    must("the long's cure (conflicts with the finalize)", &ct);
    let paid_in = ct.0.outputs[1].value as i64 + ct.0.outputs[0].value as i64 - lw.value as i64;
    let late = live(&ct, 1, 1_201, &c);
    let rt = renew_tx(&late, &sl, 1_201, 8).0;
    refused("the underwriter's renewal of a pair cured at 1,201", &rt, 0);
    assert!(verdicts(&rt)[1], "the slot's bind is satisfied: only the pair's date rule refuses");
    let (l, sh) = late.s.lapse_out(late.value);
    must("the lapse settle at the entry from 2,400", &lapse_tx(&late, late.s.lapse_from(), 0));
    println!("R10-F2: the long paid {paid_in} into its cure and gets {l} at the lapse; the short gets {sh} instead of the forfeited part {forfeit}");
    assert_eq!(l - paid_in, q(lp.s.size, lp.s.entry - lp.s.bl, lp.s.m.p.scale), "the long recovers its whole margin at the entry");
    assert!(forfeit - sh > 950 * KAS, "the short loses about its gain of the liquidation");
    // Control: the cure at 1,199 is renewed and stays covered.
    let early = live(&ct, 1, 1_199, &c);
    must("control: a cure landing before the expiry is renewed", &renew_tx(&early, &sl, 1_199, 8).0);
}

/// R10-F2, how the winner keeps its liquidation: a finalize that lands before X, and a renewal that lands before the
/// claim. With P + W < X a finalize at P + W forfeits the part (the forfeited state, born before X, is renewable by
/// the winner's underwriter and anyway belongs to the winner alone); an underwriter that renews in its window before it
/// proposes moves X a term away from any claim.
#[test]
fn r10_holds_a_prompt_finalize_or_a_renewal_before_the_claim_keeps_the_liquidation() {
    let (lp, sl, _) = opened();
    let p = 920_000_000i64;
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p, t0: 800, lock: 800 }, 800);
    let (ft, s) = finalize_tx(&lw, 800, 200);
    must("finalize at 1,000", &ft);
    let Settlement::Forfeit { part, .. } = s else { panic!() };
    let fp = live(&ft, 1, 1_000, &part);
    must("the forfeited part (born before X) renews", &renew_tx(&fp, &sl, 1_100, 8).0);
    // Renew first, then propose: the claim is far from the new expiry.
    let (rt, r, b2) = renew_tx(&lp, &sl, 1_100, 8);
    must("the on-time renewal", &rt);
    let lr = live(&rt, 0, 1_100, &r);
    let lw = proposed(&lr, &ClaimSpec { kind: LIQ, side: L, n: lr.s.size, p, t0: 1_150, lock: 1_150 }, 1_150);
    let (ct, c) = cure_tx(&lw, &long_kp());
    must("a cure after the old expiry", &ct);
    let lc = live(&ct, 1, 1_250, &c);
    must("is renewed at the new expiry's window", &renew_tx(&lc, &live(&rt, 1, 1_100, &b2), 1_950, 8).0);
}

// ---------------------------------------------------------------- lead 2: the renewal rule

/// The fixture market with `idleDaa` 300: `tailDaa + idleDaa` (500) below the term (800), as with the testnet defaults
/// (3.25 + 7 days against 28).
fn short_idle_market() -> OMarket {
    let mut m = omarket();
    m.p.idle_daa = 300;
    assert!(m.check().is_ok());
    m
}

/// Lead 2 (DESIGN-ORACLE-IMPL 27.4, "the testnet defaults do not meet it"), measured. On a market with tail + idle <=
/// term an underwriter that stays silent past every expiry and renews only after the lapse settle has become due keeps
/// resuming the cover, term after term: each such renewal races the due settle (the miner picks), its continuation was
/// accepted before its new expiry, so it is renewable again, and claims are valid again (a liquidation proposed after
/// the renewal settles). Over five terms it charges five premiums (100 KAS, the on-time schedule's number), so it takes
/// no extra premium; what it holds is the choice, at every due date, between letting one side out at the entry and
/// keeping both in. A party ends it by any entry after an expiry (an amend: the date is then fixed).
#[test]
fn r10_bounded_with_tail_plus_idle_below_the_term_a_late_underwriter_resumes_the_cover_every_term() {
    let m = short_idle_market();
    let (lp0, _, _) = opened();
    let p = OPair { m, mk: Makers::default(), ..lp0.s.clone() };
    let mut lp = genesis_pair(p.clone(), p.floor() as u64, 0x86, 300);
    let mut sl = Live { s: Bond::new(x(&uw_kp()), m.bond_market(), 0), ..slot(K) };
    let mut premiums = 0i64;
    for term in 0..5 {
        let due = lp.s.lapse_from();
        let at = due + 50;
        must(&format!("term {term}: the lapse settle is due at {due}"), &lapse_tx(&lp, at, 0));
        let x2 = lp.s.b.expiry + m.bond.term;
        let d = (x2 / m.bond.step - 12 - sl.s.j0).max(0);
        let (t, r, b2) = renew_tx(&lp, &sl, at, d);
        must(&format!("term {term}: a renewal at {at}, after the due settle"), &t);
        premiums += lp.value as i64 - t.0.outputs[0].value as i64;
        lp = live(&t, 0, at, &r);
        sl = live(&t, 1, at, &b2);
        assert!(lp.daa as i64 <= lp.s.b.expiry, "its continuation is renewable again");
    }
    assert_eq!(premiums, 5 * q(p.size, p.b.premium, m.p.scale));
    // The cover is live again: a liquidation proposed after the last renewal is valid and forfeits.
    let at = lp.daa + 10;
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p: 920_000_000, t0: at as i64, lock: at }, at);
    let (ft, s) = finalize_tx(&lw, at as i64, 200);
    must("a liquidation after a renewal that overtook the due settle", &ft);
    assert!(matches!(s, Settlement::Forfeit { .. }), "{s:?}");
    // The party's defence: an amend after the expiry fixes the date.
    let (amt, a) = amend_tx(&lp, L, &long_kp(), 1_000_000_000, 20);
    must("the long amends after the expiry", &amt);
    let fixed = live(&amt, 0, lp.s.b.expiry as u64 + 1, &a);
    refused("no renewal after a party entry past the expiry", &renew_tx(&fixed, &sl, fixed.s.lapse_from() + 50, 8).0, 0);
}

/// Lead 2: a party respending the pair (an amend every quietDaa) inside the renewal window cannot stop an on-time
/// renewal: each new UTXO is accepted before the expiry and is renewable. Only an entry after the expiry fixes the date
/// (the party's choice, its defence of 27.2).
#[test]
fn r10_holds_a_party_respending_in_the_window_cannot_stop_an_on_time_renewal() {
    let (lp, sl, _) = opened();
    let mut cur = lp.clone();
    for (i, d) in [1_100u64, 1_120, 1_140, 1_160, 1_180, 1_200].into_iter().enumerate() {
        let (at, a) = amend_tx(&cur, S, &short_kp(), 1_100_000_000 + i as i64, 20);
        must("the short amends", &at);
        cur = live(&at, 0, d, &a);
        must(&format!("a renewal from the UTXO accepted at {d}"), &renew_tx(&cur, &sl, d, 8).0);
    }
    let (at, a) = amend_tx(&cur, S, &short_kp(), 1_000_000_000, 20);
    must("an amend after the expiry", &at);
    refused("the renewal of a UTXO accepted at 1,201", &renew_tx(&live(&at, 0, 1_201, &a), &sl, 1_201, 8).0, 0);
}

/// Lead 2: who can write a pair state after the expiry. An outsider cannot: an amend needs a party's signature, a
/// transfer a listing, a claim (whose restore or cure is such a state) the underwriter's vouch, a lapse settle its date.
/// So a covered pair loses its renewal only by a party's own entry, a taker of a side its owner listed, a claim of its
/// own underwriter (R10-F2: a cure after X) or the underwriter's silence.
#[test]
fn r10_holds_no_outsider_can_write_a_covered_pair_after_its_expiry() {
    let (lp, _, _) = opened();
    let eve = keypair(88);
    refused("an amend signed by an outsider", &amend_tx(&lp, L, &eve, 1_000_000_000, 20).0, 0);
    refused("a transfer of an unlisted side", &transfer_tx(&lp, L, 1_000_000_000, lp.s.bl, &eve, 20).0, 0);
    let (t, _) = propose_tx(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p: 920_000_000, t0: 1_250, lock: 1_250 }, &eve, None);
    refused("a claim vouched by a key that is not the underwriter's", &t, 0);
    refused("a lapse settle one DAA early", &lapse_tx(&lp, lp.s.lapse_from() - 1, 0), 0);
}

// ---------------------------------------------------------------- lead 3: the documented bounds, re-derived

/// Lead 3 (`r9_bounded_a_late_renewal_...`), swept: a silent underwriter renewing as often as it can at one DAA `d`
/// (every renewal at the same lock time, as in one block), for every `d` from the window's start to five terms after
/// the expiry, never charges more renewals than an on-time underwriter (renewing at X - step, X + term - step, ...) has
/// charged by `d`; and a burst is at most two renewals.
#[test]
fn r10_holds_a_late_renewal_burst_never_runs_ahead_of_the_on_time_schedule() {
    let (lp, sl, _) = opened();
    let (x, step, term) = (lp.s.b.expiry, lp.s.m.bond.step, lp.s.m.bond.term);
    let mut most = 0;
    for d in (1_100u64..=5_200).step_by(25) {
        let (_, _, pays, _) = renew_burst(lp.clone(), sl.clone(), d);
        let on_time = (d as i64 - (x - step)) / term + 1;
        assert!(pays.len() as i64 <= on_time, "at {d}: {} renewals, the on-time schedule {on_time}", pays.len());
        most = most.max(pays.len());
    }
    assert_eq!(most, 2, "a burst is at most two renewals");
}

// ---------------------------------------------------------------- lead 1: the shares

/// Lead 1, an odd cover rate (6 KAS + 1 sompi) on the fixture pair: the short's share ends floor(coverRate / 2) above
/// the entry (13), the long's the rest below it (6.99999999); each side may grow exactly to its end and not one sompi
/// further, in either order.
#[test]
fn r10_holds_an_odd_cover_rate_gives_the_extra_sompi_to_the_long_and_no_side_more() {
    let base = opened().0.s;
    let p = OPair { b: OBinding { cover_rate: 600_000_001, ..base.b }, mk: Makers::default(), ..base.clone() };
    let gp = genesis_pair(p.clone(), p.floor() as u64, 0x85, 400);
    assert_eq!((gp.s.short_reach(), gp.s.long_reach()), (1_300_000_000, 699_999_999));
    refused("the short one sompi past its end", &add_margin_tx(&gp, S, &short_kp(), 100_000_001).0, 0);
    let (t, c) = add_margin_tx(&gp, S, &short_kp(), 100_000_000);
    must("the short to 13", &t);
    let g2 = live(&t, 0, 450, &c);
    refused("then the long one sompi past its end", &add_margin_tx(&g2, L, &long_kp(), 200_000_002).0, 0);
    must("then the long to 6.99999999", &add_margin_tx(&g2, L, &long_kp(), 200_000_001).0);
    // The other order.
    let (t, c) = add_margin_tx(&gp, L, &long_kp(), 200_000_001);
    must("the long first", &t);
    let g3 = live(&t, 0, 450, &c);
    refused("then the short one sompi past 13", &add_margin_tx(&g3, S, &short_kp(), 100_000_001).0, 0);
    must("then the short to 13", &add_margin_tx(&g3, S, &short_kp(), 100_000_000).0);
}

/// Lead 1: a cure to the end of the loser's share, then a hand-over that keeps the leaver's bound (`nb == bl`, the
/// exemption of `handOver`), takes no room of the other side's share. Fixture pair: a true liquidation of the long at
/// 7.60 has the cure bound 7.0756 (inside the long's share, which ends at 7); after the cure and a transfer of the long
/// keeping 7.0756, the short can still grow to 13 and the new long down to 7, not one sompi further.
#[test]
fn r10_holds_a_cure_at_the_share_then_a_hand_over_keeping_the_bound_takes_no_room() {
    let (lp, _, _) = opened();
    let lw = proposed(&lp, &ClaimSpec { kind: LIQ, side: L, n: lp.s.size, p: 760_000_000, t0: 450, lock: 450 }, 500);
    assert_eq!(lw.s.cure_bound, 707_560_000);
    let (ct, c) = cure_tx(&lw, &long_kp());
    must("the cure", &ct);
    let lc = live(&ct, 1, 600, &c);
    let (at, a) = amend_tx(&lc, L, &long_kp(), 1_000_000_000, 20);
    must("the long lists", &at);
    let ll = live(&at, 0, 700, &a);
    let ek = keypair(45);
    let (tt, part) = transfer_tx(&ll, L, 1_000_000_000, ll.s.bl, &ek, 20);
    must("a taker keeping the cured bound", &tt);
    let lt = live(&tt, 1, 800, &part);
    assert_eq!((lt.s.short_reach(), lt.s.long_reach()), (1_300_000_000, 700_000_000));
    refused("the short one sompi past 13", &add_margin_tx(&lt, S, &short_kp(), 100_000_001).0, 0);
    let (t2, c2) = add_margin_tx(&lt, S, &short_kp(), 100_000_000);
    must("the short to 13", &t2);
    let l2 = live(&t2, 0, 900, &c2);
    refused("the new long one sompi past 7", &add_margin_tx(&l2, L, &ek, 7_560_001).0, 0);
    must("the new long to 7", &add_margin_tx(&l2, L, &ek, 7_560_000).0);
}

/// Lead 1, the mirror of `r9_bounded_a_take_without_a_pair_sets_the_shares_by_the_entry_it_writes`: a take of a short
/// order with no pair may write any entry `e <= x`, also below 0. The shares are measured from the entry clamped to 0:
/// the order (the short) can never add margin and the counterparty's long may take the whole headroom; in exchange the
/// lapse settle (the entry clamped to the long's bound) pays the whole part to the short order.
#[test]
fn r10_bounded_a_take_of_a_short_order_with_a_negative_entry_clamps_the_shares_at_zero() {
    let now = 30_000u64;
    let so = order(S, &short_kp(), 0x72, now - 100);
    let n = so.s.size_left;
    let (t, part, _, _) = take_piece(&so, n, &slot(K), &keypair(77), 1_000_000_000, -1_000_000_000_000_000_000, 800_000_000, 1_200, 600_000_000, 0, now, uop(0x98, 0));
    must("a take of the short order at the entry -10^18", &t);
    let lp = live(&t, 1, now, &part);
    assert_eq!(lp.s.long_reach(), -300_000_000);
    refused("the short order's top-up of one sompi per unit", &add_margin_tx(&lp, S, &short_kp(), 1).0, 0);
    must("the attacker's long takes the whole headroom", &add_margin_tx(&lp, L, &keypair(77), 200_000_000).0);
    let (l, s) = lp.s.lapse_out(lp.value);
    assert_eq!((l, s), (0, lp.value as i64), "the lapse settle pays the short order the whole part");
}

// ---------------------------------------------------------------- lead 6: the live runs, replayed with the tool's builders

mod live {
    use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
    use kpf_protocol::bond::{Bond, BondParams, RING};
    use kpf_protocol::judge::{JudgeParams, Js, NO, UNIT, YES};
    use kpf_protocol::oracle::*;
    use kpf_protocol::oracle_build::*;
    use kpf_protocol::state::{KIND_LIQ, LONG, SHORT};
    use kpf_protocol::tx::{p2pk_spk, pubkey_of, KeySet, KeyUtxo, MIN_FEE_RATE};

    const KAS: u64 = 100_000_000;

    pub struct Chain {
        n: u32,
        keys: KeySet,
        pub names: Vec<String>,
    }

    impl Chain {
        pub fn new(seeds: &[u8]) -> Chain {
            let mut keys = KeySet::new();
            for s in seeds {
                keys.add_secret(&[*s; 32]).unwrap();
            }
            Chain { n: 0, keys, names: vec![] }
        }
        pub fn mint(&mut self, key: [u8; 32], sompi: u64, daa: u64) -> KeyUtxo {
            self.n += 1;
            let mut b = [0x7bu8; 32];
            b[..4].copy_from_slice(&self.n.to_le_bytes());
            KeyUtxo { outpoint: TransactionOutpoint { transaction_id: TransactionId::from_bytes(b), index: 0 }, amount: sompi, daa, key }
        }
        /// Signs and verifies (engine with consensus metering, fee floor, storage mass, block limits); a builder's refusal
        /// or a failed verification is a path that no longer works.
        pub fn sign(&mut self, name: &str, b: &kpf_protocol::error::Result<OBuilt>) -> OSigned {
            let b = b.as_ref().unwrap_or_else(|e| panic!("{name}: the builder refuses: {e}"));
            let sg = b.sign(&self.keys).unwrap_or_else(|e| panic!("{name}: {e}"));
            let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
            assert!(v.is_ok(), "{name}: {:?}", v.errors());
            self.names.push(name.into());
            sg
        }
    }

    pub fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
        let o = &sg.tx.outputs[i];
        OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
    }

    fn made_slot(sg: &OSigned, i: usize) -> Bond {
        match &sg.created.iter().find(|(j, _, _)| *j == i).expect("a created slot").1 {
            OObj::Slot(b) => b.clone(),
            o => panic!("not a slot: {}", o.kind()),
        }
    }

    pub fn k(s: u8) -> [u8; 32] {
        pubkey_of(&[s; 32]).unwrap()
    }

    /// The market of live runs 1 and 2 (`docs/LIVE-1.md`: window 1,200, rounds of 600, stake age 1,200, stake unbonding
    /// 600, 10 KAS stakes, term 36,000 of steps of 6,000, tail 1,200, slot unbonding 600, idle 3,000, quiet 100; the rest
    /// the testnet defaults).
    pub fn live_market() -> OMarket {
        let mut m = OMarket::testnet(feed_hash_of(b"review 10: the market of the live runs"));
        m.bond = BondParams { rho_bps: 10_000, step: 6_000, term: 36_000, tail: 1_200, rate: 600, unbond: 600 };
        m.judge = JudgeParams { mu_bps: 12_500, age: 1_200, unbond: 600, round: 600, burn_bps: 1_000, min_stake: UNIT };
        m.p.challenge_daa = 1_200;
        m.p.idle_daa = 3_000;
        m.p.quiet_daa = 100;
        assert!(m.check().is_ok());
        m
    }

    fn orders(m: &OMarket, slot_id: [u8; 32], lk: [u8; 32], sk: [u8; 32]) -> (OOpen, OOpen) {
        let sc = m.p.scale;
        let base = |side: i64, owner: [u8; 32], limit: i64, rate: i64| OOpen {
            m: *m,
            side,
            owner,
            size_left: 2 * sc,
            limit,
            my_rate: rate,
            min_counter_rate: 0,
            min_fill: m.p.min_size,
            tip: 0,
            active_from: 0,
            expiry_daa: 1_000_000_000,
            uw_accept: slot_id,
            prem_max: 2_000_000,
        };
        (base(LONG, lk, 1_050_000_000, 100_000_000), base(SHORT, sk, 950_000_000, 200_000_000))
    }

    /// A slot, two orders and their open at 10 (2 units, range [9, 12], cover rate 6, premium 0.02, expiry 42,000).
    fn opened(c: &mut Chain, m: &OMarket, tag: &str) -> (OLive<OPair>, OLive<Bond>) {
        let (lk, sk, uk, ek) = (k(41), k(42), k(31), k(60));
        let r = MIN_FEE_RATE;
        let f = c.mint(uk, 41 * KAS, 0);
        let sg = c.sign(&format!("{tag}: the underwriter bonds a 40 KAS slot"), &create_slot(m, uk, 40 * KAS, 0, &[f], &uk, r));
        let slot = nth(&sg, 1, Bond::new(uk, m.bond_market(), 0), 0);
        let (lo, so) = orders(m, slot.idb(), lk, sk);
        let f = c.mint(lk, 10 * KAS, 1_000);
        let esc = |o: &OOpen| o.fill_spend(o.size_left, o.prem_max) as u64 + KAS;
        let sg = c.sign(&format!("{tag}: long order"), &create_order(&lo, esc(&lo), &[f], &lk, r));
        let lorder = nth(&sg, 1, lo.clone(), 1_000);
        let f = c.mint(sk, 10 * KAS, 1_000);
        let sg = c.sign(&format!("{tag}: short order"), &create_order(&so, esc(&so), &[f], &sk, r));
        let sorder = nth(&sg, 1, so.clone(), 1_000);
        let terms = OpenTerms { n: 2 * m.p.scale, x: 1_000_000_000, cover_rate: 600_000_000, premium: 2_000_000, expiry: 42_000, lock: 2_000 };
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, p) = open(&lorder, &sorder, &slot, &terms, &[f], &ek, r).unwrap_or_else(|e| panic!("{tag}: the open is refused: {e}"));
        let sg = c.sign(&format!("{tag}: open (executor-funded, the 0.04 KAS premium alone)"), &Ok(b));
        let pair = nth(&sg, 2, p, 2_000);
        let slot = nth(&sg, 3, made_slot(&sg, 3), 2_000);
        assert_eq!(pair.value, 620_000_000, "pair 6.2 KAS, range [9, 12]");
        (pair, slot)
    }

    fn unbond_and_withdraw(c: &mut Chain, m: &OMarket, slot: &OLive<Bond>, tag: &str) {
        let (uk, ek) = (k(31), k(60));
        let r = MIN_FEE_RATE;
        let j0 = slot.s.j0;
        let last = (j0 as usize..j0 as usize + RING).rev().find(|&j| slot.s.ring[j - j0 as usize] != 0).map(|j| j as i64 + 1).unwrap_or(j0);
        let free_at = (last * m.bond.step + m.bond.tail) as u64;
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, ub) = slot_unbond(slot, last - j0, free_at, &[f], &ek, r).unwrap_or_else(|e| panic!("{tag}: the unbond is refused: {e}"));
        let sg = c.sign(&format!("{tag}: unbond the slot after the liability ended"), &Ok(b));
        let unb = nth(&sg, 0, ub, free_at);
        let sg = c.sign(&format!("{tag}: withdraw the slot"), &slot_withdraw(&unb, &uk, r));
        assert!(sg.tx.outputs.iter().any(|o| o.script_public_key == p2pk_spk(&uk) && o.value > 39 * KAS));
    }

    /// Live run 1 (21 transactions and the clean-up) and live run 2 (16), in the order and shape the runs sent them, plus
    /// the renewal live run 2 could not mine, built by the tool's builders on the pinned artifacts and verified.
    pub fn replay() -> usize {
        let (lk, sk, uk, stk, tk, ek) = (k(41), k(42), k(31), k(70), k(46), k(60));
        let mut c = Chain::new(&[41, 42, 31, 70, 46, 60]);
        let r = MIN_FEE_RATE;
        let m = live_market();
        let jm = m.judge_market();
        let sc = m.p.scale;
        // ------------------------------------------------ live run 1
        let f = c.mint(stk, 21 * KAS, 0);
        let sg = c.sign("1: the staker bonds 20 KAS", &create_stake(&m, stk, 20 * KAS, &[f], &stk, r));
        let st = nth(&sg, 1, Js::free(stk, jm), 0);
        let f = c.mint(uk, 31 * KAS, 0);
        let sg = c.sign("1: the underwriter bonds 30 KAS of stake", &create_stake(&m, uk, 30 * KAS, &[f], &uk, r));
        let ust = nth(&sg, 1, Js::free(uk, jm), 0);
        let (pair, slot) = opened(&mut c, &m, "1");
        // Claim 1: half the long at 9.20, nobody contests.
        let liq = |t0: i64, n: i64| ClaimTerms { kind: KIND_LIQ, side: LONG, n, price: 920_000_000, t0 };
        let vouch = c.mint(uk, 70 * KAS, 0);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, w1) = propose(&pair, &liq(3_000, sc), &vouch, &[f], &ek, r).unwrap();
        let sg = c.sign("1: claim 1, half the long at 9.20", &Ok(b));
        let pend1 = nth(&sg, 0, w1, 3_000);
        let rest = nth(&sg, 1, pair.s.rest_of(sc), 3_000);
        let vouch = c.mint(uk, 70 * KAS, 0);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, w2) = propose(&rest, &liq(3_100, sc), &vouch, &[f], &ek, r).unwrap();
        let sg = c.sign("1: claim 2, the other half", &Ok(b));
        let pend2 = nth(&sg, 0, w2, 3_100);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, cw, d) = contest(&pend2, &st, 50_000_000, &[f], &ek, r, &Trust::Untraced).unwrap();
        let sg = c.sign("1: contest of claim 2", &Ok(b));
        let (cpart, dispute) = (nth(&sg, 0, cw, 3_200), nth(&sg, 2, d, 3_200));
        let receipt = nth(&sg, 1, Js::receipt(stk, jm, NO, 20 * KAS as i64, 0, 3_100), 3_200);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, dw, pool) = decide(&dispute, &cpart, &[f], &ek, r).unwrap();
        assert_eq!(dw.status, REFUTED, "nobody defended");
        let sg = c.sign("1: resolve, refuted", &Ok(b));
        let (decided, pool) = (nth(&sg, 1, dw, 3_800), nth(&sg, 0, pool, 3_800));
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, s) = finalize(&pend1, &[f], &ek, r).unwrap();
        let sg = c.sign("1: finalize claim 1, forfeit", &Ok(b));
        let Settlement::Forfeit { part, .. } = s else { panic!("{s:?}") };
        let forfeited1 = nth(&sg, 1, part, 4_200);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, s) = finalize(&decided, &[f], &ek, r).unwrap();
        let sg = c.sign("1: finalize claim 2, restored, the bond to the long and the disputer", &Ok(b));
        let Settlement::Restore { part, defender, disputer, .. } = s else { panic!("{s:?}") };
        assert_eq!((defender, disputer), (80_000_000, 20_000_000), "0.8 KAS to the long, 0.2 KAS to the disputer");
        let restored = nth(&sg, 1, part, 3_900);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, p2) = pool_settle(&pool, &[receipt], &[f], &ek, r).unwrap();
        let sg = c.sign("1: settle the pool, the staker's 20 KAS back", &Ok(b));
        let won = nth(&sg, 1, Js::free(stk, jm), 4_000);
        c.sign("1: close the pool", &pool_close(&nth(&sg, 0, p2, 4_000), &ek, r));
        // Claim 3 on the restored part, contested, defended, upheld.
        let vouch = c.mint(uk, 70 * KAS, 0);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, w3) = propose(&restored, &liq(5_300, sc), &vouch, &[f], &ek, r).unwrap();
        let sg = c.sign("1: claim 3 on the restored part", &Ok(b));
        let pend3 = nth(&sg, 0, w3, 5_300);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, cw, d) = contest(&pend3, &won, 50_000_000, &[f], &ek, r, &Trust::Untraced).unwrap();
        let sg = c.sign("1: contest of claim 3", &Ok(b));
        let (cpart, dispute) = (nth(&sg, 0, cw, 5_400), nth(&sg, 2, d, 5_400));
        let r_no = nth(&sg, 1, Js::receipt(stk, jm, NO, won.value as i64, 0, 5_300), 5_400);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, d2) = deposit(&dispute, YES, &[ust.clone()], &[f], &ek, r, &Trust::Untraced).unwrap();
        let sg = c.sign("1: the underwriter defends with 30 KAS on YES", &Ok(b));
        let dispute = nth(&sg, 0, d2, 5_500);
        let r_yes = nth(&sg, 1, Js::receipt(uk, jm, YES, 30 * KAS as i64, 0, 5_300), 5_500);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, dw, pool) = decide(&dispute, &cpart, &[f], &ek, r).unwrap();
        assert_eq!(dw.status, UPHELD);
        let sg = c.sign("1: resolve, upheld", &Ok(b));
        let (decided, pool) = (nth(&sg, 1, dw, 6_100), nth(&sg, 0, pool, 6_100));
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, s) = finalize(&decided, &[f], &ek, r).unwrap();
        let sg = c.sign("1: finalize claim 3, forfeit", &Ok(b));
        let Settlement::Forfeit { part, .. } = s else { panic!("{s:?}") };
        let forfeited2 = nth(&sg, 1, part, 6_100);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, p3) = pool_settle(&pool, &[r_yes, r_no], &[f], &ek, r).unwrap();
        let sg = c.sign("1: settle the pool, the winner's stake 48 KAS", &Ok(b));
        assert_eq!(sg.tx.outputs[1].value, 48 * KAS);
        let winner = nth(&sg, 1, Js::free(uk, jm), 6_200);
        let sg = c.sign("1: close the pool, 2 KAS burned", &pool_close(&nth(&sg, 0, p3, 6_200), &ek, r));
        assert!(sg.tx.outputs.iter().any(|o| o.script_public_key == kpf_protocol::lock::burn_spk() && o.value == 2 * KAS));
        // The clean-up: two lapse settles, the slot's unbond and withdrawal.
        for (i, fp) in [forfeited1, forfeited2].iter().enumerate() {
            let f = c.mint(ek, 10 * KAS, 0);
            c.sign(&format!("1: lapse settle of forfeited part {}", i + 1), &lapse_settle(fp, &[f], &ek, r));
        }
        unbond_and_withdraw(&mut c, &m, &slot, "1");
        // ------------------------------------------------ live run 2
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, su) = stake_unbond(&winner, &[f], &ek, r).unwrap();
        let sg = c.sign("2: unbond the 48 KAS stake", &Ok(b));
        c.sign("2: withdraw it", &stake_withdraw(&nth(&sg, 0, su, 7_000), &uk, r));
        let (pair, slot) = opened(&mut c, &m, "2");
        let vouch = c.mint(uk, 4_907_000_000, 0);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, w) = propose(&pair, &liq(3_000, sc), &vouch, &[f], &ek, r).unwrap();
        let sg = c.sign("2: claim on one unit of the long, the vouch pays the bond", &Ok(b));
        let pend = nth(&sg, 0, w, 3_000);
        let rest = nth(&sg, 1, pair.s.rest_of(sc), 3_000);
        let f = c.mint(lk, 10 * KAS, 0);
        let (b, cured) = cure(&pend, &[f], &lk, r).unwrap();
        assert_eq!(cured.bl, 869_630_000, "the bound 9 -> 8.6963");
        let sg = c.sign("2: cure", &Ok(b));
        let cured = nth(&sg, 1, cured, 3_100);
        // The renewal live run 2 could not mine (the 0.02 KAS premium alone), funded by the underwriter.
        let f = c.mint(uk, 10 * KAS, 0);
        let (b, rs) = renew(&[cured.clone()], &slot, 36_000, &[f], &uk, r).unwrap();
        let sg = c.sign("2: renew the cured part in its window (underwriter-funded)", &Ok(b));
        let renewed = nth(&sg, 0, rs[0].clone(), 36_000);
        let slot = nth(&sg, 1, made_slot(&sg, 1), 36_000);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, a) = amend(&rest, LONG, 1_000_000_000, 0, 0, &[f], &ek, r).unwrap();
        let sg = c.sign("2: the long lists the remainder at 10", &Ok(b));
        let l1 = nth(&sg, 0, a, 36_100);
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, a) = amend(&l1, SHORT, 1_100_000_000, 0, 0, &[f], &ek, r).unwrap();
        let sg = c.sign("2: the short bids 11", &Ok(b));
        let both = nth(&sg, 0, a, 36_200);
        let f = c.mint(ek, 10 * KAS, 0);
        c.sign("2: mutual close at 10.5", &mutual_close(&both, 1_050_000_000, &[f], &ek, r));
        let f = c.mint(ek, 10 * KAS, 0);
        let (b, a) = amend(&renewed, LONG, 1_020_000_000, 0, 0, &[f], &ek, r).unwrap();
        let sg = c.sign("2: the long lists the cured, renewed part at 10.2", &Ok(b));
        let listed = nth(&sg, 0, a, 36_300);
        let f = c.mint(tk, 10 * KAS, 0);
        let (b, t) = transfer(&listed, LONG, 1_020_000_000, 860_000_000, tk, &[f], &tk, r).unwrap();
        let sg = c.sign("2: a third key takes the long at 10.2 with bound 8.6", &Ok(b));
        let moved = nth(&sg, 1, t, 36_400);
        c.sign("2: cooperative close (the third key and the short sign)", &coop_close(&moved, 160_000_000, r));
        unbond_and_withdraw(&mut c, &m, &slot, "2");
        let _ = sk;
        c.names.len()
    }
}

/// Lead 6 (never done before this review): the transaction shapes of live runs 1 and 2 (`docs/LIVE-1.md`,
/// `docs/LIVE-2.md`), replayed on the current templates with the tool's builders: two stakes, slot, orders and open;
/// two partial liquidations, one uncontested and forfeited, one contested and refuted (restored, the bond to the long
/// and the disputer); pool settle and close; a third claim on the restored part, contested, defended, upheld, its pool
/// settled with a loser and closed with a burn; two lapse settles, the slot's unbond and withdrawal; then a stake's
/// unbond and withdrawal, a second market life with a claim and its cure, the renewal of the cured part that run 2
/// could not mine (underwriter-funded), amends, a mutual close, a transfer to a signing taker at the bound run 2 used,
/// a cooperative close and the slot's exit. Every transaction builds, signs and passes `verify::verify`: no ordinary
/// path of the runs became impossible. The pinned artifacts equal the templates compiled from source.
#[test]
fn r10_holds_the_live_runs_replay_on_the_current_templates() {
    let pinned = kpf_protocol::oracle_artifacts::kit();
    let src = kit();
    for (a, b) in [(&pinned.open, &src.open), (&pinned.pair, &src.pair), (&pinned.pending, &src.pending), (&pinned.bond, &src.bond), (&pinned.stake, &src.stake)] {
        assert_eq!(a.hash, b.hash, "{}: the pinned artifact is not the source's template", a.name);
    }
    let n = live::replay();
    println!("live runs replayed: {n} transactions built, signed and verified");
    assert!(n >= 40);
}

/// The builders' top-up of a contract-fixed payout (27.5): a lapse settle whose long payout (0.021 KAS) lies between the
/// contracts' dust floor and the viable output size. The builder writes the long 0.025 KAS; the funder pays the 0.004
/// KAS difference and the fee and nothing else; the short gets at least what the contract owes; the transaction is
/// minable. A cost to whoever settles, bounded by 0.005 KAS per payout, never to the payee or the other party.
#[test]
fn r10_holds_a_topped_up_payout_costs_the_funder_at_most_half_a_hundredth_kas() {
    use kpf_protocol::oracle_build::*;
    let m = live::live_market();
    let sc = m.p.scale;
    let mut c = live::Chain::new(&[41, 42, 31, 60]);
    let (lk, sk, uk, ek) = (live::k(41), live::k(42), live::k(31), live::k(60));
    let p = OPair {
        m,
        size: sc,
        bl: 900_000_000,
        bs: 1_200_000_000,
        entry: 902_100_000,
        long_key: lk,
        short_key: sk,
        b: OBinding { uw_id: [0x51; 32], uw_key: uk, cover_rate: 300_000_000, expiry: 6_000, premium: 0 },
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers::default(),
    };
    let (owed_l, owed_s) = p.lapse_out(p.floor() as u64);
    assert_eq!(owed_l, 2_100_000);
    let pl = OLive { s: p.clone(), value: p.floor() as u64, id: Some(Hash::from_bytes([0x87; 32])), op: uop(0x87, 0), daa: 100 };
    let f = c.mint(ek, 10 * KAS as u64, 0);
    let fin = f.amount;
    let sg = c.sign("a lapse settle with a long payout below the viable size", &lapse_settle(&pl, &[f], &ek, kpf_protocol::tx::MIN_FEE_RATE));
    let to = |k: &[u8; 32]| -> i64 { sg.tx.outputs.iter().filter(|o| o.script_public_key == kpf_protocol::tx::p2pk_spk(k)).map(|o| o.value as i64).sum() };
    assert_eq!(to(&lk), 2_500_000, "topped up to the viable size");
    assert!(to(&sk) >= owed_s, "the short gets what the contract owes");
    let fee = sg.fee as i64;
    assert_eq!(fin as i64 - to(&ek), fee + (2_500_000 - owed_l), "the funder pays the fee and the top-up");
    assert!(2_500_000 - owed_l <= 500_000);
}

// ---------------------------------------------------------------- harness copied from audit9_tests.rs

fn slot_ctx(sl: &LSlot, lock: u64) -> Ctx {
    Ctx { lock: lock as i64, seq: 10, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true }
}

/// `KpfOpenO.take` with no pair in the transaction (DESIGN-ORACLE-IMPL 26.2 "without a pair: binds only the order"):
/// in 0 the order (take, all it has left), 1 the slot (bind, claim OUT_REF + 1 at `xe`), 2 the counterparty's funding
/// (P2PK of `ck`, it authors the part's genesis); out 0 the order's refund, 1 the part (fresh id), 2 the slot, 3 the
/// counterparty's change. The counterparty's side is `bound` (a long order: the short's bound), the entry `e`.
#[allow(clippy::too_many_arguments)]
fn take_alone(lo: &LOrder, sl: &LSlot, ck: &Keypair, xp: i64, e: i64, bound: i64, xe: i64, cr: i64, prem: i64, lock: u64) -> (T, OPair, Bond) {
    let k = kit();
    let o = &lo.s;
    let n = o.size_left;
    let sc = o.m.p.scale;
    let (bl, bs, lk, sk) = if o.side == L { (xp - o.my_rate, bound, o.owner, x(ck)) } else { (bound, xp + o.my_rate, x(ck), o.owner) };
    let mut part = OPair {
        m: o.m,
        size: n,
        bl,
        bs,
        entry: e,
        long_key: lk,
        short_key: sk,
        b: OBinding { uw_id: sl.idb(), uw_key: sl.s.owner, cover_rate: cr, expiry: xe, premium: prem },
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers::default(),
    };
    if o.side == L {
        part.mk.long = maker_of(&lo.op);
    } else {
        part.mk.short = maker_of(&lo.op);
    }
    let a = BindArgs::new(lock as i64, xe, 0, &[Claim::output(1, q(n, cr, sc))]);
    let b2 = sl.s.bind(&slot_ctx(sl, lock), &a, sl.value as i64).unwrap_or_else(|_| sl.s.clone());
    let args = vec![
        iv(1),
        iv(n),
        iv(xp),
        iv(e),
        iv(bound),
        bv(&x(ck)),
        bv(&[0; 32]),
        iv(0),
        iv(0),
        iv(0),
        iv(1),
        iv(0),
        iv(xe),
        iv(cr),
        iv(prem),
        bv(&k.pair.prefix),
        bv(&k.pair.suffix),
    ];
    let fv = 1_000_000_000_000i64;
    let fund = key_in(outpoint(0x91), fv as u64, 0, ck);
    let fop = fund.outpoint;
    let ins = vec![order_in(lo, "take", args), slot_in(sl, "bind", bind_args(&a, &uw_kp())), fund];
    let spend = o.take_spend(n);
    let pv = part.floor();
    let mut outs = vec![
        out(lo.value - spend as u64, key_spk(&o.owner), None),
        out(pv as u64, pair_spk(&part), None),
        out(sl.value, bond_spk(&b2), bind(1, sl.id.unwrap())),
        out((fv - (pv - spend)) as u64, p2pk_spk(ck), None),
    ];
    seal(&mut outs, 1, 2, fop);
    (build_ext(&ins, outs, lock, &[0, 10, 0]), part, b2)
}

/// The underwriter renews `lp` as often as the contracts let it, every renewal at the same lock time `lock` (no
/// calendar time passes between them), advancing its ring just enough each time. Returns the last pair, the slot, what
/// each accepted renewal took from the pair, and the refused attempt that ended the burst.
fn renew_burst(mut lp: LPair, mut sl: LSlot, lock: u64) -> (LPair, LSlot, Vec<i64>, T) {
    let mut pays = vec![];
    loop {
        let Some((_, pay)) = lp.s.renewed() else { panic!("the range is too narrow to renew") };
        let x2 = lp.s.b.expiry + lp.s.m.bond.term;
        let d = (x2 / lp.s.m.bond.step - 12 - sl.s.j0).max(0);
        let (t, r, b2) = renew_tx(&lp, &sl, lock, d);
        if !accepted(&t) {
            return (lp, sl, pays, t);
        }
        pays.push(pay);
        lp = live(&t, 0, lock, &r);
        sl = live(&t, 1, lock, &b2);
    }
}

/// `addMargin` of `side` by `kp` by `delta`: in 0 the pair (sequence quiet), 1 the signer's funding; out 0 the
/// continuation, 1 change.
fn add_margin_tx(lp: &LPair, side: i64, kp: &Keypair, delta: i64) -> (T, OPair) {
    let mut c = OPair { mk: Makers::default(), ..lp.s.clone() };
    if side == L {
        c.bl -= delta;
    } else {
        c.bs += delta;
    }
    let sc = lp.s.m.p.scale;
    let add = kpf_protocol::payoff::q(lp.s.size, delta, sc, sc - 1);
    let ins = vec![pair_in(lp, "addMargin", vec![Arg::Sig(*kp), iv(side), iv(delta)]), key_in(outpoint(0x92), 1_000_000_000_000, 0, kp)];
    let outs = vec![out(lp.value + add as u64, pair_spk(&c), bind(0, lp.id.unwrap())), out(500_000_000_000, p2pk_spk(kp), None)];
    (build_ext(&ins, outs, 0, &[seq_quiet(lp), 0]), c)
}

/// Close-versus-close of two whole pairs at `xp` with P3's binding `b3` and entry `e3` (as `oraclekit::close_vs_close_tx`,
/// the binding chosen by the builder): in 0 the host (closeHost), 1 the donor (closeDonor), 2 the executor; out 0 the
/// host's long, 1 the donor's short, 2 P3 (fresh id), 3 change.
fn cvc(host: &LPair, donor: &LPair, xp: i64, e3: i64, b3: OBinding) -> (T, OPair) {
    let (h, d) = (&host.s, &donor.s);
    let n = h.size;
    let mut p3 = h.rest_of(n);
    p3.b = b3;
    p3.bl = d.bl;
    p3.entry = e3;
    p3.long_key = d.long_key;
    (p3.long_ask, p3.long_min, p3.long_tip) = (d.long_ask, d.long_min, d.long_tip);
    p3.mk = Makers { host: maker_of(&host.op), donor: maker_of(&donor.op), ..Makers::default() };
    let sc = h.m.p.scale;
    let lpay = kpf_protocol::payoff::owed(n, xp - h.bl, h.long_tip, sc);
    let spay = kpf_protocol::payoff::owed(n, d.bs - xp, d.short_tip, sc) + donor.value as i64 - d.range_of(n);
    let host_args = vec![iv(n), iv(xp), iv(e3), iv(2), iv(d.bl), bv(&d.long_key), iv(d.long_ask), iv(d.long_min), iv(d.long_tip), bv(&maker_of(&donor.op))];
    let donor_args = vec![iv(n), iv(xp), iv(e3), iv(2), iv(h.bs), bv(&h.short_key), iv(h.short_bid), iv(h.short_min), iv(h.short_tip), bv(&maker_of(&host.op))];
    let ins = vec![pair_in(host, "closeHost", host_args), pair_in(donor, "closeDonor", donor_args), exec_in(229)];
    let keep_h = host.value as i64 - h.range_of(n);
    let mut outs = vec![
        out(lpay.max(2_000_000) as u64, key_spk(&h.long_key), None),
        out(spay.max(2_000_000) as u64, key_spk(&d.short_key), None),
        out((p3.range_of(n) + keep_h) as u64, pair_spk(&p3), None),
        change(90_000_000_000),
    ];
    seal(&mut outs, 2, 0, host.op);
    (build_ext(&ins, outs, 0, &[seq_quiet(host), seq_quiet(donor), 0]), p3)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
    /// A value around `c`: usually within `w` of it, sometimes far outside.
    fn around(&mut self, c: i64, w: i64) -> i64 {
        let w = w.max(1);
        if self.chance(10) {
            c + (self.below(20 * w as u64) as i64 - 10 * w)
        } else {
            c + (self.below(2 * w as u64 + 1) as i64 - w)
        }
    }
}

/// One live pair of the walk: its UTXO, the true price, the binding its lineage started with, the renewals of the
/// lineage and their DAA scores.
#[derive(Clone)]
struct WP {
    lp: LPair,
    truth: i64,
    b0: OBinding,
    renewals: Vec<u64>,
    /// The DAA the lineage was opened at (the calendar bound on its premiums).
    born: u64,
    /// The underwriter never renews this lineage (its cover lapses; the lapse settle is due at X + tail + idle).
    silent: bool,
}

/// The walk's books.
#[derive(Default, Debug)]
struct WBooks {
    steps: std::collections::BTreeMap<&'static str, usize>,
    refused_as_ruled: usize,
    hostile_refused: usize,
    /// Review 10 (R10-F1): orders taken in small parts with no pair, and the sompi they lost beyond 27.4's bound.
    shreds: usize,
    shred_excess: i64,
}

struct Walk {
    rng: Rng,
    seed: u64,
    step: usize,
    now: u64,
    slot: LSlot,
    pairs: Vec<WP>,
    keys: std::collections::HashMap<[u8; 32], Keypair>,
    next: u32,
    books: WBooks,
    /// Violations of the stated rules that the walk records instead of stopping (seed, step, what).
    violations: Vec<(u64, usize, String)>,
    hostile_takes: bool,
}

const DUST: i64 = 2_000_000;

impl Walk {
    fn new(seed: u64, hostile_takes: bool) -> Walk {
        let mut keys = std::collections::HashMap::new();
        for s in [41u8, 42, 45, 46, 47, 48, 49, 50, 31] {
            keys.insert(x(&keypair(s)), keypair(s));
        }
        Walk {
            rng: Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1),
            seed,
            step: 0,
            now: 300,
            slot: slot(20_000_000_000_000),
            pairs: vec![],
            keys,
            next: 0,
            books: WBooks::default(),
            violations: vec![],
            hostile_takes,
        }
    }
    fn fresh_op(&mut self) -> (kaspa_consensus_core::tx::TransactionOutpoint, Hash) {
        self.next += 1;
        let mut b = [0x5au8; 32];
        b[..4].copy_from_slice(&self.next.to_le_bytes());
        b[4..12].copy_from_slice(&self.seed.to_le_bytes());
        (kaspa_consensus_core::tx::TransactionOutpoint { transaction_id: kaspa_consensus_core::tx::TransactionId::from_bytes(b), index: self.next }, Hash::from_bytes(b))
    }
    fn count(&mut self, what: &'static str) {
        *self.books.steps.entry(what).or_default() += 1;
    }
    fn key(&self, k: &[u8; 32]) -> Keypair {
        *self.keys.get(k).expect("a known key")
    }
    fn entrant(&mut self) -> Keypair {
        let ks = [45u8, 46, 47, 48, 49, 50, 41, 42, 31];
        keypair(ks[self.rng.below(ks.len() as u64) as usize])
    }
    fn violation(&mut self, what: String) {
        self.violations.push((self.seed, self.step, what));
    }
    fn quiet_ok(&self, lp: &LPair) -> bool {
        self.now >= lp.daa + lp.s.m.p.quiet_daa as u64
    }
    fn hostile(&mut self, what: &str, t: &T) {
        assert!(!accepted(t), "seed {} step {}: hostile variant accepted: {what}", self.seed, self.step);
        self.books.hostile_refused += 1;
    }

    /// The lineage rule of review 8 (26.1) and of this review: a part's binding is its lineage's first binding with the
    /// expiry moved by exactly one term per renewal; checked for every live pair after every step.
    fn check_bindings(&self) {
        for w in &self.pairs {
            let mut b = w.b0;
            b.expiry += w.renewals.len() as i64 * w.lp.s.m.bond.term;
            assert_eq!(w.lp.s.b, b, "seed {} step {}: a binding changed without a renewal", self.seed, self.step);
            assert!(w.lp.value as i64 >= w.lp.s.floor(), "seed {} step {}: a pair below its floor", self.seed, self.step);
            assert!(w.lp.s.bs - w.lp.s.bl <= w.lp.s.b.cover_rate, "seed {} step {}: range above the cover rate", self.seed, self.step);
        }
    }

    fn open(&mut self) {
        let m = omarket();
        let xe = (((self.now + 800 + 99) / 100) * 100) as i64;
        let adv = (xe / 100 - self.slot.s.j0 - 12).max(0);
        let mut sp = open_spec();
        sp.lock = self.now;
        sp.xe = xe;
        sp.advance = adv;
        let (op1, id1) = self.fresh_op();
        let (op2, id2) = self.fresh_op();
        let mk = |op, id, side, kp: &Keypair, daa| Live { op, value: 2_000_000_000_000u64, daa, id: Some(id), s: sample_order(&m, side, x(kp)) };
        let (lo, so) = (mk(op1, id1, L, &long_kp(), self.now), mk(op2, id2, S, &short_kp(), self.now));
        let (t, p, b2) = open_tx(&lo, &so, &self.slot, &sp, &|_, _| {});
        if !accepted(&t) {
            self.books.refused_as_ruled += 1;
            return;
        }
        self.count("open");
        let lp = live(&t, 2, self.now, &p);
        self.slot = live(&t, 3, self.now, &b2);
        let truth = 1_000_000_000;
        let silent = self.rng.chance(25);
        self.pairs.push(WP { lp, truth, b0: p.b, renewals: vec![], born: self.now, silent });
    }

    fn tick(&mut self) {
        self.now += 10 + self.rng.below(140);
        for w in self.pairs.iter_mut() {
            let d = (self.rng.below(5) as i64 - 2) * 10_000_000;
            w.truth = (w.truth + d).clamp(800_000_000, 1_300_000_000);
        }
    }

    /// The owner of `side` lists it near the true price (its own signature) unless it is listed, and quietDaa passes.
    fn ensure_listed(&mut self, i: usize, side: i64) {
        let w = self.pairs[i].clone();
        let listed = if side == L { w.lp.s.long_ask > 0 } else { w.lp.s.short_bid > 0 };
        if listed || !self.quiet_ok(&w.lp) {
            return;
        }
        let ask = self.rng.around(w.truth, 20_000_000).max(1);
        let kp = self.key(if side == L { &w.lp.s.long_key } else { &w.lp.s.short_key });
        let (t, c) = amend_tx(&w.lp, side, &kp, ask, w.lp.s.m.p.quiet_daa as u64);
        must("list before a hand-over", &t);
        self.pairs[i].lp = live(&t, 0, self.now, &c);
        self.now += w.lp.s.m.p.quiet_daa as u64;
    }

    fn amend(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        if !self.quiet_ok(&w.lp) {
            return;
        }
        let side = if self.rng.chance(50) { L } else { S };
        let ask = self.rng.around(w.truth, 30_000_000).max(1);
        let kp = self.key(if side == L { &w.lp.s.long_key } else { &w.lp.s.short_key });
        let (t, c) = amend_tx(&w.lp, side, &kp, ask, w.lp.s.m.p.quiet_daa as u64);
        // Hostile: the other side's signature.
        let other = self.key(if side == L { &w.lp.s.short_key } else { &w.lp.s.long_key });
        if other.x_only_public_key() != kp.x_only_public_key() {
            let (h, _) = amend_tx(&w.lp, side, &other, ask, w.lp.s.m.p.quiet_daa as u64);
            self.hostile("amend signed by the other party", &h);
        }
        if ask > w.lp.s.m.p.price_max {
            assert!(!accepted(&t));
            return;
        }
        must("amend", &t);
        self.count("amend");
        self.pairs[i].lp = live(&t, 0, self.now, &c);
    }

    fn add_margin(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        if !self.quiet_ok(&w.lp) {
            return;
        }
        let p = &w.lp.s;
        let side = if self.rng.chance(50) { L } else { S };
        let head = p.b.cover_rate - (p.bs - p.bl);
        let delta = 1 + self.rng.below((2 * head).max(2) as u64) as i64;
        let kp = self.key(if side == L { &p.long_key } else { &p.short_key });
        let (t, c) = add_margin_tx(&w.lp, side, &kp, delta);
        // Review 9 (R9-F3): the growing side leaves the other its reach (its bound or its share's end).
        let share_ok = if side == L { c.bl >= p.growth_limit(L) } else { c.bs <= p.growth_limit(S) };
        let legal = c.bl >= 0 && c.bl < c.bs && c.bs <= p.m.p.price_max && c.bs - c.bl <= p.b.cover_rate && share_ok;
        assert_eq!(accepted(&t), legal, "seed {} step {}: addMargin delta {delta} head {head}", self.seed, self.step);
        if legal {
            self.count("addMargin");
            self.pairs[i].lp = live(&t, 0, self.now, &c);
        } else {
            self.books.refused_as_ruled += 1;
        }
    }

    /// The stated rule of `KpfPairO.transfer` (`handOver`) for a whole hand-over of `side` at `xp` to `ek` posting `nb`:
    /// the listing, the no-downgrade rule, health at `xp`, the range within the cover rate and (review 9, R9-F3) the
    /// stayer's reach kept unless the entrant keeps the leaver's bound.
    fn hand_over_rule(p: &OPair, side: i64, xp: i64, nb: i64, ek: &[u8; 32]) -> bool {
        let mm = p.m.p.mmr_bps;
        xp >= 0
            && xp <= p.m.p.price_max
            && *ek != p.b.uw_key
            && if side == L {
                p.long_ask > 0
                    && xp >= p.long_ask
                    && nb >= 0
                    && nb <= p.bl
                    && (xp as i128) * (10_000 - mm) as i128 >= (nb as i128) * 10_000
                    && p.bs - nb <= p.b.cover_rate
                    && (nb == p.bl || nb >= p.growth_limit(L))
            } else {
                p.short_bid > 0
                    && xp <= p.short_bid
                    && nb >= p.bs
                    && nb <= p.m.p.price_max
                    && (nb as i128) * 10_000 >= (xp as i128) * (10_000 + mm) as i128
                    && nb - p.bl <= p.b.cover_rate
                    && (nb == p.bs || nb <= p.growth_limit(S))
            }
    }

    /// A whole transfer of `side` at `xp` to `ek` posting `nb`, with the part `part` and the leaver's payout `pay` as
    /// the builder writes them.
    fn transfer_custom(lp: &LPair, side: i64, xp: i64, nb: i64, ek: &Keypair, part: &OPair, pay: i64) -> T {
        let p = &lp.s;
        let leaver = if side == L { p.long_key } else { p.short_key };
        let args = vec![iv(side), iv(p.size), iv(xp), iv(nb), bv(&x(ek)), bv(&[0; 32]), iv(1)];
        let ins = vec![pair_in(lp, "transfer", args), key_in(outpoint(223), 1_000_000_000_000, 0, ek)];
        let keep = lp.value as i64 - p.range_of(p.size) + if pay < DUST { pay } else { 0 };
        let mut outs = vec![out(pay.max(DUST) as u64, key_spk(&leaver), None), out((part.range_of(p.size) + keep) as u64, pair_spk(part), None), out(100_000_000_000, p2pk_spk(ek), None)];
        seal(&mut outs, 1, 0, lp.op);
        build_ext(&ins, outs, 0, &[p.m.p.quiet_daa as u64, 0])
    }

    fn transfer(&mut self, i: usize) {
        let ls = if self.rng.chance(50) { L } else { S };
        self.ensure_listed(i, ls);
        let w = self.pairs[i].clone();
        if !self.quiet_ok(&w.lp) {
            return;
        }
        let p = w.lp.s.clone();
        let side = if p.long_ask > 0 && (p.short_bid == 0 || self.rng.chance(50)) { L } else if p.short_bid > 0 { S } else { return };
        let ek = self.entrant();
        let sc = p.m.p.scale;
        let mm = p.m.p.mmr_bps;
        let off = self.rng.below(3) as i64 * 50_000_000;
        let (xp, nb) = if side == L {
            let xp = self.rng.around(p.long_ask, 20_000_000).max(0);
            (xp, self.rng.around(p.bl - off, 30_000_000))
        } else {
            let xp = self.rng.around(p.short_bid, 20_000_000).max(0);
            (xp, self.rng.around(p.bs + off, 30_000_000))
        };
        let pay = if side == L { kpf_protocol::payoff::owed(p.size, xp - p.bl, p.long_tip, sc) } else { kpf_protocol::payoff::owed(p.size, p.bs - xp, p.short_tip, sc) };
        let mut part = p.rest_of(p.size);
        part.mk.parent = maker_of(&w.lp.op);
        if side == L {
            part.bl = nb;
            part.long_key = x(&ek);
            (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
        } else {
            part.bs = nb;
            part.short_key = x(&ek);
            (part.short_bid, part.short_min, part.short_tip) = (0, 0, 0);
        }
        let _ = mm;
        let legal = Self::hand_over_rule(&p, side, xp, nb, &x(&ek));
        let t = Self::transfer_custom(&w.lp, side, xp, nb, &ek, &part, pay);
        assert_eq!(accepted(&t), legal, "seed {} step {}: transfer side {side} x {xp} nb {nb}", self.seed, self.step);
        if !legal {
            self.books.refused_as_ruled += 1;
            return;
        }
        // Hostile variants: the stayer's bound, key or listing moved, a binding field changed, the leaver underpaid.
        let mut h = part.clone();
        match self.rng.below(5) {
            0 => {
                if side == L {
                    h.bs += 1
                } else {
                    h.bl -= 1
                }
            }
            1 => {
                if side == L {
                    h.short_key = x(&keypair(49))
                } else {
                    h.long_key = x(&keypair(49))
                }
            }
            2 => h.b.expiry -= 100,
            3 => h.entry += 1,
            _ => {
                if side == L {
                    h.short_bid += 1
                } else {
                    h.long_ask += 1
                }
            }
        }
        if h != part {
            self.hostile("transfer moving the stayer or the binding", &Self::transfer_custom(&w.lp, side, xp, nb, &ek, &h, pay));
        }
        if pay >= DUST + 1 {
            self.hostile("transfer underpaying the leaver", &Self::transfer_custom(&w.lp, side, xp, nb, &ek, &part, pay - 1));
        }
        self.count("transfer");
        self.keys.insert(x(&ek), ek);
        self.pairs[i].lp = live(&t, 1, self.now, &part);
    }

    /// `closeDonor` with no host, P3 as the builder writes it (`p3`), the short paid `spay`.
    fn donor_custom(donor: &LPair, xp: i64, e3: i64, bs3: i64, sk3: [u8; 32], p3: &OPair, spay: i64) -> T {
        let d = &donor.s;
        let args = vec![iv(d.size), iv(xp), iv(e3), iv(1), iv(bs3), bv(&sk3), iv(0), iv(0), iv(0), bv(&[0; 32])];
        let fund = key_in(outpoint(0xa1), 1_000_000_000_000, 0, &short_kp());
        let fop = fund.outpoint;
        let mut outs = vec![out(spay.max(DUST) as u64, key_spk(&d.short_key), None), out(p3.floor() as u64, pair_spk(p3), None), out(400_000_000_000, p2pk_spk(&short_kp()), None)];
        seal(&mut outs, 1, 1, fop);
        build_ext(&[pair_in(donor, "closeDonor", args), fund], outs, 0, &[d.m.p.quiet_daa as u64, 0])
    }

    fn donor_alone(&mut self, i: usize) {
        self.ensure_listed(i, S);
        let w = self.pairs[i].clone();
        let d = w.lp.s.clone();
        if !self.quiet_ok(&w.lp) || d.short_bid == 0 {
            return;
        }
        let sk3 = if self.rng.chance(5) { d.b.uw_key } else { x(&self.entrant()) };
        let xp = self.rng.around(d.short_bid, 20_000_000).max(0);
        let off = self.rng.below(3) as i64 * 50_000_000;
        let bs3 = self.rng.around(d.bs + off, 30_000_000);
        let e3 = if self.rng.chance(70) { d.entry } else { self.rng.around(d.entry + 10_000_000, 15_000_000) };
        let mut p3 = d.rest_of(d.size);
        p3.short_key = sk3;
        p3.bs = bs3;
        p3.entry = e3;
        (p3.short_bid, p3.short_min, p3.short_tip) = (0, 0, 0);
        p3.mk = Makers { donor: maker_of(&w.lp.op), ..Makers::default() };
        let sc = d.m.p.scale;
        let mut spay = kpf_protocol::payoff::owed(d.size, d.bs - xp, d.short_tip, sc) + w.lp.value as i64 - d.range_of(d.size);
        if spay < DUST {
            spay = 0;
        }
        let legal = xp >= 0 && xp <= d.m.p.price_max && xp <= d.short_bid && bs3 >= d.bs && bs3 <= d.m.p.price_max && e3 == d.entry && sk3 != d.b.uw_key && bs3 - d.bl <= d.b.cover_rate && (bs3 == d.bs || bs3 <= d.growth_limit(S));
        let t = Self::donor_custom(&w.lp, xp, e3, bs3, sk3, &p3, spay);
        assert_eq!(accepted(&t), legal, "seed {} step {}: closeDonor alone x {xp} bs3 {bs3} e3 {e3}", self.seed, self.step);
        if !legal {
            self.books.refused_as_ruled += 1;
            return;
        }
        let mut h = p3.clone();
        match self.rng.below(5) {
            0 => h.bl -= 1,
            1 => h.long_key = x(&keypair(49)),
            2 => h.b.cover_rate += 1,
            3 => h.b.premium = 0,
            _ => h.long_ask += 1,
        }
        if h != p3 {
            self.hostile("closeDonor moving the passive long or its binding", &Self::donor_custom(&w.lp, xp, e3, bs3, sk3, &h, spay));
        }
        if spay > DUST {
            self.hostile("closeDonor underpaying the short", &Self::donor_custom(&w.lp, xp, e3, bs3, sk3, &p3, spay - 1));
        }
        // The passive long: at every price its payoff in P3 is at least what it had, and its lapse payout too.
        for pr in [800_000_000i64, 1_000_000_000, 1_200_000_000, 1_500_000_000] {
            assert!(p3.long_at(pr) >= d.long_at(pr), "seed {} step {}: the passive long lost at {pr}", self.seed, self.step);
        }
        let lap = |p: &OPair| q(p.size, p.entry.clamp(p.bl, p.bs) - p.bl, p.m.p.scale);
        assert!(lap(&p3) >= lap(&d), "seed {} step {}: the passive long's lapse payout fell", self.seed, self.step);
        self.count("closeDonor alone");
        self.pairs[i].lp = live(&t, 1, self.now, &p3);
    }

    /// `closeHost` with no donor, P3 as the builder writes it, the long paid `lpay`.
    fn host_custom(host: &LPair, xp: i64, e3: i64, bl3: i64, lk3: [u8; 32], p3: &OPair, lpay: i64) -> T {
        let h = &host.s;
        let args = vec![iv(h.size), iv(xp), iv(e3), iv(1), iv(bl3), bv(&lk3), iv(0), iv(0), iv(0), bv(&[0; 32])];
        let fund = key_in(outpoint(0xa2), 1_000_000_000_000, 0, &long_kp());
        let fop = fund.outpoint;
        let keep = host.value as i64 - h.range_of(h.size) + if lpay < DUST { lpay } else { 0 };
        let mut outs = vec![out(lpay.max(DUST) as u64, key_spk(&h.long_key), None), out((p3.range_of(h.size) + keep) as u64, pair_spk(p3), None), out(400_000_000_000, p2pk_spk(&long_kp()), None)];
        seal(&mut outs, 1, 1, fop);
        build_ext(&[pair_in(host, "closeHost", args), fund], outs, 0, &[h.m.p.quiet_daa as u64, 0])
    }

    fn host_alone(&mut self, i: usize) {
        self.ensure_listed(i, L);
        let w = self.pairs[i].clone();
        let h = w.lp.s.clone();
        if !self.quiet_ok(&w.lp) || h.long_ask == 0 {
            return;
        }
        let lk3 = if self.rng.chance(5) { h.b.uw_key } else { x(&self.entrant()) };
        let xp = self.rng.around(h.long_ask, 20_000_000).max(0);
        let off = self.rng.below(3) as i64 * 50_000_000;
        let bl3 = self.rng.around(h.bl - off, 30_000_000);
        let e3 = if self.rng.chance(70) { h.entry } else { self.rng.around(h.entry - 10_000_000, 15_000_000) };
        let mut p3 = h.rest_of(h.size);
        p3.long_key = lk3;
        p3.bl = bl3;
        p3.entry = e3;
        (p3.long_ask, p3.long_min, p3.long_tip) = (0, 0, 0);
        p3.mk = Makers { host: maker_of(&w.lp.op), ..Makers::default() };
        let lpay = kpf_protocol::payoff::owed(h.size, xp - h.bl, h.long_tip, h.m.p.scale);
        let legal = xp >= 0 && xp <= h.m.p.price_max && xp >= h.long_ask && bl3 >= 0 && bl3 <= h.bl && e3 == h.entry && lk3 != h.b.uw_key && h.bs - bl3 <= h.b.cover_rate && (bl3 == h.bl || bl3 >= h.growth_limit(L));
        let t = Self::host_custom(&w.lp, xp, e3, bl3, lk3, &p3, lpay);
        assert_eq!(accepted(&t), legal, "seed {} step {}: closeHost alone x {xp} bl3 {bl3} e3 {e3}", self.seed, self.step);
        if !legal {
            self.books.refused_as_ruled += 1;
            return;
        }
        let mut hh = p3.clone();
        match self.rng.below(4) {
            0 => hh.bs += 1,
            1 => hh.short_key = x(&keypair(49)),
            2 => hh.b.expiry += 100,
            _ => hh.short_bid += 1,
        }
        if hh != p3 {
            self.hostile("closeHost moving the passive short or its binding", &Self::host_custom(&w.lp, xp, e3, bl3, lk3, &hh, lpay));
        }
        if lpay > DUST {
            self.hostile("closeHost underpaying the long", &Self::host_custom(&w.lp, xp, e3, bl3, lk3, &p3, lpay - 1));
        }
        for pr in [800_000_000i64, 1_000_000_000, 1_200_000_000, 1_500_000_000] {
            assert!(p3.short_at(pr) >= h.short_at(pr), "seed {} step {}: the passive short lost at {pr}", self.seed, self.step);
        }
        self.count("closeHost alone");
        self.pairs[i].lp = live(&t, 1, self.now, &p3);
    }

    /// Close-versus-close of two pairs of one binding: the first lists its long, the second its short at the first's
    /// entry, then the two close against each other.
    fn combined(&mut self) {
        let n = self.pairs.len();
        if n < 2 {
            return;
        }
        let i = self.rng.below(n as u64) as usize;
        let Some(j) = (0..n).find(|&j| j != i && self.pairs[j].lp.s.b == self.pairs[i].lp.s.b) else { return };
        let (a, b) = (self.pairs[i].clone(), self.pairs[j].clone());
        if a.lp.s.b != b.lp.s.b || a.lp.s.size != b.lp.s.size || !self.quiet_ok(&a.lp) || !self.quiet_ok(&b.lp) || b.lp.s.entry != a.lp.s.entry {
            return;
        }
        let xp = a.lp.s.entry;
        let (ta, ha) = amend_tx(&a.lp, L, &self.key(&a.lp.s.long_key), xp, 20);
        let (tb, db) = amend_tx(&b.lp, S, &self.key(&b.lp.s.short_key), xp, 20);
        if !accepted(&ta) || !accepted(&tb) {
            return;
        }
        let (host, donor) = (live(&ta, 0, self.now, &ha), live(&tb, 0, self.now, &db));
        // The host's rule bl3 <= bl and the donor's bs3 >= bs: P3 is no narrower on either side than the pair it keeps.
        if host.s.bs - donor.s.bl > host.s.b.cover_rate || xp < host.s.bl || xp > donor.s.bs || donor.s.bl > host.s.bl || host.s.bs < donor.s.bs {
            return;
        }
        // Review 9 (R9-F3): each passive side keeps its reach (P3's entering bound kept or within the share).
        let host_ok = donor.s.bl == host.s.bl || donor.s.bl >= host.s.growth_limit(L);
        let donor_ok = host.s.bs == donor.s.bs || host.s.bs <= donor.s.growth_limit(S);
        if !host_ok || !donor_ok {
            let (t, _) = cvc(&host, &donor, xp, xp, host.s.b);
            assert!(!accepted(&t), "seed {} step {}: close-versus-close taking a passive side's share", self.seed, self.step);
            self.books.refused_as_ruled += 1;
            return;
        }
        let (t, p3) = cvc(&host, &donor, xp, xp, host.s.b);
        must(&format!("seed {} step {}: close-versus-close", self.seed, self.step), &t);
        self.count("close-versus-close");
        let (lo, hi) = (i.min(j), i.max(j));
        let w = WP { lp: live(&t, 2, self.now + 20, &p3), truth: a.truth, b0: a.b0, renewals: a.renewals.clone(), born: a.born, silent: a.silent };
        self.pairs.remove(hi);
        self.pairs.remove(lo);
        self.pairs.push(w);
    }

    /// The underwriter's renewal, from one step before the expiry; a late underwriter (no renewal on some steps) renews
    /// late, and as often in a row as the contracts allow. The pair's verdict is compared with the rule of review 9 (the
    /// pair UTXO accepted no later than its expiry); every accepted renewal is checked against the calendar bound of
    /// section 27: a lineage opened at `born` has paid at most `1 + (now + step - born) / term` premiums (the open's
    /// included), the most an on-time underwriter charges by then. A catch-up renewal (less than half a term after the
    /// previous one: the residual of R9-F2) is counted, not a violation.
    fn renew(&mut self, i: usize) {
        loop {
            let w = self.pairs[i].clone();
            let p = &w.lp.s;
            let step = p.m.bond.step as u64;
            if (p.b.expiry as u64) > self.now + step || self.now >= p.lapse_from() {
                return;
            }
            let Some((r, pay)) = p.renewed() else { return };
            let x2 = p.b.expiry + p.m.bond.term;
            let d = (x2 / p.m.bond.step - 12 - self.slot.s.j0).max(0);
            let (t, r2, b2) = renew_tx(&w.lp, &self.slot, self.now, d);
            let rule = w.lp.daa as i64 <= p.b.expiry;
            assert_eq!(verdicts(&t)[0], rule, "seed {} step {}: renewal of a UTXO accepted at {} with expiry {}", self.seed, self.step, w.lp.daa, p.b.expiry);
            if !accepted(&t) {
                self.books.refused_as_ruled += 1;
                if !rule {
                    self.count("renewal refused: the UTXO is younger than its expiry");
                }
                return;
            }
            assert_eq!(r2, r);
            // Hostile: one sompi more than the premium leaves the pair.
            let mut h = t.clone();
            h.0.outputs[0].value -= 1;
            h.0.finalize();
            assert!(!verdicts(&h)[0], "seed {} step {}: a renewal taking one sompi more", self.seed, self.step);
            self.books.hostile_refused += 1;
            if let Some(&prev) = w.renewals.last() {
                if self.now - prev < (p.m.bond.term / 2) as u64 {
                    self.count("renew: a catch-up of a late renewal");
                }
            }
            self.count("renew");
            self.slot = live(&t, 1, self.now, &b2);
            self.pairs[i].lp = live(&t, 0, self.now, &r2);
            self.pairs[i].renewals.push(self.now);
            let paid = 1 + self.pairs[i].renewals.len() as i64;
            let most = 1 + (self.now as i64 + step as i64 - w.born as i64) / p.m.bond.term;
            if paid > most {
                self.violation(format!("premiums ahead of the calendar: {paid} by DAA {} for a lineage opened at {} (premium {pay})", self.now, w.born));
            }
        }
    }

    /// A claim on the whole pair or on half of it, then (W later, the other pairs' clocks moving too) its finalize: a
    /// close by either party at the true price, a true liquidation (cured half the time), or a false liquidation that
    /// is contested: in time and refuted by an honest judge, in time and upheld because the colluders' YES stake leads
    /// (the pool then has a losing receipt), or after the window (the verdict ignored). Every pool is settled and
    /// closed. A claim landing too close to the expiry is void and restores the part; the remainder of a partial claim
    /// continues the lineage.
    fn claim(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        let p = w.lp.s.clone();
        if p.long_key == p.short_key {
            return;
        }
        let ms = p.m.p.min_size;
        let n = if self.rng.chance(30) && p.size / 2 >= ms && p.size - p.size / 2 >= ms { p.size / 2 } else { p.size };
        let liq_side = if p.long_liquidatable(w.truth) { Some(L) } else if p.short_liquidatable(w.truth) { Some(S) } else { None };
        let t0 = self.now as i64;
        let kind;
        let side;
        let price;
        let mut false_claim = false;
        if let Some(sd) = liq_side {
            (kind, side, price) = (LIQ, sd, w.truth);
        } else if self.now >= 600 && self.rng.chance(30) && p.range_ok() {
            // The underwriter lies: a liquidation of the long at a price that triggers it.
            let pf = p.trigger_of(L) * 9_000 / 10_000;
            if !p.long_liquidatable(pf) {
                return;
            }
            (kind, side, price, false_claim) = (LIQ, L, pf, true);
        } else {
            (kind, side, price) = (CLOSE, if self.rng.chance(50) { L } else { S }, w.truth);
        }
        let closer = if kind == CLOSE { Some(self.key(if side == L { &p.long_key } else { &p.short_key })) } else { None };
        let (pt, pw) = propose_tx(&w.lp, &ClaimSpec { kind, side, n, p: price, t0, lock: self.now }, &uw_kp(), closer.as_ref());
        if !accepted(&pt) {
            self.books.refused_as_ruled += 1;
            return;
        }
        self.count(if kind == CLOSE { "propose close" } else if false_claim { "propose false liquidation" } else { "propose liquidation" });
        let lw = live(&pt, 0, self.now, &pw);
        let pdaa = self.now as i64;
        self.pairs.remove(i);
        if n < p.size {
            self.count("partial claim");
            self.pairs.push(WP { lp: live(&pt, 1, self.now, &p.rest_of(p.size - n)), ..w.clone() });
        }
        if false_claim {
            let mode = self.rng.below(3);
            let st = stake(70, 10 * UNIT as u64, 0, 0xc0);
            // Mode 2: the contest lands at P + W, after the window.
            let c_daa = if mode == 2 { self.now + p.m.p.challenge_daa as u64 } else { self.now + 10 };
            let (ct, w2, rc, dj) = contest_tx(&lw, &st, 70, c_daa);
            must("contest", &ct);
            let (cw, r, ld) = (live(&ct, 0, c_daa, &w2), live(&ct, 1, c_daa, &rc), live(&ct, 2, c_daa, &dj));
            let mut receipts = vec![r];
            let (ld, d_daa) = if mode == 1 {
                // Colluders outvote the opener: 13 units on YES against its 10 on NO (mu 1.25).
                let s3 = stake(71, 13 * UNIT as u64, 0, 0xc1);
                let (dt, d2, rs) = deposit_tx(&ld, YES, &[(s3, 71)], c_daa + 10);
                must("deposit on YES", &dt);
                receipts.push(live(&dt, 1, c_daa + 10, &rs[0]));
                (live(&dt, 0, c_daa + 10, &d2), c_daa + 10)
            } else {
                (ld, c_daa)
            };
            let r_daa = d_daa + p.m.judge.round as u64;
            let (dt, pool, dw) = decide_tx(&ld, &cw, r_daa);
            must("resolve and decide", &dt);
            let (lpool, ldw) = (live(&dt, 0, r_daa, &pool), live(&dt, 1, r_daa, &dw));
            let (ft, s) = finalize_tx(&ldw, ldw.s.clock, 0);
            must("finalize the decided part", &ft);
            match (mode, &s) {
                (0, Settlement::Restore { part, defender, disputer, .. }) => {
                    assert_eq!(*part, p.restore_state(n), "the refuted part is restored exactly");
                    assert_eq!(defender + disputer, pw.bond, "the bond to the defender and the disputer");
                    self.count("refuted, pool closed");
                }
                (0, _) => panic!("seed {} step {}: a refuted claim did not restore: {s:?}", self.seed, self.step),
                (_, Settlement::Forfeit { part, .. }) => {
                    assert_eq!((part.long_key, part.short_key), (p.short_key, p.short_key), "the upheld false claim forfeits the long");
                    self.count(if mode == 1 { "upheld by YES, pool with a loser closed" } else { "late contest ignored, pool closed" });
                }
                (_, Settlement::Restore { defender, disputer, .. }) => {
                    assert_eq!((*defender, *disputer), (pw.bond, 0), "an upheld but void claim's bond goes to the defender");
                    self.count("upheld but void, restored");
                }
                (_, Settlement::Close { .. }) => panic!("a liquidation settled as a close"),
            }
            assert_eq!(pool.side, if mode == 1 { YES } else { NO });
            let (stx, p2, pays) = settle_tx(&lpool, &receipts, r_daa + 10);
            must("settle the receipts", &stx);
            if mode == 1 {
                assert_eq!(pays[0], 0, "the opener's NO receipt lost");
                assert!(pays[1] > receipts[1].value, "the YES receipt won a share of the loser's stake");
                self.count("contest won by YES, a pool with a losing receipt settled");
            }
            let lp2 = live(&stx, 0, r_daa + 10, &p2);
            let (ctx, _) = pool_close_tx(&lp2);
            must("close the pool", &ctx);
            self.now = self.now.max(r_daa + 10);
            let (Settlement::Restore { part, .. } | Settlement::Forfeit { part, .. }) = s else { unreachable!() };
            self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
            return;
        }
        if kind == LIQ && pw.cure_hash != [0; 32] && self.rng.chance(50) {
            let loser = self.key(if side == L { &p.long_key } else { &p.short_key });
            let (ct, c) = cure_tx(&lw, &loser);
            must("cure", &ct);
            self.count("cure");
            // Review 9 (R9-F3): the cure stays within the loser's share.
            if side == L {
                assert!(c.bl >= p.growth_limit(L), "seed {} step {}: a cure took the short's share", self.seed, self.step);
            } else {
                assert!(c.bs <= p.growth_limit(S), "seed {} step {}: a cure took the long's share", self.seed, self.step);
            }
            self.pairs.push(WP { lp: live(&ct, 1, self.now, &c), ..w });
            return;
        }
        let (ft, s) = finalize_tx(&lw, pdaa, 200);
        must(&format!("seed {} step {}: finalize {s:?}", self.seed, self.step), &ft);
        self.now += 200;
        match s {
            Settlement::Close { long, short } => {
                // The other party gets at least its payoff at the close price moved by g against the closer.
                let c = pw.close_price();
                let (lo_, so_) = (q(n, c - p.bl, p.m.p.scale), q(n, p.bs - c, p.m.p.scale));
                assert!(if side == L { short >= so_ || short == 0 } else { long >= lo_ || long == 0 }, "the non-closer underpaid");
                self.count("close settled");
            }
            Settlement::Forfeit { part, .. } => {
                self.count("liquidation settled");
                let wk = if side == L { p.short_key } else { p.long_key };
                assert_eq!((part.long_key, part.short_key), (wk, wk));
                self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
            }
            Settlement::Restore { part, defender, .. } => {
                assert_eq!(defender, pw.bond, "a void claim's bond goes to the defender");
                self.count("void claim restored");
                self.pairs.push(WP { lp: live(&ft, 1, self.now, &part), ..w });
            }
        }
    }

    fn lapse(&mut self, i: usize) {
        let w = self.pairs[i].clone();
        if self.now < w.lp.s.lapse_from() {
            refused("lapse settle before it is due", &lapse_tx(&w.lp, w.lp.s.lapse_from() - 1, 0), 0);
            return;
        }
        must("lapse settle", &lapse_tx(&w.lp, self.now, 0));
        self.count("lapse settle");
        self.pairs.remove(i);
    }

    /// T2 with a pair: an order of the listed side takes it whole (`take_tx`: the pair's transfer to the order, the
    /// order's take, the slot's bind of the part at the pair's expiry). The pair's verdict is compared with the
    /// hand-over rule (review 9's share rule included); an accepted take also kept the order's own terms.
    fn take_pair(&mut self, i: usize) {
        let ls = if self.rng.chance(50) { L } else { S };
        self.ensure_listed(i, ls);
        let w = self.pairs[i].clone();
        let p = w.lp.s.clone();
        if !self.quiet_ok(&w.lp) || (ls == L && p.long_ask == 0) || (ls == S && p.short_bid == 0) {
            return;
        }
        let ek = self.entrant();
        let rate = [50_000_000i64, 100_000_000, 200_000_000, 300_000_000][self.rng.below(4) as usize];
        let xp = if ls == L { self.rng.around(p.long_ask, 20_000_000) } else { self.rng.around(p.short_bid, 20_000_000) }.max(0);
        let (op, id) = self.fresh_op();
        let o = OOpen { size_left: p.size, min_fill: p.size, my_rate: rate, limit: xp, min_counter_rate: 0, ..sample_order(&p.m, ls, x(&ek)) };
        let lo = Live { op, value: 2_000_000_000_000u64, daa: self.now, id: Some(id), s: o };
        let (t, part) = take_tx(&w.lp, &lo, &self.slot, xp, self.now, true);
        let nb = if ls == L { xp - rate } else { xp + rate };
        let rule = Self::hand_over_rule(&p, ls, xp, nb, &x(&ek));
        assert_eq!(verdicts(&t)[0], rule, "seed {} step {}: take with a pair, side {ls} x {xp} nb {nb}", self.seed, self.step);
        if !accepted(&t) {
            self.books.refused_as_ruled += 1;
            return;
        }
        assert!(if ls == L { p.entry >= xp } else { p.entry <= xp }, "the order took an entry against it");
        assert!(part.range_ok() && part.b.cover_rate >= part.bs - part.bl && part.b == p.b);
        let a = BindArgs::new(self.now as i64, p.b.expiry, 0, &[Claim::output(2, p.cover_of(p.size))]);
        let b2 = self.slot.s.bind(&slot_ctx(&self.slot, self.now), &a, self.slot.value as i64).expect("the slot's bind");
        self.slot = live(&t, 3, self.now, &b2);
        self.count("take with a pair");
        self.keys.insert(x(&ek), ek);
        self.pairs[i].lp = live(&t, 2, self.now, &part);
    }

    /// The shape of R9-F1: a take with no pair into an expiry in the past, then a renewal, which the pair refuses.
    fn hostile_take(&mut self) {
        if self.now < 4_000 {
            return;
        }
        let (op, id) = self.fresh_op();
        let lo = Live { op, value: 2_000_000_000_000u64, daa: self.now, id: Some(id), s: sample_order(&omarket(), L, x(&long_kp())) };
        let sl = Live { op: outpoint(0x99), value: K, daa: 0, id: Some(Hash::from_bytes([0x5b; 32])), s: Bond::new(x(&uw_kp()), omarket().bond_market(), 0) };
        let xe = 1_200;
        let (t, part, b2) = take_alone(&lo, &sl, &keypair(77), 1_000_000_000, 1_000_000_000, 1_220_000_000, xe, 320_000_000, lo.s.prem_max, self.now);
        must("the hostile take", &t);
        let lp = live(&t, 1, self.now, &part);
        let s2 = live(&t, 2, self.now, &b2);
        let x2 = lp.s.b.expiry + lp.s.m.bond.term;
        let d = (x2 / lp.s.m.bond.step - 12 - s2.s.j0).max(0);
        let (rt, _, _) = renew_tx(&lp, &s2, self.now, d);
        if verdicts(&rt)[0] {
            self.violation(format!("R9-F1: a part taken at DAA {} with expiry {xe} was renewed", self.now));
        }
        self.books.hostile_refused += 1;
        self.count("hostile take, renewal refused");
    }

    /// Review 10 (R10-F1): a long order with the command's default minimum fill (the market's minimum size) and a tip,
    /// taken in 2 to 5 parts of 1 to 5 units with no pair into an expiry in the past by an outsider's genesis slot, each
    /// part lapse-settled at once. The order's loss is compared with DESIGN-ORACLE-IMPL 27.4's bound for a take into the
    /// past (its tip on what was taken and one carrier) and the excess recorded.
    fn shred(&mut self) {
        if self.now < 4_000 {
            return;
        }
        let m = omarket();
        let (op, id) = self.fresh_op();
        let o = OOpen { min_fill: m.p.min_size, tip: 1_000_000, ..sample_order(&m, L, x(&long_kp())) };
        let mut lo = Live { op, value: 2_000_000_000_000u64, daa: self.now, id: Some(id), s: o.clone() };
        let (sop, sid) = self.fresh_op();
        let mut sl = Live { op: sop, value: K, daa: 0, id: Some(sid), s: Bond::new(x(&uw_kp()), m.bond_market(), 0) };
        let parts = 2 + self.rng.below(4);
        let (mut paid, mut back, mut taken) = (0i64, 0i64, 0i64);
        for _ in 0..parts {
            let n = m.p.min_size * (1 + self.rng.below(5) as i64);
            let (fop, _) = self.fresh_op();
            let (t, part, b2, cont) = take_piece(&lo, n, &sl, &keypair(77), 1_000_000_000, 1_000_000_000, 1_220_000_000, 1_200, 320_000_000, 0, self.now, fop);
            must(&format!("seed {} step {}: a small take with no pair", self.seed, self.step), &t);
            paid += lo.value as i64 - t.0.outputs[0].value as i64;
            taken += n;
            let lp = live(&t, 1, self.now, &part);
            let (fop2, _) = self.fresh_op();
            let (lt, l, _) = lapse_by(&lp, self.now, &keypair(77), fop2);
            must("its lapse settle", &lt);
            back += l;
            lo = live(&t, 0, self.now, &cont.expect("the order continues"));
            sl = live(&t, 2, self.now, &b2);
        }
        let bound = q(taken, o.tip, m.p.scale) + m.p.carrier;
        let loss = paid - back;
        assert_eq!(loss, q(taken, o.tip, m.p.scale) + parts as i64 * m.p.carrier, "seed {} step {}: one carrier per part", self.seed, self.step);
        self.books.shreds += 1;
        self.books.shred_excess += loss - bound;
        self.count("shredded take");
    }

    fn play(&mut self, steps: usize) {
        for s in 0..steps {
            self.step = s;
            self.tick();
            if self.pairs.len() < 4 && self.rng.chance(40) {
                self.open();
                // Two opens at one DAA share a binding, so close-versus-close can join them later.
                if self.rng.chance(50) {
                    self.open();
                }
            }
            if self.pairs.is_empty() {
                continue;
            }
            let i = self.rng.below(self.pairs.len() as u64) as usize;
            match self.rng.below(100) {
                0..=14 => self.amend(i),
                15..=24 => self.add_margin(i),
                25..=36 => self.transfer(i),
                37..=46 => self.donor_alone(i),
                47..=54 => self.host_alone(i),
                55..=62 => self.combined(),
                63..=72 => self.claim(i),
                73..=76 => self.lapse(i),
                77..=79 if self.hostile_takes => self.hostile_take(),
                80..=87 => self.take_pair(i),
                88..=90 if self.hostile_takes => self.shred(),
                _ => {}
            }
            // The underwriter renews each pair in its window most of the time; sometimes it is silent.
            for k in 0..self.pairs.len() {
                if k < self.pairs.len() && !self.pairs[k].silent && self.rng.chance(60) {
                    self.renew(k);
                }
            }
            self.check_bindings();
        }
    }
}

// ---------------------------------------------------------------- lead 5: the seeded walk, new seeds, longer, hostile

fn run_walks(seeds: std::ops::RangeInclusive<u64>, steps: usize, hostile: bool) -> (WBooks, Vec<(u64, usize, String)>) {
    let mut total = WBooks::default();
    let mut violations = vec![];
    for seed in seeds {
        let mut w = Walk::new(seed, hostile);
        w.play(steps);
        for (k, v) in w.books.steps.iter() {
            *total.steps.entry(k).or_default() += v;
        }
        total.refused_as_ruled += w.books.refused_as_ruled;
        total.hostile_refused += w.books.hostile_refused;
        total.shreds += w.books.shreds;
        total.shred_excess += w.books.shred_excess;
        violations.extend(w.violations);
    }
    println!("walk steps: {:?}", total.steps);
    println!("refused as the rules say: {}, hostile variants refused: {}", total.refused_as_ruled, total.hostile_refused);
    (total, violations)
}

/// Lead 5: the walk of review 9 (every entry point of the five contracts, free arguments drawn wide, each accepted shape
/// compared with the stated rule, hostile variants at every hand-over and renewal, claims of every outcome with their
/// pools, whole and half claims, several pairs on one slot) on twenty seeds it never ran (7 to 26) and longer (400
/// steps instead of 200). After every step every live pair keeps its lineage's binding, its floor and its range within
/// the cover rate, every passive side of a hand-over keeps its payoff at four prices and its lapse payout, every cure
/// stays in the loser's share, and no lineage pays premiums ahead of the on-time schedule. No violation.
#[test]
fn r10_holds_the_seeded_walk_on_new_seeds_and_longer_runs() {
    let (total, violations) = run_walks(7..=26, 400, false);
    for (s, st, v) in &violations {
        println!("seed {s} step {st}: {v}");
    }
    for k in ["open", "amend", "addMargin", "transfer", "take with a pair", "closeDonor alone", "closeHost alone", "renew", "propose close", "partial claim", "lapse settle", "liquidation settled"] {
        assert!(total.steps.get(k).copied().unwrap_or(0) > 0, "the walk never ran {k}");
    }
    assert!(total.hostile_refused > 50);
    assert!(violations.is_empty(), "violations recorded: {violations:?}");
}

/// Lead 5, R10-F1 in the walk: with the hostile steps enabled (R9-F1's take into the past, and R10-F1's small takes of
/// an order with the default minimum fill, interleaved with every other entry point on the same clock), every shredded
/// order loses one carrier per part, beyond 27.4's bound of one carrier for the whole order; the walk's other checks
/// record no violation.
#[test]
fn r10_exploit_the_seeded_walk_with_small_takes_finds_losses_beyond_the_documented_bound() {
    let (total, violations) = run_walks(1..=8, 200, true);
    println!("shredded orders: {}, lost beyond the documented bound: {} sompi", total.shreds, total.shred_excess);
    assert!(total.shreds > 0, "the walk did not reach the small takes");
    assert!(total.shred_excess >= total.shreds as i64 * 20_000_000, "at least one extra carrier per shredded order");
    assert!(violations.is_empty(), "violations recorded: {violations:?}");
}
