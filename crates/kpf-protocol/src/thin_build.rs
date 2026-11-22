//! The builders of the thin family (docs/DESIGN-COMMITTEE.md 8a): a library's genesis, a position's opening, and every
//! operation the library's scripts allow except the proof (which waits for the price notes).
//!
//! Every operation has the same shape. Input 0 is the object (`KpfSlot.go`, which accepts a script of its library that
//! names it), input 1 the script of the operation (a UTXO of the library, which goes on unchanged as output 1), then
//! the funder's inputs. Output 0 is what the object becomes, or the first payout if it ends; payouts the scripts place
//! from the end of the outputs are written last.

use kaspa_consensus_core::tx::ScriptPublicKey;
use kaspa_consensus_core::Hash;
use kaspa_txscript::pay_to_script_hash_script;
use silverscript_abi::ArtifactValue;

use crate::error::{invalid, Error, Result};
use crate::oracle_build::{OBuilt, ODraft, OLive, OPartial, OPlan, OWhich};
use crate::state::sm8;
use crate::thin::{folded, joined, left, match_orders, merged, novated, turned_legs, turned_users, Position, Leaving, Novation, Claim, Disputed, Liq, LiqObj, LiqState, Matched, Order, ExitClaim, Rest, TObj, Template, ORACLE_FEE, MERGE_TIP, ORDER_MIN};
use crate::thin_artifacts::{self as pinned, COLLECTORS, FEE_POOL, SCRIPTS, SLOT};
use crate::tx::{Arg, FeeMode, KeySet, KeyUtxo};

/// What an opening's payload begins with, before the position's fixed and moving parts.
pub const OPENING_MAGIC: [u8; 4] = *b"KPF\x01";
/// The most copies of each script a library's genesis writes.
pub const COPIES_MAX: usize = 8;
/// The least a script of a library holds (an output's storage mass falls with its value, and every use writes it
/// again).
pub const SCRIPT_VALUE_MIN: u64 = 100_000_000;

/// A copy of a script of a library on the chain (`s`: its name).
pub type LibScript = OLive<&'static str>;

/// Who pays for a transaction and where its change goes.
#[derive(Clone, Copy, Debug)]
pub struct Payer<'a> {
    pub funds: &'a [KeyUtxo],
    pub change_to: [u8; 32],
    pub rate: u64,
}

fn iv(v: i64) -> Arg {
    Arg::V(ArtifactValue::Int(v))
}

fn bv(b: Vec<u8>) -> Arg {
    Arg::V(ArtifactValue::Bytes(b))
}

fn script_of(name: &str) -> Result<&'static [u8]> {
    pinned::kit().script(name).ok_or_else(|| Error::Invalid(format!("{name} is not a script of the thin family")))
}

/// The address of a script of the family (the same in every library).
pub fn script_spk(name: &str) -> Result<ScriptPublicKey> {
    Ok(pay_to_script_hash_script(script_of(name)?))
}

/// The address of an object in the library `lib`.
pub fn object_spk(lib: &[u8; 32], o: &TObj) -> ScriptPublicKey {
    pay_to_script_hash_script(&o.script(lib))
}

/// What a genesis' payload begins with, before the id of the committee its fee pool is for.
pub const GENESIS_MAGIC: [u8; 4] = *b"KPF\x02";

/// What a fee pool's state names: the committee the oracle fees are for, and what a copy of a script and the fee pool go on holding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeePoolOf {
    pub committee: [u8; 32],
    pub floor: i64,
}

impl FeePoolOf {
    pub fn script(&self) -> Vec<u8> {
        pinned::kit().fee_pool(&crate::thin::fee_pool_hash(&self.committee, self.floor))
    }
}

/// A library's fee pool on the chain.
pub type FeePool = OLive<FeePoolOf>;

/// The address of a fee pool (the same in every library).
pub fn fee_pool_spk(of: &FeePoolOf) -> ScriptPublicKey {
    pay_to_script_hash_script(&of.script())
}

/// A library's genesis without a fee pool: what its operations leave in the copies stays there.
pub fn lib_genesis(copies: usize, script_value: u64, p: Payer) -> Result<(OBuilt, [u8; 32])> {
    lib_genesis_for(copies, script_value, None, p)
}

/// A library's genesis: `copies` of each script under one covenant id, each holding `script_value`, authorized by the
/// funder's first input, and with `committee` the fee pool its oracle fees gather in for that committee (the last output of the id,
/// holding `script_value` too; the payload names the committee and that value, which is what a sweep leaves in a copy).
/// Output 0 is the change.
pub fn lib_genesis_for(copies: usize, script_value: u64, committee: Option<[u8; 32]>, p: Payer) -> Result<(OBuilt, [u8; 32])> {
    if !(1..=COPIES_MAX).contains(&copies) {
        return invalid(format!("a library holds 1 to {COPIES_MAX} copies of each script"));
    }
    if script_value < SCRIPT_VALUE_MIN {
        return invalid("a script of a library holds at least 1 KAS");
    }
    if p.funds.is_empty() {
        return invalid("a library's genesis needs a funding input");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    d.fund(p.funds, &p.change_to);
    for _ in 0..copies {
        for n in SCRIPTS {
            d.genesis_script(script_spk(n)?, script_value, 0, true, n);
        }
    }
    if let Some(r) = committee {
        d.genesis_script(fee_pool_spk(&FeePoolOf { committee: r, floor: script_value as i64 }), script_value, 0, true, FEE_POOL);
        d.payload = [GENESIS_MAGIC.to_vec(), r.to_vec(), sm8(script_value as i64).to_vec()].concat();
    }
    let b = d.finish()?;
    let id = b.tx.outputs[1].covenant.as_ref().map(|c| c.covenant_id.as_bytes()).ok_or_else(|| Error::Invalid("the genesis wrote no script".into()))?;
    Ok((b, id))
}

/// The name of the family's script an output pays to, if it pays to one.
fn script_named(spk: &ScriptPublicKey) -> Option<&'static str> {
    SCRIPTS.into_iter().find(|n| script_spk(n).is_ok_and(|s| s == *spk))
}

/// Reads a transaction as a library's genesis and returns the library's id, or says why it is not one a position
/// can be trusted to (review T1b, F4). An object accepts whatever carries its library's id, so a library is safe
/// only if every output its genesis wrote under the id is one of the family's scripts: one output more, of any
/// script, takes every object of the library. This checks that the outputs under the id are all of one id and one
/// authorizing input, that each pays to a pinned script, that every script of the family is among them, and that
/// the id is the covenant id of exactly these outputs (so the transaction is the id's genesis and nothing else was
/// written under it there). After its genesis a library cannot grow: each script asks for as many outputs under
/// the id as inputs. One output may be the fee pool of the committee the payload names: it is spent only beside a script
/// of the library that names it, and it names nothing itself (an object is moved by an input whose signature script
/// begins with the 24 bytes of the places; a fee pool's begins with a number).
pub fn lib_check(tx: &kaspa_consensus_core::tx::Transaction) -> Result<[u8; 32]> {
    let bound: Vec<(u32, &kaspa_consensus_core::tx::TransactionOutput)> = tx.outputs.iter().enumerate().filter(|(_, o)| o.covenant.is_some()).map(|(i, o)| (i as u32, o)).collect();
    let first = bound.first().and_then(|(_, o)| o.covenant).ok_or_else(|| Error::Invalid("the transaction writes no output under a covenant id: not a library's genesis".into()))?;
    let (id, auth) = (first.covenant_id, first.authorizing_input);
    if bound.iter().any(|(_, o)| o.covenant.map(|c| (c.covenant_id, c.authorizing_input)) != Some((id, auth))) {
        return invalid("the transaction writes outputs under more than one covenant id: not a library's genesis alone");
    }
    let mut seen = std::collections::BTreeSet::new();
    let fee_pool = genesis_fee_pool(tx).map(|of| fee_pool_spk(&of));
    let mut fee_pools = 0;
    for (i, o) in &bound {
        match script_named(&o.script_public_key) {
            Some(n) => {
                seen.insert(n);
            }
            None if fee_pool.as_ref() == Some(&o.script_public_key) && fee_pools == 0 => fee_pools = 1,
            None => return invalid(format!("REFUSED: output {i} under the library's id is not one of the family's scripts. Whoever can spend it can take every object of this library")),
        }
    }
    if let Some(missing) = SCRIPTS.into_iter().find(|n| !seen.contains(n)) {
        return invalid(format!("the library has no copy of {missing}: an object in it could not use that operation"));
    }
    let from = tx.inputs.get(auth as usize).ok_or_else(|| Error::Invalid("the authorizing input of the library's outputs is not in the transaction".into()))?.previous_outpoint;
    let bare: Vec<(u32, kaspa_consensus_core::tx::TransactionOutput)> = bound.iter().map(|(i, o)| (*i, kaspa_consensus_core::tx::TransactionOutput { covenant: None, ..(*o).clone() })).collect();
    if kaspa_consensus_core::hashing::covenant_id::covenant_id(from, bare.iter().map(|(i, o)| (*i, o)).collect::<Vec<_>>().into_iter()) != id {
        return invalid("REFUSED: the id is not the covenant id of these outputs: this transaction is not the id's genesis, and what the genesis wrote is not shown");
    }
    Ok(id.as_bytes())
}

/// What a genesis' payload says of its fee pool, if it names one: the committee, and what a copy keeps.
pub fn genesis_fee_pool(tx: &kaspa_consensus_core::tx::Transaction) -> Option<FeePoolOf> {
    let r = tx.payload.strip_prefix(&GENESIS_MAGIC).filter(|r| r.len() == 40)?;
    let floor = i64::from_le_bytes(r[32..].try_into().ok()?);
    (floor >= SCRIPT_VALUE_MIN as i64).then_some(FeePoolOf { committee: r[..32].try_into().ok()?, floor })
}

/// The fee pool a genesis transaction wrote, as a live UTXO, if it wrote one.
pub fn lib_fee_pool(b: &kaspa_consensus_core::tx::Transaction, daa: u64) -> Option<FeePool> {
    let r = genesis_fee_pool(b)?;
    let spk = fee_pool_spk(&r);
    b.outputs.iter().enumerate().find(|(_, o)| o.covenant.is_some() && o.script_public_key == spk).map(|(i, o)| OLive { s: r, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: kaspa_consensus_core::tx::TransactionOutpoint { transaction_id: b.id(), index: i as u32 }, daa })
}

/// The copies of the scripts a genesis transaction wrote, as live UTXOs: each output under a covenant id that pays to
/// one of the family's scripts, named by its script (not by its place). `lib_check` says whether the whole is a library.
pub fn lib_scripts(b: &kaspa_consensus_core::tx::Transaction, daa: u64) -> Vec<LibScript> {
    let mut v = vec![];
    for (i, o) in b.outputs.iter().enumerate() {
        let (Some(c), Some(name)) = (&o.covenant, script_named(&o.script_public_key)) else { continue };
        v.push(OLive { s: name, value: o.value, id: Some(c.covenant_id), op: kaspa_consensus_core::tx::TransactionOutpoint { transaction_id: b.id(), index: i as u32 }, daa });
    }
    v
}

/// What an opening publishes, so that anybody can test a claim on the position: the two parts of its state.
pub fn opening_payload(a: &Position) -> Vec<u8> {
    [&OPENING_MAGIC[..], &a.fixed(), &a.moving()].concat()
}

/// The position an opening's payload publishes, if the payload is one.
pub fn read_opening(payload: &[u8]) -> Option<Position> {
    let rest = payload.strip_prefix(&OPENING_MAGIC[..])?;
    if rest.len() != crate::thin::FIXED_LEN + crate::thin::MOVING_LEN {
        return None;
    }
    Position::from_parts(&rest[..crate::thin::FIXED_LEN], &rest[crate::thin::FIXED_LEN..]).ok()
}

/// An anchor of a library's registry, live.
pub type Anchor = OLive<()>;
/// The least an anchor holds (every placement writes it again).
pub const ANCHOR_VALUE_MIN: u64 = 100_000_000;

