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
use crate::thin::{folded, joined, left, match_orders, merged, novated, turned_legs, turned_users, Position, Leaving, Novation, Claim, Disputed, Liq, LiqObj, LiqState, Matched, Order, ExitClaim, Rest, TObj, Template, Note, Change, Take, Account, ATake, Pointed, Stated, NOTE_VALUE, ORACLE_FEE, MERGE_TIP, ORDER_MIN};
use crate::thin_artifacts::{self as pinned, COLLECTORS, FEE_POOL, NOTE, SCRIPTS, SLOT};
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

/// The least each copy of a library of `copies` copies of each script holds for its genesis to be one transaction
/// a block takes, in whole KAS. An output under a covenant id weighs 4 x 10^12 over its value in sompi, and a
/// block takes 500,000: so the least follows the count of scripts (with 70 scripts, 7 KAS for one copy of each
/// and 13 for two). Review 66, F115.
pub fn lib_copy_least(copies: usize) -> u64 {
    // (The copies, the second copy of the citation's script where there is one of each, and a fee pool.)
    let outputs = (copies * SCRIPTS.len() + 2) as u64;
    let sompi = outputs * 4_000_000_000_000 / 450_000;
    (sompi.div_ceil(100_000_000) * 100_000_000).max(SCRIPT_VALUE_MIN)
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
    // A take on a position of two legs is answered with two citations in one transaction, each a copy of its own:
    // a library of one copy of each script has a second of this one.
    if copies == 1 {
        d.genesis_script(script_spk("KpfTCite")?, script_value, 0, true, "KpfTCite");
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
    let mut cites = 0;
    let fee_pool = genesis_fee_pool(tx).map(|of| fee_pool_spk(&of));
    let mut fee_pools = 0;
    for (i, o) in &bound {
        match script_named(&o.script_public_key) {
            Some(n) => {
                seen.insert(n);
                cites += (n == "KpfTCite") as usize;
            }
            None if fee_pool.as_ref() == Some(&o.script_public_key) && fee_pools == 0 => fee_pools = 1,
            None => return invalid(format!("REFUSED: output {i} under the library's id is not one of the family's scripts. Whoever can spend it can take every object of this library")),
        }
    }
    if let Some(missing) = SCRIPTS.into_iter().find(|n| !seen.contains(n)) {
        return invalid(format!("the library has no copy of {missing}: an object in it could not use that operation"));
    }
    if cites < 2 {
        return invalid("the library has one copy of KpfTCite: a take on a position of two legs, answered with two citations, could not be answered in it");
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

// ---------------------------------------------------------------- price notes and their citation

/// The address of a price note (the same in every library; the output is under a library's id).
pub fn note_spk(n: &Note) -> ScriptPublicKey {
    pay_to_script_hash_script(&pinned::kit().note(&n.hash(), &n.owner))
}

/// A copy of the script `name` of the library `lib` as the next input, spent by `entry`.
fn copy_in(d: &mut ODraft, lib: &[u8; 32], script: &LibScript, name: &'static str, entry: &'static str, args: Vec<Arg>) -> Result<(usize, Hash)> {
    if script.s != name || script.id.map(|h| h.as_bytes()) != Some(*lib) {
        return invalid(format!("the script given is not a copy of {name} in this library"));
    }
    let code = script_of(name)?.to_vec();
    let i = d.input(script.op, kaspa_consensus_core::tx::UtxoEntry::new(script.value, pay_to_script_hash_script(&code), script.daa, false, script.id), OPlan::Contract { which: OWhich::T(name), redeem: code, entry, args }, name, 0);
    Ok((i, Hash::from_bytes(*lib)))
}

/// `KpfTNote`: the committee's price for a market and a time written once, as a note under the library's id that any
/// number of transactions cite (`cite`). Input 0 is the payer's first coin, input 1 the script, input 2 the
/// committee's epoch table spent by its attest; output 0 is the note (a citation reads the first output of the
/// transaction it cites), outputs 1 and 2 the script and the epoch table as they go on. What the note holds returns
/// to `owner` (`note_reclaim`).
pub fn note_write(lib: &[u8; 32], script: &LibScript, t: Attested, owner: [u8; 32], p: Payer) -> Result<(OBuilt, Note)> {
    let (epoch_table, att) = t;
    let [coin, more @ ..] = p.funds else {
        return invalid("a note's transaction begins with a coin of its payer");
    };
    let Some(committee) = epoch_table.id.map(|h| h.as_bytes()) else {
        return invalid("the epoch table carries no covenant id");
    };
    let note = Note { committee, market: att.market, t0: att.t0, price: att.p, spread_bps: epoch_table.s.spread_bps, owner };
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    d.key(coin, "funding");
    let (_, lid) = copy_in(&mut d, lib, script, "KpfTNote", "note", vec![iv(2), iv(0), bv(committee.to_vec()), bv(att.market.to_vec()), iv(att.t0), bv(owner.to_vec())])?;
    d.lock(att.t0 as u64);
    let rid = crate::committee_build::attest_input(&mut d, epoch_table, att, att.t0 as u64)?;
    d.out(NOTE_VALUE as u64, note_spk(&note), Some((1, lid)), "price note", None);
    keep(&mut d, script, lid)?;
    d.obj(crate::oracle_build::OObj::C(crate::committee_build::CObj::EpochTable(epoch_table.s)), epoch_table.value, Some((2, rid)), "epoch table");
    d.fund(more, &p.change_to);
    Ok((d.finish()?, note))
}

/// `KpfNote.reclaim`: what a note holds returns to its writer's key (output 0). Anyone sends it, at any time: a
/// citation is of the transaction that wrote the note, not of its output.
pub fn note_reclaim(note: &OLive<Note>, p: Payer) -> Result<OBuilt> {
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    d.contract(note, OWhich::T(NOTE), pinned::kit().note(&note.s.hash(), &note.s.owner), "reclaim", vec![], "price note", 0);
    d.plain(note.value as i64, &note.s.owner, "back to the note's writer");
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// The most steps `KpfTCite` follows in a lane's activity and in the lanes' tree.
pub const CITE_ACTIVITY_MAX: usize = 14;
pub const CITE_LANES_MAX: usize = 12;

/// What a citation is shown: the note, the transaction that wrote it (without its payload and signature scripts),
/// and what leads from it to the commitment of the chain block that accepted it.
#[derive(Clone, Debug)]
pub struct Cited {
    pub note: Note,
    pub id: Hash,
    pub rest: Vec<u8>,
    pub inputs: usize,
    pub payload_digest: Hash,
    pub witness: crate::cite::Witness,
}

impl Cited {
    /// The citation of the note `note` as the transaction `tx` wrote it under the library `lib`, by the witness of
    /// the chain block that accepted `tx`. Refused when `tx` is not a note's writing as the script reads one.
    pub fn of(tx: &kaspa_consensus_core::tx::Transaction, lib: &[u8; 32], note: Note, witness: crate::cite::Witness) -> Result<Cited> {
        let first = tx.outputs.first().ok_or_else(|| Error::Invalid("the transaction has no output".into()))?;
        if tx.version != 1 || first.script_public_key != note_spk(&note) || first.covenant.map(|c| c.covenant_id.as_bytes()) != Some(*lib) {
            return invalid("the transaction's first output is not this note under this library's id");
        }
        if witness.activity_path.len() > CITE_ACTIVITY_MAX || witness.lanes_path.len() > CITE_LANES_MAX {
            return invalid(format!("the block's proof is {} steps in the lane's activity and {} in the lanes' tree: the citation follows {CITE_ACTIVITY_MAX} and {CITE_LANES_MAX} (the note is written again, in another block)", witness.activity_path.len(), witness.lanes_path.len()));
        }
        Ok(Cited { note, id: tx.id(), rest: kaspa_consensus_core::hashing::tx::transaction_v1_rest_preimage(tx), inputs: tx.inputs.len(), payload_digest: kaspa_consensus_core::hashing::tx::payload_digest(&tx.payload), witness })
    }

    /// The commitment the citation leads to: the chain block's, if the citation holds.
    pub fn commitment(&self) -> Hash {
        self.witness.commitment(&self.id, 1)
    }

    fn args(&self) -> Vec<Arg> {
        let pack = |path: &[crate::cite::Step]| -> (Vec<u8>, i64) { (path.iter().flat_map(|(h, _)| h.as_bytes()).collect(), path.iter().enumerate().map(|(i, (_, right))| (*right as i64) << i).sum()) };
        let (w, n) = (&self.witness, &self.note);
        let (act, act_bits) = pack(&w.activity_path);
        let (smt, smt_bits) = pack(&w.lanes_path);
        let h = |x: &Hash| bv(x.as_bytes().to_vec());
        vec![
            h(&w.block), bv(n.committee.to_vec()), bv(n.market.to_vec()), bv(sm8(n.t0).to_vec()), bv(sm8(n.price).to_vec()), bv(sm8(n.spread_bps).to_vec()), bv(n.owner.to_vec()),
            bv(self.rest.clone()), iv(self.inputs as i64), h(&self.payload_digest), iv(w.merge_idx as i64),
            bv(act), iv(act_bits), iv(w.activity_path.len() as i64),
            h(&w.parent_ref), h(&w.lane_key), h(&w.context), iv(w.blue_score as i64),
            bv(smt), iv(smt_bits), iv(w.lanes_path.len() as i64),
            h(&w.inactivity_shortcut), h(&w.payload_and_ctx), h(&w.parent_commitment),
        ]
    }
}

/// A copy of `KpfTCite` as the next input, holding the citation `c`; the caller writes the copy's output at the same
/// index (`cite_out`). Refused here when this thread's chain view (`verify::set_chain_view`) does not give the
/// citation's block the commitment the citation leads to.
pub fn cite_in(d: &mut ODraft, lib: &[u8; 32], script: &LibScript, c: &Cited) -> Result<(usize, Hash)> {
    match crate::verify::chain_commitment(&c.witness.block) {
        None => return invalid("the citation's chain block is not in the chain view: its commitment is read in a node's header first"),
        Some(seen) if seen != c.commitment() => return invalid("the citation does not lead to the commitment of its chain block"),
        _ => {}
    }
    copy_in(d, lib, script, "KpfTCite", "cite", c.args())
}

/// The copy of `KpfTCite` goes on, as the next output (which the caller has brought to the copy's input index).
pub fn cite_out(d: &mut ODraft, script: &LibScript, at: usize, lid: Hash) -> Result<()> {
    let i = d.out(script.value + ORACLE_FEE as u64, script_spk(script.s)?, Some((at as u16, lid)), script.s, None);
    if i != at {
        return invalid("the citing script's output is not at its input's index");
    }
    Ok(())
}

/// `KpfTCite` alone: a transaction that holds a citation and nothing else (what a node charges for one, and whether
/// it agrees). Input 0 and output 0 are the script's copy.
pub fn cite(lib: &[u8; 32], script: &LibScript, c: &Cited, p: Payer) -> Result<OBuilt> {
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let (at, lid) = cite_in(&mut d, lib, script, c)?;
    cite_out(&mut d, script, at, lid)?;
    d.fund(p.funds, &p.change_to);
    d.finish()
}

// ---------------------------------------------------------------- a stated change

/// The arguments of the scripts that are shown a stated change and not its position: the inputs they serve, the age
/// they ask, the object's length, then the statement (its outputs as their hash, or as they are for the settlement).
fn change_head(age: u64, c: &Change, outs_whole: bool) -> Vec<Arg> {
    let a = &c.position;
    vec![
        bv(vec![0; 24]), bv(sm8(age as i64).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.hash().to_vec()), iv(c.kind), iv(c.t0), bv(c.arg().to_vec()),
        bv(if outs_whole { c.outs.bytes() } else { c.outs.hash().to_vec() }), iv(c.bond), bv(c.claimer.to_vec()), iv(a.dispute_daa), iv(a.x.t.late_daa), iv(c.held),
    ]
}

/// `KpfTChange` for a take: the owner of `tk.side` states that the position's entries move to the prices `tk.a` and
/// that it takes `tk.amount` out, and posts the bond. It signs; nothing is computed on the chain. After the
/// position's dispute period anyone applies it (`change_settle`); before, anyone disputes it (`change_dispute`) or,
/// holding citations of the committee's prices for `t0`, answers it at once (`take_answer`).
pub fn take(lib: &[u8; 32], position: &OLive<Position>, script: &LibScript, tk: Take, t0: i64, p: Payer) -> Result<(OBuilt, Change)> {
    let c = position.s.take(tk, t0, position.value as i64)?;
    Ok((change_state(lib, position, script, &c, p)?, c))
}

/// `KpfTChange.state`: the change `c` of the position is stated as it is given (its claimer signs, and the bond is
/// the position's). The script computes nothing: what is stated here against the rule is disputed and refuted.
pub fn change_state(lib: &[u8; 32], position: &OLive<Position>, script: &LibScript, c: &Change, p: Payer) -> Result<OBuilt> {
    let a = &position.s;
    if c.position != *a || c.bond != a.bond() || c.disputer.is_some() || c.kind != crate::thin::KIND_TAKE || c.held != position.value as i64 {
        return invalid("the change is not a statement on this position (its position, its bond or its kind)");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = head(0, a);
    // (A position shows no form: kind 1 is of a position.)
    args.extend([iv(c.kind), iv(c.t0), bv(c.take.said()), bv(c.outs.bytes()), bv(c.claimer.to_vec()), bv(vec![]), Arg::Sig(c.claimer)]);
    let (id, lid) = enter(&mut d, lib, position, TObj::Position(*a), script, "KpfTChange", "state", args, 0)?;
    d.lock(c.t0 as u64);
    d.out(position.value + c.bond as u64, object_spk(lib, &TObj::Change(*c)), Some((0, id)), "stated change", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// `KpfTChangeSettle`: anyone applies a stated change nobody disputed, once it is a dispute period old: its outputs
/// as it stated them (the position as it goes on at output 0, the payments from the end), and the claimer's bond.
/// `next` is the position the change's first output states, for whoever keeps it.
pub fn change_settle(lib: &[u8; 32], change: &OLive<Change>, script: &LibScript, p: Payer) -> Result<OBuilt> {
    let c = &change.s;
    if c.disputer.is_some() {
        return invalid("the change is disputed: it is answered or lapses, not settled");
    }
    if !c.in_time(change.daa) {
        return invalid("the statement landed outside the position's lateness of its time: it is ended as late, not settled");
    }
    let outs = c.outs.list();
    let w = c.position.dispute_daa as u64;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let (id, lid) = enter(&mut d, lib, change, TObj::Change(*c), script, "KpfTChangeSettle", "settle", change_head(w, c, true), w)?;
    if outs[0].key {
        d.payout(outs[0].value, &outs[0].to, "payment 0");
    } else {
        d.out(outs[0].value as u64, pay_to_script_hash_script(&pinned::kit().object(lib, &outs[0].to)), Some((0, id)), "position", None);
    }
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    // From the end: the bond first (the lowest place), then the payments, the first of them last.
    d.last(c.bond, &c.claimer, "the claimer's bond", &p.change_to);
    for o in outs[1..].iter().rev() {
        d.payout(o.value, &o.to, "payment");
    }
    d.finish()
}

/// `KpfTChangeDispute.dispute`: `disputer` asks for the proof of a stated change that landed in time, with a bond as
/// large as the claimer's.
pub fn change_dispute(lib: &[u8; 32], change: &OLive<Change>, script: &LibScript, disputer: [u8; 32], p: Payer) -> Result<(OBuilt, Change)> {
    let c = &change.s;
    if c.disputer.is_some() || !c.in_time(change.daa) || c.kind != crate::thin::KIND_TAKE {
        return invalid("the change is disputed already, or its statement landed outside the position's lateness of its time (it is ended as late), or it is of a kind that is disputed by a point");
    }
    let next = c.disputed(disputer);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = change_head(0, c, false);
    args.push(bv(disputer.to_vec()));
    let (id, lid) = enter(&mut d, lib, change, TObj::Change(*c), script, "KpfTChangeDispute", "dispute", args, 0)?;
    d.out(change.value + c.bond as u64, object_spk(lib, &TObj::Change(next)), Some((0, id)), "disputed change", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, next))
}

/// `KpfTChangeDispute.lapse`: a disputed change nobody answered within the dispute period: the position again, and
/// both bonds back.
pub fn change_lapse(lib: &[u8; 32], change: &OLive<Change>, script: &LibScript, p: Payer) -> Result<(OBuilt, Position)> {
    let c = &change.s;
    let Some(disputer) = c.disputer else {
        return invalid("the change is not disputed");
    };
    let w = c.position.dispute_daa as u64;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = change_head(w, c, false);
    args.push(bv(disputer.to_vec()));
    let (id, lid) = enter(&mut d, lib, change, TObj::Change(*c), script, "KpfTChangeDispute", "lapse", args, w)?;
    d.out(change.value - 2 * c.bond as u64, object_spk(lib, &TObj::Position(c.position)), Some((0, id)), "position", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    d.last(c.bond, &c.claimer, "the claimer's bond", &p.change_to);
    d.payout(c.bond, &disputer, "the disputer's bond");
    Ok((d.finish()?, c.position))
}

/// `KpfTChangeLate`: `ender` ends a statement that landed before its time or more than the position's lateness
/// after it: the position again, half the bond to `ender`, half in the script's copy (review 58, F105).
pub fn change_late(lib: &[u8; 32], change: &OLive<Change>, script: &LibScript, ender: [u8; 32], p: Payer) -> Result<(OBuilt, Position)> {
    let c = &change.s;
    if c.disputer.is_some() || c.in_time(change.daa) {
        return invalid("the statement landed in time (or is disputed): it is settled, disputed or answered, not ended as late");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = change_head(0, c, false);
    args.push(bv(ender.to_vec()));
    let (id, lid) = enter(&mut d, lib, change, TObj::Change(*c), script, "KpfTChangeLate", "late", args, 0)?;
    let half = c.bond / 2;
    d.out(change.value - c.bond as u64, object_spk(lib, &TObj::Position(c.position)), Some((0, id)), "position", None);
    keep_with(&mut d, script, lid, c.bond - half)?;
    d.fund(p.funds, &p.change_to);
    if half > 0 {
        d.last(half, &ender, "half the bond, to who ended the statement", &p.change_to);
    }
    Ok((d.finish()?, c.position))
}

/// `KpfTChangeLate` on a statement of an account or of a shard (`Stated`): anyone ends one that landed before the
/// time it says, or more than its lateness after: the object again, half the bond to `ender`, half in the
/// script's copy.
pub fn stated_late(lib: &[u8; 32], obj: &OLive<Stated>, script: &LibScript, ender: [u8; 32], p: Payer) -> Result<OBuilt> {
    let c = &obj.s;
    if c.in_time(obj.daa) {
        return invalid("the statement landed in time: it is applied, disputed or answered, not ended as late");
    }
    let half = c.bond / 2;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = stated_head(0, c, false);
    args.push(bv(ender.to_vec()));
    let (id, lid) = enter_hash(&mut d, lib, obj, &c.hash(), "statement", script, "KpfTChangeLate", "late", args, 0)?;
    d.out(obj.value - c.bond as u64, hash_spk(lib, &c.position), Some((0, id)), "object", None);
    keep_with(&mut d, script, lid, c.bond - half)?;
    d.fund(p.funds, &p.change_to);
    if half > 0 {
        d.last(half, &ender, "half the bond, to who ended the statement", &p.change_to);
    }
    d.finish()
}

/// `KpfTTake.answer`: a stated take is answered by the committee's prices for its time, read in citations (`cites`: a copy
/// of `KpfTCite` and the citation of each used leg's note, in the legs' order). The verdict is the rule's
/// (`Position::take_within`): within, the take is applied and the claimer has its amount, its bond and, disputed,
/// half of the disputer's; not within, the position again, the disputer's bond back and half of the claimer's to
/// `rkey`. A statement nobody disputed is answered the same way, at once. Returns the verdict and the position that
/// is left. Inputs: the object, the script, the citing copies; the chain view is set first (`verify::set_chain_view`).
pub fn take_answer(lib: &[u8; 32], change: &OLive<Change>, script: &LibScript, cites: &[(&LibScript, &Cited)], rkey: [u8; 32], p: Payer) -> Result<(OBuilt, bool, Position)> {
    take_answer_with(lib, change, script, cites, rkey, p, None)
}

/// As `take_answer`; with `force` the transaction says that verdict whatever the rule gives (for whoever tests that
/// the script refuses it).
#[allow(clippy::too_many_arguments)]
pub fn take_answer_with(lib: &[u8; 32], change: &OLive<Change>, script: &LibScript, cites: &[(&LibScript, &Cited)], rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool, Position)> {
    let c = &change.s;
    let a = &c.position;
    let tk = c.take;
    if c.kind != crate::thin::KIND_TAKE {
        return invalid("the change is not a take on a position");
    }
    if c.disputer.is_none() && !c.in_time(change.daa) {
        return invalid("the statement landed outside the position's lateness of its time: it is ended as late, not answered");
    }
    let two = a.x.legs[1].size > 0;
    if cites.len() != 1 + two as usize {
        return invalid("an answer holds one citation for each leg the position has");
    }
    let mut prices = [0i64; 2];
    for (i, (_, cited)) in cites.iter().enumerate() {
        let (l, n) = (&a.x.legs[i], &cited.note);
        if n.committee != l.committee || n.market != l.market || n.t0 != c.t0 {
            return invalid(format!("citation {} is not of the committee, the market and the time of leg {}", i + 1, i + 1));
        }
        if n.spread_bps * 4 > a.x.t.g_bps {
            return invalid("the committee's spread leaves the position's tolerance g no margin");
        }
        prices[i] = n.price;
    }
    let held = c.held;
    let within = force.unwrap_or_else(|| a.take_within(&tk, &c.claimer, &c.outs, held, prices));
    let disputed = c.disputer.is_some();
    let rkey = if within { if disputed { c.disputer.expect("disputed") } else { c.claimer } } else { rkey };
    let half = c.bond / 2;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, if two { 3 } else { 2 }].iter().flat_map(|w| sm8(*w)).collect();
    let args = vec![
        bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.fixed()), bv(a.moving()), iv(c.t0), bv(tk.said()),
        bv(c.outs.hash().to_vec()), iv(c.bond), iv(c.held), bv(c.claimer.to_vec()), iv(disputed as i64), bv(c.disputer.unwrap_or([0; 32]).to_vec()), bv(rkey.to_vec()), iv(within as i64),
    ];
    let (id, lid) = enter(&mut d, lib, change, TObj::Change(*c), script, "KpfTTake", "answer", args, 0)?;
    let mut copies = vec![];
    for (copy, cited) in cites {
        copies.push((*copy, cite_in(&mut d, lib, copy, cited)?.0));
    }
    let after = if within { a.taken(&tk).unwrap_or(*a) } else { *a };
    let kept = if within { if disputed { c.bond - half } else { 0 } } else { c.bond - half };
    d.out((held - if within { tk.amount } else { 0 }) as u64, object_spk(lib, &TObj::Position(after)), Some((0, id)), "position", None);
    keep_with(&mut d, script, lid, kept)?;
    for (copy, at) in copies {
        cite_out(&mut d, copy, at, lid)?;
    }
    d.fund(p.funds, &p.change_to);
    if within {
        d.last(tk.amount + c.bond + if disputed { half } else { 0 }, &c.claimer, "the claimer: its amount and its bond", &p.change_to);
    } else {
        match c.disputer {
            Some(dk) => d.last(c.bond, &dk, "the disputer's bond", &p.change_to),
            None => d.last(100_000_000, &p.change_to, "filler", &p.change_to),
        };
        d.payout(half, &rkey, "half of the bond lost, to who answered");
    }
    Ok((d.finish()?, within, after))
}

// ---------------------------------------------------------------- accounts of any number of legs

/// The address of an object of the library `lib` whose state hashes to `h`.
pub fn hash_spk(lib: &[u8; 32], h: &[u8; 32]) -> ScriptPublicKey {
    pay_to_script_hash_script(&pinned::kit().object(lib, h))
}

/// Inputs 0 and 1 of an operation on an object known by its state's hash: the object and a copy of the script.
#[allow(clippy::too_many_arguments)]
fn enter_hash<T>(d: &mut ODraft, lib: &[u8; 32], obj: &OLive<T>, h: &[u8; 32], label: &str, script: &LibScript, name: &'static str, entry: &'static str, args: Vec<Arg>, age: u64) -> Result<(Hash, Hash)> {
    let id = obj.id.ok_or_else(|| Error::Invalid(format!("the {label} carries no covenant id")))?;
    d.contract(obj, OWhich::T(SLOT), pinned::kit().object(lib, h), "go", vec![iv(1), iv(0)], label, age);
    let (_, lid) = copy_in(d, lib, script, name, entry, args)?;
    Ok((id, lid))
}

fn stated_head(age: u64, c: &Stated, outs_whole: bool) -> Vec<Arg> {
    vec![
        bv(vec![0; 24]), bv(sm8(age as i64).to_vec()), iv(pinned::kit().object_len() as i64), bv(c.position.to_vec()), iv(c.kind), iv(c.t0), bv(c.arg.to_vec()),
        bv(if outs_whole { c.outs.bytes() } else { c.outs.hash().to_vec() }), iv(c.bond), bv(c.claimer.to_vec()), iv(c.window), iv(c.late), iv(c.held),
    ]
}

/// `KpfTChange.state` on an account: the statement `st` as it is given (its claimer signs).
pub fn account_state(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, st: &Stated, said: &[u8], p: Payer) -> Result<OBuilt> {
    let a = &account.s;
    // (A covered account's own kinds have a bond of a note's least at the least: review 56, F100.)
    // (A sale's bond is no less than a hundredth of what it says it pays.)
    let bond = if crate::covered::c_kind(st.kind) { a.c_bond_for(st.kind, if said.len() >= 8 { crate::state::int8(&said[..8]) } else { 0 }) } else { a.bond() };
    if st.position != a.hash() || st.bond != bond || st.window != a.dispute_daa || st.late != a.t.late_daa {
        return invalid("the statement is not one on this account (its hash, its bond, its dispute period or its lateness)");
    }
    state_raw(lib, account, &a.fixed_hash(), &a.moving(), st, said, if st.kind >= 2 { a.form() } else { vec![] }, script, p)
}

/// `KpfTChange.state` on any object whose state is a fixed part's hash and a moving part: the statement and the form
/// as they are given. Nothing is checked here: what the script refuses is refused when the transaction is run.
#[allow(clippy::too_many_arguments)]
pub fn state_raw<T>(lib: &[u8; 32], obj: &OLive<T>, fixed: &[u8; 32], moving: &[u8], st: &Stated, said: &[u8], form: Vec<u8>, script: &LibScript, p: Payer) -> Result<OBuilt> {
    if crate::committee::blake2b(said) != st.arg {
        return invalid("what is shown as stated is not what the statement's argument is the hash of");
    }
    let age = 0i64;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let args = vec![bv(vec![0; 24]), bv(sm8(age).to_vec()), iv(pinned::kit().object_len() as i64), bv(fixed.to_vec()), bv(moving.to_vec()), iv(st.kind), iv(st.t0), bv(said.to_vec()), bv(st.outs.bytes()), bv(st.claimer.to_vec()), bv(form), Arg::Sig(st.claimer)];
    let (id, lid) = enter_hash(&mut d, lib, obj, &st.position, "object", script, "KpfTChange", "state", args, age as u64)?;
    d.lock(st.t0 as u64);
    d.out(obj.value + st.bond as u64, hash_spk(lib, &st.hash()), Some((0, id)), "stated change", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// `KpfTChangeDispute.dispute` on a statement known by its hashes (the script takes kind 1 only). Returns the hash
/// of the disputed statement's state.
pub fn stated_dispute(lib: &[u8; 32], obj: &OLive<Stated>, script: &LibScript, disputer: [u8; 32], p: Payer) -> Result<(OBuilt, [u8; 32])> {
    let c = &obj.s;
    let next = crate::committee::blake2b(&[&[0x20][..], &c.hash(), &[0x20], &disputer].concat());
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = stated_head(0, c, false);
    args.push(bv(disputer.to_vec()));
    let (id, lid) = enter_hash(&mut d, lib, obj, &c.hash(), "stated change", script, "KpfTChangeDispute", "dispute", args, 0)?;
    d.out(obj.value + c.bond as u64, hash_spk(lib, &next), Some((0, id)), "disputed change", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, next))
}

/// `KpfTChange` for a take on an account: the owner of `side` states the prices `a` (one a leg) and what it takes
/// out, with the trace of that (kept by whoever watches: an answer is shown it).
pub fn account_take(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, side: i64, amount: i64, a: &[i64], t0: i64, p: Payer) -> Result<(OBuilt, Stated, ATake)> {
    let (st, tk) = account.s.take(side, amount, a, t0, account.value as i64)?;
    Ok((account_state(lib, account, script, &st, &tk.said(), p)?, st, tk))
}

/// `KpfTChange` for a step of a liquidation of an account: `liquidator` states that `side` stands below its
/// maintenance at the prices `a` and that the leg `skip` leaves the account, for the fee the rule gives.
#[allow(clippy::too_many_arguments)]
pub fn account_liq(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, side: i64, skip: i64, a: &[i64], t0: i64, liquidator: [u8; 32], p: Payer) -> Result<(OBuilt, Stated, ATake)> {
    let (st, tk) = account.s.liq_step(side, skip, a, t0, account.value as i64, liquidator)?;
    Ok((account_state(lib, account, script, &st, &tk.said(), p)?, st, tk))
}

/// `KpfTKind.wrong`: a take of kind 1 stated on an account, which no script could answer, ends as one that is not
/// within the rule: the account again, the disputer's bond back (`disputer`: who disputed it, if anyone), half of
/// the claimer's to `rkey`.
pub fn kind_wrong<T>(lib: &[u8; 32], obj: &OLive<T>, st: &Stated, disputer: Option<[u8; 32]>, account: &Account, script: &LibScript, rkey: [u8; 32], p: Payer) -> Result<OBuilt> {
    if st.kind != crate::thin::KIND_TAKE || st.position != account.hash() {
        return invalid("the statement is not of kind 1 on this account");
    }
    let h = match disputer {
        None => st.hash(),
        Some(dk) => crate::committee::blake2b(&[&[0x20][..], &st.hash(), &[0x20], &dk].concat()),
    };
    let half = st.bond / 2;
    let bonds = st.bond * (1 + disputer.is_some() as i64);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let args = vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(account.terms()), bv(account.legs_root().to_vec()), bv(account.moving()), iv(st.t0), bv(st.arg.to_vec()), bv(st.outs.hash().to_vec()), iv(st.bond), iv(st.held), bv(st.claimer.to_vec()), iv(disputer.is_some() as i64), bv(disputer.unwrap_or([0; 32]).to_vec()), bv(rkey.to_vec())];
    let (id, lid) = enter_hash(&mut d, lib, obj, &h, "stated change", script, "KpfTKind", "wrong", args, 0)?;
    d.out(obj.value - bonds as u64, hash_spk(lib, &st.position), Some((0, id)), "account", None);
    keep_with(&mut d, script, lid, st.bond - half)?;
    d.fund(p.funds, &p.change_to);
    match disputer {
        Some(dk) => d.last(st.bond, &dk, "the disputer's bond", &p.change_to),
        None => d.last(100_000_000, &p.change_to, "filler", &p.change_to),
    };
    d.payout(half, &rkey, "half of the bond lost, to who showed the account");
    d.finish()
}

/// `KpfTChangeSettle` on a statement known by its hashes: applied once it is a dispute period old.
pub fn stated_settle(lib: &[u8; 32], obj: &OLive<Stated>, script: &LibScript, p: Payer) -> Result<OBuilt> {
    let c = &obj.s;
    if !c.in_time(obj.daa) {
        return invalid("the statement landed outside the lateness of its time: it is ended as late, not settled");
    }
    let outs = c.outs.list();
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    // (It stood a dispute period; or it was written at or after the last time to dispute it, and is applied at once.)
    let age = if (obj.daa as i64) < c.last_dispute() { c.window as u64 } else { 0 };
    let (id, lid) = enter_hash(&mut d, lib, obj, &c.hash(), "stated change", script, "KpfTChangeSettle", "settle", stated_head(age, c, true), age)?;
    if outs[0].key {
        d.payout(outs[0].value, &outs[0].to, "payment 0");
    } else {
        d.out(outs[0].value as u64, hash_spk(lib, &outs[0].to), Some((0, id)), "account", None);
    }
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    d.last(c.bond, &c.claimer, "the claimer's bond", &p.change_to);
    for (i, o) in outs.iter().enumerate().skip(1).rev() {
        if c.outs.is_note(i) {
            d.out(o.value as u64, hash_spk(lib, &o.to), Some((0, id)), "note", None);
        } else {
            d.payout(o.value, &o.to, "payment");
        }
    }
    d.finish()
}

/// `KpfTPoint.dispute`: `disputer` disputes one point of a stated take on an account (0 the end, 2i - 1 row i, 2i
/// the price of row i), with a bond as large as the claimer's.
pub fn point_dispute(lib: &[u8; 32], obj: &OLive<Stated>, script: &LibScript, disputer: [u8; 32], point: i64, p: Payer) -> Result<(OBuilt, Pointed)> {
    let c = &obj.s;
    if !c.in_time(obj.daa) || !(crate::thin::KIND_ACCOUNT_TAKE..=crate::covered::KIND_C_TOK_SOLD).contains(&c.kind) || point < 0 {
        return invalid("the statement is not one on an account that landed in time, or the point is below zero");
    }
    if (obj.daa as i64) >= c.last_dispute() {
        return invalid("the statement was written at or after the last time to dispute it: no dispute of it lands in time");
    }
    let next = Pointed { st: *c, disputer, point };
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = stated_head(0, c, false);
    args.extend([bv(disputer.to_vec()), iv(point)]);
    let (id, lid) = enter_hash(&mut d, lib, obj, &c.hash(), "stated change", script, "KpfTPoint", "dispute", args, 0)?;
    d.out(obj.value + c.bond as u64, hash_spk(lib, &next.hash()), Some((0, id)), "disputed point", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, next))
}

/// `KpfTPoint.dismiss`: a dispute that landed after the last time to dispute its statement is dismissed by anyone,
/// at once: the statement stands again, and of the disputer's bond a quarter is the claimer's and a quarter
/// `rkey`'s, who dismissed it.
pub fn point_dismiss(lib: &[u8; 32], obj: &OLive<Pointed>, script: &LibScript, rkey: [u8; 32], p: Payer) -> Result<OBuilt> {
    let q = &obj.s;
    let c = &q.st;
    if (obj.daa as i64) <= c.last_dispute() {
        return invalid("the dispute landed by the last time to dispute: it is answered, or lapses");
    }
    let (half, quarter) = (c.bond / 2, c.bond / 4);
    let split = quarter >= ANSWER_PAY_MIN;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = stated_head(0, c, false);
    args.extend([bv(q.disputer.to_vec()), iv(q.point), bv(rkey.to_vec())]);
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTPoint", "dismiss", args, 0)?;
    d.out(obj.value - c.bond as u64, hash_spk(lib, &c.again().hash()), Some((0, id)), "stated change", None);
    keep_with(&mut d, script, lid, if split { c.bond - 2 * quarter } else { c.bond - half })?;
    d.fund(p.funds, &p.change_to);
    if split {
        d.last(quarter, &rkey, "a quarter of the bond lost, to who dismissed it", &p.change_to);
        d.payout(quarter, &c.claimer, "a quarter of the bond lost, to the claimer");
    } else {
        d.last(half, &c.claimer, "half of the bond lost, to the claimer", &p.change_to);
    }
    d.finish()
}

/// `KpfTPoint.lapse`: a disputed point nobody answered within the dispute period: the account again, both bonds
/// back (kinds 5 and up: at the claimer's cost). Only of a dispute that landed by the last time to dispute.
pub fn point_lapse(lib: &[u8; 32], obj: &OLive<Pointed>, script: &LibScript, p: Payer) -> Result<OBuilt> {
    let q = &obj.s;
    let c = &q.st;
    if (obj.daa as i64) > c.last_dispute() {
        return invalid("the dispute landed after the last time to dispute: it is dismissed, and lapses nothing");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = stated_head(c.window as u64, c, false);
    args.extend([bv(q.disputer.to_vec()), iv(q.point)]);
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTPoint", "lapse", args, c.window as u64)?;
    d.out(obj.value - 2 * c.bond as u64, hash_spk(lib, &c.position), Some((0, id)), "account", None);
    if c.kind >= crate::thin::KIND_DRAW && c.kind != crate::covered::KIND_C_ENTER {
        // (A point of a draw, of an old entry or of a statement on a covered account that nobody answered: the
        // prices are the claimer's to show. Its bond is lost, half to the disputer. An entry of the new form is
        // two parties' trade: it ends at nobody's cost.)
        let half = c.bond / 2;
        keep_with(&mut d, script, lid, c.bond - half)?;
        d.fund(p.funds, &p.change_to);
        d.last(c.bond + half, &q.disputer, "the disputer's bond, and half of the claimers'", &p.change_to);
        return Ok(d.finish()?);
    }
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    d.last(c.bond, &c.claimer, "the claimer's bond", &p.change_to);
    d.payout(c.bond, &q.disputer, "the disputer's bond");
    Ok(d.finish()?)
}

/// The least quarter of a bond that a right answer pays to who answered (`ends` of the answering scripts): two
/// outputs of less fit in no transaction, and the claimer then has half, in one output (review 50, F77).
pub const ANSWER_PAY_MIN: i64 = 5_000_000;

/// How the answer of a point ends (`ends` of the answering scripts): right, the statement stands again, and of the
/// disputer's bond a quarter is the claimer's and a quarter `rkey`'s, who answered (half stays in the copy); wrong, the account again, the disputer's bond back and half of the claimer's to
/// `rkey`. `more` adds what stands between the script's output and the funding (a citing copy's output).
#[allow(clippy::too_many_arguments)]
fn point_ends(d: &mut ODraft, lib: &[u8; 32], obj: &OLive<Pointed>, script: &LibScript, id: Hash, lid: Hash, right: bool, rkey: [u8; 32], cite: Option<(&LibScript, usize)>, p: Payer) -> Result<()> {
    let q = &obj.s;
    let c = &q.st;
    let half = c.bond / 2;
    // (A quarter each to the claimer and to who answered where a quarter is a payable amount; else half to the
    // claimer, in one output: two outputs of less fit in no transaction.)
    let split = right && c.bond / 4 >= ANSWER_PAY_MIN;
    let quarter = c.bond / 4;
    if right {
        d.out(obj.value - c.bond as u64, hash_spk(lib, &c.again().hash()), Some((0, id)), "stated change", None);
    } else {
        d.out(obj.value - 2 * c.bond as u64, hash_spk(lib, &c.position), Some((0, id)), "account", None);
    }
    keep_with(d, script, lid, if split { c.bond - 2 * quarter } else { c.bond - half })?;
    if let Some((copy, at)) = cite {
        cite_out(d, copy, at, lid)?;
    }
    d.fund(p.funds, &p.change_to);
    if right {
        if split {
            d.last(quarter, &rkey, "a quarter of the bond lost, to who answered", &p.change_to);
            d.payout(quarter, &c.claimer, "a quarter of the bond lost, to the claimer");
        } else {
            d.last(half, &c.claimer, "half of the bond lost, to the claimer", &p.change_to);
        }
    } else {
        d.last(c.bond, &q.disputer, "the disputer's bond", &p.change_to);
        d.payout(half, &rkey, "half of the bond lost, to who answered");
    }
    Ok(())
}

/// The arguments the three answers of a point share after the object's opening.
fn point_args(q: &Pointed, tk: &ATake, at: i64, rkey: [u8; 32], verdict: bool) -> Vec<Arg> {
    point_args_said(q, tk.said(), at, rkey, verdict)
}

fn point_args_said(q: &Pointed, said: Vec<u8>, at: i64, rkey: [u8; 32], verdict: bool) -> Vec<Arg> {
    let c = &q.st;
    vec![iv(c.t0), bv(said), bv(c.outs.hash().to_vec()), iv(c.bond), bv(c.claimer.to_vec()), iv(c.window), iv(c.late), iv(c.held)]
        .into_iter()
        .chain([bv(q.disputer.to_vec()), iv(q.point), iv(at), bv(rkey.to_vec()), iv(verdict as i64)])
        .collect()
}

fn point_open(obj: &OLive<Pointed>, tk: &ATake, at: i64, want: impl Fn(i64) -> bool) -> Result<usize> {
    let q = &obj.s;
    if q.st.arg != tk.arg() || !(crate::thin::KIND_ACCOUNT_TAKE..=crate::thin::KIND_ACCOUNT_LEAVE).contains(&q.st.kind) || !want(at) {
        return invalid("the statement is not the one given, or the point is not one this script answers");
    }
    Ok(((at + 1) / 2) as usize)
}

/// `KpfTStepRow.answer`: the point 2i - 1 of a stated take is answered with the leg the trace has at i. Returns the
/// verdict (the model's, `row_right`; with `force` the one given, for whoever tests that the script refuses it).
#[allow(clippy::too_many_arguments)]
pub fn step_row(lib: &[u8; 32], obj: &OLive<Pointed>, tk: &ATake, leg: &crate::cross::XLeg, script: &LibScript, rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    let i = ((obj.s.point + 1) / 2) as usize;
    let prev = if i >= 2 && i <= tk.rows.len() { tk.rows[i - 2].g } else { [0; 32] };
    step_row_at(lib, obj, tk, obj.s.point, &prev, leg, script, rkey, p, force)
}

/// As `step_row`, of the point `at` (the one disputed, or any other that is answered wrong), the row's chain opened
/// by `prev` and the leg: where `prev` is not the chain of the row before, the row is wrong.
#[allow(clippy::too_many_arguments)]
pub fn step_row_at(lib: &[u8; 32], obj: &OLive<Pointed>, tk: &ATake, at: i64, prev: &[u8; 32], leg: &crate::cross::XLeg, script: &LibScript, rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    let n = tk.rows.len();
    // (A point beyond the trace, odd or even, is answered here: right.)
    let i = point_open(obj, tk, at, |pt| pt >= 1 && (pt % 2 == 1 || pt > 2 * n as i64))?;
    let q = &obj.s;
    let right = if i > n {
        force.unwrap_or(true)
    } else {
        let before = if i == 1 { crate::thin::Row::ZERO } else { tk.rows[i - 2] };
        let leave = (tk.skip == i as i64).then(|| tk.leave.unwrap_or((crate::reserve::Head::EMPTY, 0)));
        // (A row that carries the seed of every chain is wrong whatever chain and leg are shown.)
        match (force, crate::thin::row_judged_of(prev, leg, &before, &tk.rows[i - 1], leave, tk.side)) {
            (Some(v), _) => v,
            (None, Some(v)) => v,
            (None, None) => return invalid("the chain and the leg given do not open the row's chain: the row is answered by neither verdict"),
        }
    };
    if force.is_none() && at != q.point && right {
        return invalid("a point other than the one disputed is answered only where it is wrong");
    }
    let c = &q.st;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(c.position.to_vec()), iv(c.kind)];
    args.extend(point_args(q, tk, at, rkey, right));
    args.extend([bv(prev.to_vec()), bv(crate::thin::leg_bytes(leg))]);
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTStepRow", "answer", args, 0)?;
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, None, p)?;
    Ok((d.finish()?, right))
}

/// `KpfTStepPrice.answer`: the point 2i is answered with the committee's price for the leg's market at the
/// statement's time, in a citation (the chain view is set first).
#[allow(clippy::too_many_arguments)]
pub fn step_price(lib: &[u8; 32], obj: &OLive<Pointed>, account: &Account, tk: &ATake, script: &LibScript, cite: (&LibScript, &Cited), rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    let at = obj.s.point;
    let i = point_open(obj, tk, at, |pt| pt >= 2 && pt % 2 == 0)?;
    let q = &obj.s;
    let c = &q.st;
    if account.hash() != c.position || i > account.legs.len() || i > tk.rows.len() {
        return invalid("the account is not the one the statement is on");
    }
    let (l, n) = (&account.legs[i - 1], &cite.1.note);
    if n.committee != l.committee || n.market != l.market || n.t0 != c.t0 || n.spread_bps * 4 > account.t.g_bps {
        return invalid("the citation is not of the leg's committee and market and the statement's time, or the committee's spread leaves the tolerance no margin");
    }
    // (The row's chain is opened by the account's own chain before the leg: where the trace's is another, wrong.)
    let prev = account.legs[..i - 1].iter().fold([0u8; 32], |g, x| crate::committee::blake2b(&[&g[..], &crate::thin::leg_bytes(x)].concat()));
    let before_g = if i >= 2 { tk.rows[i - 2].g } else { [0; 32] };
    let right = force.unwrap_or_else(|| prev == before_g && crate::thin::price_right(l, &tk.rows[i - 1], n.price, account.t.g_bps));
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(account.terms()), bv(account.legs_root().to_vec()), bv(account.moving()), iv(c.kind)];
    args.extend(point_args(q, tk, at, rkey, right));
    args.extend([bv(prev.to_vec()), bv(crate::thin::leg_bytes(l))]);
    // (The seed of the shard's id: read where the leg is a token leg, whose committee is its shard's.)
    args.push(bv(vec![0u8; 32]));
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTStepPrice", "answer", args, 0)?;
    let (at, _) = cite_in(&mut d, lib, cite.0, cite.1)?;
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, Some((cite.0, at)), p)?;
    Ok((d.finish()?, right))
}

/// `KpfTStepEnd.answer`: the point 0 is answered: whether the last row is the account's and the take keeps the rule.
#[allow(clippy::too_many_arguments)]
pub fn step_end(lib: &[u8; 32], obj: &OLive<Pointed>, account: &Account, tk: &ATake, script: &LibScript, rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    point_open(obj, tk, 0, |pt| pt == 0)?;
    let q = &obj.s;
    let other = q.point != 0;
    let c = &q.st;
    if account.hash() != c.position {
        return invalid("the account is not the one the statement is on");
    }
    let liq = c.kind != crate::thin::KIND_ACCOUNT_TAKE;
    let held = c.held;
    let right = force.unwrap_or_else(|| if liq { account.step_end_right(c.kind, tk, &c.claimer, &c.outs, held) } else { account.end_right(tk, &c.claimer, &c.outs, held) });
    if force.is_none() && other && right {
        return invalid("a point other than the one disputed is answered only where it is wrong");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(account.terms()), bv(account.legs_root().to_vec()), bv(account.moving())];
    if liq {
        args.push(iv(c.kind));
    }
    args.extend(point_args(q, tk, 0, rkey, right));
    // (The end of a take by `KpfTStepEnd`, of a liquidation step by `KpfTStepLiq`: the script given is the kind's.)
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, if liq { "KpfTStepLiq" } else { "KpfTStepEnd" }, "answer", args, 0)?;
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, None, p)?;
    Ok((d.finish()?, right))
}

