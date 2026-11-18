//! KpfPair, evidence-free paths of DESIGN 4.3: P1 amend, P2 add margin, P8 idle settle, P9 cooperative close.
//!
//! Every accepted transaction is also checked for conservation (no output sum above the input sum).

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::{Transaction, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kaspa_txscript_errors::TxScriptError;
use kpf::*;
use secp256k1::Keypair;
use silverscript_abi::ArtifactValue;

const PAIR_ID: Hash = Hash::from_bytes([0x5a; 32]);
const F_VALUE: u64 = 1_000_000_000_000;

struct K {
    long: Keypair,
    short: Keypair,
    other: Keypair,
    f: Keypair,
}

fn keys() -> K {
    K { long: keypair(11), short: keypair(12), other: keypair(13), f: keypair(14) }
}

fn base(k: &K) -> Pair {
    pair(&k.long, &k.short)
}

fn pair_in(p: &Pair, value: u64, id: Hash, entry: &'static str, a: Vec<Arg>) -> In {
    In { outpoint: outpoint(10), utxo: UtxoEntry::new(value, p2sh(&p.art()), 0, false, Some(id)), spend: Spend::Signed { art: p.art(), entry, args: a } }
}

fn f_in(k: &K) -> In {
    p2pk_in(outpoint(11), F_VALUE, None, &k.f)
}

fn cont(p: &Pair, value: u64) -> TransactionOutput {
    out(value, p2sh(&p.art()), bind(0, PAIR_ID))
}

fn change(k: &K, v: u64) -> TransactionOutput {
    out(v, p2pk_spk(&k.f), None)
}

/// Accepted, and the transaction creates no sompi.
fn accept(tx: &Transaction, e: &[UtxoEntry]) {
    assert_accepted(tx, e);
    let ins: u64 = e.iter().map(|u| u.amount).sum();
    let outs: u64 = tx.outputs.iter().map(|o| o.value).sum();
    assert!(outs <= ins, "outputs {outs} > inputs {ins}");
}

fn i(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}

// ---------------------------------------------------------------- P1 amend

fn amend_tx(k: &K, p: &Pair, signer: &Keypair, side: i64, w: (i64, i64, i64), next: &Pair, cont_value: u64) -> (Transaction, Vec<UtxoEntry>) {
    let v = p.floor() as u64;
    let ins = [pair_in(p, v, PAIR_ID, "amend", vec![Arg::Sig(*signer), i(side), i(w.0), i(w.1), i(w.2)]), f_in(k)];
    build(&ins, vec![cont(next, cont_value), change(k, F_VALUE - 10_000)])
}

fn listed_long(p: &Pair) -> Pair {
    let mut n = p.cleared();
    (n.long_ask, n.long_min, n.long_tip) = (1_100_000_000, 100_000_000, 1_000_000);
    n
}

fn listed_short(p: &Pair) -> Pair {
    let mut n = p.cleared();
    (n.short_bid, n.short_min, n.short_tip) = (950_000_000, 50_000_000, 2_000_000);
    n
}

#[test]
fn p1_owner_sets_its_own_listing() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let (tx, e) = amend_tx(&k, &p, &k.long, LONG, (1_100_000_000, 100_000_000, 1_000_000), &listed_long(&p), v);
    accept(&tx, &e);
    let (tx, e) = amend_tx(&k, &p, &k.short, SHORT, (950_000_000, 50_000_000, 2_000_000), &listed_short(&p), v);
    accept(&tx, &e);
    // Unlisting (all zero) on an already listed side.
    let l = listed_long(&p);
    let (tx, e) = amend_tx(&k, &l, &k.long, LONG, (0, 0, 0), &p.cleared(), v);
    accept(&tx, &e);
}

