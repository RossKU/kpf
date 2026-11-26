//! Review T1b: property tests of the thin family (`contracts/KpfSlot.sil`, `contracts/KpfT*.sil`) at the raw engine
//! level. Each test builds a transaction that would break an invariant and asserts the engine refuses it; a test named
//! `t1b_finding_...` asserts an acceptance and documents it (docs/audit/REVIEW-T1b.md).

mod common;

use common::*;
use kaspa_consensus_core::tx::{Transaction, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::cross::{Cross, XLeg, XTerms, SIDE_A};
use kpf_protocol::state::{sm8, LONG, SHORT};
use secp256k1::Keypair;
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

type Tx = (Transaction, Vec<UtxoEntry>);

const XID: Hash = Hash::from_bytes([0x71; 32]);
const YID: Hash = Hash::from_bytes([0x59; 32]);
const LIB: Hash = Hash::from_bytes([0x4c; 32]);
const KAS1: u64 = 100_000_000;
/// What every operation leaves in the copy of the script it used (round 19), and what a copy of 1 KAS goes on holding.
const ORACLE_FEE: u64 = 200_000;
const LIBV: u64 = KAS1 + ORACLE_FEE;
const KAS: i64 = 100_000_000;
const DISPUTE_DAA: i64 = 50;
const BOND_BPS: i64 = 1_000;
const T0: i64 = 6_500;
const M1: [u8; 32] = [0x11; 32];
const M2: [u8; 32] = [0x33; 32];

fn blake2b(data: &[u8]) -> [u8; 32] {
    kaspa_txscript::pay_to_script_hash_script(data).script()[2..34].try_into().expect("P2SH script")
}
fn owner() -> Keypair {
    keypair(31)
}
fn other() -> Keypair {
    keypair(33)
}
fn xkey(kp: &Keypair) -> [u8; 32] {
    kp.x_only_public_key().0.serialize()
}
fn position() -> Cross {
    Cross {
        t: XTerms { term: 8_000, mmr_bps: 500, g_bps: 200, liq_fee_bps: 50, keeper_fee_min: 1_000_000, carrier: 20_000_000, idle_daa: 1_000, grace_daa: 50, late_daa: 100, quiet_daa: 20 },
        legs: [
            XLeg { market: M1, committee: [0x52; 32], scale: 1_000_000, price_max: 100_000_000_000_000, size: 1_000_000_000, dir: LONG, entry: 1_000_000_000 },
            XLeg { market: M2, committee: [0x52; 32], scale: 1_000_000, price_max: 100_000_000_000_000, size: 500_000_000, dir: SHORT, entry: 2_000_000_000 },
        ],
        a_key: xkey(&owner()),
        b_key: xkey(&other()),
        a_margin: 2_000 * KAS,
        b_margin: 3_000 * KAS,
        expiry: 14_000,
    }
}
fn push33(h: &[u8; 32]) -> Vec<u8> {
    [vec![0x20], h.to_vec()].concat()
}
fn push9(v: i64) -> Vec<u8> {
    [vec![0x08], sm8(v).to_vec()].concat()
}
fn moving(c: &Cross) -> Vec<u8> {
    [push33(&c.a_key), push33(&c.b_key), push9(c.a_margin), push9(c.b_margin), push9(c.expiry), push9(c.t.late_daa), push9(DISPUTE_DAA), push9(BOND_BPS), push9(0), push9(0)].concat()
}
fn fixed_hash(c: &Cross) -> [u8; 32] {
    blake2b(&c.state_bytes()[..312])
}
fn position_of(fixed: &[u8], mv: &[u8]) -> [u8; 32] {
    blake2b(&[vec![0x20], fixed.to_vec(), mv.to_vec()].concat())
}
fn hash_of(c: &Cross) -> [u8; 32] {
    position_of(&fixed_hash(c), &moving(c))
}
fn claim_hash(position: &[u8; 32], v: [i64; 6]) -> [u8; 32] {
    blake2b(&[push33(position), v.iter().flat_map(|x| push9(*x)).collect()].concat())
}
fn thin(name: &str) -> SilAbiArtifact {
    compile(&format!("{name}.sil"), &[ArtifactValue::Int(1)])
}
fn tslot_under(lib: Hash, h: [u8; 32]) -> SilAbiArtifact {
    compile("KpfSlot.sil", &[ArtifactValue::Bytes(lib.as_bytes().to_vec()), ArtifactValue::Bytes(h.to_vec())])
}
fn tslot(h: [u8; 32]) -> SilAbiArtifact {
    tslot_under(LIB, h)
}
fn spk(h: [u8; 32]) -> kaspa_consensus_core::tx::ScriptPublicKey {
    p2sh(&tslot(h))
}
fn slot_len() -> i64 {
    bytecode(&tslot([0; 32])).len() as i64
}
fn by(b: &[u8]) -> Arg {
    Arg::V(ArtifactValue::Bytes(b.to_vec()))
}
fn who_of(w: [i64; 3]) -> Vec<u8> {
    w.iter().flat_map(|x| sm8(*x)).collect()
}
fn pay(k: &[u8; 32], v: i64) -> TransactionOutput {
    out(v as u64, kpf_protocol::tx::p2pk_spk(k), None)
}
fn change(v: u64) -> TransactionOutput {
    out(v * KAS1, p2pk_spk(&keypair(60)), None)
}

fn all_ok(t: &Tx) -> bool {
    run(&t.0, &t.1).map(|v| v.iter().all(|r| r.is_ok())).unwrap_or(false)
}
/// The transaction is not valid: an input's script fails or the covenant bindings are refused.
fn refused(t: &Tx) -> bool {
    !all_ok(t)
}
/// Input `i`'s own script fails (the bindings being well formed).
fn fails(t: &Tx, i: usize) -> bool {
    run(&t.0, &t.1).map(|v| v[i].is_err()).unwrap_or(false)
}
fn show(t: &Tx) -> String {
    format!("{:?}", run(&t.0, &t.1))
}


// ---------------------------------------------------------------- raw pushes of a signature script

fn chunks(s: &[u8]) -> Vec<std::ops::Range<usize>> {
    let (mut v, mut i) = (vec![], 0);
    while i < s.len() {
        let n = match s[i] {
            op @ 0x01..=0x4b => 1 + op as usize,
            0x4c => 2 + s[i + 1] as usize,
            0x4d => 3 + u16::from_le_bytes([s[i + 1], s[i + 2]]) as usize,
            0x4e => 5 + u32::from_le_bytes([s[i + 1], s[i + 2], s[i + 3], s[i + 4]]) as usize,
            _ => 1,
        };
        v.push(i..i + n);
        i += n;
    }
    v
}
fn raw_push(d: &[u8]) -> Vec<u8> {
    match d.len() {
        0 => vec![0x00],
        n @ 1..=75 => [vec![n as u8], d.to_vec()].concat(),
        n @ 76..=255 => [vec![0x4c, n as u8], d.to_vec()].concat(),
        n => [vec![0x4d, (n & 0xff) as u8, (n >> 8) as u8], d.to_vec()].concat(),
    }
}
/// The signature script with its `n`th push replaced by a push of `d`, whatever the width the entry declares.
fn repush(script: &[u8], n: usize, d: &[u8]) -> Vec<u8> {
    let c = chunks(script);
    [&script[..c[n].start], &raw_push(d)[..], &script[c[n].end..]].concat()
}

// ---------------------------------------------------------------- the hashes, over bytes of any width

fn p33b(b: &[u8]) -> Vec<u8> {
    [vec![0x20], b.to_vec()].concat()
}
fn h_position(f: &[u8], mv: &[u8]) -> [u8; 32] {
    blake2b(&[p33b(f), mv.to_vec()].concat())
}
fn h_said(position: &[u8; 32], v: &[i64]) -> Vec<u8> {
    [push33(position), v.iter().flat_map(|x| push9(*x)).collect()].concat()
}
fn h_claim(position: &[u8; 32], v: &[i64; 6]) -> [u8; 32] {
    blake2b(&h_said(position, v))
}
fn h_disp(c0: &[u8; 32], dkey: &[u8]) -> [u8; 32] {
    blake2b(&[push33(c0), p33b(dkey)].concat())
}
fn h_liq(position: &[u8; 32], v: &[i64; 7], lkey: &[u8]) -> [u8; 32] {
    blake2b(&[h_said(position, v), p33b(lkey)].concat())
}
fn h_proved(h0: &[u8; 32]) -> [u8; 32] {
    blake2b(&[push9(1), push33(h0)].concat())
}
/// An order's hash: the template's hash and the owner's key at any width, then dir, size, limit, margin, tip,
/// expiry, least, ask (round 11: no rest flag; what a match leaves of an order rests).
fn h_order(tplh: &[u8], owner: &[u8], v: &[i64; 8]) -> [u8; 32] {
    blake2b(&[push9(3), p33b(tplh), p33b(owner), v.iter().flat_map(|x| push9(*x)).collect()].concat())
}
fn order_ints(o: &Order) -> [i64; 8] {
    [o.dir, o.size, o.limit, o.margin, o.tip, o.expiry, o.min_fill, o.min_counter]
}
fn h_exit(position: &[u8; 32], side: i64, since: i64, bond: i64) -> [u8; 32] {
    blake2b(&[push9(2), push33(position), push9(side), push9(since), push9(bond)].concat())
}

use kpf_protocol::thin::{match_orders, Position, LiqObj, LiqState, Order, Rest, TObj, Template};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum K {
    Position,
    Claim,
    Disputed,
    Liq,
    LiqDisputed,
    LiqProved,
    ExitClaim,
    /// Round 4: an order of the position's owner A.
    Order,
    /// Round 8: an anchor of the registry (the way in).
    Anchor,
}
const KINDS: [K; 9] = [K::Position, K::Claim, K::Disputed, K::Liq, K::LiqDisputed, K::LiqProved, K::ExitClaim, K::Order, K::Anchor];

/// One position and everything stated about it; the byte strings are free in width.
#[derive(Clone)]
struct W {
    a: Position,
    f: Vec<u8>,
    mv: Vec<u8>,
    dkey: Vec<u8>,
    lkey: Vec<u8>,
    rkey: Vec<u8>,
    t0: i64,
    since: i64,
    fee: i64,
    add: i64,
    /// The amount a claim states, if not the rule's.
    amt: Option<i64>,
    /// Who signs for B.
    b_signer: Keypair,
    /// An order of A's (round 4), under the same id when it is the scene's object.
    order: Order,
}
fn world() -> W {
    let a = Position { x: position(), dispute_daa: DISPUTE_DAA, bond_bps: BOND_BPS, a_min_counter: 0, b_min_counter: 0 };
    a.check().expect("the sample position");
    W { a, f: a.fixed_hash().to_vec(), mv: a.moving(), dkey: xkey(&keypair(40)).to_vec(), lkey: xkey(&keypair(41)).to_vec(), rkey: xkey(&keypair(42)).to_vec(), t0: T0, since: 6_000, fee: KAS, add: 10 * KAS, amt: None, b_signer: other(), order: Order { tpl: Template::of(&a).hash(), owner: a.x.a_key, dir: LONG, size: 10_000_000, limit: 1_100_000_000, margin: 40 * KAS, tip: KAS / 2, expiry: 6_000, min_fill: 0, min_counter: 40 * KAS } }
}
impl W {
    /// An anchor's state: the hash of its kind, a pushed 7.
    fn anchor_hash() -> [u8; 32] {
        blake2b(&push9(7))
    }
    fn p(&self) -> [i64; 2] {
        [1_100_000_000, 2_100_000_000]
    }
    fn amount(&self) -> i64 {
        self.amt.unwrap_or_else(|| self.a.owed(SIDE_A, self.p()).expect("a close"))
    }
    fn bond(&self) -> i64 {
        self.a.bond()
    }
    fn v0(&self) -> i64 {
        self.a.x.floor()
    }
    fn position(&self) -> [u8; 32] {
        h_position(&self.f, &self.mv)
    }
    fn said(&self) -> [i64; 6] {
        [SIDE_A, self.t0, self.p()[0], self.p()[1], self.amount(), self.bond()]
    }
    fn lsaid(&self) -> [i64; 7] {
        [SIDE_A, self.t0, self.p()[0], self.p()[1], self.fee, self.add, self.bond()]
    }
    fn hash(&self, k: K) -> [u8; 32] {
        let position = self.position();
        let (c, l) = (h_claim(&position, &self.said()), h_liq(&position, &self.lsaid(), &self.lkey));
        match k {
            K::Position => position,
            K::Claim => c,
            K::Disputed => h_disp(&c, &self.dkey),
            K::Liq => l,
            K::LiqDisputed => h_disp(&l, &self.dkey),
            K::LiqProved => h_proved(&l),
            K::ExitClaim => h_exit(&position, SIDE_A, self.since, self.bond()),
            K::Order => h_order(&self.order.tpl, &self.order.owner, &order_ints(&self.order)),
            K::Anchor => W::anchor_hash(),
        }
    }
    fn value(&self, k: K) -> i64 {
        if k == K::Order {
            return self.order.margin + self.order.tip;
        }
        if k == K::Anchor {
            return KAS;
        }
        self.v0()
            + self.bond()
                * match k {
                    K::Position => 0,
                    K::Disputed | K::LiqDisputed => 2,
                    _ => 1,
                }
    }
    /// The moving part after A's margin grew by `delta`, as the scripts write it.
    fn mv_plus(&self, delta: i64) -> Vec<u8> {
        if self.mv.len() < 76 {
            return self.mv.clone();
        }
        let end = self.mv.len().min(138);
        [&self.mv[..67], &sm8(self.a.x.a_margin + delta)[..], &self.mv[75..end]].concat()
    }
}

fn pay_raw(key: &[u8], v: i64) -> TransactionOutput {
    let s: Vec<u8> = [vec![key.len() as u8], key.to_vec(), vec![0xac]].concat();
    out(v as u64, kaspa_consensus_core::tx::ScriptPublicKey::new(0, s.into()), None)
}
fn slot_out(h: [u8; 32], v: i64) -> TransactionOutput {
    out(v as u64, spk(h), bind(0, XID))
}
fn is_change(o: &TransactionOutput) -> bool {
    o.script_public_key == p2pk_spk(&keypair(60))
}
fn funding(n: u8) -> In {
    In { outpoint: outpoint(n), utxo: UtxoEntry::new(1_000 * KAS1, p2pk_spk(&keypair(60)), 0, false, None), spend: Spend::P2pk(keypair(60)) }
}

// ---------------------------------------------------------------- one operation as a transaction

/// An argument (the harness's `Arg` is not `Clone`). `B` is a key of nominally 32 bytes, pushed raw at any width.
#[derive(Clone)]
enum A {
    I(i64),
    B(Vec<u8>),
    S(Keypair),
}
/// An input added after the funding.
#[derive(Clone)]
enum X {
    /// An object spent by `go(l, k)`: its script, value, id.
    Go(SilAbiArtifact, u64, Hash, i64, i64),
    /// The script of another scene, naming what that scene names.
    Lib(Box<Scene>),
    /// Round 19. A copy of a script under the library's id, let go by `release(l, k)`: its script, value, l, k.
    Rel(SilAbiArtifact, u64, i64, i64),
}

/// One operation of the family as a transaction: the object at input 0 (`go(l, k)`), the script at input 1 under the
/// library's id, funding at input 2, then whatever else. Every field is a knob.
#[derive(Clone)]
struct Scene {
    obj: SilAbiArtifact,
    obj_value: u64,
    obj_daa: u64,
    obj_id: Hash,
    go: (i64, i64),
    lib: SilAbiArtifact,
    lib_id: Option<Hash>,
    lib_value: u64,
    entry: &'static str,
    who: Vec<u8>,
    ageb: Vec<u8>,
    slot_len: i64,
    fixed: Vec<u8>,
    mv: Vec<u8>,
    rest: Vec<A>,
    outs: Vec<TransactionOutput>,
    seq: u64,
    extra: Vec<X>,
    /// Pushes of the script's signature script replaced after the build (index, bytes).
    edits: Vec<(usize, Vec<u8>)>,
    /// The widths the entry declares for its fourth and fifth arguments (a position's two parts; an order's
    /// template hash and owner).
    ph: (usize, usize),
    /// The transaction's lock time, and the script input's sequence.
    lock: u64,
    lib_seq: u64,
}

impl Scene {
    fn obj_in(&self, n: u8) -> In {
        let go = vec![Arg::V(ArtifactValue::Int(self.go.0)), Arg::V(ArtifactValue::Int(self.go.1))];
        In { outpoint: outpoint(n), utxo: UtxoEntry::new(self.obj_value, p2sh(&self.obj), self.obj_daa, false, Some(self.obj_id)), spend: Spend::Signed { art: self.obj.clone(), entry: "go", args: go } }
    }
    fn lib_in(&self, n: u8) -> In {
        let mut args = vec![by(&[0; 24]), by(&[0; 8]), Arg::V(ArtifactValue::Int(self.slot_len)), by(&vec![0; self.ph.0]), by(&vec![0; self.ph.1])];
        args.extend(self.rest.iter().map(|a| match a {
            A::I(v) => Arg::V(ArtifactValue::Int(*v)),
            A::B(_) => by(&[0; 32]),
            A::S(k) => Arg::Sig(*k),
        }));
        In { outpoint: outpoint(n), utxo: UtxoEntry::new(self.lib_value, p2sh(&self.lib), 90, false, self.lib_id), spend: Spend::Signed { art: self.lib.clone(), entry: self.entry, args } }
    }
    /// The script's signature script with the byte strings as the scene has them, at whatever width.
    fn patch(&self, script: &[u8]) -> Vec<u8> {
        let mut s = repush(script, 0, &self.who);
        s = repush(&s, 1, &self.ageb);
        s = repush(&s, 3, &self.fixed);
        s = repush(&s, 4, &self.mv);
        for (i, a) in self.rest.iter().enumerate() {
            if let A::B(b) = a {
                s = repush(&s, 5 + i, b);
            }
        }
        for (i, b) in &self.edits {
            s = repush(&s, *i, b);
        }
        s
    }
    fn tx(&self) -> Tx {
        let mut ins = vec![self.obj_in(10), self.lib_in(11), funding(14)];
        let mut seqs = vec![self.seq, self.lib_seq, 0];
        for (n, x) in self.extra.iter().enumerate() {
            match x {
                X::Go(art, value, id, l, k) => {
                    ins.push(In { outpoint: outpoint(20 + n as u8), utxo: UtxoEntry::new(*value, p2sh(art), self.obj_daa, false, Some(*id)), spend: Spend::Signed { art: art.clone(), entry: "go", args: vec![Arg::V(ArtifactValue::Int(*l)), Arg::V(ArtifactValue::Int(*k))] } });
                    seqs.push(self.seq);
                }
                X::Lib(s) => {
                    ins.push(s.lib_in(20 + n as u8));
                    seqs.push(0);
                }
                X::Rel(art, value, l, k) => {
                    ins.push(release_in(20 + n as u8, art, *value, *l, *k));
                    seqs.push(0);
                }
            }
        }
        let mut t = build_ext(&ins, self.outs.clone(), self.lock, &seqs);
        t.0.inputs[1].signature_script = self.patch(&t.0.inputs[1].signature_script);
        for (n, x) in self.extra.iter().enumerate() {
            if let X::Lib(s) = x {
                t.0.inputs[3 + n].signature_script = s.patch(&t.0.inputs[3 + n].signature_script);
            }
        }
        t
    }
    fn with(&self, f: impl FnOnce(&mut Scene)) -> Scene {
        let mut s = self.clone();
        f(&mut s);
        s
    }
}

struct Sc {
    name: &'static str,
    s: Scene,
    kind: K,
    dispute_daa: Option<i64>,
}

#[allow(clippy::too_many_arguments)]
fn scene(w: &W, name: &'static str, script: &str, entry: &'static str, kind: K, dispute_daa: Option<i64>, rest: Vec<A>, o0: TransactionOutput, tail: Vec<TransactionOutput>) -> Sc {
    let lib = thin(script);
    let mut outs = vec![o0, out(LIBV, p2sh(&lib), bind(1, LIB)), change(100), change(100), change(100)];
    outs.extend(tail);
    let age = dispute_daa.unwrap_or(0);
    let s = Scene {
        obj: tslot(w.hash(kind)),
        obj_value: w.value(kind) as u64,
        obj_daa: 6_510,
        obj_id: XID,
        go: (1, 0),
        lib,
        lib_id: Some(LIB),
        lib_value: KAS1,
        entry,
        who: who_of([0, 0, 0]),
        ageb: sm8(age).to_vec(),
        slot_len: slot_len(),
        fixed: w.f.clone(),
        mv: w.mv.clone(),
        rest,
        outs,
        seq: age as u64,
        extra: vec![],
        edits: vec![],
        ph: (32, 138),
        lock: 0,
        lib_seq: 0,
    };
    Sc { name, s, kind, dispute_daa }
}

/// Every entry of the family that reads one object, on the position of `w`: a transaction the engine accepts.
fn scenes(w: &W) -> Vec<Sc> {
    let (a, b) = (w.a.x.a_key, w.a.x.b_key);
    let (v0, bond, amount, position) = (w.v0(), w.bond(), w.amount(), w.position());
    let half = bond / 2;
    let i = |v: &[i64]| -> Vec<A> { v.iter().map(|x| A::I(*x)).collect() };
    let six = i(&w.said());
    let seven = i(&w.lsaid());
    let (dk, lk, rk) = (A::B(w.dkey.clone()), A::B(w.lkey.clone()), A::B(w.rkey.clone()));
    let cat = |x: &[A], y: Vec<A>| -> Vec<A> { [x.to_vec(), y].concat() };
    let win = Some(w.a.dispute_daa);
    let delta = 100 * KAS;
    let cured = h_position(&w.f, &w.mv_plus(w.add));
    let mut v = vec![
        scene(w, "add", "KpfTAdd", "add", K::Position, None, vec![A::I(SIDE_A), A::I(delta), A::S(owner())], slot_out(h_position(&w.f, &w.mv_plus(delta)), v0 + delta), vec![]),
        scene(w, "both", "KpfTBoth", "both", K::Position, None, vec![A::S(owner()), A::S(w.b_signer)], out(v0 as u64, p2pk_spk(&keypair(61)), None), vec![]),
        scene(w, "close", "KpfTClose", "close", K::Position, None, cat(&six[..5], vec![A::S(owner())]), slot_out(w.hash(K::Claim), v0 + bond), vec![]),
        scene(w, "settle", "KpfTSettle", "settle", K::Claim, win, six.clone(), pay(&a, amount + bond), vec![pay(&b, v0 - amount)]),
        scene(w, "late", "KpfTLate", "late", K::Claim, None, cat(&six, vec![rk.clone()]), slot_out(position, v0), vec![pay(&b, half), pay_raw(&w.rkey, bond - half)]),
        scene(w, "dispute", "KpfTDispute", "dispute", K::Claim, None, cat(&six, vec![dk.clone()]), slot_out(w.hash(K::Disputed), v0 + 2 * bond), vec![]),
        scene(w, "lapse", "KpfTDispute", "lapse", K::Disputed, win, cat(&six, vec![dk.clone()]), slot_out(position, v0), vec![pay(&a, bond), pay_raw(&w.dkey, bond)]),
        scene(w, "liquidate", "KpfTLiq", "liquidate", K::Position, None, cat(&seven[..6], vec![lk.clone()]), slot_out(w.hash(K::Liq), v0 + bond), vec![]),
        scene(w, "liq-late", "KpfTLiq", "late", K::Liq, None, cat(&seven, vec![lk.clone(), rk.clone()]), slot_out(position, v0), vec![pay_raw(&w.rkey, bond - half)]),
        scene(w, "liq-settle", "KpfTLiqSettle", "settle", K::Liq, win, cat(&seven, vec![lk.clone(), A::I(0)]), pay_raw(&w.lkey, w.fee + bond), vec![pay(&b, v0 - w.fee)]),
        scene(w, "liq-settle-proved", "KpfTLiqSettle", "settle", K::LiqProved, win, cat(&seven, vec![lk.clone(), A::I(1)]), pay_raw(&w.lkey, w.fee + bond), vec![pay(&b, v0 - w.fee)]),
        // A claimed liquidation is cured once it is half a dispute period old; a proved one at any age; a disputed one never.
        scene(w, "cure", "KpfTCure", "cure", K::Liq, Some(w.a.dispute_daa / 2), cat(&seven, vec![lk.clone(), A::I(0), A::S(owner())]), slot_out(cured, v0 + w.add), vec![pay_raw(&w.lkey, bond)]),
        scene(w, "cure-proved", "KpfTCure", "cure", K::LiqProved, None, cat(&seven, vec![lk.clone(), A::I(2), A::S(owner())]), slot_out(cured, v0 + w.add), vec![pay_raw(&w.lkey, bond)]),
        scene(w, "liq-dispute", "KpfTLiqDispute", "dispute", K::Liq, None, cat(&seven, vec![lk.clone(), dk.clone()]), slot_out(w.hash(K::LiqDisputed), v0 + 2 * bond), vec![]),
        scene(w, "liq-lapse", "KpfTLiqDispute", "lapse", K::LiqDisputed, win, cat(&seven, vec![lk.clone(), dk.clone()]), slot_out(position, v0), vec![pay_raw(&w.lkey, bond), pay_raw(&w.dkey, bond)]),
        scene(w, "exit", "KpfTExit", "exit", K::Position, None, vec![A::I(SIDE_A), A::I(w.since), A::S(owner())], slot_out(w.hash(K::ExitClaim), v0 + bond), vec![]),
        scene(w, "quiet-early", "KpfTExit", "early", K::ExitClaim, None, vec![A::I(SIDE_A), A::I(w.since), A::I(bond), rk.clone()], slot_out(position, v0), vec![pay(&b, half), pay_raw(&w.rkey, bond - half)]),
        scene(w, "quiet-end", "KpfTExitEnd", "settle", K::ExitClaim, Some(w.a.exit_daa()), vec![A::I(SIDE_A), A::I(w.since), A::I(bond)], pay(&a, w.a.x.a_margin + bond), vec![pay(&b, v0 - w.a.x.a_margin)]),
    ];
    // Round 4: an order of A's taken back by its owner, and returned by anyone from its expiry on. The fourth and
    // fifth arguments are the template's hash and the owner's key.
    let o = &w.order;
    let ov = o.margin + o.tip;
    let oi = i(&order_ints(o));
    v.push(scene(w, "order-cancel", "KpfTCancel", "cancel", K::Order, None, cat(&oi, vec![A::S(owner())]), pay(&o.owner, ov), vec![]));
    v.push(scene(w, "order-expire", "KpfTCancel", "expire", K::Order, None, oi.clone(), pay(&o.owner, ov), vec![]));
    // Round 8: the way into the registry. An order enters by its owner's signature, a position by its two parties';
    // the anchor is written again after what enters (output 2 here).
    v.push(scene(w, "registry-place", "KpfTPlace", "place", K::Anchor, None, cat(&oi, vec![A::S(owner())]), slot_out(w.hash(K::Order), ov), vec![]));
    v.push(scene(w, "registry-open", "KpfTOpen", "open", K::Anchor, None, vec![A::S(owner()), A::S(w.b_signer)], slot_out(position, w.a.pot()), vec![]));
    for sc in v.iter_mut().filter(|s| s.kind == K::Anchor) {
        sc.s.outs[2] = out(KAS1, spk(W::anchor_hash()), bind(0, XID));
    }
    // Round 11: the same position the other way round (KpfTTurn), by anyone once it is 100 DAA old. The entry is
    // shown the fixed part whole (312 bytes).
    v.push(scene(w, "turn-users", "KpfTTurn", "turn", K::Position, Some(100), vec![A::I(1)], slot_out(kpf_protocol::thin::turned_users(&w.a).hash(), v0), vec![]));
    v.push(scene(w, "turn-legs", "KpfTTurn", "turn", K::Position, Some(100), vec![A::I(2)], slot_out(kpf_protocol::thin::turned_legs(&w.a).map(|a| a.hash()).unwrap_or(w.a.hash()), v0), vec![]));
    for sc in v.iter_mut().filter(|s| s.name.starts_with("turn-")) {
        sc.s.fixed = w.a.fixed();
        sc.s.ph = (312, 138);
    }
    for sc in v.iter_mut().filter(|s| s.kind == K::Order || s.name == "registry-place") {
        sc.s.fixed = o.tpl.to_vec();
        sc.s.mv = o.owner.to_vec();
        sc.s.ph = (32, 32);
        if sc.name == "order-expire" {
            sc.s.lock = o.expiry as u64;
        }
    }
    for sc in v.iter_mut() {
        match sc.name {
            // A claim that landed after lateDaa; an exit claim that landed before its own `since`.
            "late" | "liq-late" => sc.s.obj_daa = (T0 + w.a.x.t.late_daa + 1) as u64,
            "quiet-early" => sc.s.obj_daa = (w.since - 1_000) as u64,
            _ => {}
        }
        // Round 3 (F2b): the half of a false or late liquidator's bond that went to the side named stays in the
        // script's own copy.
        // (Round 23: a lapse returns both bonds; the half stays only where a liquidation landed late.)
        if sc.name == "liq-late" {
            sc.s.outs[1].value += half as u64;
        }
    }
    v
}
fn the(v: &[Sc], name: &str) -> Scene {
    v.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("no scene {name}")).s.clone()
}