// ---------------------------------------------------------------- the reserve

/// Through an anchor of the registry: the object of the state hash `h` enters holding `value`, the script goes on,
/// and the anchor is written again.
#[allow(clippy::too_many_arguments)]
fn enters_hash(d: &mut ODraft, lib: &[u8; 32], anchor: &Anchor, script: &LibScript, name: &'static str, entry: &'static str, args: Vec<Arg>, h: &[u8; 32], value: u64, label: &str) -> Result<Hash> {
    let (id, lid) = enter(d, lib, anchor, TObj::Anchor, script, name, entry, args, 0)?;
    d.out(value, hash_spk(lib, h), Some((0, id)), label, None);
    keep(d, script, lid)?;
    d.out(anchor.value, object_spk(lib, &TObj::Anchor), Some((0, id)), "anchor", None);
    Ok(id)
}

fn slot_head() -> Vec<Arg> {
    vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64)]
}

/// What the id of the shard a `KpfTShardOpen` through `anchor` opens is the hash of, beside its committee: the hash
/// of the anchor's outpoint.
pub fn shard_seed(anchor: &Anchor) -> [u8; 32] {
    crate::committee::blake2b(&[&anchor.op.transaction_id.as_bytes()[..], &sm8(anchor.op.index as i64)].concat())
}