#[test]
fn p1_wrong_signer_is_refused() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let w = (1_100_000_000, 100_000_000, 1_000_000);
    for signer in [&k.short, &k.other, &k.f] {
        let (tx, e) = amend_tx(&k, &p, signer, LONG, w, &listed_long(&p), v);
        assert_input_refused(&tx, &e, 0);
    }
    for signer in [&k.long, &k.other] {
        let (tx, e) = amend_tx(&k, &p, signer, SHORT, (950_000_000, 50_000_000, 2_000_000), &listed_short(&p), v);
        assert_input_refused(&tx, &e, 0);
    }
    // A side code other than 1 or 2.
    let (tx, e) = amend_tx(&k, &p, &k.long, 3, w, &listed_long(&p), v);
    assert_input_refused(&tx, &e, 0);
}

#[test]
fn p1_cannot_touch_the_other_window_or_any_other_field() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let w = (1_100_000_000, 100_000_000, 1_000_000);
    let good = listed_long(&p);
    let mutations: Vec<(&str, Box<dyn Fn(&mut Pair)>)> = vec![
        ("short window", Box::new(|n: &mut Pair| n.short_bid = 1)),
        ("short tip", Box::new(|n: &mut Pair| n.short_tip = 1)),
        ("size", Box::new(|n: &mut Pair| n.size -= 1)),
        ("bl", Box::new(|n: &mut Pair| n.bl -= 1)),
        ("bs", Box::new(|n: &mut Pair| n.bs += 1)),
        ("entry", Box::new(|n: &mut Pair| n.entry += 1)),
        ("long key", Box::new(|n: &mut Pair| n.long_key = xonly(&keypair(13)))),
        ("short key", Box::new(|n: &mut Pair| n.short_key = xonly(&keypair(13)))),
        ("old tag L copied", Box::new(|n: &mut Pair| n.tag_l = Hash::from_bytes([0x1a; 32]))),
        ("tag S", Box::new(|n: &mut Pair| n.tag_s = Hash::from_bytes([9; 32]))),
        ("carrier", Box::new(|n: &mut Pair| n.m.carrier += 1)),
        ("idleDaa", Box::new(|n: &mut Pair| n.m.idle_daa = 0)),
        ("token", Box::new(|n: &mut Pair| n.m.token_cov_id = Hash::from_bytes([7; 32]))),
        ("priceMax", Box::new(|n: &mut Pair| n.m.price_max -= 1)),
    ];
    for (name, f) in mutations {
        let mut n = good.clone();
        f(&mut n);
        let (tx, e) = amend_tx(&k, &p, &k.long, LONG, w, &n, v);
        let r = run(&tx, &e).unwrap();
        assert!(r[0].is_err(), "{name} accepted");
    }
    // The window written must be the one in the arguments.
    let mut n = good.clone();
    n.long_ask += 1;
    let (tx, e) = amend_tx(&k, &p, &k.long, LONG, w, &n, v);
    assert_input_refused(&tx, &e, 0);
}

#[test]
fn p1_value_and_window_bounds() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    // An amend moves no value out of the pair.
    let (tx, e) = amend_tx(&k, &p, &k.long, LONG, (1_100_000_000, 100_000_000, 1_000_000), &listed_long(&p), v - 1);
    assert_input_refused(&tx, &e, 0);
    // Negative or above-priceMax window values.
    for w in [(-1, 0, 0), (0, -1, 0), (0, 0, -1), (PRICE_MAX + 1, 0, 0)] {
        let (tx, e) = amend_tx(&k, &p, &k.long, LONG, w, &p, v);
        assert_input_refused(&tx, &e, 0);
    }
}

#[test]
fn p1_no_continuation_or_two_continuations_refused() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let n = listed_long(&p);
    let a = || vec![Arg::Sig(k.long), i(LONG), i(n.long_ask), i(n.long_min), i(n.long_tip)];
    let (tx, e) = build(&[pair_in(&p, v, PAIR_ID, "amend", a()), f_in(&k)], vec![out(v, p2sh(&n.art()), None), change(&k, F_VALUE - v)]);
    assert_input_refused(&tx, &e, 0);
    let (tx, e) = build(&[pair_in(&p, v, PAIR_ID, "amend", a()), f_in(&k)], vec![cont(&n, v), cont(&n, v), change(&k, F_VALUE - v - 1)]);
    assert_input_refused(&tx, &e, 0);
}

