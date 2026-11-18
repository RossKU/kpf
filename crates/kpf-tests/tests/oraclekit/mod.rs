//! Fixtures and transaction builders for the oracle-market family (`contracts/KpfOpenO.sil`, `KpfPairO.sil`,
//! `KpfPendingO.sil`) wired to the real slot (`KpfBond`) and the real judge (`KpfStake`). The templates are compiled
//! from source once per test binary (`oracle_attack_tests` checks that they equal the pinned artifacts); states and
//! values come from the library model (`kpf_protocol::oracle`, `bond`, `judge`), so every accepted transaction here is
//! also the model's prediction.
#![allow(dead_code)]

use std::sync::OnceLock;

use kaspa_consensus_core::tx::{CovenantBinding, ScriptPublicKey, Transaction, TransactionOutpoint, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::artifacts::{splice, Template};
use kpf_protocol::bond::{BindArgs, Bond, Claim};
use kpf_protocol::judge::{Js, NO, STATUS_RESOLVED, UNIT, YES};
use kpf_protocol::oracle::*;
use kpf_protocol::state::{sm8, KIND_CLOSE, KIND_LIQ, LONG, SHORT};
use secp256k1::Keypair;
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

use crate::common::*;

pub struct OK {
    pub kit: OKit,
    pub open: SilAbiArtifact,
    pub pair: SilAbiArtifact,
    pub pending: SilAbiArtifact,
    pub bond: SilAbiArtifact,
    pub stake: SilAbiArtifact,
}

fn tag(art: &SilAbiArtifact, entry: &str) -> [u8; 4] {
    *art.contracts.values().next().unwrap().entries[entry].dispatch_tag.as_bytes()
}

// ---------------------------------------------------------------- the fixture market

/// The engine fixtures' oracle market: the slot of `bondkit` (step 100, term 800, tail 200, `a` 10, `U` 50, rho 1),
/// the judge of `judgekit` (rounds of 100, stake aged 500, unbonding 300, mu 1.25, burn 10 %, 10 KAS stakes), a token-
/// less underlying of 6 decimals priced in sompi (the DESIGN 5 example's numbers), W 200, a claim void 100 after its t0,
/// 20 DAA of quiet, a lapsed pair settles after 1,000 idle, the bond 10 % of the cover (at least 1 KAS), the disputer 20 %.
pub fn omarket() -> OMarket {
    omarket_with(kpf_protocol::bond::BondParams { rho_bps: 10_000, step: 100, term: 800, tail: 200, rate: 10, unbond: 50 })
}

pub fn omarket_with(b: kpf_protocol::bond::BondParams) -> OMarket {
    OMarket {
        bond: b,
        judge: kpf_protocol::judge::JudgeParams { mu_bps: 12_500, age: 500, unbond: 300, round: 100, burn_bps: 1_000, min_stake: UNIT },
        feed_hash: feed_hash_of(FEED.as_bytes()),
        p: OParams {
            scale: 1_000_000,
            price_max: 100_000_000_000_000,
            mmr_bps: 500,
            g_bps: 200,
            liq_fee_bps: 50,
            keeper_fee_min: 1_000_000,
            min_size: 1_000_000,
            carrier: 20_000_000,
            idle_daa: 1_000,
            challenge_daa: 200,
            late_daa: 100,
            quiet_daa: 20,
            sigma_bps: 1_000,
            beta_bps: 2_000,
            bond_min: 100_000_000,
        },
    }
}

/// The feed definition of the fixture market.
pub const FEED: &str = "kpf feed definition v1\nquantity: test underlying, a slow series published once per hour\nunit: KAS per whole unit\nsources: median of three named public sources\ntime: the price at the DAA score t0, mapped to the accepting block's timestamp\nsessions: always open\ndisagreement: a claim is true if within 1 % of the median\n";

pub fn ok() -> &'static OK {
    static K: OnceLock<OK> = OnceLock::new();
    K.get_or_init(|| {
        let m = omarket();
        let bond = compile("KpfBond.sil", &Bond::new([7; 32], m.bond_market(), 0).ctor());
        let bond_t = Template::of("KpfBond", &bond);
        let stake = compile("KpfStake.sil", &Js::free([7; 32], m.judge_market()).ctor());
        let stake_t = Template::of("KpfStake", &stake);
        let mut consts = OConsts {
            bond_tpl: bond_t.hash,
            bond_pre: bond_t.prefix.len() as i64,
            bond_suf: bond_t.suffix.len() as i64,
            bind_tag: tag(&bond, "bind"),
            judge_tpl: stake_t.hash,
            judge_pre: stake_t.prefix.len() as i64,
            judge_suf: stake_t.suffix.len() as i64,
            open_tag: tag(&stake, "open"),
            resolve_tag: tag(&stake, "resolve"),
            pending_tpl: [0; 32],
            pair_tpl: [0; 32],
        };
        let sp = sample_pair(&m);
        // The pending template needs no pair template (its return states are hashes); the pair pins the pending one,
        // the order the pair's: KpfOpenO -> KpfPairO -> KpfPendingO -> KpfStake, KpfPairO -> KpfBond, KpfOpenO -> KpfBond.
        let sw = sample_pending(&sp);
        let pending = compile("KpfPendingO.sil", &sw.ctor_with(&consts));
        let pending_t = Template::of("KpfPendingO", &pending);
        consts.pending_tpl = pending_t.hash;
        let pair = compile("KpfPairO.sil", &sp.ctor_with(&consts));
        let pair_t = Template::of("KpfPairO", &pair);
        consts.pair_tpl = pair_t.hash;
        let open = compile("KpfOpenO.sil", &sample_order(&m, LONG, [3; 32]).ctor_with(&consts));
        let open_t = Template::of("KpfOpenO", &open);
        OK { kit: OKit { open: open_t, pair: pair_t, pending: pending_t, bond: bond_t, stake: stake_t, consts }, open, pair, pending, bond, stake }
    })
}

pub fn kit() -> &'static OKit {
    &ok().kit
}

/// The sample pair the templates are compiled with (the template does not depend on the state).
pub fn sample_pair(m: &OMarket) -> OPair {
    OPair {
        m: *m,
        size: 1_000_000_000,
        bl: 900_000_000,
        bs: 1_200_000_000,
        entry: 1_000_000_000,
        long_key: [1; 32],
        short_key: [2; 32],
        b: OBinding { uw_id: [0x51; 32], uw_key: [3; 32], cover_rate: 600_000_000, expiry: 1_200, premium: 2_000_000 },
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        tag_l: [0x1a; 32],
        tag_s: [0x1b; 32],
    }
}