/// The id of the shard a `KpfTShardOpen` through `anchor` opens for the committee `committee`, with the terms and
/// periods of `shard`: a leg enters an account under it only if the leg's committee is that one and the account's
/// terms and periods are those.
pub fn shard_id(anchor: &Anchor, committee: &[u8; 32], shard: &crate::reserve::Shard) -> [u8; 32] {
    crate::reserve::shard_sid(&shard_seed(anchor), committee, &shard.terms(), &shard.moving()[93..120])
}

/// `KpfTShardOpen`: `owner` opens a shard of the terms `t` through an anchor, holding `held`.
#[allow(clippy::too_many_arguments)]
pub fn shard_open(lib: &[u8; 32], anchor: &Anchor, script: &LibScript, t: crate::cross::XTerms, committee: [u8; 32], owner: [u8; 32], held: i64, expiry: i64, dispute_daa: i64, bond_bps: i64, p: Payer) -> Result<(OBuilt, crate::reserve::Shard)> {
    let mut shard = crate::reserve::Shard::opened(t, owner, held, expiry, dispute_daa, bond_bps, [0; 32]);
    shard.sid = shard_id(anchor, &committee, &shard);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = slot_head();
    args.extend([bv(shard.terms()), bv(shard.moving()), bv(committee.to_vec()), Arg::Sig(owner)]);
    enters_hash(&mut d, lib, anchor, script, "KpfTShardOpen", "open", args, &shard.hash(), (held + crate::reserve::SHARD_CARRIER) as u64, "shard")?;
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, shard))
}

