//! Review 7 (docs/audit/REVIEW-7.md): the oracle-market family after the fresh-id change of review 6
//! (DESIGN-ORACLE-IMPL 24). Every transaction here runs every input in the real script engine with the covenant context
//! (`common::run`). `exploit_` tests build an attack and assert that the engine ACCEPTS it; `holds_` tests assert that an
//! attack is refused. Worked numbers use the fixture market of `oraclekit` (1,000 units at 10 KAS, long margin 1,000 KAS,
//! short margin 2,000 KAS, cover 6,000 KAS, a claim's bond 10 % of the cover).
//!
//! The root of R7-F1: `fresh(out)` proves that output `out` starts a new covenant id carried by no input and by no other
//! output, but nothing ties that output to the input that checks it. Before review 6 every part a pair entry wrote was
//! counted under the entry's own id (`OpCovOutputCount(selfId)`), which no other pair shares, so two pair inputs could
//! never satisfy their checks with one output. Since review 6 the part an entrant gets (`transfer`, `closeHost`,
//! `closeDonor`) and the pending part of a claim (`propose`) are found only by index and script: two pair inputs whose
//! entries compute the same script accept one and the same output, and the value of the second pair leaves with whoever
//! builds the transaction.

mod bondkit;
mod common;
mod judgekit;
mod oraclekit;

use common::*;
use kpf_protocol::oracle::*;
use oraclekit::*;
use secp256k1::Keypair;

const KAS: i64 = 100_000_000;
const FALSE_P: i64 = 920_000_000;

/// The attacker's own key (never the underwriter's).
fn attacker() -> Keypair {
    keypair(50)
}

fn liq(side: i64, n: i64, p: i64, t0: i64, lock: u64) -> ClaimSpec {
    ClaimSpec { kind: LIQ, side, n, p, t0, lock }
}

/// Two pairs opened by two long orders of the attacker against one resting short order of the victim (`short_kp`), at
/// the same price and in the same expiry step, under the same honest underwriter: what a market maker's resting order
/// gets whenever it is filled twice at one price. They differ in their covenant ids, the long's tag and nothing a
/// hand-over of the long carries into the entrant's part.
fn two_pairs_against_one_short() -> (LPair, LPair) {
    let a = attacker();
    let (lo1, lo2, so, sl) = (order(L, &a, 0x73, 0), order(L, &a, 0x74, 0), order(S, &short_kp(), 0x72, 0), slot(K));
    let sp = open_spec();
    let (t1, p1, b1) = open_tx(&lo1, &so, &sl, &sp, &|_, _| {});
    must("first open", &t1);
    let (so2, _) = order_after(&so, sp.n, so.s.fill_spend(sp.n, sp.premium));
    let so2 = live(&t1, 1, OPEN_DAA, &so2.expect("the short order rests on"));
    let sl2 = live(&t1, 3, OPEN_DAA, &b1);
    let (t2, p2, _) = open_tx(&lo2, &so2, &sl2, &sp, &|_, _| {});
    must("second open (the victim's order filled again at the same price)", &t2);
    let (l1, l2) = (live(&t1, 2, OPEN_DAA, &p1), live(&t2, 2, OPEN_DAA + 10, &p2));
    assert_ne!(l1.id, l2.id);
    assert_eq!(l1.s.short_key, x(&short_kp()));
    assert_eq!(l2.s.short_key, x(&short_kp()));
    (l1, l2)
}

/// The whole-pair transfer of the long of `lp` at `xp` to `ek`, bound `nb`, the part at output `c` (the args alone).
fn transfer_args(lp: &LPair, xp: i64, nb: i64, ek: &Keypair, c: i64) -> Vec<Arg> {
    vec![iv(L), iv(lp.s.size), iv(xp), iv(nb), bv(&x(ek)), bv(&[0; 32]), iv(c)]
}