// ---------------------------------------------------------------- P2 add margin

fn add_tx(k: &K, p: &Pair, signer: &Keypair, side: i64, delta: i64, next: &Pair, cont_value: u64) -> (Transaction, Vec<UtxoEntry>) {
    let v = p.floor() as u64;
    let ins = [pair_in(p, v, PAIR_ID, "addMargin", vec![Arg::Sig(*signer), i(side), i(delta)]), f_in(k)];
    let extra = cont_value.saturating_sub(v);
    build(&ins, vec![cont(next, cont_value), change(k, F_VALUE - extra - 10_000)])
}

#[test]
fn p2_long_lowers_bl_and_short_raises_bs() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let d = 100_000_000; // 0.1 KAS per token on 1,000 tokens = 100 KAS
    let mut n = p.cleared();
    n.bl -= d;
    let need = q(p.size, d, SCALE, SCALE - 1) as u64;
    assert_eq!(need, 100_000_000_000);
    let (tx, e) = add_tx(&k, &p, &k.long, LONG, d, &n, v + need);
    accept(&tx, &e);
    let mut n = p.cleared();
    n.bs += d;
    let (tx, e) = add_tx(&k, &p, &k.short, SHORT, d, &n, v + need);
    accept(&tx, &e);
    // The long may go down to bl = 0.
    let mut n = p.cleared();
    n.bl = 0;
    let need0 = q(p.size, p.bl, SCALE, SCALE - 1) as u64;
    let (tx, e) = add_tx(&k, &p, &k.long, LONG, p.bl, &n, v + need0);
    accept(&tx, &e);
}

#[test]
fn p2_rounds_the_increment_up() {
    let k = keys();
    let mut p = base(&k);
    p.size = 1_500_001; // 1.500001 tokens
    let v = p.floor() as u64;
    let d = 3;
    let up = q(p.size, d, SCALE, SCALE - 1);
    let down = q(p.size, d, SCALE, 0);
    assert_eq!((down, up), (4, 5));
    let mut n = p.cleared();
    n.bl -= d;
    let (tx, e) = add_tx(&k, &p, &k.long, LONG, d, &n, v + up as u64);
    accept(&tx, &e);
    let (tx, e) = add_tx(&k, &p, &k.long, LONG, d, &n, v + down as u64);
    assert_input_refused(&tx, &e, 0);
}

#[test]
fn p2_wrong_signer_is_refused() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let d = 100_000_000;
    let need = q(p.size, d, SCALE, SCALE - 1) as u64;
    let mut n = p.cleared();
    n.bl -= d;
    for signer in [&k.short, &k.other] {
        let (tx, e) = add_tx(&k, &p, signer, LONG, d, &n, v + need);
        assert_input_refused(&tx, &e, 0);
    }
    let mut n = p.cleared();
    n.bs += d;
    for signer in [&k.long, &k.other] {
        let (tx, e) = add_tx(&k, &p, signer, SHORT, d, &n, v + need);
        assert_input_refused(&tx, &e, 0);
    }
}

#[test]
fn p2_too_little_value_is_refused() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let d = 100_000_000;
    let need = q(p.size, d, SCALE, SCALE - 1) as u64;
    let mut n = p.cleared();
    n.bl -= d;
    let (tx, e) = add_tx(&k, &p, &k.long, LONG, d, &n, v + need - 1);
    assert_input_refused(&tx, &e, 0);
    // A pair already holding more than its floor still has to grow by the increment.
    let ins = [pair_in(&p, v + 5_000, PAIR_ID, "addMargin", vec![Arg::Sig(k.long), i(LONG), i(d)]), f_in(&k)];
    let (tx, e) = build(&ins, vec![cont(&n, v + need), change(&k, F_VALUE - need)]);
    assert_input_refused(&tx, &e, 0);
}