#[test]
fn t1b_a0_every_scene_is_accepted_and_the_model_agrees() {
    let w = world();
    // The hashes written out here are the model's (`kpf_protocol::thin`).
    let c = w.a.true_claim(SIDE_A, T0, w.p()).unwrap();
    let l = w.a.liq_stated(SIDE_A, T0, w.p(), w.fee, w.add, w.lkey.clone().try_into().unwrap()).unwrap();
    let dk: [u8; 32] = w.dkey.clone().try_into().unwrap();
    assert_eq!(w.hash(K::Position), w.a.hash());
    assert_eq!(w.hash(K::Position), hash_of(&position()));
    assert_eq!(w.hash(K::Claim), c.hash());
    assert_eq!(w.hash(K::Claim), claim_hash(&w.a.hash(), w.said()));
    assert_eq!(w.hash(K::Disputed), c.disputed(dk).hash());
    assert_eq!(w.hash(K::Liq), l.hash());
    assert_eq!(w.hash(K::LiqDisputed), LiqObj { liq: l, state: LiqState::Disputed(dk) }.hash());
    assert_eq!(w.hash(K::LiqProved), LiqObj { liq: l, state: LiqState::Proved }.hash());
    assert_eq!(w.hash(K::ExitClaim), w.a.exit(SIDE_A, w.since).unwrap().hash());
    assert_eq!(TObj::Claim(c).script(&LIB.as_bytes()), bytecode(&tslot(c.hash())));
    let mut bad = vec![];
    let all = scenes(&w);
    assert_eq!(all.len(), 24);
    assert_eq!(w.hash(K::Order), w.order.hash());
    for sc in all {
        let t = sc.s.tx();
        if !all_ok(&t) {
            bad.push(format!("{}: {}", sc.name, show(&t)));
        }
        // What the entry moves (D): the object's value before, what is written at output 0, what is paid at the end.
        let tail: Vec<u64> = sc.s.outs[5..].iter().filter(|o| !is_change(o)).map(|o| o.value).collect();
        println!("T1b scene {:18} {:?} holding {:>13} -> output 0 {:>13} ({}), paid at the end {:?}", sc.name, sc.kind, sc.s.obj_value, sc.s.outs[0].value, if sc.s.outs[0].covenant.is_some() { "the object" } else { "a party" }, tail);
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

// ---------------------------------------------------------------- the batteries

/// Collects the transactions the engine accepted and should not have.
struct Bad {
    v: Vec<String>,
    n: usize,
}
impl Bad {
    fn new() -> Bad {
        Bad { v: vec![], n: 0 }
    }
    /// `input` Some(i): input i's own script must fail. None: the transaction must not be valid.
    fn must_fail(&mut self, name: &str, what: &str, s: &Scene, input: Option<usize>) {
        let t = s.tx();
        self.n += 1;
        let ok = match input {
            Some(i) => fails(&t, i),
            None => refused(&t),
        };
        if !ok {
            self.v.push(format!("{name}: {what}: {}", show(&t)));
        }
    }
    fn must_pass(&mut self, name: &str, what: &str, s: &Scene) {
        let t = s.tx();
        self.n += 1;
        if !all_ok(&t) {
            self.v.push(format!("{name}: {what} (should be accepted): {}", show(&t)));
        }
    }
    fn done(self, label: &str) {
        println!("T1b {label}: {} transactions tried, {} not as expected", self.n, self.v.len());
        assert!(self.v.is_empty(), "{label}: {} not as expected:\n{}", self.v.len(), self.v.join("\n"));
    }
}

/// The two scripts that collect the oracle fees (round 19): they take none and have no `release`.
const COLLECTORS: [&str; 2] = ["KpfTSweep", "KpfTPay"];
/// What `release(l, k)` asks, in every script that has it: the body, line for line.
const RELEASE: [&str; 6] = [
    "int self = this.activeInputIndex;",
    "require(k >= 0);",
    "require(k <= 2);",
    "require(OpInputCovenantId(l) == OpInputCovenantId(self));",
    "require(OpTxInputScriptSigSubstr(l, 0, 1) == byte[](0x18));",
    "require(OpTxInputScriptSigSubstr(l, 1 + 8 * k, 9 + 8 * k) == byte[](self as byte[8]));",
];
const SCRIPTS: [&str; 61] = ["KpfTAdd", "KpfTBoth", "KpfTClose", "KpfTSettle", "KpfTLate", "KpfTDispute", "KpfTProve", "KpfTLiq", "KpfTLiqSettle", "KpfTCure", "KpfTLiqDispute", "KpfTLiqProve", "KpfTExit", "KpfTExitEnd", "KpfTExitRefute", "KpfTMatch", "KpfTCancel", "KpfTMerge", "KpfTNovate", "KpfTPlace", "KpfTOpen", "KpfTJoin", "KpfTTurn", "KpfTFold", "KpfTLeave", "KpfTReplace", "KpfTNote", "KpfTCite", "KpfTChange", "KpfTChangeSettle", "KpfTChangeDispute", "KpfTChangeLate", "KpfTTake", "KpfTKind", "KpfTPoint", "KpfTStepRow", "KpfTStepPrice", "KpfTStepEnd", "KpfTStepLiq", "KpfTShardOpen", "KpfTOpenC", "KpfTAddC", "KpfTAcctEnd", "KpfTShardEnd", "KpfTCallEnd", "KpfTCStep", "KpfTCallPay", "KpfTCallEnter", "KpfTShardPrice", "KpfTDrawNote", "KpfTLeft", "KpfTCRow", "KpfTCEnd", "KpfTLeftAll", "KpfTLeftVoid", "KpfTEnterS", "KpfTCEnter", "KpfTEnterOff", "KpfTDrawEntry", "KpfTSweep", "KpfTPay"];
/// Round 30. The two services of the library are spent not to move an object named by `who` but to write or cite a
/// price note: their first argument is not the 24 bytes of the places, and their "going on" is their own (a note
/// writer writes two outputs under the id, a citation ties the counts). The who-based lints (A3's entry shape, R19's
/// places) are of the object operations only.
const SERVICES: [&str; 2] = ["KpfTNote", "KpfTCite"];
/// Round 30. The scripts whose answer keeps half of a bond lost in their own copy: their self-output asks for more
/// than the oracle fee. The note writer counts its outputs its own way; every other script ties the counts.
const KEEP_HALF: [&str; 6] = ["KpfTTake", "KpfTKind", "KpfTStepRow", "KpfTStepPrice", "KpfTStepEnd", "KpfTStepLiq"];
/// Round 30. The twenty-eight object operations of rounds 1 to 27 (the collectors among them). The source lints of
/// those rounds (`t1b_r19_...`, `t1b_b1_...`, `t1b_j5_...`) model this shape — one object named by `who`, counted in
/// and out, a payout at `len - 1 - 2j`. The thirteen scripts of the thin family added this round are of two new
/// shapes (a service that writes or cites a note; an answer that keeps half a bond and may read a citation): `A3`
/// now covers all forty-one, and the new scripts' own lints are `t1b_holds_r30_...` and the F51 to F57 tests through
/// the builders. The other three lints stay on the shape they were written for.
const OBJ_OPS: [&str; 28] = ["KpfTAdd", "KpfTBoth", "KpfTClose", "KpfTSettle", "KpfTLate", "KpfTDispute", "KpfTProve", "KpfTLiq", "KpfTLiqSettle", "KpfTCure", "KpfTLiqDispute", "KpfTLiqProve", "KpfTExit", "KpfTExitEnd", "KpfTExitRefute", "KpfTMatch", "KpfTCancel", "KpfTMerge", "KpfTNovate", "KpfTPlace", "KpfTOpen", "KpfTJoin", "KpfTTurn", "KpfTFold", "KpfTLeave", "KpfTReplace", "KpfTSweep", "KpfTPay"];

// ---------------------------------------------------------------- A1: one script, one object

#[test]
fn t1b_a1_a_script_that_reads_one_object_moves_one_object() {
    let w = world();
    let mut bad = Bad::new();
    for sc in scenes(&w) {
        let s = &sc.s;
        // A second object of the library spent beside the first (input 3), naming the script at place k: a twin of
        // the object (the same state under another id) and an object of a state nobody knows.
        let cases: [([i64; 3], i64, Option<usize>); 10] = [
            ([0, 3, 3], 1, Some(1)),
            ([0, 3, 3], 2, Some(1)),
            ([0, 3, 0], 1, Some(1)),
            ([0, 0, 3], 2, Some(1)),
            ([3, 0, 0], 0, Some(1)),
            ([3, 0, 3], 2, Some(1)),
            ([0, 0, 0], 0, Some(3)),
            ([0, 0, 0], 1, Some(3)),
            ([0, 0, 0], 2, Some(3)),
            ([3, 3, 3], 0, None),
        ];
        for (who, k, input) in cases {
            for (label, x) in [("a twin", X::Go(s.obj.clone(), s.obj_value, YID, 1, k)), ("an unknown object", X::Go(tslot([0x77; 32]), 500 * KAS1, YID, 1, k))] {
                bad.must_fail(sc.name, &format!("{label} at input 3 naming place {k}, who {who:?}"), &s.with(|s| { s.who = who_of(who); s.extra = vec![x]; }), input);
            }
        }
        // No second object: a place that names another input than the object.
        for who in [[0, 0, 2], [0, 2, 0], [0, 1, 1], [0, 0, 1], [0, 9, 9], [0, -1, 0]] {
            bad.must_fail(sc.name, &format!("who {who:?}"), &s.with(|s| s.who = who_of(who)), Some(1));
        }
    }
    bad.done("A1 one script, one object");
}

#[test]
fn t1b_a1_two_scripts_naming_one_object_must_both_hold() {
    let w = world();
    let v = scenes(&w);
    let mut bad = Bad::new();
    for s1 in &v {
        for s2 in &v {
            if s1.kind != s2.kind || s1.name == s2.name || bytecode(&s1.s.lib) == bytecode(&s2.s.lib) {
                continue;
            }
            // s1's transaction, with s2's script spent beside it (input 3, going on at output 3) and naming the same
            // object with the arguments that satisfy it alone.
            let t = s1.s.with(|s| {
                s.extra = vec![X::Lib(Box::new(s2.s.clone()))];
                s.outs[3] = out(s2.s.outs[1].value, p2sh(&s2.s.lib), bind(3, LIB));
            });
            // (Round 8: KpfTBoth too holds the outputs of the object's id, to none.)
            bad.must_fail(s1.name, &format!("with {} beside it", s2.name), &t, Some(3));
        }
    }
    bad.done("A1 two scripts, one object");
}

// ---------------------------------------------------------------- A2: who

#[test]
fn t1b_a2_who_at_the_script_itself_a_funding_input_or_no_input() {
    let w = world();
    let mut bad = Bad::new();
    for sc in scenes(&w) {
        for who in [[1, 1, 1], [2, 2, 2], [3, 3, 3], [9, 9, 9], [-1, -1, -1], [1 << 40, 1 << 40, 1 << 40]] {
            bad.must_fail(sc.name, &format!("who {who:?}"), &sc.s.with(|s| s.who = who_of(who)), Some(1));
        }
        // The funding input's signature script is a push of 65 bytes: every length a script could ask of it.
        for len in [35, 40, 65, 66] {
            bad.must_fail(sc.name, &format!("the funding input as an object of {len} bytes"), &sc.s.with(|s| { s.who = who_of([2, 2, 2]); s.slot_len = len; }), Some(1));
        }
        // The object's own length a byte off.
        for d in [-1, 1] {
            bad.must_fail(sc.name, "the object's length a byte off", &sc.s.with(|s| s.slot_len += d), Some(1));
        }
    }
    bad.done("A2 who");
}

// ---------------------------------------------------------------- A3: the library's scripts go on

/// A3 over the sources, round 19. Every entry of every script that is an operation ends by `goesOn`, and `goesOn` is
/// four conditions: the same script at the same index, the library's id, at least what the copy held and `ORACLE_FEE` more
/// (the two scripts that collect: at least what it held), and as many outputs under the id as inputs. `release` is
/// the one entry that does not go on, by design: it is the same six lines in each of the 25 scripts, reads no output
/// and no value, and leaves what becomes of the copy to the input that names it (what that input may be is
/// `t1b_r19_...` below). The two scripts that collect have no `release`.
#[test]
fn t1b_a3_every_entry_of_every_script_holds_the_script_to_go_on() {
    // Round 30: the lint now covers all 41 scripts. The "going on" of every entry is read from the union of its own
    // body and the two functions that may hold it (`goesOn`, the change family; `ends`, the answers of a point and
    // of a take, which keep half a bond). The four conditions it must contain:
    //  - the copy's own script goes on, under the library's id, at its index;
    //  - its value is at least what it held plus the oracle fee (a copy that keeps half a bond asks for more);
    //  - the library's input and output counts are tied, EXCEPT the note writer, which writes two outputs under the
    //    id (the copy and the note) for its one input.
    let heads = ["int self = this.activeInputIndex;", "byte[32] lib = OpInputCovenantId(self);"];
    let (mut entries, mut releases) = (0, 0);
    for name in SCRIPTS {
        let collector = COLLECTORS.contains(&name);
        let service = SERVICES.contains(&name);
        // The two collectors name the fee pool and the copy by `who` as an operation names its object; only the two
        // services (a note written or cited) are not named by the places.
        let who_based = !service;
        let src = contract_source(&format!("{name}.sil"));
        let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        assert_eq!(code.contains("int constant ORACLE_FEE = 200000;"), !collector, "{name}: the oracle fees are a constant of every script but the two that collect");
        // ORACLE_FEE is read once for the constant and once where the copy goes on; KpfTLiq and KpfTKind read it a
        // third time (a liquidation beside a vouch, a kind that keeps half a bond).
        // (Round 39: KpfTPoint too, where the lapse of a point of an entry keeps half of the claimers' bond in the
        // copy: the copy holds at least what it held, the fee and that half.)
        // (Round 50: and a fourth time, where a dispute that landed after the last time is dismissed: the copy holds
        // the fee and what is left of the disputer's bond after the two quarters.)
        assert_eq!(code.matches("ORACLE_FEE").count(), if collector { 0 } else if name == "KpfTPoint" { 4 } else if ["KpfTLiq", "KpfTKind"].contains(&name) { 3 } else { 2 }, "{name}: ORACLE_FEE is read where the copy goes on and nowhere else");
        // Where the four conditions live: the entry body, plus any `goesOn`/`ends` the entry calls.
        let func = |n: &str| -> Option<String> { code.split(&format!("function {n}()")).nth(1).map(|b| b.split("\n    }").next().unwrap().to_string()) };
        let ends_fn = code.split("function ends(").nth(1).map(|b| b.split("\n    }").next().unwrap().to_string());
        let goes_on = func("goesOn");
        if let Some(b) = &goes_on {
            assert_eq!(b.matches("require(").count(), 4, "{name}: goesOn asks four things");
        }
        // The self-value line: with the fee, or with the fee and the half bond kept (the note writer and the
        // citation ask the fee only).
        let self_value = |blob: &str| -> bool {
            blob.contains("require(tx.outputs[self].value >= tx.inputs[self].value + ORACLE_FEE")
                || (collector && blob.contains("require(tx.outputs[self].value >= tx.inputs[self].value);"))
        };
        // The count line: the note writer writes two outputs for its one input; every other ties the counts.
        let counts = |blob: &str| -> bool {
            if name == "KpfTNote" {
                blob.contains("require(OpCovInputCount(lib) == 1);") && blob.contains("require(OpCovOutputCount(lib) == 2);")
            } else {
                blob.contains("require(OpCovOutputCount(lib) == OpCovInputCount(lib));")
            }
        };
        let parts: Vec<&str> = code.split("\n    entry ").collect();
        assert!(parts.len() >= 2, "{name}: no entry");
        let mut has_release = false;
        for e in &parts[1..] {
            let body = e.split("\n    }").next().unwrap();
            if body.starts_with("release(") {
                assert!(body.starts_with("release(int l, int k) {"), "{name}: release takes the input that names the copy and the place");
                let lines: Vec<&str> = body.lines().skip(1).map(str::trim).filter(|l| !l.is_empty()).collect();
                assert_eq!(lines, RELEASE, "{name}: release is not the six lines");
                has_release = true;
                releases += 1;
                continue;
            }
            entries += 1;
            // The going-on lives in the entry, or in the `goesOn`/`ends` it uses. The entry never returns early, and
            // the going-on is the last thing it does (a call to `goesOn`/`ends`, or the conditions inline).
            let blob = [body, goes_on.as_deref().unwrap_or(""), ends_fn.as_deref().unwrap_or("")].join("\n");
            let closed = body.trim_end().ends_with(';')
                && (body.trim_end().ends_with("goesOn();") || body.trim_end().ends_with(");"));
            let goes = heads.iter().all(|r| blob.contains(r))
                && blob.contains("require(tx.outputs[self].scriptPubKey == tx.inputs[self].scriptPubKey);")
                && blob.contains("require(OpOutputCovenantId(self) == lib);")
                && self_value(&blob)
                && counts(&blob);
            assert!(goes && closed, "{name}: an entry that does not end by going on: {}", &body[..body.len().min(80)]);
            assert!(!body.contains("return"), "{name}: an entry that returns early");
            assert!(body.contains("require(version == 1);"), "{name}");
            // An object operation names the places; a service (a note written or cited) does not.
            assert_eq!(body.starts_with(&format!("{}(byte[24] who, ", body.split('(').next().unwrap())), who_based, "{name}: who-based iff an object operation");
        }
        assert_eq!(has_release, !collector, "{name}: every script that takes oracle fees lets go of them, and the two that collect have no way out");
        let _ = (who_based, KEEP_HALF);
    }
    println!("T1b A3: {entries} entries of {} scripts, each ending by going on (the copy kept, the fee held, the counts tied); {releases} entries `release`, the same six lines, which do not", SCRIPTS.len());
    let mut sorted = SCRIPTS.to_vec();
    sorted.sort();
    let mut pinned = kpf_protocol::thin_artifacts::SCRIPTS.to_vec();
    pinned.sort();
    assert_eq!(sorted, pinned, "every script of the library");
    assert_eq!(COLLECTORS, kpf_protocol::thin_artifacts::COLLECTORS);
    assert_eq!((entries, releases), (71, 59));
}

/// Round 19, the places over the sources. `release` (and `KpfSlot.go`, `KpfFeePool.go`) asks only to be named at a
/// place by an input under the library's id, so every place of every entry must be answered for. Each of the three
/// places of `who` is, in every entry: read as an object (`slotOf` and the hash of a state), read as the fee pool
/// (`fee pool`), read as an epoch table (`priced`, `seenTick`: the pinned template under the committee's id), the copy a sweep
/// answers for, or held equal to a place that is read. A place that is none of these would be spent under no rule.
#[test]
fn t1b_r19_every_place_of_every_entry_is_read_or_held_to_one_that_is() {
    let mut rows = 0;
    for name in OBJ_OPS {
        // Round 30: this lint models the object operations; the two services (a note written, a citation) and the
        // citation-reading answers of the thin family are read in `t1b_holds_r30_...` and the F-tests, not here.
        if SERVICES.contains(&name) {
            continue;
        }
        let src = contract_source(&format!("{name}.sil"));
        let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for e in code.split("\n    entry ").skip(1) {
            let body = e.split("\n    }").next().unwrap();
            let entry = body.split('(').next().unwrap();
            if entry == "release" {
                continue;
            }
            let cut = |k: usize| format!("who.slice({}, {})", 8 * k, 8 * k + 8);
            let mut how = vec![];
            for k in 0..3 {
                // A variable that holds the place, and what is done with it.
                let var = body.lines().find_map(|l| l.trim().strip_prefix("int ").filter(|l| l.ends_with(&format!("= int(byte[8]({}));", cut(k)))).map(|l| l.split(' ').next().unwrap().to_string()));
                let read = match &var {
                    Some(v) if body.contains(&format!("slotOf({v}, slotLen)")) => Some("an object"),
                    // (The proofs and the take's answer read their object in the entry itself: the same lines as `slotOf`.)
                    Some(v) if body.contains(&format!("byte[] rs = OpTxInputScriptSigSubstr({v}, OpTxInputScriptSigLen({v}) - slotLen, OpTxInputScriptSigLen({v}));")) && body.contains(&format!("require(tx.inputs[{v}].scriptPubKey == byte[](new ScriptPubKeyP2SHFromRedeemScript(rs)));")) && (body.contains(&format!("require({v} != self);")) || body.contains(&format!("require({v} != this.activeInputIndex);"))) => Some("an object"),
                    Some(v) if body.contains(&format!("feePool({v}, slotLen, rid, floor);")) => Some("the fee pool"),
                    // (Round 30: a citation copy of KpfTCite, held to its script and read for its price. KpfTStepPrice
                    // binds the place to `u`; KpfTTake reads it inline through `cited`.)
                    Some(v) if body.contains(&format!("require(tx.inputs[{v}].scriptPubKey == byte[](CITE_SPK));")) => Some("a citation"),
                    Some(v) if name == "KpfTSweep" && v == "c" => {
                        for r in ["require(c != self);", "require(c != j);", "require(OpInputCovenantId(c) == lib);", "require(tx.outputs[c].scriptPubKey == tx.inputs[c].scriptPubKey);", "require(OpOutputCovenantId(c) == lib);", "require(tx.outputs[c].value == floor);", "require(OpCovInputCount(lib) == 3);"] {
                            assert!(body.contains(r), "KpfTSweep: the copy is not held to `{r}`");
                        }
                        Some("the copy swept")
                    }
                    _ if body.contains(&format!("cited(int(byte[8]({})), ", cut(k))) => Some("a citation"),
                    _ if body.contains(&format!("priced(int(byte[8]({}))", cut(k))) || body.contains(&format!("seenPrice(int(byte[8]({}))", cut(k))) => Some("an epoch table"),
                    _ => None,
                };
                let tied = (0..3).filter(|o| *o != k).any(|o| body.contains(&format!("require({} == {});", cut(k), cut(o))));
                assert!(read.is_some() || tied, "{name}.{entry}: place {k} is neither read nor held to another");
                if read == Some("an object") {
                    // (The way in holds the anchor to its hash in `enters`.)
                    let anchor = body.contains("enters(j, rs, slotLen, ") && code.contains("require(byte[32](rs.slice(STATE_AT, STATE_AT + 32)) == blake2b(i9(KIND_ANCHOR)));");
                    assert!(body.contains(".slice(STATE_AT, STATE_AT + 32)") || anchor, "{name}.{entry}: an object is held to the hash of a state");
                }
                how.push(read.map(str::to_string).unwrap_or_else(|| "held to another".into()));
            }
            // A place held to another is held to one that is read, not to another that is only held.
            assert!(how.iter().any(|h| h != "held to another"), "{name}.{entry}");
            assert!(how[0] != "held to another", "{name}.{entry}: the first place is read");
            println!("T1b R19 places: {name}.{entry}: {}", how.join(" | "));
            rows += 1;
        }
    }
    assert_eq!(rows, 33);
}

#[test]
fn t1b_a3_a_library_script_goes_on_unchanged_at_its_index_and_nothing_else_takes_the_id() {
    let w = world();
    let mut bad = Bad::new();
    for sc in scenes(&w) {
        let s = &sc.s;
        let another = thin(if sc.name == "both" { "KpfTAdd" } else { "KpfTBoth" });
        bad.must_fail(sc.name, "the script a sompi short", &s.with(|s| s.outs[1].value -= 1), Some(1));
        bad.must_fail(sc.name, "the script under no id", &s.with(|s| s.outs[1].covenant = None), None);
        bad.must_fail(sc.name, "the script under another id", &s.with(|s| s.outs[1].covenant = bind(1, YID)), None);
        bad.must_fail(sc.name, "the script under the object's id", &s.with(|s| s.outs[1].covenant = bind(0, XID)), None);
        bad.must_fail(sc.name, "another script of the family in its place", &s.with(|s| s.outs[1].script_public_key = p2sh(&another)), Some(1));
        bad.must_fail(sc.name, "an object in its place", &s.with(|s| s.outs[1].script_public_key = spk([0x55; 32])), Some(1));
        bad.must_fail(sc.name, "the script's value to a key", &s.with(|s| s.outs[1] = change(1)), None);
        bad.must_fail(sc.name, "the script at another index", &s.with(|s| s.outs.swap(1, 2)), None);
        bad.must_fail(sc.name, "a second copy of the script under the id", &s.with(|s| s.outs[2] = out(LIBV, p2sh(&s.lib), bind(1, LIB))), None);
        bad.must_fail(sc.name, "another script written under the id", &s.with(|s| s.outs[2] = out(LIBV, p2sh(&another), bind(1, LIB))), None);
        bad.must_fail(sc.name, "an object written under the id", &s.with(|s| s.outs[2] = out(KAS1, spk([0x55; 32]), bind(1, LIB))), None);
        bad.must_fail(sc.name, "a key's output under the id", &s.with(|s| s.outs[2].covenant = bind(1, LIB)), None);
        // The script may hold more than it did (the fee the design leaves there), never less.
        bad.must_pass(sc.name, "the script a sompi richer", &s.with(|s| s.outs[1].value += 1));
    }
    bad.done("A3 the library goes on");
}

// ---------------------------------------------------------------- A4: KpfSlot.go

#[test]
fn t1b_a4_slot_go_bounds() {
    let w = world();
    let v = scenes(&w);
    let mut bad = Bad::new();
    for sc in &v {
        let s = &sc.s;
        for k in [-1, 3, 4, 1 << 31] {
            bad.must_fail(sc.name, &format!("k = {k}"), &s.with(|s| s.go.1 = k), Some(0));
        }
        // The script has every place name the object, so the object may name any of the three.
        for k in [1, 2] {
            bad.must_pass(sc.name, &format!("k = {k}"), &s.with(|s| s.go.1 = k));
        }
        // l its own index, the funding input, out of range, negative.
        for l in [0, 2, 3, 99, -1] {
            bad.must_fail(sc.name, &format!("l = {l}"), &s.with(|s| s.go.0 = l), Some(0));
        }
        // l an input whose signature script is a script's, under no covenant id or under another id.
        bad.must_fail(sc.name, "a script under no id", &s.with(|s| { s.lib_id = None; s.outs[1].covenant = None; }), Some(0));
        bad.must_fail(sc.name, "a script under another id", &s.with(|s| { s.lib_id = Some(YID); s.outs[1].covenant = bind(1, YID); }), Some(0));
        // An object of another library is not moved by this library's script.
        bad.must_fail(sc.name, "an object of another library", &s.with(|s| s.obj = tslot_under(YID, w.hash(sc.kind))), Some(0));
    }
    bad.done("A4 KpfSlot.go");
}

// ---------------------------------------------------------------- A5: ages

#[test]
fn t1b_a5_the_object_is_as_old_as_the_script_states_and_the_script_states_the_window() {
    let w = world();
    let mut bad = Bad::new();
    let mut dispute_daas = 0;
    for sc in scenes(&w) {
        let s = &sc.s;
        let age0 = sc.dispute_daa.unwrap_or(0);
        let set = |age: i64, seq: i64| s.with(|s| { s.ageb = sm8(age).to_vec(); s.seq = seq as u64; });
        bad.must_pass(sc.name, "older than stated", &set(age0, age0 + 10));
        bad.must_pass(sc.name, "more stated and as old", &set(age0 + 10, age0 + 10));
        bad.must_fail(sc.name, "younger than stated", &set(age0 + 10, age0 + 9), Some(0));
        bad.must_fail(sc.name, "younger than stated by far", &set(age0 + 1_000, 0), Some(0));
        bad.must_fail(sc.name, "a negative age", &set(-1, age0 + 10), None);
        bad.must_fail(sc.name, "an age of nine bytes", &s.with(|s| s.ageb.push(0)), None);
        bad.must_fail(sc.name, "an age of seven bytes", &s.with(|s| { s.ageb.pop(); }), None);
        // The sequence's top bit set (a relative lock that is switched off) does not make the object old.
        bad.must_fail(sc.name, "a sequence with its top bit set", &s.with(|s| { s.ageb = sm8(age0 + 10).to_vec(); s.seq = (1u64 << 63) | (age0 as u64 + 10); }), Some(0));
        if let Some(win) = sc.dispute_daa {
            dispute_daas += 1;
            bad.must_fail(sc.name, "a DAA less than the dispute period stated", &set(win - 1, win), Some(1));
            bad.must_fail(sc.name, "no age stated", &set(0, win), Some(1));
            bad.must_fail(sc.name, "the dispute period stated, the object a DAA younger", &set(win, win - 1), Some(0));
            bad.must_fail(sc.name, "the dispute period stated, the object new", &set(win, 0), Some(0));
        }
    }
    // KpfTSettle, KpfTDispute.lapse, KpfTLiqSettle (claimed and proved), KpfTLiqDispute.lapse, KpfTExitEnd, and
    // since round 2 KpfTCure of a claimed liquidation (half a dispute period).
    // Round 11: and KpfTTurn in its two modes (100 DAA, a constant of the script).
    assert_eq!(dispute_daas, 9);
    bad.done("A5 ages");
}

// ---------------------------------------------------------------- B, C: an entry reads the kind of object it is for

#[test]
fn t1b_c_every_entry_refuses_every_other_kind_of_object() {
    let w = world();
    let mut bad = Bad::new();
    for sc in scenes(&w) {
        // The real object of every other kind on the same position, the entry's arguments as they satisfy it on its own
        // kind: position operations on claims, claim operations on positions, one claim's on another's.
        for k in KINDS {
            if k != sc.kind {
                bad.must_fail(sc.name, &format!("given a {k:?}"), &sc.s.with(|s| { s.obj = tslot(w.hash(k)); s.obj_value = w.value(k) as u64; }), Some(1));
                // And with the output written for that kind too (whatever the entry would write from it).
                bad.must_fail(sc.name, &format!("given a {k:?}, holding what the entry expects"), &sc.s.with(|s| s.obj = tslot(w.hash(k))), Some(1));
            }
        }
        // Another position's object with this position's state as the arguments, and the reverse.
        let mut w2 = w.clone();
        w2.a.x.a_margin += 1;
        w2.mv = w2.a.moving();
        w2.order.size += 1;
        if sc.kind != K::Anchor {
            bad.must_fail(sc.name, "another position's object", &sc.s.with(|s| s.obj = tslot(w2.hash(sc.kind))), Some(1));
        }
        bad.must_fail(sc.name, "another position's state shown", &sc.s.with(|s| s.mv = w2.mv.clone()), Some(1));
        let mut w3 = w.clone();
        w3.f[0] ^= 1;
        bad.must_fail(sc.name, "another fixed part shown", &sc.s.with(|s| s.fixed = w3.f.clone()), Some(1));
    }
    bad.done("C kinds");
}

#[test]
fn t1b_b_state_numbers_and_flags_are_the_objects() {
    let w = world();
    let v = scenes(&w);
    let mut bad = Bad::new();
    // KpfTCure's state number (the ninth argument after the two parts): every other number for every real object.
    for name in ["cure", "cure-proved"] {
        let s = the(&v, name);
        let right = match &s.rest[8] {
            A::I(n) => *n,
            _ => panic!("the state number"),
        };
        for n in [-1, 0, 1, 2, 3] {
            if n != right {
                bad.must_fail(name, &format!("state {n}"), &s.with(|s| s.rest[8] = A::I(n)), Some(1));
            }
        }
        // Round 2 (F2): a disputed liquidation is not cured under any state number, at any age.
        for n in [-1, 0, 1, 2, 3] {
            for age in [0, w.a.dispute_daa / 2, w.a.dispute_daa, 10 * w.a.dispute_daa] {
                bad.must_fail(name, &format!("a disputed liquidation, state {n}, age {age}"), &s.with(|s| { s.rest[8] = A::I(n); s.obj = tslot(w.hash(K::LiqDisputed)); s.obj_value = w.value(K::LiqDisputed) as u64; s.ageb = sm8(age).to_vec(); s.seq = age as u64; }), Some(1));
            }
        }
    }
    // A cure of a claimed liquidation as a disputed one cannot be arranged either: the object holds one bond.
    // KpfTLiqSettle's `proved`: a claim as proved, a proved one as a claim, another number.
    for (name, wrong) in [("liq-settle", [1, 2, -1]), ("liq-settle-proved", [0, 2, -1])] {
        let s = the(&v, name);
        for n in wrong {
            bad.must_fail(name, &format!("proved {n}"), &s.with(|s| s.rest[8] = A::I(n)), Some(1));
        }
    }
    // A disputed liquidation is settled under neither flag.
    for n in [0, 1] {
        bad.must_fail("liq-settle", "a disputed liquidation", &the(&v, "liq-settle").with(|s| { s.rest[8] = A::I(n); s.obj = tslot(w.hash(K::LiqDisputed)); s.obj_value = w.value(K::LiqDisputed) as u64; }), Some(1));
    }
    // A side that is neither, where the entry reads it.
    for name in ["add", "close", "liquidate", "exit"] {
        let s = the(&v, name);
        for side in [0, 3, -1] {
            bad.must_fail(name, &format!("side {side}"), &s.with(|s| s.rest[0] = A::I(side)), Some(1));
        }
    }
    bad.done("B state numbers");
}

/// The worlds in which one byte string has another width than the entry declares, every hash following it.
fn widths(w: &W) -> Vec<(String, W)> {
    let mut v = vec![];
    let cut = |b: &[u8]| b[..b.len() - 1].to_vec();
    let more = |b: &[u8]| [b, &[0u8][..]].concat();
    let field = |w: &mut W, name: &str, b: Vec<u8>| match name {
        "moving" => w.mv = b,
        "fixed" => w.f = b,
        "dkey" => w.dkey = b,
        "lkey" => w.lkey = b,
        _ => w.rkey = b,
    };
    for name in ["moving", "fixed", "dkey", "lkey", "rkey"] {
        let base = match name {
            "moving" => w.mv.clone(),
            "fixed" => w.f.clone(),
            "dkey" => w.dkey.clone(),
            "lkey" => w.lkey.clone(),
            _ => w.rkey.clone(),
        };
        for (label, b) in [("a byte short", cut(&base)), ("a byte long", more(&base)), ("empty", vec![]), ("of 119 bytes", [base.clone(), vec![7; 119]].concat()[..119].to_vec()), ("of 64 bytes", [base.clone(), base.clone()].concat()[..64].to_vec())] {
            if b.len() == base.len() {
                continue;
            }
            let mut x = w.clone();
            field(&mut x, name, b);
            v.push((format!("{name} {label}"), x));
        }
    }
    v
}

#[test]
fn t1b_b_every_byte_string_has_the_width_the_entry_declares() {
    let w = world();
    let mut bad = Bad::new();
    // who and the age: the object reads them at fixed places of the signature script.
    for sc in scenes(&w) {
        for (label, who) in [("23", who_of([0, 0, 0])[..23].to_vec()), ("25", [who_of([0, 0, 0]), vec![0]].concat()), ("32", [who_of([0, 0, 0]), vec![0; 8]].concat()), ("16", who_of([0, 0, 0])[..16].to_vec()), ("0", vec![])] {
            bad.must_fail(sc.name, &format!("who of {label} bytes"), &sc.s.with(|s| s.who = who.clone()), None);
        }
    }
    // The two parts of the state and the keys: the object's hash is that of the bytes shown, so only the width
    // stands between the entry and a state of another shape.
    for (label, x) in widths(&w) {
        for sc in scenes(&x) {
            let uses = |b: &Vec<u8>| sc.s.rest.iter().any(|a| matches!(a, A::B(k) if k == b));
            // (An order's scenes show no part of a position: their widths are `t1b_o5_...`.)
            let touched = ((label.starts_with("moving") || label.starts_with("fixed")) && sc.kind != K::Order && sc.name != "registry-place") || (label.starts_with("dkey") && uses(&x.dkey)) || (label.starts_with("lkey") && uses(&x.lkey)) || (label.starts_with("rkey") && uses(&x.rkey));
            if touched {
                // The script's own refusal: the object does not see these bytes.
                bad.must_fail(sc.name, &label, &sc.s, Some(1));
            }
        }
    }
    bad.done("B widths");
}

/// The state of an object of one kind read as a position: `fixed` and `moving` cut so that
/// `blake2b(push(fixed) + moving)` is that object's hash.
#[test]
fn t1b_b_no_object_is_an_position_by_another_cut_of_its_preimage() {
    let w = world();
    let position = w.position();
    let said: Vec<u8> = w.said().iter().flat_map(|x| push9(*x)).collect();
    let lsaid: Vec<u8> = [w.lsaid().iter().flat_map(|x| push9(*x)).collect::<Vec<u8>>(), p33b(&w.lkey)].concat();
    let c = h_claim(&position, &w.said());
    let l = h_liq(&position, &w.lsaid(), &w.lkey);
    // (what, the kind whose hash it is, fixed, moving)
    let cuts: Vec<(&str, K, Vec<u8>, Vec<u8>)> = vec![
        ("a claim: the position's hash as the fixed part, what was stated as the moving part (54 bytes)", K::Claim, position.to_vec(), said.clone()),
        ("a liquidation: the position's hash, what was stated and the key (96 bytes)", K::Liq, position.to_vec(), lsaid.clone()),
        ("a liquidation: 8 bytes of the position's hash, the other 120 bytes", K::Liq, position[..8].to_vec(), [&position[8..], &lsaid[..]].concat()),
        ("a disputed claim: the claim's hash, the pushed key (33 bytes)", K::Disputed, c.to_vec(), p33b(&w.dkey)),
        ("a disputed liquidation: the liquidation's hash, the pushed key", K::LiqDisputed, l.to_vec(), p33b(&w.dkey)),
    ];
    let mut bad = Bad::new();
    for (what, k, f, mv) in cuts {
        let mut x = w.clone();
        x.f = f;
        x.mv = mv;
        assert_eq!(x.position(), w.hash(k), "{what}: the cut is that object's preimage");
        for sc in scenes(&x).iter().filter(|s| s.kind == K::Position) {
            bad.must_fail(sc.name, what, &sc.s.with(|s| s.obj_value = w.value(k) as u64), Some(1));
        }
    }
    // A proved liquidation and an exit claim begin with a pushed integer, not a pushed hash: no cut of them begins
    // with the byte 0x20 a position's preimage begins with.
    assert_eq!(push9(1)[0], 0x08);
    // The other way: a disputed claim whose "key" is the rest of a position's moving part would be a position with a
    // fixed part of the disputer's choice. KpfTDispute.dispute and KpfTLiqDispute.dispute with a key of 119 bytes
    // that is such a moving part (keys the disputer's, margins as it likes) must not write it.
    let mut thief = w.a;
    thief.x.a_key = xkey(&keypair(40));
    thief.x.b_key = xkey(&keypair(43));
    let mut x = w.clone();
    x.dkey = thief.moving()[1..].to_vec();
    assert_eq!(x.hash(K::Disputed), h_position(&c, &thief.moving()), "a disputed claim with that key is a position of the thief's keys");
    for sc in scenes(&x).iter().filter(|s| ["dispute", "liq-dispute"].contains(&s.name)) {
        bad.must_fail(sc.name, "a key of 119 bytes that makes the disputed claim a position", &sc.s, Some(1));
        println!("T1b B cuts: {} with a key of 119 bytes: {}", sc.name, show(&sc.s.tx()));
    }
    bad.done("B cuts");
}

// ---------------------------------------------------------------- D: times, and what a rewrite costs

#[test]
fn t1b_d_a_rewrite_pays_what_the_entry_asks_and_every_output_is_whole() {
    let w = world();
    let v = scenes(&w);
    let mut bad = Bad::new();
    for sc in &v {
        if sc.name == "both" {
            continue;
        }
        for i in (0..sc.s.outs.len()).filter(|i| !is_change(&sc.s.outs[*i]) && sc.s.outs[*i].value > 0) {
            bad.must_fail(sc.name, &format!("output {i} a sompi short"), &sc.s.with(|s| s.outs[i].value -= 1), Some(1));
        }
        // An object written back is written under its own id and nowhere else.
        if sc.s.outs[0].covenant.is_some() {
            bad.must_fail(sc.name, "the object written under no id", &sc.s.with(|s| s.outs[0].covenant = None), None);
            bad.must_fail(sc.name, "the object written twice", &sc.s.with(|s| s.outs[2] = s.outs[0].clone()), None);
        } else {
            bad.must_fail(sc.name, "an object of the id written beside the payment", &sc.s.with(|s| s.outs[2] = slot_out(w.position(), 1)), None);
        }
    }
    // The entries that write a claim back under its id while it stands, and what each asks:
    // a claim in time is not ended as late; a liquidation in time neither; an exit claim of the past is not early.
    for name in ["late", "liq-late"] {
        for landed in [T0, T0 + 1, T0 + w.a.x.t.late_daa] {
            bad.must_fail(name, &format!("a claim that landed at {landed}, in time"), &the(&v, name).with(|s| s.obj_daa = landed as u64), Some(1));
        }
        // A claim that landed before its own time is ended.
        bad.must_pass(name, "a claim that landed before its time", &the(&v, name).with(|s| s.obj_daa = (T0 - 1) as u64));
    }
    for landed in [w.since, w.since + 1, w.since + 10_000] {
        bad.must_fail("quiet-early", "a claim that landed at or after its since", &the(&v, "quiet-early").with(|s| s.obj_daa = landed as u64), Some(1));
    }
    // A claim that is late (or before its time) is neither settled nor disputed; an exit claim of a time to come is
    // not settled.
    for name in ["settle", "dispute", "liq-settle", "liq-dispute"] {
        for landed in [T0 - 1, T0 + w.a.x.t.late_daa + 1, 0] {
            bad.must_fail(name, &format!("a claim that landed at {landed}"), &the(&v, name).with(|s| s.obj_daa = landed as u64), Some(1));
        }
        bad.must_pass(name, "a claim that landed at the last DAA in time", &the(&v, name).with(|s| s.obj_daa = (T0 + w.a.x.t.late_daa) as u64));
    }
    bad.must_fail("quiet-end", "a claim of a time to come", &the(&v, "quiet-end").with(|s| s.obj_daa = (w.since - 1) as u64), Some(1));
    // A proved liquidation is settled whenever it landed (its proof is its time), after the dispute period.
    bad.must_pass("liq-settle-proved", "landed long after", &the(&v, "liq-settle-proved").with(|s| s.obj_daa = 99_000));
    // A cure and an added margin are the side's own: another key's signature, the other party's.
    for name in ["cure", "cure-proved", "add", "close", "exit", "order-cancel", "registry-place"] {
        for k in [other(), keypair(41), keypair(40)] {
            bad.must_fail(name, "signed by another key", &the(&v, name).with(|s| { let n = s.rest.len() - 1; s.rest[n] = A::S(k); }), Some(1));
        }
    }
    for (a, b) in [(owner(), owner()), (other(), other()), (other(), owner()), (owner(), keypair(40))] {
        bad.must_fail("both", "not the two parties' signatures in their places", &the(&v, "both").with(|s| s.rest = vec![A::S(a), A::S(b)]), Some(1));
    }
    // A disputed object is not settled, a proved liquidation not disputed again, a disputed one not disputed twice
    // (also in the kinds' table; here by name).
    for (name, k) in [("settle", K::Disputed), ("liq-settle", K::LiqDisputed), ("liq-settle-proved", K::LiqDisputed), ("liq-dispute", K::LiqProved), ("liq-dispute", K::LiqDisputed), ("dispute", K::Disputed), ("liq-late", K::LiqProved), ("liq-late", K::LiqDisputed), ("late", K::Disputed)] {
        bad.must_fail(name, &format!("on a {k:?}"), &the(&v, name).with(|s| { s.obj = tslot(w.hash(k)); s.obj_value = w.value(k) as u64; }), Some(1));
    }
    bad.done("D times and costs");
}


/// Round 1, F4 (the engine's side, unchanged by design; the tool's check is `t1b_fixed_lib_check_...` in
/// `auditT1_builders.rs`). An object trusts every input that carries its library's id and begins its signature script with
/// the two pushes. That is "a script of the family" only if the library's genesis wrote the family's scripts and
/// nothing else: an output more under the id, of any script (here: drop two items, true), takes every object of that
/// library whatever its state, and goes on doing it. Nothing in the scripts can hold a genesis to the family; the
/// tool now reads a genesis before a position is opened in its library (`thin_build::lib_check`).
#[test]
fn t1b_design_an_object_trusts_every_input_of_its_librarys_id() {
    let w = world();
    let rogue = vec![0x6d, 0x51];
    let rogue_spk = kaspa_txscript::pay_to_script_hash_script(&rogue);
    for k in KINDS {
        let obj = tslot(w.hash(k));
        let value = w.value(k) as u64;
        let go = || vec![Arg::V(ArtifactValue::Int(1)), Arg::V(ArtifactValue::Int(0))];
        let tx_under = |id: Hash| {
            let ins = vec![
                In { outpoint: outpoint(10), utxo: UtxoEntry::new(value, p2sh(&obj), 6_510, false, Some(XID)), spend: Spend::Signed { art: obj.clone(), entry: "go", args: go() } },
                // The second input's signature script is written below; this spend is a placeholder.
                In { outpoint: outpoint(11), utxo: UtxoEntry::new(KAS1, rogue_spk.clone(), 90, false, Some(id)), spend: Spend::Signed { art: obj.clone(), entry: "go", args: go() } },
            ];
            let outs = vec![out(value, p2pk_spk(&keypair(66)), None), out(KAS1, rogue_spk.clone(), bind(1, id))];
            let mut t = build_ext(&ins, outs, 0, &[]);
            t.0.inputs[1].signature_script = [raw_push(&who_of([0, 0, 0])), raw_push(&sm8(0)), raw_push(&rogue)].concat();
            t
        };
        let t = tx_under(LIB);
        assert!(all_ok(&t), "{k:?}: {}", show(&t));
        assert_eq!(t.0.outputs[0].value, value, "the whole object to a key, no signature of a party, no dispute period");
        // The same output under any other id moves nothing.
        assert!(fails(&tx_under(YID), 0), "{k:?}");
    }
}

// ================================================================ round 2

fn paid_to(t: &Tx, key: &[u8]) -> u64 {
    let spk = pay_raw(key, 0).script_public_key;
    t.0.outputs.iter().filter(|o| o.script_public_key == spk).map(|o| o.value).sum()
}

// ---------------------------------------------------------------- F1: no output pays two oracle fees

#[derive(Clone, Copy, PartialEq, Debug)]
enum Place {
    /// At the object's own index.
    Front,
    /// Counted from the end: the entry's first place (0) or its second (1).
    End(usize),
    /// The object written back, wherever its id's output is.
    Any,
}

/// What a scene's script asks of the outputs beside its own going on.
fn reqs(sc: &Sc) -> Vec<(Place, TransactionOutput)> {
    let o = &sc.s.outs;
    let mut v = vec![(if o[0].covenant.is_some() { Place::Any } else { Place::Front }, o[0].clone())];
    let tail = &o[5..];
    for (i, t) in tail.iter().enumerate() {
        v.push((Place::End(tail.len() - 1 - i), t.clone()));
    }
    v
}

/// The position of `world` with one key everywhere: both parties, the disputer, the liquidator, whoever ends a claim.
fn one_key_world() -> W {
    let mut w = world();
    w.a.x.b_key = w.a.x.a_key;
    w.mv = w.a.moving();
    let k = w.a.x.a_key.to_vec();
    w.dkey = k.clone();
    w.lkey = k.clone();
    w.rkey = k;
    w.b_signer = owner();
    w
}

struct Laid {
    t: Tx,
    /// An output stands at a place two requirements name.
    shared: bool,
    /// A place counted from the end falls among the input indices.
    low: bool,
    /// For every payment asked (x's, then y's): the output it stands at and the input of the script that asks it.
    asked: Vec<(usize, usize)>,
}

/// Two operations in one transaction of five inputs: x's object at input `jx`, y's (under another id) at `jy`, the two
/// scripts and the funding at the other three in that order. `len` outputs; a place from the end is
/// `len - 1 - coef * j - k`. Where two payments to one key meet at one index a single output of the larger is
/// written. `short`: the payment (in the order of `asked`) written a sompi short. None if the layout cannot be
/// written at all (two different scripts at one index, no room).
fn lay(x: &Sc, y: &Sc, jx: usize, jy: usize, len: usize, coef: usize, short: Option<usize>) -> Option<Laid> {
    let free: Vec<usize> = (0..5).filter(|i| *i != jx && *i != jy).collect();
    let (lx, ly, fu) = (free[0], free[1], free[2]);
    if lx >= len || ly >= len {
        return None;
    }
    let xs = x.s.with(|s| { s.who = who_of([jx as i64; 3]); s.go = (lx as i64, 0); s.obj_id = XID; });
    let ys = y.s.with(|s| { s.who = who_of([jy as i64; 3]); s.go = (ly as i64, 0); s.obj_id = YID; });
    let mut ins: Vec<Option<In>> = (0..5).map(|_| None).collect();
    ins[jx] = Some(xs.obj_in(10));
    ins[jy] = Some(ys.obj_in(12));
    ins[lx] = Some(xs.lib_in(11));
    ins[ly] = Some(ys.lib_in(13));
    ins[fu] = Some(funding(14));
    let ins: Vec<In> = ins.into_iter().map(|i| i.expect("five inputs")).collect();
    let mut seqs = vec![0u64; 5];
    seqs[jx] = xs.seq;
    seqs[jy] = ys.seq;
    let mut slots: Vec<Vec<TransactionOutput>> = vec![vec![]; len];
    slots[lx].push(out(xs.outs[1].value, p2sh(&xs.lib), bind(lx as u16, LIB)));
    slots[ly].push(out(ys.outs[1].value, p2sh(&ys.lib), bind(ly as u16, LIB)));
    let (mut any, mut low, mut asked, mut n) = (vec![], false, vec![], 0);
    for (sc, j, l, id) in [(x, jx, lx, XID), (y, jy, ly, YID)] {
        for (place, mut o) in reqs(sc) {
            let at = match place {
                Place::Front => j as i64,
                Place::End(k) => len as i64 - 1 - (coef * j) as i64 - k as i64,
                Place::Any => {
                    any.push(out(o.value, o.script_public_key.clone(), bind(j as u16, id)));
                    continue;
                }
            };
            if at < 0 || at >= len as i64 {
                return None;
            }
            if matches!(place, Place::End(_)) && at < 5 {
                low = true;
            }
            if short == Some(n) {
                o.value -= 1;
            }
            n += 1;
            asked.push((at as usize, l));
            slots[at as usize].push(o);
        }
    }
    let mut shared = false;
    let mut outs: Vec<Option<TransactionOutput>> = vec![];
    for s in &slots {
        outs.push(match s.len() {
            0 => None,
            1 => Some(s[0].clone()),
            _ => {
                if s.iter().any(|o| o.covenant.is_some() || o.script_public_key != s[0].script_public_key) {
                    return None;
                }
                shared = true;
                Some(out(s.iter().map(|o| o.value).max().unwrap(), s[0].script_public_key.clone(), None))
            }
        });
    }
    for a in any {
        let i = outs.iter().position(|o| o.is_none())?;
        outs[i] = Some(a);
    }
    let outs: Vec<TransactionOutput> = outs.into_iter().map(|o| o.unwrap_or_else(|| change(1))).collect();
    let mut t = build_ext(&ins, outs, xs.lock.max(ys.lock), &seqs);
    t.0.inputs[lx].signature_script = xs.patch(&t.0.inputs[lx].signature_script);
    t.0.inputs[ly].signature_script = ys.patch(&t.0.inputs[ly].signature_script);
    Some(Laid { t, shared, low, asked })
}

/// Round 1, F1, fixed: a place counted from the end is `len - 1 - 2j` (and one before it). Every ordered pair of the
/// thirteen entries that pay a key (eleven before round 4), the two objects at inputs (0,1), (0,2), (1,2) in both orders, every key the same
/// key, 5 to 12 outputs. Whatever layout puts two payments at one output, or a place from the end among the input
/// indices, is refused; every other is accepted; and in an accepted one a sompi less at any payment fails the one
/// script that asked it and not the other.
#[test]
fn t1b_fixed_no_output_pays_two_dues_of_one_key() {
    let w = one_key_world();
    let v: Vec<Sc> = scenes(&w).into_iter().filter(|s| s.name != "both" && reqs(s).iter().any(|(p, _)| *p != Place::Any)).collect();
    assert_eq!(v.len(), 13, "settle, late, lapse, liq-late, liq-settle (two), cure (two), liq-lapse, quiet-early, quiet-end, and since round 4 order-cancel and order-expire");
    let (mut accepted, mut refused_shared, mut refused_low, mut old_shared, mut probes) = (0, 0, 0, 0, 0);
    let mut bad = vec![];
    for x in &v {
        for y in &v {
            for (jx, jy) in [(0, 1), (1, 0), (0, 2), (2, 0), (1, 2), (2, 1)] {
                let mut once = false;
                for len in 5..=12 {
                    let Some(l) = lay(x, y, jx, jy, len, 2, None) else { continue };
                    let ok = all_ok(&l.t);
                    if l.shared || l.low {
                        if ok {
                            bad.push(format!("{} at {jx} + {} at {jy}, {len} outputs: accepted with {}", x.name, y.name, if l.shared { "one output for two payments" } else { "a place among the inputs" }));
                        }
                        if l.shared { refused_shared += 1 } else { refused_low += 1 }
                        continue;
                    }
                    if !ok {
                        bad.push(format!("{} at {jx} + {} at {jy}, {len} outputs: every payment at its place, refused: {}", x.name, y.name, show(&l.t)));
                        continue;
                    }
                    accepted += 1;
                    if once {
                        continue;
                    }
                    once = true;
                    // Each payment is asked by one script: a sompi short fails that script's input and no other.
                    for n in 0..l.asked.len() {
                        let s = lay(x, y, jx, jy, len, 2, Some(n)).expect("the same layout");
                        let (at, by) = s.asked[n];
                        let r = run(&s.t.0, &s.t.1).expect("bindings");
                        let failed: Vec<usize> = (0..5).filter(|i| r[*i].is_err()).collect();
                        probes += 1;
                        if failed != vec![by] {
                            bad.push(format!("{} at {jx} + {} at {jy}, {len} outputs: output {at} a sompi short fails inputs {failed:?}, asked by {by}", x.name, y.name));
                        }
                    }
                }
                if !once {
                    bad.push(format!("{} at {jx} + {} at {jy}: no layout of 5 to 12 outputs is accepted", x.name, y.name));
                }
                // The places of round 1 (`len - 1 - j`): wherever they put two payments at one output, refused now.
                for len in 5..=12 {
                    if let Some(l) = lay(x, y, jx, jy, len, 1, None) {
                        if l.shared {
                            old_shared += 1;
                            if all_ok(&l.t) {
                                bad.push(format!("{} at {jx} + {} at {jy}, {len} outputs, the places of round 1: one output for two payments, accepted", x.name, y.name));
                            }
                        }
                    }
                }
            }
        }
    }
    println!("T1b F1 fixed: {accepted} layouts accepted with every payment at its own output; refused: {refused_shared} with one output for two payments, {refused_low} with a place among the inputs, {old_shared} shared at the places of round 1; {probes} payments a sompi short, each failing the one script that asked it");
    assert!(bad.is_empty(), "{} not as expected:\n{}", bad.len(), bad.join("\n"));
    assert!(accepted >= 169 * 6 && refused_shared > 0 && refused_low > 0 && old_shared > 100);
}

// ---------------------------------------------------------------- F3: a claim not in time loses its bond

/// Round 1, F3, fixed: `late` shares the bond. Of a declared claim: half to whoever ends it, half to the party it was
/// made against. Of a liquidation (round 3): half to whoever ends it, half left in the script's own copy, so that a
/// liquidation that names its maker's own side gives no half back to it. Nothing goes back to the maker as such.
#[test]
fn t1b_fixed_a_claim_of_a_price_long_past_loses_its_bond() {
    for (label, t0) in [("long past", 1), ("still to come", 9_000)] {
        let mut w = world();
        w.t0 = t0;
        let v = scenes(&w);
        let (bond, half) = (w.bond(), w.bond() / 2);
        let (a, b) = (w.a.x.a_key.to_vec(), w.a.x.b_key.to_vec());
        // (the entry that makes it, the one that ends it, ..., its maker, who has the other half: a party or the script)
        for (make, end, settle, dispute, maker, other) in [("liquidate", "liq-late", "liq-settle", "liq-dispute", w.lkey.clone(), None), ("close", "late", "settle", "dispute", a.clone(), Some(b.clone()))] {
            let landed = 6_510;
            assert!(all_ok(&the(&v, make).tx()), "{make} of a signed price {label}: the entry reads no time");
            assert!(fails(&the(&v, settle).with(|s| s.obj_daa = landed).tx(), 1));
            assert!(fails(&the(&v, dispute).with(|s| s.obj_daa = landed).tx(), 1));
            let e = the(&v, end).with(|s| s.obj_daa = landed);
            let n = e.outs.len();
            let t = e.tx();
            assert!(all_ok(&t), "{end}: {}", show(&t));
            assert_eq!(paid_to(&t, &w.rkey) as i64, bond - half, "{end}: half to whoever ends it");
            match &other {
                Some(k) => assert_eq!(paid_to(&t, k) as i64, half, "{end}: half to the party it was made against"),
                None => {
                    assert_eq!(e.outs[1].value as i64, KAS + ORACLE_FEE as i64 + half, "{end}: half stays in the script");
                    assert_eq!((paid_to(&t, &a), paid_to(&t, &b)), (0, 0), "{end}: nothing to either party");
                    assert!(fails(&e.with(|s| s.outs[1].value -= 1).tx(), 1), "{end}: the script a sompi short of the half");
                    // The transaction of round 2 (the half to the side named, the script as it was).
                    assert!(fails(&e.with(|s| { s.outs[1].value = LIBV; s.outs[4] = pay(&w.a.x.a_key, half); }).tx(), 1), "{end}: the half to the side named");
                }
            }
            assert_eq!(paid_to(&t, &maker), 0, "{end}: nothing to the maker");
            // The transaction of round 1 (the whole bond back to the maker at the last output), and the maker named
            // as the ender taking the whole bond.
            assert!(fails(&e.with(|s| s.outs[n - 1] = pay_raw(&maker, bond)).with(|s| if other.is_some() { s.outs[n - 2] = change(1) }).tx(), 1));
            let k = e.rest.len() - 1;
            let greedy = e.with(|s| { s.rest[k] = A::B(maker.clone()); s.outs[n - 1] = pay_raw(&maker, bond); });
            assert!(fails(&greedy.with(|s| if other.is_some() { s.outs[n - 2] = change(1) } else { s.outs[1].value = LIBV }).tx(), 1), "{end}: the maker as ender takes the whole bond");
            // The maker ending its own claim has the ender's half and no more.
            let own = e.with(|s| { s.rest[k] = A::B(maker.clone()); s.outs[n - 1] = pay_raw(&maker, bond - half); });
            let t = own.tx();
            assert!(all_ok(&t), "{end} by the maker");
            assert_eq!(paid_to(&t, &maker) as i64, bond - half);
            println!("T1b F3 fixed: {make} at a signed price {label}: of its bond {bond}, {} to whoever ends it, {half} {}", bond - half, if other.is_some() { "to the party it was made against" } else { "left in the script" });
        }
    }
    // Round 2's note, closed in round 3: a liquidation that names its maker's own side, ended by its maker, no longer
    // gives the whole bond back: the side named has no half.
    let mut w = world();
    w.t0 = 1;
    w.lkey = w.a.x.a_key.to_vec();
    w.rkey = w.a.x.a_key.to_vec();
    let v = scenes(&w);
    let e = the(&v, "liq-late").with(|s| s.obj_daa = 6_510);
    let t = e.tx();
    assert!(all_ok(&t));
    assert_eq!(paid_to(&t, &w.lkey) as i64, w.bond() - w.bond() / 2, "a side that names itself and ends it itself has half its bond");
    let n = e.outs.len();
    assert!(fails(&e.with(|s| { s.outs[n - 1].value = w.bond() as u64; s.outs[1].value = LIBV; }).tx(), 1));
}

// ---------------------------------------------------------------- edge amounts

fn tight(label: &str, w: &W, skip: &[&str], bad: &mut Bad) {
    for sc in scenes(w) {
        if skip.contains(&sc.name) {
            bad.must_fail(sc.name, &format!("{label}: not to be accepted"), &sc.s, Some(1));
            continue;
        }
        bad.must_pass(sc.name, label, &sc.s);
        if sc.name == "both" {
            continue;
        }
        for i in (0..sc.s.outs.len()).filter(|i| !is_change(&sc.s.outs[*i]) && sc.s.outs[*i].value > 0) {
            bad.must_fail(sc.name, &format!("{label}: output {i} a sompi short"), &sc.s.with(|s| s.outs[i].value -= 1), Some(1));
        }
    }
}

#[test]
fn t1b_r2_edge_amounts() {
    use kpf_protocol::cross::MARGIN_MAX;
    let mut bad = Bad::new();
    // A bond of one sompi (a pot of 10,000 sompi at one basis point): every entry runs; the half that is nothing is
    // not asked, the whole goes to the first place.
    let mut w = world();
    w.a.x.a_margin = 6_000;
    w.a.x.b_margin = 4_000;
    w.a.bond_bps = 1;
    w.mv = w.a.moving();
    w.amt = Some(5_000);
    w.fee = 100;
    w.add = 10;
    assert_eq!(w.bond(), 1);
    tight("a bond of one sompi", &w, &[], &mut bad);
    // A bond that would be nothing: no claim is made (9,999 sompi at one basis point; any pot at none).
    for (am, bps) in [(5_999, 1), (6_000, 0), (2_000 * KAS, 0)] {
        let mut z = w.clone();
        z.a.x.a_margin = am;
        z.a.bond_bps = bps;
        z.mv = z.a.moving();
        z.amt = Some(1_000);
        assert_eq!(z.bond(), 0);
        let v = scenes(&z);
        for name in ["close", "liquidate", "exit"] {
            bad.must_fail(name, &format!("a claim with no bond (margin {am}, rate {bps})"), &the(&v, name), Some(1));
        }
        // And a position of that kind is still a position to its two parties.
        bad.must_pass("both", "a position whose bond is nothing", &the(&v, "both"));
    }
    // Both margins at MARGIN_MAX: nothing can be added to A's (add, cure), everything else runs, with a claim of the
    // whole pot.
    let mut m = world();
    m.a.x.a_margin = MARGIN_MAX;
    m.a.x.b_margin = MARGIN_MAX;
    m.mv = m.a.moving();
    m.amt = Some(m.a.pot());
    tight("margins at MARGIN_MAX", &m, &["add", "cure", "cure-proved"], &mut bad);
    // A margin that an added 100 KAS brings exactly to MARGIN_MAX: taken; a sompi more: not.
    let mut m2 = world();
    m2.a.x.a_margin = MARGIN_MAX - 100 * KAS;
    m2.mv = m2.a.moving();
    m2.amt = Some(1_000 * KAS);
    let v = scenes(&m2);
    bad.must_pass("add", "to MARGIN_MAX exactly", &the(&v, "add"));
    bad.must_fail("add", "a sompi above MARGIN_MAX", &the(&v, "add").with(|s| { s.rest[1] = A::I(100 * KAS + 1); s.outs[0] = slot_out(h_position(&m2.f, &m2.mv_plus(100 * KAS + 1)), m2.v0() + 100 * KAS + 1); }), Some(1));
    // An object that holds less than its state says (nothing in a script ties a position's value to its margins: the
    // opening does). A settlement pays the claimer first, up to what the object holds; the entries that write the
    // position back need the bonds to be there.
    let w = world();
    let v = scenes(&w);
    let (a, bond, amount) = (w.a.x.a_key, w.bond(), w.amount());
    for (name, key, mine) in [("settle", a.to_vec(), amount + bond), ("liq-settle", w.lkey.clone(), w.fee + bond), ("liq-settle-proved", w.lkey.clone(), w.fee + bond), ("quiet-end", a.to_vec(), w.a.x.a_margin + bond)] {
        for short in [1, 7, mine - 1] {
            let held = mine - short;
            let s = the(&v, name).with(|s| { s.obj_value = held as u64; s.outs[0] = pay_raw(&key, held); s.outs.truncate(5); });
            bad.must_pass(name, &format!("an object holding {short} less than the first party's due: all of it to that party"), &s);
            bad.must_fail(name, "and a sompi less to it", &s.with(|s| s.outs[0].value -= 1), Some(1));
        }
        // Holding a sompi more than the first party's due: the sompi is the other party's.
        let s = the(&v, name).with(|s| { s.obj_value = (mine + 1) as u64; let n = s.outs.len(); s.outs[n - 1].value = 1; });
        bad.must_pass(name, "an object holding a sompi more than the first party's due", &s);
        bad.must_fail(name, "and that sompi not paid", &s.with(|s| { let n = s.outs.len(); s.outs[n - 1].value = 0; }), Some(1));
    }
    for (name, need) in [("lapse", 2 * bond), ("liq-lapse", 2 * bond), ("late", bond), ("liq-late", bond), ("quiet-early", bond), ("cure", bond), ("cure-proved", bond)] {
        for held in [need - 1, 0] {
            bad.must_fail(name, &format!("an object holding {held}, less than the bonds it gives back"), &the(&v, name).with(|s| { s.obj_value = held as u64; s.outs[0].value = 0; }), Some(1));
        }
        // Exactly the bonds: the position is written back holding nothing.
        bad.must_pass(name, "an object holding exactly its bonds", &the(&v, name).with(|s| { s.obj_value = need as u64; s.outs[0].value = if name.starts_with("cure") { w.add as u64 } else { 0 }; }));
    }
    bad.done("edge amounts");
}

// ================================================================ round 3

/// Round 2, F2b, fixed in round 3 (the engine's side): the half of a false or late liquidator's bond that went to the
/// side named stays in the script's own copy, and no arrangement of the outputs gives it to a key. Here every key
/// the side could be (the side named, the liquidator, the disputer, whoever ends the claim) is one key.
#[test]
fn t1b_fixed_the_half_of_a_liquidators_bond_stays_in_the_script() {
    let mut w = world();
    let k = w.a.x.a_key.to_vec();
    w.dkey = k.clone();
    w.lkey = k.clone();
    w.rkey = k.clone();
    let v = scenes(&w);
    let (bond, half) = (w.bond(), w.bond() / 2);
    let mut bad = Bad::new();
    for (name, due) in [("liq-late", bond - half)] {
        let s = the(&v, name);
        let n = s.outs.len();
        let t = s.tx();
        assert!(all_ok(&t), "{name}: {}", show(&t));
        assert_eq!(paid_to(&t, &k) as i64, due, "{name}: what the side's keys have of the bonds");
        assert_eq!(s.outs[1].value as i64, KAS + ORACLE_FEE as i64 + half, "{name}: half a bond in the script");
        let plain = |s: &mut Scene| s.outs[1].value = LIBV;
        bad.must_fail(name, "round 2's transaction: the half to the side named, before the last output", &s.with(|s| { plain(s); s.outs[n - 2] = pay_raw(&k, half); }), Some(1));
        bad.must_fail(name, "the half added to the last output", &s.with(|s| { plain(s); s.outs[n - 1].value += half as u64; }), Some(1));
        bad.must_fail(name, "the half left in the position", &s.with(|s| { plain(s); s.outs[0].value += half as u64; }), Some(1));
        bad.must_fail(name, "the script a sompi short of the half", &s.with(|s| s.outs[1].value -= 1), Some(1));
        bad.must_fail(name, "the half in a second copy of the script", &s.with(|s| { plain(s); s.outs[2] = out(LIBV + half as u64, p2sh(&s.lib), bind(1, LIB)); }), None);
        bad.must_fail(name, "the half in the script's address under no id", &s.with(|s| { plain(s); s.outs[2] = out(half as u64, p2sh(&s.lib), None); }), Some(1));
        bad.must_pass(name, "more than the half in the script", &s.with(|s| s.outs[1].value += 1));
    }
    // Round 23: a dispute nobody answers decides nothing. A lapse (of a close and of a liquidation) returns both
    // bonds, each to its key at its own place, and leaves nothing more in the script than the oracle fee; no
    // arrangement gives a bond's part to anybody else. (With every key one key here, the two bonds are that key's.)
    for name in ["lapse", "liq-lapse"] {
        let s = the(&v, name);
        let n = s.outs.len();
        let t = s.tx();
        assert!(all_ok(&t), "{name}: {}", show(&t));
        assert_eq!(paid_to(&t, &k) as i64, 2 * bond, "{name}: both bonds back");
        assert_eq!(s.outs[1].value, LIBV, "{name}: the script keeps the oracle fee and no part of a bond");
        bad.must_fail(name, "the disputer's bond a sompi short", &s.with(|s| s.outs[n - 1].value -= 1), Some(1));
        bad.must_fail(name, "the claimer's bond a sompi short", &s.with(|s| s.outs[n - 2].value -= 1), Some(1));
        bad.must_fail(name, "both bonds in the last output", &s.with(|s| { s.outs[n - 2] = change(1); s.outs[n - 1].value += bond as u64; }), Some(1));
        bad.must_fail(name, "both bonds in the output before the last", &s.with(|s| { s.outs[n - 1] = change(1); s.outs[n - 2].value += bond as u64; }), Some(1));
        bad.must_fail(name, "a bond left in the position and not paid", &s.with(|s| { s.outs[n - 2] = change(1); s.outs[0].value += bond as u64; }), Some(1));
        bad.must_fail(name, "the position a sompi short", &s.with(|s| s.outs[0].value -= 1), None);
    }
    // And with three different keys: each bond at its own key's place, not at the other's.
    {
        let w3 = world();
        let v3 = scenes(&w3);
        for name in ["lapse", "liq-lapse"] {
            let s = the(&v3, name);
            let n = s.outs.len();
            assert!(all_ok(&s.tx()), "{name}");
            bad.must_fail(name, "the two bonds at each other's places", &s.with(|s| s.outs.swap(n - 1, n - 2)), Some(1));
            bad.must_fail(name, "the claimer's bond to the disputer", &s.with(|s| { let o = s.outs[n - 1].script_public_key.clone(); s.outs[n - 2].script_public_key = o; }), Some(1));
            bad.must_fail(name, "the disputer's bond to the claimer", &s.with(|s| { let o = s.outs[n - 2].script_public_key.clone(); s.outs[n - 1].script_public_key = o; }), Some(1));
        }
    }
    // Unchanged on purpose, and held here: of a declared close's late end and of an exit claim's early end, half
    // goes to the party the claim was made against, whatever keys the claimer ends it with.
    for name in ["late", "quiet-early"] {
        let s = the(&v, name);
        let n = s.outs.len();
        let t = s.tx();
        assert!(all_ok(&t), "{name}");
        assert_eq!(paid_to(&t, &w.a.x.b_key) as i64, half, "{name}: half to the other party");
        bad.must_fail(name, "the other party's half to the claimer's key", &s.with(|s| s.outs[n - 2] = pay_raw(&k, half)), Some(1));
        bad.must_fail(name, "the other party's half added to the claimer's output", &s.with(|s| { s.outs[n - 2] = change(1); s.outs[n - 1].value += half as u64; }), Some(1));
        bad.must_fail(name, "the other party a sompi short", &s.with(|s| s.outs[n - 2].value -= 1), Some(1));
    }
    bad.done("F2b the half of a bond");
}

// ================================================================ round 4: orders (KpfTMatch, KpfTCancel)

const UNIT: i64 = 1_000_000;
const PX: i64 = 1_000_000_000;
const IDL: Hash = Hash::from_bytes([0x61; 32]);
const IDS: Hash = Hash::from_bytes([0x62; 32]);

thread_local! {
    static SLOTS: std::cell::RefCell<std::collections::HashMap<[u8; 32], SilAbiArtifact>> = std::cell::RefCell::new(std::collections::HashMap::new());
    static LIBS: std::cell::RefCell<std::collections::HashMap<&'static str, SilAbiArtifact>> = std::cell::RefCell::new(std::collections::HashMap::new());
}
/// `tslot`, compiled once a hash.
fn oslot(h: [u8; 32]) -> SilAbiArtifact {
    SLOTS.with(|c| c.borrow_mut().entry(h).or_insert_with(|| tslot(h)).clone())
}
fn lib_of(name: &'static str) -> SilAbiArtifact {
    LIBS.with(|c| {
        c.borrow_mut()
            .entry(name)
            .or_insert_with(|| {
                // KpfTMerge takes two constructor constants: MERGE_TIP and the version.
                if name == "KpfTMerge" {
                    compile("KpfTMerge.sil", &[ArtifactValue::Int(kpf_protocol::thin::MERGE_TIP), ArtifactValue::Int(1)])
                } else if name == "KpfTNovate" {
                    compile("KpfTNovate.sil", &[ArtifactValue::Int(kpf_protocol::thin::NOVATE_TIP), ArtifactValue::Int(1)])
                } else if name == "KpfTJoin" {
                    compile("KpfTJoin.sil", &[ArtifactValue::Int(kpf_protocol::thin::MERGE_TIP), ArtifactValue::Int(1)])
                } else if name == "KpfTFold" {
                    compile("KpfTFold.sil", &[ArtifactValue::Int(kpf_protocol::thin::MERGE_TIP), ArtifactValue::Int(1)])
                } else if name == "KpfTLeave" {
                    compile("KpfTLeave.sil", &[ArtifactValue::Int(kpf_protocol::thin::NOVATE_TIP), ArtifactValue::Int(1)])
                } else {
                    thin(name)
                }
            })
            .clone()
    })
}
/// `KpfTMatch.share`.
fn share(v: i64, n: i64, size: i64) -> i64 {
    if size <= 0 {
        return 0;
    }
    (v / size) * n + ((v % size) * n) / size
}

/// An input of a transaction put together from several operations.
struct Part {
    at: usize,
    input: In,
    seq: u64,
    patch: Option<Box<dyn Fn(&[u8]) -> Vec<u8>>>,
}
/// The transaction of these inputs at their indices, funding inputs at the indices nobody took.
fn assemble(parts: Vec<Part>, n_in: usize, outs: Vec<TransactionOutput>, lock: u64) -> Tx {
    let mut slots: Vec<Option<Part>> = (0..n_in).map(|_| None).collect();
    for p in parts {
        let at = p.at;
        assert!(slots[at].is_none(), "two inputs at {at}");
        slots[at] = Some(p);
    }
    let (mut ins, mut seqs, mut patches) = (vec![], vec![], vec![]);
    for (i, s) in slots.into_iter().enumerate() {
        match s {
            Some(p) => {
                ins.push(p.input);
                seqs.push(p.seq);
                if let Some(f) = p.patch {
                    patches.push((i, f));
                }
            }
            None => {
                ins.push(funding(40 + i as u8));
                seqs.push(0);
            }
        }
    }
    let mut t = build_ext(&ins, outs, lock, &seqs);
    for (i, f) in patches {
        t.0.inputs[i].signature_script = f(&t.0.inputs[i].signature_script);
    }
    t
}

/// The outputs of a transaction being laid out: what stands at each index, and the objects written wherever there is
/// room, in their order.
struct Slots {
    v: Vec<Vec<(TransactionOutput, usize)>>,
    any: Vec<(TransactionOutput, usize)>,
    low: bool,
    off: bool,
    n_in: usize,
}
/// A laid-out transaction's outputs.
struct Out {
    outs: Vec<TransactionOutput>,
    /// An output stands at a place two payments name.
    shared: bool,
    /// A place counted from the end falls among the input indices.
    low: bool,
    /// For every output asked by one script alone: its index and the input of that script.
    asked: Vec<(usize, usize)>,
}
impl Slots {
    fn new(len: usize, n_in: usize) -> Slots {
        Slots { v: vec![vec![]; len], any: vec![], low: false, off: false, n_in }
    }
    fn len(&self) -> i64 {
        self.v.len() as i64
    }
    /// `o` at the index `at`, asked by the script at input `by`.
    fn put(&mut self, at: i64, o: TransactionOutput, by: usize) {
        if at < 0 || at >= self.len() {
            self.off = true;
        } else {
            self.v[at as usize].push((o, by));
        }
    }
    /// The same at a place counted from the end.
    fn end(&mut self, at: i64, o: TransactionOutput, by: usize) {
        if at < self.n_in as i64 {
            self.low = true;
        }
        self.put(at, o, by);
    }
    fn any(&mut self, o: TransactionOutput, by: usize) {
        self.any.push((o, by));
    }
    /// None if it cannot be written (a place outside the outputs, two different scripts at one index, no room).
    fn done(self) -> Option<Out> {
        if self.off {
            return None;
        }
        let (mut shared, mut asked) = (false, vec![]);
        let mut outs: Vec<Option<TransactionOutput>> = vec![];
        for (i, s) in self.v.iter().enumerate() {
            outs.push(match s.len() {
                0 => None,
                1 => {
                    asked.push((i, s[0].1));
                    Some(s[0].0.clone())
                }
                _ => {
                    if s.iter().any(|o| o.0.covenant.is_some() || o.0.script_public_key != s[0].0.script_public_key) {
                        return None;
                    }
                    shared = true;
                    Some(out(s.iter().map(|o| o.0.value).max().unwrap(), s[0].0.script_public_key.clone(), None))
                }
            });
        }
        let mut from = 0;
        for (a, by) in self.any {
            let i = (from..outs.len()).find(|i| outs[*i].is_none())?;
            outs[i] = Some(a);
            asked.push((i, by));
            from = i + 1;
        }
        Some(Out { outs: outs.into_iter().map(|o| o.unwrap_or_else(|| change(1))).collect(), shared, low: self.low, asked })
    }
}

/// A match as a transaction, every field a knob: the two order objects as they are on the chain, what the script is
/// shown of them, the inputs they stand at.
#[derive(Clone)]
struct Mx {
    lib: SilAbiArtifact,
    tpl: Vec<u8>,
    /// The two objects: (the template's hash, the owner, dir, size, limit, margin, tip, expiry, rest, least, ask).
    lobj: (Vec<u8>, Vec<u8>, [i64; 8]),
    sobj: (Vec<u8>, Vec<u8>, [i64; 8]),
    /// An object of another hash in the place of the long order, the short order.
    lhash: Option<[u8; 32]>,
    shash: Option<[u8; 32]>,
    lval: i64,
    sval: i64,
    idl: Hash,
    ids: Hash,
    /// What the script is shown of each: the key, and size, limit, margin, tip, expiry, rest, least, ask.
    lk: Vec<u8>,
    la: [i64; 7],
    sk: Vec<u8>,
    sa: [i64; 7],
    n: i64,
    x: i64,
    /// The inputs of the long order, the short order and the script.
    at: [usize; 3],
    n_in: usize,
    who: Vec<u8>,
    go: [(i64, i64); 2],
    edits: Vec<(usize, Vec<u8>)>,
}
/// What the script computes of a match.
struct Exp {
    am: i64,
    bm: i64,
    lf: i64,
    sf: i64,
    lv: i64,
    sv: i64,
    lkeep: bool,
    skeep: bool,
    position: [u8; 32],
    lrest: [u8; 32],
    srest: [u8; 32],
}
fn tail8(v: &[i64; 8]) -> [i64; 7] {
    [v[1], v[2], v[3], v[4], v[5], v[6], v[7]]
}
fn mx(tpl: &[u8], long: &Order, short: &Order, n: i64, x: i64, at: [usize; 3]) -> Mx {
    let (li, si) = (order_ints(long), order_ints(short));
    Mx {
        lib: lib_of("KpfTMatch"),
        tpl: tpl.to_vec(),
        lobj: (long.tpl.to_vec(), long.owner.to_vec(), li),
        sobj: (short.tpl.to_vec(), short.owner.to_vec(), si),
        lhash: None,
        shash: None,
        lval: long.margin + long.tip,
        sval: short.margin + short.tip,
        idl: IDL,
        ids: IDL,
        lk: long.owner.to_vec(),
        la: tail8(&li),
        sk: short.owner.to_vec(),
        sa: tail8(&si),
        n,
        x,
        at,
        n_in: 4,
        who: who_of([at[0] as i64, at[1] as i64, at[1] as i64]),
        go: [(at[2] as i64, 0), (at[2] as i64, 1)],
        edits: vec![],
    }
}
impl Mx {
    /// The object and what the script is shown, changed together.
    fn long(&self, f: impl FnOnce(&mut [i64; 8])) -> Mx {
        let mut k = self.clone();
        f(&mut k.lobj.2);
        k.la = tail8(&k.lobj.2);
        k.lval = k.lobj.2[3] + k.lobj.2[4];
        k
    }
    fn short(&self, f: impl FnOnce(&mut [i64; 8])) -> Mx {
        let mut k = self.clone();
        f(&mut k.sobj.2);
        k.sa = tail8(&k.sobj.2);
        k.sval = k.sobj.2[3] + k.sobj.2[4];
        k
    }
    fn with(&self, f: impl FnOnce(&mut Mx)) -> Mx {
        let mut k = self.clone();
        f(&mut k);
        k
    }
    fn cut(&self, a: usize, b: usize) -> &[u8] {
        &self.tpl[a.min(self.tpl.len())..b.min(self.tpl.len())]
    }
    fn exp(&self) -> Exp {
        let [ls, ll, lm, lt, le, ln, la] = self.la;
        let [ss, sl, sm, st, se, sn, sa] = self.sa;
        let n = self.n;
        let (am, bm, lf, sf) = (share(lm, n, ls), share(sm, n, ss), share(lt, n, ls), share(st, n, ss));
        let fixed = [self.cut(0, 174), &push9(n)[..], &push9(LONG), &push9(self.x), self.cut(174, 285)].concat();
        let position = blake2b(&[push33(&blake2b(&fixed)), p33b(&self.lk), p33b(&self.sk), push9(am), push9(bm), self.cut(285, 321).to_vec(), push9(share(la, n, ls)), push9(share(sa, n, ss))].concat());
        let th = blake2b(&self.tpl);
        Exp {
            am,
            bm,
            lf,
            sf,
            lv: self.lval - am - lf,
            sv: self.sval - bm - sf,
            lkeep: ls - n > 0,
            skeep: ss - n > 0,
            position,
            lrest: h_order(&th, &self.lk, &[LONG, ls - n, ll, lm - am, lt - lf, le, ln, la - share(la, n, ls)]),
            srest: h_order(&th, &self.sk, &[SHORT, ss - n, sl, sm - bm, st - sf, se, sn, sa - share(sa, n, ss)]),
        }
    }
    /// What the script asks of the outputs, at its places.
    fn place(&self, s: &mut Slots) {
        let e = self.exp();
        let [j1, j2, js] = self.at;
        let len = s.len();
        let v = |x: i64| x.max(0) as u64;
        s.put(js as i64, out(LIBV, p2sh(&self.lib), bind(js as u16, LIB)), js);
        // (Round 11: nothing is paid to a key by a match. The tips, and what an order used up held beyond its margin
        // and tip, are in no output the script asks: they are the sender's.)
        let _ = len;
        s.any(out(v(e.am + e.bm), spk(e.position), bind(j1 as u16, self.idl)), js);
        if e.lkeep {
            s.any(out(v(e.lv), spk(e.lrest), bind(j1 as u16, self.idl)), js);
        }
        if e.skeep {
            s.any(out(v(e.sv), spk(e.srest), bind(j2 as u16, self.ids)), js);
        }
    }
    fn honest(&self, len: usize) -> Option<Out> {
        let mut s = Slots::new(len, self.n_in);
        self.place(&mut s);
        s.done()
    }
    fn patch(&self, script: &[u8]) -> Vec<u8> {
        let mut s = repush(script, 0, &self.who);
        for (i, b) in [(3, &self.tpl), (4, &self.lk), (12, &self.sk)] {
            s = repush(&s, i, b);
        }
        for (i, b) in &self.edits {
            s = repush(&s, *i, b);
        }
        s
    }
    fn parts(&self) -> Vec<Part> {
        let int = |v: i64| Arg::V(ArtifactValue::Int(v));
        let lo = oslot(self.lhash.unwrap_or_else(|| h_order(&self.lobj.0, &self.lobj.1, &self.lobj.2)));
        let so = oslot(self.shash.unwrap_or_else(|| h_order(&self.sobj.0, &self.sobj.1, &self.sobj.2)));
        let mut args = vec![by(&[0; 24]), by(&[0; 8]), int(slot_len()), by(&[0; 321]), by(&[0; 32])];
        args.extend(self.la.iter().map(|v| int(*v)));
        args.push(by(&[0; 32]));
        args.extend(self.sa.iter().map(|v| int(*v)));
        args.extend([int(self.n), int(self.x)]);
        let me = self.clone();
        let obj = |n: u8, art: SilAbiArtifact, value: i64, id: Hash, g: (i64, i64)| In { outpoint: outpoint(n), utxo: UtxoEntry::new(value.max(0) as u64, p2sh(&art), 6_200, false, Some(id)), spend: Spend::Signed { art, entry: "go", args: vec![int(g.0), int(g.1)] } };
        vec![
            Part { at: self.at[0], input: obj(60, lo, self.lval, self.idl, self.go[0]), seq: 0, patch: None },
            Part { at: self.at[1], input: obj(61, so, self.sval, self.ids, self.go[1]), seq: 0, patch: None },
            Part { at: self.at[2], input: In { outpoint: outpoint(62), utxo: UtxoEntry::new(KAS1, p2sh(&self.lib), 90, false, Some(LIB)), spend: Spend::Signed { art: self.lib.clone(), entry: "match", args } }, seq: 0, patch: Some(Box::new(move |s| me.patch(s))) },
        ]
    }
    fn tx_with(&self, outs: Vec<TransactionOutput>) -> Tx {
        assemble(self.parts(), self.n_in, outs, 0)
    }
    /// The transaction with what the script asks at its places, among 12 outputs.
    fn tx(&self) -> Tx {
        self.tx_with(self.honest(self.n_in + 8).expect("a layout").outs)
    }
}

/// The inputs of a scene's operation at other indices, and what it asks of the outputs.
fn scene_parts(s: &Scene, j: usize, l: usize, n: u8) -> Vec<Part> {
    let s = s.with(|x| { x.who = who_of([j as i64; 3]); x.go = (l as i64, 0); });
    let me = s.clone();
    vec![Part { at: j, input: s.obj_in(n), seq: s.seq, patch: None }, Part { at: l, input: s.lib_in(n + 1), seq: s.lib_seq, patch: Some(Box::new(move |x| me.patch(x))) }]
}
fn place_scene(sc: &Sc, j: usize, l: usize, s: &mut Slots) {
    let len = s.len();
    s.put(l as i64, out(sc.s.outs[1].value, p2sh(&sc.s.lib), bind(l as u16, LIB)), l);
    for (place, o) in reqs(sc) {
        match place {
            Place::Front => s.put(j as i64, o, l),
            Place::End(k) => s.end(len - 1 - 2 * j as i64 - k as i64, o, l),
            Place::Any => s.any(out(o.value, o.script_public_key.clone(), bind(j as u16, sc.s.obj_id)), l),
        }
    }
}

fn tpl_of() -> Template {
    Template::of(&world().a)
}
fn ord(owner: &Keypair, dir: i64, size: i64, limit: i64, margin: i64, tip: i64, _rested: bool, min_fill: i64) -> Order {
    // The ask defaults to 0 (no margin asked of the other side), so the amount and place tests match freely; the
    // F6 tests set the ask with `with_ask`.
    Order { tpl: tpl_of().hash(), owner: xkey(owner), dir, size, limit, margin, tip, expiry: 9_000, min_fill, min_counter: 0 }
}
fn with_ask(o: Order, min_counter: i64) -> Order {
    Order { min_counter, ..o }
}
/// (what, the long order and what it holds, the short one and what it holds, the size matched)
fn match_cases() -> Vec<(&'static str, Order, i64, Order, i64, i64)> {
    let l = |units: i64, rest: bool, tip: i64| ord(&owner(), LONG, units * UNIT, PX + PX / 10, 40 * KAS, tip, rest, 0);
    let s = |units: i64, rest: bool, tip: i64| ord(&other(), SHORT, units * UNIT, PX - PX / 20, 60 * KAS, tip, rest, 0);
    let v = |o: &Order| o.margin + o.tip;
    let odd_l = ord(&owner(), LONG, 7_000_003, PX + PX / 10, 40 * KAS + 1, 333_333_331, true, 0);
    let odd_s = ord(&other(), SHORT, 5_000_011, PX - PX / 20, 60 * KAS + 7, 77_777_777, false, 0);
    vec![
        ("both rest", l(10, true, KAS / 2), v(&l(10, true, KAS / 2)), s(4, true, KAS / 3), v(&s(4, true, KAS / 3)), 3 * UNIT),
        ("the short filled whole, the long rests", l(10, true, KAS / 2), v(&l(10, true, KAS / 2)), s(4, true, KAS / 3), v(&s(4, true, KAS / 3)), 4 * UNIT),
        ("the long filled whole, the short rests", l(3, true, KAS / 2), v(&l(3, true, KAS / 2)), s(4, true, KAS / 3), v(&s(4, true, KAS / 3)), 3 * UNIT),
        ("both rest, no tips", l(10, true, 0), v(&l(10, true, 0)), s(4, true, 0), v(&s(4, true, 0)), 3 * UNIT),
        ("both filled whole", l(4, true, KAS / 2), v(&l(4, true, KAS / 2)), s(4, true, KAS / 3), v(&s(4, true, KAS / 3)), 4 * UNIT),
        ("both filled whole, no tips", l(4, true, 0), v(&l(4, true, 0)), s(4, true, 0), v(&s(4, true, 0)), 4 * UNIT),
        ("both filled whole, each holding more than its margin and tip", l(4, true, KAS / 2), v(&l(4, true, KAS / 2)) + 12_345, s(4, true, KAS / 3), v(&s(4, true, KAS / 3)) + 1, 4 * UNIT),
        ("sizes and amounts that do not divide", odd_l, v(&odd_l), odd_s, v(&odd_s), 1_234_567),
    ]
}
fn arrangements(n: usize) -> Vec<[usize; 3]> {
    let mut v = vec![];
    for a in 0..n {
        for b in 0..n {
            for c in 0..n {
                if a != b && b != c && a != c {
                    v.push([a, b, c]);
                }
            }
        }
    }
    v
}
fn failed_inputs(t: &Tx) -> Vec<usize> {
    let r = run(&t.0, &t.1).expect("bindings");
    (0..r.len()).filter(|i| r[*i].is_err()).collect()
}

/// O1, O4. A match gives each owner what the model says and nobody else anything, wherever its three inputs stand
/// among four: the position's margins are the two shares, what leaves each order is what it held, every output a sompi
/// short fails the script.
#[test]
fn t1b_o1_a_match_pays_what_the_model_says_wherever_its_inputs_stand() {
    let tpl = tpl_of();
    assert_eq!(Template::from_bytes(&tpl.bytes()).unwrap(), tpl);
    let tb = tpl.bytes();
    let (mut txs, mut shorts) = (0, 0);
    for (what, lo, lval, so, sval, n) in match_cases() {
        let m = match_orders(&tpl, &lo, lval, &so, sval, n, PX).unwrap_or_else(|e| panic!("{what}: {e}"));
        for at in arrangements(4) {
            let k = mx(&tb, &lo, &so, n, PX, at).with(|k| { k.lval = lval; k.sval = sval; });
            let e = k.exp();
            // The script's arithmetic written out here is the model's.
            assert_eq!((e.position, e.am, e.bm, e.lf + e.sf), (m.position.hash(), m.position.x.a_margin, m.position.x.b_margin, m.tips), "{what}");
            for (rest, keep, left, hash) in [(m.long, e.lkeep, e.lv, e.lrest), (m.short, e.skeep, e.sv, e.srest)] {
                match rest {
                    Rest::Order(o, v) => assert_eq!((keep, left, hash), (true, v, o.hash()), "{what}"),
                    // An order used up: what it held beyond its margin and tip (`left`) is in no output: the sender's.
                    Rest::Nothing => assert!(!keep && left >= 0, "{what}"),
                }
            }
            // What leaves each order is what it held.
            assert_eq!((e.am + e.lf + e.lv, e.bm + e.sf + e.sv), (lval, sval), "{what}");
            let o = k.honest(12).expect("a layout");
            assert!(!o.shared && !o.low);
            let t = k.tx_with(o.outs.clone());
            assert!(all_ok(&t), "{what}, inputs {at:?}: {}", show(&t));
            txs += 1;
            let js = at[2];
            let total: i64 = o.outs.iter().enumerate().filter(|(i, x)| *i != js && !is_change(x)).map(|(_, x)| x.value as i64).sum();
            assert_eq!(total, e.am + e.bm + if e.lkeep { e.lv } else { 0 } + if e.skeep { e.sv } else { 0 }, "{what}: the outputs the script asks are the position and what rests");
            assert_eq!(lval + sval - total, m.tips + if e.lkeep { 0 } else { e.lv } + if e.skeep { 0 } else { e.sv }, "{what}: the sender has the tips and what an order used up held beyond its margin and tip");
            for (i, _) in o.asked.iter().filter(|(i, _)| o.outs[*i].value > 0) {
                let mut short = o.outs.clone();
                short[*i].value -= 1;
                assert_eq!(failed_inputs(&k.tx_with(short)), vec![js], "{what}, inputs {at:?}: output {i} a sompi short");
                shorts += 1;
            }
        }
    }
    println!("T1b O1: {txs} matches accepted (8 cases, 24 arrangements of the inputs), {shorts} outputs a sompi short refused");
}

/// O1, O8 through the engine: random orders, random sizes matched, at the script's own places.
#[test]
fn t1b_o1_random_matches_are_the_models() {
    let tpl = tpl_of();
    let tb = tpl.bytes();
    let mut seed = 0x51ab_0c0f_fee0_0004u64;
    let mut r = |lo: i64, hi: i64| -> i64 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        lo + (seed % (hi - lo) as u64) as i64
    };
    let (mut accepted, mut refused) = (0, 0);
    while accepted < 60 {
        let (ls, ss) = (r(1, 2_000_000_001), r(1, 2_000_000_001));
        let lo = ord(&owner(), LONG, ls, PX + r(0, PX), r(1, 5_000 * KAS), r(0, 3 * KAS), r(0, 2) == 1, r(0, ls + 1) / 3);
        let so = ord(&other(), SHORT, ss, PX - r(0, PX / 2), r(1, 5_000 * KAS), r(0, 3 * KAS), r(0, 2) == 1, r(0, ss + 1) / 3);
        let n = if r(0, 3) == 0 { ls.min(ss) } else { r(1, ls.min(ss) + 1) };
        let (lval, sval) = (lo.margin + lo.tip + if r(0, 4) == 0 { r(0, KAS) } else { 0 }, so.margin + so.tip + if r(0, 4) == 0 { r(0, KAS) } else { 0 });
        let k = mx(&tb, &lo, &so, n, PX, [0, 1, 2]).with(|k| { k.lval = lval; k.sval = sval; });
        let model = match_orders(&tpl, &lo, lval, &so, sval, n, PX);
        let t = k.tx();
        let e = k.exp();
        match &model {
            Ok(m) => {
                assert!(all_ok(&t), "{lo:?} {so:?} {n}: the model takes it: {}", show(&t));
                assert_eq!((e.position, e.lf + e.sf), (m.position.hash(), m.tips));
                assert_eq!((e.am + e.lf + e.lv, e.bm + e.sf + e.sv), (lval, sval));
                accepted += 1;
            }
            // The model refuses more than the script only for what the model's own position check refuses (a pot too
            // small for a bond): whatever the script takes that the model does not is looked at below.
            Err(why) => {
                if all_ok(&t) {
                    assert!(tpl.position(lo.owner, so.owner, n, PX, e.am, e.bm).check().is_err(), "{lo:?} {so:?} {n}: the script takes what the model refuses: {why}");
                }
                refused += 1;
            }
        }
    }
    println!("T1b O1 random: {accepted} matches the model takes accepted at the model's amounts, {refused} it refuses");
}

/// O2. A position is made only with both orders' consent.
#[test]
fn t1b_o2_a_match_is_within_what_both_orders_say() {
    let tpl = tpl_of();
    let tb = tpl.bytes();
    let (_, lo, _, so, _, n) = match_cases().remove(0);
    let base = mx(&tb, &lo, &so, n, PX, [0, 1, 2]);
    assert!(all_ok(&base.tx()), "{}", show(&base.tx()));
    let mut bad: Vec<String> = vec![];
    let mut tried = 0;
    let mut no = |what: &str, k: &Mx| {
        tried += 1;
        if all_ok(&k.tx()) {
            bad.push(format!("accepted: {what}"));
        }
    };
    let yes = |what: &str, k: &Mx| assert!(all_ok(&k.tx()), "{what} (should be accepted): {}", show(&k.tx()));
    let pmax = tpl.price_max;
    // The price: within the long's limit, the short's, the market's.
    no("a sompi above the long's limit", &base.with(|k| k.x = lo.limit + 1));
    no("a sompi below the short's limit", &base.with(|k| k.x = so.limit - 1));
    no("a price of nothing", &base.with(|k| k.x = 0));
    no("a negative price", &base.with(|k| k.x = -PX));
    no("above the market's limit", &base.long(|o| o[2] = pmax + 10).with(|k| k.x = pmax + 1));
    yes("at the long's limit", &base.with(|k| k.x = lo.limit));
    yes("at the short's limit", &base.with(|k| k.x = so.limit));
    yes("at the market's limit", &base.long(|o| o[2] = pmax + 10).with(|k| k.x = pmax));
    // The size: within what both have left.
    no("more than the short has", &base.with(|k| k.n = so.size + 1));
    no("more than the long has", &base.short(|o| o[1] = 20 * UNIT).with(|k| k.n = lo.size + 1));
    no("a size of nothing", &base.with(|k| k.n = 0));
    no("a negative size", &base.with(|k| k.n = -UNIT));
    // The least fill of each: at least it, or all that is left.
    no("below the long's least fill", &base.long(|o| o[6] = 5 * UNIT));
    no("below the short's least fill", &base.short(|o| o[6] = 4 * UNIT));
    yes("the short's least fill exactly", &base.short(|o| o[6] = 3 * UNIT));
    yes("below the long's least fill, all that is left of it", &base.long(|o| { o[1] = 3 * UNIT; o[6] = 5 * UNIT; }));
    yes("below the short's least fill, all that is left of it", &base.short(|o| o[6] = 9 * UNIT).with(|k| k.n = so.size));
    // A size above the script's most.
    no("a long order of 2,000,000,001", &base.long(|o| o[1] = 2_000_000_001));
    no("a short order of 2,000,000,001", &base.short(|o| o[1] = 2_000_000_001));
    yes("a long order of 2,000,000,000", &base.long(|o| o[1] = 2_000_000_000));
    // A share of margin that is nothing (O8: no tip without a position).
    no("a share of the long's margin that rounds to nothing", &base.long(|o| o[3] = 5).with(|k| k.n = 1));
    no("a share of the short's margin that rounds to nothing", &base.short(|o| o[3] = 3).with(|k| k.n = 1));
    no("a long order without margin", &base.long(|o| o[3] = 0).with(|k| k.lval = KAS));
    // Another template, another side.
    let mut t2 = tpl;
    t2.dispute_daa += 1;
    let tb2 = t2.bytes();
    no("a short order of another template", &base.with(|k| k.sobj.0 = blake2b(&tb2).to_vec()));
    no("a short order of another template, that template shown", &base.with(|k| { k.sobj.0 = blake2b(&tb2).to_vec(); k.tpl = tb2.clone(); }));
    no("two orders of another template than the one shown", &base.with(|k| k.tpl = tb2.clone()));
    no("a short order in the long's place", &base.with(|k| k.lobj.2[0] = SHORT));
    no("a long order in the short's place", &base.with(|k| k.sobj.2[0] = LONG));
    no("the two orders in each other's place", &base.with(|k| std::mem::swap(&mut k.lobj, &mut k.sobj)));
    no("the two orders in each other's place, shown so", &base.with(|k| { std::mem::swap(&mut k.lobj, &mut k.sobj); std::mem::swap(&mut k.lk, &mut k.sk); std::mem::swap(&mut k.la, &mut k.sa); }));
    no("a side that is neither", &base.with(|k| k.lobj.2[0] = 3));
    // What the script is shown is what the orders carry.
    for i in 0..7 {
        for d in [-1, 1] {
            no(&format!("the long's value {i} shown {d} off"), &base.with(|k| k.la[i] += d));
            no(&format!("the short's value {i} shown {d} off"), &base.with(|k| k.sa[i] += d));
        }
    }
    no("the long's key another's", &base.with(|k| k.lk = xkey(&keypair(45)).to_vec()));
    no("the short's key another's", &base.with(|k| k.sk = xkey(&keypair(45)).to_vec()));
    no("the two keys in each other's place", &base.with(|k| std::mem::swap(&mut k.lk, &mut k.sk)));
    // An object that is no order, of every kind, in either place.
    let w = world();
    for kind in KINDS.iter().filter(|k| **k != K::Order) {
        no(&format!("a {kind:?} in the long's place"), &base.with(|k| k.lhash = Some(w.hash(*kind))));
        no(&format!("a {kind:?} in the short's place"), &base.with(|k| k.shash = Some(w.hash(*kind))));
    }
    // Who: the long order, the short order, the short again, and nothing else over the four inputs and one past them.
    for a in 0..=4i64 {
        for b in 0..=4i64 {
            for c in 0..=4i64 {
                if [a, b, c] != [0, 1, 1] {
                    no(&format!("who {:?}", [a, b, c]), &base.with(|k| k.who = who_of([a, b, c])));
                }
            }
        }
    }
    no("the two orders under two ids (B2: an order written beside the registry)", &base.with(|k| k.ids = IDS));
    // Widths: the template and the keys at other widths, the orders' hashes following the bytes shown.
    for t in [[&tb[..], &[0u8][..]].concat(), tb[..320].to_vec(), [&tb[..], &tb[..]].concat(), vec![]] {
        no(&format!("a template of {} bytes", t.len()), &base.with(|k| { k.lobj.0 = blake2b(&t).to_vec(); k.sobj.0 = blake2b(&t).to_vec(); k.tpl = t.clone(); }));
        no(&format!("a template of {} bytes on the real orders", t.len()), &base.with(|k| k.tpl = t.clone()));
    }
    for key in [[&base.lk[..], &[0u8][..]].concat(), base.lk[..31].to_vec(), vec![]] {
        no(&format!("a long's key of {} bytes", key.len()), &base.with(|k| { k.lobj.1 = key.clone(); k.lk = key.clone(); }));
        no(&format!("a short's key of {} bytes", key.len()), &base.with(|k| { k.sobj.1 = key.clone(); k.sk = key.clone(); }));
    }
    no("who of 25 bytes", &base.with(|k| k.who.push(0)));
    no("an age of 9 bytes", &base.with(|k| k.edits = vec![(1, vec![0; 9])]));
    no("a negative age", &base.with(|k| k.edits = vec![(1, sm8(-1).to_vec())]));
    // The script going on (A3): its copy a sompi short, under no id, replaced, an output more under the library's id.
    let o = base.honest(12).unwrap();
    let alter = |f: &dyn Fn(&mut Vec<TransactionOutput>)| {
        let mut outs = o.outs.clone();
        f(&mut outs);
        all_ok(&base.tx_with(outs))
    };
    let free = (0..o.outs.len()).find(|i| is_change(&o.outs[*i])).unwrap();
    assert!(!alter(&|x| x[2].value -= 1), "the script a sompi short");
    assert!(!alter(&|x| x[2].covenant = None), "the script under no id");
    assert!(!alter(&|x| x[2].script_public_key = p2sh(&lib_of("KpfTBoth"))), "another script in its place");
    assert!(!alter(&|x| x[free] = out(LIBV, p2sh(&lib_of("KpfTMatch")), bind(2, LIB))), "a second copy under the library's id");
    assert!(alter(&|x| x[2].value += 1), "the script a sompi richer");
    drop(no);
    println!("T1b O2: {tried} matches outside what the orders say, all refused but {}", bad.len());
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// O2, what the script does not ask and the model or the tool does. Two orders of one owner match into a position
/// whose two keys are one (the model refuses it; nobody else's money is in it). Any 321 bytes both orders name are a
/// template to the script (the tool reads a template back and refuses what is not one; an honest order's hash is
/// never that of other bytes).
#[test]
fn t1b_design_what_a_match_leaves_to_the_orders_owners() {
    let tpl = tpl_of();
    let tb = tpl.bytes();
    let (_, lo, lval, so, sval, n) = match_cases().remove(0);
    let mine = Order { owner: lo.owner, ..so };
    assert!(match_orders(&tpl, &lo, lval, &mine, sval, n, PX).is_err(), "the model refuses two orders of one owner");
    let k = mx(&tb, &lo, &mine, n, PX, [0, 1, 2]);
    assert!(all_ok(&k.tx()), "the script takes two orders of one owner: {}", show(&k.tx()));
    // 321 bytes that are no template: the price limit the script reads at 166 set high, the rest noise.
    let mut junk: Vec<u8> = (0..321u32).map(|i| (i * 131 % 251) as u8).collect();
    junk[166..174].copy_from_slice(&sm8(1 << 50));
    assert!(Template::from_bytes(&junk).is_err());
    let k = mx(&tb, &lo, &so, n, PX, [0, 1, 2]).with(|k| { k.tpl = junk.clone(); k.lobj.0 = blake2b(&junk).to_vec(); k.sobj.0 = blake2b(&junk).to_vec(); });
    assert!(all_ok(&k.tx()), "the script takes any 321 bytes both orders name");
}

/// O3 (round 8: one id, the registry's). What a match writes under the registry's id is the position, then what rests of the
/// long order, then what rests of the short, and nothing else; and no third object of the registry is spent beside a
/// match, nor two objects of it by two single-object entries.
#[test]
fn t1b_o3_a_match_writes_under_the_registry_exactly_what_its_rule_says() {
    let tpl = tpl_of();
    let tb = tpl.bytes();
    let cases = match_cases();
    let mut tried = 0;
    // (the case, whether the long rests, whether the short rests)
    for (ci, lrest, srest) in [(0usize, true, true), (1, true, false), (2, false, true), (4, false, false)] {
        let (what, lo, _, so, _, n) = cases[ci];
        let k = mx(&tb, &lo, &so, n, PX, [0, 1, 2]);
        let e = k.exp();
        assert_eq!((e.lkeep, e.skeep), (lrest, srest));
        let o = k.honest(12).unwrap();
        assert!(all_ok(&k.tx_with(o.outs.clone())), "{what}");
        let at = |h: [u8; 32]| o.outs.iter().position(|x| x.script_public_key == spk(h));
        let (ia, il, is) = (at(e.position).unwrap(), if lrest { at(e.lrest) } else { None }, if srest { at(e.srest) } else { None });
        let free: Vec<usize> = (0..o.outs.len()).filter(|i| is_change(&o.outs[*i]) && *i >= 4).collect();
        let mut no = |why: &str, f: &dyn Fn(&mut Vec<TransactionOutput>)| {
            let mut outs = o.outs.clone();
            f(&mut outs);
            let t = k.tx_with(outs);
            assert!(refused(&t), "{what}: {why}: {}", show(&t));
            tried += 1;
        };
        let position = o.outs[ia].clone();
        no("no position", &|x| x[ia] = change(1));
        no("the position under no id", &|x| x[ia].covenant = None);
        no("the position under another id", &|x| x[ia].covenant = bind(0, IDS));
        no("the position written twice", &|x| x[free[0]] = position.clone());
        no("a key's output under the registry's id", &|x| x[free[0]].covenant = bind(0, IDL));
        // B1: a position nobody signed written beside, under the registry's id, authorized by any input.
        for auth in 0..4u16 {
            no(&format!("a position nobody signed under the registry's id, by input {auth}"), &|x| x[free[0]] = out(KAS1, spk([0x44; 32]), bind(auth, IDL)));
            no(&format!("a position nobody signed in the place of the position, by input {auth}"), &|x| { x[ia].script_public_key = spk([0x44; 32]); x[ia].covenant = bind(auth, IDL); });
        }
        if let Some(il) = il {
            let rest = o.outs[il].clone();
            no("what rests of the long written before the position", &|x| x.swap(ia, il));
            no("what rests of the long not written", &|x| x[il] = change(1));
            no("what rests of the long written twice", &|x| x[free[0]] = rest.clone());
            no("the position in the place of what rests", &|x| x[il] = position.clone());
            no("what rests paid to its owner instead", &|x| { x[il] = pay_raw(&k.lk, e.lv); });
        } else {
            no("an order of the long's that does not rest", &|x| x[free[0]] = out(KAS1, spk(e.lrest), bind(0, IDL)));
        }
        if let Some(is) = is {
            let rest = o.outs[is].clone();
            no("what rests of the short not written", &|x| x[is] = change(1));
            no("what rests of the short written twice", &|x| x[free[0]] = rest.clone());
            no("what rests of the short before the position", &|x| x.swap(ia, is));
            if let Some(il) = il {
                no("the two rests in each other's place", &|x| x.swap(il, is));
            }
        } else {
            no("an order of the short's that does not rest", &|x| x[free[0]] = out(KAS1, spk(e.srest), bind(1, IDL)));
        }
    }
    // A third object of the registry in the same transaction: every scene's object put under the registry's id, beside a
    // match, and beside the cancel and the expiry of an order of the registry. Alone each is accepted; together nothing.
    let w = world();
    let v = scenes(&w);
    let (_, lo, _, so, _, n) = cases[0];
    let k = mx(&tb, &lo, &so, n, PX, [0, 1, 2]).with(|k| k.n_in = 6);
    let in_registry = |y: &Sc| Sc {
        name: y.name,
        s: y.s.with(|s| {
            s.obj_id = IDL;
            for o in s.outs.iter_mut() {
                if o.covenant.is_some_and(|c| c.covenant_id == XID) {
                    o.covenant = bind(0, IDL);
                }
            }
        }),
        kind: y.kind,
        dispute_daa: y.dispute_daa,
    };
    let mut together = 0;
    for y in &v {
        let ys = in_registry(y);
        assert!(all_ok(&ys.s.tx()), "{} alone under the registry's id", y.name);
        for len in [12usize, 14] {
            let mut s = Slots::new(len, 6);
            k.place(&mut s);
            place_scene(&ys, 3, 4, &mut s);
            let Some(o) = s.done() else { continue };
            let mut parts = k.parts();
            parts.extend(scene_parts(&ys.s, 3, 4, 70));
            let t = assemble(parts, 6, o.outs, ys.s.lock);
            assert!(refused(&t), "a match beside {} of an object of the same registry: {}", y.name, show(&t));
            together += 1;
        }
        for end in ["order-cancel", "order-expire", "add"] {
            let cs = in_registry(v.iter().find(|s| s.name == end).unwrap());
            let mut s = Slots::new(12, 5);
            place_scene(&cs, 0, 1, &mut s);
            place_scene(&ys, 2, 3, &mut s);
            let Some(o) = s.done() else { continue };
            let mut parts = scene_parts(&cs.s, 0, 1, 80);
            parts.extend(scene_parts(&ys.s, 2, 3, 70));
            let t = assemble(parts, 5, o.outs, cs.s.lock.max(ys.s.lock));
            assert!(refused(&t), "{end} beside {} of an object of the same registry: {}", y.name, show(&t));
            together += 1;
        }
    }
    println!("T1b O3: {tried} wrong writings under the registry's id refused; {together} transactions spending objects of one registry by two operations refused ({} scenes, each accepted alone)", v.len());
    assert!(together >= 3 * v.len());
}

/// O4 (round 11). A match asks no place counted from the end any more: with one key owning both orders, at every
/// arrangement of the inputs and from 5 to 14 outputs, it is taken wherever the position and the two rests fit, and
/// nothing is paid to a key.
#[test]
fn t1b_o4_a_match_asks_no_place_from_the_end() {
    let tb = tpl_of().bytes();
    let me = owner();
    let lo = ord(&me, LONG, 10 * UNIT, PX + PX / 10, 40 * KAS, KAS / 2, true, 0);
    let so = ord(&me, SHORT, 4 * UNIT, PX - PX / 20, 60 * KAS, KAS / 3, true, 0);
    let mut accepted = 0;
    for at in arrangements(4) {
        for len in 4..=14 {
            let k = mx(&tb, &lo, &so, 3 * UNIT, PX, at);
            let Some(o) = k.honest(len) else { continue };
            assert!(!o.shared && !o.low);
            let t = k.tx_with(o.outs.clone());
            assert!(all_ok(&t), "inputs {at:?}, {len} outputs: {}", show(&t));
            assert_eq!(paid_to(&t, &lo.owner), 0, "nothing is paid to an owner by a match");
            accepted += 1;
        }
    }
    println!("T1b O4: {accepted} layouts of a match accepted; no output is paid to a key");
    assert!(accepted > 24);
}

/// O4, F1 again with a match as one of two operations: beside each of the thirteen entries that pay a key, every key
/// the same, at seven arrangements of the six inputs and 6 to 17 outputs. Accepted exactly when every payment has its
/// own output and no place from the end falls among the inputs; then each output a sompi short fails the one script
/// that asked it.
#[test]
fn t1b_o4_a_match_beside_another_operation_shares_no_output() {
    let w = one_key_world();
    let v: Vec<Sc> = scenes(&w).into_iter().filter(|s| s.name != "both" && reqs(s).iter().any(|(p, _)| *p != Place::Any)).collect();
    let tb = tpl_of().bytes();
    let me = owner();
    let lo = ord(&me, LONG, 10 * UNIT, PX + PX / 10, 40 * KAS, KAS / 2, true, 0);
    let so = ord(&me, SHORT, 4 * UNIT, PX - PX / 20, 60 * KAS, KAS / 3, true, 0);
    let (mut accepted, mut refused_n, mut shared_n, mut probes) = (0, 0, 0, 0);
    let mut bad = vec![];
    for y in &v {
        // (the match's long order, short order and script; the other operation's object and script); funding last.
        for (at, j, l) in [([0, 1, 2], 3, 4), ([1, 0, 2], 3, 4), ([0, 2, 1], 3, 4), ([3, 4, 2], 0, 1), ([0, 3, 1], 2, 4), ([2, 0, 4], 1, 3), ([4, 1, 0], 2, 3)] {
            let k = mx(&tb, &lo, &so, 3 * UNIT, PX, at).with(|k| k.n_in = 6);
            let build = |len: usize, short: Option<usize>| -> Option<(Tx, Out)> {
                let mut s = Slots::new(len, 6);
                k.place(&mut s);
                place_scene(y, j, l, &mut s);
                let mut o = s.done()?;
                if let Some(i) = short {
                    o.outs[i].value -= 1;
                }
                let mut parts = k.parts();
                parts.extend(scene_parts(&y.s, j, l, 70));
                Some((assemble(parts, 6, o.outs.clone(), y.s.lock), o))
            };
            let mut once = false;
            for len in 6..=17 {
                let Some((t, o)) = build(len, None) else { continue };
                let ok = all_ok(&t);
                if o.shared || o.low {
                    if ok {
                        bad.push(format!("a match at {at:?} beside {} at {j}, {len} outputs: accepted with {}", y.name, if o.shared { "one output for two payments" } else { "a place among the inputs" }));
                    }
                    refused_n += 1;
                    shared_n += o.shared as usize;
                    continue;
                }
                if !ok {
                    bad.push(format!("a match at {at:?} beside {} at {j}, {len} outputs: every payment at its place, refused: {}", y.name, show(&t)));
                    continue;
                }
                accepted += 1;
                if once {
                    continue;
                }
                once = true;
                for (i, by) in o.asked.iter().filter(|(i, _)| o.outs[*i].value > 0) {
                    let (t, _) = build(len, Some(*i)).unwrap();
                    let failed = failed_inputs(&t);
                    probes += 1;
                    if failed != vec![*by] {
                        bad.push(format!("a match at {at:?} beside {} at {j}, {len} outputs: output {i} a sompi short fails {failed:?}, asked by {by}", y.name));
                    }
                }
            }
            if !once {
                bad.push(format!("a match at {at:?} beside {} at {j}: no layout accepted", y.name));
            }
        }
    }
    println!("T1b O4 pairs: {accepted} layouts accepted, {refused_n} refused ({shared_n} with one output for two payments); {probes} outputs a sompi short, each failing the one script that asked it");
    assert!(bad.is_empty(), "{} not as expected:\n{}", bad.len(), bad.join("\n"));
    // (Round 11: a match pays no key, so it shares no output with the other operation; the other's own places still
    // fall among the inputs in some layouts.)
    let _ = shared_n;
    assert!(accepted >= 13 * 7);
}

/// O5. Cancel and expire: the owner cancels; nobody returns an order before its expiry; the whole value to the owner.
#[test]
fn t1b_o5_only_the_owner_cancels_and_nobody_expires_an_order_early() {
    let w = world();
    let v = scenes(&w);
    let (cancel, expire) = (the(&v, "order-cancel"), the(&v, "order-expire"));
    let exp = w.order.expiry as u64;
    let mut bad = Bad::new();
    for k in [other(), keypair(45), keypair(60)] {
        bad.must_fail("order-cancel", "signed by another key", &cancel.with(|s| { let n = s.rest.len() - 1; s.rest[n] = A::S(k); }), Some(1));
    }
    bad.must_pass("order-cancel", "at any lock time, before the expiry", &cancel.with(|s| s.lock = 0));
    // The lock time proves the DAA: below the expiry, none, of the other kind (a time stamp), and the lock switched
    // off by a final sequence at the script's input.
    bad.must_pass("order-expire", "at the expiry", &expire);
    bad.must_pass("order-expire", "long after the expiry", &expire.with(|s| s.lock = exp + 1_000_000));
    bad.must_fail("order-expire", "a DAA before the expiry", &expire.with(|s| s.lock = exp - 1), Some(1));
    bad.must_fail("order-expire", "no lock time", &expire.with(|s| s.lock = 0), Some(1));
    bad.must_fail("order-expire", "a lock time that is a time stamp", &expire.with(|s| s.lock = 1_700_000_000_000), Some(1));
    bad.must_fail("order-expire", "the script's input final", &expire.with(|s| s.lib_seq = u64::MAX), Some(1));
    bad.must_fail("order-expire", "the script's input final, a lock time far ahead", &expire.with(|s| { s.lib_seq = u64::MAX; s.lock = 400_000_000_000; }), Some(1));
    // An expiry shown that is not the order's.
    bad.must_fail("order-expire", "an earlier expiry shown", &expire.with(|s| { s.rest[5] = A::I(1); s.lock = exp - 1; }), Some(1));
    // The whole value to the owner at the order's index, nothing written under its id.
    for (name, s) in [("order-cancel", &cancel), ("order-expire", &expire)] {
        bad.must_fail(name, "to another key", &s.with(|s| s.outs[0] = pay(&w.a.x.b_key, w.value(K::Order))), Some(1));
        bad.must_fail(name, "the owner's output elsewhere", &s.with(|s| s.outs.swap(0, 3)), Some(1));
        bad.must_fail(name, "half to the owner, half to another", &s.with(|s| { s.outs[0].value /= 2; s.outs[2] = pay(&w.a.x.b_key, w.value(K::Order) / 2); }), Some(1));
        bad.must_fail(name, "the order written again under its id", &s.with(|s| s.outs[2] = slot_out(w.hash(K::Order), 1)), None);
        // Widths: the template's hash and the owner's key at other widths, the order's hash following.
        for t in [vec![7u8; 31], vec![7u8; 33], vec![]] {
            let h = h_order(&t, &w.order.owner, &order_ints(&w.order));
            bad.must_fail(name, &format!("a template hash of {} bytes", t.len()), &s.with(|s| { s.obj = tslot(h); s.fixed = t.clone(); }), Some(1));
            let h = h_order(&w.order.tpl, &t, &order_ints(&w.order));
            bad.must_fail(name, &format!("an owner's key of {} bytes", t.len()), &s.with(|s| { s.obj = tslot(h); s.mv = t.clone(); s.outs[0] = pay_raw(&t, w.value(K::Order)); }), Some(1));
        }
    }
    // An order whose expiry is 0 is returned by anyone at once; one whose expiry is a time stamp by nobody, ever
    // (its owner cancels it).
    for (expiry, lock, ok) in [(0i64, 0u64, true), (600_000_000_000, 600_000_000_000, false), (600_000_000_000, 0, false)] {
        let mut z = w.clone();
        z.order.expiry = expiry;
        let e = the(&scenes(&z), "order-expire").with(|s| s.lock = lock);
        if ok {
            bad.must_pass("order-expire", "an order whose expiry is 0, at once", &e);
        } else {
            bad.must_fail("order-expire", "an order whose expiry is a time stamp", &e, Some(1));
        }
        bad.must_pass("order-cancel", "whatever its expiry", &the(&scenes(&z), "order-cancel"));
    }
    bad.done("O5 cancel and expire");
    // A cancel beside a match of the same order. The long order: the match writes the position under its id and the
    // cancel asks for nothing under it. The short order filled whole: both hold only if somebody pays the order's
    // whole value to its owner on top of the match (nobody loses by it).
    let tb = tpl_of().bytes();
    let (_, lo, _, so, _, n) = match_cases().remove(4);
    for (which, taken) in [(0usize, false), (1, false)] {
        let o = if which == 0 { lo } else { so };
        let mut z = w.clone();
        z.order = o;
        let signer = if which == 0 { owner() } else { other() };
        let c = the(&scenes(&z), "order-cancel").with(|s| { let n = s.rest.len() - 1; s.rest[n] = A::S(signer); });
        let k = mx(&tb, &lo, &so, n, PX, [0, 1, 2]).with(|k| k.n_in = 5);
        let mut s = Slots::new(12, 5);
        k.place(&mut s);
        s.put(3, out(LIBV, p2sh(&c.lib), bind(3, LIB)), 3);
        s.put(which as i64, pay(&o.owner, o.margin + o.tip), 3);
        let outs = s.done().unwrap().outs;
        let mut parts = k.parts();
        let me = c.with(|x| x.who = who_of([which as i64; 3]));
        let patch = me.clone();
        parts.push(Part { at: 3, input: me.lib_in(71), seq: 0, patch: Some(Box::new(move |x| patch.patch(x))) });
        let t = assemble(parts, 5, outs, 0);
        assert_eq!(all_ok(&t), taken, "a cancel beside a match of the same {} order: {}", if which == 0 { "long" } else { "short" }, show(&t));
    }
}

/// O6, O8. What a match cannot leave behind, and what an owner can only do to itself.
#[test]
fn t1b_o6_what_rests_is_an_order_that_can_be_filled() {
    let tpl = tpl_of();
    let tb = tpl.bytes();
    // A long order of 10 units holding 0.25 KAS of margin: 2 units leave 0.2 KAS (rests), 3 units leave 0.175.
    let lo = ord(&owner(), LONG, 10 * UNIT, PX + PX / 10, 25_000_000, 0, true, 0);
    let so = ord(&other(), SHORT, 20 * UNIT, PX - PX / 20, 60 * KAS, 0, true, 0);
    let k = |n: i64| mx(&tb, &lo, &so, n, PX, [0, 1, 2]);
    assert!(all_ok(&k(2 * UNIT).tx()), "what rests holds 0.2 KAS exactly");
    assert!(fails(&k(3 * UNIT).tx(), 2), "what would rest holds less than 0.2 KAS");
    assert!(match_orders(&tpl, &lo, lo.margin, &so, so.margin, 3 * UNIT, PX).is_err());
    assert!(all_ok(&k(10 * UNIT).tx()), "filled whole");
    // Round 11: no order returns what is left in a match. An order to be taken whole says so by its least fill.
    let lo2 = Order { min_fill: lo.size, ..lo };
    assert!(fails(&mx(&tb, &lo2, &so, 2 * UNIT, PX, [0, 1, 2]).tx(), 2), "a part of an order to be taken whole");
    let t = mx(&tb, &lo2, &so, 10 * UNIT, PX, [0, 1, 2]).tx();
    assert!(all_ok(&t));
    assert_eq!(paid_to(&t, &lo2.owner), 0);
    // After any fill that rests, what is left is filled whole by one more match (the margin left is above nothing).
    let mut left = lo;
    let mut held = lo.margin;
    for n in [2 * UNIT, 1, 999_999, UNIT] {
        let k = mx(&tb, &left, &so, n, PX, [0, 1, 2]).with(|k| k.lval = held);
        if !all_ok(&k.tx()) {
            continue;
        }
        let e = k.exp();
        left = Order { size: left.size - n, margin: left.margin - e.am, ..left };
        held = e.lv;
        assert!(left.margin > 0 && held >= 20_000_000);
        let whole = mx(&tb, &left, &so, left.size, PX, [0, 1, 2]).with(|k| k.lval = held);
        assert!(all_ok(&whole.tx()), "what rests after a fill of {n} is filled whole");
    }
    // An order holding more than its margin and tip, filled whole: since round 11 the excess is in no output the
    // script asks. Nothing is paid to the owner; whoever sends the match has it (by design: the owner gave it).
    let (_, lo, lval, so, sval, n) = match_cases().remove(4);
    let k = mx(&tb, &lo, &so, n, PX, [0, 1, 2]).with(|k| { k.lval = lval + 5 * KAS; k.sval = sval; });
    let t = k.tx();
    assert!(all_ok(&t), "{}", show(&t));
    assert_eq!(paid_to(&t, &lo.owner), 0, "the excess of an order used up is not paid to its owner");
    let e = k.exp();
    assert_eq!(e.lv, 5 * KAS, "what the order held beyond its margin and tip");
    println!("T1b DESIGN excess: an order used up that held {} sompi more than its margin and tip: the match writes the position ({}) and nothing for the owner; the sender has the excess and the tips ({})", e.lv, e.am + e.bm, e.lf + e.sf);
    // An order holding less than its margin and tip is filled only as far as it holds.
    let k = mx(&tb, &lo, &so, n, PX, [0, 1, 2]).with(|k| k.lval = lval - 1);
    assert!(refused(&k.tx()));
}

/// Round 4, F7, by design at the script level (the tool's refusal is `t1b_fixed_the_tool_refuses_a_match_past_the_expiry`
/// in auditT1_builders.rs). An order's expiry is the DAA from which anyone returns it to its owner; it is not a DAA
/// after which it is no longer matched. A match reads no time (a script can prove that a DAA has come, not that it has
/// not passed), so an order past its expiry is still matched by the script.
#[test]
fn t1b_design_the_script_matches_an_order_past_its_expiry() {
    let tpl = tpl_of();
    let tb = tpl.bytes();
    let (_, lo, lval, so, sval, n) = match_cases().remove(0);
    let (lo, so) = (Order { expiry: 1, ..lo }, Order { expiry: 1, ..so });
    let k = mx(&tb, &lo, &so, n, lo.limit, [0, 1, 2]);
    assert!(all_ok(&k.tx()), "{}", show(&k.tx()));
    assert!(match_orders(&tpl, &lo, lval, &so, sval, n, lo.limit).is_ok(), "the model matches them too");
    // Even with a lock time long after the expiry: the transaction itself says the expiry has passed, and the script
    // still takes it (a script could only forbid a match after a lock time, which would end every order at its expiry).
    let o = k.honest(12).unwrap();
    assert!(all_ok(&assemble(k.parts(), 4, o.outs, 9_000_000)));
    println!("T1b DESIGN expiry: the script matches two orders that ended at DAA 1 even when the transaction is locked to DAA 9,000,000 (the tool is what refuses it now)");
}

/// Round 4, F6, fixed: an order states `ask`, the least margin the other side must post for what is left of it, and a
/// match requires each side's margin share to be at least the other order's `ask` share. So the attack of round 4 (a
/// sompi of margin against a real one) is refused; a match where both asks are met is taken; an order that asks
/// nothing is matched against anything, which is its owner's choice.
#[test]
fn t1b_fixed_a_match_posts_the_margin_each_order_asks() {
    let tpl = tpl_of();
    let tb = tpl.bytes();
    // B sells 4 units at 10 with 60 KAS of margin and asks the buyer to post 60 KAS (its default ask).
    let so = with_ask(ord(&other(), SHORT, 4 * UNIT, PX, 60 * KAS, KAS / 3, true, 0), 60 * KAS);
    // A would buy the 4 units with a sompi of margin: refused now (am = 1 < the short's ask share of 60 KAS).
    let lo = ord(&owner(), LONG, 4 * UNIT, PX, 1, 20_000_000, true, 0);
    let k = mx(&tb, &lo, &so, 4 * UNIT, PX, [0, 1, 2]);
    assert!(fails(&k.tx(), 2), "a sompi of margin against a side that asks 60 KAS: {}", show(&k.tx()));
    assert!(match_orders(&tpl, &lo, lo.value(), &so, so.value(), 4 * UNIT, PX).is_err(), "the model refuses it too");
    // A posting 60 KAS (meeting the ask) is taken; a sompi short of it is not.
    let fair = with_ask(ord(&owner(), LONG, 4 * UNIT, PX, 60 * KAS, KAS / 20, true, 0), 60 * KAS);
    assert!(all_ok(&mx(&tb, &fair, &so, 4 * UNIT, PX, [0, 1, 2]).tx()), "the ask met");
    let short = with_ask(ord(&owner(), LONG, 4 * UNIT, PX, 60 * KAS - 1, KAS / 20, true, 0), 60 * KAS - 1);
    assert!(fails(&mx(&tb, &short, &so, 4 * UNIT, PX, [0, 1, 2]).tx(), 2), "a sompi short of the short's ask");
    // An order that asks nothing (the owner's choice) is matched against a sompi of margin.
    let open = ord(&other(), SHORT, 4 * UNIT, PX, 60 * KAS, KAS / 3, true, 0);
    assert!(all_ok(&mx(&tb, &lo, &open, 4 * UNIT, PX, [0, 1, 2]).tx()), "an ask of nothing lets a sompi in");
    // The ask cannot be got around by a sequence of partial fills: `t1b_fixed_the_ask_holds_across_partial_fills`.
    println!("T1b F6 fixed: an order posts at least what the other order asks of it, or the match is refused");
}

/// Round 4, F6: the ask holds across partial fills. For random orders, down to nothing, the sum of what each side
/// posts into the positions is at least the sum of what the other order asked a fill, so rounding never lets a side
/// post less in total than the ask. Driven through the model (the script's arithmetic, which `t1b_o1_...` holds equal).
#[test]
fn t1b_fixed_the_ask_holds_across_partial_fills() {
    let tpl = tpl_of();
    let px = world().a.x.legs[0].entry;
    let mut seed = 0x51ab_0c0f_fee0_0005u64;
    let mut r = |lo: i64, hi: i64| -> i64 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        lo + (seed % (hi - lo) as u64) as i64
    };
    let (mut orders, mut fills) = (0, 0);
    for _ in 0..400 {
        // A long that rests, asking a margin a unit of the short; the short filled whole a match, posting exactly its
        // margin. The short's margin must meet the long's ask share each fill, or the model refuses the fill.
        let size = r(1, 2_000_000_001);
        let min_counter = r(1, 50 * KAS);
        let mut long = Order { tpl: tpl.hash(), owner: [1; 32], dir: LONG, size, limit: 2 * px, margin: r(size, 5_000 * KAS), tip: 0, expiry: 9_000, min_fill: 0, min_counter };
        if long.check().is_err() {
            continue;
        }
        let mut held = long.value();
        let (mut posted, mut asked) = (0i64, 0i64);
        orders += 1;
        loop {
            let n = if r(0, 3) == 0 { long.size } else { r(1, long.size + 1) };
            // The short posts exactly the long's ask share (the least that is accepted).
            let want = share(long.min_counter, n, long.size);
            let short = Order { tpl: tpl.hash(), owner: [2; 32], dir: SHORT, size: n, limit: 1, margin: want.max(1), tip: 0, expiry: 9_000, min_fill: 0, min_counter: 0 };
            match match_orders(&tpl, &long, held, &short, short.value(), n, px) {
                Ok(m) => {
                    posted += m.position.x.b_margin;
                    asked += want;
                    assert!(m.position.x.b_margin >= want, "the short posts its margin, which met the ask");
                    fills += 1;
                    match m.long {
                        Rest::Order(o, v) => {
                            long = o;
                            held = v;
                        }
                        _ => break,
                    }
                }
                Err(_) => {
                    // The short's margin (= the ask share) was below the long's ask share: only when `want` rounded to
                    // less than the share the match computes, which share() never does (both are the same floor).
                    break;
                }
            }
            if long.size < 50 {
                break;
            }
        }
        assert!(posted >= asked || asked == 0, "across the fills the short posted {posted}, the long asked {asked}");
    }
    println!("T1b F6 partial fills: {orders} orders, {fills} fills; a side never posts less in total than the ask");
}

/// Round 4, F8, by design at the script level (the tool defaults `least`; `t1b_fixed_the_tool_defaults_the_least_fill`
/// is in auditT1_builders.rs). The script, shown an order without a least fill, takes a match of one unit of size when
/// both sides post what the asks require. So the cut-up is kept off only by the tool's default.
#[test]
fn t1b_design_the_script_matches_a_fill_of_one_without_a_least_fill() {
    let tpl = tpl_of();
    let tb = tpl.bytes();
    // A long of 10 units asking nothing (so a one-unit fill is not stopped by the ask), the short one unit of size.
    for rest in [false, true] {
        let lo = with_ask(ord(&owner(), LONG, 10 * UNIT, PX + PX / 10, 40 * KAS, KAS / 2, rest, 0), 0);
        let mine = with_ask(ord(&keypair(45), SHORT, 1, PX, 1, 20_000_000, false, 0), 0);
        let k = mx(&tb, &lo, &mine, 1, PX, [0, 1, 2]);
        assert!(all_ok(&k.tx()), "{}", show(&k.tx()));
        let e = k.exp();
        assert_eq!((e.am, e.bm), (400, 1), "a position of 401 sompi");
        assert!(match_orders(&tpl, &lo, lo.value(), &mine, mine.value(), 1, PX).is_ok(), "the model takes it too");
        // A least fill of two units refuses it: the owner's own guard, which the tool now sets by default.
        assert!(fails(&k.long(|o| o[6] = 2 * UNIT).tx(), 2));
    }
    println!("T1b DESIGN least: the script matches a fill of one unit of an order without a least fill (the tool defaults the least now)");
}

// ================================================================ round 5: the merge (KpfTMerge) at the engine

use kpf_protocol::thin::{merged, MERGE_TIP};

/// A single-leg position of A (owner) and B (other) of the sample template.
fn ol(n: i64, e: i64, am: i64, bm: i64) -> Position {
    tpl_of().position(xkey(&owner()), xkey(&other()), n, e, am, bm)
}

/// A merge as a transaction, every field a knob: the two position objects (shown whole for the first, as the second's
/// leg size/entry and margins for the second), the inputs they stand at and their ids.
#[derive(Clone)]
struct Mrg {
    lib: SilAbiArtifact,
    /// The first position, shown whole (st, moving).
    st: Vec<u8>,
    mv: Vec<u8>,
    a1: Position,
    a2: Position,
    /// The hashes the two objects carry (so a wrong kind or a tweaked object can be put in).
    h1: [u8; 32],
    h2: [u8; 32],
    v1: i64,
    v2: i64,
    id1: Hash,
    id2: Hash,
    /// The second's leg size, entry, and the two margins, as the script is shown them.
    n2: i64,
    e2: i64,
    am2: i64,
    bm2: i64,
    at: [usize; 3],
    n_in: usize,
    who: Vec<u8>,
    go: [(i64, i64); 2],
}
fn mrg(a1: Position, a2: Position, at: [usize; 3]) -> Mrg {
    let l2 = &a2.x.legs[0];
    Mrg {
        lib: lib_of("KpfTMerge"),
        st: a1.fixed(),
        mv: a1.moving(),
        a1,
        a2,
        h1: a1.hash(),
        h2: a2.hash(),
        v1: a1.pot(),
        v2: a2.pot(),
        id1: IDL,
        id2: IDL,
        n2: l2.size,
        e2: l2.entry,
        am2: a2.x.a_margin,
        bm2: a2.x.b_margin,
        at,
        n_in: 4,
        who: who_of([at[0] as i64, at[1] as i64, at[1] as i64]),
        go: [(at[2] as i64, 0), (at[2] as i64, 1)],
    }
}
impl Mrg {
    fn with(&self, f: impl FnOnce(&mut Mrg)) -> Mrg {
        let mut k = self.clone();
        f(&mut k);
        k
    }
    fn out_hash(&self) -> [u8; 32] {
        // For the negative cases the pair need not be mergeable; any output then stands (the tx is refused anyway).
        merged(&self.a1, &self.a2).map(|m| m.hash()).unwrap_or_else(|_| self.a1.hash())
    }
    fn out_val(&self) -> i64 {
        self.v1 + self.v2 - MERGE_TIP
    }
    fn patch(&self, script: &[u8]) -> Vec<u8> {
        // pushes: who(0) ageb(1) slotLen(2) st(3) moving(4) n2(5) e2(6) a2(7) b2(8)
        let mut s = repush(script, 0, &self.who);
        s = repush(&s, 3, &self.st);
        s = repush(&s, 4, &self.mv);
        s
    }
    fn parts(&self) -> Vec<Part> {
        let int = |v: i64| Arg::V(ArtifactValue::Int(v));
        let o1 = oslot(self.h1);
        let o2 = oslot(self.h2);
        let args = vec![by(&[0; 24]), by(&[0; 8]), int(slot_len()), by(&[0; 312]), by(&[0; 138]), int(self.n2), int(self.e2), int(self.am2), int(self.bm2), int(self.a2.a_min_counter), int(self.a2.b_min_counter)];
        let me = self.clone();
        let obj = |n: u8, art: SilAbiArtifact, value: i64, id: Hash, g: (i64, i64)| In { outpoint: outpoint(n), utxo: UtxoEntry::new(value.max(0) as u64, p2sh(&art), 6_200, false, Some(id)), spend: Spend::Signed { art, entry: "go", args: vec![int(g.0), int(g.1)] } };
        vec![
            Part { at: self.at[0], input: obj(63, o1, self.v1, self.id1, self.go[0]), seq: 0, patch: None },
            Part { at: self.at[1], input: obj(64, o2, self.v2, self.id2, self.go[1]), seq: 0, patch: None },
            Part { at: self.at[2], input: In { outpoint: outpoint(65), utxo: UtxoEntry::new(KAS1, p2sh(&self.lib), 90, false, Some(LIB)), spend: Spend::Signed { art: self.lib.clone(), entry: "merge", args } }, seq: 0, patch: Some(Box::new(move |s| me.patch(s))) },
        ]
    }
    /// The honest layout: the merged position under id1 at the first free output, the script at its index, change.
    fn outs(&self, len: usize) -> Vec<TransactionOutput> {
        let mut v: Vec<TransactionOutput> = (0..len).map(|_| change(1)).collect();
        v[self.at[2]] = out(LIBV, p2sh(&self.lib), bind(self.at[2] as u16, LIB));
        // The position at the first slot that is not an input index or the script.
        let position = out(self.out_val().max(0) as u64, spk(self.out_hash()), bind(self.at[0] as u16, self.id1));
        let free = (0..len).find(|i| *i != self.at[2] && !self.at[..2].contains(i)).unwrap();
        v[free] = position;
        v
    }
    fn tx_with(&self, outs: Vec<TransactionOutput>) -> Tx {
        assemble(self.parts(), self.n_in, outs, 0)
    }
    fn tx(&self) -> Tx {
        self.tx_with(self.outs(self.n_in + 4))
    }
}

/// M1, M5 (round 8: the two positions are of one id, the registry's). A merge of two plain positions of the same two
/// parties, wherever its inputs stand, pays the merged pot and nothing more; a sompi short at the position is refused;
/// two positions of two ids are not merged.
#[test]
fn t1b_m1_a_merge_pays_the_two_pots_less_the_tip() {
    let a1 = ol(5 * UNIT, PX, 100 * KAS, 100 * KAS);
    let a2 = ol(3 * UNIT, PX + PX / 50, 70 * KAS, 50 * KAS);
    let m = merged(&a1, &a2).expect("mergeable");
    assert_eq!(m.pot(), a1.pot() + a2.pot() - MERGE_TIP, "the merged pot is the two less the tip");
    let mut tried = 0;
    for at in arrangements(4) {
        let k = mrg(a1, a2, at);
        let t = k.tx();
        assert!(all_ok(&t), "inputs {at:?}: {}", show(&t));
        assert_eq!(k.out_val(), a1.pot() + a2.pot() - MERGE_TIP);
        let o = k.outs(k.n_in + 4);
        let ai = o.iter().position(|x| x.script_public_key == spk(k.out_hash())).unwrap();
        let short = { let mut o = o.clone(); o[ai].value -= 1; o };
        assert!(refused(&k.tx_with(short)), "inputs {at:?}: a sompi short at the position");
        let more = { let mut o = o.clone(); o[ai].value += 1; o };
        assert!(all_ok(&k.tx_with(more)), "inputs {at:?}: a sompi more at the position");
        // B2: the second position under another id (a position written beside the registry).
        assert!(fails(&k.with(|k| k.id2 = IDS).tx(), at[2]), "inputs {at:?}: the second position of another id");
        tried += 4;
    }
    println!("T1b M1: {tried} merges, short and over outputs, and second positions of another id");
}

/// M1. The sender keeps MERGE_TIP and no more: it cannot take a second slice by any arrangement.
#[test]
fn t1b_m1_the_sender_keeps_only_the_tip() {
    let a1 = ol(5 * UNIT, PX, 100 * KAS, 100 * KAS);
    let a2 = ol(3 * UNIT, PX, 70 * KAS, 70 * KAS);
    let k = mrg(a1, a2, [0, 1, 2]);
    // The position written for less than the pot less the tip: refused (M1). So the sender cannot keep more than the tip.
    for less in [1i64, MERGE_TIP, MERGE_TIP + 1] {
        let o = k.outs(8);
        let ai = o.iter().position(|x| x.script_public_key == spk(k.out_hash())).unwrap();
        let mut bad = o.clone();
        bad[ai].value -= less as u64;
        assert!(refused(&k.tx_with(bad)), "the position {less} sompi short");
    }
    // The same position written twice (two outputs of id1) is refused: only one output of the id.
    let o = k.outs(8);
    let ai = o.iter().position(|x| x.script_public_key == spk(k.out_hash())).unwrap();
    let free = (0..o.len()).find(|i| is_change(&o[*i])).unwrap();
    let mut twice = o.clone();
    twice[free] = out(k.out_val() as u64, spk(k.out_hash()), bind(0, IDL));
    assert!(refused(&k.tx_with(twice)), "the position written twice under id1");
    // A position merged with itself (the same input at both places) is refused by the script (j1 != j2) and the
    // builder; here the same object at inputs 0 and 1 with one id and count 2 but one real UTXO is not expressible, so
    // the model's guard stands (the builder refuses `first.op == second.op`).
    println!("T1b M1 sender: the merged position is fixed from below at the pot less the tip; no second slice");
}

/// M3, M5. Only two plain positions are merged: a claim, a disputed claim, a liquidation (claimed/disputed/proved), a
/// exit claim or an order in either place is refused; and an object of another kind under id1 beside the two
/// positions.
#[test]
fn t1b_m3_only_plain_positions_are_merged() {
    let w = world();
    let a1 = ol(5 * UNIT, PX, 100 * KAS, 100 * KAS);
    let a2 = ol(3 * UNIT, PX, 70 * KAS, 70 * KAS);
    assert!(all_ok(&mrg(a1, a2, [0, 1, 2]).tx()));
    let mut tried = 0;
    for k in KINDS.iter().filter(|k| **k != K::Position) {
        // The other kind in the first position's place, and in the second's.
        for first in [true, false] {
            let m = mrg(a1, a2, [0, 1, 2]).with(|x| {
                if first {
                    x.h1 = w.hash(*k);
                    x.v1 = w.value(*k);
                } else {
                    x.h2 = w.hash(*k);
                    x.v2 = w.value(*k);
                }
            });
            assert!(fails(&m.tx(), 2), "a {k:?} in the {} position's place", if first { "first" } else { "second" });
            tried += 1;
        }
    }
    // A position that is not the first's state with only size/entry/margins changed (another dispute period) in the second's
    // place: the model refuses, and the script, shown the first's parts, computes a different second hash.
    let mut other = a2;
    other.dispute_daa += 1;
    assert!(merged(&a1, &other).is_err());
    let m = mrg(a1, other, [0, 1, 2]);
    assert!(fails(&m.tx(), 2), "a second position of another dispute period");
    tried += 1;
    println!("T1b M3: {tried} merges with an object that is not a plain position of the pair refused");
}

/// M5 (round 8). The merge writes one output under the registry's id, the merged position, and goes on; `who` names the
/// two positions and nothing else. B1: a position nobody signed written beside it, or in its place, is refused.
#[test]
fn t1b_m5_the_merge_writes_only_the_position_under_id1_and_ends_id2() {
    let a1 = ol(5 * UNIT, PX, 100 * KAS, 100 * KAS);
    let a2 = ol(3 * UNIT, PX, 70 * KAS, 70 * KAS);
    let k = mrg(a1, a2, [0, 1, 2]);
    let o = k.outs(8);
    let ai = o.iter().position(|x| x.script_public_key == spk(k.out_hash())).unwrap();
    let free = (0..o.len()).find(|i| is_change(&o[*i])).unwrap();
    let bad = |why: &str, f: &dyn Fn(&mut Vec<TransactionOutput>)| {
        let mut x = o.clone();
        f(&mut x);
        assert!(refused(&k.tx_with(x)), "{why}");
    };
    bad("the position under another id", &|x| x[ai].covenant = bind(0, IDS));
    bad("the position under no id", &|x| x[ai].covenant = None);
    bad("no position", &|x| x[ai] = change(1));
    bad("the position written twice", &|x| x[free] = out(k.out_val() as u64, spk(k.out_hash()), bind(0, IDL)));
    for auth in 0..4u16 {
        bad(&format!("a position nobody signed under the registry's id, by input {auth}"), &|x| x[free] = out(KAS1, spk([0x55; 32]), bind(auth, IDL)));
        bad(&format!("a position nobody signed in the merged one's place, by input {auth}"), &|x| { x[ai].script_public_key = spk([0x55; 32]); x[ai].covenant = bind(auth, IDL); });
    }
    bad("one of the two positions written again beside the merged one", &|x| x[free] = out(a2.pot() as u64, spk(a2.hash()), bind(1, IDL)));
    bad("the script a sompi short", &|x| x[2].value -= 1);
    bad("the script under no id", &|x| x[2].covenant = None);
    bad("another script in the script's place", &|x| x[2].script_public_key = p2sh(&lib_of("KpfTBoth")));
    for who in [[2, 1, 1], [3, 1, 1], [0, 2, 2], [0, 3, 3], [9, 1, 1], [0, 0, 0]] {
        assert!(refused(&k.with(|k| k.who = who_of(who)).tx()), "who {who:?}");
    }
    println!("T1b M5: the merge writes one position under the registry's id and goes on; wrong writings refused");
}

/// M5, F1-style: a merge beside another operation in one transaction shares no output (the merge's only payment is
/// the position under id1, a from-the-front place, so it cannot collide with a from-the-end payment; checked against a
/// lapse, a settle and a cancel).
#[test]
fn t1b_m5_a_merge_beside_another_operation() {
    let w = one_key_world();
    let a1 = ol(5 * UNIT, PX, 100 * KAS, 100 * KAS);
    let a2 = ol(3 * UNIT, PX, 70 * KAS, 70 * KAS);
    let v = scenes(&w);
    let mut tried = 0;
    for name in ["lapse", "liq-settle", "order-cancel", "late"] {
        let y = v.iter().find(|s| s.name == name).unwrap();
        // The merge at inputs 0,1 (script 2); the other operation at 3 (script 4); funding 5.
        let k = mrg(a1, a2, [0, 1, 2]).with(|k| k.n_in = 6);
        let mut s = Slots::new(14, 6);
        // The merge's position under id1 (a front/any place) and its script.
        s.put(2, out(LIBV, p2sh(&k.lib), bind(2, LIB)), 2);
        s.any(out(k.out_val() as u64, spk(k.out_hash()), bind(0, IDL)), 2);
        place_scene(y, 3, 4, &mut s);
        let o = s.done().expect("a layout");
        let mut parts = k.parts();
        parts.extend(scene_parts(&y.s, 3, 4, 70));
        let t = assemble(parts, 6, o.outs.clone(), y.s.lock);
        assert!(all_ok(&t), "a merge beside {name}: {}", show(&t));
        // Each payment a sompi short fails the one script that asked it.
        for (at, by) in o.asked.iter().filter(|(i, _)| o.outs[*i].value > 0) {
            let mut x = o.outs.clone();
            x[*at].value -= 1;
            let mut parts = k.parts();
            parts.extend(scene_parts(&y.s, 3, 4, 70));
            let t = assemble(parts, 6, x, y.s.lock);
            assert_eq!(failed_inputs(&t), vec![*by], "a merge beside {name}: output {at} short");
        }
        tried += 1;
    }
    println!("T1b M5 pairs: {tried} merges beside another operation, no shared output");
}

// ================================================================ round 7: the merge is signed by both parties

// ================================================================ round 8: the registry

/// B1 at the source. Every entry of every script holds the object's id to a number of inputs and of outputs: by
/// `becomes` (one in, one out, script and value fixed), `back` (one in, none out), `enters` (one in, two out, both
/// fixed), or by its own counts. Printed as a table; an entry without both is a way to write under the registry.
#[test]
fn t1b_b1_every_entry_counts_the_inputs_and_outputs_of_the_objects_id() {
    let mut rows = vec![];
    for name in OBJ_OPS {
        let src = contract_source(&format!("{name}.sil"));
        let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let helper = |f: &str| -> Option<&str> { code.split(&format!("function {f}(")).nth(1).map(|b| b.split("\n    }").next().unwrap()) };
        // What each helper asks, read from its own body.
        for (f, ins, outs) in [("becomes", "== 1", "== 1"), ("back", "== 1", "== 0"), ("enters", "== 1", "== 2")] {
            if let Some(b) = helper(f) {
                assert!(b.contains(&format!("require(OpCovInputCount(id) {ins});")) && b.contains(&format!("require(OpCovOutputCount(id) {outs});")), "{name}: {f} does not count as expected");
            }
        }
        for e in code.split("\n    entry ").skip(1) {
            let body = e.split("\n    }").next().unwrap();
            let entry = body.split('(').next().unwrap();
            // Round 19. `release` reads no object and writes nothing: it has no id to count (what the copy becomes
            // is the naming script's to answer: A3 and the places above). The two that collect move the fee pool and a
            // copy, which are under the library's own id: they count that id in (3 and 2) and, by going on, out.
            if entry == "release" {
                assert!(!body.contains("tx.outputs") && !body.contains("Count("), "{name}.release reads an output or a count");
                rows.push(format!("{name}.release: no object (the script that names the copy answers)"));
                continue;
            }
            if COLLECTORS.contains(&name) {
                let n = if name == "KpfTSweep" { 3 } else { 2 };
                assert!(body.contains(&format!("require(OpCovInputCount(lib) == {n});")) && body.contains("goesOn();") && code.contains("require(OpCovOutputCount(lib) == OpCovInputCount(lib));"), "{name}.{entry}: the library's id is not counted in and out");
                rows.push(format!("{name}.{entry}: the library's id: {n} in, {n} out"));
                continue;
            }
            let via = ["becomes", "back", "enters"].iter().find(|f| body.contains(&format!("{f}(")) && helper(f).is_some());
            let count = |what: &str| -> Vec<String> { body.lines().filter(|l| l.contains(what) && l.contains("require(") && !l.contains("(lib)")).map(|l| l.trim().trim_start_matches("require(").trim_end_matches(");").to_string()).collect() };
            let (ins, outs) = (count("OpCovInputCount("), count("OpCovOutputCount("));
            let how = match via {
                Some(f) => format!("{f}: {}", match *f { "becomes" => "1 in, 1 out", "back" => "1 in, 0 out", _ => "1 in, 2 out" }),
                None => format!("{} | {}", ins.join(", "), outs.join(", ")),
            };
            assert!(via.is_some() || (!ins.is_empty() && !outs.is_empty()), "{name}.{entry}: the object's id is not counted in and out: {how}");
            rows.push(format!("{name}.{entry}: {how}"));
        }
    }
    for r in &rows {
        println!("T1b B1 counts: {r}");
    }
    assert_eq!(rows.len(), 59);
}

/// B1 at the engine, for the 22 scenes (every entry that reads one object, the two of `KpfTPlace` among them): one
/// output more under the object's id, a position nobody signed, authorized by the object's input, the script's or
/// the funding's; and the same in the place of what the entry writes. All refused.
#[test]
fn t1b_b1_no_entry_writes_an_output_more_under_the_objects_id() {
    let w = world();
    let fab = spk(tpl_of().position(xkey(&keypair(77)), xkey(&keypair(78)), UNIT, PX, 50 * KAS, 50 * KAS).hash());
    let mut bad = Bad::new();
    for sc in scenes(&w) {
        for auth in 0..3u16 {
            for at in [3usize, 4] {
                bad.must_fail(sc.name, &format!("a position nobody signed at output {at} under the id, by input {auth}"), &sc.s.with(|s| s.outs[at] = out(KAS1, fab.clone(), bind(auth, XID))), None);
            }
            if sc.s.outs[0].covenant.is_some() {
                bad.must_fail(sc.name, &format!("a position nobody signed in the place of what the entry writes, by input {auth}"), &sc.s.with(|s| { s.outs[0].script_public_key = fab.clone(); s.outs[0].covenant = bind(auth, XID); }), None);
            }
        }
        // The same output under no id is nobody's concern (it is not in the registry): the entry runs.
        bad.must_pass(sc.name, "the same output under no id", &sc.s.with(|s| s.outs[4] = out(KAS1, fab.clone(), None)));
    }
    bad.done("B1 one output more under the id");
}

/// B1, the way in. `KpfTPlace` spends an anchor and nothing else; it writes what enters and the anchor again, in that
/// order, and nothing more; an order enters by its owner's signature, a position by both parties'.
#[test]
fn t1b_b1_the_way_into_the_registry() {
    let w = world();
    let v = scenes(&w);
    let (place, open) = (the(&v, "registry-place"), the(&v, "registry-open"));
    let anchor = W::anchor_hash();
    let mut bad = Bad::new();
    for (name, s) in [("registry-place", &place), ("registry-open", &open)] {
        bad.must_pass(name, "as built", s);
        bad.must_fail(name, "the anchor not written again", &s.with(|s| s.outs[2] = change(1)), Some(1));
        bad.must_fail(name, "the anchor a sompi poorer", &s.with(|s| s.outs[2].value -= 1), Some(1));
        bad.must_pass(name, "the anchor a sompi richer", &s.with(|s| s.outs[2].value += 1));
        bad.must_fail(name, "another object in the anchor's place", &s.with(|s| s.outs[2].script_public_key = spk([0x66; 32])), Some(1));
        bad.must_fail(name, "the anchor written before what enters", &s.with(|s| s.outs.swap(0, 2)), Some(1));
        bad.must_fail(name, "two anchors written", &s.with(|s| s.outs[3] = out(KAS1, spk(anchor), bind(0, XID))), None);
        bad.must_fail(name, "what enters written twice", &s.with(|s| s.outs[3] = s.outs[0].clone()), None);
        bad.must_fail(name, "what enters under no id", &s.with(|s| s.outs[0].covenant = None), None);
        bad.must_fail(name, "what enters a sompi short", &s.with(|s| s.outs[0].value -= 1), Some(1));
        bad.must_fail(name, "nothing enters: the anchor twice", &s.with(|s| s.outs[0] = out(KAS1, spk(anchor), bind(0, XID))), Some(1));
    }
    // The template's hash at another width, the order's hash following the bytes shown.
    for t in [vec![7u8; 31], vec![7u8; 33], vec![]] {
        let h = h_order(&t, &w.order.owner, &order_ints(&w.order));
        bad.must_fail("registry-place", &format!("a template hash of {} bytes", t.len()), &place.with(|s| { s.fixed = t.clone(); s.outs[0].script_public_key = spk(h); }), Some(1));
    }
    // The signatures: an order by its owner, a position by its two parties in their places.
    let n = place.rest.len() - 1;
    for k in [other(), keypair(45), keypair(60)] {
        bad.must_fail("registry-place", "signed by another key than the owner's", &place.with(|s| s.rest[n] = A::S(k)), Some(1));
    }
    for (a, b) in [(owner(), owner()), (other(), other()), (other(), owner()), (owner(), keypair(45)), (keypair(45), other())] {
        bad.must_fail("registry-open", "not signed by the two parties in their places", &open.with(|s| s.rest = vec![A::S(a), A::S(b)]), Some(1));
    }
    // An order's values the script holds: a side that is neither, no size, no limit, no margin, a negative tip.
    for (i, bad_value) in [(0usize, 3i64), (0, 0), (1, 0), (1, 2_000_000_001), (2, 0), (3, 0), (4, -1), (6, -1), (7, -1)] {
        let mut z = w.clone();
        let mut ints = order_ints(&z.order);
        ints[i] = bad_value;
        z.order = Order { dir: ints[0], size: ints[1], limit: ints[2], margin: ints[3], tip: ints[4], expiry: ints[5], min_fill: ints[6], min_counter: ints[7], ..z.order };
        let s = the(&scenes(&z), "registry-place").with(|s| s.rest[i] = A::I(bad_value));
        bad.must_fail("registry-place", &format!("an order whose value {i} is {bad_value}"), &s, Some(1));
    }
    // A position without a margin on one side does not enter.
    for side in 0..2 {
        let mut z = w.clone();
        if side == 0 { z.a.x.a_margin = 0 } else { z.a.x.b_margin = 0 }
        z.mv = z.a.moving();
        z.amt = Some(1);
        bad.must_fail("registry-open", "a position with no margin on a side", &the(&scenes(&z), "registry-open"), Some(1));
    }
    // Two anchors of one registry spent in one transaction, and an anchor beside an operation on an object of the registry.
    let in_tx = |x: &Scene, y: &Scene| -> Tx {
        let mut outs = vec![x.outs[0].clone(), out(LIBV, p2sh(&x.lib), bind(1, LIB)), y.outs[0].clone(), out(y.outs[1].value, p2sh(&y.lib), bind(3, LIB))];
        for (i, o) in [x.outs[2].clone(), y.outs[2].clone()].into_iter().enumerate() {
            if o.covenant.is_some() {
                outs.push(out(o.value, o.script_public_key, bind(2 * i as u16, XID)));
            }
        }
        outs[2].covenant = outs[2].covenant.map(|_| kaspa_consensus_core::tx::CovenantBinding { authorizing_input: 2, covenant_id: XID });
        outs.extend([change(1), change(1), change(1)]);
        let mut parts = scene_parts(x, 0, 1, 80);
        parts.extend(scene_parts(y, 2, 3, 70));
        assemble(parts, 5, outs, x.lock.max(y.lock))
    };
    assert!(refused(&in_tx(&place, &open)), "two anchors of one registry in one transaction");
    assert!(refused(&in_tx(&place, &the(&v, "add"))), "an anchor beside an operation on an object of the registry");
    assert!(refused(&in_tx(&the(&v, "add"), &open)), "an operation on an object of the registry beside an anchor");
    bad.done("B1 the way in");
}

/// B2, B3. An object is moved only by a script of its own library, named by its own input; an object of the registry
/// is no script to another object; and (round 7's signer test, replaced) a merge takes no position of another id.
#[test]
fn t1b_b2_b3_objects_of_the_registry_and_scripts_of_the_library_are_not_confused() {
    let w = world();
    let mut bad = Bad::new();
    for sc in scenes(&w) {
        let s = &sc.s;
        // The object names, as its script, another object of the same registry (input 3), whose signature script is a
        // slot's: refused by the object (that input does not carry the library's id).
        let twin = X::Go(s.obj.clone(), s.obj_value, XID, 1, 0);
        bad.must_fail(sc.name, "an object of the registry named as the script", &s.with(|s| { s.extra = vec![twin.clone()]; s.go.0 = 3; }), Some(0));
        // The script itself under the registry's id instead of the library's: the object refuses it.
        bad.must_fail(sc.name, "the script under the registry's id", &s.with(|s| { s.lib_id = Some(XID); s.outs[1].covenant = bind(1, XID); }), None);
        // The same script under a third id (a copy of the library's code that is not of the library).
        bad.must_fail(sc.name, "the script under an id that is not the library's", &s.with(|s| { s.lib_id = Some(YID); s.outs[1].covenant = bind(1, YID); }), None);
        // The object of another library's slot (it names another library id), under the same registry id.
        bad.must_fail(sc.name, "an object of another library under the registry's id", &s.with(|s| s.obj = tslot_under(YID, w.hash(sc.kind))), Some(0));
    }
    bad.done("B2/B3 objects and scripts");
    // The anchor's state is the hash of nine bytes (a pushed 7); every other state's preimage is longer and of a
    // fixed shape, so no entry reads an anchor as anything else (the kinds' table refuses it for every entry too).
    assert_eq!(W::anchor_hash(), blake2b(&push9(7)));
    assert_eq!(push9(7).len(), 9);
}

/// B1. A merge takes two positions of its registry and no third object of it: beside every scene whose object is put under
/// the same id (each accepted alone), the transaction is refused.
#[test]
fn t1b_b1_a_merge_beside_an_operation_on_an_object_of_the_same_registry() {
    let w = one_key_world();
    let a1 = ol(5 * UNIT, PX, 100 * KAS, 100 * KAS);
    let a2 = ol(3 * UNIT, PX, 70 * KAS, 70 * KAS);
    let v = scenes(&w);
    let mut tried = 0;
    for y in &v {
        let ys = Sc {
            name: y.name,
            s: y.s.with(|s| {
                s.obj_id = IDL;
                for o in s.outs.iter_mut() {
                    if o.covenant.is_some_and(|c| c.covenant_id == XID) {
                        o.covenant = bind(0, IDL);
                    }
                }
            }),
            kind: y.kind,
            dispute_daa: y.dispute_daa,
        };
        assert!(all_ok(&ys.s.tx()), "{} alone under the registry's id", y.name);
        let k = mrg(a1, a2, [0, 1, 2]).with(|k| k.n_in = 6);
        for len in [12usize, 14] {
            let mut s = Slots::new(len, 6);
            s.put(2, out(LIBV, p2sh(&k.lib), bind(2, LIB)), 2);
            s.any(out(k.out_val() as u64, spk(k.out_hash()), bind(0, IDL)), 2);
            place_scene(&ys, 3, 4, &mut s);
            let Some(o) = s.done() else { continue };
            let mut parts = k.parts();
            parts.extend(scene_parts(&ys.s, 3, 4, 70));
            let t = assemble(parts, 6, o.outs, ys.s.lock);
            assert!(refused(&t), "a merge beside {} of an object of the same registry: {}", y.name, show(&t));
            tried += 1;
        }
    }
    println!("T1b B1 merge: {tried} merges beside an operation on a third object of the same registry, all refused ({} scenes, each accepted alone)", v.len());
    assert!(tried >= v.len());
}

// ================================================================ round 9: the join (KpfTJoin) at the engine

use kpf_protocol::thin::joined;

const M3: [u8; 32] = [0x4E; 32];

/// A position of the same two users in another market (its scale and price limit other than the first's): the second
/// of a join. `turned`: its A is the first's B.
fn oy(turned: bool, n: i64, e: i64, am: i64, bm: i64) -> Position {
    let mut y = ol(n, e, am, bm);
    y.x.legs[0].market = M3;
    y.x.legs[0].scale = 1_000;
    y.x.legs[0].price_max = 50_000_000_000;
    if turned {
        std::mem::swap(&mut y.x.a_key, &mut y.x.b_key);
    }
    y
}
/// The state `KpfTJoin` writes, over the bytes it is shown (the script's own arithmetic, mirrored).
fn join_raw(st: &[u8], mv: &[u8], lg: &[u8], turned: i64, a2: i64, b2: i64, aq2: i64, bq2: i64) -> [u8; 32] {
    let at = |b: &[u8], i: usize| i64::from_le_bytes(b[i..i + 8].try_into().unwrap());
    let (ja, jb, jaq, jbq, dir) = if turned == 1 { (b2, a2, bq2, aq2, SHORT) } else { (a2, b2, aq2, bq2, LONG) };
    let half = MERGE_TIP / 2;
    let (am, bm) = (at(mv, 67) + ja - half, at(mv, 76) + jb - (MERGE_TIP - half));
    let (aq, bq) = (at(mv, 121) + jaq, at(mv, 130) + jbq);
    let nt = [&st[..201], &lg[..93], &push9(dir)[..], &lg[102..111]].concat();
    let nm = [&mv[..66], &push9(am)[..], &push9(bm)[..], &mv[84..120], &push9(aq)[..], &push9(bq)[..]].concat();
    h_position(&blake2b(&nt), &nm)
}

/// A join as a transaction, every field a knob.
#[derive(Clone)]
struct Jn {
    lib: SilAbiArtifact,
    st: Vec<u8>,
    mv: Vec<u8>,
    x: Position,
    y: Position,
    h1: [u8; 32],
    h2: [u8; 32],
    v1: i64,
    v2: i64,
    id1: Hash,
    id2: Hash,
    /// The second's leg (111 bytes), whether its keys are turned, its margins and asks, as the script is shown them.
    leg: Vec<u8>,
    turned: i64,
    a2: i64,
    b2: i64,
    aq2: i64,
    bq2: i64,
    /// The hash of the output written; by default what the script computes from what it is shown.
    out: Option<[u8; 32]>,
    /// Who signs for the first position's A and for its B (round 10: a join asks both).
    sa: Keypair,
    sb: Keypair,
    at: [usize; 3],
    n_in: usize,
    who: Vec<u8>,
    go: [(i64, i64); 2],
}
fn jn(x: Position, y: Position, at: [usize; 3]) -> Jn {
    Jn {
        lib: lib_of("KpfTJoin"),
        st: x.fixed(),
        mv: x.moving(),
        x,
        y,
        h1: x.hash(),
        h2: y.hash(),
        v1: x.pot(),
        v2: y.pot(),
        id1: IDL,
        id2: IDL,
        leg: y.fixed()[90..201].to_vec(),
        turned: (x.x.a_key != y.x.a_key) as i64,
        a2: y.x.a_margin,
        b2: y.x.b_margin,
        aq2: y.a_min_counter,
        bq2: y.b_min_counter,
        out: None,
        sa: owner(),
        sb: other(),
        at,
        n_in: 4,
        who: who_of([at[0] as i64, at[1] as i64, at[1] as i64]),
        go: [(at[2] as i64, 0), (at[2] as i64, 1)],
    }
}
impl Jn {
    fn with(&self, f: impl FnOnce(&mut Jn)) -> Jn {
        let mut k = self.clone();
        f(&mut k);
        k
    }
    fn out_hash(&self) -> [u8; 32] {
        self.out.unwrap_or_else(|| if self.st.len() == 312 && self.mv.len() == 138 && self.leg.len() == 111 { join_raw(&self.st, &self.mv, &self.leg, self.turned, self.a2, self.b2, self.aq2, self.bq2) } else { self.x.hash() })
    }
    fn out_val(&self) -> i64 {
        self.v1 + self.v2 - MERGE_TIP
    }
    fn patch(&self, script: &[u8]) -> Vec<u8> {
        // pushes: who(0) ageb(1) slotLen(2) st(3) moving(4) leg(5) turned(6) a2 b2 aq2 bq2
        let mut s = repush(script, 0, &self.who);
        s = repush(&s, 3, &self.st);
        s = repush(&s, 4, &self.mv);
        repush(&s, 5, &self.leg)
    }
    fn parts(&self) -> Vec<Part> {
        let int = |v: i64| Arg::V(ArtifactValue::Int(v));
        let o1 = oslot(self.h1);
        let o2 = oslot(self.h2);
        let args = vec![by(&[0; 24]), by(&[0; 8]), int(slot_len()), by(&[0; 312]), by(&[0; 138]), by(&[0; 111]), int(self.turned), int(self.a2), int(self.b2), int(self.aq2), int(self.bq2), Arg::Sig(self.sa), Arg::Sig(self.sb)];
        let me = self.clone();
        let obj = |n: u8, art: SilAbiArtifact, value: i64, id: Hash, g: (i64, i64)| In { outpoint: outpoint(n), utxo: UtxoEntry::new(value.max(0) as u64, p2sh(&art), 6_200, false, Some(id)), spend: Spend::Signed { art, entry: "go", args: vec![int(g.0), int(g.1)] } };
        vec![
            Part { at: self.at[0], input: obj(63, o1, self.v1, self.id1, self.go[0]), seq: 0, patch: None },
            Part { at: self.at[1], input: obj(64, o2, self.v2, self.id2, self.go[1]), seq: 0, patch: None },
            Part { at: self.at[2], input: In { outpoint: outpoint(65), utxo: UtxoEntry::new(KAS1, p2sh(&self.lib), 90, false, Some(LIB)), spend: Spend::Signed { art: self.lib.clone(), entry: "join", args } }, seq: 0, patch: Some(Box::new(move |s| me.patch(s))) },
        ]
    }
    fn outs(&self, len: usize) -> Vec<TransactionOutput> {
        let mut v: Vec<TransactionOutput> = (0..len).map(|_| change(1)).collect();
        v[self.at[2]] = out(LIBV, p2sh(&self.lib), bind(self.at[2] as u16, LIB));
        let free = (0..len).find(|i| *i != self.at[2] && !self.at[..2].contains(i)).unwrap();
        v[free] = out(self.out_val().max(0) as u64, spk(self.out_hash()), bind(self.at[0] as u16, self.id1));
        v
    }
    fn tx_with(&self, outs: Vec<TransactionOutput>) -> Tx {
        assemble(self.parts(), self.n_in, outs, 0)
    }
    fn tx(&self) -> Tx {
        self.tx_with(self.outs(self.n_in + 4))
    }
}
/// The pairs the join tests run on: (the first, the second), the first long or short for its A, the second's keys as
/// the first's or turned.
fn join_pairs() -> Vec<(&'static str, Position, Position)> {
    let mut xs = ol(5 * UNIT, PX, 100 * KAS, 120 * KAS);
    xs.x.legs[0].dir = SHORT;
    let mut asked = oy(true, 7_000, 2 * PX, 30 * KAS, 40 * KAS);
    asked.a_min_counter = 11 * KAS;
    asked.b_min_counter = 13 * KAS;
    let mut xa = ol(5 * UNIT, PX, 100 * KAS, 120 * KAS);
    xa.a_min_counter = 17 * KAS;
    xa.b_min_counter = 19 * KAS;
    vec![
        ("the keys in the same places", ol(5 * UNIT, PX, 100 * KAS, 120 * KAS), oy(false, 7_000, 2 * PX, 30 * KAS, 40 * KAS)),
        ("the keys turned", ol(5 * UNIT, PX, 100 * KAS, 120 * KAS), oy(true, 7_000, 2 * PX, 30 * KAS, 40 * KAS)),
        ("the first short for its A", xs, oy(false, 7_000, 2 * PX, 30 * KAS, 40 * KAS)),
        ("the first short, the keys turned", xs, oy(true, 7_000, 2 * PX, 30 * KAS, 40 * KAS)),
        ("asks on both, the keys turned", xa, asked),
    ]
}

/// J1, J3. A join of two one-leg positions of the same two users in two markets, in every orientation and wherever its
/// inputs stand, is taken when it writes the model's position (`thin::joined`, whose hash is what the script computes
/// from the bytes: checked here byte for byte through the hash) holding the two values less the tip; a sompi short
/// is refused; a second position of another id is refused.
#[test]
fn t1b_j1_a_join_writes_the_models_position_and_pays_the_two_pots_less_the_tip() {
    let mut tried = 0;
    for (what, x, y) in join_pairs() {
        let m = joined(&x, &y).unwrap_or_else(|e| panic!("{what}: {e}"));
        let turned = x.x.a_key != y.x.a_key;
        assert_eq!(m.pot(), x.pot() + y.pot() - MERGE_TIP, "{what}: the pot");
        assert_eq!(m.x.legs[0], x.x.legs[0], "{what}: the first's leg as it was");
        assert_eq!(m.x.legs[1], XLeg { dir: if turned { SHORT } else { LONG }, ..y.x.legs[0] }, "{what}: the second's leg, short for A when the keys were turned");
        let (ya, yb, yaq, ybq) = if turned { (y.x.b_margin, y.x.a_margin, y.b_min_counter, y.a_min_counter) } else { (y.x.a_margin, y.x.b_margin, y.a_min_counter, y.b_min_counter) };
        assert_eq!((m.x.a_margin, m.x.b_margin), (x.x.a_margin + ya - MERGE_TIP / 2, x.x.b_margin + yb - MERGE_TIP / 2), "{what}: each user's margins put together");
        assert_eq!((m.a_min_counter, m.b_min_counter), (x.a_min_counter + yaq, x.b_min_counter + ybq), "{what}: each user's asks put together");
        let k0 = jn(x, y, [0, 1, 2]);
        // The state the script writes is the model's, byte for byte: the fixed part and the moving part.
        let lg = &k0.leg;
        assert_eq!([&k0.st[..201], &lg[..93], &push9(m.x.legs[1].dir)[..], &lg[102..111]].concat(), m.fixed(), "{what}: the fixed part");
        assert_eq!(k0.out_hash(), m.hash(), "{what}: the hash");
        for at in arrangements(4) {
            let k = jn(x, y, at);
            let t = k.tx();
            assert!(all_ok(&t), "{what}, inputs {at:?}: {}", show(&t));
            let o = k.outs(k.n_in + 4);
            let ai = o.iter().position(|z| z.script_public_key == spk(m.hash())).unwrap();
            let short = { let mut o = o.clone(); o[ai].value -= 1; o };
            assert!(fails(&k.tx_with(short), at[2]), "{what}, inputs {at:?}: a sompi short at the position");
            let more = { let mut o = o.clone(); o[ai].value += 1; o };
            assert!(all_ok(&k.tx_with(more)), "{what}, inputs {at:?}: a sompi more");
            assert!(fails(&k.with(|k| k.id2 = IDS).tx(), at[2]), "{what}, inputs {at:?}: the second position of another id");
            tried += 4;
        }
    }
    println!("T1b J1: {tried} joins over 5 pairs and 24 arrangements of the inputs: the model's position, the two pots less the tip; short outputs and second positions of another id refused");
}

/// J1. Under the registry's id a join writes one output, the joined position, and goes on.
#[test]
fn t1b_j1_the_join_writes_only_the_joined_position() {
    let mut tried = 0;
    for (what, x, y) in join_pairs() {
        let k = jn(x, y, [0, 1, 2]);
        let o = k.outs(8);
        let ai = o.iter().position(|z| z.script_public_key == spk(k.out_hash())).unwrap();
        let free = (0..o.len()).find(|i| is_change(&o[*i])).unwrap();
        let mut bad = |why: &str, f: &dyn Fn(&mut Vec<TransactionOutput>)| {
            let mut z = o.clone();
            f(&mut z);
            assert!(refused(&k.tx_with(z)), "{what}: {why}");
            tried += 1;
        };
        bad("the position under another id", &|z| z[ai].covenant = bind(0, IDS));
        bad("the position under no id", &|z| z[ai].covenant = None);
        bad("no position", &|z| z[ai] = change(1));
        bad("the position written twice", &|z| z[free] = out(k.out_val() as u64, spk(k.out_hash()), bind(0, IDL)));
        for auth in 0..4u16 {
            bad(&format!("a position nobody signed under the registry's id, by input {auth}"), &|z| z[free] = out(KAS1, spk([0x55; 32]), bind(auth, IDL)));
            bad(&format!("a position nobody signed in the joined one's place, by input {auth}"), &|z| { z[ai].script_public_key = spk([0x55; 32]); z[ai].covenant = bind(auth, IDL); });
        }
        bad("the first position written again beside the joined one", &|z| z[free] = out(x.pot() as u64, spk(x.hash()), bind(0, IDL)));
        bad("the second position written again beside the joined one", &|z| z[free] = out(y.pot() as u64, spk(y.hash()), bind(1, IDL)));
        bad("the first position in the joined one's place", &|z| z[ai].script_public_key = spk(x.hash()));
        bad("the second leg written long where the keys were turned (or short where they were not)", &|z| z[ai].script_public_key = spk(join_raw(&k.st, &k.mv, &k.leg, 1 - k.turned, k.a2, k.b2, k.aq2, k.bq2)));
        bad("the script a sompi short", &|z| z[2].value -= 1);
        bad("the script under no id", &|z| z[2].covenant = None);
        bad("another script in the script's place", &|z| z[2].script_public_key = p2sh(&lib_of("KpfTMerge")));
        for who in [[2, 1, 1], [3, 1, 1], [0, 2, 2], [0, 3, 3], [9, 1, 1], [0, 0, 0], [1, 1, 1], [0, 1, 0], [0, 1, 2], [1, 0, 0]] {
            assert!(refused(&k.with(|k| k.who = who_of(who)).tx()), "{what}: who {who:?}");
            tried += 1;
        }
    }
    println!("T1b J1 writes: {tried} wrong writings and wrong places refused over 5 pairs");
}

/// J1. What is joined is two plain positions of one leg each, of the same two users and the same terms, in two markets.
#[test]
fn t1b_j1_only_two_one_leg_positions_of_the_same_two_users_are_joined() {
    let w = world();
    let mut tried = 0;
    for (what, x, y) in join_pairs() {
        let base = jn(x, y, [0, 1, 2]);
        assert!(all_ok(&base.tx()), "{what}");
        let mut no = |why: &str, k: Jn| {
            let t = k.tx();
            assert!(fails(&t, 2), "{what}: {why}: {}", show(&t));
            tried += 1;
        };
        // An object of another kind, an anchor among them, in either place.
        for kind in KINDS.iter().filter(|k| **k != K::Position) {
            no(&format!("a {kind:?} in the first's place"), base.with(|k| { k.h1 = w.hash(*kind); k.v1 = w.value(*kind); }));
            no(&format!("a {kind:?} in the second's place"), base.with(|k| { k.h2 = w.hash(*kind); k.v2 = w.value(*kind); }));
        }
        // A position of two legs in either place: the world's own, and a joined one (a join is not joined again).
        let two = joined(&x, &y).unwrap();
        for (name, acc) in [("the world's two-leg position", w.a), ("a position a join wrote", two)] {
            no(&format!("{name} as the first"), base.with(|k| { k.st = acc.fixed(); k.mv = acc.moving(); k.x = acc; k.h1 = acc.hash(); k.v1 = acc.pot(); k.out = Some(acc.hash()); }));
            no(&format!("{name} as the second"), base.with(|k| { k.h2 = acc.hash(); k.v2 = acc.pot(); }));
            no(&format!("{name} as the second, its first leg shown"), base.with(|k| { k.h2 = acc.hash(); k.v2 = acc.pot(); k.leg = acc.fixed()[90..201].to_vec(); k.a2 = acc.x.a_margin; k.b2 = acc.x.b_margin; k.out = Some(acc.hash()); }));
        }
        // The same position twice (two UTXOs of one state): one market.
        no("the first position as the second too", base.with(|k| { k.h2 = x.hash(); k.v2 = x.pot(); k.leg = x.fixed()[90..201].to_vec(); k.turned = 0; k.a2 = x.x.a_margin; k.b2 = x.x.b_margin; k.aq2 = x.a_min_counter; k.bq2 = x.b_min_counter; k.out = Some(x.hash()); }));
        // Another position of the first's market at another entry (the merge's, not the join's).
        let mut same_market = y;
        same_market.x.legs[0].market = x.x.legs[0].market;
        no("a second position of the first's market", jn(x, same_market, [0, 1, 2]).with(|k| k.out = Some(x.hash())));
        // The second's leg: short for its A, of no size; shown otherwise than it is.
        let mut short = y;
        short.x.legs[0].dir = SHORT;
        no("a second position short for its A", jn(x, short, [0, 1, 2]));
        let mut empty = y;
        empty.x.legs[0].size = 0;
        no("a second position of no size", jn(x, empty, [0, 1, 2]));
        for at in [0usize, 40, 70, 80, 90, 105] {
            no(&format!("the second's leg shown with byte {at} changed"), base.with(|k| k.leg[at] ^= 1));
        }
        no("a leg of 110 bytes", base.with(|k| { k.leg.pop(); k.out = Some(x.hash()); }));
        no("a leg of 112 bytes", base.with(|k| { k.leg.push(0); k.out = Some(x.hash()); }));
        // The keys' places stated wrongly, and a value that is neither.
        no("the keys' places stated the other way", base.with(|k| k.turned = 1 - k.turned));
        no("turned = 2", base.with(|k| k.turned = 2));
        no("turned = -1", base.with(|k| k.turned = -1));
        // The second's margins and asks shown otherwise than they are.
        no("the second's margins in each other's place", base.with(|k| std::mem::swap(&mut k.a2, &mut k.b2)));
        no("the second's A margin a sompi more", base.with(|k| k.a2 += 1));
        no("the second's B margin a sompi less", base.with(|k| k.b2 -= 1));
        no("the second's ask a sompi more", base.with(|k| k.aq2 += 1));
        no("the second's other ask a sompi more", base.with(|k| k.bq2 += 1));
        // A second position of other terms, of another user, of another dispute period.
        let mut terms = y;
        terms.x.t.mmr_bps += 1;
        no("a second position of another maintenance", jn(x, terms, [0, 1, 2]));
        let mut dispute_daa = y;
        dispute_daa.dispute_daa += 1;
        no("a second position of another dispute period", jn(x, dispute_daa, [0, 1, 2]));
        let mut late = y;
        late.x.expiry += 1;
        no("a second position of another no-price wait", jn(x, late, [0, 1, 2]));
        let mut third = y;
        third.x.b_key = xkey(&keypair(45));
        no("a second position with a third user", jn(x, third, [0, 1, 2]));
        // A margin that would not cover the sender's share.
        let mut poor = y;
        let mut px_ = x;
        px_.x.a_margin = 1;
        if x.x.a_key == y.x.a_key { poor.x.a_margin = 1 } else { poor.x.b_margin = 1 }
        no("margins of a sompi each on one side (the sender's share is not covered)", jn(px_, poor, [0, 1, 2]));
        // The first shown otherwise than it is.
        no("the first shown with another margin", base.with(|k| k.mv[70] ^= 1));
        no("the first shown with another entry", base.with(|k| k.st[195] ^= 1));
    }
    println!("T1b J1 kinds: {tried} joins of what is not two one-leg positions of the same two users, terms and registry, in two markets: all refused by the join's own input");
}

/// J1. A join takes two positions of its registry and no third object of it: beside every scene whose object is put under
/// the same id (each accepted alone), the transaction is refused.
#[test]
fn t1b_j1_a_join_beside_an_operation_on_an_object_of_the_same_registry() {
    let w = one_key_world();
    let (_, x, y) = join_pairs()[1];
    let v = scenes(&w);
    let mut tried = 0;
    for sc in &v {
        let ys = Sc {
            name: sc.name,
            s: sc.s.with(|s| {
                s.obj_id = IDL;
                for o in s.outs.iter_mut() {
                    if o.covenant.is_some_and(|c| c.covenant_id == XID) {
                        o.covenant = bind(0, IDL);
                    }
                }
            }),
            kind: sc.kind,
            dispute_daa: sc.dispute_daa,
        };
        assert!(all_ok(&ys.s.tx()), "{} alone under the registry's id", sc.name);
        let k = jn(x, y, [0, 1, 2]).with(|k| k.n_in = 6);
        for len in [12usize, 14] {
            let mut s = Slots::new(len, 6);
            s.put(2, out(LIBV, p2sh(&k.lib), bind(2, LIB)), 2);
            s.any(out(k.out_val() as u64, spk(k.out_hash()), bind(0, IDL)), 2);
            place_scene(&ys, 3, 4, &mut s);
            let Some(o) = s.done() else { continue };
            let mut parts = k.parts();
            parts.extend(scene_parts(&ys.s, 3, 4, 70));
            let t = assemble(parts, 6, o.outs, ys.s.lock);
            assert!(refused(&t), "a join beside {} of an object of the same registry: {}", sc.name, show(&t));
            tried += 1;
        }
    }
    println!("T1b J1 beside: {tried} joins beside an operation on a third object of the same registry, all refused ({} scenes)", v.len());
    assert!(tried >= v.len());
}

/// J5. The way in is two scripts. Each moves an anchor by its own entry only: the transaction of an opening with
/// `KpfTPlace`'s code in the script's place (the opening's arguments kept), and of a placement with `KpfTOpen`'s, is
/// refused by that input.
#[test]
fn t1b_j5_each_script_of_the_way_in_refuses_the_others_arguments() {
    let w = world();
    let v = scenes(&w);
    let swap = |name: &str, other: &'static str| -> Tx {
        let s = the(&v, name);
        let (mut tx, mut entries) = s.tx();
        let code = bytecode(&lib_of(other));
        let sig = tx.inputs[1].signature_script.clone();
        let c = chunks(&sig);
        let last = c.last().unwrap().clone();
        tx.inputs[1].signature_script = [&sig[..last.start], &raw_push(&code)[..]].concat();
        entries[1].script_public_key = p2sh(&lib_of(other));
        tx.outputs[1].script_public_key = p2sh(&lib_of(other));
        (tx, entries)
    };
    assert!(all_ok(&the(&v, "registry-open").tx()) && all_ok(&the(&v, "registry-place").tx()));
    let t = swap("registry-open", "KpfTPlace");
    // (The funding input's signature fails too, the outputs having changed; the anchor's own script does not.)
    assert!(fails(&t, 1) && !fails(&t, 0), "an opening's arguments under KpfTPlace: {}", show(&t));
    let t = swap("registry-place", "KpfTOpen");
    assert!(fails(&t, 1) && !fails(&t, 0), "a placement's arguments under KpfTOpen: {}", show(&t));
    // And every other script of the library in either's place (but the three proofs, which this file does not
    // compile: they are built in auditT1_builders.rs).
    let mut tried = 2;
    for other in OBJ_OPS.iter().filter(|s| !["KpfTPlace", "KpfTOpen", "KpfTProve", "KpfTLiqProve", "KpfTExitRefute", "KpfTPay"].contains(s)) {
        for name in ["registry-open", "registry-place"] {
            assert!(fails(&swap(name, *other), 1), "{name} under {other}");
            tried += 1;
        }
    }
    println!("T1b J5: {tried} transactions of the way in with another script's code in the script's place, all refused");
}