/// A registry's genesis: `copies` anchors of the library `lib` under one covenant id of their own, the registry's. Every order
/// and position that enters through one of them (`KpfTPlace`) is under that id, and so is whatever a script writes from
/// them. Returns the transaction and the registry's id.
pub fn registry_genesis(lib: &[u8; 32], copies: usize, value: u64, p: Payer) -> Result<(OBuilt, [u8; 32])> {
    if !(1..=64).contains(&copies) {
        return invalid("a registry begins with 1 to 64 anchors");
    }
    if value < ANCHOR_VALUE_MIN {
        return invalid("an anchor holds at least 1 KAS");
    }
    if p.funds.is_empty() {
        return invalid("a registry's genesis needs a funding input");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    d.fund(p.funds, &p.change_to);
    for _ in 0..copies {
        d.genesis_script(object_spk(lib, &TObj::Anchor), value, 0, true, "anchor");
    }
    let b = d.finish()?;
    let id = b.tx.outputs[1].covenant.as_ref().map(|c| c.covenant_id.as_bytes()).ok_or_else(|| Error::Invalid("the genesis wrote no anchor".into()))?;
    Ok((b, id))
}

/// Whether `tx` is the genesis of a registry of the library `lib`: every output under a covenant id is an anchor of that
/// library, all under one id, and that id is the covenant id of those outputs. An object of any other state written
/// there would be an order or a position nobody signed, among those the scripts take for signed. Returns the id.
pub fn registry_check(tx: &kaspa_consensus_core::tx::Transaction, lib: &[u8; 32]) -> Result<[u8; 32]> {
    let bound: Vec<(u32, &kaspa_consensus_core::tx::TransactionOutput)> = tx.outputs.iter().enumerate().filter(|(_, o)| o.covenant.is_some()).map(|(i, o)| (i as u32, o)).collect();
    let first = bound.first().and_then(|(_, o)| o.covenant).ok_or_else(|| Error::Invalid("the transaction writes no output under a covenant id: not a registry's genesis".into()))?;
    let (id, auth) = (first.covenant_id, first.authorizing_input);
    if bound.iter().any(|(_, o)| o.covenant.map(|c| (c.covenant_id, c.authorizing_input)) != Some((id, auth))) {
        return invalid("the transaction writes outputs under more than one covenant id: not a registry's genesis alone");
    }
    let anchor = object_spk(lib, &TObj::Anchor);
    if let Some((i, _)) = bound.iter().find(|(_, o)| o.script_public_key != anchor) {
        return invalid(format!("REFUSED: output {i} under the registry's id is not an anchor of this library. An order or a position written there was signed by nobody, and the scripts would take it for one that was"));
    }
    let from = tx.inputs.get(auth as usize).ok_or_else(|| Error::Invalid("the authorizing input of the registry's outputs is not in the transaction".into()))?.previous_outpoint;
    let bare: Vec<(u32, kaspa_consensus_core::tx::TransactionOutput)> = bound.iter().map(|(i, o)| (*i, kaspa_consensus_core::tx::TransactionOutput { covenant: None, ..(*o).clone() })).collect();
    if kaspa_consensus_core::hashing::covenant_id::covenant_id(from, bare.iter().map(|(i, o)| (*i, o)).collect::<Vec<_>>().into_iter()) != id {
        return invalid("REFUSED: the id is not the covenant id of these outputs: this transaction is not the id's genesis, and what the genesis wrote is not shown");
    }
    Ok(id.as_bytes())
}

/// The anchors a registry's genesis wrote, as live UTXOs.
pub fn registry_anchors(tx: &kaspa_consensus_core::tx::Transaction, lib: &[u8; 32], daa: u64) -> Vec<Anchor> {
    let anchor = object_spk(lib, &TObj::Anchor);
    tx.outputs
        .iter()
        .enumerate()
        .filter(|(_, o)| o.covenant.is_some() && o.script_public_key == anchor)
        .map(|(i, o)| OLive { s: (), value: o.value, id: o.covenant.map(|c| c.covenant_id), op: kaspa_consensus_core::tx::TransactionOutpoint { transaction_id: tx.id(), index: i as u32 }, daa })
        .collect()
}

/// What enters a registry: input 0 the anchor, input 1 a copy of `name` (`KpfTPlace` for an order, `KpfTOpen` for a position); output 0 what enters, output 1 the script,
/// output 2 the anchor again. Returns the registry's id.
fn enters(d: &mut ODraft, lib: &[u8; 32], anchor: &Anchor, script: &LibScript, name: &'static str, entry: &'static str, args: Vec<Arg>, o: TObj, value: u64) -> Result<Hash> {
    let (id, lid) = enter(d, lib, anchor, TObj::Anchor, script, name, entry, args, 0)?;
    d.out(value, object_spk(lib, &o), Some((0, id)), o.kind(), None);
    keep(d, script, lid)?;
    d.out(anchor.value, object_spk(lib, &TObj::Anchor), Some((0, id)), "anchor", None);
    Ok(id)
}

/// A position's opening in the library `lib`, through an anchor of its registry (`KpfTOpen`): both parties sign. B's
/// inputs, if any are given, pay B's margin (the rest back to `b_change`), the payer's pay A's margin, the carrier and
/// the fee (and B's margin if B gave no input). Output 0 is the position, under the registry's id. With `publish` the
/// transaction's payload carries the position's state.
pub fn open(lib: &[u8; 32], anchor: &Anchor, script: &LibScript, a: &Position, b_funds: &[KeyUtxo], b_change: &[u8; 32], publish: bool, p: Payer) -> Result<OBuilt> {
    a.check()?;
    // Without inputs of the counterparty the payer pays its margin too.
    let b_in: u64 = b_funds.iter().map(|u| u.amount).sum();
    let back = if b_funds.is_empty() { 0 } else { (b_in as i64).checked_sub(a.x.b_margin).filter(|v| *v >= 0).ok_or_else(|| Error::Invalid("the counterparty's inputs do not cover its margin".into()))? };
    if back > 0 && back < crate::tx::DUST_OUTPUT_MIN as i64 {
        return invalid("the counterparty's inputs leave a change below the dust limit: give inputs that are exact or leave more");
    }
    if p.funds.is_empty() && b_funds.is_empty() {
        return invalid("an opening needs a funding input");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, a);
    args.extend([Arg::Sig(a.x.a_key), Arg::Sig(a.x.b_key)]);
    enters(&mut d, lib, anchor, script, "KpfTOpen", "open", args, TObj::Position(*a), a.x.floor() as u64)?;
    for u in b_funds {
        d.key(u, "the counterparty's margin");
    }
    if back > 0 {
        d.plain(back, b_change, "the counterparty's change");
    }
    d.fund(p.funds, &p.change_to);
    if publish {
        d.payload = opening_payload(a);
    }
    d.finish()
}

/// The first five arguments of every entry: the inputs it serves (the object, thrice), the age it asks of it, the
/// object's length, and the two parts of the position (the fixed one as its hash).
fn head(age: u64, a: &Position) -> Vec<Arg> {
    vec![bv(vec![0; 24]), bv(sm8(age as i64).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.fixed_hash().to_vec()), bv(a.moving())]
}

/// Inputs 0 and 1 of an operation: the object and a copy of the script, with the entry's arguments after the head.
/// Returns the object's covenant id and the library's.
fn enter<T>(d: &mut ODraft, lib: &[u8; 32], obj: &OLive<T>, o: TObj, script: &LibScript, name: &'static str, entry: &'static str, args: Vec<Arg>, age: u64) -> Result<(Hash, Hash)> {
    let id = obj.id.ok_or_else(|| Error::Invalid(format!("the {} carries no covenant id", o.kind())))?;
    if script.s != name || script.id.map(|h| h.as_bytes()) != Some(*lib) {
        return invalid(format!("the script given is not a copy of {name} in the object's library"));
    }
    let lid = Hash::from_bytes(*lib);
    d.contract(obj, OWhich::T(SLOT), o.script(lib), "go", vec![iv(1), iv(0)], o.kind(), age);
    let code = script_of(name)?.to_vec();
    d.input(script.op, kaspa_consensus_core::tx::UtxoEntry::new(script.value, pay_to_script_hash_script(&code), script.daa, false, script.id), OPlan::Contract { which: OWhich::T(name), redeem: code, entry, args }, name, 0);
    Ok((id, lid))
}

/// Output 1: the script goes on as it is.
fn keep(d: &mut ODraft, script: &LibScript, lid: Hash) -> Result<()> {
    keep_with(d, script, lid, 0)
}

/// What a use of the script `name` leaves in its copy: the oracle fees, but for the two scripts that collect them.
pub fn oracle_fee_of(name: &str) -> i64 {
    if COLLECTORS.contains(&name) { 0 } else { ORACLE_FEE }
}

/// Output 1: the script goes on, its copy holding the oracle fees and `more` than it did (what an entry leaves there).
fn keep_with(d: &mut ODraft, script: &LibScript, lid: Hash, more: i64) -> Result<()> {
    let i = d.out(script.value + (oracle_fee_of(script.s) + more) as u64, script_spk(script.s)?, Some((1, lid)), script.s, None);
    debug_assert_eq!(i, 1);
    Ok(())
}

/// `KpfTAdd`: the owner of `side` adds `delta` to its margin.
pub fn add(lib: &[u8; 32], position: &OLive<Position>, script: &LibScript, side: i64, delta: i64, p: Payer) -> Result<(OBuilt, Position)> {
    let a = &position.s;
    let next = a.added(side, delta).ok_or_else(|| Error::Invalid("the margin added must be positive, of side a or b, and within range".into()))?;
    let key = a.key_of(side).expect("a side");
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, a);
    args.extend([iv(side), iv(delta), Arg::Sig(key)]);
    let (id, lid) = enter(&mut d, lib, position, TObj::Position(*a), script, "KpfTAdd", "add", args, 0)?;
    d.out(position.value + delta as u64, object_spk(lib, &TObj::Position(next)), Some((0, id)), "position", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, next))
}

/// `KpfTClose`: `side` states what it is owed at the prices `a` of the time `t0` and posts the bond. With `amount`
/// absent the statement is what the rule gives (a close the rule does not allow there is refused); given, it is stated
/// as given, which a dispute tests.
pub fn close(lib: &[u8; 32], position: &OLive<Position>, script: &LibScript, side: i64, t0: i64, prices: [i64; 2], amount: Option<i64>, p: Payer) -> Result<(OBuilt, Claim)> {
    let a = &position.s;
    let claim = match amount {
        None => a.true_claim(side, t0, prices).ok_or_else(|| Error::Invalid("the rule of a close gives this side nothing to declare at these prices (a liquidation is due, or the side or the signed price is not valid)".into()))?,
        Some(v) => a.claim(side, t0, prices, v).ok_or_else(|| Error::Invalid("a claim states an amount of 0 to the pot, for side a or b, at a valid signed price".into()))?,
    };
    let key = a.key_of(side).expect("a side");
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, a);
    args.extend([iv(side), iv(t0), iv(prices[0]), iv(prices[1]), iv(claim.amount), Arg::Sig(key)]);
    let (id, lid) = enter(&mut d, lib, position, TObj::Position(*a), script, "KpfTClose", "close", args, 0)?;
    d.out(position.value + claim.bond as u64, object_spk(lib, &TObj::Claim(claim)), Some((0, id)), "claim", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, claim))
}

fn stated(c: &Claim) -> Vec<Arg> {
    vec![iv(c.side), iv(c.t0), iv(c.a[0]), iv(c.a[1]), iv(c.amount), iv(c.bond)]
}

/// The payouts of a transaction that ends an object: (to A at output 0, to B at the last output). Where A has nothing
/// the change takes output 0.
fn pay_out(d: &mut ODraft, script: &LibScript, lid: Hash, a: &Position, pa: i64, pb: i64, p: Payer) -> Result<()> {
    if pa > 0 {
        d.payout(pa, &a.x.a_key, "party a");
        keep(d, script, lid)?;
        d.fund(p.funds, &p.change_to);
    } else {
        d.fund(p.funds, &p.change_to);
        keep(d, script, lid)?;
    }
    if pb > 0 {
        d.last(pb, &a.x.b_key, "party b", &p.change_to);
    }
    Ok(())
}