pub fn sample_pending(p: &OPair) -> OPending {
    OPending {
        m: p.m,
        kind: KIND_LIQ,
        side: LONG,
        n: p.size,
        bl: p.bl,
        bs: p.bs,
        quote: 900_000_000,
        t0: 1,
        cure_bound: 800_000_000,
        fee: 1_000_000,
        bond: 100_000_000,
        refund: 100_000_000,
        status: PENDING,
        clock: 0,
        disputer: [0; 32],
        entry: p.entry,
        long_key: p.long_key,
        short_key: p.short_key,
        b: p.b,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        restore_hash: [0x2a; 32],
        cure_hash: [0x2b; 32],
        forfeit_hash: [0x2c; 32],
    }
}

pub fn sample_order(m: &OMarket, side: i64, owner: [u8; 32]) -> OOpen {
    OOpen {
        m: *m,
        side,
        owner,
        size_left: 2_000_000_000,
        limit: if side == LONG { 1_050_000_000 } else { 950_000_000 },
        my_rate: if side == LONG { 100_000_000 } else { 200_000_000 },
        min_counter_rate: if side == LONG { 150_000_000 } else { 50_000_000 },
        min_fill: 100_000_000,
        tip: 0,
        active_from: 0,
        expiry_daa: 1_000_000_000,
        uw_accept: [0; 32],
        prem_max: 10_000_000,
    }
}

// ---------------------------------------------------------------- keys, artifacts, inputs

pub fn x(kp: &Keypair) -> [u8; 32] {
    kp.x_only_public_key().0.serialize()
}
/// The parties, the underwriter, the stakers, the executor.
pub fn long_kp() -> Keypair {
    keypair(41)
}
pub fn short_kp() -> Keypair {
    keypair(42)
}
pub fn uw_kp() -> Keypair {
    keypair(31)
}
pub fn exec_kp() -> Keypair {
    keypair(60)
}

pub fn open_art(o: &OOpen) -> SilAbiArtifact {
    splice(&ok().open, &o.state_bytes())
}
pub fn pair_art(p: &OPair) -> SilAbiArtifact {
    splice(&ok().pair, &p.state_bytes())
}
pub fn pend_art(w: &OPending) -> SilAbiArtifact {
    splice(&ok().pending, &w.state_bytes())
}
pub fn bond_art(b: &Bond) -> SilAbiArtifact {
    splice(&ok().bond, &b.state_bytes())
}
pub fn stake_art(s: &Js) -> SilAbiArtifact {
    splice(&ok().stake, &s.state_bytes())
}

pub fn iv(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}
pub fn bv(b: &[u8]) -> Arg {
    Arg::V(ArtifactValue::Bytes(b.to_vec()))
}
pub fn b8(v: i64) -> Arg {
    Arg::V(ArtifactValue::Bytes(sm8(v).to_vec()))
}

/// A live UTXO of a contract state.
#[derive(Clone, Debug)]
pub struct Live<T> {
    pub op: TransactionOutpoint,
    pub value: u64,
    pub daa: u64,
    pub id: Option<Hash>,
    pub s: T,
}

pub type LOrder = Live<OOpen>;
pub type LPair = Live<OPair>;
pub type LPend = Live<OPending>;
pub type LSlot = Live<Bond>;
pub type LStake = Live<Js>;

impl<T> Live<T> {
    pub fn idb(&self) -> [u8; 32] {
        self.id.map(|h| h.as_bytes()).unwrap_or([0; 32])
    }
}

fn utxo(value: u64, spk: ScriptPublicKey, daa: u64, id: Option<Hash>) -> UtxoEntry {
    UtxoEntry::new(value, spk, daa, false, id)
}

pub fn order_in(l: &LOrder, entry: &'static str, args: Vec<Arg>) -> In {
    let art = open_art(&l.s);
    In { outpoint: l.op, utxo: utxo(l.value, p2sh(&art), l.daa, l.id), spend: Spend::Signed { art, entry, args } }
}
pub fn pair_in(l: &LPair, entry: &'static str, args: Vec<Arg>) -> In {
    let art = pair_art(&l.s);
    In { outpoint: l.op, utxo: utxo(l.value, p2sh(&art), l.daa, l.id), spend: Spend::Signed { art, entry, args } }
}
pub fn pend_in(l: &LPend, entry: &'static str, args: Vec<Arg>) -> In {
    let art = pend_art(&l.s);
    In { outpoint: l.op, utxo: utxo(l.value, p2sh(&art), l.daa, l.id), spend: Spend::Signed { art, entry, args } }
}
pub fn slot_in(l: &LSlot, entry: &'static str, args: Vec<Arg>) -> In {
    let art = bond_art(&l.s);
    In { outpoint: l.op, utxo: utxo(l.value, p2sh(&art), l.daa, l.id), spend: Spend::Signed { art, entry, args } }
}
pub fn stake_in(l: &LStake, entry: &'static str, args: Vec<Arg>) -> In {
    let art = stake_art(&l.s);
    In { outpoint: l.op, utxo: utxo(l.value, p2sh(&art), l.daa, l.id), spend: Spend::Signed { art, entry, args } }
}
/// A plain P2PK input of `kp` signed SIGHASH_ALL.
pub fn key_in(op: TransactionOutpoint, value: u64, daa: u64, kp: &Keypair) -> In {
    In { outpoint: op, utxo: utxo(value, p2pk_spk(kp), daa, None), spend: Spend::P2pk(*kp) }
}

pub fn pair_spk(p: &OPair) -> ScriptPublicKey {
    p2sh(&pair_art(p))
}
pub fn pend_spk(w: &OPending) -> ScriptPublicKey {
    p2sh(&pend_art(w))
}
pub fn order_spk(o: &OOpen) -> ScriptPublicKey {
    p2sh(&open_art(o))
}
pub fn bond_spk(b: &Bond) -> ScriptPublicKey {
    p2sh(&bond_art(b))
}
pub fn stake_spk(s: &Js) -> ScriptPublicKey {
    p2sh(&stake_art(s))
}
pub fn key_spk(k: &[u8; 32]) -> ScriptPublicKey {
    kpf_protocol::tx::p2pk_spk(k)
}
pub fn p2sh_of_hash(h: &[u8; 32]) -> ScriptPublicKey {
    let mut s = vec![0xaa, 0x20];
    s.extend_from_slice(h);
    s.push(0x87);
    ScriptPublicKey::new(0, s.into())
}