/// `KpfTOpenC`: the two parties open the covered account `a` (no leg yet; its shard named) through an anchor.
pub fn account_open_c(lib: &[u8; 32], anchor: &Anchor, script: &LibScript, a: &Account, seed: [u8; 32], committee: [u8; 32], p: Payer) -> Result<OBuilt> {
    if a.shard.is_none() || !a.legs.is_empty() || a.a_margin < 0 || a.b_margin < 0 || a.pot() <= 0 {
        return invalid("a covered account is opened with its shard named, no leg, and margins that hold something");
    }
    if a.shard != Some(crate::reserve::shard_sid(&seed, &committee, &a.terms(), &a.periods())) {
        return invalid("a covered account has its shard's terms, lateness, dispute period and bond rate: the shard's id is of them, of its committee and of its opening (the seed)");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = slot_head();
    args.extend([bv(a.terms()), bv(a.moving()), bv(seed.to_vec()), bv(committee.to_vec()), Arg::Sig(a.a_key), Arg::Sig(a.b_key)]);
    enters_hash(&mut d, lib, anchor, script, "KpfTOpenC", "open", args, &a.hash(), a.pot() as u64, "account")?;
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// `KpfTAddC` on any object of a moving part of 171 bytes: `side`'s key adds `delta` to its margin.
#[allow(clippy::too_many_arguments)]
fn add_c<T>(lib: &[u8; 32], obj: &OLive<T>, script: &LibScript, fixed: [u8; 32], moving: Vec<u8>, side: i64, key: [u8; 32], delta: i64, next: [u8; 32], label: &str, p: Payer) -> Result<OBuilt> {
    if delta <= 0 {
        return invalid("what is added is above zero");
    }
    let h = crate::committee::blake2b(&[&[0x20][..], &fixed, &moving].concat());
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    // (A shard is added to at any time, a note's least or more at a time: review 56, F101. An account says the time
    // before which nothing is done on it at will, and the transaction shows an input that landed at or after it:
    // review 57, F104.)
    let shard = moving.get(34..66).is_some_and(|k| k.iter().all(|x| *x == 0));
    if shard && delta < crate::reserve::NOTE_MIN {
        return invalid("a shard is added to by a note's least or more at a time");
    }
    let f = if shard { 0 } else { at_will_input(moving.get(85..93).map(crate::state::int8).unwrap_or(0), obj.daa, p.funds)? };
    let mut args = slot_head();
    args.extend([bv(fixed.to_vec()), bv(moving), iv(side), iv(delta), iv(f), Arg::Sig(key)]);
    let (id, lid) = enter_hash(&mut d, lib, obj, &h, label, script, "KpfTAddC", "add", args, 0)?;
    d.out(obj.value + delta as u64, hash_spk(lib, &next), Some((0, id)), label, None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// `KpfTAddC`: a shard's owner adds `delta` to what it holds.
pub fn shard_fund(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, script: &LibScript, delta: i64, p: Payer) -> Result<(OBuilt, crate::reserve::Shard)> {
    let s = &shard.s;
    let next = crate::reserve::Shard { held: s.held + delta, ..*s };
    Ok((add_c(lib, shard, script, s.fixed_hash(), s.moving(), crate::cross::SIDE_A, s.owner, delta, next.hash(), "shard", p)?, next))
}

/// `KpfTAddC`: a party of a covered account adds `delta` to its margin.
pub fn account_add(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, side: i64, delta: i64, p: Payer) -> Result<(OBuilt, Account)> {
    let a = &account.s;
    let (key, next) = match side {
        crate::cross::SIDE_A => (a.a_key, Account { a_margin: a.a_margin + delta, expiry: a.at_will(account.daa as i64), ..a.clone() }),
        crate::cross::SIDE_B => (a.b_key, Account { b_margin: a.b_margin + delta, ..a.clone() }),
        _ => return invalid("a margin is added by side a (1) or b (2)"),
    };
    if a.shard.is_none() {
        return invalid("this account is not covered: its margin is added to by the script of a position");
    }
    // (A covered account has nothing of a second party's: `KpfTAddC` takes side A alone. Side B is built, for a
    // test to show it refused.)
    Ok((add_c(lib, account, script, a.fixed_hash(), a.moving(), side, key, delta, next.hash(), "account", p)?, next))
}

/// `KpfTAcctEnd`: `side`'s key ends a covered account that has no leg: A's margin to A (output 0), B's as a deposit
/// to the account's shard. Returns the deposit's note.
pub fn acct_end(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, side: i64, p: Payer) -> Result<OBuilt> {
    let a = &account.s;
    let to_a = a.ended()?;
    let key = match side {
        crate::cross::SIDE_A => a.a_key,
        crate::cross::SIDE_B => a.b_key,
        _ => return invalid("an account is ended by side a (1) or b (2)"),
    };
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = slot_head();
    args.extend([bv(a.terms()), bv(a.moving()), iv(side), Arg::Sig(key)]);
    let (id, lid) = enter_hash(&mut d, lib, account, &a.hash(), "account", script, "KpfTAcctEnd", "end", args, 0)?;
    if to_a > 0 {
        d.payout(to_a, &a.a_key, "party a");
        keep(&mut d, script, lid)?;
        d.fund(p.funds, &p.change_to);
    } else {
        d.fund(p.funds, &p.change_to);
        keep(&mut d, script, lid)?;
    }
    let _ = id;
    d.finish()
}

/// `KpfTChange` for a leg that leaves an account by its party's own statement (kind 4): the owner of `side` states
/// the prices `a` and that the leg `skip` leaves. On a covered account the settlement writes the note that it left,
/// and the note of what A is owed where the pot is short.
#[allow(clippy::too_many_arguments)]
pub fn account_leave(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, side: i64, skip: i64, a: &[i64], t0: i64, p: Payer) -> Result<(OBuilt, Stated, ATake, crate::thin::Stepped)> {
    let x = &account.s;
    let claimer = if side == crate::cross::SIDE_A { x.a_key } else { x.b_key };
    let (st, tk, stepped) = x.step(crate::thin::KIND_ACCOUNT_LEAVE, side, skip, a, t0, account.value as i64, claimer)?;
    Ok((account_state(lib, account, script, &st, &tk.said(), p)?, st, tk, stepped))
}

/// `KpfTChange` for a take stated by `claimer`: the side's key, or anyone for side B of a covered account (the
/// amount is then a deposit to the account's shard).
#[allow(clippy::too_many_arguments)]
pub fn account_take_by(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, side: i64, amount: i64, a: &[i64], t0: i64, claimer: [u8; 32], p: Payer) -> Result<(OBuilt, Stated, ATake)> {
    let (st, tk) = account.s.take_by(side, amount, a, t0, account.value as i64, claimer)?;
    Ok((account_state(lib, account, script, &st, &tk.said(), p)?, st, tk))
}

// ---------------------------------------------------------------- a covered account's own statements

/// `KpfTChange.state` on a covered account (`crate::covered`): a take of `amount` (kind 7), a leaving of the leg
/// `skip` or of every leg (8, `skip` -1), or a step of its user's liquidation that names the leg `skip` (9, by
/// `claimer`), at the prices `a`, one a leg.
#[allow(clippy::too_many_arguments)]
pub fn c_state(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, kind: i64, amount: i64, skip: i64, a: &[i64], t0: i64, claimer: [u8; 32], p: Payer) -> Result<(OBuilt, Stated, crate::covered::CSaid, crate::covered::COutcome)> {
    let (st, s, o) = account.s.c_state(kind, amount, skip, a, t0, account.value as i64, claimer)?;
    Ok((account_state(lib, account, script, &st, &s.said(), p)?, st, s, o))
}

fn c_open(obj: &OLive<Pointed>, s: &crate::covered::CSaid) -> Result<()> {
    let c = &obj.s.st;
    if c.arg != s.arg() || !crate::covered::c_kind(c.kind) {
        return invalid("the statement is not the one given, or is none of a covered account's own");
    }
    Ok(())
}

/// `KpfTCRow.answer`: the point `at` (2i - 1: the one disputed, or any other that is answered wrong) of a statement
/// on a covered account, the row's chain opened by `prev` and the leg. Returns the verdict (the model's,
/// `c_row_judged`; with `force` the one given, for whoever tests that the script refuses it).
#[allow(clippy::too_many_arguments)]
pub fn c_row(lib: &[u8; 32], obj: &OLive<Pointed>, s: &crate::covered::CSaid, at: i64, prev: &[u8; 32], leg: &crate::cross::XLeg, script: &LibScript, rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    c_open(obj, s)?;
    let n = s.rows.len();
    if at < 1 || (at % 2 == 0 && at <= 2 * n as i64) {
        return invalid("the point is not a row's, nor beyond the trace");
    }
    let i = ((at + 1) / 2) as usize;
    let q = &obj.s;
    let right = if i > n {
        force.unwrap_or(true)
    } else {
        let before = if i == 1 { crate::thin::Row::ZERO } else { s.rows[i - 2] };
        match (force, crate::covered::c_row_judged_of(obj.s.st.kind, prev, leg, &before, &s.rows[i - 1], i as i64, s)) {
            (Some(v), _) => v,
            (None, Some(v)) => v,
            (None, None) => return invalid("the chain and the leg given do not open the row's chain, or a sum does not fit: the row is answered by neither verdict"),
        }
    };
    if force.is_none() && at != q.point && right {
        return invalid("a point other than the one disputed is answered only where it is wrong");
    }
    let c = &q.st;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(c.position.to_vec()), iv(c.kind)];
    args.extend(point_args_said(q, s.said(), at, rkey, right));
    args.extend([bv(prev.to_vec()), bv(crate::thin::leg_bytes(leg))]);
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTCRow", "answer", args, 0)?;
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, None, p)?;
    Ok((d.finish()?, right))
}

/// `KpfTStepPrice.answer` on a statement of a covered account: the point 2i, with the committee's price for the
/// leg's market at the statement's time, in a citation.
#[allow(clippy::too_many_arguments)]
pub fn c_price(lib: &[u8; 32], obj: &OLive<Pointed>, account: &Account, s: &crate::covered::CSaid, script: &LibScript, cite: (&LibScript, &Cited), rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    c_price_seeded(lib, obj, account, s, script, cite, [0; 32], rkey, p, force)
}

/// As `c_price`, with the seed of the shard's id: the price of a token leg's row is the shard's committee's, and
/// the script opens the shard's id (the hash of the seed, the committee, the terms and the periods) for it.
#[allow(clippy::too_many_arguments)]
pub fn c_price_seeded(lib: &[u8; 32], obj: &OLive<Pointed>, account: &Account, s: &crate::covered::CSaid, script: &LibScript, cite: (&LibScript, &Cited), price_seed: [u8; 32], rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    c_open(obj, s)?;
    let q = &obj.s;
    let c = &q.st;
    let at = q.point;
    let i = (at / 2) as usize;
    if at < 2 || at % 2 != 0 || account.hash() != c.position || i > account.legs.len() || i > s.rows.len() {
        return invalid("the point is not a price's, or the account is not the one the statement is on");
    }
    let (l, n) = (&account.legs[i - 1], &cite.1.note);
    // (A covered account's leg does not say its committee: it is its shard's, whose id the seed opens. 17.2.)
    let committee = account.shard == Some(crate::reserve::shard_sid(&price_seed, &n.committee, &account.terms(), &account.moving()[93..120]));
    if !committee || n.market != l.market || n.t0 != c.t0 || n.spread_bps * 4 > account.t.g_bps {
        return invalid("the citation is not of the leg's committee and market and the statement's time, or the committee's spread leaves the tolerance no margin");
    }
    let prev = account.legs[..i - 1].iter().fold([0u8; 32], |g, x| crate::committee::blake2b(&[&g[..], &crate::thin::leg_bytes(x)].concat()));
    let before_g = if i >= 2 { s.rows[i - 2].g } else { [0; 32] };
    let right = force.unwrap_or_else(|| prev == before_g && crate::thin::price_right(l, &s.rows[i - 1], n.price, account.t.g_bps));
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(account.terms()), bv(account.legs_root().to_vec()), bv(account.moving()), iv(c.kind)];
    args.extend(point_args_said(q, s.said(), at, rkey, right));
    args.extend([bv(prev.to_vec()), bv(crate::thin::leg_bytes(l))]);
    // (The seed of the shard's id: read where the leg is a token leg, whose committee is its shard's.)
    args.push(bv(price_seed.to_vec()));
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTStepPrice", "answer", args, 0)?;
    let (at, _) = cite_in(&mut d, lib, cite.0, cite.1)?;
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, Some((cite.0, at)), p)?;
    Ok((d.finish()?, right))
}

/// `KpfTCEnd.answer`: the point 0 of a statement on a covered account: whether the last row is the account's and
/// the statement keeps its kind's rule.
#[allow(clippy::too_many_arguments)]
pub fn c_end(lib: &[u8; 32], obj: &OLive<Pointed>, account: &Account, s: &crate::covered::CSaid, script: &LibScript, rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    c_open(obj, s)?;
    let q = &obj.s;
    let c = &q.st;
    if account.hash() != c.position {
        return invalid("the account is not the one the statement is on");
    }
    let right = match (force, account.c_end_judged(c.kind, s, &c.claimer, &c.outs, c.held, c.t0)) {
        (Some(v), _) => v,
        (None, Some(v)) => v,
        (None, None) => return invalid("a sum of the end does not fit: it is answered by neither verdict"),
    };
    if force.is_none() && q.point != 0 && right {
        return invalid("a point other than the one disputed is answered only where it is wrong");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    // (The end of a split and of a mark each have a script of their own, which is told no kind; the end of a token
    // leg's leaving and of its sale have one between them, which is.)
    let (name, told) = match c.kind {
        crate::covered::KIND_C_SPLIT => ("KpfTCSplit", false),
        crate::covered::KIND_C_MARK => ("KpfTCMark", false),
        crate::covered::KIND_C_TOK_OUT | crate::covered::KIND_C_TOK_SOLD => ("KpfTCTok", true),
        _ => ("KpfTCEnd", true),
    };
    let mut args = vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(account.terms()), bv(account.legs_root().to_vec()), bv(account.moving())];
    if told {
        args.push(iv(c.kind));
    }
    args.extend(point_args_said(q, s.said(), 0, rkey, right));
    if name == "KpfTCTok" {
        // (The end of a token leg's leaving or sale is shown the leg the statement names.)
        let Some(leg) = account.legs.iter().find(|l| crate::covered::chain_of_one(l) == s.lc) else { return invalid("the statement names no leg of the account: nobody answers its end") };
        args.push(bv(crate::thin::leg_bytes(leg)));
    }
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, name, "answer", args, 0)?;
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, None, p)?;
    Ok((d.finish()?, right))
}

// ---------------------------------------------------------------- an entry into a covered account

/// `KpfTEnterS.state`: the entry of `leg` into a covered account, with `more` coins, at the prices `a` of the legs
/// it has and the time `t0`, stated in a transaction that spends its shard: input 0 the account, input 2 the
/// shard; output 0 the statement, on the account marked, output 2 the shard with the leg in its books. The owner
/// of the shard and the user sign. Returns also the statement, what it says, the marked account, the account it
/// becomes, and the shard after.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn c_enter_state(lib: &[u8; 32], account: &OLive<Account>, shard: &OLive<crate::reserve::Shard>, script: &LibScript, seed: [u8; 32], leg: crate::cross::XLeg, more: i64, a: &[i64], t0: i64, p: Payer) -> Result<(OBuilt, Stated, crate::covered::CEnterSaid, crate::covered::Marked, Account, crate::reserve::Shard)> {
    let s = &shard.s;
    if !s.of_committee(&seed, &leg) {
        return invalid("the leg is not of the shard's committee (the shard's id is the hash of its opening, its committee, and the terms and periods of its accounts)");
    }
    let head = crate::reserve::Head::of(&leg);
    let r = s.row_for(&head).ok_or_else(|| Error::Invalid("the shard has no row for the leg's market: its four are other markets'".into()))?;
    let next = s.entered(&leg, r, t0)?;
    let (st, said, marked, after) = account.s.c_enter(leg, more, r as i64, a, t0, account.value as i64)?;
    let b = c_enter_state_as(lib, account, shard, script, seed, &st, &said, &next, p)?;
    Ok((b, st, said, marked, after, next))
}