#[test]
fn p2_range_violations_are_refused() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    // bl < 0.
    let d = p.bl + 1;
    let mut n = p.cleared();
    n.bl = -1;
    let (tx, e) = add_tx(&k, &p, &k.long, LONG, d, &n, v + q(p.size, d, SCALE, SCALE - 1) as u64);
    assert_input_refused(&tx, &e, 0);
    // bs > priceMax.
    let d = PRICE_MAX - p.bs + 1;
    let mut n = p.cleared();
    n.bs = PRICE_MAX + 1;
    let (tx, e) = add_tx(&k, &p, &k.short, SHORT, d, &n, v);
    assert_input_refused(&tx, &e, 0);
    // A negative delta would move bl to or above bs (and reduce margin).
    for d in [-(p.bs - p.bl), -1, 0] {
        let mut n = p.cleared();
        n.bl -= d;
        let (tx, e) = add_tx(&k, &p, &k.long, LONG, d, &n, v);
        assert_input_refused(&tx, &e, 0);
    }
    // A forged pair whose own state has bl >= bs is refused on entry.
    let mut f = p.clone();
    f.bl = f.bs;
    let mut n = f.cleared();
    n.bl -= 1;
    let (tx, e) = add_tx(&k, &f, &k.long, LONG, 1, &n, v + 1_000_000);
    assert_input_refused(&tx, &e, 0);
    // Margin added on the long but written into bs.
    let mut n = p.cleared();
    n.bs += 100_000_000;
    let (tx, e) = add_tx(&k, &p, &k.long, LONG, 100_000_000, &n, v + 100_000_000_000);
    assert_input_refused(&tx, &e, 0);
}

// ---------------------------------------------------------------- P8 idle settle

/// in 0 pair, 1 F; out 0 long, 1 change, 2 short (M - 1 - 0 = 2 >= 2 inputs).
fn idle_tx(k: &K, p: &Pair, v: u64, long_out: u64, short_out: u64, seq: u64) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [pair_in(p, v, PAIR_ID, "idleSettle", vec![]), f_in(k)];
    build_ext(
        &ins,
        vec![out(long_out, p2pk_spk(&k.long), None), change(k, F_VALUE - 10_000), out(short_out, p2pk_spk(&k.short), None)],
        0,
        &[seq],
    )
}

#[test]
fn p8_settles_at_entry_after_idle() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    // E = 10 KAS: long q(s, E - BL) = 1,000 KAS, short the rest (2,000 KAS + C).
    let lp = q(p.size, p.entry - p.bl, SCALE, 0) as u64;
    assert_eq!(lp, 100_000_000_000);
    let (tx, e) = idle_tx(&k, &p, v, lp, v - lp, IDLE as u64);
    accept(&tx, &e);
    // Any later age works too.
    let (tx, e) = idle_tx(&k, &p, v, lp, v - lp, IDLE as u64 + 1_000);
    accept(&tx, &e);
}

#[test]
fn p8_before_idle_is_refused() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let lp = q(p.size, p.entry - p.bl, SCALE, 0) as u64;
    for seq in [0, IDLE as u64 - 1] {
        let (tx, e) = idle_tx(&k, &p, v, lp, v - lp, seq);
        let err = assert_input_fails(&tx, &e, 0);
        assert!(matches!(err, TxScriptError::UnsatisfiedLockTime(_)), "{err:?}");
    }
}