/// blake3 of a script public key as a script reads it (`blake3(tx.outputs[j].scriptPubKey)`: version bytes first).
pub fn spk_hash(spk: &ScriptPublicKey) -> [u8; 32] {
    let mut b = spk.version().to_be_bytes().to_vec();
    b.extend_from_slice(spk.script());
    *blake3::hash(&b).as_bytes()
}

pub fn exec_in(n: u8) -> In {
    key_in(outpoint(n), 100_000_000_000, 0, &exec_kp())
}
pub fn change(v: u64) -> TransactionOutput {
    out(v, p2pk_spk(&exec_kp()), None)
}

/// A built transaction: (tx, entries).
pub type T = (Transaction, Vec<UtxoEntry>);

pub fn verdicts(t: &T) -> Vec<bool> {
    match run(&t.0, &t.1) {
        Ok(v) => v.iter().map(|r| r.is_ok()).collect(),
        Err(_) => vec![false; t.0.inputs.len()],
    }
}
pub fn accepted(t: &T) -> bool {
    verdicts(t).iter().all(|x| *x)
}
/// Every input passes; otherwise the failing input and its error.
pub fn must(what: &str, t: &T) {
    match run(&t.0, &t.1) {
        Ok(v) => {
            for (i, r) in v.iter().enumerate() {
                assert!(r.is_ok(), "{what}: input {i} failed: {r:?}");
            }
        }
        Err(e) => panic!("{what}: covenant context refused the transaction: {e:?}"),
    }
}
/// Input `i` fails (and the bindings are valid, so the refusal is a script's).
pub fn refused(what: &str, t: &T, i: usize) {
    match run(&t.0, &t.1) {
        Ok(v) => assert!(v[i].is_err(), "{what}: input {i} was accepted"),
        Err(e) => panic!("{what}: covenant context refused the transaction (not a script refusal): {e:?}"),
    }
}
/// The transaction is not accepted (any input, or the bindings).
pub fn not_accepted(what: &str, t: &T) {
    assert!(!accepted(t), "{what}: accepted");
}

/// The UTXO output `j` of `t` becomes at DAA `daa`.
pub fn live<S: Clone>(t: &T, j: usize, daa: u64, s: &S) -> Live<S> {
    let o = &t.0.outputs[j];
    Live { op: TransactionOutpoint { transaction_id: t.0.id(), index: j as u32 }, value: o.value, daa, id: o.covenant.map(|c| c.covenant_id), s: s.clone() }
}

// ---------------------------------------------------------------- the actors' UTXOs

pub const SLOT_ID: Hash = Hash::from_bytes([0x51; 32]);
pub const K: u64 = 3_000_000_000_000; // 30,000 KAS bonded

/// The underwriter's slot of value `v`, fresh at ring position 0, at DAA 0.
pub fn slot(v: u64) -> LSlot {
    Live { op: outpoint(100), value: v, daa: 0, id: Some(SLOT_ID), s: Bond::new(x(&uw_kp()), omarket().bond_market(), 0) }
}

/// An order of `kp` on `side`, its UTXO at DAA `daa` under covenant id `id`.
pub fn order(side: i64, kp: &Keypair, id: u8, daa: u64) -> LOrder {
    let o = sample_order(&omarket(), side, x(kp));
    Live { op: outpoint(id), value: 2_000_000_000_000, daa, id: Some(Hash::from_bytes([id; 32])), s: o }
}

/// A free judge stake of `seed` worth `v`, accepted at `daa`.
pub fn stake(seed: u8, v: u64, daa: u64, op: u8) -> LStake {
    Live { op: outpoint(op), value: v, daa, id: None, s: Js::free(x(&keypair(seed)), omarket().judge_market()) }
}

// ---------------------------------------------------------------- T1' open

/// The terms of an open: n units at x, the long's rate and the short's (from their orders), the cover rate, the
/// premium, the expiry, the lock time and the slot's advance.
#[derive(Clone, Copy, Debug)]
pub struct OpenSpec {
    pub n: i64,
    pub x: i64,
    pub cover_rate: i64,
    pub premium: i64,
    pub xe: i64,
    pub lock: u64,
    pub advance: i64,
}

pub fn open_spec() -> OpenSpec {
    OpenSpec { n: 1_000_000_000, x: 1_000_000_000, cover_rate: 600_000_000, premium: 2_000_000, xe: 1_200, lock: OPEN_DAA, advance: 0 }
}

/// The pair an open of these orders writes.
pub fn opened_pair(lo: &LOrder, so: &LOrder, sl: &LSlot, sp: &OpenSpec) -> OPair {
    OPair {
        m: lo.s.m,
        size: sp.n,
        bl: sp.x - lo.s.my_rate,
        bs: sp.x + so.s.my_rate,
        entry: sp.x,
        long_key: lo.s.owner,
        short_key: so.s.owner,
        b: OBinding { uw_id: sl.idb(), uw_key: sl.s.owner, cover_rate: sp.cover_rate, expiry: sp.xe, premium: sp.premium },
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        tag_l: lo.idb(),
        tag_s: so.idb(),
    }
}

/// What an order leaves after a fill of n: its continuation (state, least value) or its refund (None, least value).
pub fn order_after(lo: &LOrder, n: i64, spend: i64) -> (Option<OOpen>, u64) {
    let left = lo.s.size_left - n;
    let v = lo.value - spend as u64;
    if left > 0 {
        (Some(OOpen { size_left: left, ..lo.s.clone() }), v)
    } else {
        (None, v)
    }
}