/// `KpfTSettle`: anyone settles an undisputed claim that landed in time, once it is a dispute period old.
pub fn settle(lib: &[u8; 32], claim: &OLive<Claim>, script: &LibScript, p: Payer) -> Result<OBuilt> {
    let c = &claim.s;
    if !c.in_time(claim.daa) {
        return invalid("the claim landed later than lateDaa after its time: it is ended as late, not settled");
    }
    let (pa, pb) = c.settle_pay(claim.value as i64);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(c.position.dispute_daa as u64, &c.position);
    args.extend(stated(c));
    let (_, lid) = enter(&mut d, lib, claim, TObj::Claim(*c), script, "KpfTSettle", "settle", args, c.position.dispute_daa as u64)?;
    pay_out(&mut d, script, lid, &c.position, pa, pb, p)?;
    d.finish()
}

/// The two payouts of a claim that is ended as late: half of its bond to the party it was made against (the output
/// before the last), the rest to who ended it (the last).
fn bond_shared(d: &mut ODraft, other: &[u8; 32], ender: &[u8; 32], to_ender: i64, to_other: i64, p: Payer) {
    if to_other > 0 {
        d.last(to_other, other, "half of the bond, to the party the claim was made against", &p.change_to);
    } else {
        d.last(100_000_000, &p.change_to, "filler", &p.change_to);
    }
    d.payout(to_ender, ender, "half of the bond, to who ended the claim");
}

/// `KpfTLate`: `ender` ends a claim that landed late or before its own time: the position again, the claimer's bond
/// shared by `ender` and the other party.
pub fn late(lib: &[u8; 32], claim: &OLive<Claim>, script: &LibScript, ender: [u8; 32], p: Payer) -> Result<(OBuilt, Position)> {
    let c = &claim.s;
    if c.in_time(claim.daa) {
        return invalid("the claim landed in time: it is settled or disputed, not ended as late");
    }
    let (rest, to_ender, to_other) = c.late_pay(claim.value as i64).ok_or_else(|| Error::Invalid("the claim holds less than its bond".into()))?;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, &c.position);
    args.extend(stated(c));
    args.push(bv(ender.to_vec()));
    let (id, lid) = enter(&mut d, lib, claim, TObj::Claim(*c), script, "KpfTLate", "late", args, 0)?;
    d.out(rest as u64, object_spk(lib, &TObj::Position(c.position)), Some((0, id)), "position", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    bond_shared(&mut d, &c.other_key(), &ender, to_ender, to_other, p);
    Ok((d.finish()?, c.position))
}

/// `KpfTDispute.dispute`: `disputer` asks for the proof of a claim that landed in time and posts a bond as large as
/// the claimer's.
pub fn dispute(lib: &[u8; 32], claim: &OLive<Claim>, script: &LibScript, disputer: [u8; 32], p: Payer) -> Result<(OBuilt, Disputed)> {
    let c = &claim.s;
    if !c.in_time(claim.daa) {
        return invalid("the claim landed later than lateDaa after its time: it is ended as late, not disputed");
    }
    if c.bond <= 0 {
        return invalid("a claim without a bond cannot be disputed");
    }
    let next = c.disputed(disputer);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, &c.position);
    args.extend(stated(c));
    args.push(bv(disputer.to_vec()));
    let (id, lid) = enter(&mut d, lib, claim, TObj::Claim(*c), script, "KpfTDispute", "dispute", args, 0)?;
    d.out(claim.value + c.bond as u64, object_spk(lib, &TObj::Disputed(next)), Some((0, id)), "disputed claim", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, next))
}

/// `KpfTDispute.lapse`: anyone ends a disputed claim nobody proved within the dispute period: the position again, half of the
/// claimer's bond to the party the claim was made against, the rest and its own bond to the disputer (the last output).
pub fn lapse(lib: &[u8; 32], dis: &OLive<Disputed>, script: &LibScript, p: Payer) -> Result<(OBuilt, Position)> {
    let c = &dis.s.claim;
    let (rest, to_disputer, to_claimer) = dis.s.lapse_pay(dis.value as i64).ok_or_else(|| Error::Invalid("the disputed claim holds less than its two bonds".into()))?;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(c.position.dispute_daa as u64, &c.position);
    args.extend(stated(c));
    args.push(bv(dis.s.disputer.to_vec()));
    let (id, lid) = enter(&mut d, lib, dis, TObj::Disputed(dis.s), script, "KpfTDispute", "lapse", args, c.position.dispute_daa as u64)?;
    d.out(rest as u64, object_spk(lib, &TObj::Position(c.position)), Some((0, id)), "position", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    // The script reads the two payouts from the end: the claimer's bond, then the disputer's.
    d.last(to_claimer, &dis.s.claimer_key(), "the claimer's bond", &p.change_to);
    d.payout(to_disputer, &dis.s.disputer, "the disputer's bond");
    Ok((d.finish()?, c.position))
}

/// `KpfTBoth`: the two parties end the position as they agreed, `to_a` to A and `to_b` to B (the script asks for the
/// two signatures and nothing else, so the same entry carries any other agreement; this builder writes a payout).
pub fn both_close(lib: &[u8; 32], position: &OLive<Position>, script: &LibScript, to_a: i64, to_b: i64, p: Payer) -> Result<OBuilt> {
    let a = &position.s;
    if to_a < 0 || to_b < 0 || to_a.checked_add(to_b).map(|s| s > position.value as i64).unwrap_or(true) {
        return invalid("the two payouts are not negative and together at most what the position holds");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, a);
    args.extend([Arg::Sig(a.x.a_key), Arg::Sig(a.x.b_key)]);
    let (_, lid) = enter(&mut d, lib, position, TObj::Position(*a), script, "KpfTBoth", "both", args, 0)?;
    pay_out(&mut d, script, lid, a, to_a, to_b, p)?;
    d.finish()
}

// ---------------------------------------------------------------- the proof

/// An epoch table of a leg's committee and the attestation of the claim's signed price it is spent with.
pub type Attested<'a> = (&'a OLive<crate::committee::EpochTable>, &'a crate::committee::Attestation);

/// The epoch tables of a proof as inputs 2 (and 3), checked against the position's legs, the signed price and the prices stated;
/// returns the places the script is told and the epoch tables' covenant ids.
fn epoch_tables_in(d: &mut ODraft, a: &Position, t0: i64, prices: [i64; 2], t1: Attested, t2: Option<Attested>) -> Result<(Vec<u8>, Vec<Hash>)> {
    ticks_in(d, a, [Some((t0, prices[0])), Some((t0, prices[1]))], t1, t2)
}

/// As `epoch_tables_in`, each leg's attestation held to its own (signed price, price) where one is given.
fn ticks_in(d: &mut ODraft, a: &Position, want: [Option<(i64, i64)>; 2], t1: Attested, t2: Option<Attested>) -> Result<(Vec<u8>, Vec<Hash>)> {
    let two = a.x.legs[1].size > 0;
    if two != t2.is_some() {
        return invalid("a proof spends one epoch table for each leg the position has");
    }
    let mut rids = vec![];
    for (i, (epoch_table, att)) in [Some(t1), t2].into_iter().flatten().enumerate() {
        let l = &a.x.legs[i];
        if epoch_table.id.map(|h| h.as_bytes()) != Some(l.committee) || att.market != l.market {
            return invalid(format!("epoch table {} or its attestation is not of the committee and the market of leg {}", i + 1, i + 1));
        }
        d.lock(att.t0 as u64);
        if want[i].is_some_and(|(t0, p)| att.t0 != t0 || (p >= 0 && att.p != p)) {
            return invalid(format!("attestation {} is not of the time (and, where one is asked, the price) the claim states (the committee signed {} at DAA {})", i + 1, att.p, att.t0));
        }
        if epoch_table.s.spread_bps * 4 > a.x.t.g_bps {
            return invalid("the committee's spread leaves the position's tolerance g no margin");
        }
        rids.push(crate::committee_build::attest_input(d, epoch_table, att, att.t0 as u64)?);
    }
    let who: Vec<u8> = [0i64, 2, if two { 3 } else { 2 }].iter().flat_map(|w| sm8(*w)).collect();
    Ok((who, rids))
}

/// The epoch tables go on at their own indices (outputs 2 and 3).
fn epoch_tables_out(d: &mut ODraft, t1: Attested, t2: Option<Attested>, rids: &[Hash]) {
    for (i, (epoch_table, _)) in [Some(t1), t2].into_iter().flatten().enumerate() {
        d.obj(crate::oracle_build::OObj::C(crate::committee_build::CObj::EpochTable(epoch_table.s)), epoch_table.value, Some((2 + i as u16, rids[i])), "epoch table");
    }
}

/// `KpfTProve`: the claimer answers a dispute with the committee's signatures. Inputs 2 (and 3) are an epoch table of each
/// leg's committee spent by its attest for the claim's signed price and prices; the script computes the rule and, the amount
/// being the rule's, pays the claimer its amount and both bonds at once. A claim that is not the rule's is refused
/// here as it would be by the script.
pub fn prove(lib: &[u8; 32], dis: &OLive<Disputed>, script: &LibScript, t1: Attested, t2: Option<Attested>, p: Payer) -> Result<OBuilt> {
    prove_with(lib, dis, script, t1, t2, p, true)
}

/// `prove` without the tool's own check of the claim: what the script itself answers to a claim that is not the
/// rule's (the tests' use; the transaction it builds is refused by the engine).
#[doc(hidden)]
pub fn prove_with(lib: &[u8; 32], dis: &OLive<Disputed>, script: &LibScript, t1: Attested, t2: Option<Attested>, p: Payer, checked: bool) -> Result<OBuilt> {
    let c = &dis.s.claim;
    let a = &c.position;
    // The committee's prices need not be the claim's: a claim states its claimer's own, and is proved when its amount
    // is at most what the committee's give.
    let signed = [t1.1.p, t2.map_or(0, |t| t.1.p)];
    if checked && !c.within(signed) {
        return invalid(format!("the claim states {} sompi at prices of its own {:?}, and the committee signs {signed:?}, which give its side {}: nobody proves it (it is refuted: `refute`)", c.amount, c.a, a.x.close_pay(c.side, signed).map_or("no close".to_string(), |_| format!("{} sompi", a.x.owed_at(c.side, signed)))));
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    // The object and the script are inputs 0 and 1; the script's arguments name the epoch tables, which come next.
    let mut probe = ODraft::new(p.rate, FeeMode::Relay);
    let (who, _) = epoch_tables_in(&mut probe, a, c.t0, [-1, -1], t1, t2)?;
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.fixed()), bv(a.moving())];
    args.extend(stated(c));
    args.extend([bv(dis.s.disputer.to_vec()), bv(dis.s.disputer.to_vec()), iv(1)]);
    let (_, lid) = enter(&mut d, lib, dis, TObj::Disputed(dis.s), script, "KpfTProve", "answer", args, 0)?;
    let (_, rids) = epoch_tables_in(&mut d, a, c.t0, [-1, -1], t1, t2)?;
    let (pa, pb, kept) = dis.s.prove_pay(dis.value as i64);
    let funded = pa <= 0;
    if pa > 0 {
        d.payout(pa, &a.x.a_key, "party a");
    } else {
        d.fund(p.funds, &p.change_to);
    }
    keep_with(&mut d, script, lid, kept)?;
    epoch_tables_out(&mut d, t1, t2, &rids);
    if !funded {
        d.fund(p.funds, &p.change_to);
    }
    if pb > 0 {
        d.last(pb, &a.x.b_key, "party b", &p.change_to);
    }
    d.finish()
}

/// `KpfTProve.answer` with the verdict 0: anyone answers a dispute against the claim, with the committee's signatures for the claim's
/// time: the claim states more than the committee's prices give its side (or closes where no close is allowed). The position
/// again (output 0); the disputer has its bond back, `refuter` half of the claimer's (the last two outputs), and the
/// other half stays in the script's copy.
pub fn refute(lib: &[u8; 32], dis: &OLive<Disputed>, script: &LibScript, t1: Attested, t2: Option<Attested>, refuter: [u8; 32], p: Payer) -> Result<(OBuilt, Position)> {
    refute_with(lib, dis, script, t1, t2, refuter, p, true)
}

