//! KpfOpen: T1 open (DESIGN 4.5) against an opposite KpfOpen, cancel and refund (DESIGN 4.3).
//!
//! T1 shape: in 0 KpfOpen long L, 1 KpfOpen short S, 2 funder F (P2PK). out 0 L continuation-or-refund, 1 S
//! continuation-or-refund, 2 the new KpfPair (a genesis authored by F), 3 change. Every accepted transaction is
//! checked for conservation.

mod common;
mod kpf;

use common::*;
use kaspa_consensus_core::tx::{Transaction, TransactionOutpoint, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kaspa_txscript_errors::TxScriptError;
use kpf::*;
use secp256k1::Keypair;
use silverscript_abi::ArtifactValue;

const L_ID: Hash = Hash::from_bytes([0x41; 32]);
const S_ID: Hash = Hash::from_bytes([0x42; 32]);
const F_VALUE: u64 = 1_000_000_000;

fn lk() -> Keypair {
    keypair(21)
}
fn sk() -> Keypair {
    keypair(22)
}
fn other() -> Keypair {
    keypair(23)
}
fn fk() -> Keypair {
    keypair(24)
}

fn i(v: i64) -> ArtifactValue {
    ArtifactValue::Int(v)
}

/// Accepted, and the transaction creates no sompi.
fn accept(tx: &Transaction, e: &[UtxoEntry]) {
    assert_accepted(tx, e);
    let ins: u64 = e.iter().map(|u| u.amount).sum();
    let outs: u64 = tx.outputs.iter().map(|o| o.value).sum();
    assert!(outs <= ins, "outputs {outs} > inputs {ins}");
}

#[derive(Clone)]
enum PairBind {
    Genesis,
    Idless,
    /// F's UTXO carries this id and continues it into the pair output.
    Inherit(Hash),
}

#[derive(Clone)]
struct Spec {
    l: Open,
    s: Open,
    l_id: Hash,
    s_id: Hash,
    vl: u64,
    vs: u64,
    n: i64,
    x: i64,
    bl: i64,
    bs: i64,
    /// The pair state written into output 2.
    pair: Pair,
    pair_value: u64,
    /// (counterKey, counterTag) arguments of each order.
    l_counter: ([u8; 32], Hash),
    s_counter: ([u8; 32], Hash),
    l_out: Option<TransactionOutput>,
    s_out: Option<TransactionOutput>,
    bind: PairBind,
    lock_time: u64,
}

/// 1,000 tokens at X = 10 KAS between the default long (rate 1) and short (rate 2) orders.
fn spec() -> Spec {
    let l = long_order(&lk());
    let s = short_order(&sk());
    let (n, x) = (1_000_000_000, 1_000_000_000);
    let (bl, bs) = (x - l.my_rate, x + s.my_rate);
    let mut p = pair(&lk(), &sk());
    (p.size, p.bl, p.bs, p.entry, p.tag_l, p.tag_s) = (n, bl, bs, x, L_ID, S_ID);
    let pv = p.floor() as u64;
    Spec {
        vl: l.escrow() as u64,
        vs: s.escrow() as u64,
        l,
        s,
        l_id: L_ID,
        s_id: S_ID,
        n,
        x,
        bl,
        bs,
        pair: p,
        pair_value: pv,
        l_counter: (xonly(&sk()), S_ID),
        s_counter: (xonly(&lk()), L_ID),
        l_out: None,
        s_out: None,
        bind: PairBind::Genesis,
        lock_time: 0,
    }
}

fn spend_of(o: &Open, n: i64) -> u64 {
    (q(n, o.my_rate + o.tip, o.m.scale, 0) + o.m.carrier) as u64
}

/// The continuation-or-refund an order requires at `idx`.
fn order_out(o: &Open, id: Hash, v: u64, n: i64, idx: u16) -> TransactionOutput {
    let left = o.size_left - n;
    let rest = v - spend_of(o, n);
    if left > 0 {
        let mut c = o.clone();
        c.size_left = left;
        out(rest, p2sh(&c.art()), bind(idx, id))
    } else {
        out(rest, p2pk_spk(&kp_of(o)), None)
    }
}

fn kp_of(o: &Open) -> Keypair {
    [lk(), sk(), other(), fk(), keypair(25), keypair(26)].into_iter().find(|k| xonly(k) == o.owner).expect("known owner")
}

fn fill_args(sp: &Spec, counter: ([u8; 32], Hash)) -> Vec<Arg> {
    let (pre, suf) = parts(pair_base());
    args(&[i(2), i(sp.n), i(sp.x), i(sp.bl), i(sp.bs), bytes(&counter.0), bytes(&counter.1.as_bytes()), bytes(&pre), bytes(&suf)])
}

fn open_in(o: &Open, op: TransactionOutpoint, v: u64, id: Hash, entry: &'static str, a: Vec<Arg>) -> In {
    In { outpoint: op, utxo: UtxoEntry::new(v, p2sh(&o.art()), 0, false, Some(id)), spend: Spend::Signed { art: o.art(), entry, args: a } }
}

fn t1(sp: &Spec) -> (Transaction, Vec<UtxoEntry>) {
    let f_cov = match sp.bind {
        PairBind::Inherit(h) => Some(h),
        _ => None,
    };
    let ins = [
        open_in(&sp.l, outpoint(1), sp.vl, sp.l_id, "fill", fill_args(sp, sp.l_counter)),
        open_in(&sp.s, outpoint(2), sp.vs, sp.s_id, "fill", fill_args(sp, sp.s_counter)),
        p2pk_in(outpoint(3), F_VALUE, f_cov, &fk()),
    ];
    let mut outs = vec![
        sp.l_out.clone().unwrap_or_else(|| order_out(&sp.l, sp.l_id, sp.vl, sp.n, 0)),
        sp.s_out.clone().unwrap_or_else(|| order_out(&sp.s, sp.s_id, sp.vs, sp.n, 1)),
        out(sp.pair_value, p2sh(&sp.pair.art()), None),
        out(F_VALUE - 10_000, p2pk_spk(&fk()), None),
    ];
    outs[2].covenant = match sp.bind {
        PairBind::Genesis => bind(2, genesis_id(outpoint(3), &outs, &[2])),
        PairBind::Idless => None,
        PairBind::Inherit(h) => bind(2, h),
    };
    build_ext(&ins, outs, sp.lock_time, &[])
}

fn both_refused(sp: &Spec) {
    let (tx, e) = t1(sp);
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
}

fn only_refused(sp: &Spec, idx: usize) {
    let (tx, e) = t1(sp);
    let r = run(&tx, &e).expect("bindings");
    assert!(r[1 - idx].is_ok(), "the other order should accept: {:?}", r[1 - idx]);
    assert_input_refused(&tx, &e, idx);
}

// ---------------------------------------------------------------- T1 accepted

#[test]
fn t1_opens_a_pair_partial_long_full_short() {
    let sp = spec();
    let (tx, e) = t1(&sp);
    accept(&tx, &e);
    // The executor keeps the tips and one spare carrier: inputs - outputs - fee.
    let ins: u64 = e.iter().map(|u| u.amount).sum();
    let outs: u64 = tx.outputs.iter().map(|o| o.value).sum();
    assert_eq!(ins - outs - 10_000, 2 * 1_000_000_000 + C as u64);

    // The new pair is a live KpfPair: both owners can close it cooperatively.
    let pu = utxo_of(&tx, 2);
    assert_ne!(pu.1.covenant_id, None);
    let p = sp.pair.clone();
    let ins = [In { outpoint: pu.0, utxo: pu.1, spend: Spend::Signed { art: p.art(), entry: "coopClose", args: vec![Arg::Sig(lk()), Arg::Sig(sk())] } }];
    let (ctx, ce) = build(&ins, vec![out(sp.pair_value, p2pk_spk(&lk()), None)]);
    accept(&ctx, &ce);

    // The long's continuation (1,000 tokens left) fills again, against a second short, at another price.
    let lc = utxo_of(&tx, 0);
    let mut l2 = sp.l.clone();
    l2.size_left = 1_000_000_000;
    let mut sp2 = spec();
    sp2.l = l2;
    sp2.vl = lc.1.amount;
    sp2.x = 1_050_000_000;
    sp2.bl = sp2.x - sp2.l.my_rate;
    sp2.bs = sp2.x + sp2.s.my_rate;
    (sp2.pair.bl, sp2.pair.bs, sp2.pair.entry) = (sp2.bl, sp2.bs, sp2.x);
    sp2.pair_value = sp2.pair.floor() as u64;
    let ins = [
        In { outpoint: lc.0, utxo: lc.1.clone(), spend: Spend::Signed { art: sp2.l.art(), entry: "fill", args: fill_args(&sp2, sp2.l_counter) } },
        open_in(&sp2.s, outpoint(2), sp2.vs, S_ID, "fill", fill_args(&sp2, sp2.s_counter)),
        p2pk_in(outpoint(3), F_VALUE, None, &fk()),
    ];
    let mut outs = vec![
        order_out(&sp2.l, L_ID, sp2.vl, sp2.n, 0),
        order_out(&sp2.s, S_ID, sp2.vs, sp2.n, 1),
        out(sp2.pair_value, p2sh(&sp2.pair.art()), None),
        out(F_VALUE - 10_000, p2pk_spk(&fk()), None),
    ];
    outs[2].covenant = bind(2, genesis_id(outpoint(3), &outs, &[2]));
    let (tx2, e2) = build(&ins, outs);
    accept(&tx2, &e2);
    // The long ended: its rest went to its owner at output 0.
    assert_eq!(tx2.outputs[0].script_public_key, p2pk_spk(&lk()));
}

#[test]
fn t1_any_input_order() {
    // S at input 0, L at input 1; the pair at output 3 and change at 2.
    let sp = spec();
    let (pre, suf) = parts(pair_base());
    let a = |c: ([u8; 32], Hash)| args(&[i(3), i(sp.n), i(sp.x), i(sp.bl), i(sp.bs), bytes(&c.0), bytes(&c.1.as_bytes()), bytes(&pre), bytes(&suf)]);
    let ins = [
        open_in(&sp.s, outpoint(2), sp.vs, S_ID, "fill", a(sp.s_counter)),
        open_in(&sp.l, outpoint(1), sp.vl, L_ID, "fill", a(sp.l_counter)),
        p2pk_in(outpoint(3), F_VALUE, None, &fk()),
    ];
    let mut outs = vec![
        order_out(&sp.s, S_ID, sp.vs, sp.n, 0),
        order_out(&sp.l, L_ID, sp.vl, sp.n, 1),
        out(F_VALUE - 10_000, p2pk_spk(&fk()), None),
        out(sp.pair_value, p2sh(&sp.pair.art()), None),
    ];
    outs[3].covenant = bind(2, genesis_id(outpoint(3), &outs, &[3]));
    let (tx, e) = build(&ins, outs);
    accept(&tx, &e);
}

#[test]
fn t1_minfill_exception_takes_all_that_is_left() {
    let mut sp = spec();
    // The short has only 50 tokens left, below its minFill of 100: a fill of all 50 is allowed.
    sp.s.size_left = 50_000_000;
    sp.vs = sp.s.escrow() as u64;
    sp.n = 50_000_000;
    sp.pair.size = sp.n;
    sp.pair_value = sp.pair.floor() as u64;
    // The long (2,000 left, minFill 100) refuses a 50-token part; with minFill 10 it accepts.
    only_refused(&sp, 0);
    sp.l.min_fill = 10_000_000;
    sp.vl = sp.l.escrow() as u64;
    let (tx, e) = t1(&sp);
    accept(&tx, &e);
}

// ---------------------------------------------------------------- T1 refused

#[test]
fn t1_idless_pair_output_is_refused() {
    let mut sp = spec();
    sp.bind = PairBind::Idless;
    both_refused(&sp);
}

#[test]
fn t1_inherited_id_is_refused() {
    let mut sp = spec();
    sp.bind = PairBind::Inherit(Hash::from_bytes([0x77; 32]));
    both_refused(&sp);
    // Nor may the pair continue the long order's own id.
    let mut sp = spec();
    sp.bind = PairBind::Inherit(L_ID);
    let (tx, e) = t1(&sp);
    // F carries L_ID and continues it into the pair: two inputs now carry L_ID, so the long's single-input guard
    // refuses, and the short sees the pair id already counted among the inputs.
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
}

#[test]
fn t1_price_beyond_either_limit_is_refused() {
    let with_x = |x: i64| {
        let mut sp = spec();
        sp.x = x;
        sp.bl = x - sp.l.my_rate;
        sp.bs = x + sp.s.my_rate;
        (sp.pair.bl, sp.pair.bs, sp.pair.entry) = (sp.bl, sp.bs, x);
        sp.pair_value = sp.pair.floor() as u64;
        sp
    };
    let (tx, e) = t1(&with_x(1_050_000_000));
    accept(&tx, &e);
    only_refused(&with_x(1_050_000_001), 0);
    let (tx, e) = t1(&with_x(950_000_000));
    accept(&tx, &e);
    only_refused(&with_x(949_999_999), 1);
}

#[test]
fn t1_counter_rate_below_minimum_is_refused() {
    // The short posts 1.49999999 KAS per token; the long wants at least 1.5.
    let mut sp = spec();
    sp.s.my_rate = 149_999_999;
    sp.vs = sp.s.escrow() as u64;
    sp.bs = sp.x + sp.s.my_rate;
    sp.pair.bs = sp.bs;
    sp.pair_value = sp.pair.floor() as u64;
    only_refused(&sp, 0);
    sp.s.my_rate = 150_000_000;
    sp.vs = sp.s.escrow() as u64;
    sp.bs = sp.x + sp.s.my_rate;
    sp.pair.bs = sp.bs;
    sp.pair_value = sp.pair.floor() as u64;
    let (tx, e) = t1(&sp);
    accept(&tx, &e);
    // The long posts 0.59999999 KAS; the short wants 0.6. Premise changed by review 3 (DESIGN 4.5c): the long used to
    // post 0.4 against a wish of 0.5, but a rate below 5 % of x = 10 is now refused by both orders (neither side may be
    // born liquidatable), so the counter-rate rule is shown above the maintenance margin.
    let mut sp = spec();
    sp.s.min_counter_rate = 60_000_000;
    sp.vs = sp.s.escrow() as u64;
    sp.l.my_rate = 59_999_999;
    sp.vl = sp.l.escrow() as u64;
    sp.bl = sp.x - sp.l.my_rate;
    sp.pair.bl = sp.bl;
    sp.pair_value = sp.pair.floor() as u64;
    only_refused(&sp, 1);
    // A range that does not match the order's own rate.
    let mut sp = spec();
    sp.bl -= 1;
    sp.pair.bl = sp.bl;
    sp.pair_value = sp.pair.floor() as u64;
    only_refused(&sp, 0);
}

#[test]
fn t1_pair_value_one_sompi_short_is_refused() {
    let mut sp = spec();
    sp.pair_value -= 1;
    both_refused(&sp);
}

#[test]
fn t1_wrong_key_or_tag_in_the_pair_is_refused() {
    // Long key replaced; the short is told the same key, so only the long refuses.
    let mut sp = spec();
    sp.pair.long_key = xonly(&other());
    sp.s_counter.0 = xonly(&other());
    only_refused(&sp, 0);
    // Long tag zero.
    let mut sp = spec();
    sp.pair.tag_l = ZERO;
    sp.s_counter.1 = ZERO;
    only_refused(&sp, 0);
    // Short key and tag.
    let mut sp = spec();
    sp.pair.short_key = xonly(&other());
    sp.l_counter.0 = xonly(&other());
    only_refused(&sp, 1);
    let mut sp = spec();
    sp.pair.tag_s = L_ID;
    sp.l_counter.1 = L_ID;
    only_refused(&sp, 1);
    // Keys swapped between the sides.
    let mut sp = spec();
    (sp.pair.long_key, sp.pair.short_key) = (sp.pair.short_key, sp.pair.long_key);
    both_refused(&sp);
    // Any other state difference: a listing, the entry, the size, the market block.
    let fs: [fn(&mut Pair); 6] = [
        |p: &mut Pair| p.long_ask = 1,
        |p: &mut Pair| p.short_tip = 1,
        |p: &mut Pair| p.entry += 1,
        |p: &mut Pair| p.size -= 1,
        |p: &mut Pair| p.m.idle_daa = 1,
        |p: &mut Pair| p.m.ext = Hash::from_bytes([9; 32]),
    ];
    for f in fs {
        let mut sp = spec();
        f(&mut sp.pair);
        sp.pair_value = sp.pair.floor().max(spec().pair.floor()) as u64;
        both_refused(&sp);
    }
}

#[test]
fn t1_other_template_is_refused() {
    // The pair output under another script with the same state (here the KpfOpen template), as a look-alike.
    let sp = spec();
    let (pre, suf) = parts(open_base());
    let a = |c: ([u8; 32], Hash)| args(&[i(2), i(sp.n), i(sp.x), i(sp.bl), i(sp.bs), bytes(&c.0), bytes(&c.1.as_bytes()), bytes(&pre), bytes(&suf)]);
    let ins = [
        open_in(&sp.l, outpoint(1), sp.vl, L_ID, "fill", a(sp.l_counter)),
        open_in(&sp.s, outpoint(2), sp.vs, S_ID, "fill", a(sp.s_counter)),
        p2pk_in(outpoint(3), F_VALUE, None, &fk()),
    ];
    let mut outs = vec![
        order_out(&sp.l, L_ID, sp.vl, sp.n, 0),
        order_out(&sp.s, S_ID, sp.vs, sp.n, 1),
        out(sp.pair_value, pay_to_redeem(&[pre.clone(), sp.pair.state_bytes(), suf.clone()].concat()), None),
        out(F_VALUE - 10_000, p2pk_spk(&fk()), None),
    ];
    outs[2].covenant = bind(2, genesis_id(outpoint(3), &outs, &[2]));
    let (tx, e) = build(&ins, outs);
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
}

fn pay_to_redeem(rs: &[u8]) -> kaspa_consensus_core::tx::ScriptPublicKey {
    kaspa_txscript::pay_to_script_hash_script(rs)
}

#[test]
fn t1_partial_fill_below_minfill_is_refused() {
    let mut sp = spec();
    sp.n = 99_999_999; // minFill is 100 tokens for both
    sp.pair.size = sp.n;
    sp.pair_value = sp.pair.floor() as u64;
    both_refused(&sp);
    // A part below minSize is refused even when it takes all that is left.
    let mut sp = spec();
    sp.s.size_left = 999_999;
    sp.s.min_fill = 0;
    sp.vs = sp.s.escrow() as u64;
    sp.n = 999_999;
    sp.pair.size = sp.n;
    sp.pair_value = sp.pair.floor() as u64;
    both_refused(&sp);
}

#[test]
fn t1_remainder_below_minimum_is_refused() {
    // The long has 2,000 tokens; a fill of 1,999.5 leaves 0.5 token, below minSize (1 token).
    let mut sp = spec();
    sp.s.size_left = 3_000_000_000;
    sp.vs = sp.s.escrow() as u64;
    sp.n = 1_999_500_000;
    sp.pair.size = sp.n;
    sp.pair_value = sp.pair.floor() as u64;
    only_refused(&sp, 0);
    // Leaving exactly minSize is fine.
    sp.n = 1_999_000_000;
    sp.pair.size = sp.n;
    sp.pair_value = sp.pair.floor() as u64;
    let (tx, e) = t1(&sp);
    accept(&tx, &e);
}

#[test]
fn t1_continuation_and_refund_are_enforced() {
    let base = spec();
    // The long's continuation 1 sompi short.
    let mut sp = base.clone();
    let mut o = order_out(&sp.l, L_ID, sp.vl, sp.n, 0);
    o.value -= 1;
    sp.l_out = Some(o);
    only_refused(&sp, 0);
    // The long's continuation with the wrong sizeLeft (nothing taken off).
    let mut sp = base.clone();
    sp.l_out = Some(out(sp.vl - spend_of(&sp.l, sp.n), p2sh(&sp.l.art()), bind(0, L_ID)));
    only_refused(&sp, 0);
    // The long's continuation with a changed limit.
    let mut sp = base.clone();
    let mut c = sp.l.clone();
    c.size_left -= sp.n;
    c.limit += 1;
    sp.l_out = Some(out(sp.vl - spend_of(&sp.l, sp.n), p2sh(&c.art()), bind(0, L_ID)));
    only_refused(&sp, 0);
    // The short's refund to another key, or 1 sompi short.
    let mut sp = base.clone();
    sp.s_out = Some(out(sp.vs - spend_of(&sp.s, sp.n), p2pk_spk(&other()), None));
    only_refused(&sp, 1);
    let mut sp = base.clone();
    sp.s_out = Some(out(sp.vs - spend_of(&sp.s, sp.n) - 1, p2pk_spk(&sk()), None));
    only_refused(&sp, 1);
    // The short filled out but its id kept alive on an output.
    let mut sp = base.clone();
    sp.s_out = Some(out(sp.vs - spend_of(&sp.s, sp.n), p2pk_spk(&sk()), bind(1, S_ID)));
    only_refused(&sp, 1);
}

#[test]
fn t1_before_active_from_is_refused() {
    let mut sp = spec();
    sp.l.active_from = 5_000;
    sp.lock_time = 4_999;
    let (tx, e) = t1(&sp);
    let err = assert_input_fails(&tx, &e, 0);
    assert!(matches!(err, TxScriptError::UnsatisfiedLockTime(_)), "{err:?}");
    // At activeFrom the order fills (its continuation keeps activeFrom).
    sp.lock_time = 5_000;
    let (tx, e) = t1(&sp);
    accept(&tx, &e);
}

// ---------------------------------------------------------------- one output, two claimers

#[test]
fn a_pair_output_claimed_by_another_long_is_refused() {
    // L1 (id L_ID) and L2 (id 0x43..) both claim the pair output whose tagL is L1; S fills against it.
    let sp = spec();
    let l2_id = Hash::from_bytes([0x43; 32]);
    let l2 = long_order(&keypair(25));
    let vl2 = l2.escrow() as u64;
    let (pre, suf) = parts(pair_base());
    let a = |c: ([u8; 32], Hash)| args(&[i(3), i(sp.n), i(sp.x), i(sp.bl), i(sp.bs), bytes(&c.0), bytes(&c.1.as_bytes()), bytes(&pre), bytes(&suf)]);
    let ins = [
        open_in(&sp.l, outpoint(1), sp.vl, L_ID, "fill", a(sp.l_counter)),
        open_in(&sp.s, outpoint(2), sp.vs, S_ID, "fill", a(sp.s_counter)),
        open_in(&l2, outpoint(4), vl2, l2_id, "fill", a(sp.l_counter)),
        p2pk_in(outpoint(3), F_VALUE, None, &fk()),
    ];
    let mut outs = vec![
        order_out(&sp.l, L_ID, sp.vl, sp.n, 0),
        order_out(&sp.s, S_ID, sp.vs, sp.n, 1),
        order_out(&l2, l2_id, vl2, sp.n, 2),
        out(sp.pair_value, p2sh(&sp.pair.art()), None),
        out(F_VALUE - 10_000, p2pk_spk(&fk()), None),
    ];
    outs[3].covenant = bind(3, genesis_id(outpoint(3), &outs, &[3]));
    let (tx, e) = build(&ins, outs);
    let r = run(&tx, &e).unwrap();
    assert!(r[0].is_ok() && r[1].is_ok(), "{r:?}");
    assert_input_refused(&tx, &e, 2);
}

#[test]
fn two_long_orders_sharing_an_id_cannot_claim() {
    // Two long orders from one multi-output genesis share an id; each would rebuild the same pair output.
    let l = long_order(&lk());
    let v = l.escrow() as u64;
    let mut outs = vec![out(v, p2sh(&l.art()), None), out(v, p2sh(&l.art()), None)];
    let g = genesis_id(outpoint(9), &outs, &[0, 1]);
    outs[0].covenant = bind(0, g);
    outs[1].covenant = bind(0, g);
    let (gtx, ge) = build(&[p2pk_in(outpoint(9), 3 * v, None, &fk())], outs);
    accept(&gtx, &ge);
    let (t0, t1u) = (utxo_of(&gtx, 0), utxo_of(&gtx, 1));

    let mut sp = spec();
    sp.pair.tag_l = g;
    sp.s_counter.1 = g;
    let (pre, suf) = parts(pair_base());
    let a = |c: ([u8; 32], Hash)| args(&[i(3), i(sp.n), i(sp.x), i(sp.bl), i(sp.bs), bytes(&c.0), bytes(&c.1.as_bytes()), bytes(&pre), bytes(&suf)]);
    let tw = |u: &(TransactionOutpoint, UtxoEntry)| In { outpoint: u.0, utxo: u.1.clone(), spend: Spend::Signed { art: l.art(), entry: "fill", args: a(sp.l_counter) } };
    let ins = [tw(&t0), tw(&t1u), open_in(&sp.s, outpoint(2), sp.vs, S_ID, "fill", a(sp.s_counter)), p2pk_in(outpoint(3), F_VALUE, None, &fk())];
    let mut c = l.clone();
    c.size_left -= sp.n;
    let rest = v - spend_of(&l, sp.n);
    let mut outs = vec![
        out(rest, p2sh(&c.art()), bind(0, g)),
        out(rest, p2sh(&c.art()), bind(1, g)),
        order_out(&sp.s, S_ID, sp.vs, sp.n, 2),
        out(sp.pair_value, p2sh(&sp.pair.art()), None),
    ];
    outs[3].covenant = bind(3, genesis_id(outpoint(3), &outs, &[3]));
    let (tx, e) = build(&ins, outs);
    assert_input_refused(&tx, &e, 0);
    assert_input_refused(&tx, &e, 1);
}

// ---------------------------------------------------------------- cancel and refund

#[test]
fn cancel_needs_the_owner() {
    let l = long_order(&lk());
    let v = l.escrow() as u64;
    let mk = |kp: &Keypair| {
        let ins = [open_in(&l, outpoint(1), v, L_ID, "cancel", vec![Arg::Sig(*kp)])];
        build(&ins, vec![out(v - 10_000, p2pk_spk(&other()), None)])
    };
    let (tx, e) = mk(&lk());
    accept(&tx, &e);
    for kp in [sk(), other(), fk()] {
        let (tx, e) = mk(&kp);
        assert_input_refused(&tx, &e, 0);
    }
}

#[test]
fn refund_only_at_expiry_or_idle() {
    let mut l = long_order(&lk());
    l.expiry_daa = 50_000;
    let v = l.escrow() as u64;
    let mk = |o: &Open, utxo_daa: u64, lock: u64, pay: u64, to: &Keypair| {
        let ins = [In {
            outpoint: outpoint(1),
            utxo: UtxoEntry::new(v, p2sh(&o.art()), utxo_daa, false, Some(L_ID)),
            spend: Spend::Signed { art: o.art(), entry: "refund", args: vec![] },
        }];
        build_ext(&ins, vec![out(pay, p2pk_spk(to), None)], lock, &[])
    };
    // Before expiry.
    let (tx, e) = mk(&l, 0, 49_999, v, &lk());
    let err = assert_input_fails(&tx, &e, 0);
    assert!(matches!(err, TxScriptError::UnsatisfiedLockTime(_)), "{err:?}");
    // At expiry, to the owner, in full (the network fee comes from another input in practice).
    let (tx, e) = mk(&l, 0, 50_000, v, &lk());
    accept(&tx, &e);
    let (tx, e) = mk(&l, 0, 50_000, v - 1, &lk());
    assert_input_refused(&tx, &e, 0);
    let (tx, e) = mk(&l, 0, 50_000, v, &other());
    assert_input_refused(&tx, &e, 0);
    // A far expiry: refundable 90 days after the UTXO's DAA.
    l.expiry_daa = 1_000_000_000;
    let (tx, e) = mk(&l, 1_000, 1_000 + 77_760_000 - 1, v, &lk());
    assert!(matches!(assert_input_fails(&tx, &e, 0), TxScriptError::UnsatisfiedLockTime(_)));
    let (tx, e) = mk(&l, 1_000, 1_000 + 77_760_000, v, &lk());
    accept(&tx, &e);
}

// ---------------------------------------------------------------- measurements

/// Run with `--nocapture` to print the table.
#[test]
fn metrics_open_paths() {
    let sp = spec();
    let (tx, e) = t1(&sp);
    let r = run_metered(&tx, &e);
    println!("KpfOpen: script {} B, state {} B", bytecode(open_base()).len(), sp.l.state_bytes().len());
    for (k, name) in [(0, "T1 fill (long, partial)"), (1, "T1 fill (short, filled out)")] {
        assert!(r[k].result.is_ok(), "{name}: {:?}", r[k].result);
        assert!(r[k].max_stack < 244);
        println!("{name:<28} sigscript {:>5} B  script units {:>7}  max stack {:>3}", tx.inputs[k].signature_script.len(), r[k].script_units, r[k].max_stack);
    }
    let l = long_order(&lk());
    let v = l.escrow() as u64;
    let (tx, e) = build(&[open_in(&l, outpoint(1), v, L_ID, "cancel", vec![Arg::Sig(lk())])], vec![out(v, p2pk_spk(&lk()), None)]);
    let mut l2 = l.clone();
    l2.expiry_daa = 0;
    let (tx2, e2) = build(&[open_in(&l2, outpoint(1), v, L_ID, "refund", vec![])], vec![out(v, p2pk_spk(&lk()), None)]);
    for (name, tx, e) in [("cancel", &tx, &e), ("refund", &tx2, &e2)] {
        let r = &run_metered(tx, e)[0];
        assert!(r.result.is_ok(), "{name}: {:?}", r.result);
        println!("{name:<28} sigscript {:>5} B  script units {:>7}  max stack {:>3}", tx.inputs[0].signature_script.len(), r.script_units, r.max_stack);
    }
}