#[test]
fn p8_payout_below_the_formula_is_refused() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let lp = q(p.size, p.entry - p.bl, SCALE, 0) as u64;
    let (tx, e) = idle_tx(&k, &p, v, lp - 1, v - lp, IDLE as u64);
    assert_input_refused(&tx, &e, 0);
    let (tx, e) = idle_tx(&k, &p, v, lp, v - lp - 1, IDLE as u64);
    assert_input_refused(&tx, &e, 0);
    // Payout to the wrong key, or long and short swapped.
    let ins = [pair_in(&p, v, PAIR_ID, "idleSettle", vec![]), f_in(&k)];
    let (tx, e) = build_ext(
        &ins,
        vec![out(lp, p2pk_spk(&k.other), None), change(&k, F_VALUE - 10_000), out(v - lp, p2pk_spk(&k.short), None)],
        0,
        &[IDLE as u64],
    );
    assert_input_refused(&tx, &e, 0);
    let ins = [pair_in(&p, v, PAIR_ID, "idleSettle", vec![]), f_in(&k)];
    let (tx, e) =
        build_ext(&ins, vec![out(v - lp, p2pk_spk(&k.short), None), change(&k, F_VALUE - 10_000), out(lp, p2pk_spk(&k.long), None)], 0, &[IDLE as u64]);
    assert_input_refused(&tx, &e, 0);
}

#[test]
fn p8_clamps_entry_outside_the_range() {
    let k = keys();
    // E above bs: the long gets the whole range, the short only the carrier.
    let mut p = base(&k);
    p.entry = 5_000_000_000;
    let v = p.floor() as u64;
    let lp = q(p.size, p.bs - p.bl, SCALE, 0) as u64;
    let (tx, e) = idle_tx(&k, &p, v, lp, v - lp, IDLE as u64);
    accept(&tx, &e);
    let (tx, e) = idle_tx(&k, &p, v, lp - 1, v - lp + 1, IDLE as u64);
    assert_input_refused(&tx, &e, 0);
    // E below bl (here 0): the long's payout is 0, below dust, so it is not created; the short gets everything.
    let mut p = base(&k);
    p.entry = 0;
    let ins = [pair_in(&p, v, PAIR_ID, "idleSettle", vec![]), f_in(&k)];
    let (tx, e) =
        build_ext(&ins, vec![out(1, p2pk_spk(&k.f), None), change(&k, F_VALUE - 10_000), out(v, p2pk_spk(&k.short), None)], 0, &[IDLE as u64]);
    accept(&tx, &e);
    let (tx, e) = idle_tx(&k, &p, v, 0, v - 1, IDLE as u64);
    assert_input_refused(&tx, &e, 0);
}

#[test]
fn p8_reverse_slot_and_lineage_rules() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let lp = q(p.size, p.entry - p.bl, SCALE, 0) as u64;
    // Only two outputs: the short's slot M - 1 - 0 = 1 is below the input count 2.
    let ins = [pair_in(&p, v, PAIR_ID, "idleSettle", vec![]), f_in(&k)];
    let (tx, e) = build_ext(&ins, vec![out(lp, p2pk_spk(&k.long), None), out(v - lp, p2pk_spk(&k.short), None)], 0, &[IDLE as u64]);
    assert_input_refused(&tx, &e, 0);
    // An output still bound to the pair's id.
    let ins = [pair_in(&p, v, PAIR_ID, "idleSettle", vec![]), f_in(&k)];
    let (tx, e) = build_ext(
        &ins,
        vec![out(lp, p2pk_spk(&k.long), None), out(10_000, p2pk_spk(&k.f), bind(0, PAIR_ID)), out(v - lp, p2pk_spk(&k.short), None)],
        0,
        &[IDLE as u64],
    );
    assert_input_refused(&tx, &e, 0);
    // The pair at input 1 (F first): long at output 1, short at M - 2.
    let ins = [f_in(&k), pair_in(&p, v, PAIR_ID, "idleSettle", vec![])];
    let (tx, e) = build_ext(
        &ins,
        vec![change(&k, F_VALUE - 10_000), out(lp, p2pk_spk(&k.long), None), out(v - lp, p2pk_spk(&k.short), None), change(&k, 0)],
        0,
        &[0, IDLE as u64],
    );
    accept(&tx, &e);
}

