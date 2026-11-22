//! Fixtures and transaction shapes of the committee family (`docs/DESIGN-COMMITTEE.md`): the committee (`KpfCommittee`), its
//! epoch tables (`KpfEpochTable`) and exits (`KpfExit`). The templates are compiled from source once per test binary; states and
//! values come from the library model (`kpf_protocol::committee`), so every accepted transaction here is also the
//! model's prediction.
#![allow(dead_code)]

use std::sync::OnceLock;

use kaspa_consensus_core::tx::{ScriptPublicKey, Transaction, TransactionOutpoint, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kpf_protocol::artifacts::{splice, Template};
use kpf_protocol::committee::*;
use kpf_protocol::state::sm8;
use secp256k1::Keypair;
use silverscript_abi::{ArtifactValue, SilAbiArtifact};

use crate::common::*;

mod family;
pub use family::*;

pub struct CK {
    pub epoch_table: SilAbiArtifact,
    pub epoch_table_t: Template,
    pub exit: SilAbiArtifact,
    pub exit_t: Template,
    pub committee: SilAbiArtifact,
    pub committee_t: Template,
    pub charge: SilAbiArtifact,
    pub charge_t: Template,
    pub attest_tag: [u8; 4],
}

fn tag(art: &SilAbiArtifact, entry: &str) -> [u8; 4] {
    *art.contracts.values().next().unwrap().entries[entry].dispatch_tag.as_bytes()
}

/// The fixture committee's parameters: epochs of 1,000 DAA, exits of 500, members of at least 10 KAS, a quorum of 67 %
/// within 0.5 % of the price, no epoch table below 30 KAS of total stake, 3 epoch tables an epoch of 1 KAS each.
pub fn rparams() -> CommitteeParams {
    CommitteeParams { epoch: 1_000, exit: 500, min_member_stake: 10 * KAS, quorum_bps: 6_700, spread_bps: 50, min_total: 30 * KAS, epoch_tables: 3, epoch_table_value: KAS, charge: cparams() }
}

/// The fixture's judge and charge parameters: a judge stake of 20 KAS aged 200 DAA, rounds of 100 DAA; a charge costs
/// 5 KAS and waits 300 DAA for a contest.
pub fn cparams() -> ChargeParams {
    use kpf_protocol::judge::{JudgeMarket, JudgeParams};
    ChargeParams { judge: JudgeMarket { id: [0x6a; 32], p: JudgeParams { mu_bps: 12_500, age: 200, unbond: 200, round: 100, burn_bps: 1_000, min_stake: 20 * KAS } }, bond: 5 * KAS, daa: 300 }
}

pub const RID: Hash = Hash::from_bytes([0x52; 32]);
pub const MARKET: [u8; 32] = [0x4d; 32];

pub fn rid() -> [u8; 32] {
    RID.as_bytes()
}

pub fn member_kp(i: usize) -> Keypair {
    keypair(100 + i as u8)
}

pub fn x(kp: &Keypair) -> [u8; 32] {
    kp.x_only_public_key().0.serialize()
}

/// A full table whose weights are `w` KAS, member i held by `member_kp(i)`.
pub fn table(w: [i64; MEMBERS]) -> Table {
    let mut t = EMPTY_TABLE;
    for i in 0..MEMBERS {
        if w[i] > 0 {
            t[i] = Member { key: x(&member_kp(i)), weight: w[i] * KAS };
        }
    }
    t
}

/// 100 units of weight: the three largest members hold exactly 67.
pub const TOP3: [i64; MEMBERS] = [30, 22, 15, 12, 8, 2, 2, 1, 1, 1, 1, 1, 1, 1, 2];

pub fn ck() -> &'static CK {
    static K: OnceLock<CK> = OnceLock::new();
    K.get_or_init(|| {
        // The pinned artifacts (`kpf-artifacts check` holds them to the sources): the committee, the exit and the charge
        // are compiled to a fixed point of each other's constants, which the generator does once.
        use kpf_protocol::committee_artifacts as pinned;
        let (epoch_table, exit, committee, charge) = (pinned::epoch_table_base().clone(), pinned::exit_base().clone(), pinned::committee_base().clone(), pinned::charge_base().clone());
        let (epoch_table_t, exit_t, committee_t, charge_t) = (Template::of("KpfEpochTable", &epoch_table), Template::of("KpfExit", &exit), Template::of("KpfCommittee", &committee), Template::of("KpfCharge", &charge));
        let attest_tag = tag(&epoch_table, "attest");
        CK { epoch_table, epoch_table_t, exit, exit_t, committee, committee_t, charge, charge_t, attest_tag }
    })
}