/// J3, a leg that is not well formed. A position enters the registry by the hash of its fixed part, so a leg is whatever
/// its two users signed. The join holds the second's leg to another market, a size above zero and "long" at the
/// places it reads, and to nothing else: a scale of nothing, a price limit of nothing, push bytes that are no pushes
/// are joined as they are (the byte before the direction is the one the join writes itself). Both users signed that
/// second position, and the first is theirs too: nobody else's money is in what is written.
#[test]
fn t1b_design_a_leg_its_two_users_made_ill_formed_is_joined_as_it_is() {
    let (_, x, y) = join_pairs()[0];
    let base = jn(x, y, [0, 1, 2]);
    let cases: [(&str, fn(&mut Vec<u8>)); 4] = [
        ("a scale of nothing", |lg| lg[67..75].copy_from_slice(&sm8(0))),
        ("a price limit of nothing", |lg| lg[76..84].copy_from_slice(&sm8(0))),
        ("push bytes that are no pushes", |lg| { for i in [0usize, 33, 66, 75, 84, 93, 102] { lg[i] = 0xEE } }),
        ("an entry above the price limit", |lg| lg[103..111].copy_from_slice(&sm8(i64::MAX / 4))),
    ];
    for (what, f) in cases {
        let mut lg = base.leg.clone();
        f(&mut lg);
        // The second position as its two users would have signed it: the first's terms, this leg, no second leg.
        let fixed2 = [&base.st[..90], &lg[..], &base.st[201..]].concat();
        let mv2 = [&base.mv[..66], &push9(base.a2)[..], &push9(base.b2)[..], &base.mv[84..120], &push9(base.aq2)[..], &push9(base.bq2)[..]].concat();
        let k = base.with(|k| { k.leg = lg.clone(); k.h2 = h_position(&blake2b(&fixed2), &mv2); });
        let t = k.tx();
        assert!(all_ok(&t), "{what}: {}", show(&t));
        // Read back: the model takes the bytes for no position (the tool does not read what the join wrote).
        let nt = [&base.st[..201], &lg[..93], &push9(LONG)[..], &lg[102..]].concat();
        assert_eq!(nt[201 + 93], 0x08, "{what}: the push before the direction is the join's own");
        assert_eq!(k.out_hash(), h_position(&blake2b(&nt), &[&base.mv[..66], &push9(x.x.a_margin + base.a2 - MERGE_TIP / 2)[..], &push9(x.x.b_margin + base.b2 - MERGE_TIP / 2)[..], &base.mv[84..120], &push9(x.a_min_counter + base.aq2)[..], &push9(x.b_min_counter + base.bq2)[..]].concat()));
    }
    println!("T1b DESIGN ill-formed leg: 4 second positions whose leg is not well formed (scale 0, limit 0, no pushes, entry above the limit) are joined as they are");
}