/// As `c_enter_state`, the statement, what it says and the shard after as they are given (for whoever tests what
/// the script refuses, and what is answered wrong).
#[allow(clippy::too_many_arguments)]
pub fn c_enter_state_as(lib: &[u8; 32], account: &OLive<Account>, shard: &OLive<crate::reserve::Shard>, script: &LibScript, seed: [u8; 32], st: &Stated, said: &crate::covered::CEnterSaid, next: &crate::reserve::Shard, p: Payer) -> Result<OBuilt> {
    let (a, s) = (&account.s, &shard.s);
    if a.shard != Some(s.sid) || a.b_key != s.owner || said.add < 0 {
        return invalid("the shard is not the account's (its id; its owner the account's second key), or coins are taken where they are added");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    // (An input that landed at or after the entry's time: the funder's first coin (input 3), the shard (2) or the
    // account (0). Review 58, F105.)
    let f = if p.funds.first().is_some_and(|c| c.daa as i64 >= st.t0) {
        3
    } else if shard.daa as i64 >= st.t0 {
        2
    } else if account.daa as i64 >= st.t0 {
        0
    } else {
        return invalid("an entry's time is not ahead of the chain: fund with a coin that landed at or after it");
    };
    // (The shard's committee, shown: the head of the row the leg is counted in, in the shard after.)
    let Some(row) = usize::try_from(said.r).ok().and_then(|r| next.rows.get(r)) else { return invalid("the entry names no row of the shard") };
    let args = vec![
        bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.terms()), bv(a.legs_root().to_vec()), bv(a.moving()), bv(s.rows_bytes()), bv(s.moving()),
        bv(seed.to_vec()), bv(row.head.committee.to_vec()), iv(st.t0), bv(said.said()), bv(st.outs.bytes()), iv(f), Arg::Sig(a.a_key), Arg::Sig(a.b_key),
    ];
    let (id, lid) = enter_hash(&mut d, lib, account, &a.hash(), "account", script, "KpfTEnterS", "state", args, 0)?;
    d.contract(shard, OWhich::T(SLOT), pinned::kit().object(lib, &s.hash()), "go", vec![iv(1), iv(1)], "shard", 0);
    d.lock(st.t0 as u64);
    d.out(account.value + (said.add + st.bond) as u64, hash_spk(lib, &st.hash()), Some((0, id)), "stated entry", None);
    keep(&mut d, script, lid)?;
    d.out(shard.value, hash_spk(lib, &next.hash()), Some((0, id)), "shard", None);
    d.fund(p.funds, &p.change_to);
    d.finish()
}

fn c_enter_open(obj: &OLive<Pointed>, s: &crate::covered::CEnterSaid, marked: Option<&crate::covered::Marked>) -> Result<()> {
    let c = &obj.s.st;
    if c.arg != s.arg() || c.kind != crate::covered::KIND_C_ENTER || marked.is_some_and(|m| m.hash() != c.position) {
        return invalid("the statement is not the entry given, or the marked account is not the one it is on");
    }
    Ok(())
}

/// `KpfTCRow.answer` on a row of an entry (the point `at`, 2i - 1, or any other that is answered wrong; a point
/// beyond the trace is of nothing: right).
#[allow(clippy::too_many_arguments)]
pub fn c_enter_row(lib: &[u8; 32], obj: &OLive<Pointed>, s: &crate::covered::CEnterSaid, at: i64, prev: &[u8; 32], leg: &crate::cross::XLeg, script: &LibScript, rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    c_enter_open(obj, s, None)?;
    let n = s.rows.len();
    if at < 1 || (at % 2 == 0 && at <= 2 * n as i64) {
        return invalid("the point is not a row's, nor beyond the trace");
    }
    let i = ((at + 1) / 2) as usize;
    let q = &obj.s;
    let right = if i > n {
        force.unwrap_or(true)
    } else {
        let before = if i == 1 { crate::thin::Row::ZERO } else { s.rows[i - 2] };
        match (force, crate::covered::c_enter_row_judged(prev, leg, &before, &s.rows[i - 1], i as i64, s)) {
            (Some(v), _) => v,
            (None, Some(v)) => v,
            (None, None) => return invalid("the chain and the leg given do not open the row's chain, or a sum does not fit: the row is answered by neither verdict"),
        }
    };
    if force.is_none() && at != q.point && right {
        return invalid("a point other than the one disputed is answered only where it is wrong");
    }
    let c = &q.st;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(c.position.to_vec()), iv(c.kind)];
    args.extend(point_args_said(q, s.said(), at, rkey, right));
    args.extend([bv(prev.to_vec()), bv(crate::thin::leg_bytes(leg))]);
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTCRow", "answer", args, 0)?;
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, None, p)?;
    Ok((d.finish()?, right))
}

/// `KpfTStepPrice.answer` on a price of an entry's trace (the point 2i), with the committee's price for the leg's
/// market at the statement's time, in a citation. The last row's is the price of the leg that enters.
#[allow(clippy::too_many_arguments)]
pub fn c_enter_price(lib: &[u8; 32], obj: &OLive<Pointed>, marked: &crate::covered::Marked, s: &crate::covered::CEnterSaid, script: &LibScript, cite: (&LibScript, &Cited), price_seed: [u8; 32], rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    c_enter_open(obj, s, Some(marked))?;
    let (q, account) = (&obj.s, &marked.0);
    let c = &q.st;
    let at = q.point;
    let i = (at / 2) as usize;
    if at < 2 || at % 2 != 0 || i > account.legs.len() || i > s.rows.len() {
        return invalid("the point is not a price's");
    }
    let (l, n) = (&account.legs[i - 1], &cite.1.note);
    // (The committee is the shard's, whose id the seed opens: 17.2.)
    if account.shard != Some(crate::reserve::shard_sid(&price_seed, &n.committee, &account.terms(), &marked.moving()[93..120])) || n.market != l.market || n.t0 != c.t0 || n.spread_bps * 4 > account.t.g_bps {
        return invalid("the citation is not of the shard's committee, the leg's market and the statement's time, or the committee's spread leaves the tolerance no margin");
    }
    let prev = crate::reserve::Left::chain(&account.legs[..i - 1]);
    let before_g = if i >= 2 { s.rows[i - 2].g } else { [0; 32] };
    let right = force.unwrap_or_else(|| prev == before_g && crate::thin::price_right(l, &s.rows[i - 1], n.price, account.t.g_bps));
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(account.terms()), bv(account.legs_root().to_vec()), bv(marked.moving()), iv(c.kind)];
    args.extend(point_args_said(q, s.said(), at, rkey, right));
    args.extend([bv(prev.to_vec()), bv(crate::thin::leg_bytes(l))]);
    // (The seed of the shard's id: read where the leg is a token leg, whose committee is its shard's.)
    args.push(bv(price_seed.to_vec()));
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTStepPrice", "answer", args, 0)?;
    let (at, _) = cite_in(&mut d, lib, cite.0, cite.1)?;
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, Some((cite.0, at)), p)?;
    Ok((d.finish()?, right))
}

/// `KpfTCEnter.answer`: the point 0 of an entry: whether the user stands with the leg and the output is the
/// account with it.
#[allow(clippy::too_many_arguments)]
pub fn c_enter_end(lib: &[u8; 32], obj: &OLive<Pointed>, marked: &crate::covered::Marked, s: &crate::covered::CEnterSaid, script: &LibScript, rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    c_enter_open(obj, s, Some(marked))?;
    let q = &obj.s;
    let c = &q.st;
    let judged = Account::c_enter_end(marked, s, &c.claimer, c.held, c.t0).map(|o| o.is_some_and(|(_, outs)| outs == c.outs));
    let right = match (force, judged) {
        (Some(v), _) => v,
        (None, Some(v)) => v,
        (None, None) => return invalid("a sum of the end does not fit: it is answered by neither verdict"),
    };
    if force.is_none() && q.point != 0 && right {
        return invalid("a point other than the one disputed is answered only where it is wrong");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(marked.0.terms()), bv(marked.0.legs_root().to_vec()), bv(marked.moving())];
    args.extend(point_args_said(q, s.said(), 0, rkey, right));
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTCEnter", "answer", args, 0)?;
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, None, p)?;
    Ok((d.finish()?, right))
}

/// `KpfTEnterOff.off`: an entry that ended left the marked account; anyone undoes it. Output 0 the account
/// without the leg, output 2 the note that the leg did not enter; the terms' least fee of a keeper, out of the
/// account's coins, to `rkey`.
pub fn enter_off(lib: &[u8; 32], obj: &OLive<crate::covered::Marked>, script: &LibScript, p: Payer) -> Result<(OBuilt, Account, crate::covered::Unentered)> {
    let m = &obj.s;
    let ((leg, before), (after, fee, note)) = m.leg().zip(m.undone()).ok_or_else(|| Error::Invalid("the marked account has no leg to undo, or its coins are beyond what is counted".into()))?;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let args = vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(m.0.terms()), bv(before.to_vec()), bv(crate::thin::leg_bytes(&leg)), bv(m.moving())];
    let (id, lid) = enter_hash(&mut d, lib, obj, &m.hash(), "marked account", script, "KpfTEnterOff", "off", args, 0)?;
    d.out(obj.value - fee as u64, hash_spk(lib, &after.hash()), Some((0, id)), "account", None);
    keep(&mut d, script, lid)?;
    d.out(note.value() as u64, hash_spk(lib, &note.hash()), Some((0, id)), "note", None);
    // (No output of the fee: the account holds it less, and what the transaction leaves is its funder's. Beside the
    // account and the note a block takes no small payment: review 51, F83.)
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, after, note))
}

// ---------------------------------------------------------------- the notes of a shard held to its requirement

/// The script that applies a note of so many legs: `KpfTLeft` of one, `KpfTLeftAll` of two to six.
pub fn left_script(legs: usize) -> &'static str {
    if legs == 1 {
        "KpfTLeft"
    } else {
        "KpfTLeftAll"
    }
}

/// `KpfTLeft.apply` or `KpfTLeftAll.apply` (`left_script`: the copy given is that script's): the note that legs
/// left a covered account is applied to its shard, whole. Input 0 is the shard, input 2 the note; output 0 the
/// shard without the legs, output 2 the note of the rest where the shard did not hold what was asked; what it paid
/// is the last output, to the user's key.
pub fn left_apply(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::reserve::Left>, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::reserve::Shard, Option<crate::reserve::Rest>)> {
    let (next, at, paid, rest) = shard.s.left_holding(&note.s, note.value as i64)?;
    left_apply_as(lib, shard, note, &at, next, paid, rest, script, p)
}

/// As `left_apply`, with the rows named and the outputs said (for whoever tests what the script refuses).
#[allow(clippy::too_many_arguments)]
pub fn left_apply_as(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::reserve::Left>, at: &[usize], next: crate::reserve::Shard, paid: i64, rest: Option<crate::reserve::Rest>, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::reserve::Shard, Option<crate::reserve::Rest>)> {
    left_apply_of(lib, shard, note, &note.s, note.s.hash(), crate::reserve::NOTE_LEFT, at, next, paid, rest, script, p)
}

/// `KpfTLeft.apply` on the note that a leg did not enter (`KpfTEnterOff` wrote it): the leg is taken out of the
/// shard's books at its entry, and nothing is paid.
pub fn unentered_apply(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::covered::Unentered>, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::reserve::Shard)> {
    let left = note.s.as_left();
    let (next, at, paid, rest) = shard.s.left(&left)?;
    let (b, next, _) = left_apply_of(lib, shard, note, &left, note.s.hash(), crate::covered::NOTE_UNENTERED, &at, next, paid, rest, script, p)?;
    Ok((b, next))
}

/// `KpfTLeftVoid.void`: a note of legs one of which is no leg of the shard's books is spent, the shard as it is
/// (it is applied by nothing, and would end every draw). `k`: the leg that is not in the books (`Shard::gone_at`).
pub fn left_void(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::reserve::Left>, k: usize, script: &LibScript, p: Payer) -> Result<OBuilt> {
    left_void_of(lib, shard, note, &note.s, note.s.hash(), crate::reserve::NOTE_LEFT, k, script, p)
}

/// As `left_void`, of the note that a leg did not enter.
pub fn unentered_void(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::covered::Unentered>, script: &LibScript, p: Payer) -> Result<OBuilt> {
    left_void_of(lib, shard, note, &note.s.as_left(), note.s.hash(), crate::covered::NOTE_UNENTERED, 0, script, p)
}