/// T1': in 0 the long order, 1 the short order, 2 the slot (bind, the underwriter's signature), 3 the executor (P2PK,
/// it authors the pair's genesis); out 0 / 1 the orders' continuations or refunds, 2 the pair, 3 the slot, 4 the
/// premium to the underwriter, 5 change. `tweak` may edit the outputs before the genesis id is computed.
pub fn open_tx(lo: &LOrder, so: &LOrder, sl: &LSlot, sp: &OpenSpec, tweak: &dyn Fn(&mut Vec<TransactionOutput>, &mut OPair)) -> (T, OPair, Bond) {
    let k = kit();
    let mut p = opened_pair(lo, so, sl, sp);
    let cover = q(sp.n, sp.cover_rate, p.m.p.scale);
    let a = BindArgs::new(sp.lock as i64, sp.xe, sp.advance, &[Claim::output(2, cover)]);
    let ctx = kpf_protocol::bond::Ctx { lock: sp.lock as i64, seq: 10, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true };
    let b2 = sl.s.bind(&ctx, &a, sl.value as i64).unwrap_or_else(|_| sl.s.clone());
    let fill_args = |ck: [u8; 32], ct: [u8; 32]| -> Vec<Arg> {
        vec![
            iv(2),
            iv(sp.n),
            iv(sp.x),
            iv(p.bl),
            iv(p.bs),
            bv(&ck),
            bv(&ct),
            iv(2),
            iv(0),
            iv(sp.xe),
            iv(sp.cover_rate),
            iv(sp.premium),
            bv(&k.pair.prefix),
            bv(&k.pair.suffix),
        ]
    };
    let ins = vec![
        order_in(lo, "fill", fill_args(so.s.owner, so.idb())),
        order_in(so, "fill", fill_args(lo.s.owner, lo.idb())),
        slot_in(sl, "bind", bind_args(&a, &uw_kp())),
        exec_in(200),
    ];
    let (lc, lv) = order_after(lo, sp.n, lo.s.fill_spend(sp.n, sp.premium));
    let (sc, sv) = order_after(so, sp.n, so.s.fill_spend(sp.n, sp.premium));
    let order_out = |l: &LOrder, c: Option<OOpen>, v: u64| match c {
        Some(o) => out(v, order_spk(&o), l.id.and_then(|id| bind(if l.s.side == LONG { 0 } else { 1 }, id))),
        None => out(v, key_spk(&l.s.owner), None),
    };
    let prem = q(sp.n, sp.premium, p.m.p.scale) as u64;
    let mut outs = vec![
        order_out(lo, lc, lv),
        order_out(so, sc, sv),
        out(p.floor() as u64, pair_spk(&p), None),
        out(sl.value, bond_spk(&b2), sl.id.and_then(|id| bind(2, id))),
        out(prem.max(2_000_000), key_spk(&sl.s.owner), None),
        change(90_000_000_000),
    ];
    tweak(&mut outs, &mut p);
    // The genesis group: the pair, and every output a tweak marked as its twin.
    let mut group = vec![2usize];
    group.extend((0..outs.len()).filter(|&j| j != 2 && outs[j].covenant.map(|c| c.covenant_id) == Some(TWIN)));
    for &j in &group {
        outs[j].covenant = None;
    }
    let g = genesis_id(outpoint(200), &outs, &group);
    for &j in &group {
        outs[j].covenant = bind(3, g);
    }
    (build_ext(&ins, outs, sp.lock, &[0, 0, 10, 0]), p, b2)
}

/// `q` with the model's rounding (down).
pub fn q(n: i64, r: i64, scale: i64) -> i64 {
    kpf_protocol::payoff::q(n, r, scale, 0)
}

pub fn bind_args(a: &BindArgs, kp: &Keypair) -> Vec<Arg> {
    let mut v: Vec<Arg> = a.arg_bytes().into_iter().map(|b| Arg::V(ArtifactValue::Bytes(b.to_vec()))).collect();
    v.push(Arg::Sig(*kp));
    v
}

/// The standard open (both orders of `order(..)` at DAA 0, the slot of value `K`), accepted; returns the pair, the slot
/// continuation and the transaction.
pub fn opened() -> (LPair, LSlot, T) {
    let (lo, so, sl) = (order(LONG, &long_kp(), 0x71, 0), order(SHORT, &short_kp(), 0x72, 0), slot(K));
    let (t, p, b2) = open_tx(&lo, &so, &sl, &open_spec(), &|_, _| {});
    must("open", &t);
    (live(&t, 2, OPEN_DAA, &p), live(&t, 3, OPEN_DAA, &b2), t)
}

/// The open's lock time and the DAA its outputs land at: claims on X = 1,200 are valid for P in [300, 1,000].
pub const OPEN_DAA: u64 = 300;

// ---------------------------------------------------------------- propose

/// A claim: kind, side, the part n, the vouched price p, its t0, the lock time.
#[derive(Clone, Copy, Debug)]
pub struct ClaimSpec {
    pub kind: i64,
    pub side: i64,
    pub n: i64,
    pub p: i64,
    pub t0: i64,
    pub lock: u64,
}

/// A proposal: in 0 the pair (propose), 1 the vouching input of `voucher` (its P2PK, it funds the bond), 2 the closer's
/// own funding (kind 1) or the executor; out 0 the pending part (ordinal 0 of the id), 1 the remainder (n < s), then
/// change. `closer`: the key whose signature the close carries.
pub fn propose_tx(lp: &LPair, c: &ClaimSpec, voucher: &Keypair, closer: Option<&Keypair>) -> (T, OPending) {
    let k = kit();
    let (w, fb) = lp.s.pending_of(k, c.n, c.kind, c.side, c.p, c.t0).unwrap_or_else(|| {
        // A claim the model refuses still gets a pending state to build (the engine must refuse it too).
        let mut w = sample_pending(&lp.s);
        (w.kind, w.side, w.n, w.quote, w.t0) = (c.kind, c.side, c.n, c.p, c.t0);
        (w, if c.side == LONG { lp.s.bl } else { lp.s.bs })
    });
    let sig = match closer {
        Some(kp) => Arg::Sig(*kp),
        None => bv(&[0u8; 65]),
    };
    let args = vec![iv(c.n), iv(c.kind), iv(c.side), iv(c.p), iv(c.t0), iv(1), sig, iv(fb), iv(0), bv(&k.pending.prefix), bv(&k.pending.suffix)];
    let ins = vec![pair_in(lp, "propose", args), key_in(outpoint(210), 50_000_000_000, 0, voucher), exec_in(211)];
    let id = lp.id.unwrap();
    let mut outs = vec![out(lp.s.pending_value(lp.value, &w) as u64, pend_spk(&w), bind(0, id))];
    if c.n < lp.s.size {
        let rest = lp.s.rest_of(lp.s.size - c.n);
        outs.push(out(lp.s.rest_value(lp.value, c.n) as u64, pair_spk(&rest), bind(0, id)));
    }
    outs.push(change(90_000_000_000));
    (build_ext(&ins, outs, c.lock, &[0, 0, 0]), w)
}

/// Proposes and requires acceptance; the pending part lands at DAA `daa`.
pub fn proposed(lp: &LPair, c: &ClaimSpec, daa: u64) -> LPend {
    let closer = if c.kind == KIND_CLOSE { Some(if c.side == LONG { long_kp() } else { short_kp() }) } else { None };
    let (t, w) = propose_tx(lp, c, &uw_kp(), closer.as_ref());
    must("propose", &t);
    live(&t, 0, daa, &w)
}

// ---------------------------------------------------------------- contest and the judge

pub const DISPUTE_BASE: u64 = 50_000_000;