/// F15, fixed in round 10: a join carries the transaction signatures of the first position's A and B, in their places.
/// With any other key in either place, the two in each other's place, or one user's in both, the join's own input
/// refuses; with both it is taken. In every orientation: where the second's keys are turned, the places are still
/// the first position's.
#[test]
fn t1b_fixed_f15_a_join_is_signed_by_both_users_in_their_places() {
    let mut tried = 0;
    for (what, x, y) in join_pairs() {
        let base = jn(x, y, [0, 1, 2]);
        assert!(all_ok(&base.tx()), "{what}: signed by both");
        for (sa, sb) in [(other(), owner()), (owner(), owner()), (other(), other()), (keypair(45), other()), (owner(), keypair(45)), (keypair(45), keypair(46)), (keypair(60), keypair(60))] {
            let t = base.with(|k| { k.sa = sa; k.sb = sb; }).tx();
            assert_eq!(failed_inputs(&t), vec![2], "{what}: a join not signed by the two users in their places: {}", show(&t));
            tried += 1;
        }
    }
    println!("T1b F15 fixed: {tried} joins without the two users' signatures in their places refused by the join's own input, over 5 pairs");
}

// ================================================================ round 11: the turn (KpfTTurn) at the engine

/// L4. A turn writes the same position the other way round and nothing else: the users in each other's places (keys,
/// margins, asks; each leg of a size turned), or the two legs in each other's places. Any other mode, the legs of
/// a position of one leg, the position as it was, and the other mode's position in the output are refused; turned
/// twice the position is what it was.
#[test]
fn t1b_l4_a_turn_writes_the_position_the_other_way_round_and_nothing_else() {
    use kpf_protocol::thin::{turned_legs, turned_users};
    let w = world();
    let v = scenes(&w);
    let (users, legs) = (the(&v, "turn-users"), the(&v, "turn-legs"));
    let (tu, tl) = (turned_users(&w.a), turned_legs(&w.a).unwrap());
    assert_eq!(turned_users(&tu), w.a, "turned twice, the users are where they were");
    assert_eq!(turned_legs(&tl).unwrap(), w.a);
    assert_eq!((tu.x.a_key, tu.x.b_key, tu.x.a_margin, tu.x.b_margin, tu.x.legs[0].dir, tu.x.legs[1].dir), (w.a.x.b_key, w.a.x.a_key, w.a.x.b_margin, w.a.x.a_margin, SHORT, LONG));
    assert_eq!(tu.pot(), w.a.pot());
    let mut bad = Bad::new();
    bad.must_pass("turn-users", "as built", &users);
    bad.must_pass("turn-legs", "as built", &legs);
    for mode in [0i64, 3, -1, 1 << 40] {
        bad.must_fail("turn-users", &format!("mode {mode}"), &users.with(|s| s.rest = vec![A::I(mode)]), Some(1));
    }
    bad.must_fail("turn-users", "the position as it was in the output", &users.with(|s| s.outs[0].script_public_key = spk(w.a.hash())), Some(1));
    bad.must_fail("turn-users", "the legs turned in the output", &users.with(|s| s.outs[0].script_public_key = spk(tl.hash())), Some(1));
    bad.must_fail("turn-legs", "the users turned in the output", &legs.with(|s| s.outs[0].script_public_key = spk(tu.hash())), Some(1));
    bad.must_fail("turn-users", "the keys turned and the margins not", &users.with(|s| { let mut x = tu; std::mem::swap(&mut x.x.a_margin, &mut x.x.b_margin); s.outs[0].script_public_key = spk(x.hash()); }), Some(1));
    bad.must_fail("turn-users", "the users turned and a leg not", &users.with(|s| { let mut x = tu; x.x.legs[1].dir = SHORT; s.outs[0].script_public_key = spk(x.hash()); }), Some(1));
    bad.must_fail("turn-users", "a sompi less in the output", &users.with(|s| s.outs[0].value -= 1), Some(1));
    bad.must_pass("turn-users", "a sompi more in the output", &users.with(|s| s.outs[0].value += 1));
    // A position of one leg: its users are turned (the empty second leg stays as it is written), its legs are not.
    let mut z = w.clone();
    z.a.x.legs[1] = XLeg::none();
    z.f = z.a.fixed_hash().to_vec();
    z.mv = z.a.moving();
    let v1 = scenes(&z);
    let one = turned_users(&z.a);
    assert_eq!((one.x.legs[0].dir, one.x.legs[1]), (SHORT, XLeg::none()), "the empty leg is not turned");
    bad.must_pass("turn-users", "a position of one leg", &the(&v1, "turn-users"));
    assert!(turned_legs(&z.a).is_err());
    bad.must_fail("turn-legs", "a position of one leg (the position written as it was)", &the(&v1, "turn-legs"), Some(1));
    bad.done("L4 the turn");
}