/// `refute` without the tool's own check (the tests' use).
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn refute_with(lib: &[u8; 32], dis: &OLive<Disputed>, script: &LibScript, t1: Attested, t2: Option<Attested>, refuter: [u8; 32], p: Payer, checked: bool) -> Result<(OBuilt, Position)> {
    let c = &dis.s.claim;
    let a = &c.position;
    let signed = [t1.1.p, t2.map_or(0, |t| t.1.p)];
    if checked && c.within(signed) {
        return invalid(format!("the claim states {} sompi and the committee signs {signed:?}, which give its side as much or more: it is not refuted (it is proved: `prove`)", c.amount));
    }
    let (rest, to_disputer, to_refuter, kept) = dis.s.refute_pay(dis.value as i64).ok_or_else(|| Error::Invalid("the disputed claim holds less than its two bonds".into()))?;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut probe = ODraft::new(p.rate, FeeMode::Relay);
    let (who, _) = epoch_tables_in(&mut probe, a, c.t0, [-1, -1], t1, t2)?;
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.fixed()), bv(a.moving())];
    args.extend(stated(c));
    args.extend([bv(dis.s.disputer.to_vec()), bv(refuter.to_vec()), iv(0)]);
    let (id, lid) = enter(&mut d, lib, dis, TObj::Disputed(dis.s), script, "KpfTProve", "answer", args, 0)?;
    let (_, rids) = epoch_tables_in(&mut d, a, c.t0, [-1, -1], t1, t2)?;
    d.out(rest as u64, object_spk(lib, &TObj::Position(*a)), Some((0, id)), "position", None);
    keep_with(&mut d, script, lid, kept)?;
    epoch_tables_out(&mut d, t1, t2, &rids);
    d.fund(p.funds, &p.change_to);
    d.last(to_disputer, &dis.s.disputer, "the disputer's bond", &p.change_to);
    if to_refuter > 0 {
        d.payout(to_refuter, &refuter, "half of the claimer's bond, to who refuted");
    } else {
        d.plain(100_000_000, &p.change_to, "filler");
    }
    Ok((d.finish()?, *a))
}

/// `KpfTLiqProve`: the liquidator answers a dispute with the committee's signatures (inputs 2 and 3 as `prove`). The
/// script computes the rule of a liquidation; the fee and the cure stated being the rule's, the claim becomes a
/// proved one (output 0), which waits a dispute period again, and the liquidator has the disputer's bond (the last output).
pub fn liq_prove(lib: &[u8; 32], l: &OLive<LiqObj>, script: &LibScript, t1: Attested, t2: Option<Attested>, p: Payer) -> Result<(OBuilt, LiqObj)> {
    liq_prove_with(lib, l, script, t1, t2, p, true)
}

/// `liq_prove` without the tool's own check of the claim (as `prove_with`).
#[doc(hidden)]
pub fn liq_prove_with(lib: &[u8; 32], l: &OLive<LiqObj>, script: &LibScript, t1: Attested, t2: Option<Attested>, p: Payer, checked: bool) -> Result<(OBuilt, LiqObj)> {
    let q = &l.s.liq;
    let a = &q.position;
    let LiqState::Disputed(dkey) = l.s.state else { return invalid("only a disputed liquidation is proved") };
    let signed = [t1.1.p, t2.map_or(0, |t| t.1.p)];
    if checked && !q.within(signed) {
        return invalid(format!("the liquidation is not within the rule at the prices {signed:?} the committee signs: nobody proves it (it is refuted: `liq_refute`)"));
    }
    let rest = (l.value as i64).checked_sub(q.bond).filter(|v| *v >= 0).ok_or_else(|| Error::Invalid("the disputed liquidation holds less than a bond".into()))?;
    let mut probe = ODraft::new(p.rate, FeeMode::Relay);
    let (who, _) = epoch_tables_in(&mut probe, a, q.t0, [-1, -1], t1, t2)?;
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.fixed()), bv(a.moving())];
    args.extend(liq_stated(q));
    args.extend([iv(q.bond), bv(q.liquidator.to_vec()), bv(dkey.to_vec()), bv(dkey.to_vec()), iv(1)]);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let (id, lid) = enter(&mut d, lib, l, TObj::Liq(l.s), script, "KpfTLiqProve", "answer", args, 0)?;
    let (_, rids) = epoch_tables_in(&mut d, a, q.t0, [-1, -1], t1, t2)?;
    let (to_liquidator, kept) = l.s.prove_pay();
    let next = LiqObj { liq: *q, state: LiqState::Proved };
    d.out(rest as u64, object_spk(lib, &TObj::Liq(next)), Some((0, id)), "proved liquidation", None);
    keep_with(&mut d, script, lid, kept)?;
    epoch_tables_out(&mut d, t1, t2, &rids);
    d.fund(p.funds, &p.change_to);
    if to_liquidator > 0 {
        d.last(to_liquidator, &q.liquidator, "half of the disputer's bond, to the liquidator", &p.change_to);
    }
    Ok((d.finish()?, next))
}

/// `KpfTLiqProve.answer` with the verdict 0: anyone answers a dispute against the liquidation, with the committee's signatures for its
/// time: it is not within the rule at the committee's prices. The position again (output 0); the disputer has its
/// bond back, `refuter` half of the liquidator's (the last two outputs), and the other half stays in the script's copy.
pub fn liq_refute(lib: &[u8; 32], l: &OLive<LiqObj>, script: &LibScript, t1: Attested, t2: Option<Attested>, refuter: [u8; 32], p: Payer) -> Result<(OBuilt, Position)> {
    liq_refute_with(lib, l, script, t1, t2, refuter, p, true)
}

/// `liq_refute` without the tool's own check (the tests' use).
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn liq_refute_with(lib: &[u8; 32], l: &OLive<LiqObj>, script: &LibScript, t1: Attested, t2: Option<Attested>, refuter: [u8; 32], p: Payer, checked: bool) -> Result<(OBuilt, Position)> {
    let q = &l.s.liq;
    let a = &q.position;
    let LiqState::Disputed(dkey) = l.s.state else { return invalid("only a disputed liquidation is refuted") };
    let signed = [t1.1.p, t2.map_or(0, |t| t.1.p)];
    if checked && q.within(signed) {
        return invalid(format!("the liquidation is within the rule at the prices {signed:?} the committee signs: it is not refuted (it is proved: `liq_prove`)"));
    }
    let (rest, to_disputer, to_refuter, kept) = l.s.refute_pay(l.value as i64).ok_or_else(|| Error::Invalid("the disputed liquidation holds less than its two bonds".into()))?;
    let mut probe = ODraft::new(p.rate, FeeMode::Relay);
    let (who, _) = epoch_tables_in(&mut probe, a, q.t0, [-1, -1], t1, t2)?;
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.fixed()), bv(a.moving())];
    args.extend(liq_stated(q));
    args.extend([iv(q.bond), bv(q.liquidator.to_vec()), bv(dkey.to_vec()), bv(refuter.to_vec()), iv(0)]);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let (id, lid) = enter(&mut d, lib, l, TObj::Liq(l.s), script, "KpfTLiqProve", "answer", args, 0)?;
    let (_, rids) = epoch_tables_in(&mut d, a, q.t0, [-1, -1], t1, t2)?;
    d.out(rest as u64, object_spk(lib, &TObj::Position(*a)), Some((0, id)), "position", None);
    keep_with(&mut d, script, lid, kept)?;
    epoch_tables_out(&mut d, t1, t2, &rids);
    d.fund(p.funds, &p.change_to);
    d.last(to_disputer, &dkey, "the disputer's bond", &p.change_to);
    if to_refuter > 0 {
        d.payout(to_refuter, &refuter, "half of the liquidator's bond, to who refuted");
    } else {
        d.plain(100_000_000, &p.change_to, "filler");
    }
    Ok((d.finish()?, *a))
}

// ---------------------------------------------------------------- the liquidation

fn liq_stated(l: &Liq) -> Vec<Arg> {
    vec![iv(l.side), iv(l.t0), iv(l.a[0]), iv(l.a[1]), iv(l.fee), iv(l.add)]
}

fn liq_live_obj(l: &OLive<LiqObj>) -> TObj {
    TObj::Liq(l.s)
}

/// `KpfTLiq.liquidate`: `liquidator` states that `side` is below its maintenance at the prices `a` of the time `t0`
/// and posts the bond. With `stated` absent the fee and the cure are the rule's (a position the rule does not
/// liquidate there is refused); given, they are stated as given, which a dispute tests.
#[allow(clippy::too_many_arguments)]
pub fn liquidate(lib: &[u8; 32], position: &OLive<Position>, script: &LibScript, side: i64, t0: i64, prices: [i64; 2], stated: Option<(i64, i64)>, liquidator: [u8; 32], p: Payer) -> Result<(OBuilt, LiqObj)> {
    let a = &position.s;
    let liq = match stated {
        None => a.liquidation(side, t0, prices, liquidator).ok_or_else(|| Error::Invalid("the rule does not liquidate this side at these prices (it has its maintenance there, or the other party lacks its own)".into()))?,
        Some((fee, add)) => a.liq_stated(side, t0, prices, fee, add, liquidator).ok_or_else(|| Error::Invalid("a liquidation states a fee of 0 to the pot and a cure above 0, for side a or b, at a valid signed price".into()))?,
    };
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, a);
    args.extend(liq_stated(&liq));
    args.push(bv(liquidator.to_vec()));
    let (id, lid) = enter(&mut d, lib, position, TObj::Position(*a), script, "KpfTLiq", "liquidate", args, 0)?;
    let next = LiqObj { liq, state: LiqState::Claimed };
    d.out(position.value + liq.bond as u64, object_spk(lib, &TObj::Liq(next)), Some((0, id)), "liquidation", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, next))
}

/// `KpfTLiq.late`: `ender` ends a liquidation that landed late or before its own time: the position again, half of the
/// liquidator's bond to `ender` (the last output), the other half left in the script's copy.
pub fn liq_late(lib: &[u8; 32], l: &OLive<LiqObj>, script: &LibScript, ender: [u8; 32], p: Payer) -> Result<(OBuilt, Position)> {
    let q = &l.s.liq;
    if l.s.state != LiqState::Claimed || q.in_time(l.daa) {
        return invalid("only a liquidation nobody disputed that landed later than lateDaa after its time, or before it, is ended as late");
    }
    let (rest, to_ender, stays) = q.late_pay(l.value as i64).ok_or_else(|| Error::Invalid("the liquidation holds less than its bond".into()))?;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, &q.position);
    args.extend(liq_stated(q));
    args.extend([iv(q.bond), bv(q.liquidator.to_vec()), bv(ender.to_vec())]);
    let (id, lid) = enter(&mut d, lib, l, liq_live_obj(l), script, "KpfTLiq", "late", args, 0)?;
    d.out(rest as u64, object_spk(lib, &TObj::Position(q.position)), Some((0, id)), "position", None);
    keep_with(&mut d, script, lid, stays)?;
    d.fund(p.funds, &p.change_to);
    if to_ender > 0 {
        d.last(to_ender, &ender, "half of the bond, to who ended the claim", &p.change_to);
    }
    Ok((d.finish()?, q.position))
}