/// A contest: in 0 the pending part (contest(1, 2)), 1 the opener's stake (open(0, 0, 2), its owner's signature), 2 the
/// executor; out 0 the contested part, 1 the opener's receipt, 2 the dispute, 3 change.
pub fn contest_tx(lw: &LPend, st: &LStake, seed: u8, lock: u64) -> (T, OPending, Js, Js) {
    let id = lw.id.unwrap();
    let p = lw.daa as i64;
    let opener = st.s.owner;
    let w2 = lw.s.contested(p, opener);
    let jm = lw.s.m.judge_market();
    let receipt = Js::receipt(opener, jm, NO, st.value as i64, 0, p);
    let d = Js::opened(jm, spk_hash(&pend_spk(&w2)), p, st.value as i64);
    let ins = vec![pend_in(lw, "contest", vec![b8(1), b8(2)]), stake_in(st, "open", vec![b8(0), b8(0), b8(2), Arg::Sig(keypair(seed))]), exec_in(212)];
    let outs = vec![
        out(lw.value, pend_spk(&w2), bind(0, id)),
        out(st.value, stake_spk(&receipt), bind(0, id)),
        out(DISPUTE_BASE, stake_spk(&d), bind(0, id)),
        change(90_000_000_000),
    ];
    (build_ext(&ins, outs, lock, &[0, 0, 0]), w2, receipt, d)
}

/// The dispute after a deposit of `stakes` on `side` (KpfStake.deposit).
pub fn after_deposit(d: &Js, side: i64, sum: i64, units: i64, dispute_daa: u64) -> Js {
    let mut n = d.clone();
    let (mine, lead) = if side == YES { (d.yes + sum, d.no) } else { (d.no + sum, d.yes) };
    if side == YES {
        n.yes += sum;
        n.yes_u += units;
    } else {
        n.no += sum;
        n.no_u += units;
    }
    let need = (lead / 10_000) * d.m.p.mu_bps + ((lead % 10_000) * d.m.p.mu_bps + 9_999) / 10_000;
    if mine >= need {
        n.side = side;
        n.round += 1;
        n.start = -1;
    } else if d.start == -1 {
        n.start = dispute_daa as i64;
    }
    n
}

/// A deposit: in 0 the dispute (deposit(side, 1, k)), 1..=k the stakes (commit, each its owner's signature), then the
/// executor; out 0 the dispute, 1..=k the receipts, change.
pub fn deposit_tx(ld: &LStake, side: i64, stakes: &[(LStake, u8)], lock: u64) -> (T, Js, Vec<Js>) {
    let id = ld.id.unwrap();
    let d = &ld.s;
    let mut ins = vec![stake_in(ld, "deposit", vec![b8(side), b8(1), b8(stakes.len() as i64)])];
    let mut receipts = vec![];
    let (mut sum, mut units) = (0i64, 0i64);
    for (s, seed) in stakes {
        ins.push(stake_in(s, "commit", vec![b8(side), b8(d.round), b8(d.anchor), Arg::Sig(keypair(*seed))]));
        receipts.push(Js::receipt(s.s.owner, d.m, side, s.value as i64, d.round, d.anchor));
        sum += s.value as i64;
        units += s.value as i64 / UNIT;
    }
    ins.push(exec_in(213));
    let d2 = after_deposit(d, side, sum, units, ld.daa);
    let mut outs = vec![out(ld.value, stake_spk(&d2), bind(0, id))];
    for (r, (s, _)) in receipts.iter().zip(stakes) {
        outs.push(out(s.value, stake_spk(r), bind(0, id)));
    }
    outs.push(change(90_000_000_000));
    let seqs = vec![0u64; ins.len()];
    (build_ext(&ins, outs, lock, &seqs), d2, receipts)
}

/// The resolved pool of a dispute of value `v` (KpfStake.resolve).
pub fn resolved(d: &Js, v: u64, dispute_daa: u64) -> Js {
    let st = if d.start == -1 { dispute_daa as i64 } else { d.start };
    let (wl, ll) = if d.side == YES { (d.yes_u, d.no) } else { (d.no_u, d.yes) };
    Js { status: STATUS_RESOLVED, start: st, w_left: wl, l_left: ll, base: v as i64, ..d.clone() }
}

/// Resolution and delivery: in 0 the dispute (resolve(1)), 1 the contested part (decide(0)), 2 the executor; out 0 the
/// pool, 1 the decided part, 2 change.
pub fn decide_tx(ld: &LStake, lw: &LPend, lock: u64) -> (T, Js, OPending) {
    let id = lw.id.unwrap();
    let pool = resolved(&ld.s, ld.value, ld.daa);
    let w2 = lw.s.decided(ld.s.side == YES, lw.daa as i64);
    let ins = vec![stake_in(ld, "resolve", vec![b8(1)]), pend_in(lw, "decide", vec![b8(0)]), exec_in(214)];
    let outs = vec![out(ld.value, stake_spk(&pool), bind(0, id)), out(lw.value, pend_spk(&w2), bind(1, id)), change(90_000_000_000)];
    (build_ext(&ins, outs, lock, &[0, 0, 0]), pool, w2)
}

/// A settlement of receipts (KpfStake.settle): in 0 the pool, 1..=k the receipts (redeem(0)), the executor; out 0 the
/// pool, then the winners' free stakes at their own indices (a loser's index carries a plain output of the executor).
pub fn settle_tx(lp: &LStake, receipts: &[LStake], lock: u64) -> (T, Js, Vec<u64>) {
    let id = lp.id.unwrap();
    let pool = &lp.s;
    let (wu, l) = if pool.side == YES { (pool.yes_u, pool.no) } else { (pool.no_u, pool.yes) };
    let burn = (l / 10_000) * pool.m.p.burn_bps + ((l % 10_000) * pool.m.p.burn_bps) / 10_000;
    let lnet = l - burn;
    let mut ins = vec![stake_in(lp, "settle", vec![b8(1), b8(receipts.len() as i64)])];
    let (mut wl, mut ll, mut gained, mut paid) = (pool.w_left, pool.l_left, 0i64, 0i64);
    let mut outs = vec![];
    let mut pays = vec![];
    for r in receipts {
        ins.push(stake_in(r, "redeem", vec![b8(0)]));
    }
    ins.push(exec_in(215));
    outs.push(out(0, stake_spk(pool), bind(0, id))); // placeholder, fixed below
    // A loser's index carries the executor's change (a small filler output would cost ~10^12 / value of storage mass).
    let mut change_placed = false;
    for r in receipts {
        if r.s.side == pool.side {
            let u = r.s.amt / UNIT;
            let sh = kpf_protocol::judge::share(u, lnet, wu).unwrap();
            let free = Js::free(r.s.owner, pool.m);
            outs.push(out(r.value + sh as u64, stake_spk(&free), None));
            pays.push(r.value + sh as u64);
            wl -= u;
            paid += sh;
        } else {
            ll -= r.s.amt;
            gained += r.value as i64;
            outs.push(if change_placed { out(100_000_000, p2pk_spk(&exec_kp()), None) } else { change(90_000_000_000) });
            change_placed = true;
            pays.push(0);
        }
    }
    let p2 = Js { w_left: wl, l_left: ll, ..pool.clone() };
    outs[0] = out((lp.value as i64 + gained - paid) as u64, stake_spk(&p2), bind(0, id));
    if !change_placed {
        outs.push(change(90_000_000_000));
    }
    let seqs = vec![0u64; ins.len()];
    (build_ext(&ins, outs, lock, &seqs), p2, pays)
}