// ---------------------------------------------------------------- round 19: the oracle fees (KpfFeePool, KpfTSweep, release)

const RID: [u8; 32] = [0x52; 32];
/// What a copy and the fee pool go on holding in these tests: what a copy of this file holds.
const FLOOR: i64 = KAS;
const SWEEP_MIN: i64 = 20_000_000;

fn fee_pool_art(rid: &[u8; 32], floor: i64) -> SilAbiArtifact {
    compile("KpfFeePool.sil", &[ArtifactValue::Bytes(kpf_protocol::thin::fee_pool_hash(rid, floor).to_vec())])
}
/// A copy of a script under the library's id, spent by `release(l, k)`.
fn release_in(n: u8, art: &SilAbiArtifact, value: u64, l: i64, k: i64) -> In {
    In { outpoint: outpoint(n), utxo: UtxoEntry::new(value, p2sh(art), 90, false, Some(LIB)), spend: Spend::Signed { art: art.clone(), entry: "release", args: vec![Arg::V(ArtifactValue::Int(l)), Arg::V(ArtifactValue::Int(k))] } }
}
/// An object (a slot, a fee pool) under `id`, spent by `go(l, k)`.
fn go_in(n: u8, art: &SilAbiArtifact, value: u64, id: Hash, l: i64, k: i64) -> In {
    In { outpoint: outpoint(n), utxo: UtxoEntry::new(value, p2sh(art), 90, false, Some(id)), spend: Spend::Signed { art: art.clone(), entry: "go", args: vec![Arg::V(ArtifactValue::Int(l)), Arg::V(ArtifactValue::Int(k))] } }
}