/// `KpfTLiqSettle`: anyone settles a liquidation nobody cured, once it is a dispute period old: one nobody disputed that
/// landed in time, or a proved one. The liquidator has its fee and its bond (output 0), the other party the rest.
pub fn liq_settle(lib: &[u8; 32], l: &OLive<LiqObj>, script: &LibScript, p: Payer) -> Result<OBuilt> {
    let q = &l.s.liq;
    let proved = match l.s.state {
        LiqState::Claimed if q.in_time(l.daa) => 0,
        LiqState::Claimed => return invalid("the liquidation landed later than lateDaa after its time: it is ended as late, not settled"),
        LiqState::Proved => 1,
        LiqState::Disputed(_) => return invalid("the liquidation is disputed: its liquidator proves it, or it lapses"),
    };
    let (mine, rest) = q.settle_pay(l.value as i64);
    let w = q.position.dispute_daa as u64;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(w, &q.position);
    args.extend(liq_stated(q));
    args.extend([iv(q.bond), bv(q.liquidator.to_vec()), iv(proved)]);
    let (_, lid) = enter(&mut d, lib, l, liq_live_obj(l), script, "KpfTLiqSettle", "settle", args, w)?;
    if mine > 0 {
        d.payout(mine, &q.liquidator, "the liquidator's fee and bond");
        keep(&mut d, script, lid)?;
        d.fund(p.funds, &p.change_to);
    } else {
        d.fund(p.funds, &p.change_to);
        keep(&mut d, script, lid)?;
    }
    if rest > 0 {
        d.last(rest, &q.winner_key(), "the other party", &p.change_to);
    }
    d.finish()
}

/// `KpfTCure`: the side liquidated signs and adds what the claim asks: the position again with that margin, the
/// liquidator's bond back (the last output). Of a claim nobody disputed, once it is half a dispute period old; of a proved
/// one, at any time; a disputed one is not cured.
pub fn cure(lib: &[u8; 32], l: &OLive<LiqObj>, script: &LibScript, p: Payer) -> Result<(OBuilt, Position)> {
    let q = &l.s.liq;
    let age = l.s.cure_age().ok_or_else(|| Error::Invalid("the liquidation is disputed: its liquidator proves it, and then it is cured, or it lapses".into()))? as u64;
    let next = q.cured().ok_or_else(|| Error::Invalid("the cure would take the margin out of its range".into()))?;
    let rest = (l.value as i64).checked_sub(q.bond).filter(|v| *v >= 0).ok_or_else(|| Error::Invalid("the liquidation holds less than its bond".into()))?;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(age, &q.position);
    args.extend(liq_stated(q));
    args.extend([iv(q.bond), bv(q.liquidator.to_vec()), iv(l.s.state_no()), Arg::Sig(q.loser_key())]);
    let (id, lid) = enter(&mut d, lib, l, liq_live_obj(l), script, "KpfTCure", "cure", args, age)?;
    d.out((rest + q.add) as u64, object_spk(lib, &TObj::Position(next)), Some((0, id)), "position", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    if q.bond > 0 {
        d.last(q.bond, &q.liquidator, "the liquidator's bond", &p.change_to);
    }
    Ok((d.finish()?, next))
}

/// `KpfTLiqDispute.dispute`: `disputer` asks for the proof of a liquidation that landed in time, with a bond as large
/// as the liquidator's.
pub fn liq_dispute(lib: &[u8; 32], l: &OLive<LiqObj>, script: &LibScript, disputer: [u8; 32], p: Payer) -> Result<(OBuilt, LiqObj)> {
    let q = &l.s.liq;
    if l.s.state != LiqState::Claimed {
        return invalid("a liquidation is disputed once: this one is disputed or proved already");
    }
    if !q.in_time(l.daa) {
        return invalid("the liquidation landed later than lateDaa after its time: it is ended as late, not disputed");
    }
    if q.bond <= 0 {
        return invalid("a liquidation without a bond cannot be disputed");
    }
    let next = LiqObj { liq: *q, state: LiqState::Disputed(disputer) };
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, &q.position);
    args.extend(liq_stated(q));
    args.extend([iv(q.bond), bv(q.liquidator.to_vec()), bv(disputer.to_vec())]);
    let (id, lid) = enter(&mut d, lib, l, liq_live_obj(l), script, "KpfTLiqDispute", "dispute", args, 0)?;
    d.out(l.value + q.bond as u64, object_spk(lib, &TObj::Liq(next)), Some((0, id)), "disputed liquidation", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, next))
}

/// `KpfTLiqDispute.lapse`: anyone ends a disputed liquidation its liquidator did not prove within the dispute period: the
/// position again, the disputer its bond and half of the liquidator's (the last output), the other half left in the
/// script's copy.
pub fn liq_lapse(lib: &[u8; 32], l: &OLive<LiqObj>, script: &LibScript, p: Payer) -> Result<(OBuilt, Position)> {
    let q = &l.s.liq;
    let LiqState::Disputed(dkey) = l.s.state else { return invalid("only a disputed liquidation lapses") };
    let (rest, to_disputer, to_liquidator) = l.s.lapse_pay(l.value as i64).ok_or_else(|| Error::Invalid("the disputed liquidation holds less than its two bonds".into()))?;
    let w = q.position.dispute_daa as u64;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(w, &q.position);
    args.extend(liq_stated(q));
    args.extend([iv(q.bond), bv(q.liquidator.to_vec()), bv(dkey.to_vec())]);
    let (id, lid) = enter(&mut d, lib, l, liq_live_obj(l), script, "KpfTLiqDispute", "lapse", args, w)?;
    d.out(rest as u64, object_spk(lib, &TObj::Position(q.position)), Some((0, id)), "position", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    // The script reads the two payouts from the end: the liquidator's bond, then the disputer's.
    d.last(to_liquidator, &q.liquidator, "the liquidator's bond", &p.change_to);
    d.payout(to_disputer, &dkey, "the disputer's bond");
    Ok((d.finish()?, q.position))
}

// ---------------------------------------------------------------- the exit from a committee that stopped signing

fn exit_stated(q: &ExitClaim) -> Vec<Arg> {
    vec![iv(q.side), iv(q.since)]
}

/// The two payouts of a "no price" claim that is ended (as `bond_shared`).
fn exit_ended(d: &mut ODraft, q: &ExitClaim, ender: &[u8; 32], to_ender: i64, to_other: i64, p: Payer) {
    bond_shared(d, &q.other_key(), ender, to_ender, to_other, p);
}

/// `KpfTExit.exit`: the owner of `side` states that the committee signed no price of one of the position's markets
/// since the DAA `since`, and posts the bond. Settled at the entries after the position's exit time, unless somebody
/// shows the prices.
pub fn exit(lib: &[u8; 32], position: &OLive<Position>, script: &LibScript, side: i64, since: i64, p: Payer) -> Result<(OBuilt, ExitClaim)> {
    let a = &position.s;
    let q = a.exit(side, since).ok_or_else(|| Error::Invalid("an exit claim is of side a or b, since a valid DAA".into()))?;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, a);
    args.extend(exit_stated(&q));
    args.push(Arg::Sig(a.key_of(side).expect("a side")));
    let (id, lid) = enter(&mut d, lib, position, TObj::Position(*a), script, "KpfTExit", "exit", args, 0)?;
    d.out(position.value + q.bond as u64, object_spk(lib, &TObj::ExitClaim(q)), Some((0, id)), "exit claim", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, q))
}

/// `KpfTExit.early`: `ender` ends a claim that landed before its own `since`: the position again, the bond shared.
pub fn exit_early(lib: &[u8; 32], l: &OLive<ExitClaim>, script: &LibScript, ender: [u8; 32], p: Payer) -> Result<(OBuilt, Position)> {
    let q = &l.s;
    if q.of_the_past(l.daa) {
        return invalid("the claim landed at or after its `since`: it is ended by the prices it denies, or settled");
    }
    let (rest, to_ender, to_other) = q.end_pay(l.value as i64).ok_or_else(|| Error::Invalid("the claim holds less than its bond".into()))?;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, &q.position);
    args.extend(exit_stated(q));
    args.extend([iv(q.bond), bv(ender.to_vec())]);
    let (id, lid) = enter(&mut d, lib, l, TObj::ExitClaim(*q), script, "KpfTExit", "early", args, 0)?;
    d.out(rest as u64, object_spk(lib, &TObj::Position(q.position)), Some((0, id)), "position", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    exit_ended(&mut d, q, &ender, to_ender, to_other, p);
    Ok((d.finish()?, q.position))
}

/// `KpfTExitEnd`: anyone settles a "no price" claim nobody ended, once it is as old as the position's exit time:
/// each party its margin.
pub fn exit_settle(lib: &[u8; 32], l: &OLive<ExitClaim>, script: &LibScript, p: Payer) -> Result<OBuilt> {
    let q = &l.s;
    if !q.of_the_past(l.daa) {
        return invalid("the claim landed before its own `since`: it is ended, not settled");
    }
    let w = q.position.exit_daa() as u64;
    let (pa, pb) = q.settle_pay(l.value as i64);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(w, &q.position);
    args.extend(exit_stated(q));
    args.push(iv(q.bond));
    let (_, lid) = enter(&mut d, lib, l, TObj::ExitClaim(*q), script, "KpfTExitEnd", "settle", args, w)?;
    pay_out(&mut d, script, lid, &q.position, pa, pb, p)?;
    d.finish()
}

/// `KpfTExitRefute`: `ender` ends a "no price" claim with the prices it denies: an epoch table of each leg's committee spent
/// by its attest for a signed price at or after the claim's `since` (the signed prices may differ a leg). The position again, the bond
/// shared by `ender` and the party the claim was made against.
pub fn exit_refute(lib: &[u8; 32], l: &OLive<ExitClaim>, script: &LibScript, t1: Attested, t2: Option<Attested>, ender: [u8; 32], p: Payer) -> Result<(OBuilt, Position)> {
    exit_refute_with(lib, l, script, t1, t2, ender, p, true)
}

/// `quiet_refute` without the tool's own check of the signed prices' time (as `prove_with`).
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn exit_refute_with(lib: &[u8; 32], l: &OLive<ExitClaim>, script: &LibScript, t1: Attested, t2: Option<Attested>, ender: [u8; 32], p: Payer, checked: bool) -> Result<(OBuilt, Position)> {
    let q = &l.s;
    let a = &q.position;
    if checked && [Some(t1), t2].into_iter().flatten().any(|(_, att)| att.t0 < q.since) {
        return invalid(format!("a signed price older than the claim's `since` ({}) does not answer it", q.since));
    }
    let (rest, to_ender, to_other) = q.end_pay(l.value as i64).ok_or_else(|| Error::Invalid("the claim holds less than its bond".into()))?;
    let mut probe = ODraft::new(p.rate, FeeMode::Relay);
    let (who, _) = ticks_in(&mut probe, a, [None, None], t1, t2)?;
    let (x1, x2) = (t1.1, t2.map(|t| t.1));
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.fixed()), bv(a.moving())];
    args.extend(exit_stated(q));
    args.extend([iv(q.bond), iv(x1.t0), iv(x1.p), iv(x2.map_or(0, |x| x.t0)), iv(x2.map_or(0, |x| x.p)), bv(ender.to_vec())]);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let (id, lid) = enter(&mut d, lib, l, TObj::ExitClaim(*q), script, "KpfTExitRefute", "refute", args, 0)?;
    let (_, rids) = ticks_in(&mut d, a, [None, None], t1, t2)?;
    d.out(rest as u64, object_spk(lib, &TObj::Position(*a)), Some((0, id)), "position", None);
    keep(&mut d, script, lid)?;
    epoch_tables_out(&mut d, t1, t2, &rids);
    d.fund(p.funds, &p.change_to);
    exit_ended(&mut d, q, &ender, to_ender, to_other, p);
    Ok((d.finish()?, *a))
}

// ---------------------------------------------------------------- orders between users

fn order_args(o: &Order) -> Vec<Arg> {
    vec![bv(o.owner.to_vec()), iv(o.size), iv(o.limit), iv(o.margin), iv(o.tip), iv(o.expiry), iv(o.min_fill), iv(o.min_counter)]
}