/// The part a whole transfer of the long of `p` to `ek` at bound `nb` writes.
fn long_part(p: &OPair, nb: i64, ek: &Keypair) -> OPair {
    let mut part = p.rest_of(p.size);
    part.bl = nb;
    part.long_key = x(ek);
    (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
    part
}

/// R7-F1, the party route. Two pairs share the victim as short and nothing else a hand-over carries; the attacker is
/// the long of both (it listed both longs at 10 KAS). One transaction transfers both longs, each whole, to the
/// attacker's new key, and both pair inputs accept ONE part output at index 2: each input checks `fresh(2)` (no input,
/// no other output carries its id), the part's script (identical for both, since the part replaces the long's key,
/// bound, listing and tags) and a value of at least its own floor. Pair 2's 3,000.2 KAS has no output of its own; the
/// builder takes it as change.
///
/// Who loses what: the victim short had 2,000 KAS of margin in each pair (4,000 KAS) and a short of 2,000 units; after
/// the transaction it has one short of 1,000 units with 2,000 KAS of margin. It loses 2,000 KAS and half its position
/// without signing anything, without any price and with nothing to contest. The attacker pays its own two longs 1,000
/// KAS each (to itself) and gains 2,000.2 KAS net. A bystander who is not the long gains the same by buying two listed
/// longs at their asks (the honest long is paid in full, the stayer is robbed). No claim, no underwriter and no slot is
/// involved, so neither rho nor the judge bounds it.
#[test]
fn exploit_two_pairs_merge_into_one_part_by_transfer() {
    let a = attacker();
    let (l1, l2) = two_pairs_against_one_short();
    let ask = 1_000_000_000;
    let (at1, s1) = amend_tx(&l1, L, &a, ask, 20);
    must("the attacker lists the long of pair 1", &at1);
    let (at2, s2) = amend_tx(&l2, L, &a, ask, 20);
    must("the attacker lists the long of pair 2", &at2);
    let (l1, l2) = (live(&at1, 0, 400, &s1), live(&at2, 0, 400, &s2));
    let ek = keypair(51);
    let nb = l1.s.bl;
    let part = long_part(&l1.s, nb, &ek);
    assert_eq!(part, long_part(&l2.s, nb, &ek), "both hand-overs write the same part");
    let pay = kpf_protocol::payoff::owed(l1.s.size, ask - l1.s.bl, 0, l1.s.m.p.scale);
    let part_v = part.range_of(part.size) + (l1.value as i64 - l1.s.range_of(l1.s.size));
    let funding = 5_000 * KAS as u64;
    let ins = vec![
        pair_in(&l1, "transfer", transfer_args(&l1, ask, nb, &ek, 2)),
        pair_in(&l2, "transfer", transfer_args(&l2, ask, nb, &ek, 2)),
        key_in(outpoint(230), funding, 0, &a),
    ];
    let total_in = l1.value + l2.value + funding;
    let fixed = 2 * pay as u64 + part_v as u64;
    let mut outs = vec![
        out(pay as u64, key_spk(&l1.s.long_key), None),
        out(pay as u64, key_spk(&l2.s.long_key), None),
        out(part_v as u64, pair_spk(&part), None),
        out(total_in - fixed, p2pk_spk(&a), None),
    ];
    seal(&mut outs, 2, 0, l1.op);
    let t = build_ext(&ins, outs, 0, &[seq_quiet(&l1), seq_quiet(&l2), 0]);
    must("two pairs handed over into one part", &t);
    // The arithmetic of the loss: pair 2's value has no output; the builder's change carries it.
    assert_eq!(l2.value, 300_020_000_000);
    assert_eq!(part_v as u64, 300_020_000_000, "one part of 1,000 units, 3,000.2 KAS");
    assert_eq!(total_in - fixed - funding, 300_020_000_000 - 2 * pay as u64, "change = pair 2 less the two longs' payouts");
    let victim_before = l1.s.short_at(l1.s.entry) + l2.s.short_at(l2.s.entry);
    let victim_after = part.short_at(part.entry);
    assert_eq!((victim_before, victim_after), (4_000 * KAS, 2_000 * KAS));
    // The attacker: +2 x 1,000 KAS of payouts and +1,000.2 KAS of change in cash, two longs (1,000 KAS of equity each at
    // entry) given up, one long of 1,000 KAS of equity received: +2,000.2 KAS.
    let gain = 2 * pay + (total_in - fixed - funding) as i64 - 2 * l1.s.long_at(l1.s.entry) + part.long_at(part.entry);
    assert_eq!(gain, 2_000 * KAS + 20_000_000);
    println!("R7-F1 (transfer): victim short margin {} -> {} KAS; attacker +{} sompi", victim_before / KAS, victim_after / KAS, gain);
}

/// R7-F1, the same merge with a partial hand-over: the two pairs need not even have equal sizes or longs, since each
/// keeps its remainder under its own id and only the entered part is shared. 400 units of each long are handed over
/// into one part; each remainder is exact. The victim loses q(400, bs - entry) = 800 KAS of short margin.
#[test]
fn exploit_partial_hand_overs_of_two_pairs_share_one_part() {
    let a = attacker();
    let (l1, l2) = two_pairs_against_one_short();
    let ask = 1_000_000_000;
    let (at1, s1) = amend_tx(&l1, L, &a, ask, 20);
    must("list 1", &at1);
    let (at2, s2) = amend_tx(&l2, L, &a, ask, 20);
    must("list 2", &at2);
    let (l1, l2) = (live(&at1, 0, 400, &s1), live(&at2, 0, 400, &s2));
    let ek = keypair(51);
    let n = 400_000_000;
    let nb = l1.s.bl;
    let mut part = long_part(&l1.s, nb, &ek);
    part.size = n;
    let sc = l1.s.m.p.scale;
    let pay = kpf_protocol::payoff::owed(n, ask - l1.s.bl, 0, sc);
    let part_v = part.range_of(n) + l1.s.m.p.carrier;
    let rest = |l: &LPair| (l.s.rest_of(l.s.size - n), l.s.rest_value(l.value, n));
    let (r1, v1) = rest(&l1);
    let (r2, v2) = rest(&l2);
    let args = |c: i64| vec![iv(L), iv(n), iv(ask), iv(nb), bv(&x(&ek)), bv(&[0; 32]), iv(c)];
    let funding = 5_000 * KAS as u64;
    let ins = vec![pair_in(&l1, "transfer", args(2)), pair_in(&l2, "transfer", args(2)), key_in(outpoint(231), funding, 0, &a)];
    let total_in = l1.value + l2.value + funding;
    let fixed = 2 * pay as u64 + part_v as u64 + v1 as u64 + v2 as u64;
    let mut outs = vec![
        out(pay as u64, key_spk(&l1.s.long_key), None),
        out(pay as u64, key_spk(&l2.s.long_key), None),
        out(part_v as u64, pair_spk(&part), None),
        out(v1 as u64, pair_spk(&r1), bind(0, l1.id.unwrap())),
        out(v2 as u64, pair_spk(&r2), bind(1, l2.id.unwrap())),
        out(total_in - fixed, p2pk_spk(&a), None),
    ];
    seal(&mut outs, 2, 0, l1.op);
    let t = build_ext(&ins, outs, 0, &[seq_quiet(&l1), seq_quiet(&l2), 0]);
    must("two partial hand-overs into one part", &t);
    // Two parts of 400 units were paid for (2 x 1,200 KAS of range) but one exists: the change keeps 1,200 KAS of range,
    // of which 800 KAS was the victim's short margin and 400 KAS the long's (paid out to the long at its ask).
    assert_eq!(part.range_of(n), 1_200 * KAS);
    // (less the new part's carrier, which the builder funds)
    assert_eq!(total_in - fixed - funding, (1_200 * KAS - 2 * pay - l1.s.m.p.carrier) as u64);
    assert_eq!(part.short_at(part.entry), 800 * KAS);
}

/// R7-F1, the underwriter's route. The underwriter needs no party's help to make two pairs that write the same pending
/// part: (1) it proposes a void liquidation on half the pair (t0 more than lateDaa before it lands); (2) anyone
/// finalizes it after W: the half is restored under the claim's id with the remainder's exact state (unlisted, tags
/// 0); (3) it proposes the same liquidation on both halves in one transaction vouched once, and both pair inputs accept
/// ONE pending output (each checks `fresh(0)`, the pending script, identical for both, and a value of its own value plus
/// the bond). The other half's 1,500.2 KAS goes to the underwriter's own output.
///
/// Who loses what (fixture pair): the long loses its 500 KAS of margin in the vanished half and the short its 1,000 KAS,
/// without any claim against that half existing: nothing to contest, nothing to cure, no window. The underwriter paid
/// the void claim's 300 KAS bond (to the long) and takes 1,500.2 KAS: net +1,200.2 KAS from one pair, beyond the
/// documented bound of a dishonest underwriter (a false uncontested claim takes at most the loser's margin of the cover
/// it counts, 1,000 KAS here, and a contested one takes nothing). Repeated on the surviving half (split, void, merge),
/// it drains the pair geometrically.
#[test]
fn exploit_underwriter_merges_two_halves_into_one_claim_and_keeps_the_other() {
    let (lp, _, _) = opened();
    let half = lp.s.size / 2;
    // (1) A void claim on half the pair: P = 400, t0 = 250, P - t0 = 150 > lateDaa (100).
    let (t, w) = propose_tx(&lp, &liq(L, half, FALSE_P, 250, 400), &uw_kp(), None);
    must("a void claim on half", &t);
    let lw = live(&t, 0, 400, &w);
    let rest = live(&t, 1, 400, &lp.s.rest_of(lp.s.size - half));
    // (2) Finalized after W: restored with the bond to the long.
    let (ft, s) = finalize_tx(&lw, 400, 200);
    must("the void claim restores", &ft);
    let Settlement::Restore { part: back, defender, .. } = s else { panic!("{s:?}") };
    assert_eq!(defender, 300 * KAS, "the void claim's bond, to the long");
    let back = live(&ft, 1, 600, &back);
    assert_eq!(back.s, rest.s, "the restored half and the remainder have one state");
    assert_ne!(back.id, rest.id);
    // (3) One liquidation of both halves, vouched once, one pending output.
    let k = kit();
    let (w2, fb) = rest.s.pending_of(k, half, LIQ, L, FALSE_P, 650).expect("a liquidation the pair accepts");
    assert_eq!(back.s.pending_of(k, half, LIQ, L, FALSE_P, 650).unwrap().0, w2, "both pairs write one pending state");
    let args = || vec![iv(half), iv(LIQ), iv(L), iv(FALSE_P), iv(650), iv(2), bv(&[0u8; 65]), iv(fb), iv(0), bv(&k.pending.prefix), bv(&k.pending.suffix)];
    let vouch = 500 * KAS as u64;
    let ins = vec![pair_in(&rest, "propose", args()), pair_in(&back, "propose", args()), key_in(outpoint(232), vouch, 0, &uw_kp()), exec_in(233)];
    let pend_v = rest.value.max(back.value) + w2.bond as u64;
    let loot = rest.value.min(back.value);
    let mut outs = vec![out(pend_v, pend_spk(&w2), None), out(loot, key_spk(&x(&uw_kp())), None), change(90_000_000_000)];
    seal(&mut outs, 0, 0, rest.op);
    let t = build_ext(&ins, outs, 650, &[0, 0, 0, 0]);
    must("two halves proposed into one pending part", &t);
    assert_eq!(loot, 150_020_000_000, "the vanished half: 1,500 KAS of range and its carrier");
    assert_eq!(back.s.long_at(back.s.entry) + back.s.short_at(back.s.entry), 1_500 * KAS);
    println!("R7-F1 (propose): the underwriter takes {} KAS for a 300 KAS bond", loot as i64 / KAS);
}

/// The fresh-id scan itself: a ninth output is refused (the merge of `exploit_two_pairs_merge_into_one_part_by_transfer`
/// with one more change output), so the merge is bounded by output count only, not prevented.
#[test]
fn holds_the_fresh_scan_refuses_a_ninth_output() {
    let a = attacker();
    let (l1, _) = two_pairs_against_one_short();
    let (at1, s1) = amend_tx(&l1, L, &a, 1_000_000_000, 20);
    must("list", &at1);
    let l1 = live(&at1, 0, 400, &s1);
    let (t, _) = transfer_tx(&l1, L, 1_000_000_000, l1.s.bl, &keypair(51), 20);
    must("an ordinary transfer", &t);
    let mut outs = t.0.outputs.clone();
    for _ in 0..6 {
        outs.push(out(2_000_000, p2pk_spk(&a), None));
    }
    seal(&mut outs, 1, 0, l1.op);
    let ins = vec![pair_in(&l1, "transfer", vec![iv(L), iv(l1.s.size), iv(1_000_000_000), iv(l1.s.bl), bv(&x(&keypair(51))), bv(&[0; 32]), iv(1)]), key_in(outpoint(223), 500_000_000_000, 0, &keypair(51))];
    let t9 = build_ext(&ins, outs, 0, &[20, 0]);
    refused("a hand-over with nine outputs", &t9, 0);
}

/// An order's fill or take cannot be shared: the pair or part an order writes carries the order's own covenant id as
/// its tag (`tl` or `ts = selfId`), so two orders of one side never compute one script, and R7-F1 does not reach
/// `KpfOpenO`'s own escrow (counted under its own id).
#[test]
fn holds_two_orders_never_share_one_new_pair() {
    let a = attacker();
    let (lo1, lo2, so, sl) = (order(L, &a, 0x73, 0), order(L, &a, 0x74, 0), order(S, &short_kp(), 0x72, 0), slot(K));
    let p = opened_pair(&lo1, &so, &sl, &open_spec());
    let p2 = opened_pair(&lo2, &so, &sl, &open_spec());
    assert_ne!(pair_spk(&p), pair_spk(&p2), "the long order's id is in the pair it writes");
}

/// Renewal after the lapse settle is due (point 5 of the brief): the underwriter's counted renewal moves the expiry of a
/// lapsed pair and so cancels a due lapse settle (documented in DESIGN-ORACLE-IMPL 24.2 (c)); a party cannot. Checked:
/// the lapse settle is due at expiry + tail + idle, a renewal at that DAA is still accepted, and the renewed pair can
/// no longer be lapse-settled.
#[test]
fn holds_only_the_underwriters_counted_renewal_cancels_a_due_lapse_settle() {
    let (lp, sl, _) = opened();
    let due = lp.s.lapse_from();
    assert_eq!(due, 1_200 + 200 + 1_000);
    must("lapse settle due", &lapse_tx(&lp, due, 0));
    // The slot advances 10 steps (every dropped bucket's liability has ended) so that X + term lies inside its ring.
    let (t, r, _) = renew_tx(&lp, &sl, due, 10);
    must("a renewal at the DAA the lapse settle is due (documented 24.2 (c))", &t);
    let rl = live(&t, 0, due, &r);
    assert_eq!(rl.s.b.expiry, 2_000);
    assert!(!accepted(&lapse_tx(&rl, due, 0)), "after the renewal the lapse settle is not due");
    // A party's amend does not move the clock.
    let (at, c) = amend_tx(&lp, S, &short_kp(), 0, 20);
    must("amend", &at);
    must("lapse settle after an amend", &lapse_tx(&live(&at, 0, due, &c), due, 0));
}