/// A sweep as a transaction: the fee pool at input 0 (`go`), KpfTSweep at 1, the copy swept at 2 (`release`), funding at
/// 3, then whatever else; the fee pool, the sweep's copy and the copy swept go on at outputs 0, 1 and 2.
#[derive(Clone)]
struct Sw {
    fee_pool: SilAbiArtifact,
    fee_pool_value: u64,
    fee_pool_go: (i64, i64),
    copy: SilAbiArtifact,
    copy_value: u64,
    rel: (i64, i64),
    who: [i64; 3],
    slot_len: i64,
    rid: Vec<u8>,
    floor: i64,
    outs: Vec<TransactionOutput>,
    more: Vec<In>,
}
fn sw(copy: &SilAbiArtifact, above: i64) -> Sw {
    let fee_pool = fee_pool_art(&RID, FLOOR);
    let moved = above;
    let outs = vec![
        out((10 * KAS + moved - moved / 20) as u64, p2sh(&fee_pool), bind(0, LIB)),
        out(KAS1, p2sh(&thin("KpfTSweep")), bind(1, LIB)),
        out(FLOOR as u64, p2sh(copy), bind(2, LIB)),
        change(900),
        change(1),
        change(1),
    ];
    Sw { slot_len: bytecode(&fee_pool).len() as i64, fee_pool, fee_pool_value: 10 * KAS1, fee_pool_go: (1, 0), copy: copy.clone(), copy_value: (FLOOR + above) as u64, rel: (1, 1), who: [0, 2, 2], rid: RID.to_vec(), floor: FLOOR, outs, more: vec![] }
}
impl Sw {
    fn with(&self, f: impl FnOnce(&mut Sw)) -> Sw {
        let mut s = self.clone();
        f(&mut s);
        s
    }
    fn tx(&self) -> Tx {
        let sweep = thin("KpfTSweep");
        let args = vec![by(&who_of(self.who)), by(&sm8(0)), Arg::V(ArtifactValue::Int(self.slot_len)), by(&self.rid), Arg::V(ArtifactValue::Int(self.floor))];
        let mut ins = vec![
            go_in(70, &self.fee_pool, self.fee_pool_value, LIB, self.fee_pool_go.0, self.fee_pool_go.1),
            In { outpoint: outpoint(71), utxo: UtxoEntry::new(KAS1, p2sh(&sweep), 90, false, Some(LIB)), spend: Spend::Signed { art: sweep.clone(), entry: "sweep", args } },
            release_in(72, &self.copy, self.copy_value, self.rel.0, self.rel.1),
            funding(73),
        ];
        ins.extend(self.more.iter().cloned());
        build_ext(&ins, self.outs.clone(), 0, &vec![0; ins.len()])
    }
}