/// An order placed in the library `lib`: output 0, under a covenant id of its own, holding its margin and its tip.
/// The payer's first input authorizes it.
pub fn order_place(lib: &[u8; 32], anchor: &Anchor, script: &LibScript, o: &Order, p: Payer) -> Result<OBuilt> {
    o.check()?;
    if o.value() < ORDER_MIN {
        return invalid("an order holds at least 0.2 KAS of margin and tip");
    }
    if p.funds.is_empty() {
        return invalid("an order needs a funding input");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(o.tpl.to_vec()), bv(o.owner.to_vec())];
    args.extend([o.dir, o.size, o.limit, o.margin, o.tip, o.expiry, o.min_fill, o.min_counter].map(iv));
    args.push(Arg::Sig(o.owner));
    enters(&mut d, lib, anchor, script, "KpfTPlace", "place", args, TObj::Order(*o), o.value() as u64)?;
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// `KpfTMatch`: whoever pays for this transaction matches a long order (input 0) and a short one (input 1) of the
/// template `tpl` and of one registry for `n` units at the price `x`; the script is input 2. Output 0 is the position; what
/// is left of each order is written after it (the long's first); the two tips' shares stay in the payer's change.
pub fn order_match(lib: &[u8; 32], tpl: &Template, long: &OLive<Order>, short: &OLive<Order>, script: &LibScript, n: i64, x: i64, p: Payer) -> Result<(OBuilt, Matched)> {
    let m = match_orders(tpl, &long.s, long.value as i64, &short.s, short.value as i64, n, x)?;
    let idl = match (long.id, short.id) {
        (Some(a), Some(b)) if a == b => a,
        _ => return invalid("the two orders are not of one registry: one of them did not enter it through an anchor"),
    };
    let ids = idl;
    if script.s != "KpfTMatch" || script.id.map(|h| h.as_bytes()) != Some(*lib) {
        return invalid("the script given is not a copy of KpfTMatch in the orders' library");
    }
    let lid = Hash::from_bytes(*lib);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    d.contract(long, OWhich::T(SLOT), TObj::Order(long.s).script(lib), "go", vec![iv(2), iv(0)], "long order", 0);
    d.contract(short, OWhich::T(SLOT), TObj::Order(short.s).script(lib), "go", vec![iv(2), iv(1)], "short order", 0);
    let who: Vec<u8> = [0i64, 1, 1].iter().flat_map(|w| sm8(*w)).collect();
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(tpl.bytes())];
    args.extend(order_args(&long.s));
    args.extend(order_args(&short.s));
    args.extend([iv(n), iv(x)]);
    let code = script_of("KpfTMatch")?.to_vec();
    d.input(script.op, kaspa_consensus_core::tx::UtxoEntry::new(script.value, pay_to_script_hash_script(&code), script.daa, false, script.id), OPlan::Contract { which: OWhich::T("KpfTMatch"), redeem: code, entry: "match", args }, "KpfTMatch", 0);
    // Output 0: the position. Output 1: what rests of the long order (the second output of its id), else what rests of
    // the short one, else the change. Output 2: the script.
    d.out((m.position.x.a_margin + m.position.x.b_margin) as u64, object_spk(lib, &TObj::Position(m.position)), Some((0, idl)), "position", None);
    let rests = |d: &mut ODraft, r: &Rest, auth: u16, id: Hash, label: &str| {
        if let Rest::Order(o, v) = r {
            d.out(*v as u64, object_spk(lib, &TObj::Order(*o)), Some((auth, id)), label, None);
        }
    };
    let (long_rests, short_rests) = (matches!(m.long, Rest::Order(..)), matches!(m.short, Rest::Order(..)));
    let mut funded = false;
    if long_rests {
        rests(&mut d, &m.long, 0, idl, "what rests of the long order");
    } else if short_rests {
        rests(&mut d, &m.short, 1, ids, "what rests of the short order");
    } else {
        d.fund(p.funds, &p.change_to);
        funded = true;
    }
    d.out(script.value + ORACLE_FEE as u64, script_spk("KpfTMatch")?, Some((2, lid)), "KpfTMatch", None);
    if long_rests && short_rests {
        rests(&mut d, &m.short, 1, ids, "what rests of the short order");
    }
    if !funded {
        d.fund(p.funds, &p.change_to);
    }
    // The tips' shares are written nowhere: the payer's change has them.
    Ok((d.finish()?, m))
}

fn order_end(lib: &[u8; 32], order: &OLive<Order>, script: &LibScript, signed: bool, p: Payer) -> Result<OBuilt> {
    let o = &order.s;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(o.tpl.to_vec()), bv(o.owner.to_vec()), iv(o.dir), iv(o.size), iv(o.limit), iv(o.margin), iv(o.tip), iv(o.expiry), iv(o.min_fill), iv(o.min_counter)];
    if signed {
        args.push(Arg::Sig(o.owner));
    } else {
        d.lock(o.expiry as u64);
    }
    let (_, lid) = enter(&mut d, lib, order, TObj::Order(*o), script, "KpfTCancel", if signed { "cancel" } else { "expire" }, args, 0)?;
    d.payout(order.value as i64, &o.owner, "the order, back to its owner");
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// `KpfTCancel.cancel`: the owner takes its order back (its key signs).
pub fn order_cancel(lib: &[u8; 32], order: &OLive<Order>, script: &LibScript, p: Payer) -> Result<OBuilt> {
    order_end(lib, order, script, true, p)
}

/// `KpfTCancel.expire`: anyone returns an order to its owner from its expiry on (the lock time proves the DAA).
pub fn order_expire(lib: &[u8; 32], order: &OLive<Order>, script: &LibScript, p: Payer) -> Result<OBuilt> {
    order_end(lib, order, script, false, p)
}

// ---------------------------------------------------------------- two positions into one

/// `KpfTMerge`: whoever pays for this transaction merges two positions of the same two parties and of one registry (inputs 0 and 1; the
/// script is input 2). Output 0 is the one position, under the first's covenant id, holding what the two held less
/// `MERGE_TIP`, which the payer's change takes.
pub fn merge(lib: &[u8; 32], first: &OLive<Position>, second: &OLive<Position>, script: &LibScript, p: Payer) -> Result<(OBuilt, Position)> {
    let out = merged(&first.s, &second.s)?;
    let id1 = match (first.id, second.id) {
        (Some(a), Some(b)) if a == b => a,
        _ => return invalid("the two positions are not of one registry: one of them was not opened or matched in it"),
    };
    if first.op == second.op {
        return invalid("a position is not merged with itself");
    }
    if script.s != "KpfTMerge" || script.id.map(|h| h.as_bytes()) != Some(*lib) {
        return invalid("the script given is not a copy of KpfTMerge in the positions' library");
    }
    let held = first.value as i64 + second.value as i64;
    if held < MERGE_TIP || held - MERGE_TIP < out.pot() {
        return invalid("the two positions hold less than their margins");
    }
    let lid = Hash::from_bytes(*lib);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    d.contract(first, OWhich::T(SLOT), TObj::Position(first.s).script(lib), "go", vec![iv(2), iv(0)], "first position", 0);
    d.contract(second, OWhich::T(SLOT), TObj::Position(second.s).script(lib), "go", vec![iv(2), iv(1)], "second position", 0);
    let who: Vec<u8> = [0i64, 1, 1].iter().flat_map(|w| sm8(*w)).collect();
    let l2 = &second.s.x.legs[0];
    let args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(first.s.fixed()), bv(first.s.moving()), iv(l2.size), iv(l2.entry), iv(second.s.x.a_margin), iv(second.s.x.b_margin), iv(second.s.a_min_counter), iv(second.s.b_min_counter)];
    let code = script_of("KpfTMerge")?.to_vec();
    d.input(script.op, kaspa_consensus_core::tx::UtxoEntry::new(script.value, pay_to_script_hash_script(&code), script.daa, false, script.id), OPlan::Contract { which: OWhich::T("KpfTMerge"), redeem: code, entry: "merge", args }, "KpfTMerge", 0);
    d.out((held - MERGE_TIP) as u64, object_spk(lib, &TObj::Position(out)), Some((0, id1)), "position", None);
    d.fund(p.funds, &p.change_to);
    d.out(script.value + ORACLE_FEE as u64, script_spk("KpfTMerge")?, Some((2, lid)), "KpfTMerge", None);
    Ok((d.finish()?, out))
}

// ---------------------------------------------------------------- a user leaves two positions

/// `KpfTNovate`: whoever pays for this transaction takes a user out of two positions of one registry that cancel each other: `x`, in
/// which it is long (its A; input 0), and `y`, in which it is short (its B; input 1); the script is input 2. The
/// positions written (the two it faced against each other, and what is left of the larger position) are outputs 0 and
/// 1, with their places returned; the user's own is the last output.
pub fn novate(lib: &[u8; 32], x: &OLive<Position>, y: &OLive<Position>, script: &LibScript, p: Payer) -> Result<(OBuilt, Novation, Vec<(usize, Position)>)> {
    novate_with(lib, x, y, script, p, true)
}

/// `KpfTReplace`: whoever pays for this transaction matches two orders of a registry into a position of it: the
/// party `side` of `x` leaves by its own order the other way, and the owner of the other order takes its place, in one
/// transaction where a match and a novation were two. The tips' shares of the two orders are the payer's.
#[allow(clippy::too_many_arguments)]
pub fn replace(lib: &[u8; 32], x: &OLive<Position>, long: &OLive<Order>, short: &OLive<Order>, script: &LibScript, n: i64, price: i64, side: i64, p: Payer) -> Result<(OBuilt, crate::thin::Replaced)> {
    replace_with(lib, x, long, short, script, n, price, side, p, true)
}

/// As `replace`; `rules: false` builds what the script must refuse (a test's).
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn replace_with(lib: &[u8; 32], x: &OLive<Position>, long: &OLive<Order>, short: &OLive<Order>, script: &LibScript, n: i64, price: i64, side: i64, p: Payer, rules: bool) -> Result<(OBuilt, crate::thin::Replaced)> {
    let r = crate::thin::replaced(&x.s, x.value as i64, &long.s, long.value as i64, &short.s, short.value as i64, n, price, side, rules)?;
    let id = match (x.id, long.id, short.id) {
        (Some(a), Some(b), Some(c)) if a == b && b == c => a,
        _ => return invalid("the position and the two orders are not of one registry"),
    };
    if script.s != "KpfTReplace" || script.id.map(|h| h.as_bytes()) != Some(*lib) {
        return invalid("the script given is not a copy of KpfTReplace in the position's library");
    }
    let lid = Hash::from_bytes(*lib);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    d.contract(x, OWhich::T(SLOT), TObj::Position(x.s).script(lib), "go", vec![iv(3), iv(0)], "the position", 0);
    d.contract(long, OWhich::T(SLOT), TObj::Order(long.s).script(lib), "go", vec![iv(3), iv(1)], "long order", 0);
    d.contract(short, OWhich::T(SLOT), TObj::Order(short.s).script(lib), "go", vec![iv(3), iv(2)], "short order", 0);
    let who: Vec<u8> = [0i64, 1, 2].iter().flat_map(|w| sm8(*w)).collect();
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(x.s.fixed()), bv(x.s.moving())];
    args.extend(order_args(&long.s));
    args.extend(order_args(&short.s));
    args.extend([iv(n), iv(price), iv(side)]);
    let code = script_of("KpfTReplace")?.to_vec();
    d.input(script.op, kaspa_consensus_core::tx::UtxoEntry::new(script.value, pay_to_script_hash_script(&code), script.daa, false, script.id), OPlan::Contract { which: OWhich::T("KpfTReplace"), redeem: code, entry: "replace", args }, "KpfTReplace", 0);
    // Under the registry's id, in this order: what is left of the position, the new position, what rests of the long
    // order, what rests of the short one. The script is input 3 and goes on at output 3.
    let mut objs: Vec<(TObj, i64, u16, &str)> = vec![];
    if let Some((a, v)) = r.rest {
        objs.push((TObj::Position(a), v, 0, "what is left of the position"));
    }
    if let Some((a, v)) = r.z {
        objs.push((TObj::Position(a), v, 0, "the position, the newcomer in the leaver's place"));
    }
    if let Rest::Order(o, v) = r.long {
        objs.push((TObj::Order(o), v, 1, "what rests of the long order"));
    }
    if let Rest::Order(o, v) = r.short {
        objs.push((TObj::Order(o), v, 2, "what rests of the short order"));
    }
    let mut funded = false;
    for k in 0..3 {
        if let Some((o, v, auth, label)) = objs.get(k) {
            d.out(*v as u64, object_spk(lib, o), Some((*auth, id)), label, None);
        } else if !funded {
            d.fund(p.funds, &p.change_to);
            funded = true;
        } else {
            d.plain(100_000_000, &p.change_to, "filler");
        }
    }
    d.out(script.value + ORACLE_FEE as u64, script_spk("KpfTReplace")?, Some((3, lid)), "KpfTReplace", None);
    if let Some((o, v, auth, label)) = objs.get(3) {
        d.out(*v as u64, object_spk(lib, o), Some((*auth, id)), label, None);
    }
    if !funded {
        d.fund(p.funds, &p.change_to);
    }
    // From the end: the user that leaves (last); when the newcomer is the one that stays, its own the place before.
    if r.to_stayer > 0 {
        d.last(r.to_stayer, &r.stayer, "the one that stays, paid", &p.change_to);
        if r.to_leaver > 0 {
            d.payout(r.to_leaver, &r.leaver, "the user that leaves");
        } else {
            d.plain(100_000_000, &p.change_to, "filler");
        }
    } else if r.to_leaver > 0 {
        d.last(r.to_leaver, &r.leaver, "the user that leaves", &p.change_to);
    }
    // The tips' shares are written nowhere: the payer's change has them.
    Ok((d.finish()?, r))
}