// ---------------------------------------------------------------- P9 cooperative close

fn coop_tx(k: &K, p: &Pair, sl: &Keypair, ss: &Keypair, outs: Vec<TransactionOutput>) -> (Transaction, Vec<UtxoEntry>) {
    let ins = [pair_in(p, p.floor() as u64, PAIR_ID, "coopClose", vec![Arg::Sig(*sl), Arg::Sig(*ss)]), f_in(k)];
    build(&ins, outs)
}

#[test]
fn p9_both_owners_split_freely() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    for split in [0, 1, v / 3, v] {
        let (tx, e) = coop_tx(&k, &p, &k.long, &k.short, vec![out(split, p2pk_spk(&k.long), None), out(v - split, p2pk_spk(&k.short), None)]);
        accept(&tx, &e);
    }
    // Even to a third party.
    let (tx, e) = coop_tx(&k, &p, &k.long, &k.short, vec![out(v, p2pk_spk(&k.other), None)]);
    accept(&tx, &e);
}

#[test]
fn p9_needs_both_signatures() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let outs = || vec![out(v, p2pk_spk(&k.long), None)];
    for (sl, ss) in [(&k.long, &k.long), (&k.short, &k.short), (&k.short, &k.long), (&k.other, &k.short), (&k.long, &k.other)] {
        let (tx, e) = coop_tx(&k, &p, sl, ss, outs());
        assert_input_refused(&tx, &e, 0);
    }
}

#[test]
fn p9_ends_the_pair() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let (tx, e) = coop_tx(&k, &p, &k.long, &k.short, vec![cont(&p, v)]);
    assert_input_refused(&tx, &e, 0);
}

// ---------------------------------------------------------------- same-id twins

/// Two pair outputs of one multi-output genesis share an id (X5). Spent together, every path refuses both.
#[test]
fn twins_with_one_id_are_refused_on_every_path() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let mut outs = vec![out(v, p2sh(&p.art()), None), out(v, p2sh(&p.art()), None)];
    let g = genesis_id(outpoint(11), &outs, &[0, 1]);
    outs[0].covenant = bind(0, g);
    outs[1].covenant = bind(0, g);
    let (gtx, ge) = build(&[f_in(&k)], outs);
    accept(&gtx, &ge);
    let (t0, t1) = (utxo_of(&gtx, 0), utxo_of(&gtx, 1));
    assert_eq!(t0.1.covenant_id, t1.1.covenant_id);

    let twin = |u: &(kaspa_consensus_core::tx::TransactionOutpoint, UtxoEntry), entry: &'static str, a: Vec<Arg>| In {
        outpoint: u.0,
        utxo: u.1.clone(),
        spend: Spend::Signed { art: p.art(), entry, args: a },
    };
    let lp = q(p.size, p.entry - p.bl, SCALE, 0) as u64;
    // P8: each twin alone settles.
    let (tx, e) = build_ext(
        &[twin(&t0, "idleSettle", vec![]), f_in(&k)],
        vec![out(lp, p2pk_spk(&k.long), None), change(&k, F_VALUE - 10_000), out(v - lp, p2pk_spk(&k.short), None)],
        0,
        &[IDLE as u64],
    );
    accept(&tx, &e);
    // Together (outputs for both at their own slots: in 0 -> out 0 / out 3, in 1 -> out 1 / out 2).
    let (tx, e) = build_ext(
        &[twin(&t0, "idleSettle", vec![]), twin(&t1, "idleSettle", vec![])],
        vec![
            out(lp, p2pk_spk(&k.long), None),
            out(lp, p2pk_spk(&k.long), None),
            out(v - lp, p2pk_spk(&k.short), None),
            out(v - lp, p2pk_spk(&k.short), None),
        ],
        0,
        &[IDLE as u64, IDLE as u64],
    );
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
    // P9 together.
    let sigs = || vec![Arg::Sig(k.long), Arg::Sig(k.short)];
    let (tx, e) = build(&[twin(&t0, "coopClose", sigs()), twin(&t1, "coopClose", sigs())], vec![out(2 * v, p2pk_spk(&k.long), None)]);
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
    // P1 and P2 together, each with a continuation bound to the shared id.
    let n = listed_long(&p);
    let a = || vec![Arg::Sig(k.long), i(LONG), i(n.long_ask), i(n.long_min), i(n.long_tip)];
    let (tx, e) = build(
        &[twin(&t0, "amend", a()), twin(&t1, "amend", a())],
        vec![out(v, p2sh(&n.art()), bind(0, g)), out(v, p2sh(&n.art()), bind(1, g))],
    );
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
    let mut m = p.cleared();
    m.bl -= 1_000_000;
    let need = q(p.size, 1_000_000, SCALE, SCALE - 1) as u64;
    let a = || vec![Arg::Sig(k.long), i(LONG), i(1_000_000)];
    let (tx, e) = build(
        &[twin(&t0, "addMargin", a()), twin(&t1, "addMargin", a()), f_in(&k)],
        vec![out(v + need, p2sh(&m.art()), bind(0, g)), out(v + need, p2sh(&m.art()), bind(1, g)), change(&k, F_VALUE - 2 * need)],
    );
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
}