/// The close of a settled pool: in 0 the pool (close(1)), 1 the executor; out 0 the executor's base back, 1 the burn.
pub fn pool_close_tx(lp: &LStake) -> (T, u64) {
    let b = lp.value as i64 - lp.s.base;
    let ins = vec![stake_in(lp, "close", vec![b8(1)]), exec_in(216)];
    let mut outs = vec![change(lp.s.base.max(2_000_000) as u64)];
    let burn = if b >= 2_000_000 { b as u64 } else { 0 };
    outs.push(out(burn.max(2_000_000), kpf_protocol::lock::burn_spk(), None));
    (build_ext(&ins, outs, 0, &[0, 0]), burn)
}

// ---------------------------------------------------------------- finalize, cure, renew, lapse

/// A finalize: in 0 the part (finalize), 1 the executor; the outputs of its settlement (`Settlement`), the long or the
/// defender at 0, the forfeited or restored part at 1, then change, and the short or the disputer last (M - 1 - 0).
/// `seq` is the part's committed sequence (challengeDaa while pending).
pub fn finalize_tx(lw: &LPend, p: i64, seq: u64) -> (T, Settlement) {
    let id = lw.id.unwrap();
    let s = lw.s.settlement(lw.value, p);
    let ins = vec![pend_in(lw, "finalize", vec![]), exec_in(217)];
    let mut outs = vec![];
    let k = kit();
    match &s {
        Settlement::Close { long, short } => {
            outs.push(out((*long).max(2_000_000) as u64, key_spk(&lw.s.long_key), None));
            outs.push(change(90_000_000_000));
            outs.push(out((*short).max(2_000_000) as u64, key_spk(&lw.s.short_key), None));
        }
        Settlement::Forfeit { part, value, uw } => {
            outs.push(out(*uw as u64, key_spk(&lw.s.b.uw_key), None));
            outs.push(out(*value as u64, pair_spk(part), bind(0, id)));
            outs.push(change(90_000_000_000));
        }
        Settlement::Restore { part, value, defender, disputer } => {
            outs.push(out((*defender).max(2_000_000) as u64, key_spk(&lw.s.defender()), None));
            outs.push(out(*value as u64, pair_spk(part), bind(0, id)));
            outs.push(change(90_000_000_000));
            if *disputer > 0 {
                outs.push(out(*disputer as u64, key_spk(&lw.s.disputer), None));
            }
        }
    }
    let _ = k;
    (build_ext(&ins, outs, 0, &[seq, 0]), s)
}

/// A cure by the loser: in 0 the part (cure), 1 the loser's funding; out 0 the underwriter's refund, 1 the cured part,
/// 2 change.
pub fn cure_tx(lw: &LPend, loser: &Keypair) -> (T, OPair) {
    let id = lw.id.unwrap();
    let (c, v) = lw.s.cure(lw.value).expect("a cure");
    let ins = vec![pend_in(lw, "cure", vec![Arg::Sig(*loser)]), key_in(outpoint(218), 500_000_000_000, 0, loser)];
    let outs = vec![out(lw.s.refund as u64, key_spk(&lw.s.b.uw_key), None), out(v as u64, pair_spk(&c), bind(0, id)), out(100_000_000_000, p2pk_spk(loser), None)];
    (build_ext(&ins, outs, 0, &[0, 0]), c)
}

/// A renewal: in 0 the pair (renew(1, 0)), 1 the slot (bind, the underwriter's signature, claim 0 = input 0, advancing
/// `advance` steps), 2 the executor; out 0 the renewed pair, 1 the slot, 2 the premium to the underwriter, 3 change.
pub fn renew_tx(lp: &LPair, sl: &LSlot, lock: u64, advance: i64) -> (T, OPair, Bond) {
    let (r, pay) = lp.s.renewed().unwrap_or((lp.s.clone(), 0));
    let cover = lp.s.cover_of(lp.s.size);
    let a = BindArgs::new(lock as i64, lp.s.b.expiry + lp.s.m.bond.term, advance, &[Claim::input(0, cover)]);
    let ctx = kpf_protocol::bond::Ctx { lock: lock as i64, seq: 10, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true };
    let b2 = sl.s.bind(&ctx, &a, sl.value as i64).unwrap_or_else(|_| sl.s.clone());
    let ins = vec![pair_in(lp, "renew", vec![iv(1), iv(0)]), slot_in(sl, "bind", bind_args(&a, &uw_kp())), exec_in(219)];
    let outs = vec![
        out(lp.value - pay as u64, pair_spk(&r), bind(0, lp.id.unwrap())),
        out(sl.value, bond_spk(&b2), bind(1, sl.id.unwrap())),
        out((pay as u64).max(2_000_000), key_spk(&sl.s.owner), None),
        change(90_000_000_000),
    ];
    (build_ext(&ins, outs, lock, &[0, 10, 0]), r, b2)
}

/// A lapse settle: in 0 the pair (lapseSettle, sequence idleDaa), 1 the executor; out 0 the long, 1 change, 2 the short.
pub fn lapse_tx(lp: &LPair, lock: u64, seq: u64) -> T {
    let (l, s) = lp.s.lapse_out(lp.value);
    let ins = vec![pair_in(lp, "lapseSettle", vec![]), exec_in(220)];
    let outs = vec![out(l.max(2_000_000) as u64, key_spk(&lp.s.long_key), None), change(90_000_000_000), out(s.max(2_000_000) as u64, key_spk(&lp.s.short_key), None)];
    build_ext(&ins, outs, lock, &[seq, 0])
}