#[allow(clippy::too_many_arguments)]
fn left_void_of<N>(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<N>, n: &crate::reserve::Left, nhash: [u8; 32], tag: i64, k: usize, script: &LibScript, p: Payer) -> Result<OBuilt> {
    let s = &shard.s;
    if n.sid != s.sid || n.legs.is_empty() || n.legs.len() > 6 || k >= n.legs.len() {
        return invalid("a note to this shard, of one to six legs, and one of them named");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let args = vec![
        bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(s.terms()), bv(s.rows_bytes()), bv(s.moving()),
        bv(n.legs.iter().flat_map(crate::thin::leg_bytes).collect()), iv(k as i64), bv(n.key.to_vec()), iv(n.asked), iv(tag),
    ];
    let (id, lid) = enter_hash(&mut d, lib, shard, &s.hash(), "shard", script, "KpfTLeftVoid", "void", args, 0)?;
    d.contract(note, OWhich::T(SLOT), pinned::kit().object(lib, &nhash), "go", vec![iv(1), iv(1)], "note", 0);
    // (The shard holds what the note held for it: `Shard::voided`. Where the leg named is a leg of the books the
    // shard is written as it is, and the script refuses.)
    let more = crate::reserve::held_for(n.asked, note.value as i64);
    let next = s.voided_holding(n, note.value as i64).unwrap_or(*s);
    d.out(shard.value + more as u64, hash_spk(lib, &next.hash()), Some((0, id)), "shard", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// The applying of a note of legs whose object is `note` (its state's hash `nhash`, its kind `tag`).
#[allow(clippy::too_many_arguments)]
fn left_apply_of<N>(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<N>, n: &crate::reserve::Left, nhash: [u8; 32], tag: i64, at: &[usize], next: crate::reserve::Shard, paid: i64, rest: Option<crate::reserve::Rest>, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::reserve::Shard, Option<crate::reserve::Rest>)> {
    let s = &shard.s;
    if n.legs.is_empty() || at.len() != n.legs.len() {
        return invalid("a note of legs, and a row a leg");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let mut args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(s.terms()), bv(s.rows_bytes()), bv(s.moving())];
    if n.legs.len() == 1 {
        args.extend([bv(crate::thin::leg_bytes(&n.legs[0])), iv(at[0] as i64)]);
    } else {
        args.extend([bv(n.legs.iter().flat_map(crate::thin::leg_bytes).collect()), bv(at.iter().flat_map(|r| sm8(*r as i64)).collect())]);
    }
    args.extend([bv(n.key.to_vec()), iv(n.asked)]);
    if n.legs.len() == 1 {
        args.push(iv(tag));
    }
    let (id, lid) = enter_hash(&mut d, lib, shard, &s.hash(), "shard", script, left_script(n.legs.len()), "apply", args, 0)?;
    d.contract(note, OWhich::T(SLOT), pinned::kit().object(lib, &nhash), "go", vec![iv(1), iv(1)], "note", 0);
    // (Below zero, what is asked is what the account paid: the note holds it, and the shard takes what the note's
    // output holds above a note's least, and no more than the note says.)
    let taken = crate::reserve::held_for(n.asked, note.value as i64);
    let value = (shard.value as i64).checked_sub(paid).and_then(|v| v.checked_add(taken)).filter(|v| *v >= 0).ok_or_else(|| Error::Invalid("what the shard would hold is beyond what is counted".into()))?;
    d.out(value as u64, hash_spk(lib, &next.hash()), Some((0, id)), "shard", None);
    keep(&mut d, script, lid)?;
    if let Some(left) = &rest {
        d.out(left.value() as u64, hash_spk(lib, &left.hash()), Some((0, id)), "note", None);
    }
    d.fund(p.funds, &p.change_to);
    if paid > 0 {
        d.last(paid, &n.key, "what the user is paid", &p.change_to);
    }
    Ok((d.finish()?, next, rest))
}

/// `KpfTMarkNote.apply`: the note that a leg was marked is applied to its shard. Input 0 is the shard, input 2 the
/// note; output 0 the shard, what its legs were entered at moved by what it paid or owes; output 2 the note of the
/// rest where the shard did not hold what was asked; what it paid is the last output, to the user's key.
pub fn mark_apply(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::reserve::MarkNote>, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::reserve::Shard, Option<crate::reserve::Rest>)> {
    let (next, paid, rest) = shard.s.marked(&note.s)?;
    mark_apply_as(lib, shard, note, next, paid, rest, script, p)
}

/// As `mark_apply`, with the outputs said (for whoever tests what the script refuses).
#[allow(clippy::too_many_arguments)]
pub fn mark_apply_as(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::reserve::MarkNote>, next: crate::reserve::Shard, paid: i64, rest: Option<crate::reserve::Rest>, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::reserve::Shard, Option<crate::reserve::Rest>)> {
    let (s, n) = (&shard.s, &note.s);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let args = vec![
        bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(s.terms()), bv(crate::committee::blake2b(&s.rows_bytes()).to_vec()), bv(s.moving()),
        bv(crate::thin::leg_bytes(&n.leg)), iv(n.entry), bv(n.key.to_vec()), iv(n.asked),
    ];
    let (id, lid) = enter_hash(&mut d, lib, shard, &s.hash(), "shard", script, "KpfTMarkNote", "apply", args, 0)?;
    d.contract(note, OWhich::T(SLOT), pinned::kit().object(lib, &n.hash()), "go", vec![iv(1), iv(1)], "note", 0);
    let value = (shard.value as i64).checked_sub(paid).filter(|v| *v >= 0).ok_or_else(|| Error::Invalid("what the shard would hold is beyond what is counted".into()))?;
    d.out(value as u64, hash_spk(lib, &next.hash()), Some((0, id)), "shard", None);
    keep(&mut d, script, lid)?;
    if let Some(left) = &rest {
        d.out(left.value() as u64, hash_spk(lib, &left.hash()), Some((0, id)), "note", None);
    }
    d.fund(p.funds, &p.change_to);
    if paid > 0 {
        d.last(paid, &n.key, "what the user is paid", &p.change_to);
    }
    Ok((d.finish()?, next, rest))
}

/// `KpfTCTok.lapse`: a sale of tokens (kind 15) that stands and that nobody applied, as old as its life
/// (`Stated::sale_life`), is ended by anyone: the account again, half of the bond to `ender`, half in the script's
/// copy. `age`: the age the transaction says of the statement's output (the life, for whoever does not test).
pub fn tok_lapse(lib: &[u8; 32], obj: &OLive<Stated>, script: &LibScript, ender: [u8; 32], age: u64, p: Payer) -> Result<OBuilt> {
    let c = &obj.s;
    if c.kind != crate::covered::KIND_C_TOK_SOLD {
        return invalid("only a sale of tokens is ended as not applied");
    }
    let half = c.bond / 2;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = vec![bv(vec![0; 24]), bv(sm8(age as i64).to_vec()), iv(pinned::kit().object_len() as i64), bv(c.position.to_vec()), iv(c.t0), bv(c.arg.to_vec()), bv(c.outs.hash().to_vec()), iv(c.bond), bv(c.claimer.to_vec()), iv(c.window), iv(c.late), iv(c.held)];
    args.push(bv(ender.to_vec()));
    let (id, lid) = enter_hash(&mut d, lib, obj, &c.hash(), "statement", script, "KpfTCTok", "lapse", args, age)?;
    d.out(obj.value - c.bond as u64, hash_spk(lib, &c.position), Some((0, id)), "object", None);
    keep_with(&mut d, script, lid, c.bond - half)?;
    d.fund(p.funds, &p.change_to);
    if half > 0 {
        d.last(half, &ender, "half the bond, to who ended the sale", &p.change_to);
    }
    d.finish()
}

/// What a vault's output and a token output hold, in KAS: a carrier a block takes beside the others.
pub const VAULT_VALUE: u64 = 100_000_000;

/// The token input of a transaction: a holding of the reference program, its leader (the first input of the
/// token's id), which says the next states. `key`: the owner's key where a key owns it; none where a covenant id
/// does (a vault's presence is the leave).
fn token_in(d: &mut ODraft, tokens: &OLive<crate::kob::TokenState>, next: &[crate::kob::TokenState], key: Option<[u8; 32]>) -> Result<(usize, Hash)> {
    let id = tokens.id.ok_or_else(|| Error::Invalid("the tokens carry no covenant id".into()))?;
    let witness = match key {
        Some(k) => Arg::LeaderSig(k),
        None => Arg::V(ArtifactValue::Bytes(vec![0x00])),
    };
    let args = vec![Arg::V(ArtifactValue::Array(next.iter().map(|s| s.value()).collect())), witness];
    let entry = kaspa_consensus_core::tx::UtxoEntry::new(tokens.value, tokens.s.spk(), tokens.daa, false, Some(id));
    let i = d.input(tokens.op, entry, OPlan::Contract { which: OWhich::Token, redeem: tokens.s.redeem(), entry: "transfer", args }, "tokens", 0);
    Ok((i, id))
}

/// `KpfTAddSpot`: `amount` of the user's tokens `tokens` enter a covered account as a token leg at the discount
/// `disc` (basis points), priced in units of `scale` up to `price_max`. Input 0 is the account, input 2 the
/// tokens, input 3 the funder's first coin (which gives birth to the vault); output 0 the account with the leg,
/// output 2 the vault, output 3 the tokens owned by the vault's id, output 4 the user's token change. The user
/// and the shard's owner sign. Returns also the account after, the vault, the leg and the tokens' state.
#[allow(clippy::too_many_arguments)]
pub fn spot_add(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, tokens: &OLive<crate::kob::TokenState>, amount: i64, disc: i64, scale: i64, price_max: i64, p: Payer) -> Result<(OBuilt, Account, crate::covered::Vault, crate::cross::XLeg, crate::kob::TokenState)> {
    let a = &account.s;
    let rid = account.id.ok_or_else(|| Error::Invalid("the account carries no covenant id".into()))?;
    let tid = tokens.id.ok_or_else(|| Error::Invalid("the tokens carry no covenant id".into()))?;
    let Some(coin) = p.funds.first() else { return invalid("tokens enter with a coin of the funder's: it gives birth to the vault") };
    if amount <= 0 || amount > tokens.s.amount || tokens.s.scheme != crate::kob::SCHEME_P2PK || tokens.s.owner != a.a_key {
        return invalid("the tokens are the user's own (owned by its key), and no fewer than enter");
    }
    let token = crate::covered::TokenOf::reference(tid.as_bytes(), tokens.s.ext.as_bytes());
    let vault = crate::covered::Vault { rid: rid.as_bytes(), token, amount };
    // (The vault's id is of the coin that gives birth to it and of its output, the third: it is known before the
    // leg that names it is written.)
    let born = kaspa_consensus_core::tx::TransactionOutput { value: VAULT_VALUE, script_public_key: hash_spk(lib, &vault.hash()), covenant: None };
    let vid = kaspa_consensus_core::hashing::covenant_id::covenant_id(coin.outpoint, std::iter::once((2u32, &born)));
    let leg = crate::cross::XLeg { market: token.market(), committee: vid.as_bytes(), scale, price_max, size: amount, dir: crate::covered::DIR_SPOT, entry: disc };
    let next = a.spot_added(&leg, &token, account.daa as i64)?;
    // An input that landed at or after the account's time: the funder's coin (input 3), or the account (0).
    let f = if coin.daa as i64 >= a.expiry {
        3
    } else if account.daa as i64 >= a.expiry {
        0
    } else {
        return invalid("the account says a time before which nothing is done on it at will: fund with a coin that landed at or after it");
    };
    let custody = crate::kob::TokenState::covid(amount, vid, tokens.s.ext);
    let change = (tokens.s.amount > amount).then(|| crate::kob::TokenState::p2pk(tokens.s.amount - amount, tokens.s.owner, tokens.s.ext));
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let args = vec![
        bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.terms()), bv(a.legs_root().to_vec()), bv(a.moving()), bv(crate::thin::leg_bytes(&leg)),
        bv(token.id.to_vec()), bv(token.tpl.to_vec()), iv(token.pre), iv(token.suf), bv(token.ext.to_vec()), iv(2), iv(2), iv(3), iv(f), iv(3), Arg::Sig(a.a_key), Arg::Sig(a.b_key),
    ];
    let (id, lid) = enter_hash(&mut d, lib, account, &a.hash(), "account", script, "KpfTAddSpot", "add", args, 0)?;
    let states: Vec<crate::kob::TokenState> = std::iter::once(custody.clone()).chain(change.clone()).collect();
    let (ti, _) = token_in(&mut d, tokens, &states, Some(tokens.s.owner))?;
    d.out(account.value, hash_spk(lib, &next.hash()), Some((0, id)), "account", None);
    keep(&mut d, script, lid)?;
    let vo = d.genesis_script(hash_spk(lib, &vault.hash()), VAULT_VALUE, 3, false, "vault");
    let co = d.out(VAULT_VALUE, custody.spk(), Some((ti as u16, tid)), "the vault's tokens", None);
    if let Some(c) = &change {
        d.out(tokens.value.max(VAULT_VALUE), c.spk(), Some((ti as u16, tid)), "the user's tokens", None);
    }
    if (ti, vo, co) != (2, 2, 3) {
        return invalid("the places of the tokens, the vault and its tokens are not the ones the script is told");
    }
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, next, vault, leg, custody))
}

/// `KpfTVaultPay`: a ticket is paid. Input 0 is the vault, input 2 the ticket, input 3 the vault's tokens; output
/// 0 the tokens, owned by the ticket's key. The vault ends and the ticket is spent. `leg`: the token leg the
/// ticket is of.
pub fn vault_pay(lib: &[u8; 32], vault: &OLive<crate::covered::Vault>, ticket: &OLive<crate::covered::Ticket>, tokens: &OLive<crate::kob::TokenState>, leg: &crate::cross::XLeg, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::kob::TokenState)> {
    let (v, t) = (&vault.s, &ticket.s);
    let vid = vault.id.ok_or_else(|| Error::Invalid("the vault carries no covenant id".into()))?;
    let tid = tokens.id.ok_or_else(|| Error::Invalid("the tokens carry no covenant id".into()))?;
    if crate::covered::chain_of_one(leg) != t.chain || leg.committee != vid.as_bytes() || leg.size != v.amount || ticket.id.map(|x| x.as_bytes()) != Some(v.rid) {
        return invalid("the ticket is of the leg shown, under the id the vault says; the leg names this vault and its amount");
    }
    if tokens.s.amount != v.amount || tokens.s.owner != vid.as_bytes() || tokens.s.scheme != crate::kob::SCHEME_COVID || tid.as_bytes() != v.token.id {
        return invalid("the tokens are the vault's: of its token and amount, owned by its id");
    }
    let paid = crate::kob::TokenState::p2pk(v.amount, t.key, tokens.s.ext);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let k = &v.token;
    let args = vec![
        bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(v.rid.to_vec()), bv(k.id.to_vec()), bv(k.tpl.to_vec()), iv(k.pre), iv(k.suf), bv(k.ext.to_vec()),
        bv(crate::thin::leg_bytes(leg)), bv(t.key.to_vec()), iv(3), iv(0),
    ];
    let (_, lid) = enter_hash(&mut d, lib, vault, &v.hash(), "vault", script, "KpfTVaultPay", "pay", args, 0)?;
    d.contract(ticket, OWhich::T(SLOT), pinned::kit().object(lib, &t.hash()), "go", vec![iv(1), iv(1)], "ticket", 0);
    let (ti, _) = token_in(&mut d, tokens, std::slice::from_ref(&paid), None)?;
    d.out(tokens.value.max(VAULT_VALUE), paid.spk(), Some((ti as u16, tid)), "the tokens, to the ticket's key", None);
    keep(&mut d, script, lid)?;
    if ti != 3 {
        return invalid("the place of the tokens is not the one the script is told");
    }
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, paid))
}

/// `KpfTOwed.pay`: anyone pays `amount` of what a covered account owes its shard. Output 0 is the account, owing
/// that much less; output 2 the note that holds the coins.
pub fn owed_pay(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, amount: i64, p: Payer) -> Result<(OBuilt, Account, crate::reserve::Owed)> {
    let (next, note) = account.s.owed_paid(amount)?;
    let b = owed_pay_as(lib, account, script, amount, &next, &note, p)?;
    Ok((b, next, note))
}

/// As `owed_pay`, with the outputs said (for whoever tests what the script refuses).
pub fn owed_pay_as(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, amount: i64, next: &Account, note: &crate::reserve::Owed, p: Payer) -> Result<OBuilt> {
    let a = &account.s;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let args = vec![bv(vec![0; 24]), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.fixed_hash().to_vec()), bv(a.moving()), iv(amount)];
    let (id, lid) = enter_hash(&mut d, lib, account, &a.hash(), "account", script, "KpfTOwed", "pay", args, 0)?;
    d.out(account.value, hash_spk(lib, &next.hash()), Some((0, id)), "account", None);
    keep(&mut d, script, lid)?;
    d.out(note.value() as u64, hash_spk(lib, &note.hash()), Some((0, id)), "note", None);
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// `KpfTOwed.apply`: a note of what was paid of what an account owed is applied to its shard. Input 0 is the
/// shard, input 2 the note; output 0 the shard, holding that much more.
pub fn owed_apply(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::reserve::Owed>, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::reserve::Shard)> {
    let next = shard.s.paid_in(&note.s, note.value as i64)?;
    Ok((owed_apply_as(lib, shard, note, next, script, p)?, next))
}

/// As `owed_apply`, with the shard after said.
pub fn owed_apply_as(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::reserve::Owed>, next: crate::reserve::Shard, script: &LibScript, p: Payer) -> Result<OBuilt> {
    let (s, n) = (&shard.s, &note.s);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(s.terms()), bv(crate::committee::blake2b(&s.rows_bytes()).to_vec()), bv(s.moving()), iv(n.amount)];
    let (id, lid) = enter_hash(&mut d, lib, shard, &s.hash(), "shard", script, "KpfTOwed", "apply", args, 0)?;
    d.contract(note, OWhich::T(SLOT), pinned::kit().object(lib, &n.hash()), "go", vec![iv(1), iv(1)], "note", 0);
    let more = (next.held - s.held).max(0);
    d.out(shard.value + more as u64, hash_spk(lib, &next.hash()), Some((0, id)), "shard", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// `KpfTLeft.rest`: a note of the rest is paid as far as the shard holds.
pub fn rest_apply(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::reserve::Rest>, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::reserve::Shard, Option<crate::reserve::Rest>)> {
    let (s, n) = (&shard.s, &note.s);
    let (next, paid, rest) = s.rest_paid(n)?;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(s.terms()), bv(s.rows_bytes()), bv(s.moving()), bv(n.key.to_vec()), iv(n.amount)];
    let (id, lid) = enter_hash(&mut d, lib, shard, &s.hash(), "shard", script, "KpfTLeft", "rest", args, 0)?;
    d.contract(note, OWhich::T(SLOT), pinned::kit().object(lib, &n.hash()), "go", vec![iv(1), iv(1)], "note", 0);
    d.out((shard.value as i64 - paid) as u64, hash_spk(lib, &next.hash()), Some((0, id)), "shard", None);
    keep(&mut d, script, lid)?;
    if let Some(left) = &rest {
        d.out(left.value() as u64, hash_spk(lib, &left.hash()), Some((0, id)), "note", None);
    }
    d.fund(p.funds, &p.change_to);
    if paid > 0 {
        d.last(paid, &n.key, "what the user is paid", &p.change_to);
    }
    Ok((d.finish()?, next, rest))
}

// ---------------------------------------------------------------- a leg enters by a statement

// ---------------------------------------------------------------- the draw