/// A pair without a covenant id (or a foreign one) is never spendable on these paths: OpCovInputCount(ZERO) == 0.
#[test]
fn idless_pair_is_refused() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let lp = q(p.size, p.entry - p.bl, SCALE, 0) as u64;
    let ins = [
        In {
            outpoint: outpoint(10),
            utxo: UtxoEntry::new(v, p2sh(&p.art()), 0, false, None),
            spend: Spend::Signed { art: p.art(), entry: "idleSettle", args: vec![] },
        },
        f_in(&k),
    ];
    let (tx, e) = build_ext(
        &ins,
        vec![out(lp, p2pk_spk(&k.long), None), change(&k, F_VALUE - 10_000), out(v - lp, p2pk_spk(&k.short), None)],
        0,
        &[IDLE as u64],
    );
    assert_input_refused(&tx, &e, 0);
}

// ---------------------------------------------------------------- measurements

/// Script units (consensus flags, signature checks charged) and the largest stack of each path's accepted shape.
/// Run with `--nocapture` to print the table.
#[test]
fn metrics_pair_paths() {
    let k = keys();
    let p = base(&k);
    let v = p.floor() as u64;
    let lp = q(p.size, p.entry - p.bl, SCALE, 0) as u64;
    let d = 100_000_000;
    let mut m = p.cleared();
    m.bl -= d;
    let need = q(p.size, d, SCALE, SCALE - 1) as u64;
    let shapes = vec![
        ("P1 amend", amend_tx(&k, &p, &k.long, LONG, (1_100_000_000, 100_000_000, 1_000_000), &listed_long(&p), v)),
        ("P2 addMargin", add_tx(&k, &p, &k.long, LONG, d, &m, v + need)),
        ("P8 idleSettle", idle_tx(&k, &p, v, lp, v - lp, IDLE as u64)),
        ("P9 coopClose", coop_tx(&k, &p, &k.long, &k.short, vec![out(v, p2pk_spk(&k.long), None)])),
    ];
    println!("KpfPair: script {} B, state {} B", bytecode(pair_base()).len(), p.state_bytes().len());
    for (name, (tx, e)) in shapes {
        let r = &run_metered(&tx, &e)[0];
        assert!(r.result.is_ok(), "{name}: {:?}", r.result);
        assert!(r.max_stack < 244);
        println!("{name:<16} sigscript {:>5} B  script units {:>7}  max stack {:>3}", tx.inputs[0].signature_script.len(), r.script_units, r.max_stack);
    }
}