/// Round 19, questions 1 to 3 at the sweep. What gathered in a copy reaches the fee pool less a twentieth, the copy goes
/// on as the same script under the library's id holding its floor, and nothing else is under the id: every other
/// arrangement of the outputs, the places and the arguments is refused.
#[test]
fn t1b_r19_a_sweep_moves_a_copys_dues_to_the_fee_pool_and_nothing_else() {
    let add = thin("KpfTAdd");
    let base = sw(&add, 50_000_000);
    assert!(all_ok(&base.tx()), "the honest sweep: {}", show(&base.tx()));
    // Every script that takes oracle fees is swept the same way (the three proofs are not compiled in this file).
    let mut swept = 0;
    for name in OBJ_OPS.iter().filter(|n| !COLLECTORS.contains(n) && !["KpfTProve", "KpfTLiqProve", "KpfTExitRefute"].contains(n)) {
        let c = lib_of(*name);
        assert!(all_ok(&sw(&c, SWEEP_MIN).tx()), "a copy of {name} holding the least above its floor");
        assert!(fails(&sw(&c, SWEEP_MIN - 1).with(|s| s.outs[0].value = (10 * KAS + SWEEP_MIN - 1 - (SWEEP_MIN - 1) / 20) as u64).tx(), 1), "a copy of {name} a sompi under the least");
        swept += 1;
    }
    let (mut tried, mut wrong) = (0, vec![]);
    let mut no = |what: &str, s: Sw, input: Option<usize>| {
        let t = s.tx();
        tried += 1;
        let refused = match input {
            Some(i) => fails(&t, i),
            None => refused(&t),
        };
        if !refused {
            wrong.push(format!("{what}: {}", show(&t)));
        }
    };
    let other = thin("KpfTBoth");
    let evil = spk([0x55; 32]);
    let part = 50_000_000 - 50_000_000 / 20;
    // ---- the outputs
    no("the fee pool a sompi short of its part", base.with(|s| s.outs[0].value -= 1), Some(1));
    no("the fee pool as it was: the sender keeps what moved", base.with(|s| s.outs[0].value = 10 * KAS1), Some(1));
    no("the copy a sompi under its floor, the fee pool a sompi more", base.with(|s| { s.outs[2].value -= 1; s.outs[0].value += 1; }), Some(1));
    no("the copy left with nothing", base.with(|s| { s.outs[2].value = 1; s.outs[0].value += KAS1; }), Some(1));
    no("another script of the family in the copy's place", base.with(|s| s.outs[2].script_public_key = p2sh(&other)), Some(1));
    no("an object in the copy's place, under the library's id", base.with(|s| s.outs[2].script_public_key = evil.clone()), Some(1));
    no("a key in the copy's place, under the library's id", base.with(|s| s.outs[2].script_public_key = p2pk_spk(&keypair(60))), Some(1));
    no("the copy under no id", base.with(|s| s.outs[2].covenant = None), None);
    no("the copy under another id", base.with(|s| s.outs[2].covenant = bind(2, YID)), None);
    no("the copy at another index", base.with(|s| s.outs.swap(2, 4)), None);
    no("the fee pool for another committee in the fee pool's place", base.with(|s| s.outs[0].script_public_key = p2sh(&fee_pool_art(&[0x53; 32], FLOOR))), Some(1));
    no("the fee pool of another floor in the fee pool's place", base.with(|s| s.outs[0].script_public_key = p2sh(&fee_pool_art(&RID, FLOOR - 1))), Some(1));
    no("a key in the fee pool's place", base.with(|s| s.outs[0].script_public_key = p2pk_spk(&keypair(60))), Some(1));
    no("the fee pool under no id", base.with(|s| s.outs[0].covenant = None), None);
    no("the fee pool and the copy in each other's places", base.with(|s| { s.outs.swap(0, 2); s.outs[0].covenant = bind(0, LIB); s.outs[2].covenant = bind(2, LIB); }), Some(1));
    no("the sweep's own copy a sompi short", base.with(|s| s.outs[1].value -= 1), Some(1));
    no("another script in the sweep's own place", base.with(|s| s.outs[1].script_public_key = p2sh(&other)), Some(1));
    for auth in 0..3u16 {
        no(&format!("one output more under the library's id (an object), by input {auth}"), base.with(|s| s.outs[4] = out(KAS1, evil.clone(), bind(auth, LIB))), None);
        no(&format!("a second copy of the copy swept under the id, by input {auth}"), base.with(|s| s.outs[4] = out(KAS1, p2sh(&add), bind(auth, LIB))), None);
        no(&format!("a second fee pool under the id, by input {auth}"), base.with(|s| s.outs[4] = out(KAS1, p2sh(&s.fee_pool), bind(auth, LIB))), None);
    }
    // ---- the places and the arguments
    for who in [[0, 2, 1], [0, 1, 1], [0, 0, 0], [2, 2, 2], [0, 3, 3], [0, 2, 3], [0, 3, 2], [0, -1, -1], [0, 9, 9], [2, 0, 0], [1, 2, 2], [3, 2, 2], [0, 2, 0], [0, 1 << 40, 1 << 40]] {
        no(&format!("who {who:?}"), base.with(|s| s.who = who), None);
    }
    no("the committee is not the fee pool's", base.with(|s| s.rid = vec![0x53; 32]), Some(1));
    for floor in [FLOOR - 1, FLOOR + 1, 0, -FLOOR, 1] {
        no(&format!("the floor told is {floor}"), base.with(|s| s.floor = floor), Some(1));
        // And with the copy left at that floor, the fee pool given the difference.
        if floor > 0 && floor < FLOOR {
            no(&format!("the floor told is {floor} and the copy is left it"), base.with(|s| { s.floor = floor; s.outs[2].value = floor as u64; s.outs[0].value += (FLOOR - floor) as u64; }), Some(1));
        }
    }
    for d in [-1, 1] {
        no("the fee pool's length a byte off", base.with(|s| s.slot_len += d), Some(1));
    }
    // ---- what the copy and the fee pool are told
    for (l, k) in [(0, 1), (2, 1), (3, 1), (1, 0), (1, 3), (1, -1), (9, 1), (-1, 1)] {
        no(&format!("release({l}, {k})"), base.with(|s| s.rel = (l, k)), Some(2));
    }
    for (l, k) in [(1, 1), (1, 2), (2, 0), (0, 0), (3, 0), (1, 3), (1, -1)] {
        no(&format!("the fee pool's go({l}, {k})"), base.with(|s| s.fee_pool_go = (l, k)), Some(0));
    }
    // ---- one input more under the library's id
    let rich = 6 * KAS1;
    no("a second copy let go beside, named nowhere", base.with(|s| { s.more = vec![release_in(74, &other, rich, 1, 1)]; s.outs[4] = out(KAS1, p2sh(&other), bind(4, LIB)); }), None);
    no("a second copy at the third place", base.with(|s| { s.who = [0, 2, 4]; s.more = vec![release_in(74, &other, rich, 1, 2)]; s.outs[4] = out(KAS1, p2sh(&other), bind(4, LIB)); }), None);
    no("a second fee pool let go beside at the third place", base.with(|s| { s.who = [0, 2, 4]; s.more = vec![go_in(74, &fee_pool_art(&[0x53; 32], FLOOR), rich, LIB, 1, 2)]; s.outs[4] = out(KAS1, p2sh(&fee_pool_art(&[0x53; 32], FLOOR)), bind(4, LIB)); }), None);
    // A second fee pool as the copy swept: one fee pool's holdings moved into the other. (`lib_check` accepts one fee pool:
    // the script does not ask what the third input under the id is, only that it goes on as it is at its floor.)
    let p2 = fee_pool_art(&[0x53; 32], FLOOR);
    let two = base.with(|s| { s.copy = p2.clone(); s.outs[2].script_public_key = p2sh(&p2); });
    let t = {
        let mut ins_s = two.clone();
        ins_s.more = vec![];
        // (The harness spends the third input by `release`; a fee pool is spent by `go`.)
        let sweep = thin("KpfTSweep");
        let args = vec![by(&who_of(ins_s.who)), by(&sm8(0)), Arg::V(ArtifactValue::Int(ins_s.slot_len)), by(&ins_s.rid), Arg::V(ArtifactValue::Int(ins_s.floor))];
        let ins = vec![go_in(70, &ins_s.fee_pool, ins_s.fee_pool_value, LIB, 1, 0), In { outpoint: outpoint(71), utxo: UtxoEntry::new(KAS1, p2sh(&sweep), 90, false, Some(LIB)), spend: Spend::Signed { art: sweep.clone(), entry: "sweep", args } }, go_in(72, &p2, ins_s.copy_value, LIB, 1, 1), funding(73)];
        build_ext(&ins, ins_s.outs.clone(), 0, &[0; 4])
    };
    let second_fee_pool_swept = all_ok(&t);
    // ---- more than the fee pool's part is fine (the sender gives its own).
    assert!(all_ok(&base.with(|s| s.outs[0].value += 1).tx()));
    // (Round 20, F24: a copy left above its floor is refused, whatever the fee pool is given.)
    assert!(fails(&base.with(|s| { s.outs[2].value += 10_000_000; s.outs[0].value = (10 * KAS + 40_000_000 - 2_000_000) as u64; }).tx(), 1), "a copy left above its floor, the fee pool given a twentieth less of what moved");
    assert!(fails(&base.with(|s| s.outs[2].value += 10_000_000).tx(), 1), "a copy left above its floor, the fee pool given its whole part");
    assert!(all_ok(&base.with(|s| s.rel = (1, 2)).tx()), "the copy named at the third place as at the second");
    let _ = part;
    assert!(wrong.is_empty(), "{} of {tried} accepted:\n{}", wrong.len(), wrong.join("\n"));
    println!("T1b R19 sweep: {swept} scripts swept at the least and refused a sompi under it; {tried} other arrangements of a sweep refused; a second fee pool under the id is swept into the first as a copy is: {second_fee_pool_swept}");
}