/// A cooperative close: in 0 the pair (both signatures), 1 the executor; out 0 the long's `long_pay`, 1 the short's
/// rest, 2 change.
pub fn coop_tx(lp: &LPair, long_pay: u64) -> T {
    let ins = vec![pair_in(lp, "coopClose", vec![Arg::Sig(long_kp()), Arg::Sig(short_kp())]), exec_in(221)];
    let outs = vec![out(long_pay, key_spk(&lp.s.long_key), None), out(lp.value - long_pay, key_spk(&lp.s.short_key), None), change(90_000_000_000)];
    build_ext(&ins, outs, 0, &[0, 0])
}

/// An amend of `side` by `kp`: in 0 the pair (sequence `seq`), 1 the executor; out 0 the continuation, 1 change.
pub fn amend_tx(lp: &LPair, side: i64, kp: &Keypair, ask: i64, seq: u64) -> (T, OPair) {
    let mut c = OPair { tag_l: [0; 32], tag_s: [0; 32], ..lp.s.clone() };
    if side == LONG {
        (c.long_ask, c.long_min, c.long_tip) = (ask, 0, 0);
    } else {
        (c.short_bid, c.short_min, c.short_tip) = (ask, 0, 0);
    }
    let ins = vec![pair_in(lp, "amend", vec![Arg::Sig(*kp), iv(side), iv(ask), iv(0), iv(0)]), exec_in(222)];
    let outs = vec![out(lp.value, pair_spk(&c), bind(0, lp.id.unwrap())), change(90_000_000_000)];
    (build_ext(&ins, outs, 0, &[seq, 0]), c)
}