/// As `novate`; `rules: false` builds what the script must refuse (a test's).
pub fn novate_with(lib: &[u8; 32], x: &OLive<Position>, y: &OLive<Position>, script: &LibScript, p: Payer, rules: bool) -> Result<(OBuilt, Novation, Vec<(usize, Position)>)> {
    let nv = novated(&x.s, x.value as i64, &y.s, y.value as i64, rules)?;
    let id = match (x.id, y.id) {
        (Some(a), Some(b)) if a == b => a,
        _ => return invalid("the two positions are not of one registry: one of them was not opened or matched in it"),
    };
    if script.s != "KpfTNovate" || script.id.map(|h| h.as_bytes()) != Some(*lib) {
        return invalid("the script given is not a copy of KpfTNovate in the positions' library");
    }
    let lid = Hash::from_bytes(*lib);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    d.contract(x, OWhich::T(SLOT), TObj::Position(x.s).script(lib), "go", vec![iv(2), iv(0)], "the position the user is long in", 0);
    d.contract(y, OWhich::T(SLOT), TObj::Position(y.s).script(lib), "go", vec![iv(2), iv(1)], "the position the user is short in", 0);
    let who: Vec<u8> = [0i64, 1, 1].iter().flat_map(|w| sm8(*w)).collect();
    let ly = &y.s.x.legs[0];
    let args = vec![
        bv(who),
        bv(sm8(0).to_vec()),
        iv(pinned::kit().object_len() as i64),
        bv(x.s.fixed()),
        bv(x.s.moving()),
        bv(y.s.x.a_key.to_vec()),
        iv(ly.size),
        iv(ly.entry),
        iv(y.s.x.a_margin),
        iv(y.s.x.b_margin),
        iv(y.s.a_min_counter),
        iv(y.s.b_min_counter),
    ];
    let code = script_of("KpfTNovate")?.to_vec();
    d.input(script.op, kaspa_consensus_core::tx::UtxoEntry::new(script.value, pay_to_script_hash_script(&code), script.daa, false, script.id), OPlan::Contract { which: OWhich::T("KpfTNovate"), redeem: code, entry: "novate", args }, "KpfTNovate", 0);
    // Under the registry's id, in this order: what is left of X, the new position, what is left of Y.
    let mut objs: Vec<(Position, i64, u16, Hash, &str)> = vec![];
    if let Some((a, v)) = nv.rest_x {
        objs.push((a, v, 0, id, "what is left of the first position"));
    }
    if let Some((a, v)) = nv.z {
        objs.push((a, v, 0, id, "the position of the two that stay"));
    }
    if let Some((a, v)) = nv.rest_y {
        objs.push((a, v, 1, id, "what is left of the second position"));
    }
    let mut made = vec![];
    let mut funded = false;
    for k in 0..2 {
        if let Some((a, v, auth, id, label)) = objs.get(k) {
            let i = d.out(*v as u64, object_spk(lib, &TObj::Position(*a)), Some((*auth, *id)), label, None);
            made.push((i, *a));
        } else if !funded {
            d.fund(p.funds, &p.change_to);
            funded = true;
        } else {
            d.plain(100_000_000, &p.change_to, "filler");
        }
    }
    d.out(script.value + ORACLE_FEE as u64, script_spk("KpfTNovate")?, Some((2, lid)), "KpfTNovate", None);
    if !funded {
        d.fund(p.funds, &p.change_to);
    }
    // From the end: the user that leaves (last); when the two that stay are one user, its own two places before.
    if nv.to_stayer > 0 {
        d.last(nv.to_stayer, &x.s.x.b_key, "the one that stays, paid", &p.change_to);
        d.plain(100_000_000, &p.change_to, "filler");
        if nv.to_leaver > 0 {
            d.payout(nv.to_leaver, &x.s.x.a_key, "the user that leaves");
        } else {
            d.plain(100_000_000, &p.change_to, "filler");
        }
    } else if nv.to_leaver > 0 {
        d.last(nv.to_leaver, &x.s.x.a_key, "the user that leaves", &p.change_to);
    }
    Ok((d.finish()?, nv, made))
}

// ---------------------------------------------------------------- two markets into one position

/// `KpfTJoin`: the two users sign, and whoever pays for this transaction joins their two positions, of one registry and of two
/// markets (inputs 0 and 1; the script is input 2). Output 0 is the cross position, holding what the two held less
/// `MERGE_TIP`, which the payer's change takes.
pub fn join(lib: &[u8; 32], first: &OLive<Position>, second: &OLive<Position>, script: &LibScript, p: Payer) -> Result<(OBuilt, Position)> {
    let out = joined(&first.s, &second.s)?;
    let id = match (first.id, second.id) {
        (Some(a), Some(b)) if a == b => a,
        _ => return invalid("the two positions are not of one registry: one of them was not opened or matched in it"),
    };
    if script.s != "KpfTJoin" || script.id.map(|h| h.as_bytes()) != Some(*lib) {
        return invalid("the script given is not a copy of KpfTJoin in the positions' library");
    }
    let held = first.value as i64 + second.value as i64;
    if held < MERGE_TIP || held - MERGE_TIP < out.pot() {
        return invalid("the two positions hold less than their margins");
    }
    let lid = Hash::from_bytes(*lib);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    d.contract(first, OWhich::T(SLOT), TObj::Position(first.s).script(lib), "go", vec![iv(2), iv(0)], "first position", 0);
    d.contract(second, OWhich::T(SLOT), TObj::Position(second.s).script(lib), "go", vec![iv(2), iv(1)], "second position", 0);
    let who: Vec<u8> = [0i64, 1, 1].iter().flat_map(|w| sm8(*w)).collect();
    let turned = first.s.x.a_key != second.s.x.a_key;
    let leg = second.s.fixed()[90..201].to_vec();
    let args = vec![
        bv(who),
        bv(sm8(0).to_vec()),
        iv(pinned::kit().object_len() as i64),
        bv(first.s.fixed()),
        bv(first.s.moving()),
        bv(leg),
        iv(turned as i64),
        iv(second.s.x.a_margin),
        iv(second.s.x.b_margin),
        iv(second.s.a_min_counter),
        iv(second.s.b_min_counter),
        Arg::Sig(first.s.x.a_key),
        Arg::Sig(first.s.x.b_key),
    ];
    let code = script_of("KpfTJoin")?.to_vec();
    d.input(script.op, kaspa_consensus_core::tx::UtxoEntry::new(script.value, pay_to_script_hash_script(&code), script.daa, false, script.id), OPlan::Contract { which: OWhich::T("KpfTJoin"), redeem: code, entry: "join", args }, "KpfTJoin", 0);
    d.out((held - MERGE_TIP) as u64, object_spk(lib, &TObj::Position(out)), Some((0, id)), "position", None);
    d.fund(p.funds, &p.change_to);
    d.out(script.value + ORACLE_FEE as u64, script_spk("KpfTJoin")?, Some((2, lid)), "KpfTJoin", None);
    Ok((d.finish()?, out))
}

// ---------------------------------------------------------------- one side of a position of two legs

/// `KpfTTurn`: the same position written the other way round (`users`: its two users change places; else its two legs
/// do). Anyone, once the position is `TURN_AGE` old.
pub fn turn(lib: &[u8; 32], position: &OLive<Position>, script: &LibScript, users: bool, p: Payer) -> Result<(OBuilt, Position)> {
    let out = if users { turned_users(&position.s) } else { turned_legs(&position.s)? };
    let age = crate::thin::TURN_AGE;
    let args = vec![bv(vec![0; 24]), bv(sm8(age as i64).to_vec()), iv(pinned::kit().object_len() as i64), bv(position.s.fixed()), bv(position.s.moving()), iv(if users { 1 } else { 2 })];
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let (id, lid) = enter(&mut d, lib, position, TObj::Position(position.s), script, "KpfTTurn", "turn", args, age)?;
    d.out(position.value, object_spk(lib, &TObj::Position(out)), Some((0, id)), "position", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, out))
}

/// The two inputs and the script's of an operation on two positions of one registry: `x` shown whole, then `more`.
fn two_positions(d: &mut ODraft, lib: &[u8; 32], x: &OLive<Position>, y: &OLive<Position>, script: &LibScript, name: &'static str, entry: &'static str, more: Vec<Arg>) -> Result<(Hash, Hash)> {
    let id = match (x.id, y.id) {
        (Some(a), Some(b)) if a == b => a,
        _ => return invalid("the two positions are not of one registry: one of them was not opened or matched in it"),
    };
    if x.op == y.op {
        return invalid("a position is not put together with itself");
    }
    if script.s != name || script.id.map(|h| h.as_bytes()) != Some(*lib) {
        return invalid(format!("the script given is not a copy of {name} in the positions' library"));
    }
    d.contract(x, OWhich::T(SLOT), TObj::Position(x.s).script(lib), "go", vec![iv(2), iv(0)], "first position", 0);
    d.contract(y, OWhich::T(SLOT), TObj::Position(y.s).script(lib), "go", vec![iv(2), iv(1)], "second position", 0);
    let who: Vec<u8> = [0i64, 1, 1].iter().flat_map(|w| sm8(*w)).collect();
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(x.s.fixed()), bv(x.s.moving())];
    args.extend(more);
    let code = script_of(name)?.to_vec();
    d.input(script.op, kaspa_consensus_core::tx::UtxoEntry::new(script.value, pay_to_script_hash_script(&code), script.daa, false, script.id), OPlan::Contract { which: OWhich::T(name), redeem: code, entry, args }, name, 0);
    Ok((id, Hash::from_bytes(*lib)))
}

/// `KpfTFold`: whoever pays for this transaction folds the position `y` into the first leg of `x` (the same two users,
/// one registry). Output 0 is the position after, holding what the two held less `MERGE_TIP`.
pub fn fold(lib: &[u8; 32], x: &OLive<Position>, y: &OLive<Position>, script: &LibScript, p: Payer) -> Result<(OBuilt, Position)> {
    let out = folded(&x.s, &y.s)?;
    let held = x.value as i64 + y.value as i64;
    if held < MERGE_TIP || held - MERGE_TIP < out.pot() {
        return invalid("the two positions hold less than their margins");
    }
    let turned = x.s.x.a_key != y.s.x.a_key;
    let ly = &y.s.x.legs[0];
    let more = vec![iv(turned as i64), iv(ly.size), iv(ly.entry), iv(y.s.x.a_margin), iv(y.s.x.b_margin), iv(y.s.a_min_counter), iv(y.s.b_min_counter)];
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let (id, lid) = two_positions(&mut d, lib, x, y, script, "KpfTFold", "fold", more)?;
    d.out((held - MERGE_TIP) as u64, object_spk(lib, &TObj::Position(out)), Some((0, id)), "position", None);
    d.fund(p.funds, &p.change_to);
    d.out(script.value + ORACLE_FEE as u64, script_spk("KpfTFold")?, Some((2, lid)), "KpfTFold", None);
    Ok((d.finish()?, out))
}