pub fn committee_art(r: &Committee) -> SilAbiArtifact {
    splice(&ck().committee, &r.state_bytes())
}
pub fn epoch_table_art(t: &EpochTable) -> SilAbiArtifact {
    splice(&ck().epoch_table, &t.state_bytes())
}
pub fn exit_art(e: &Exit) -> SilAbiArtifact {
    splice(&ck().exit, &e.state_bytes())
}
pub fn charge_art(c: &Charge) -> SilAbiArtifact {
    splice(&ck().charge, &c.state_bytes())
}
pub fn charge_spk(c: &Charge) -> ScriptPublicKey {
    p2sh(&charge_art(c))
}
pub fn committee_spk(r: &Committee) -> ScriptPublicKey {
    p2sh(&committee_art(r))
}
pub fn epoch_table_spk(t: &EpochTable) -> ScriptPublicKey {
    p2sh(&epoch_table_art(t))
}
pub fn exit_spk(e: &Exit) -> ScriptPublicKey {
    p2sh(&exit_art(e))
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

pub type LCommittee = Live<Committee>;
pub type LEpochTable = Live<EpochTable>;
pub type LExit = Live<Exit>;

fn utxo(value: u64, spk: ScriptPublicKey, daa: u64, id: Option<Hash>) -> UtxoEntry {
    UtxoEntry::new(value, spk, daa, false, id)
}

pub fn committee_in(l: &LCommittee, entry: &'static str, args: Vec<Arg>) -> In {
    let art = committee_art(&l.s);
    In { outpoint: l.op, utxo: utxo(l.value, p2sh(&art), l.daa, l.id), spend: Spend::Signed { art, entry, args } }
}
pub fn epoch_table_in(l: &LEpochTable, entry: &'static str, args: Vec<Arg>) -> In {
    let art = epoch_table_art(&l.s);
    In { outpoint: l.op, utxo: utxo(l.value, p2sh(&art), l.daa, l.id), spend: Spend::Signed { art, entry, args } }
}
pub fn exit_in(l: &LExit, entry: &'static str, args: Vec<Arg>) -> In {
    let art = exit_art(&l.s);
    In { outpoint: l.op, utxo: utxo(l.value, p2sh(&art), l.daa, l.id), spend: Spend::Signed { art, entry, args } }
}
/// A plain P2PK input of `kp` signed SIGHASH_ALL.
pub fn key_in(op: TransactionOutpoint, value: u64, kp: &Keypair) -> In {
    In { outpoint: op, utxo: utxo(value, p2pk_spk(kp), 0, None), spend: Spend::P2pk(*kp) }
}

pub fn exec_kp() -> Keypair {
    keypair(60)
}
/// The executor's funding input: 1,000,000 KAS, so that no shape here is short of funds.
pub fn exec_in(n: u8) -> In {
    key_in(outpoint(n), 100_000_000_000_000, &exec_kp())
}
pub fn change(v: u64) -> TransactionOutput {
    out(v, p2pk_spk(&exec_kp()), None)
}

/// A built transaction: (tx, entries).
pub type T = (Transaction, Vec<UtxoEntry>);

pub fn accepted(t: &T) -> bool {
    match run(&t.0, &t.1) {
        Ok(v) => v.iter().all(|r| r.is_ok()),
        Err(_) => false,
    }
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

/// The UTXO output `j` of `t` becomes at DAA `daa`.
pub fn live<S: Clone>(t: &T, j: usize, daa: u64, s: &S) -> Live<S> {
    let o = &t.0.outputs[j];
    Live { op: TransactionOutpoint { transaction_id: t.0.id(), index: j as u32 }, value: o.value, daa, id: o.covenant.map(|c| c.covenant_id), s: s.clone() }
}

/// A committee UTXO of the fixture id holding exactly its members' stake and 1 KAS.
pub fn committee(r: &Committee) -> LCommittee {
    Live { op: outpoint(1), value: (r.total() + KAS) as u64, daa: 0, id: Some(RID), s: *r }
}
pub fn epoch_table(t: &EpochTable) -> LEpochTable {
    Live { op: outpoint(2), value: rparams().epoch_table_value as u64, daa: 0, id: Some(RID), s: *t }
}

// ---------------------------------------------------------------- transaction shapes

thread_local! {
    static REST: std::cell::Cell<u64> = const { std::cell::Cell::new(REST_DAA) };
}

/// The sequence the committee input of a join, top-up or leave is given: REST_DAA unless a test sets another.
pub fn rest_seq() -> u64 {
    REST.with(|r| r.get())
}

/// Runs `f` with the committee input's sequence set to `seq` (what `this.ageDaa >= REST_DAA` reads).
pub fn with_rest<T>(seq: u64, f: impl FnOnce() -> T) -> T {
    let old = REST.with(|r| r.replace(seq));
    let v = f();
    REST.with(|r| r.set(old));
    v
}
// Input 0 is the committee (or epoch table, or exit), input 1 the executor. The committee continues at output 0, an exit is
// output 1, epoch tables follow the committee; the executor's change is last.

fn pre_suf(t: &Template) -> [Arg; 2] {
    [bv(&t.prefix), bv(&t.suffix)]
}

/// What a join writes: the committee's continuation, the exit of an outbid holder, the change. `tweak` edits the outputs
/// before the transaction is built.
pub fn join_tx(l: &LCommittee, i: usize, key: [u8; 32], amount: i64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let mut outs = vec![];
    match l.s.join(i, key, amount) {
        Ok((r, exit, delta)) => {
            outs.push(out((l.value as i64 + delta) as u64, committee_spk(&r), bind(0, RID)));
            if let Some(e) = exit {
                outs.push(out(e.amount as u64, exit_spk(&e), bind(0, RID)));
            }
        }
        // What a builder that ignored the rule would write: the member replaced, no exit.
        Err(_) => {
            let mut r = l.s;
            r.members[i.min(MEMBERS - 1)] = Member { key, weight: amount };
            outs.push(out(l.value + amount.max(0) as u64, committee_spk(&r), bind(0, RID)));
        }
    }
    outs.push(change(1));
    tweak(&mut outs);
    let [pre, suf] = pre_suf(&ck().exit_t);
    build_ext(&[committee_in(l, "join", vec![iv(i as i64), bv(&key), iv(amount), pre, suf]), exec_in(60)], outs, 0, &[rest_seq()])
}

pub fn top_up_tx(l: &LCommittee, i: usize, amount: i64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let mut r = l.s;
    r.members[i].weight += amount;
    let mut outs = vec![out((l.value as i64 + amount) as u64, committee_spk(&r), bind(0, RID)), change(1)];
    tweak(&mut outs);
    build_ext(&[committee_in(l, "topUp", vec![iv(i as i64), iv(amount)]), exec_in(60)], outs, 0, &[rest_seq()])
}

pub fn leave_tx(l: &LCommittee, i: usize, signer: &Keypair, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let mut r = l.s;
    let e = Exit { key: r.members[i].key, amount: r.members[i].weight, release: l.s.release(), hold: kpf_protocol::committee::NO_HOLD, charge: rparams().charge };
    r.members[i] = Member::EMPTY;
    let mut outs = vec![
        out((l.value as i64 - e.amount) as u64, committee_spk(&r), bind(0, RID)),
        out(e.amount.max(1) as u64, exit_spk(&e), bind(0, RID)),
        change(1),
    ];
    tweak(&mut outs);
    let [pre, suf] = pre_suf(&ck().exit_t);
    build_ext(&[committee_in(l, "leave", vec![iv(i as i64), Arg::Sig(*signer), pre, suf]), exec_in(60)], outs, 0, &[rest_seq()])
}

/// What a roll to `from + m E` writes, whatever the lock time: the committee and its epoch tables.
pub fn roll_outs(l: &LCommittee, m: i64) -> (Committee, EpochTable, Vec<TransactionOutput>) {
    let p = l.s.p;
    let nf = l.s.from + m * p.epoch;
    let mut r = l.s;
    r.from = nf;
    let t = EpochTable { members: l.s.members, total: l.s.total(), from: nf, until: nf + p.epoch, quorum_bps: p.quorum_bps, spread_bps: p.spread_bps };
    let mut outs = vec![out(l.value, committee_spk(&r), bind(0, RID))];
    for _ in 0..p.epoch_tables {
        outs.push(out(p.epoch_table_value as u64, epoch_table_spk(&t), bind(0, RID)));
    }
    outs.push(change(1));
    (r, t, outs)
}

pub fn roll_tx(l: &LCommittee, m: i64, lock: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let (_, _, mut outs) = roll_outs(l, m);
    tweak(&mut outs);
    let [pre, suf] = pre_suf(&ck().epoch_table_t);
    build_ext(&[committee_in(l, "roll", vec![iv(m), pre, suf]), exec_in(60)], outs, lock, &[])
}

/// An attest with the given arguments at a lock time of `lock`: the epoch table continues at output 0.
pub fn attest_tx(l: &LEpochTable, a: &Attestation, lock: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let mut outs = vec![out(l.value, epoch_table_spk(&l.s), bind(0, RID)), change(1)];
    tweak(&mut outs);
    let args = a.args().into_iter().map(Arg::V).collect();
    build_ext(&[epoch_table_in(l, "attest", args), exec_in(60)], outs, lock, &[])
}

pub fn retire_tx(l: &LEpochTable, lock: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let mut outs = vec![change(1)];
    tweak(&mut outs);
    build_ext(&[epoch_table_in(l, "retire", vec![]), exec_in(60)], outs, lock, &[])
}

pub fn withdraw_tx(l: &LExit, lock: u64, tweak: impl FnOnce(&mut Vec<TransactionOutput>)) -> T {
    let mut outs = vec![out(l.s.amount as u64, ScriptPublicKey::new(0, [vec![0x20], l.s.key.to_vec(), vec![0xac]].concat().into()), None), change(1)];
    tweak(&mut outs);
    build_ext(&[exit_in(l, "withdraw", vec![]), exec_in(60)], outs, lock, &[])
}

/// Signed prices of the members `(i, price)` of the fixture table for a signed price.
pub fn signed_prices(t0: i64, list: &[(usize, i64)]) -> Vec<SignedPrice> {
    list.iter().map(|(i, p)| sign_price(&member_kp(*i), &rid(), &MARKET, t0, *p)).collect()
}