/// A whole transfer of `side` at x to a signing taker `ek` posting bound `nb`: in 0 the pair (sequence `seq`), 1 the
/// taker's funding; out 0 the leaver's payout, 1 the new pair, 2 change.
pub fn transfer_tx(lp: &LPair, side: i64, xp: i64, nb: i64, ek: &Keypair, seq: u64) -> (T, OPair) {
    let p = &lp.s;
    let mut part = p.rest_of(p.size);
    let pay = if side == LONG { kpf_protocol::payoff::owed(p.size, xp - p.bl, p.long_tip, p.m.p.scale) } else { kpf_protocol::payoff::owed(p.size, p.bs - xp, p.short_tip, p.m.p.scale) };
    if side == LONG {
        part.bl = nb;
        part.long_key = x(ek);
        (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
    } else {
        part.bs = nb;
        part.short_key = x(ek);
        (part.short_bid, part.short_min, part.short_tip) = (0, 0, 0);
    }
    let leaver = if side == LONG { p.long_key } else { p.short_key };
    let ins = vec![pair_in(lp, "transfer", vec![iv(side), iv(p.size), iv(xp), iv(nb), bv(&x(ek)), bv(&[0; 32]), iv(0)]), key_in(outpoint(223), 500_000_000_000, 0, ek)];
    let keep = lp.value as i64 - p.range_of(p.size);
    let floor = part.range_of(p.size) + keep;
    let outs = vec![out(pay.max(2_000_000) as u64, key_spk(&leaver), None), out(floor as u64, pair_spk(&part), bind(0, lp.id.unwrap())), out(100_000_000_000, p2pk_spk(ek), None)];
    (build_ext(&ins, outs, 0, &[seq, 0]), part)
}

/// A mutual close of the whole pair at x (both listings crossing): in 0 the pair (sequence `seq`), 1 the executor;
/// out 0 the long, 1 change, 2 the short.
pub fn mutual_tx(lp: &LPair, xp: i64, seq: u64) -> T {
    let p = &lp.s;
    let sc = p.m.p.scale;
    let lpay = kpf_protocol::payoff::owed(p.size, xp - p.bl, p.long_tip, sc);
    let spay = lp.value as i64 - lpay;
    let ins = vec![pair_in(lp, "mutualClose", vec![iv(p.size), iv(xp)]), exec_in(224)];
    let outs = vec![out(lpay as u64, key_spk(&p.long_key), None), change(90_000_000_000), out(spay as u64, key_spk(&p.short_key), None)];
    build_ext(&ins, outs, 0, &[seq, 0])
}

/// An unbond of the slot after advancing `d` steps.
pub fn unbond_tx(sl: &LSlot, d: i64, lock: u64) -> T {
    let ctx = kpf_protocol::bond::Ctx { lock: lock as i64, seq: 10, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true };
    let b2 = sl.s.unbond(&ctx, d, sl.value as i64).unwrap_or_else(|_| Bond { status: 3, ..sl.s.clone() });
    let ins = vec![slot_in(sl, "unbond", vec![b8(d), Arg::Sig(uw_kp())]), exec_in(225)];
    build_ext(&ins, vec![out(sl.value, bond_spk(&b2), bind(0, sl.id.unwrap())), change(90_000_000_000)], lock, &[10, 0])
}

/// An order entering a listed side of a pair (T2 with the fresh bind): in 0 the pair (transfer of `side` for its whole
/// size at x to the order), 1 the order (take), 2 the slot (bind for OUT_REF + 2 at the pair's expiry, unless `bind` is
/// false: then a plain input of the underwriter stands in), 3 the executor; out 0 the leaver's payout, 1 the order's
/// refund or continuation, 2 the part, 3 the slot (or the stand-in's change), 4 change.
pub fn take_tx(lp: &LPair, lo: &LOrder, sl: &LSlot, xp: i64, lock: u64, with_bind: bool) -> (T, OPair) {
    let k = kit();
    let p = &lp.s;
    let o = &lo.s;
    let n = p.size;
    let mut part = p.rest_of(n);
    let (stayer, bound, cl) = if o.side == LONG {
        part.bl = xp - o.my_rate;
        part.long_key = o.owner;
        (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
        part.tag_l = lo.idb();
        (p.short_key, p.bs, (p.short_bid, p.short_min, p.short_tip))
    } else {
        part.bs = xp + o.my_rate;
        part.short_key = o.owner;
        (part.short_bid, part.short_min, part.short_tip) = (0, 0, 0);
        part.tag_s = lo.idb();
        (p.long_key, p.bl, (p.long_ask, p.long_min, p.long_tip))
    };
    let pay = if o.side == LONG { kpf_protocol::payoff::owed(n, xp - p.bl, p.long_tip, p.m.p.scale) } else { kpf_protocol::payoff::owed(n, p.bs - xp, p.short_tip, p.m.p.scale) };
    let leaver = if o.side == LONG { p.long_key } else { p.short_key };
    let nb = if o.side == LONG { part.bl } else { part.bs };
    let cover = p.cover_of(n);
    let a = BindArgs::new(lock as i64, p.b.expiry, 0, &[Claim::output(2, cover)]);
    let ctx = kpf_protocol::bond::Ctx { lock: lock as i64, seq: 10, v_in: sl.value as i64, cov_in: 1, cov_out: 1, signed: true, has_id: true };
    let b2 = sl.s.bind(&ctx, &a, sl.value as i64).unwrap_or_else(|_| sl.s.clone());
    let take_args = vec![
        iv(2),
        iv(n),
        iv(xp),
        iv(p.entry),
        iv(bound),
        bv(&stayer),
        iv(cl.0),
        iv(cl.1),
        iv(cl.2),
        iv(2),
        iv(0),
        iv(p.b.expiry),
        iv(p.b.cover_rate),
        iv(p.b.premium),
        bv(&k.pair.prefix),
        bv(&k.pair.suffix),
    ];
    let third = if with_bind { slot_in(sl, "bind", bind_args(&a, &uw_kp())) } else { key_in(outpoint(226), 1_000_000_000, 0, &uw_kp()) };
    let ins = vec![
        pair_in(lp, "transfer", vec![iv(o.side), iv(n), iv(xp), iv(nb), bv(&o.owner), bv(&lo.idb()), iv(0)]),
        order_in(lo, "take", take_args),
        third,
        exec_in(227),
    ];
    let (oc, ov) = order_after(lo, n, o.take_spend(n));
    let order_out = match oc {
        Some(c) => out(ov, order_spk(&c), bind(1, lo.id.unwrap())),
        None => out(ov, key_spk(&o.owner), None),
    };
    let keep = lp.value as i64 - p.range_of(n);
    let outs = vec![
        out(pay.max(2_000_000) as u64, key_spk(&leaver), None),
        order_out,
        out((part.range_of(n) + keep) as u64, pair_spk(&part), bind(0, lp.id.unwrap())),
        if with_bind { out(sl.value, bond_spk(&b2), bind(2, sl.id.unwrap())) } else { change(900_000_000) },
        change(90_000_000_000),
    ];
    (build_ext(&ins, outs, lock, &[seq_quiet(lp), 0, 10, 0]), part)
}

/// The sequence a party entry commits: the market's quiet time.
pub fn seq_quiet(lp: &LPair) -> u64 {
    lp.s.m.p.quiet_daa as u64
}

/// A pair written by a genesis (not by two orders and a bind): any state, any id, its own author's KAS.
pub fn genesis_pair(p: OPair, value: u64, id: u8, daa: u64) -> LPair {
    Live { op: outpoint(id), value, daa, id: Some(Hash::from_bytes([id; 32])), s: p }
}

/// A slot split into child A (ring `a`, value `va`) and child B (the rest), by its owner: in 0 the slot, 1 the executor;
/// out 0 child A, 1 child B, 2 change.
pub fn split_tx(sl: &LSlot, a: &[i64; 12], va: u64, lock: u64) -> (T, Bond, Bond) {
    let ctx = kpf_protocol::bond::Ctx { lock: lock as i64, seq: 10, v_in: sl.value as i64, cov_in: 1, cov_out: 2, signed: true, has_id: true };
    let vb = sl.value - va;
    let (ba, bb) = sl.s.split(&ctx, a, va as i64, vb as i64).unwrap_or((sl.s.clone(), sl.s.clone()));
    let mut args: Vec<Arg> = a.iter().map(|v| b8(*v)).collect();
    args.push(Arg::Sig(uw_kp()));
    let ins = vec![slot_in(sl, "split", args), exec_in(228)];
    let id = sl.id.unwrap();
    let outs = vec![out(va, bond_spk(&ba), bind(0, id)), out(vb, bond_spk(&bb), bind(0, id)), change(90_000_000_000)];
    (build_ext(&ins, outs, lock, &[10, 0]), ba, bb)
}

/// Close-versus-close of two whole pairs at x: the host's long listing and the donor's short listing are filled; the
/// stayers form P3 under the host's id with the host's binding. in 0 the host (closeHost), 1 the donor (closeDonor), 2
/// the executor; out 0 the host's long, 1 the donor's short, 2 P3, 3 change.
pub fn close_vs_close_tx(host: &LPair, donor: &LPair, xp: i64, e3: i64) -> (T, OPair) {
    let (h, d) = (&host.s, &donor.s);
    let n = h.size;
    let mut p3 = h.rest_of(n);
    p3.bl = d.bl;
    p3.entry = e3;
    p3.long_key = d.long_key;
    (p3.long_ask, p3.long_min, p3.long_tip) = (d.long_ask, d.long_min, d.long_tip);
    p3.tag_l = donor.idb();
    let sc = h.m.p.scale;
    let lpay = kpf_protocol::payoff::owed(n, xp - h.bl, h.long_tip, sc);
    let spay = kpf_protocol::payoff::owed(n, d.bs - xp, d.short_tip, sc) + donor.value as i64 - d.range_of(n);
    let host_args = vec![iv(n), iv(xp), iv(e3), iv(0), iv(d.bl), bv(&d.long_key), iv(d.long_ask), iv(d.long_min), iv(d.long_tip), bv(&donor.idb())];
    let donor_args = vec![iv(n), iv(xp), iv(e3), iv(2), iv(h.bs), bv(&h.short_key), iv(h.short_bid), iv(h.short_min), iv(h.short_tip), iv(h.b.cover_rate), iv(h.b.expiry), iv(h.b.premium)];
    let ins = vec![pair_in(host, "closeHost", host_args), pair_in(donor, "closeDonor", donor_args), exec_in(229)];
    let keep_h = host.value as i64 - h.range_of(n);
    let outs = vec![
        out(lpay as u64, key_spk(&h.long_key), None),
        out(spay as u64, key_spk(&d.short_key), None),
        out((p3.range_of(n) + keep_h) as u64, pair_spk(&p3), bind(0, host.id.unwrap())),
        change(90_000_000_000),
    ];
    (build_ext(&ins, outs, 0, &[seq_quiet(host), seq_quiet(donor), 0]), p3)
}

/// Kinds and sides, for readability.
pub const CLOSE: i64 = KIND_CLOSE;
pub const LIQ: i64 = KIND_LIQ;
pub const L: i64 = LONG;
pub const S: i64 = SHORT;

/// A binding id placeholder for `open_tx`'s tweak: an output carrying it gets the pair's genesis id (a twin).
pub const TWIN: Hash = Hash::from_bytes([0xee; 32]);
pub fn twin_binding() -> Option<CovenantBinding> {
    Some(CovenantBinding { authorizing_input: 3, covenant_id: TWIN })
}