/// `KpfTChange` for a draw: the shard's owner states that it draws `amount` at the prices `prices` (one a row).
/// After the dispute period anyone applies it (`stated_settle`); before, anyone disputes its end or the price of a
/// row (`point_dispute`: the points 0 and 1 to 4), or ends it by a note to the shard that is not applied
/// (`draw_note`).
pub fn shard_draw(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, script: &LibScript, amount: i64, prices: [i64; 4], t0: i64, p: Payer) -> Result<(OBuilt, Stated, crate::reserve::Draw)> {
    let s = &shard.s;
    let (st, d) = s.draw(amount, prices, t0, shard.value as i64)?;
    Ok((state_raw(lib, shard, &s.fixed_hash(), &s.moving(), &st, &d.said(), s.form(), script, p)?, st, d))
}

fn draw_open(obj: &OLive<Pointed>, shard: &crate::reserve::Shard, d: &crate::reserve::Draw) -> Result<()> {
    let c = &obj.s.st;
    if (c.kind != crate::thin::KIND_DRAW && c.kind != crate::reserve::KIND_CALL) || c.arg != d.arg() || c.position != shard.hash() {
        return invalid("the statement is not this draw, or this call, on this shard");
    }
    Ok(())
}

/// `KpfTChange.state`: `claimer` (anyone) states a call of the shard at the prices `prices` of the time `t0`.
#[allow(clippy::too_many_arguments)]
pub fn shard_call(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, script: &LibScript, prices: [i64; 4], t0: i64, claimer: [u8; 32], p: Payer) -> Result<(OBuilt, Stated, crate::reserve::Draw)> {
    let s = &shard.s;
    let (st, d) = s.call(prices, t0, shard.value as i64, claimer)?;
    Ok((state_raw(lib, shard, &s.fixed_hash(), &s.moving(), &st, &d.said(), s.form(), script, p)?, st, d))
}

/// The input that shows a covered account's time has come (`KpfTAddC`, `KpfTPass`): one that landed at or after the
/// time before which nothing is done on the account at will. The first of the funder's coins (input 2), or the
/// account itself (input 0). `Err` where neither did: the funder's coin is older than the account's last change
/// and a lateness.
fn at_will_input(time: i64, object_daa: u64, funds: &[KeyUtxo]) -> Result<i64> {
    if funds.first().is_some_and(|c| c.daa as i64 >= time) {
        Ok(2)
    } else if object_daa as i64 >= time {
        Ok(0)
    } else {
        invalid("nothing is done on the account at will before the time it says: fund with a coin that landed at or after it")
    }
}

/// `KpfTPass.pass`: a covered account's user hands its side to the key `to` (docs 15.1): both sign, the taker (the
/// funder here) pays the leaver `paid` in plain coins in the same transaction, and the account's output has stood
/// a lateness. Nothing else of the account changes, and its shard is not shown.
pub fn account_pass(lib: &[u8; 32], account: &OLive<Account>, script: &LibScript, to: [u8; 32], paid: i64, p: Payer) -> Result<(OBuilt, Account)> {
    let a = &account.s;
    let next = a.passed(to, account.daa as i64)?;
    let f = at_will_input(a.expiry, account.daa, p.funds)?;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = slot_head();
    args.extend([bv(a.fixed_hash().to_vec()), bv(a.moving()), bv(to.to_vec()), iv(f), Arg::Sig(a.a_key), Arg::Sig(to)]);
    let (id, lid) = enter_hash(&mut d, lib, account, &a.hash(), "account", script, "KpfTPass", "pass", args, 0)?;
    d.out(account.value, hash_spk(lib, &next.hash()), Some((0, id)), "account", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    if paid > 0 {
        d.payout(paid, &a.a_key, "what the taker pays the leaver");
    }
    Ok((d.finish()?, next))
}

/// `KpfTJoinC.join`: two covered accounts of one user under one shard become one (docs 15.2), by the user's
/// signature. The shard is shown and written again as it is (it is not called), and every object has stood a
/// lateness.
pub fn account_join(lib: &[u8; 32], first: &OLive<Account>, second: &OLive<Account>, shard: &OLive<crate::reserve::Shard>, script: &LibScript, p: Payer) -> Result<(OBuilt, Account)> {
    let (a, b, s) = (&first.s, &second.s, &shard.s);
    let next = a.joined(b, s)?;
    let late = a.t.late_daa.max(0);
    let legs = |x: &Account| -> Vec<u8> { x.legs.iter().flat_map(crate::thin::leg_bytes).collect() };
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 3].iter().flat_map(|w| sm8(*w)).collect();
    let args = vec![
        bv(who), bv(sm8(late).to_vec()), iv(pinned::kit().object_len() as i64), bv(a.terms()), bv(legs(a)), bv(a.moving()), bv(legs(b)), bv(b.moving()), bv(s.fixed_hash().to_vec()), bv(s.moving()), Arg::Sig(a.a_key),
    ];
    let (id, lid) = enter_hash(&mut d, lib, first, &a.hash(), "account", script, "KpfTJoinC", "join", args, late as u64)?;
    d.contract(second, OWhich::T(SLOT), pinned::kit().object(lib, &b.hash()), "go", vec![iv(1), iv(1)], "second account", late as u64);
    d.contract(shard, OWhich::T(SLOT), pinned::kit().object(lib, &s.hash()), "go", vec![iv(1), iv(2)], "shard", late as u64);
    d.out(first.value + second.value, hash_spk(lib, &next.hash()), Some((0, id)), "account", None);
    keep(&mut d, script, lid)?;
    d.out(shard.value, hash_spk(lib, &s.hash()), Some((0, id)), "shard", None);
    d.fund(p.funds, &p.change_to);
    Ok((d.finish()?, next))
}

/// Whether a statement is of a call's time or after, as `KpfTCStep` and `KpfTCallEnter` read it: its own time is at
/// or after the call's; or it stands as it was stated (not disputed, and not standing again) and landed at or
/// after it. (A statement that stands again, or is disputed, is an object no older than the answer or the
/// dispute: it is held to its own time. Review 54, F90.)
pub fn after_call(st: &Stated, disputed: bool, landed: u64, call_time: i64) -> bool {
    st.t0 >= call_time || (!disputed && st.late < crate::thin::LATE_AGAIN && landed as i64 >= call_time)
}

/// What a statement on an object is shown as, standing or disputed: (the form, the hash of the object's state, the
/// bonds it holds, the disputer's key, the point).
fn stood_of(st: &Stated, pointed: Option<([u8; 32], i64)>) -> (i64, [u8; 32], i64, [u8; 32], i64) {
    match pointed {
        None => (1, st.hash(), st.bond, [0; 32], 0),
        Some((disputer, point)) => (2, Pointed { st: *st, disputer, point }.hash(), 2 * st.bond, disputer, point),
    }
}

/// `KpfTCStep.step`: anyone steps the covered account `account` under the call of its shard: every leg leaves at
/// the call's price. `obj` is the account's object, or (`stood`) a statement of the account's own kinds on it,
/// disputed or not, that landed at or after the call's time: it is ended, and its bonds go back. The funder adds
/// the note's least (and has it back where the note is applied), and has what the transaction leaves: the fee.
#[allow(clippy::too_many_arguments)]
pub fn call_step<T>(lib: &[u8; 32], obj: &OLive<T>, account: &Account, stood: Option<(&Stated, Option<([u8; 32], i64)>)>, call: &OLive<crate::reserve::Called>, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::covered::CallStep)> {
    let bonds = stood.map(|(st, pointed)| stood_of(st, pointed).2).unwrap_or(0);
    if let Some((st, _)) = stood {
        if st.position != account.hash() || !crate::covered::c_kind(st.kind) || !after_call(st, stood.is_some_and(|(_, q)| q.is_some()), obj.daa, call.s.time) {
            return invalid("the statement is not one of this account's own kinds of the call's time or after: it stands, and is answered and applied as any");
        }
    }
    let v = (obj.value as i64).checked_sub(bonds).filter(|v| *v >= 0).ok_or_else(|| Error::Invalid("the object holds less than its bonds".into()))?;
    let over = stood.map(|(st, pointed)| (st.bond, st.claimer, pointed.is_some()));
    let step = account.call_step_at(&call.s, v, over, obj.daa as i64).ok_or_else(|| Error::Invalid("no step of this account under this call: another shard's, no leg, or a leg of no row of the call".into()))?;
    Ok((call_step_as(lib, obj, account, stood, call, &step, script, p)?, step))
}

/// As `call_step`, with the outcome given (a test writes one the rule does not make).
#[allow(clippy::too_many_arguments)]
pub fn call_step_as<T>(lib: &[u8; 32], obj: &OLive<T>, account: &Account, stood: Option<(&Stated, Option<([u8; 32], i64)>)>, call: &OLive<crate::reserve::Called>, step: &crate::covered::CallStep, script: &LibScript, p: Payer) -> Result<OBuilt> {
    let (form, h, _, dkey, point) = match stood {
        None => (0, account.hash(), 0, [0; 32], 0),
        Some((st, pointed)) => stood_of(st, pointed),
    };
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let mut args = vec![
        bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(account.terms()), bv(account.legs.iter().flat_map(crate::thin::leg_bytes).collect()),
        bv(step.at.iter().flat_map(|r| sm8(*r as i64)).collect()), bv(account.moving()), iv(call.s.time), bv(call.s.body()), iv(form),
    ];
    match stood {
        Some((st, _)) => args.extend([iv(st.kind), iv(st.t0), bv(st.arg.to_vec()), bv(st.outs.hash().to_vec()), iv(st.bond), bv(st.claimer.to_vec()), iv(st.window), iv(st.late), iv(st.held)]),
        None => args.extend([iv(0), iv(0), bv(vec![0; 32]), bv(vec![0; 32]), iv(0), bv(vec![0; 32]), iv(0), iv(0), iv(0)]),
    }
    args.extend([bv(dkey.to_vec()), iv(point)]);
    let (id, lid) = enter_hash(&mut d, lib, obj, &h, "account", script, "KpfTCStep", "step", args, 0)?;
    d.contract(call, OWhich::T(SLOT), pinned::kit().object(lib, &call.s.hash()), "go", vec![iv(1), iv(1)], "call", 0);
    d.out(call.value, hash_spk(lib, &call.s.hash()), Some((0, id)), "call", None);
    keep(&mut d, script, lid)?;
    d.out((crate::reserve::NOTE_MIN + step.pay) as u64, hash_spk(lib, &step.note.hash()), Some((0, id)), "note", None);
    // (An account that holds token legs goes on with them: it holds its coins, and a KAS at the least, which
    // whoever steps puts in where the coins are less.)
    if let Some(next) = &step.after {
        d.out(next.a_margin.max(crate::reserve::OWING_KEEP) as u64, hash_spk(lib, &next.hash()), Some((0, id)), "account", None);
    }
    d.fund(p.funds, &p.change_to);
    // The payments, the last the user's: the disputer's bond, the claimer's, the user. (No output of the fee: what
    // the transaction leaves is its funder's.)
    let mut pays: Vec<(i64, [u8; 32], &str)> = vec![];
    if step.disputer_bond > 0 {
        pays.push((step.disputer_bond, dkey, "the disputer's bond"));
    }
    if let Some((st, _)) = stood.filter(|_| step.claimer_bond > 0) {
        pays.push((step.claimer_bond, st.claimer, "the claimer's bond"));
    }
    if step.user > 0 {
        pays.push((step.user, account.a_key, "the user"));
    }
    for (i, (v, k, what)) in pays.iter().enumerate() {
        if i == 0 {
            d.last(*v, k, what, &p.change_to);
        } else {
            d.payout(*v, k, what);
        }
    }
    d.finish()
}

/// `KpfTCallPay.claim`: anyone pays a claim its share of what its called shard holds, once no row of the shard has
/// a unit. The claim is spent whole.
pub fn claim_pay(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::reserve::Rest>, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::reserve::Shard, i64)> {
    let (next, paid) = shard.s.claim_paid(&note.s)?;
    Ok((claim_pay_as(lib, shard, note, next, paid, script, p)?, next, paid))
}

/// As `claim_pay`, with the outcome given.
pub fn claim_pay_as(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, note: &OLive<crate::reserve::Rest>, next: crate::reserve::Shard, paid: i64, script: &LibScript, p: Payer) -> Result<OBuilt> {
    let (s, n) = (&shard.s, &note.s);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(s.terms()), bv(s.rows_bytes()), bv(s.moving()), bv(n.key.to_vec()), iv(n.amount)];
    let (id, lid) = enter_hash(&mut d, lib, shard, &s.hash(), "shard", script, "KpfTCallPay", "claim", args, 0)?;
    d.contract(note, OWhich::T(SLOT), pinned::kit().object(lib, &n.hash()), "go", vec![iv(1), iv(1)], "claim", 0);
    let value = (shard.value as i64).checked_sub(paid).filter(|v| *v >= 0).ok_or_else(|| Error::Invalid("the shard's output holds less than is paid".into()))?;
    d.out(value as u64, hash_spk(lib, &next.hash()), Some((0, id)), "shard", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    if paid > 0 {
        d.last(paid, &n.key, "the claim's share", &p.change_to);
    }
    d.finish()
}

/// `KpfTCallPay.off`: anyone ends the call of a shard no row of which has a unit and which owes nothing by claims:
/// the shard not called, and the call spent (what it held returns to the shard's output).
pub fn call_off(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, call: &OLive<crate::reserve::Called>, script: &LibScript, p: Payer) -> Result<(OBuilt, crate::reserve::Shard)> {
    let next = shard.s.call_off(&call.s)?;
    Ok((call_off_as(lib, shard, call, next, script, p)?, next))
}

/// As `call_off`, with the shard that goes on given.
pub fn call_off_as(lib: &[u8; 32], shard: &OLive<crate::reserve::Shard>, call: &OLive<crate::reserve::Called>, next: crate::reserve::Shard, script: &LibScript, p: Payer) -> Result<OBuilt> {
    let s = &shard.s;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let args = vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(s.terms()), bv(s.rows_bytes()), bv(s.moving()), iv(call.s.time), bv(crate::committee::blake2b(&call.s.body()).to_vec())];
    let (id, lid) = enter_hash(&mut d, lib, shard, &s.hash(), "shard", script, "KpfTCallPay", "off", args, 0)?;
    d.contract(call, OWhich::T(SLOT), pinned::kit().object(lib, &call.s.hash()), "go", vec![iv(1), iv(1)], "call", 0);
    d.out(shard.value + call.value, hash_spk(lib, &next.hash()), Some((0, id)), "shard", None);
    keep(&mut d, script, lid)?;
    d.fund(p.funds, &p.change_to);
    d.finish()
}

/// `KpfTCallEnter.end`: anyone ends an entry (kind 10, on the account marked), standing or disputed, that landed
/// at or after the time of the call of its shard: the marked account again, the bonds back.
#[allow(clippy::too_many_arguments)]
pub fn call_enter_end<T>(lib: &[u8; 32], obj: &OLive<T>, st: &Stated, pointed: Option<([u8; 32], i64)>, marked: &crate::covered::Marked, call: &OLive<crate::reserve::Called>, script: &LibScript, p: Payer) -> Result<OBuilt> {
    if st.kind != crate::covered::KIND_C_ENTER || st.position != marked.hash() || marked.0.shard != Some(call.s.sid) || !after_call(st, pointed.is_some(), obj.daa, call.s.time) {
        return invalid("the statement is no entry on this marked account, under this call's shard, of the call's time or after");
    }
    let (form, h, bonds, dkey, point) = stood_of(st, pointed);
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let args = vec![
        bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(marked.0.fixed_hash().to_vec()), bv(marked.moving()), iv(st.t0), bv(st.arg.to_vec()), bv(st.outs.hash().to_vec()),
        iv(st.bond), bv(st.claimer.to_vec()), iv(st.window), iv(st.late), iv(st.held), iv(form), bv(dkey.to_vec()), iv(point), iv(call.s.time), bv(crate::committee::blake2b(&call.s.body()).to_vec()),
    ];
    let (id, lid) = enter_hash(&mut d, lib, obj, &h, "stated entry", script, "KpfTCallEnter", "end", args, 0)?;
    d.contract(call, OWhich::T(SLOT), pinned::kit().object(lib, &call.s.hash()), "go", vec![iv(1), iv(1)], "call", 0);
    d.out(obj.value - bonds as u64, hash_spk(lib, &st.position), Some((0, id)), "marked account", None);
    keep(&mut d, script, lid)?;
    d.out(call.value, hash_spk(lib, &call.s.hash()), Some((0, id)), "call", None);
    d.fund(p.funds, &p.change_to);
    // (A bond of less than a note's least is not asked: it is the funder's.)
    if st.bond >= crate::covered::PAY_MIN {
        match pointed {
            Some((disputer, _)) => {
                d.last(st.bond, &disputer, "the disputer's bond", &p.change_to);
                d.payout(st.bond, &st.claimer, "the claimer's bond");
            }
            None => {
                d.last(st.bond, &st.claimer, "the claimer's bond", &p.change_to);
            }
        }
    }
    d.finish()
}