/// `KpfTLeave`: the two users of `x` sign, and whoever pays for this transaction takes the first leg of `x` (two legs;
/// its A the user that sells) to the third user of `y`. Output 0 is `x` after, output 1 the position of the two that face each other, then what is
/// left of `y`; the user's margin in `y` is the last output.
pub fn leave(lib: &[u8; 32], x: &OLive<Position>, y: &OLive<Position>, script: &LibScript, p: Payer) -> Result<(OBuilt, Leaving)> {
    leave_with(lib, x, y, script, p, true)
}

/// As `leave`; `rules: false` builds what the script must refuse (a test's).
pub fn leave_with(lib: &[u8; 32], x: &OLive<Position>, y: &OLive<Position>, script: &LibScript, p: Payer, rules: bool) -> Result<(OBuilt, Leaving)> {
    let lv = left(&x.s, x.value as i64, &y.s, y.value as i64, rules)?;
    let ly = &y.s.x.legs[0];
    let third = if x.s.x.legs[0].dir == crate::state::LONG { y.s.x.a_key } else { y.s.x.b_key };
    let more = vec![bv(third.to_vec()), iv(ly.size), iv(ly.entry), iv(y.s.x.a_margin), iv(y.s.x.b_margin), iv(y.s.a_min_counter), iv(y.s.b_min_counter), Arg::Sig(x.s.x.a_key), Arg::Sig(x.s.x.b_key)];
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let (id, lid) = two_positions(&mut d, lib, x, y, script, "KpfTLeave", "leave", more)?;
    d.out(lv.x_after.1 as u64, object_spk(lib, &TObj::Position(lv.x_after.0)), Some((0, id)), "the position after", None);
    d.out(lv.z.1 as u64, object_spk(lib, &TObj::Position(lv.z.0)), Some((0, id)), "the position of the two that face each other", None);
    d.out(script.value + ORACLE_FEE as u64, script_spk("KpfTLeave")?, Some((2, lid)), "KpfTLeave", None);
    if let Some((r, v)) = lv.rest_y {
        d.out(v as u64, object_spk(lib, &TObj::Position(r)), Some((1, id)), "what is left of the second position", None);
    }
    d.fund(p.funds, &p.change_to);
    if lv.to_leaver > 0 {
        d.last(lv.to_leaver, &x.s.x.a_key, "the user that sold the leg", &p.change_to);
    }
    Ok((d.finish()?, lv))
}

// ---------------------------------------------------------------- the oracle fees

fn collector(script: &LibScript, name: &'static str, lib: &[u8; 32], fee_pool: &FeePool) -> Result<Hash> {
    if script.s != name || script.id.map(|h| h.as_bytes()) != Some(*lib) {
        return invalid(format!("the script given is not a copy of {name} in this library"));
    }
    if fee_pool.id.map(|h| h.as_bytes()) != Some(*lib) {
        return invalid("the fee pool given is not this library's");
    }
    Ok(Hash::from_bytes(*lib))
}

/// Inputs 0 and 1 of a collection: the fee pool and a copy of the collecting script, whose arguments begin with the
/// places, the age, the length of the fee pool's script and the committee.
fn fee_pool_in(d: &mut ODraft, fee_pool: &FeePool, script: &LibScript, name: &'static str, entry: &'static str, who: [i64; 3], more: Vec<Arg>) -> Result<()> {
    let redeem = fee_pool.s.script();
    let mut args = vec![bv(who.iter().flat_map(|w| sm8(*w)).collect()), bv(sm8(0).to_vec()), iv(redeem.len() as i64), bv(fee_pool.s.committee.to_vec()), iv(fee_pool.s.floor)];
    args.extend(more);
    d.contract(fee_pool, OWhich::T(FEE_POOL), redeem, "go", vec![iv(1), iv(0)], "fee pool", 0);
    let code = script_of(name)?.to_vec();
    d.input(script.op, kaspa_consensus_core::tx::UtxoEntry::new(script.value, pay_to_script_hash_script(&code), script.daa, false, script.id), OPlan::Contract { which: OWhich::T(name), redeem: code, entry, args }, name, 0);
    Ok(())
}

/// `KpfTSweep`: what gathered in `copy` above its floor moves to the fee pool, less the sender's part (which its change
/// takes). Returns what moved and what of it reached the fee pool.
pub fn sweep(lib: &[u8; 32], fee_pool: &FeePool, script: &LibScript, copy: &LibScript, p: Payer) -> Result<(OBuilt, i64, i64)> {
    let lid = collector(script, "KpfTSweep", lib, fee_pool)?;
    if copy.id.map(|h| h.as_bytes()) != Some(*lib) || copy.op == script.op {
        return invalid("the copy to sweep is another copy of a script of this library");
    }
    if COLLECTORS.contains(&copy.s) {
        return invalid("a copy of a script that collects takes no oracle fees: there is nothing in it to sweep");
    }
    let (moved, to_fee_pool) = crate::thin::swept(copy.value as i64, fee_pool.s.floor).ok_or_else(|| Error::Invalid(format!("the copy holds less than {} sompi above its floor: nothing to sweep yet", crate::thin::SWEEP_MIN)))?;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    fee_pool_in(&mut d, fee_pool, script, "KpfTSweep", "sweep", [0, 2, 2], vec![])?;
    let code = script_of(copy.s)?.to_vec();
    d.input(copy.op, kaspa_consensus_core::tx::UtxoEntry::new(copy.value, pay_to_script_hash_script(&code), copy.daa, false, copy.id), OPlan::Contract { which: OWhich::T(copy.s), redeem: code, entry: "release", args: vec![iv(1), iv(1)] }, copy.s, 0);
    d.out(fee_pool.value + to_fee_pool as u64, fee_pool_spk(&fee_pool.s), Some((0, lid)), "fee pool", None);
    keep(&mut d, script, lid)?;
    d.out(fee_pool.s.floor as u64, script_spk(copy.s)?, Some((2, lid)), copy.s, None);
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, moved, to_fee_pool))
}

/// `KpfTPay`: the fee pool is paid to the members of its committee, each by its weight, less the sender's part. Input 2 is a
/// epoch table of the committee spent by its attest (any signed price of its dispute period a quorum signed): its table is who is paid.
/// The epoch table is one of an epoch that has ended (the fees are paid in arrears, to the members that served the epoch).
/// The payer gives one coin: the transaction is the fee pool, the script, the epoch table and that coin (review T1b, F22).
/// Returns what each member is paid, in the table's order.
pub fn pay(lib: &[u8; 32], fee_pool: &FeePool, script: &LibScript, t: Attested, p: Payer) -> Result<(OBuilt, Vec<([u8; 32], i64)>)> {
    let lid = collector(script, "KpfTPay", lib, fee_pool)?;
    let (epoch_table, att) = t;
    if epoch_table.id.map(|h| h.as_bytes()) != Some(fee_pool.s.committee) {
        return invalid("the epoch table is not of the committee this library's oracle fees are for");
    }
    let (taken, shares) = crate::thin::oracle_fee_shares(fee_pool.value as i64, fee_pool.s.floor, &epoch_table.s.members, epoch_table.s.total).ok_or_else(|| Error::Invalid("the fee pool holds nothing to share yet".into()))?;
    if let Some((_, smallest)) = shares.iter().min_by_key(|s| s.1) {
        if *smallest < crate::oracle::VIABLE_OUTPUT_MIN as i64 {
            return invalid(format!("the smallest member's share is {smallest} sompi: an output that small cannot be mined. Every member is paid at once or none is: the fee pool is paid when it holds more"));
        }
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let [coin] = p.funds else {
        return invalid("a payment is paid for by one coin: its transaction is the fee pool, the script, the epoch table and that coin");
    };
    fee_pool_in(&mut d, fee_pool, script, "KpfTPay", "pay", [0, 0, 0], vec![bv(coin.key.to_vec())])?;
    // The table paid is an ended epoch's: the transaction is not valid before the epoch table's `until`.
    d.lock(att.t0.max(epoch_table.s.until) as u64);
    let rid = crate::committee_build::attest_input(&mut d, epoch_table, att, att.t0 as u64)?;
    d.out(fee_pool.value - taken as u64, fee_pool_spk(&fee_pool.s), Some((0, lid)), "fee pool", None);
    keep(&mut d, script, lid)?;
    d.obj(crate::oracle_build::OObj::C(crate::committee_build::CObj::EpochTable(epoch_table.s)), epoch_table.value, Some((2, rid)), "epoch table");
    for (key, share) in &shares {
        d.out(*share as u64, crate::tx::p2pk_spk(key), None, "member", None);
    }
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, shares))
}

// ---------------------------------------------------------------- an executor's coins

/// The payer's coins as `count` coins of `each` and the change: a transaction takes a coin, and one not yet in a
/// block is not taken again, so an executor sends as many transactions a round as it has coins.
pub fn split(count: usize, each: u64, p: Payer) -> Result<OBuilt> {
    if !(1..=32).contains(&count) {
        return invalid("1 to 32 coins a transaction");
    }
    if each < 20_000_000 {
        return invalid("a coin of less than 0.2 KAS pays for little and costs its storage: 0.2 KAS at least");
    }
    if p.funds.is_empty() {
        return invalid("no coin to split");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    for _ in 0..count {
        d.plain(each as i64, &p.change_to, "coin");
    }
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// A payment of `amount` to the key `to`, with `payload` as the transaction's payload (a note anyone who reads the
/// transaction reads; at most 1,000 bytes).
pub fn send(to: &[u8; 32], amount: u64, payload: Vec<u8>, p: Payer) -> Result<OBuilt> {
    if amount < 20_000_000 {
        return invalid("an output of less than 0.2 KAS costs its storage: 0.2 KAS at least");
    }
    if payload.len() > 1_000 {
        return invalid("a note of at most 1,000 bytes");
    }
    if p.funds.is_empty() {
        return invalid("no coin to pay with");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    d.plain(amount as i64, to, "payment");
    d.fund(p.funds, &p.change_to);
    d.payload = payload;
    d.finish()
}

// ---------------------------------------------------------------- signing on two machines

/// Keys as a builder is given them: as they are, or each replaced by another (`sign_apart`).
pub type Swap<'a> = &'a dyn Fn([u8; 32]) -> [u8; 32];

pub fn swap_position(x: Position, f: Swap) -> Position {
    let mut y = x;
    (y.x.a_key, y.x.b_key) = (f(x.x.a_key), f(x.x.b_key));
    y
}

pub fn swap_funds(u: &[KeyUtxo], f: Swap) -> Vec<KeyUtxo> {
    u.iter().map(|x| KeyUtxo { outpoint: x.outpoint, amount: x.amount, daa: x.daa, key: f(x.key) }).collect()
}

/// A transaction that two users sign (an opening, a close by both, a join, a leg that leaves), signed by the keys at
/// hand only: the others sign on their own machines (`OPartial::sign_more`), and the last has the whole
/// (`OPartial::finished`). `mk` builds the transaction with the keys it is given.
///
/// A script that waits for a signature cannot be run, so what it costs to run cannot be measured, and the budget an
/// input commits to is signed. It is measured on a twin: the same transaction built with a key of this call's own in
/// the place of every key (the same inputs, the same sizes, the same path through every script), which is signed
/// whole here. Returned with the partly signed transaction: its size in bytes when whole.
pub fn sign_apart(mk: &dyn Fn(Swap) -> Result<OBuilt>, keys: &KeySet) -> Result<(OPartial, u64)> {
    let b = mk(&|k| k)?;
    let twin_keys = std::cell::RefCell::new(KeySet::new());
    let twin = |k: [u8; 32]| -> [u8; 32] {
        let secret = crate::committee::blake2b(&[&b"kpf twin key"[..], &k[..]].concat());
        twin_keys.borrow_mut().add_secret(&secret).expect("a hash is a secret key")
    };
    let tb = mk(&twin)?;
    let tw = tb.sign(&twin_keys.borrow())?;
    if tw.tx.inputs.len() != b.tx.inputs.len() || tw.tx.outputs.len() != b.tx.outputs.len() {
        return invalid("the twin of this transaction has another shape");
    }
    Ok((b.sign_open(keys, &tw.units)?, tw.mass.size))
}