/// Round 19, question 1 at the engine, for the 22 scenes (every entry that reads one object). A copy of a script of
/// the library let go (`release`) beside the operation, and the fee pool (`go`), named at each place and at none: the
/// operation is refused, whatever is written for the copy (the same script holding less, or an object under the
/// library's id). The same for the two-object entries this file builds (match, merge, join), with a copy or the
/// fee pool in an object's place.
#[test]
fn t1b_r19_no_operation_passes_with_a_copy_or_the_fee_pool_at_a_place() {
    let w = world();
    let mut bad = Bad::new();
    let fee_pool = fee_pool_art(&RID, FLOOR);
    let rich = 6 * KAS1;
    let cases: [([i64; 3], i64, Option<usize>); 10] = [
        ([0, 3, 3], 1, Some(1)),
        ([0, 3, 3], 2, Some(1)),
        ([0, 3, 0], 1, Some(1)),
        ([0, 0, 3], 2, Some(1)),
        ([3, 0, 0], 0, Some(1)),
        ([3, 0, 3], 2, Some(1)),
        ([0, 0, 0], 0, Some(3)),
        ([0, 0, 0], 1, Some(3)),
        ([0, 0, 0], 2, Some(3)),
        ([3, 3, 3], 0, Some(1)),
    ];
    for sc in scenes(&w) {
        let s = &sc.s;
        let another = thin(if sc.name == "both" { "KpfTAdd" } else { "KpfTBoth" });
        for (who, k, input) in cases {
            for (label, art) in [("a second copy of the script itself", s.lib.clone()), ("a copy of another script", another.clone())] {
                for (wrote, o) in [("going on with less", out(KAS1, p2sh(&art), bind(3, LIB))), ("an object written under the library's id in its place", out(rich, spk([0x55; 32]), bind(3, LIB))), ("a key's output under the id in its place", out(rich, p2pk_spk(&keypair(60)), bind(3, LIB)))] {
                    let t = s.with(|s| {
                        s.who = who_of(who);
                        s.extra = vec![X::Rel(art.clone(), rich, 1, k)];
                        s.outs[3] = o.clone();
                        if who == [3, 3, 3] {
                            s.slot_len = bytecode(&art).len() as i64;
                        }
                    });
                    bad.must_fail(sc.name, &format!("{label} let go at input 3 naming place {k}, who {who:?}, {wrote}"), &t, input);
                }
            }
            let t = s.with(|s| {
                s.who = who_of(who);
                s.extra = vec![X::Go(fee_pool.clone(), rich, LIB, 1, k)];
                s.outs[3] = out(KAS1, p2sh(&fee_pool), bind(3, LIB));
                if who == [3, 3, 3] {
                    s.slot_len = bytecode(&fee_pool).len() as i64;
                }
            });
            bad.must_fail(sc.name, &format!("the fee pool at input 3 naming place {k}, who {who:?}"), &t, input);
        }
        // The fee pool as the object itself (input 0), under the library's id and under the object's.
        for id in [LIB, XID] {
            let t = s.with(|s| {
                s.obj = fee_pool.clone();
                s.obj_id = id;
                s.slot_len = bytecode(&fee_pool).len() as i64;
                // (What the entry writes for its object is then under the id the fee pool carries.)
                for o in s.outs.iter_mut() {
                    if let Some(c) = &mut o.covenant {
                        if c.covenant_id == XID {
                            c.covenant_id = id;
                        }
                    }
                }
            });
            bad.must_fail(sc.name, "the fee pool in the object's place", &t, Some(1));
        }
        // An object whose state is the fee pool's hash (a slot of the registry holding it).
        let t = s.with(|s| s.obj = tslot(kpf_protocol::thin::fee_pool_hash(&RID, FLOOR)));
        bad.must_fail(sc.name, "an object whose state is a fee pool's", &t, Some(1));
    }
    bad.done("R19 a copy or the fee pool at a place of an operation");
    // ---- the two-object entries: a copy or the fee pool in the first or the second object's place.
    let tpl = tpl_of();
    let tb = tpl.bytes();
    let lo = ord(&owner(), LONG, 3 * UNIT, PX + PX / 10, 40 * KAS, KAS, true, 0);
    let so = ord(&other(), SHORT, 3 * UNIT, PX - PX / 20, 60 * KAS, KAS, true, 0);
    let m = mx(&tb, &lo, &so, 3 * UNIT, PX, [0, 1, 2]);
    assert!(all_ok(&m.tx()));
    let g = mrg(ol(5 * UNIT, PX, 100 * KAS, 120 * KAS), ol(3 * UNIT, PX, 60 * KAS, 70 * KAS), [0, 1, 2]);
    assert!(all_ok(&g.tx()));
    let (_, jx, jy) = join_pairs().into_iter().next().unwrap();
    let j = jn(jx, jy, [0, 1, 2]);
    assert!(all_ok(&j.tx()));
    let another = thin("KpfTBoth");
    let mut tried = 0;
    let sets: Vec<(&str, Box<dyn Fn() -> Vec<Part>>, usize, Vec<TransactionOutput>)> = vec![
        ("match", Box::new(|| m.parts()), m.n_in, m.honest(m.n_in + 8).unwrap().outs),
        ("merge", Box::new(|| g.parts()), g.n_in, g.outs(g.n_in + 4)),
        ("join", Box::new(|| j.parts()), j.n_in, j.outs(j.n_in + 4)),
    ];
    for (name, parts, n_in, outs) in &sets {
        for place in 0..2usize {
            for which in ["a copy of another script", "the fee pool"] {
                let mut p = parts();
                let at = p[place].at;
                p[place].input = if which == "the fee pool" { go_in(80, &fee_pool, rich, LIB, 2, place as i64) } else { release_in(80, &another, rich, 2, place as i64) };
                p[place].patch = None;
                // What the attacker would write for it: itself, under the library's id, at its own index.
                let mut o = outs.clone();
                let art = if which == "the fee pool" { &fee_pool } else { &another };
                // (What the entry writes by that input is then under the id the input carries.)
                for x in o.iter_mut() {
                    if let Some(c) = &mut x.covenant {
                        if c.authorizing_input == at as u16 {
                            c.covenant_id = LIB;
                        }
                    }
                }
                o.push(out(KAS1, p2sh(art), bind(at as u16, LIB)));
                let t = assemble(p, *n_in, o, 0);
                assert!(fails(&t, 2), "{name}: {which} in the place of object {place}: {}", show(&t));
                tried += 1;
            }
        }
    }
    println!("T1b R19: {tried} transactions of match, merge and join with a copy or the fee pool in an object's place, all refused by the script");
}

/// Round 19, who may name a copy. `release` (and `KpfFeePool.go`, `KpfSlot.go`) asks that the input naming it is under
/// the library's id and that its signature script begins with the 24 bytes of the places. That input is meant to be
/// an entry of a script; this is what else it could be, at the engine:
///  - an input under no id or under another id (a key, an object of a registry): refused;
///  - two copies letting each other go, each one's `l` the other: refused (a signature script that begins with a
///    number does not begin with 24 bytes);
///  - the same with `l` pushed as 24 bytes that name the other at a place: refused (a number is at most 8 bytes);
///  - the same with a push of 24 bytes BEFORE the arguments, naming the other: refused (the engine asks a clean
///    stack: what a script does not consume fails it). This is the rule the whole family stands on: without it any
///    two inputs under the library's id would let each other go.
#[test]
fn t1b_r19_only_an_entry_that_begins_with_the_places_lets_a_copy_go() {
    let (a, b) = (thin("KpfTAdd"), thin("KpfTBoth"));
    let fee_pool = fee_pool_art(&RID, FLOOR);
    let rich = 6 * KAS1;
    // Outputs: both copies go on with less (the count of the id kept), the rest to a key.
    let outs = |x: &SilAbiArtifact, y: &SilAbiArtifact| vec![out(KAS1, p2sh(x), bind(0, LIB)), out(KAS1, p2sh(y), bind(1, LIB)), change(900)];
    let pair = |x: In, y: In, o: Vec<TransactionOutput>| build_ext(&[x, y, funding(73)], o, 0, &[0; 3]);
    // The plain forms.
    let t = pair(release_in(70, &a, rich, 1, 0), release_in(71, &b, rich, 0, 0), outs(&a, &b));
    assert!(fails(&t, 0) && fails(&t, 1), "two copies naming each other: {}", show(&t));
    let t = pair(release_in(70, &a, rich, 2, 0), release_in(71, &b, rich, 2, 0), outs(&a, &b));
    assert!(fails(&t, 0) && fails(&t, 1), "two copies naming the funding input: {}", show(&t));
    let t = pair(release_in(70, &a, rich, 1, 0), go_in(71, &tslot([0x55; 32]), rich, XID, 0, 0), vec![out(KAS1, p2sh(&a), bind(0, LIB)), change(900)]);
    assert!(fails(&t, 0), "a copy naming an object of a registry: {}", show(&t));
    let t = pair(release_in(70, &a, rich, 1, 0), go_in(71, &fee_pool, rich, LIB, 0, 0), outs(&a, &fee_pool));
    assert!(fails(&t, 0) && fails(&t, 1), "a copy and the fee pool naming each other: {}", show(&t));
    // ---- the crafted forms. The first push of each signature script made 24 bytes naming the other input.
    let names = |other: i64| who_of([other, other, other]);
    let mut crafted = 0;
    for (label, x, y, o) in [
        ("two copies", release_in(70, &a, rich, 1, 0), release_in(71, &b, rich, 0, 0), outs(&a, &b)),
        ("a copy and the fee pool", release_in(70, &a, rich, 1, 0), go_in(71, &fee_pool, rich, LIB, 0, 0), outs(&a, &fee_pool)),
        ("the fee pool and a copy of the sweep's script let go", go_in(70, &fee_pool, rich, LIB, 1, 0), release_in(71, &b, rich, 0, 0), outs(&fee_pool, &b)),
    ] {
        // (a) `l` itself as 24 bytes.
        let mut t = pair(x.clone(), y.clone(), o.clone());
        t.0.inputs[0].signature_script = repush(&t.0.inputs[0].signature_script, 0, &names(1));
        t.0.inputs[1].signature_script = repush(&t.0.inputs[1].signature_script, 0, &names(0));
        assert!(fails(&t, 0) && fails(&t, 1), "{label}, l as 24 bytes: {}", show(&t));
        // (b) 24 bytes pushed before the arguments, which stay as they were.
        let mut t = pair(x.clone(), y.clone(), o.clone());
        t.0.inputs[0].signature_script = [raw_push(&names(1)), t.0.inputs[0].signature_script.clone()].concat();
        t.0.inputs[1].signature_script = [raw_push(&names(0)), t.0.inputs[1].signature_script.clone()].concat();
        assert!(fails(&t, 0) && fails(&t, 1), "{label}, 24 bytes before the arguments: {}", show(&t));
        // (c) one of the two honest about nothing: only the first crafted, the second naming it plainly.
        let mut t = pair(x.clone(), y.clone(), o.clone());
        t.0.inputs[0].signature_script = [raw_push(&names(1)), t.0.inputs[0].signature_script.clone()].concat();
        assert!(fails(&t, 0), "{label}, one crafted: {}", show(&t));
        crafted += 3;
    }
    // The same on an object of a registry: a slot let go by a copy whose signature script was given the places in front.
    let slot = tslot([0x55; 32]);
    let mut t = build_ext(&[go_in(70, &slot, rich, XID, 1, 0), release_in(71, &a, rich, 0, 0), funding(73)], vec![out(KAS1, p2sh(&a), bind(1, LIB)), change(900)], 0, &[0; 3]);
    t.0.inputs[1].signature_script = [raw_push(&names(0)), t.0.inputs[1].signature_script.clone()].concat();
    assert!(fails(&t, 1), "an object named by a copy let go, the places pushed before its arguments: {}", show(&t));
    // And a real entry given a push before its arguments (the clean stack again): an add that is otherwise valid.
    let w = world();
    let add = the(&scenes(&w), "add");
    assert!(all_ok(&add.tx()));
    let mut t = add.tx();
    t.0.inputs[1].signature_script = [raw_push(&[0x18; 24]), t.0.inputs[1].signature_script.clone()].concat();
    assert!(fails(&t, 1), "an entry with a push before its arguments: {}", show(&t));
    println!("T1b R19: a copy is let go by no input but an entry whose first argument is the places: {crafted} crafted pairs refused (a number is at most 8 bytes; a script leaves a clean stack)");
}

/// Round 19, a sweep beside an operation. The fee pool, the sweep's own copy and the copy swept are all that is under
/// the library's id in a sweep's transaction: an add that is valid on its own, in the same transaction, refuses the
/// sweep (the add's own script passes).
#[test]
fn t1b_r19_a_sweep_is_alone_with_the_fee_pool_and_its_copy() {
    let w = world();
    let add = the(&scenes(&w), "add");
    let base = sw(&thin("KpfTBoth"), 50_000_000);
    // The sweep's four inputs, then the add's object at 4 and its script at 5.
    let s = add.with(|s| { s.go = (5, 0); s.who = who_of([4, 4, 4]); });
    let mut outs = base.outs.clone();
    outs[3] = change(800);
    outs[4] = out(s.outs[0].value, s.outs[0].script_public_key.clone(), bind(4, XID));
    outs[5] = out(LIBV, p2sh(&s.lib), bind(5, LIB));
    let both = base.with(|b| { b.more = vec![s.obj_in(80), s.lib_in(81)]; b.outs = outs.clone(); });
    let mut t = both.tx();
    t.0.inputs[5].signature_script = s.patch(&t.0.inputs[5].signature_script);
    // The add's signature was made before its signature script was patched: the patch changes no signed field
    // (signature scripts are not signed), so the add itself passes.
    let r = run(&t.0, &t.1).expect("run");
    assert!(r[5].is_ok() && r[4].is_ok(), "the add beside the sweep is itself valid: {}", show(&t));
    assert!(r[1].is_err(), "a sweep beside an operation: {}", show(&t));
    // The sweep's copy swept being the add's own script copy (one script in two roles) is the same count.
    println!("T1b R19: a sweep with an add beside it is refused by the sweep (four inputs under the library's id), the add's own script passing");
}

/// F24 (Low in round 19), fixed in round 20. A sweep left the copy at its floor OR ABOVE, so a copy holding three
/// times the least was swept in three sweeps of the least, a block each, each paying its sender. The copy now goes on
/// holding exactly its floor: a sweep moves everything above it or nothing. The three pieces of round 19, each
/// refused; the copy left a sompi above its floor, refused; the whole, taken. And what the exact floor could make
/// impossible: a copy that holds less than its floor is not swept (nothing is above it), and is swept again once it
/// holds the floor and the least, like any other.
#[test]
fn t1b_fixed_f24_a_copy_is_swept_whole_or_not_at_all() {
    let add = thin("KpfTAdd");
    let (mut copy, mut fee_pool) = (FLOOR + 3 * SWEEP_MIN, 10 * KAS);
    for piece in 1..=3 {
        let s = sw(&add, 0).with(|s| {
            s.copy_value = copy as u64;
            s.fee_pool_value = fee_pool as u64;
            s.outs[2].value = (copy - SWEEP_MIN) as u64;
            s.outs[0].value = (fee_pool + SWEEP_MIN - SWEEP_MIN / 20) as u64;
        });
        // The last piece leaves the copy at its floor: it is a whole sweep of a copy holding the least, and taken.
        if copy - SWEEP_MIN == FLOOR {
            assert!(all_ok(&s.tx()), "the last piece is a whole sweep: {}", show(&s.tx()));
        } else {
            assert!(fails(&s.tx(), 1), "F24 is open again: piece {piece} of a copy swept in pieces is taken: {}", show(&s.tx()));
            // The fee pool given the whole of what the copy holds above its floor, the copy still left above: refused.
            assert!(fails(&s.with(|s| s.outs[0].value += (3 * SWEEP_MIN) as u64).tx(), 1));
        }
        copy -= SWEEP_MIN;
        fee_pool += SWEEP_MIN - SWEEP_MIN / 20;
    }
    let whole = sw(&add, 3 * SWEEP_MIN);
    assert!(all_ok(&whole.tx()), "the copy swept whole");
    for d in [1i64, -1, 1_000_000] {
        assert!(fails(&whole.with(|s| { s.outs[2].value = (FLOOR + d) as u64; s.outs[0].value += 1_000_000; }).tx(), 1), "the copy left {d} sompi from its floor");
    }
    // The sender may still give the fee pool more than it must (its own money).
    assert!(all_ok(&whole.with(|s| s.outs[0].value += 1).tx()));
    // A copy holding less than its floor: not swept, whatever it is left with; holding the floor and the least: swept.
    for held in [FLOOR - 1, FLOOR / 2, FLOOR, FLOOR + SWEEP_MIN - 1] {
        for left in [held, FLOOR] {
            let s = sw(&add, 0).with(|s| { s.copy_value = held as u64; s.outs[2].value = left as u64; s.outs[0].value = 10 * KAS1; });
            assert!(fails(&s.tx(), 1), "a copy holding {held} left {left}");
        }
    }
    assert!(all_ok(&sw(&add, SWEEP_MIN).tx()));
    println!("T1b F24 fixed: a copy holding three times the least is not swept in pieces (two pieces refused, the last is a whole sweep); left a sompi above or under its floor: refused; a copy under its floor is not swept until it holds the floor and the least");
}