/// `KpfTCallEnd.answer`: the point 0 of a call is answered: whether the shard's equity, each price moved the
/// tolerance in its favour, is below its lower level, and the outputs are the shard called, the call and the fee.
#[allow(clippy::too_many_arguments)]
pub fn call_end(lib: &[u8; 32], obj: &OLive<Pointed>, shard: &crate::reserve::Shard, dr: &crate::reserve::Draw, script: &LibScript, rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    draw_open(obj, shard, dr)?;
    let q = &obj.s;
    let c = &q.st;
    if c.kind != crate::reserve::KIND_CALL {
        return invalid("the statement is a draw: its end is answered by `shard_end`");
    }
    if force.is_none() && shard.call_end_judged(dr, c.t0, c.held).is_none() {
        return invalid("the shard's rows at these prices are beyond what a script computes: nobody answers this point, and the dispute lapses");
    }
    let right = force.unwrap_or_else(|| shard.call_end_right(dr, &c.claimer, &c.outs, c.held, c.t0));
    if force.is_none() && q.point != 0 && right {
        return invalid("a point other than the one disputed is answered only where it is wrong");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = shard_args(shard, vec![0; 24]);
    args.extend(point_args_said(q, dr.said(), 0, rkey, right));
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTCallEnd", "answer", args, 0)?;
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, None, p)?;
    Ok((d.finish()?, right))
}

fn shard_args(shard: &crate::reserve::Shard, who: Vec<u8>) -> Vec<Arg> {
    vec![bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(shard.terms()), bv(shard.rows_bytes()), bv(shard.moving())]
}

/// `KpfTShardEnd.answer`: the point 0 of a draw is answered: whether the shard holds its requirement at the prices
/// stated once the amount is drawn, and the outputs are that.
#[allow(clippy::too_many_arguments)]
pub fn shard_end(lib: &[u8; 32], obj: &OLive<Pointed>, shard: &crate::reserve::Shard, dr: &crate::reserve::Draw, script: &LibScript, rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    draw_open(obj, shard, dr)?;
    let q = &obj.s;
    let c = &q.st;
    if c.kind != crate::thin::KIND_DRAW {
        return invalid("the statement is a call: its end is answered by `call_end`");
    }
    if force.is_none() && shard.draw_end_judged(dr, &c.claimer, c.t0).is_none() {
        return invalid("the shard's rows at these prices are beyond what a script computes: nobody answers this point, and the dispute lapses");
    }
    let right = force.unwrap_or_else(|| shard.draw_end_right(dr, &c.claimer, &c.outs, c.held, c.t0));
    if force.is_none() && q.point != 0 && right {
        return invalid("a point other than the one disputed is answered only where it is wrong");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let mut args = shard_args(shard, vec![0; 24]);
    args.extend(point_args_said(q, dr.said(), 0, rkey, right));
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTShardEnd", "answer", args, 0)?;
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, None, p)?;
    Ok((d.finish()?, right))
}

/// `KpfTShardPrice.answer`: the point `at` (1 to 4, or beyond) of a draw is answered with the committee's price for
/// the row's market at the statement's time, in a citation; a row with no market and a point beyond the fourth are
/// answered right with none. A row is named by the hash of its kind and its market: the citation is of that market.
#[allow(clippy::too_many_arguments)]
pub fn shard_price(lib: &[u8; 32], obj: &OLive<Pointed>, shard: &crate::reserve::Shard, dr: &crate::reserve::Draw, at: i64, script: &LibScript, cite: Option<(&LibScript, &Cited)>, rkey: [u8; 32], p: Payer, force: Option<bool>) -> Result<(OBuilt, bool)> {
    draw_open(obj, shard, dr)?;
    let q = &obj.s;
    let c = &q.st;
    if at < 1 {
        return invalid("a row's price is a point from 1");
    }
    let row = shard.rows.get(at as usize - 1).filter(|r| r.head.market != [0; 32]);
    // The market the citation is of: the row carries the hash of its kind and of that market's name.
    let mut under = [0u8; 32];
    let right = match (row, cite) {
        (Some(row), Some((_, cited))) => {
            let n = &cited.note;
            under = n.market;
            let named = crate::reserve::futures_market(&n.market) == row.head.market || crate::reserve::option_market(&n.market) == row.head.market;
            if n.committee != row.head.committee || !named || n.t0 != c.t0 || n.spread_bps * 4 > shard.t.g_bps {
                return invalid("the citation is not of the row's committee and market and the statement's time, or the committee's spread leaves the tolerance no margin");
            }
            force.unwrap_or_else(|| shard.draw_price_right(dr, at as usize, n.price))
        }
        (Some(_), None) => return invalid("the row has a market: its price is answered with a citation"),
        (None, Some(_)) => return invalid("the row has no market (or the point is beyond the fourth): it is answered with no citation"),
        (None, None) => force.unwrap_or(true),
    };
    if force.is_none() && at != q.point && right {
        return invalid("a point other than the one disputed is answered only where it is wrong");
    }
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, if cite.is_some() { 2 } else { 0 }, if cite.is_some() { 2 } else { 0 }].iter().flat_map(|w| sm8(*w)).collect();
    let mut args = shard_args(shard, who);
    args.extend(point_args_said(q, dr.said(), at, rkey, right));
    args.push(bv(under.to_vec()));
    // (Of a draw, or of a call: the statement's kind.)
    args.push(iv(c.kind));
    let (id, lid) = enter_hash(&mut d, lib, obj, &q.hash(), "disputed point", script, "KpfTShardPrice", "answer", args, 0)?;
    let cited = match cite {
        Some((copy, cited)) => Some((copy, cite_in(&mut d, lib, copy, cited)?.0)),
        None => None,
    };
    point_ends(&mut d, lib, obj, script, id, lid, right, rkey, cited, p)?;
    Ok((d.finish()?, right))
}

/// As `draw_note`, by a note that legs left a covered account (whatever it asks).
#[allow(clippy::too_many_arguments)]
pub fn draw_note_left<T>(lib: &[u8; 32], obj: &OLive<T>, st: &Stated, pointed: Option<([u8; 32], i64)>, shard: &crate::reserve::Shard, note: &OLive<crate::reserve::Left>, script: &LibScript, rkey: [u8; 32], p: Payer) -> Result<OBuilt> {
    let n = &note.s;
    if n.sid != shard.sid {
        return invalid("the note is to another shard");
    }
    // (The legs are shown, and their chain is the note's: review 54, F87.)
    let legs: Vec<u8> = n.legs.iter().flat_map(crate::thin::leg_bytes).collect();
    draw_note_of(lib, obj, st, pointed, shard, note, n.hash(), (crate::reserve::NOTE_LEFT, legs, n.key, n.asked), script, rkey, p)
}

/// As `draw_note`, by a note that a leg was marked, where it is one the shard would apply (review 60, F108).
#[allow(clippy::too_many_arguments)]
pub fn draw_note_mark<T>(lib: &[u8; 32], obj: &OLive<T>, st: &Stated, pointed: Option<([u8; 32], i64)>, shard: &crate::reserve::Shard, note: &OLive<crate::reserve::MarkNote>, script: &LibScript, rkey: [u8; 32], p: Payer) -> Result<OBuilt> {
    if shard.marked(&note.s).is_err() {
        return invalid("the note is to another shard, or asks other than its leg gained between its two entries: the shard applies no such note, and it ends no draw");
    }
    draw_note_mark_as(lib, obj, st, pointed, shard, note, script, rkey, p)
}

/// As `draw_note_mark`, whatever the note says (for whoever tests what the script refuses).
#[allow(clippy::too_many_arguments)]
pub fn draw_note_mark_as<T>(lib: &[u8; 32], obj: &OLive<T>, st: &Stated, pointed: Option<([u8; 32], i64)>, shard: &crate::reserve::Shard, note: &OLive<crate::reserve::MarkNote>, script: &LibScript, rkey: [u8; 32], p: Payer) -> Result<OBuilt> {
    let n = &note.s;
    let shown = [crate::thin::leg_bytes(&n.leg), sm8(n.entry).to_vec()].concat();
    draw_note_of(lib, obj, st, pointed, shard, note, n.hash(), (crate::reserve::NOTE_MARK, shown, n.key, n.asked), script, rkey, p)
}

/// As `draw_note`, by a note of the rest.
#[allow(clippy::too_many_arguments)]
pub fn draw_note_rest<T>(lib: &[u8; 32], obj: &OLive<T>, st: &Stated, pointed: Option<([u8; 32], i64)>, shard: &crate::reserve::Shard, note: &OLive<crate::reserve::Rest>, script: &LibScript, rkey: [u8; 32], p: Payer) -> Result<OBuilt> {
    let n = &note.s;
    if n.sid != shard.sid {
        return invalid("the note is to another shard");
    }
    if n.amount > shard.owed {
        return invalid("the shard owes less than the note asks: it was never counted, and ends no draw (it is spent for nothing)");
    }
    draw_note_of(lib, obj, st, pointed, shard, note, n.hash(), (crate::reserve::NOTE_REST, vec![], n.key, n.amount), script, rkey, p)
}

/// An entry of a shard that ended and is not undone, as `KpfTDrawEntry` is shown it: the marked account it left,
/// or the note that the leg did not enter. (An entry that stands or is disputed is not shown: it wrote the shard's
/// time, and no draw is stated before it is settled or ended.)
pub enum Pending<'a> {
    Marked(&'a OLive<crate::covered::Marked>),
    Note(&'a OLive<crate::covered::Unentered>),
}

/// `KpfTDrawEntry.end`: a stated draw (disputed at a point or not) is ended by an entry of its shard that is not
/// settled, whenever it landed: the books the draw was stated on have a leg that may yet not have entered. The
/// shard again; what is shown goes on as it is; the claimer's bond is lost, half to `rkey`.
#[allow(clippy::too_many_arguments)]
pub fn draw_entry<T>(lib: &[u8; 32], obj: &OLive<T>, st: &Stated, pointed: Option<([u8; 32], i64)>, shard: &crate::reserve::Shard, pending: Pending, script: &LibScript, rkey: [u8; 32], p: Payer) -> Result<OBuilt> {
    if st.kind != crate::thin::KIND_DRAW || st.position != shard.hash() {
        return invalid("the statement is not a draw on this shard");
    }
    // (The form; the marked account's terms, the chain before its last leg and its moving part; the leg, of the
    // account or of the note; a key; what the object's state is the hash of; its value. What is shown is opened:
    // review 54, F87.)
    let (form, sid, eterms, eroot, emoving, eleg, ekey, nhash, value): (i64, [u8; 32], Vec<u8>, [u8; 32], Vec<u8>, Vec<u8>, [u8; 32], [u8; 32], u64) = match &pending {
        Pending::Marked(m) => {
            let (leg, before) = m.s.leg().ok_or_else(|| Error::Invalid("the marked account has no leg".into()))?;
            (0, m.s.0.shard.unwrap_or([0; 32]), m.s.0.terms(), before, m.s.moving(), crate::thin::leg_bytes(&leg), [0; 32], m.s.hash(), m.value)
        }
        Pending::Note(n) => (3, n.s.sid, vec![0; 90], [0; 32], vec![0; 180], crate::thin::leg_bytes(&n.s.leg), n.s.key, n.s.hash(), n.value),
    };
    if sid != shard.sid {
        return invalid("what is shown is no entry of this shard that ended and is not undone");
    }
    let h = match pointed {
        None => st.hash(),
        Some((disputer, point)) => Pointed { st: *st, disputer, point }.hash(),
    };
    let bonds = st.bond * (1 + pointed.is_some() as i64);
    let half = st.bond / 2;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let args = vec![
        bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(shard.fixed_hash().to_vec()), bv(shard.moving()), iv(st.t0), bv(st.arg.to_vec()), bv(st.outs.hash().to_vec()),
        iv(st.bond), bv(st.claimer.to_vec()), iv(st.window), iv(st.late), iv(st.held), iv(pointed.is_some() as i64), bv(pointed.map(|x| x.0).unwrap_or([0; 32]).to_vec()), iv(pointed.map(|x| x.1).unwrap_or(0)),
        iv(form), bv(eterms), bv(eroot.to_vec()), bv(emoving), bv(eleg), bv(ekey.to_vec()), bv(rkey.to_vec()),
    ];
    let (id, lid) = enter_hash(&mut d, lib, obj, &h, "stated draw", script, "KpfTDrawEntry", "end", args, 0)?;
    match &pending {
        Pending::Marked(m) => d.contract(*m, OWhich::T(SLOT), pinned::kit().object(lib, &nhash), "go", vec![iv(1), iv(1)], "entry", 0),
        Pending::Note(n) => d.contract(*n, OWhich::T(SLOT), pinned::kit().object(lib, &nhash), "go", vec![iv(1), iv(1)], "entry", 0),
    };
    d.out(obj.value - bonds as u64, hash_spk(lib, &st.position), Some((0, id)), "shard", None);
    keep_with(&mut d, script, lid, st.bond - half)?;
    d.out(value, hash_spk(lib, &nhash), Some((0, id)), "entry", None);
    d.fund(p.funds, &p.change_to);
    match pointed {
        Some((disputer, _)) => d.last(st.bond, &disputer, "the disputer's bond", &p.change_to),
        None => d.last(100_000_000, &p.change_to, "filler", &p.change_to),
    };
    d.payout(half, &rkey, "half of the bond lost, to who showed the entry");
    d.finish()
}

/// `KpfTDrawNote.end` with the note given by its hash and by what the script is shown of it (its kind, the 84 bytes
/// where a market stands, two numbers, a key, an amount).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw_note_of<T, N>(lib: &[u8; 32], obj: &OLive<T>, st: &Stated, pointed: Option<([u8; 32], i64)>, shard: &crate::reserve::Shard, note: &OLive<N>, nhash: [u8; 32], shown: (i64, Vec<u8>, [u8; 32], i64), script: &LibScript, rkey: [u8; 32], p: Payer) -> Result<OBuilt> {
    if st.kind != crate::thin::KIND_DRAW || st.position != shard.hash() {
        return invalid("the statement is not a draw on this shard");
    }
    if note.daa >= obj.daa {
        return invalid("the note landed at or after the statement");
    }
    let h = match pointed {
        None => st.hash(),
        Some((disputer, point)) => Pointed { st: *st, disputer, point }.hash(),
    };
    let bonds = st.bond * (1 + pointed.is_some() as i64);
    let half = st.bond / 2;
    let mut d = ODraft::new(p.rate, FeeMode::Relay);
    let who: Vec<u8> = [0i64, 2, 2].iter().flat_map(|w| sm8(*w)).collect();
    let args = vec![
        bv(who), bv(sm8(0).to_vec()), iv(pinned::kit().object_len() as i64), bv(shard.fixed_hash().to_vec()), bv(shard.moving()), iv(st.t0), bv(st.arg.to_vec()), bv(st.outs.hash().to_vec()),
        iv(st.bond), bv(st.claimer.to_vec()), iv(st.window), iv(st.late), iv(st.held), iv(pointed.is_some() as i64), bv(pointed.map(|x| x.0).unwrap_or([0; 32]).to_vec()), iv(pointed.map(|x| x.1).unwrap_or(0)),
        iv(shown.0), bv(shown.1), bv(shown.2.to_vec()), iv(shown.3), bv(rkey.to_vec()),
    ];
    let (id, lid) = enter_hash(&mut d, lib, obj, &h, "stated draw", script, "KpfTDrawNote", "end", args, 0)?;
    d.contract(note, OWhich::T(SLOT), pinned::kit().object(lib, &nhash), "go", vec![iv(1), iv(1)], "note", 0);
    d.out(obj.value - bonds as u64, hash_spk(lib, &st.position), Some((0, id)), "shard", None);
    keep_with(&mut d, script, lid, st.bond - half)?;
    d.out(note.value, hash_spk(lib, &nhash), Some((0, id)), "note", None);
    d.fund(p.funds, &p.change_to);
    match pointed {
        Some((disputer, _)) => d.last(st.bond, &disputer, "the disputer's bond", &p.change_to),
        None => d.last(100_000_000, &p.change_to, "filler", &p.change_to),
    };
    d.payout(half, &rkey, "half of the bond lost, to who showed the note");
    d.finish()
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
