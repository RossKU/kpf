//! Transaction builders of the committee family (docs/DESIGN-COMMITTEE.md) on the pinned artifacts
//! (`crate::committee_artifacts`): every shape of the committee, its epoch tables and exits, the order, the pair and the
//! pending part. Each builder returns an [`OBuilt`] (the oracle family's machinery: unsigned, with a provisional fee)
//! that [`OBuilt::sign`] signs, meters, balances and commits; the result passes [`crate::verify::verify`]. Values are
//! the least the contracts accept (`crate::committee`, `crate::cmarket`).
//!
//! Output indices follow the contracts' conventions: a payout a contract fixes (`payTo`) sits at the paying input's
//! own index or at its mirror `M - 1 - i`; an epoch table continues at its own input index; the committee continues as the
//! first output of its id.

use kaspa_consensus_core::hashing::covenant_id::covenant_id;
use kaspa_consensus_core::tx::{ScriptPublicKey, TransactionOutpoint, TransactionOutput};
use kaspa_consensus_core::Hash;
use kaspa_txscript::pay_to_script_hash_script;
use silverscript_abi::SilAbiArtifact;

use crate::cmarket::*;
use crate::committee::*;
use crate::committee_artifacts as pinned;
use crate::error::{invalid, Error, Result};
use crate::oracle::{maker_of, Makers, VIABLE_OUTPUT_MIN};
use crate::oracle_build::{bv, iv, ODraft, OBuilt, OLive, OObj, OWhich};
use crate::payoff::{owed, q};
use crate::cross::{Cross, XPending, XSettlement, SIDE_A};
use crate::state::{BPS, KIND_CLOSE, KIND_LIQ, KIND_TRIM, LONG, PLAIN_DUST, SHORT};
use crate::tx::{Arg, FeeMode, KeyUtxo};

/// What a new committee holds besides its members' stake.
pub const COMMITTEE_CARRIER: u64 = 100_000_000;
/// 90 days of DAA at 10 per second: an order untouched this long is refundable by anyone (`KpfOpenC.MAX_IDLE`).
pub const ORDER_MAX_IDLE: i64 = 77_760_000;

/// Which committee template an input runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CWhich {
    Committee,
    EpochTable,
    Exit,
    Open,
    Pair,
    Pending,
    Charge,
    Cross,
    PendingX,
}

impl CWhich {
    pub fn base(self) -> &'static SilAbiArtifact {
        match self {
            CWhich::Committee => pinned::committee_base(),
            CWhich::EpochTable => pinned::epoch_table_base(),
            CWhich::Exit => pinned::exit_base(),
            CWhich::Open => pinned::open_base(),
            CWhich::Pair => pinned::pair_base(),
            CWhich::Pending => pinned::pending_base(),
            CWhich::Charge => pinned::charge_base(),
            CWhich::Cross => pinned::cross_base(),
            CWhich::PendingX => pinned::pendx_base(),
        }
    }
}

/// An object of the committee family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CObj {
    Committee(Committee),
    EpochTable(EpochTable),
    Exit(Exit),
    Order(COpen),
    Pair(CPair),
    Pending(CPending),
    Charge(Charge),
    /// A cross-margin account (`KpfCrossC`).
    Cross(Cross),
    /// A pending claim on one (`KpfPendingX`).
    PendingX(XPending),
}

impl CObj {
    pub fn redeem(&self) -> Vec<u8> {
        let k = pinned::kit();
        match self {
            CObj::Committee(r) => k.committee.redeem(&r.state_bytes()),
            CObj::EpochTable(t) => k.epoch_table.redeem(&t.state_bytes()),
            CObj::Exit(e) => k.exit.redeem(&e.state_bytes()),
            CObj::Order(o) => o.redeem(k),
            CObj::Pair(p) => p.redeem(k),
            CObj::Pending(w) => w.redeem(k),
            CObj::Charge(c) => k.charge.redeem(&c.state_bytes()),
            CObj::Cross(x) => x.redeem(pinned::xkit()),
            CObj::PendingX(w) => w.redeem(pinned::xkit()),
        }
    }
    pub fn spk(&self) -> ScriptPublicKey {
        pay_to_script_hash_script(&self.redeem())
    }
    pub fn kind(&self) -> &'static str {
        match self {
            CObj::Committee(_) => "committee",
            CObj::EpochTable(_) => "epoch table",
            CObj::Exit(_) => "exit",
            CObj::Order(_) => "committee order",
            CObj::Pair(_) => "committee pair",
            CObj::Pending(_) => "committee pending part",
            CObj::Charge(_) => "charge",
            CObj::Cross(_) => "cross account",
            CObj::PendingX(_) => "cross pending claim",
        }
    }
    /// Decodes a redeem script of one of the six pinned templates.
    pub fn decode(redeem: &[u8]) -> Result<CObj> {
        let k = pinned::kit();
        if let Ok(st) = k.pair.state_of(redeem) {
            return Ok(CObj::Pair(CPair::decode_state(st)?));
        }
        if let Ok(st) = k.pending.state_of(redeem) {
            return Ok(CObj::Pending(CPending::decode_state(st)?));
        }
        if let Ok(st) = k.open.state_of(redeem) {
            return Ok(CObj::Order(COpen::decode_state(st)?));
        }
        if let Ok(st) = k.committee.state_of(redeem) {
            return Ok(CObj::Committee(Committee::decode_state(st)?));
        }
        if let Ok(st) = k.epoch_table.state_of(redeem) {
            return Ok(CObj::EpochTable(EpochTable::decode_state(st)?));
        }
        if let Ok(st) = k.exit.state_of(redeem) {
            return Ok(CObj::Exit(Exit::decode_state(st)?));
        }
        if let Ok(st) = k.charge.state_of(redeem) {
            return Ok(CObj::Charge(Charge::decode_state(st)?));
        }
        let x = pinned::xkit();
        if let Ok(st) = x.cross.state_of(redeem) {
            return Ok(CObj::Cross(Cross::decode_state(st)?));
        }
        if let Ok(st) = x.pending.state_of(redeem) {
            return Ok(CObj::PendingX(XPending::decode_state(st)?));
        }
        invalid("not an instance of any pinned committee template")
    }
}

fn refuse(e: &'static str) -> Error {
    Error::Invalid(e.to_string())
}

fn id_of<T>(l: &OLive<T>, what: &str) -> Result<Hash> {
    l.id.ok_or_else(|| Error::Invalid(format!("the {what} has no covenant id")))
}

fn draft() -> ODraft {
    ODraft::new(0, FeeMode::Relay)
}

fn start(rate: u64) -> ODraft {
    let _ = draft;
    ODraft::new(rate, FeeMode::Relay)
}

fn tmpl(t: &crate::artifacts::Template) -> [Arg; 2] {
    [bv(&t.prefix), bv(&t.suffix)]
}

// ---------------------------------------------------------------- the committee

fn unbacked(r: &Committee, value: i64) -> Result<()> {
    if r.backed(value) {
        Ok(())
    } else {
        invalid("the committee's value would not cover its table: it did not begin empty, and the contract accepts no join, top-up or leave that leaves a weight unbacked")
    }
}

/// The covenant id of a committee whose lineage began as `genesis` (an empty committee) at output `index` holding `value`,
/// authorized by the input `auth`: what a wallet recomputes to know that every epoch table of the id descends from an empty
/// committee (docs/DESIGN-COMMITTEE.md 2). A covenant id is a hash of its genesis outputs.
pub fn committee_id_of(auth: &TransactionOutpoint, index: u32, value: u64, genesis: &Committee) -> Hash {
    let out = TransactionOutput { value, script_public_key: CObj::Committee(*genesis).spk(), covenant: None };
    covenant_id(*auth, std::iter::once((index, &out)))
}

/// A new, empty committee: a genesis authored by the first funding input, holding [`COMMITTEE_CARRIER`].
pub fn create_committee(p: CommitteeParams, from: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Committee)> {
    p.check().map_err(refuse)?;
    if funder.is_empty() {
        return invalid("a committee's genesis needs a funding input to author it");
    }
    if from < 0 {
        return invalid("the first epoch cannot start before DAA 0");
    }
    let r = Committee::empty(p, from);
    let mut d = start(rate);
    d.fund(funder, change_to);
    d.genesis(OObj::C(CObj::Committee(r)), COMMITTEE_CARRIER, 0, "committee");
    Ok((d.finish()?, r))
}

/// `KpfCommittee.join`: in 0 the committee, then the funding; out 0 the committee, 1 the outbid holder's exit, then change.
pub fn join(committee: &OLive<Committee>, i: usize, key: [u8; 32], amount: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Committee, Option<Exit>)> {
    let k = pinned::kit();
    let id = id_of(committee, "committee")?;
    let (r, exit, delta) = committee.s.join(i, key, amount).map_err(refuse)?;
    unbacked(&r, committee.value as i64 + delta)?;
    let [pre, suf] = tmpl(&k.exit);
    let mut d = start(rate);
    d.contract(committee, OWhich::C(CWhich::Committee), CObj::Committee(committee.s).redeem(), "join", vec![iv(i as i64), bv(&key), iv(amount), pre, suf], "committee", REST_DAA);
    d.obj(OObj::C(CObj::Committee(r)), (committee.value as i64 + delta) as u64, Some((0, id)), "committee");
    if let Some(e) = exit {
        d.obj(OObj::C(CObj::Exit(e)), e.amount as u64, Some((0, id)), "exit of the outbid member");
    }
    d.fund(funder, change_to);
    Ok((d.finish()?, r, exit))
}

/// `KpfCommittee.topUp`.
pub fn top_up(committee: &OLive<Committee>, i: usize, amount: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Committee)> {
    let id = id_of(committee, "committee")?;
    let r = committee.s.top_up(i, amount).map_err(refuse)?;
    unbacked(&r, committee.value as i64 + amount)?;
    let mut d = start(rate);
    d.contract(committee, OWhich::C(CWhich::Committee), CObj::Committee(committee.s).redeem(), "topUp", vec![iv(i as i64), iv(amount)], "committee", REST_DAA);
    d.obj(OObj::C(CObj::Committee(r)), (committee.value as i64 + amount) as u64, Some((0, id)), "committee");
    d.fund(funder, change_to);
    Ok((d.finish()?, r))
}

/// `KpfCommittee.leave` (the member's key signs): out 0 the committee, 1 the exit.
pub fn leave(committee: &OLive<Committee>, i: usize, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Committee, Exit)> {
    let k = pinned::kit();
    let id = id_of(committee, "committee")?;
    let (r, e) = committee.s.leave(i).map_err(refuse)?;
    unbacked(&r, committee.value as i64 - e.amount)?;
    let [pre, suf] = tmpl(&k.exit);
    let mut d = start(rate);
    d.contract(committee, OWhich::C(CWhich::Committee), CObj::Committee(committee.s).redeem(), "leave", vec![iv(i as i64), Arg::Sig(committee.s.members[i].key), pre, suf], "committee", REST_DAA);
    d.obj(OObj::C(CObj::Committee(r)), (committee.value as i64 - e.amount) as u64, Some((0, id)), "committee");
    d.obj(OObj::C(CObj::Exit(e)), e.amount as u64, Some((0, id)), "exit");
    d.fund(funder, change_to);
    Ok((d.finish()?, r, e))
}

/// `KpfCommittee.roll` by `m` epochs: out 0 the committee, then its epoch tables.
pub fn roll(committee: &OLive<Committee>, m: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Committee, EpochTable)> {
    let k = pinned::kit();
    let id = id_of(committee, "committee")?;
    let nf = committee.s.from + m * committee.s.p.epoch;
    let (r, t) = committee.s.roll(m, nf, committee.value as i64).map_err(refuse)?;
    let [pre, suf] = tmpl(&k.epoch_table);
    let mut d = start(rate);
    d.lock(nf as u64);
    d.contract(committee, OWhich::C(CWhich::Committee), CObj::Committee(committee.s).redeem(), "roll", vec![iv(m), pre, suf], "committee", 0);
    d.obj(OObj::C(CObj::Committee(r)), committee.value, Some((0, id)), "committee");
    for _ in 0..committee.s.p.epoch_tables {
        d.obj(OObj::C(CObj::EpochTable(t)), committee.s.p.epoch_table_value as u64, Some((0, id)), "epoch table");
    }
    d.fund(funder, change_to);
    Ok((d.finish()?, r, t))
}

fn burn_spk() -> ScriptPublicKey {
    // P2SH of the script OP_RETURN: nobody can spend it (the BURN constant of KpfCommittee, KpfExit and KpfStake).
    pay_to_script_hash_script(&[0x6a])
}

fn forfeit_args(market: &[u8; 32], a: &SignedPrice, b: &SignedPrice) -> Vec<Arg> {
    let by = |v: Vec<u8>| Arg::V(silverscript_abi::ArtifactValue::Bytes(v));
    vec![by(market.to_vec()), by(crate::state::sm8(a.t0).to_vec()), by(crate::state::sm8(a.price).to_vec()), by(a.sig.to_vec()), by(crate::state::sm8(b.price).to_vec()), by(b.sig.to_vec())]
}

/// `KpfCommittee.forfeit`: member i signed the two signed prices `a` and `b` (one time of `market`, two prices). In 0 the committee,
/// then the funding; out 0 the committee with the member empty, change (with the sender's tenth), the burn last.
pub fn forfeit_member(committee: &OLive<Committee>, i: usize, market: &[u8; 32], a: &SignedPrice, b: &SignedPrice, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Committee)> {
    let id = id_of(committee, "committee")?;
    let (r, w, burn) = committee.s.forfeit(i).map_err(refuse)?;
    if a.key != committee.s.members[i].key || !equivocation(a, b, &id.as_bytes(), market) {
        return invalid("not a proof against this member: it needs two valid signatures of the member's key for one time of one market with two different prices");
    }
    unbacked(&r, committee.value as i64 - w)?;
    let mut args = vec![iv(i as i64)];
    args.extend(forfeit_args(market, a, b));
    let mut d = start(rate);
    d.contract(committee, OWhich::C(CWhich::Committee), CObj::Committee(committee.s).redeem(), "forfeit", args, "committee", 0);
    d.obj(OObj::C(CObj::Committee(r)), (committee.value as i64 - w) as u64, Some((0, id)), "committee");
    d.fund(funder, change_to);
    if burn >= BURN_DUST {
        d.last_script(burn as u64, burn_spk(), "burned", change_to);
    }
    Ok((d.finish()?, r))
}

/// `KpfExit.forfeit`: the exit's key signed the two signed prices. In 0 the exit, then the funding; out: change (with the
/// sender's tenth), the burn last.
pub fn forfeit_exit(exit: &OLive<Exit>, market: &[u8; 32], a: &SignedPrice, b: &SignedPrice, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let id = id_of(exit, "exit")?;
    let burn = exit.s.forfeit().map_err(refuse)?;
    if a.key != exit.s.key || !equivocation(a, b, &id.as_bytes(), market) {
        return invalid("not a proof against this exit's key: it needs two valid signatures of the key for one time of one market with two different prices");
    }
    let mut d = start(rate);
    d.contract(exit, OWhich::C(CWhich::Exit), CObj::Exit(exit.s).redeem(), "forfeit", forfeit_args(market, a, b), "exit", 0);
    // A held exit keeps a tenth under its holds (review C5): it continues as the first output.
    if let Some(x) = exit.s.forfeit_rest().map_err(refuse)? {
        d.obj(OObj::C(CObj::Exit(x)), (exit.value as i64 - burn) as u64, Some((0, id)), "exit");
    }
    d.fund(funder, change_to);
    if burn >= BURN_DUST {
        d.last_script(burn as u64, burn_spk(), "burned", change_to);
    }
    d.finish()
}

// ---------------------------------------------------------------- charges

/// The stake a charge names: a member of the committee, or an exit.
pub enum Staked<'a> {
    Member(&'a OLive<Committee>, usize),
    Exit(&'a OLive<Exit>),
}

impl Staked<'_> {
    fn id(&self) -> Result<Hash> {
        match self {
            Staked::Member(r, _) => id_of(r, "committee"),
            Staked::Exit(x) => id_of(x, "exit"),
        }
    }
    pub fn key(&self) -> Result<[u8; 32]> {
        match self {
            Staked::Member(r, i) if *i < MEMBERS => Ok(r.s.members[*i].key),
            Staked::Member(..) => invalid("no such member"),
            Staked::Exit(x) => Ok(x.s.key),
        }
    }
    pub fn params(&self) -> ChargeParams {
        match self {
            Staked::Member(r, _) => r.s.p.charge,
            Staked::Exit(x) => x.s.charge,
        }
    }
    /// What it holds.
    pub fn stake(&self) -> i64 {
        match self {
            Staked::Member(r, i) => r.s.members.get(*i).map(|s| s.weight).unwrap_or(0),
            Staked::Exit(x) => x.s.amount,
        }
    }
    pub fn hold(&self) -> [u8; 32] {
        match self {
            Staked::Member(r, i) => r.s.holds.get(*i).copied().unwrap_or(NO_HOLD),
            Staked::Exit(x) => x.s.hold,
        }
    }
}

fn price_args(market: &[u8; 32], t: &SignedPrice) -> Vec<Arg> {
    vec![bv(market), bv(&crate::state::sm8(t.t0)), bv(&crate::state::sm8(t.price)), bv(&t.sig)]
}

/// `KpfCommittee.charge` / `KpfExit.charge`: the stake's key signed `signed price` of `market`, and the charge says the price is
/// false. In 0 the committee or the exit, then the funding (whose first input authors the charge's id and which pays the
/// bond); out 0 the committee or the exit holding the charge, 1 the charge, then change. Returns the charge and its id.
pub fn charge(stake: &Staked, market: &[u8; 32], signed_price: &SignedPrice, pay_key: [u8; 32], funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Charge, Hash)> {
    let k = pinned::kit();
    let id = stake.id()?;
    let key = stake.key()?;
    let p = stake.params();
    p.check().map_err(refuse)?;
    if signed_price.key != key || !price_verifies(signed_price, &id.as_bytes(), market) {
        return invalid("the signed price is not signed by this stake's key for this committee and market: nothing to charge");
    }
    let Some(first) = funder.first() else { return invalid("a charge needs a funding input to author its id") };
    let c = Charge::new(id.as_bytes(), key, pay_key, *market, signed_price.t0, signed_price.price, stake.stake(), p);
    let cout = TransactionOutput { value: p.bond as u64, script_public_key: CObj::Charge(c).spk(), covenant: None };
    let cid = covenant_id(first.outpoint, std::iter::once((1u32, &cout)));
    let [pre, suf] = tmpl(&k.charge);
    let mut d = start(rate);
    match stake {
        Staked::Member(r, i) => {
            let r2 = r.s.charged(*i, &cid.as_bytes()).map_err(refuse)?;
            let mut args = price_args(market, signed_price);
            args.extend([iv(*i as i64), bv(&pay_key), iv(1), pre, suf]);
            d.contract(*r, OWhich::C(CWhich::Committee), CObj::Committee(r.s).redeem(), "charge", args, "committee", 0);
            d.obj(OObj::C(CObj::Committee(r2)), r.value, Some((0, id)), "committee");
        }
        Staked::Exit(x) => {
            let x2 = x.s.charged(&cid.as_bytes()).map_err(refuse)?;
            let mut args = price_args(market, signed_price);
            args.extend([bv(&pay_key), iv(1), pre, suf]);
            d.contract(*x, OWhich::C(CWhich::Exit), CObj::Exit(x.s).redeem(), "charge", args, "exit", 0);
            d.obj(OObj::C(CObj::Exit(x2)), x.value, Some((0, id)), "exit");
        }
    }
    d.genesis(OObj::C(CObj::Charge(c)), p.bond as u64, 1, "charge");
    d.fund(funder, change_to);
    let b = d.finish()?;
    if b.tx.outputs[1].covenant.map(|x| x.covenant_id) != Some(cid) {
        return invalid("internal: the charge's id is not the one its stake holds");
    }
    Ok((b, c, cid))
}

/// A free judge stake of `owner` under the judge block `jm` (a committee's `charge.judge`), no covenant id: what contests
/// a charge once it is `ageDaa` old.
pub fn judge_stake(jm: &crate::judge::JudgeMarket, owner: [u8; 32], value: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    jm.p.check().map_err(refuse)?;
    if (value as i64) < jm.p.min_stake {
        return invalid(format!("a stake needs at least {} sompi", jm.p.min_stake));
    }
    let mut d = start(rate);
    d.fund(funder, change_to);
    d.obj(OObj::Stake(crate::judge::Js::free(owner, *jm)), value, None, "stake");
    d.finish()
}

/// `KpfCharge.contest`: in 0 the charge, 1 the opener's free stake (its owner signs; on NO: "the signed price is false"), then
/// the funding (which also funds the dispute's base); out 0 the contested charge, 1 the opener's receipt, 2 the
/// dispute, then change.
pub fn charge_contest(ch: &OLive<Charge>, opener: &OLive<crate::judge::Js>, base: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Charge, crate::judge::Js)> {
    use crate::judge::{Js, NO, STATUS_FREE};
    let ok = crate::oracle_artifacts::kit();
    let id = id_of(ch, "charge")?;
    let born = ch.daa as i64;
    let c2 = ch.s.contested(born).map_err(refuse)?;
    let jm = ch.s.p.judge;
    if opener.s.m != jm || opener.s.status != STATUS_FREE {
        return invalid("the opener is not a free stake of this committee's judge");
    }
    if opener.daa as i64 + jm.p.age > born {
        return invalid(format!("the opener's stake must be accepted {} DAA before the charge (at {born})", jm.p.age));
    }
    if (opener.value as i64) < jm.p.min_stake {
        return invalid("the opener's stake is below the judge's minimum");
    }
    let spk = CObj::Charge(c2).spk();
    let mut h = spk.version().to_be_bytes().to_vec();
    h.extend_from_slice(spk.script());
    let dsp = Js::opened(jm, *blake3::hash(&h).as_bytes(), born, opener.value as i64);
    let receipt = Js::receipt(opener.s.owner, jm, NO, opener.value as i64, 0, born);
    let mut d = start(rate);
    d.contract(ch, OWhich::C(CWhich::Charge), CObj::Charge(ch.s).redeem(), "contest", vec![crate::oracle_build::b8(1), crate::oracle_build::b8(2)], "charge", 0);
    d.contract(opener, OWhich::Stake, ok.stake.redeem(&opener.s.state_bytes()), "open", vec![crate::oracle_build::b8(0), crate::oracle_build::b8(0), crate::oracle_build::b8(2), Arg::Sig(opener.s.owner)], "opener's stake", 0);
    d.obj(OObj::C(CObj::Charge(c2)), ch.value, Some((0, id)), "contested charge");
    d.obj(OObj::Stake(receipt), opener.value, Some((0, id)), "receipt");
    // (A base below 0.5 KAS makes the dispute's output most of a block's storage mass: review C5, C5-N3.)
    d.obj(OObj::Stake(dsp.clone()), base.max(50_000_000), Some((0, id)), "dispute");
    d.fund(funder, change_to);
    Ok((d.finish()?, c2, dsp))
}

/// The judge's verdict delivered to the charge: in 0 the dispute (`resolve`), 1 the contested charge (`decide`), then
/// the funding; out 0 the pool, 1 the decided charge, then change. The lock time is the round's end.
pub fn charge_decide(dispute: &OLive<crate::judge::Js>, ch: &OLive<Charge>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Charge, crate::judge::Js)> {
    use crate::judge::{Js, STATUS_DISPUTE, STATUS_RESOLVED, YES};
    let ok = crate::oracle_artifacts::kit();
    let id = id_of(ch, "charge")?;
    if dispute.id != ch.id || dispute.s.status != STATUS_DISPUTE {
        return invalid("the dispute is not this charge's");
    }
    let d0 = &dispute.s;
    let st = if d0.start == -1 { dispute.daa as i64 } else { d0.start };
    let (wl, ll) = if d0.side == YES { (d0.yes_u, d0.no) } else { (d0.no_u, d0.yes) };
    let pool = Js { status: STATUS_RESOLVED, start: st, w_left: wl, l_left: ll, base: dispute.value as i64, ..d0.clone() };
    let c2 = ch.s.decided(d0.side == YES, ch.daa as i64, d0.no).map_err(refuse)?;
    let mut d = start(rate);
    d.lock(crate::oracle_build::resolve_from(dispute));
    d.contract(dispute, OWhich::Stake, ok.stake.redeem(&d0.state_bytes()), "resolve", vec![crate::oracle_build::b8(1)], "dispute", 0);
    d.contract(ch, OWhich::C(CWhich::Charge), CObj::Charge(ch.s).redeem(), "decide", vec![crate::oracle_build::b8(0)], "contested charge", 0);
    d.obj(OObj::Stake(pool.clone()), dispute.value, Some((0, id)), "pool");
    d.obj(OObj::C(CObj::Charge(c2)), ch.value, Some((1, id)), "decided charge");
    d.fund(funder, change_to);
    Ok((d.finish()?, c2, pool))
}

/// What a settled charge did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settled {
    /// Of `stake`, `taken` left it (no more than the judge's NO side had at risk): `reward` and the bond to the
    /// charge's payKey, `burned` to nobody.
    Slashed { stake: i64, taken: i64, reward: i64, burned: i64 },
    /// The hold ended: the bond to the charged key (the signed price was upheld) or burned (the charge lapsed).
    Cleared { bond_to_key: bool },
}

/// `slash` or `clear` of the stake with `KpfCharge.settle`, whichever the charge's status calls for at a lock time of
/// `lock` (a pending charge lapses `chargeDaa` after it was accepted). `a` and `later` show the charge to be among the
/// stake's holds ([`hold_witness`]; a stake held for this charge alone: zero and none). In 0 the committee or the exit, 1
/// the charge, then the funding.
/// Slash: out 0 the committee (a member), what is left of the exit, or change (an exit taken whole), 1 the payKey's bond
/// and share, the burn last.
/// Clear: out 0 the committee or the exit, 1 the bond (to the charged key, or burned), then change.
pub fn charge_settle(stake: &Staked, ch: &OLive<Charge>, a: &[u8; 32], later: &Later, lock: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Settled)> {
    let id = stake.id()?;
    let cid = id_of(ch, "charge")?.as_bytes();
    if ch.s.committee_id != id.as_bytes() {
        return invalid("the charge is of another committee");
    }
    let Some(slash) = ch.s.outcome(ch.daa as i64, lock as i64) else {
        return invalid(match ch.s.status {
            CHARGE_CONTESTED => "the charge is with the judge: `charge-decide` delivers the verdict first".to_string(),
            _ => format!("the charge is pending and can still be contested until DAA {}", ch.daa as i64 + ch.s.p.daa),
        });
    };
    let wit = vec![crate::oracle_build::b8(1), crate::oracle_build::b8(match stake { Staked::Member(_, i) => *i as i64, Staked::Exit(_) => 0 }), bv(a), bv(&later_bytes(later))];
    let entry = if slash { "slash" } else { "clear" };
    let mut d = start(rate);
    if ch.s.status == CHARGE_PENDING {
        d.lock((ch.daa as i64 + ch.s.p.daa) as u64);
    }
    let settle = |d: &mut ODraft| d.contract(ch, OWhich::C(CWhich::Charge), CObj::Charge(ch.s).redeem(), "settle", vec![crate::oracle_build::b8(0)], "charge", 0);
    let w = stake.stake();
    let out = if slash {
        const AUTHOR: &str = "the charge's author: its bond and its share of the slash";
        let (s, reward) = match stake {
            Staked::Member(r, i) => {
                let (r2, s, reward) = r.s.slash(*i, &ch.s, &cid, a, later).map_err(refuse)?;
                unbacked(&r2, r.value as i64 - s)?;
                d.contract(*r, OWhich::C(CWhich::Committee), CObj::Committee(r.s).redeem(), entry, wit, "committee", 0);
                settle(&mut d);
                d.obj(OObj::C(CObj::Committee(r2)), (r.value as i64 - s) as u64, Some((0, id)), "committee");
                d.plain(ch.value as i64 + reward, &ch.s.pay_key, AUTHOR);
                d.fund(funder, change_to);
                (s, reward)
            }
            Staked::Exit(x) => {
                let (left, s, reward) = x.s.slash(&ch.s, &cid, a, later).map_err(refuse)?;
                d.contract(*x, OWhich::C(CWhich::Exit), CObj::Exit(x.s).redeem(), entry, wit, "exit", 0);
                settle(&mut d);
                match left {
                    Some(x2) => {
                        d.obj(OObj::C(CObj::Exit(x2)), (x.value as i64 - s) as u64, Some((0, id)), "exit");
                        d.plain(ch.value as i64 + reward, &ch.s.pay_key, AUTHOR);
                        d.fund(funder, change_to);
                    }
                    None => {
                        d.fund(funder, change_to);
                        d.plain(ch.value as i64 + reward, &ch.s.pay_key, AUTHOR);
                    }
                }
                (s, reward)
            }
        };
        if s - reward >= BURN_DUST {
            d.last_script((s - reward) as u64, burn_spk(), "burned", change_to);
        }
        Settled::Slashed { stake: w, taken: s, reward, burned: s - reward }
    } else {
        match stake {
            Staked::Member(r, i) => {
                let r2 = r.s.clear(*i, &cid, a, later).map_err(refuse)?;
                d.contract(*r, OWhich::C(CWhich::Committee), CObj::Committee(r.s).redeem(), entry, wit, "committee", 0);
                settle(&mut d);
                d.obj(OObj::C(CObj::Committee(r2)), r.value, Some((0, id)), "committee");
            }
            Staked::Exit(x) => {
                let x2 = x.s.clear(&cid, a, later).map_err(refuse)?;
                d.contract(*x, OWhich::C(CWhich::Exit), CObj::Exit(x.s).redeem(), entry, wit, "exit", 0);
                settle(&mut d);
                d.obj(OObj::C(CObj::Exit(x2)), x.value, Some((0, id)), "exit");
            }
        }
        if ch.s.bond_to_key() {
            d.plain(ch.value as i64, &ch.s.stake_key, "the charged key: the bond of an upheld charge");
        } else {
            d.out(ch.value, burn_spk(), None, "the bond of a lapsed charge, burned", None);
        }
        d.fund(funder, change_to);
        Settled::Cleared { bond_to_key: ch.s.bond_to_key() }
    };
    Ok((d.finish()?, out))
}

/// `KpfEpochTable.retire`: the epoch table's value joins the funder's change.
pub fn retire(epoch_table: &OLive<EpochTable>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let mut d = start(rate);
    d.lock(epoch_table.s.retire_from().max(0) as u64);
    d.contract(epoch_table, OWhich::C(CWhich::EpochTable), CObj::EpochTable(epoch_table.s).redeem(), "retire", vec![], "epoch table", 0);
    d.fund(funder, change_to);
    d.finish()
}

/// `KpfExit.withdraw` (anyone): out 0 the stake to its key.
pub fn withdraw(exit: &OLive<Exit>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let amount = exit.s.withdraw(exit.s.release).map_err(refuse)?;
    let mut d = start(rate);
    d.lock(exit.s.release as u64);
    d.contract(exit, OWhich::C(CWhich::Exit), CObj::Exit(exit.s).redeem(), "withdraw", vec![], "exit", 0);
    d.payout(amount, &exit.s.key, "the member's stake");
    d.fund(funder, change_to);
    d.finish()
}

// ---------------------------------------------------------------- the epoch table as a price

pub(crate) fn attest_input(d: &mut ODraft, epoch_table: &OLive<EpochTable>, a: &Attestation, lock: u64) -> Result<Hash> {
    let rid = id_of(epoch_table, "epoch table")?;
    a.check(&epoch_table.s, &rid.as_bytes(), lock as i64).map_err(refuse)?;
    let args = a.args().into_iter().map(Arg::V).collect();
    d.contract(epoch_table, OWhich::C(CWhich::EpochTable), CObj::EpochTable(epoch_table.s).redeem(), "attest", args, "epoch table", 0);
    Ok(rid)
}

fn market_of_epoch_table(m: &CMarket, epoch_table: &OLive<EpochTable>, a: &Attestation) -> Result<()> {
    if epoch_table.id.map(|h| h.as_bytes()) != Some(m.committee_id) {
        return invalid("the epoch table is not of this market's committee");
    }
    if a.market != m.id() {
        return invalid("the attestation is for another market");
    }
    if !m.spread_ok(epoch_table.s.spread_bps) {
        return invalid(format!(
            "the committee's spread ({} bps) leaves the market's tolerance g ({} bps) no margin: g must be at least {} times the spread, or no epoch table of the committee prices the market",
            epoch_table.s.spread_bps, m.p.g_bps, SPREAD_MARGIN
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------- orders and the open

/// A new order: a genesis authored by the first funding input, escrowing `value`.
pub fn create_order(o: &COpen, value: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    o.m.check().map_err(refuse)?;
    if o.my_rate <= 0 || o.min_counter_rate < 0 || o.tip < 0 {
        return invalid("an order's rate must be positive, its counter rate and tip not negative");
    }
    if funder.is_empty() {
        return invalid("an order's genesis needs a funding input to author it");
    }
    let mut d = start(rate);
    d.fund(funder, change_to);
    d.genesis(OObj::C(CObj::Order(o.clone())), value, 0, "order");
    d.finish()
}

fn healthy_pair(p: &CPair, x: i64) -> Result<()> {
    let (b, m) = (BPS as i128, p.m.p.mmr_bps as i128);
    if p.bl < 0 || p.bl >= p.bs || p.bs > p.m.p.price_max {
        return invalid("the bounds do not form a range within priceMax");
    }
    if (x as i128) * (b - m) < p.bl as i128 * b || (p.bs as i128) * b < (x as i128) * (b + m) {
        return invalid("a side would be below its maintenance at the entry");
    }
    if !p.range_ok() {
        return invalid("the range is not wider than both maintenance bands");
    }
    Ok(())
}

fn order_out(d: &mut ODraft, o: &OLive<COpen>, n: i64, at: u16) -> Result<()> {
    let id = id_of(o, "order")?;
    let v = o.value as i64 - o.s.spend(n);
    if v < 0 {
        return invalid("the order's escrow does not cover this part");
    }
    if n < o.s.size_left {
        d.obj(OObj::C(CObj::Order(o.s.after(n))), v as u64, Some((at, id)), "order (rest)");
    } else {
        d.payout(v, &o.s.owner, "order's refund");
    }
    Ok(())
}

fn lease_lock(orders: &[&OLive<COpen>], xe: i64) -> Result<u64> {
    let term = orders[0].s.m.p.term;
    let least = orders.iter().map(|o| o.s.least_lease(o.daa as i64)).max().expect("an order");
    if xe < least {
        return invalid(format!("the lease must be at least DAA {least}: a term after the youngest order was accepted"));
    }
    Ok(orders.iter().map(|o| o.s.active_from).max().expect("an order").max(xe - term).max(0) as u64)
}

/// Order x order: n units at x with the lease `xe`, at a lock time of at least `lock`. In 0 the long order, 1 the
/// short order, then the funding; out 0 and 1 each order's rest or refund, 2 the pair (a fresh id authored by the
/// first funding input), then change.
pub fn open(long: &OLive<COpen>, short: &OLive<COpen>, n: i64, x: i64, xe: i64, lock: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPair)> {
    let k = pinned::kit();
    if long.s.side != LONG || short.s.side != SHORT || long.s.m != short.s.m {
        return invalid("an open needs a long and a short order of one market");
    }
    if !long.s.part_ok(n) || !short.s.part_ok(n) {
        return invalid("the part is not one both orders can be filled in (size, minimum fill, or less than 0.2 KAS of margin in the part or in what it leaves)");
    }
    if x > long.s.limit || x < short.s.limit {
        return invalid("the price is outside the orders' limits");
    }
    let p = CPair {
        m: long.s.m,
        size: n,
        bl: x - long.s.my_rate,
        bs: x + short.s.my_rate,
        entry: x,
        long_key: long.s.owner,
        short_key: short.s.owner,
        expiry: xe,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: Makers { long: maker_of(&long.op), short: maker_of(&short.op), ..Makers::default() },
    };
    healthy_pair(&p, x)?;
    if short.s.my_rate < long.s.min_counter_rate || long.s.my_rate < short.s.min_counter_rate {
        return invalid("an order's rate is below the other's minimum counter rate");
    }
    if funder.is_empty() {
        return invalid("an open needs a funding input to author the pair");
    }
    let [pre, suf] = tmpl(&k.pair);
    let args = |other: &OLive<COpen>| vec![iv(2), iv(n), iv(x), iv(p.bl), iv(p.bs), bv(&other.s.owner), bv(&maker_of(&other.op)), iv(xe), pre.clone(), suf.clone()];
    let mut d = start(rate);
    d.lock(lease_lock(&[long, short], xe)?.max(lock));
    d.contract(long, OWhich::C(CWhich::Open), long.s.redeem(k), "fill", args(short), "long order", 0);
    d.contract(short, OWhich::C(CWhich::Open), short.s.redeem(k), "fill", args(long), "short order", 0);
    order_out(&mut d, long, n, 0)?;
    order_out(&mut d, short, n, 1)?;
    d.genesis(OObj::C(CObj::Pair(p.clone())), p.floor() as u64, 2, "pair");
    d.fund(funder, change_to);
    Ok((d.finish()?, p))
}

/// Order x signing taker: the funder's key `taker` takes the other side with the margin rate `taker_rate`. In 0 the
/// order, then the funding; out 0 the order's rest or refund, 1 the pair, then change.
pub fn open_taker(order: &OLive<COpen>, n: i64, x: i64, taker_rate: i64, taker: [u8; 32], xe: i64, lock: u64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPair)> {
    let k = pinned::kit();
    let o = &order.s;
    if !o.part_ok(n) {
        return invalid("the part is not one the order can be filled in");
    }
    if taker_rate < o.min_counter_rate || taker_rate <= 0 {
        return invalid("the taker's rate is below the order's minimum counter rate");
    }
    let long_side = o.side == LONG;
    if (long_side && x > o.limit) || (!long_side && x < o.limit) {
        return invalid("the price is outside the order's limit");
    }
    let (bl, bs) = if long_side { (x - o.my_rate, x + taker_rate) } else { (x - taker_rate, x + o.my_rate) };
    let p = CPair {
        m: o.m,
        size: n,
        bl,
        bs,
        entry: x,
        long_key: if long_side { o.owner } else { taker },
        short_key: if long_side { taker } else { o.owner },
        expiry: xe,
        long_ask: 0,
        long_min: 0,
        long_tip: 0,
        short_bid: 0,
        short_min: 0,
        short_tip: 0,
        mk: if long_side { Makers { long: maker_of(&order.op), ..Makers::default() } } else { Makers { short: maker_of(&order.op), ..Makers::default() } },
    };
    healthy_pair(&p, x)?;
    if funder.is_empty() {
        return invalid("an open needs a funding input to author the pair");
    }
    let [pre, suf] = tmpl(&k.pair);
    let mut d = start(rate);
    d.lock(lease_lock(&[order], xe)?.max(lock));
    d.contract(order, OWhich::C(CWhich::Open), o.redeem(k), "fill", vec![iv(1), iv(n), iv(x), iv(bl), iv(bs), bv(&taker), bv(&[0; 32]), iv(xe), pre, suf], "order", 0);
    order_out(&mut d, order, n, 0)?;
    d.genesis(OObj::C(CObj::Pair(p.clone())), p.floor() as u64, 1, "pair");
    d.fund(funder, change_to);
    Ok((d.finish()?, p))
}

/// `KpfOpenC.cancel` (the owner signs): everything to the owner, less the fee.
pub fn cancel(order: &OLive<COpen>, rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let mut d = start(rate);
    d.contract(order, OWhich::C(CWhich::Open), order.s.redeem(k), "cancel", vec![Arg::Sig(order.s.owner)], "order", 0);
    let c = d.out(0, crate::tx::p2pk_spk(&order.s.owner), None, "owner (absorbs the fee)", None);
    d.change = Some(c);
    d.finish()
}

/// `KpfOpenC.refund` (anyone, once the order has expired or rested 90 days): out 0 the escrow to the owner, less
/// `keeperFeeMin`, which stays with the funder who sends it.
pub fn refund(order: &OLive<COpen>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let mut d = start(rate);
    d.lock(order.s.expiry_daa.min(order.daa as i64 + ORDER_MAX_IDLE).max(0) as u64);
    d.contract(order, OWhich::C(CWhich::Open), order.s.redeem(k), "refund", vec![], "order", 0);
    d.payout(order.value as i64 - order.s.refund_fee(order.value), &order.s.owner, "owner");
    d.fund(funder, change_to);
    d.finish()
}

// ---------------------------------------------------------------- claims

/// A claim on the part n at the attested price: a close (`kind` 1, the owner of `side` signs) or a liquidation (`kind`
/// 2, the loser `side`; the fee goes to `proposer`). In 0 the pair, 1 the epoch table, then the funding; out 0 the pending
/// part (a fresh id authored by the pair), 1 the epoch table, 2 the remainder of a split, then change.
pub fn propose(pair: &OLive<CPair>, epoch_table: &OLive<EpochTable>, a: &Attestation, kind: i64, side: i64, n: i64, proposer: [u8; 32], funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPending)> {
    let k = pinned::kit();
    let p = &pair.s;
    let id = id_of(pair, "pair")?;
    market_of_epoch_table(&p.m, epoch_table, a)?;
    let why = match (kind, side) {
        (KIND_CLOSE, _) if !p.close_ok(side, a.p.min(p.m.p.price_max)) => "the closer would be below its maintenance at the price it is paid (the attested price moved by g against it): add margin, or the exit is the liquidation",
        (KIND_LIQ, _) if p.long_key == p.short_key => "a part with one owner is not liquidated",
        (KIND_LIQ, LONG) if !p.long_liquidatable(a.p.min(p.m.p.price_max)) => "the attested price does not liquidate the long (the tolerance g is inside the trigger)",
        (KIND_LIQ, SHORT) if !p.short_liquidatable(a.p.min(p.m.p.price_max)) => "the attested price does not liquidate the short (the tolerance g is inside the trigger)",
        _ => "the part cannot be claimed (its size, or a split that leaves a side less than 0.2 KAS at the entry)",
    };
    if !p.price_fresh(a.t0, pair.daa) {
        return invalid(format!("the signed price (DAA {}) is more than lateDaa ({}) older than the pair's UTXO (DAA {}): a claim with it would be void, and the contract refuses it", a.t0, p.m.p.late_daa, pair.daa));
    }
    let (w, fb) = p.pending_of(k, n, kind, side, a.p, a.t0, proposer).ok_or_else(|| Error::Invalid(why.into()))?;
    let w = w.made_by(&pair.op);
    let [pre, suf] = tmpl(&k.pending);
    let (s, seq) = if kind == KIND_CLOSE { (Arg::Sig(w.proposer), p.m.p.quiet_daa as u64) } else { (bv(&[0u8; 65]), 0) };
    let mut d = start(rate);
    d.lock(a.t0 as u64);
    d.contract(pair, OWhich::C(CWhich::Pair), p.redeem(k), "propose", vec![iv(n), iv(kind), iv(side), iv(a.p), iv(a.t0), iv(1), s, iv(fb), iv(0), bv(&proposer), pre, suf], "pair", seq);
    let rid = attest_input(&mut d, epoch_table, a, a.t0 as u64)?;
    d.genesis(OObj::C(CObj::Pending(w.clone())), p.pending_value(pair.value, &w) as u64, 0, "pending part");
    d.obj(OObj::C(CObj::EpochTable(epoch_table.s)), epoch_table.value, Some((1, rid)), "epoch table");
    if n < p.size {
        d.obj(OObj::C(CObj::Pair(p.rest_of(p.size - n))), p.rest_value(pair.value, n) as u64, Some((0, id)), "remainder");
    }
    d.fund(funder, change_to);
    Ok((d.finish()?, w))
}

/// A trim (`KpfPairC.propose` kind 3): the owner of `side` moves its bound inward to `nb` at the attested price and is
/// paid what that frees when the claim is finalized. In 0 the pair, 1 the epoch table, then the funding; out 0 the pending
/// part (the whole pair, a fresh id authored by the pair), 1 the epoch table, then change.
pub fn trim(pair: &OLive<CPair>, epoch_table: &OLive<EpochTable>, a: &Attestation, side: i64, nb: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPending)> {
    let k = pinned::kit();
    let p = &pair.s;
    id_of(pair, "pair")?;
    market_of_epoch_table(&p.m, epoch_table, a)?;
    if side != LONG && side != SHORT {
        return invalid("the side is long or short");
    }
    if !p.price_fresh(a.t0, pair.daa) {
        return invalid(format!("the signed price (DAA {}) is more than lateDaa ({}) older than the pair's UTXO (DAA {}): a claim with it would be void, and the contract refuses it", a.t0, p.m.p.late_daa, pair.daa));
    }
    let px = a.p.min(p.m.p.price_max);
    let w = match p.trim_pending(k, side, a.p, a.t0, nb) {
        Some(w) => w.made_by(&pair.op),
        None => {
            let t = p.trim_state(side, nb);
            let inward = nb > p.bl && nb < p.bs;
            return invalid(if !inward {
                "a trim moves the side's bound inward, inside the range"
            } else if !t.close_ok(side, px) {
                "the side would be below its maintenance at the attested price moved by g against it: the bound asked for is too near"
            } else if !t.range_ok() {
                "the trimmed range would not be wider than both maintenance bands"
            } else {
                "the trim frees less than 0.02 KAS, or the pair is smaller than the market's least size"
            });
        }
    };
    let [pre, suf] = tmpl(&k.pending);
    let mut d = start(rate);
    d.lock(a.t0 as u64);
    d.contract(pair, OWhich::C(CWhich::Pair), p.redeem(k), "propose", vec![iv(p.size), iv(KIND_TRIM), iv(side), iv(a.p), iv(a.t0), iv(1), Arg::Sig(w.proposer), iv(nb), iv(0), bv(&w.proposer), pre, suf], "pair", p.m.p.quiet_daa as u64);
    let rid = attest_input(&mut d, epoch_table, a, a.t0 as u64)?;
    d.genesis(OObj::C(CObj::Pending(w.clone())), pair.value, 0, "pending trim");
    d.obj(OObj::C(CObj::EpochTable(epoch_table.s)), epoch_table.value, Some((1, rid)), "epoch table");
    d.fund(funder, change_to);
    Ok((d.finish()?, w))
}

/// Finalize (anyone): in 0 the part, then the funding; the outputs of [`CPending::settlement`]: (close) the long at 0,
/// change, the short last; (forfeit) the proposer's fee at 0, the forfeited part, change; (void) the restored part;
/// (trim) the freed margin to the side's owner at 0, the trimmed pair, change.
pub fn finalize(pending: &OLive<CPending>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CSettlement)> {
    let k = pinned::kit();
    let w = &pending.s;
    let id = id_of(pending, "part")?;
    let s = w.settlement(pending.value, pending.daa as i64);
    let mut d = start(rate);
    d.contract(pending, OWhich::C(CWhich::Pending), w.redeem(k), "finalize", vec![], "pending part", w.finalize_age(pending.daa as i64));
    let mut last: Option<(i64, [u8; 32])> = None;
    match &s {
        CSettlement::Close { long, short } => {
            if *long > 0 {
                d.payout(*long, &w.long_key, "long");
            }
            if *short > 0 {
                last = Some((*short, w.short_key));
            }
        }
        CSettlement::Forfeit { part, value, fee } => {
            if *fee > 0 {
                d.payout(*fee, &w.proposer, "proposer's fee");
            }
            d.obj(OObj::C(CObj::Pair(part.clone())), *value as u64, Some((0, id)), "forfeited part");
        }
        CSettlement::Restore { part, value } => {
            d.obj(OObj::C(CObj::Pair(part.clone())), *value as u64, Some((0, id)), "restored part");
        }
        CSettlement::Trim { part, value, take } => {
            d.payout(*take, &w.proposer, "freed margin");
            d.obj(OObj::C(CObj::Pair(part.clone())), *value as u64, Some((0, id)), "trimmed pair");
        }
    }
    d.fund(funder, change_to);
    if let Some((v, key)) = last {
        d.last(v, &key, "short", change_to);
    }
    Ok((d.finish()?, s))
}

/// A cure by the loser (its key signs; `funder` is the loser's own funding): out 0 the proposer's refund when there is
/// one, then the cured part.
pub fn cure(pending: &OLive<CPending>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPair)> {
    let k = pinned::kit();
    let w = &pending.s;
    let id = id_of(pending, "part")?;
    let (c, v) = w.cure(pending.value).ok_or_else(|| Error::Invalid("no cure: a close, or a short whose cured bound would exceed priceMax".into()))?;
    let loser = if w.side == LONG { w.long_key } else { w.short_key };
    let mut d = start(rate);
    d.contract(pending, OWhich::C(CWhich::Pending), w.redeem(k), "cure", vec![Arg::Sig(loser)], "pending part", 0);
    if w.refund > 0 {
        d.payout(w.refund, &w.proposer, "proposer's carrier back");
    }
    d.obj(OObj::C(CObj::Pair(c.clone())), v as u64, Some((0, id)), "cured part");
    d.fund(funder, change_to);
    Ok((d.finish()?, c))
}

// ---------------------------------------------------------------- the lease

/// `KpfPairC.keepAlive`: in 0 the pair, 1 the epoch table, then the funding; out 0 the pair with its lease at t0 + term, 1
/// the epoch table, then change.
pub fn keep_alive(pair: &OLive<CPair>, epoch_table: &OLive<EpochTable>, a: &Attestation, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPair)> {
    let k = pinned::kit();
    let p = &pair.s;
    let id = id_of(pair, "pair")?;
    market_of_epoch_table(&p.m, epoch_table, a)?;
    let next = p.kept_alive(a.t0).ok_or_else(|| Error::Invalid(format!("a signed price at DAA {} does not move the lease ({}) by a quarter of a term: the first signed price that does is DAA {}", a.t0, p.expiry, p.keep_alive_from())))?;
    let mut d = start(rate);
    d.lock(a.t0 as u64);
    d.contract(pair, OWhich::C(CWhich::Pair), p.redeem(k), "keepAlive", vec![iv(1), iv(a.t0)], "pair", 0);
    let rid = attest_input(&mut d, epoch_table, a, a.t0 as u64)?;
    d.obj(OObj::C(CObj::Pair(next.clone())), pair.value, Some((0, id)), "pair");
    d.obj(OObj::C(CObj::EpochTable(epoch_table.s)), epoch_table.value, Some((1, rid)), "epoch table");
    d.fund(funder, change_to);
    Ok((d.finish()?, next))
}

/// A lapse settle (anyone): out 0 the long, change, the short last.
pub fn lapse_settle(pair: &OLive<CPair>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let p = &pair.s;
    let (l, s) = p.lapse_out(pair.value);
    let mut d = start(rate);
    d.lock(p.lapse_from());
    d.contract(pair, OWhich::C(CWhich::Pair), p.redeem(k), "lapseSettle", vec![], "pair", 0);
    if l > 0 {
        d.payout(l, &p.long_key, "long");
    }
    d.fund(funder, change_to);
    if s > 0 {
        d.last(s, &p.short_key, "short", change_to);
    }
    d.finish()
}

// ---------------------------------------------------------------- the pair's price-free entries

/// An amend of `side`'s listing (its owner signs; after quietDaa).
pub fn amend(pair: &OLive<CPair>, side: i64, ask: i64, min_part: i64, tip: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPair)> {
    let k = pinned::kit();
    let p = &pair.s;
    let id = id_of(pair, "pair")?;
    if ask < 0 || ask > p.m.p.price_max || min_part < 0 || tip < 0 {
        return invalid("a listing's price must lie in [0, priceMax], its minimum part and tip not negative");
    }
    let mut c = p.rest_of(p.size);
    let key = if side == LONG {
        (c.long_ask, c.long_min, c.long_tip) = (ask, min_part, tip);
        p.long_key
    } else {
        (c.short_bid, c.short_min, c.short_tip) = (ask, min_part, tip);
        p.short_key
    };
    let mut d = start(rate);
    d.contract(pair, OWhich::C(CWhich::Pair), p.redeem(k), "amend", vec![Arg::Sig(key), iv(side), iv(ask), iv(min_part), iv(tip)], "pair", p.m.p.quiet_daa as u64);
    d.obj(OObj::C(CObj::Pair(c.clone())), pair.value, Some((0, id)), "pair");
    d.fund(funder, change_to);
    Ok((d.finish()?, c))
}

/// `addMargin` by `delta` per whole unit on `side` (its owner signs; after quietDaa).
pub fn add_margin(pair: &OLive<CPair>, side: i64, delta: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPair)> {
    let k = pinned::kit();
    let p = &pair.s;
    let id = id_of(pair, "pair")?;
    let mut c = p.rest_of(p.size);
    let key = if side == LONG {
        c.bl -= delta;
        p.long_key
    } else {
        c.bs += delta;
        p.short_key
    };
    if delta <= 0 || c.bl < 0 || c.bs > p.m.p.price_max {
        return invalid("the margin added must be positive and keep the bounds within [0, priceMax]");
    }
    let add = q(p.size, delta, p.m.p.scale, p.m.p.scale - 1);
    let mut d = start(rate);
    d.contract(pair, OWhich::C(CWhich::Pair), p.redeem(k), "addMargin", vec![Arg::Sig(key), iv(side), iv(delta)], "pair", p.m.p.quiet_daa as u64);
    d.obj(OObj::C(CObj::Pair(c.clone())), (pair.value as i64 + add).max(c.floor()) as u64, Some((0, id)), "pair");
    d.fund(funder, change_to);
    Ok((d.finish()?, c))
}

/// The bounds a hand-over of `side` at `x` puts on the entrant's bound `nb` (`KpfPairC.handOver`): the allowed range.
pub fn hand_over_bounds(p: &CPair, side: i64, x: i64, nb: i64) -> Result<(i64, i64)> {
    use crate::error::kas_str;
    let mp = &p.m.p;
    let b = BPS as i128;
    let kas = |v: i64| kas_str(v as i128);
    if x < 0 || x > mp.price_max {
        return invalid(format!("the price {} KAS is outside [0, priceMax]", kas(x)));
    }
    let (lo, hi) = if side == LONG {
        (0, p.bl.min(((x as i128) * (b - mp.mmr_bps as i128) / b) as i64))
    } else {
        (p.bs.max((((x as i128) * (b + mp.mmr_bps as i128) + b - 1) / b) as i64), mp.price_max)
    };
    if lo > hi || nb < lo || nb > hi {
        return invalid(format!(
            "the entrant's bound {} KAS is refused: it must lie in {} .. {} KAS (no nearer than the leaver's bound, and healthy at {} KAS with {} % maintenance)",
            kas(nb),
            kas(lo),
            kas(hi),
            kas(x),
            mp.mmr_bps as f64 / 100.0
        ));
    }
    Ok((lo, hi))
}

/// What a hand-over writes: (the leaver's payout or 0, the part, the part's value, the remainder and its value).
fn handed(pair: &OLive<CPair>, side: i64, n: i64, x: i64, nb: i64, ek: [u8; 32], et: [u8; 32], tip: i64) -> (i64, CPair, i64, Option<(CPair, i64)>) {
    let p = &pair.s;
    let sc = p.m.p.scale;
    let mut pay = owed(n, if side == LONG { x - p.bl } else { p.bs - x }, tip, sc);
    let mut keep = pair.value as i64 - p.range_of(n);
    if pay < PLAIN_DUST {
        keep += pay;
        pay = 0;
    }
    let mut part = p.rest_of(n);
    part.mk = Makers { parent: maker_of(&pair.op), ..Makers::default() };
    if side == LONG {
        part.bl = nb;
        part.long_key = ek;
        (part.long_ask, part.long_min, part.long_tip) = (0, 0, 0);
        part.mk.long = et;
    } else {
        part.bs = nb;
        part.short_key = ek;
        (part.short_bid, part.short_min, part.short_tip) = (0, 0, 0);
        part.mk.short = et;
    }
    let range = part.range_of(n);
    if n == p.size {
        (pay, part, range + keep.max(p.m.p.carrier), None)
    } else {
        let rest = p.rest_of(p.size - n);
        let rv = keep.max(rest.floor());
        (pay, part, range + p.m.p.carrier, Some((rest, rv)))
    }
}

fn hand_over(pair: &OLive<CPair>, entry: &'static str, side: i64, n: i64, x: i64, nb: i64, entrant: [u8; 32], funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPair)> {
    let k = pinned::kit();
    let p = &pair.s;
    let id = id_of(pair, "pair")?;
    if !p.split_ok(n) {
        return invalid("the part cannot be handed over (its size, or a split that leaves a side less than 0.2 KAS at the entry)");
    }
    hand_over_bounds(p, side, x, nb)?;
    let listed = entry == "transfer";
    let tip = if !listed {
        0
    } else if side == LONG {
        if !(p.long_ask > 0 && x >= p.long_ask && (n >= p.long_min || n == p.size)) {
            return invalid("the long is not listed at or below this price for this size");
        }
        p.long_tip
    } else {
        if !(p.short_bid > 0 && x <= p.short_bid && (n >= p.short_min || n == p.size)) {
            return invalid("the short is not listed at or above this price for this size");
        }
        p.short_tip
    };
    let (pay, part, pv, rest) = handed(pair, side, n, x, nb, entrant, [0; 32], tip);
    let leaver = if side == LONG { p.long_key } else { p.short_key };
    let c = if pay > 0 { 1 } else { 0 };
    let args = if listed {
        vec![iv(side), iv(n), iv(x), iv(nb), bv(&entrant), bv(&[0; 32]), iv(c)]
    } else {
        vec![Arg::Sig(leaver), iv(side), iv(n), iv(x), iv(nb), bv(&entrant), iv(c)]
    };
    let mut d = start(rate);
    d.contract(pair, OWhich::C(CWhich::Pair), p.redeem(k), entry, args, "pair", p.m.p.quiet_daa as u64);
    if pay > 0 {
        d.payout(pay, &leaver, "leaver");
    }
    d.genesis(OObj::C(CObj::Pair(part.clone())), pv as u64, 0, "pair (the entrant's part)");
    if let Some((r, rv)) = rest {
        d.obj(OObj::C(CObj::Pair(r)), rv as u64, Some((0, id)), "remainder");
    }
    d.fund(funder, change_to);
    Ok((d.finish()?, part))
}

/// `assign`: the owner of `side` signs its side of the part n over to `entrant` at `x` (the entrant funds and posts
/// the bound `nb`). Out 0 the leaver's payout, then the part (a fresh id authored by the pair), the remainder, change.
pub fn assign(pair: &OLive<CPair>, side: i64, n: i64, x: i64, nb: i64, entrant: [u8; 32], funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPair)> {
    hand_over(pair, "assign", side, n, x, nb, entrant, funder, change_to, rate)
}

/// `transfer`: a listed `side` of the part n goes to a signing taker `entrant` at `x` within the listing.
pub fn transfer(pair: &OLive<CPair>, side: i64, n: i64, x: i64, nb: i64, entrant: [u8; 32], funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPair)> {
    hand_over(pair, "transfer", side, n, x, nb, entrant, funder, change_to, rate)
}

/// An order takes a listed side of the part n at `x`: in 0 the pair (transfer), 1 the order (take), then the funding.
/// Out 0 the leaver's payout (or the part when there is none), 1 the order's rest or refund, then the part, the
/// remainder, change.
pub fn take(pair: &OLive<CPair>, order: &OLive<COpen>, n: i64, x: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, CPair)> {
    let k = pinned::kit();
    let (p, o) = (&pair.s, &order.s);
    let id = id_of(pair, "pair")?;
    if p.m != o.m {
        return invalid("the order is of another market");
    }
    if !p.split_ok(n) || !o.part_ok(n) {
        return invalid("the part is not one the pair can split off and the order can take");
    }
    let side = o.side;
    let nb = if side == LONG { x - o.my_rate } else { x + o.my_rate };
    hand_over_bounds(p, side, x, nb)?;
    let (tip, stayer, ca, cm, ct, bound, ok) = if side == LONG {
        (p.long_tip, p.short_key, p.short_bid, p.short_min, p.short_tip, p.bs, p.long_ask > 0 && x >= p.long_ask && (n >= p.long_min || n == p.size) && x <= o.limit && p.entry >= x && p.bs - x >= o.min_counter_rate)
    } else {
        (p.short_tip, p.long_key, p.long_ask, p.long_min, p.long_tip, p.bl, p.short_bid > 0 && x <= p.short_bid && (n >= p.short_min || n == p.size) && x >= o.limit && p.entry <= x && x - p.bl >= o.min_counter_rate)
    };
    if !ok {
        return invalid("the side is not listed at this price for this size, or the price, the pair's entry or the stayer's rate is outside the order's terms");
    }
    let (pay, part, pv, rest) = handed(pair, side, n, x, nb, o.owner, maker_of(&order.op), tip);
    if !part.range_ok() {
        return invalid("the part's range would not be wider than both maintenance bands");
    }
    healthy_pair(&part, x)?;
    // The lease an order enters: at least half a term after the order was accepted, at most a term from the present.
    let least = order.daa as i64 + o.m.p.term / 2;
    if p.expiry < least {
        return invalid(format!("the pair's lease (DAA {}) ends less than half a term after the order was accepted (it must reach DAA {least}): renew it first (`c-keep-alive`)", p.expiry));
    }
    let leaver = if side == LONG { p.long_key } else { p.short_key };
    let c = if pay > 0 { 2 } else { 0 };
    let [pre, suf] = tmpl(&k.pair);
    let mut d = start(rate);
    d.lock(o.active_from.max(p.expiry - o.m.p.term).max(0) as u64);
    d.contract(pair, OWhich::C(CWhich::Pair), p.redeem(k), "transfer", vec![iv(side), iv(n), iv(x), iv(nb), bv(&o.owner), bv(&maker_of(&order.op)), iv(c)], "pair", p.m.p.quiet_daa as u64);
    d.contract(
        order,
        OWhich::C(CWhich::Open),
        o.redeem(k),
        "take",
        vec![iv(c), iv(n), iv(x), iv(p.entry), iv(bound), bv(&stayer), bv(&maker_of(&pair.op)), iv(ca), iv(cm), iv(ct), iv(p.expiry), pre, suf],
        "order",
        0,
    );
    if pay > 0 {
        d.payout(pay, &leaver, "leaver");
        order_out(&mut d, order, n, 1)?;
        d.genesis(OObj::C(CObj::Pair(part.clone())), pv as u64, 0, "pair (the order's part)");
    } else {
        d.genesis(OObj::C(CObj::Pair(part.clone())), pv as u64, 0, "pair (the order's part)");
        order_out(&mut d, order, n, 1)?;
    }
    if let Some((r, rv)) = rest {
        d.obj(OObj::C(CObj::Pair(r)), rv as u64, Some((0, id)), "remainder");
    }
    d.fund(funder, change_to);
    Ok((d.finish()?, part))
}

/// A mutual close of the whole pair at `x` (both listings cross; anyone): out 0 the long, change, the short last.
pub fn mutual_close(pair: &OLive<CPair>, x: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let p = &pair.s;
    if !(p.long_ask > 0 && p.short_bid > 0 && x >= p.long_ask && x <= p.short_bid) {
        return invalid("both sides must be listed and the price must lie between the long's ask and the short's bid");
    }
    let sc = p.m.p.scale;
    let mut lp = owed(p.size, x - p.bl, p.long_tip, sc);
    let mut sp = owed(p.size, p.bs - x, p.short_tip, sc) + pair.value as i64 - p.range_of(p.size);
    if lp < PLAIN_DUST {
        sp += lp;
        lp = 0;
    }
    if sp < PLAIN_DUST {
        lp += sp;
        sp = 0;
    }
    let mut d = start(rate);
    d.contract(pair, OWhich::C(CWhich::Pair), p.redeem(k), "mutualClose", vec![iv(p.size), iv(x)], "pair", p.m.p.quiet_daa as u64);
    if lp > 0 {
        d.payout(lp, &p.long_key, "long");
    }
    d.fund(funder, change_to);
    if sp > 0 {
        d.last(sp, &p.short_key, "short", change_to);
    }
    d.finish()
}

/// A cooperative close (both owners sign): out 0 the long's `long_pay`, 1 the short's rest less the fee.
pub fn coop_close(pair: &OLive<CPair>, long_pay: u64, rate: u64) -> Result<OBuilt> {
    let k = pinned::kit();
    let p = &pair.s;
    if long_pay > pair.value {
        return invalid("the long's payout exceeds the pair");
    }
    let mut d = start(rate);
    d.contract(pair, OWhich::C(CWhich::Pair), p.redeem(k), "coopClose", vec![Arg::Sig(p.long_key), Arg::Sig(p.short_key)], "pair", 0);
    if long_pay > 0 {
        d.out(long_pay.max(VIABLE_OUTPUT_MIN), crate::tx::p2pk_spk(&p.long_key), None, "long", None);
    }
    let c = d.out(0, crate::tx::p2pk_spk(&p.short_key), None, "short (absorbs the fee)", None);
    d.change = Some(c);
    d.finish()
}

// ---------------------------------------------------------------- the cross-margin account

/// A new account (docs/DESIGN-COMMITTEE.md 3b): a genesis authored by the first funding input, holding the pot and the
/// carrier. Whoever funds it signs; the other party is given its side.
pub fn x_open(account: &Cross, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    account.check()?;
    if funder.is_empty() {
        return invalid("an account's genesis needs a funding input to author it");
    }
    let mut d = start(rate);
    d.fund(funder, change_to);
    d.genesis(OObj::C(CObj::Cross(*account)), account.floor() as u64, 0, "account");
    d.finish()
}

/// What A's side of `pair` is worth at the price `x` (what A gives up when the executor takes the side over).
pub fn side_worth(p: &CPair, side: i64, x: i64) -> i64 {
    if side == LONG {
        p.long_at(x)
    } else {
        p.short_at(x)
    }
}

/// The nearest bound the executor may take `side` of a pair over with at the side's own bound (`x_swap`): healthy at
/// that price with the market's maintenance, and no nearer than the leaver's.
pub fn swap_bound(p: &CPair, side: i64) -> i64 {
    let (b, m) = (BPS as i128, p.m.p.mmr_bps as i128);
    if side == LONG {
        ((p.bl as i128) * (b - m) / b) as i64
    } else {
        (((p.bs as i128) * (b + m) + b - 1) / b).min(p.m.p.price_max as i128) as i64
    }
}

/// The swap: A's side of one or two pairs becomes one account against B, in one transaction. Each pair is handed over
/// to B by `assign` at the side's own bound, so A is paid nothing by the pair: what A gets is the account, which B
/// funds. A signs every pair input (SIGHASH_ALL: the account it is given is under its signature), B the funding. In
/// 0.. the pairs, then B's funding; out 0.. the pairs with B in A's place (fresh ids authored by the pairs), then the
/// account (a genesis authored by the first funding input), then change. `pairs` are (the pair, A's side, the bound
/// B posts); leg i of `account` is pair i: its market, committee, scale, price limit and size, and A's side as direction.
pub fn x_swap(pairs: &[(&OLive<CPair>, i64, i64)], account: &Cross, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Vec<CPair>)> {
    let k = pinned::kit();
    account.check()?;
    if pairs.is_empty() || pairs.len() > 2 {
        return invalid("an account takes one or two positions");
    }
    if funder.is_empty() {
        return invalid("the account's genesis needs the counterparty's funding input to author it");
    }
    if pairs.len() == 1 && account.legs[1].size != 0 {
        return invalid("the account has a second leg and one pair was given");
    }
    let mut d = start(rate);
    let mut parts = vec![];
    for (i, (pair, side, nb)) in pairs.iter().enumerate() {
        let (p, leg) = (&pair.s, &account.legs[i]);
        id_of(pair, "pair")?;
        let leaver = if *side == LONG { p.long_key } else { p.short_key };
        if leaver != account.a_key {
            return invalid(format!("pair {}: its {} is not the account's owner", i + 1, if *side == LONG { "long" } else { "short" }));
        }
        if leg.market != p.m.id() || leg.committee != p.m.committee_id || leg.scale != p.m.p.scale || leg.price_max != p.m.p.price_max || leg.size != p.size || leg.dir != *side {
            return invalid(format!("leg {} of the account is not the position of pair {} (its market, committee, scale, price limit, size or direction)", i + 1, i + 1));
        }
        if !p.split_ok(p.size) {
            return invalid("the pair cannot be handed over");
        }
        let x = if *side == LONG { p.bl } else { p.bs };
        hand_over_bounds(p, *side, x, *nb)?;
        let (pay, part, pv, rest) = handed(pair, *side, p.size, x, *nb, account.b_key, [0; 32], 0);
        if pay != 0 || rest.is_some() {
            return invalid("the hand-over of a swap pays nothing and leaves no remainder");
        }
        d.contract(pair, OWhich::C(CWhich::Pair), p.redeem(k), "assign", vec![Arg::Sig(leaver), iv(*side), iv(p.size), iv(x), iv(*nb), bv(&account.b_key), iv(i as i64)], "pair", p.m.p.quiet_daa as u64);
        parts.push((part, pv));
    }
    for (i, (part, pv)) in parts.iter().enumerate() {
        d.genesis(OObj::C(CObj::Pair(part.clone())), *pv as u64, i, "pair (the counterparty in the owner's place)");
    }
    d.genesis(OObj::C(CObj::Cross(*account)), account.floor() as u64, pairs.len(), "account");
    d.fund(funder, change_to);
    Ok((d.finish()?, parts.into_iter().map(|(p, _)| p).collect()))
}

/// A claim on an account at the attested prices: a close (`kind` 1, the owner of `side` signs) or a liquidation
/// (`kind` 2, the loser `side`; the fee goes to `proposer`). `legs` are an epoch table and its attestation for every leg, for
/// one time. In 0 the account, 1.. the epoch tables, then the funding; out 0 the pending claim (a fresh id authored by the
/// account), 1.. the epoch tables, then change.
pub fn x_propose(account: &OLive<Cross>, legs: &[(&OLive<EpochTable>, &Attestation)], kind: i64, side: i64, proposer: [u8; 32], funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, XPending)> {
    let k = pinned::xkit();
    let c = &account.s;
    id_of(account, "account")?;
    let n = if c.legs[1].size > 0 { 2 } else { 1 };
    if legs.len() != n {
        return invalid(format!("the account has {n} leg(s): give an epoch table and its signed prices for each"));
    }
    let t0 = legs[0].1.t0;
    for (i, (t, a)) in legs.iter().enumerate() {
        if t.id.map(|h| h.as_bytes()) != Some(c.legs[i].committee) {
            return invalid(format!("the epoch table of leg {} is not of the leg's committee", i + 1));
        }
        if a.market != c.legs[i].market {
            return invalid(format!("the attestation of leg {} is for another market", i + 1));
        }
        if a.t0 != t0 {
            return invalid("the legs must be attested for one time");
        }
        if t.s.spread_bps * SPREAD_MARGIN > c.t.g_bps {
            return invalid(format!("the committee's spread ({} bps) leaves the account's tolerance g ({} bps) no margin", t.s.spread_bps, c.t.g_bps));
        }
    }
    if !c.price_fresh(t0, account.daa) {
        return invalid(format!("the signed price (DAA {t0}) is more than lateDaa ({}) older than the account's UTXO (DAA {}): a claim with it would be void, and the contract refuses it", c.t.late_daa, account.daa));
    }
    let p = [legs[0].1.p, if n == 2 { legs[1].1.p } else { 0 }];
    let why = if kind == KIND_CLOSE {
        "the closer would be below the maintenance at the prices it is paid at (each moved by g against it): add margin, or the exit is the liquidation"
    } else if c.a_key == c.b_key {
        "an account with one owner is not liquidated"
    } else {
        "the attested prices do not liquidate that side (the tolerance g is inside the trigger)"
    };
    let w = c.pending_of(k, kind, side, p, t0, proposer).ok_or_else(|| Error::Invalid(why.into()))?.made_by(&account.op);
    let [pre, suf] = tmpl(&k.pending);
    let (s, seq) = if kind == KIND_CLOSE { (Arg::Sig(w.proposer), c.t.quiet_daa as u64) } else { (bv(&[0u8; 65]), 0) };
    let mut d = start(rate);
    d.lock(t0 as u64);
    d.contract(account, OWhich::C(CWhich::Cross), c.redeem(k), "propose", vec![iv(kind), iv(side), iv(legs[0].1.p), iv(p[1]), iv(t0), iv(1), iv(2), s, iv(0), bv(&proposer), pre, suf], "account", seq);
    let mut rids = vec![];
    for (t, a) in legs {
        rids.push(attest_input(&mut d, t, a, t0 as u64)?);
    }
    d.genesis(OObj::C(CObj::PendingX(w)), account.value, 0, "pending claim");
    for (i, (t, _)) in legs.iter().enumerate() {
        d.obj(OObj::C(CObj::EpochTable(t.s)), t.value, Some(((1 + i) as u16, rids[i])), "epoch table");
    }
    d.fund(funder, change_to);
    Ok((d.finish()?, w))
}

/// Finalize a claim on an account (anyone): in 0 the claim, then the funding; (close) A at 0, change, B last;
/// (liquidation) the proposer's fee at 0, change, the winner last; (void) the restored account.
pub fn x_finalize(pending: &OLive<XPending>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, XSettlement)> {
    let k = pinned::xkit();
    let w = &pending.s;
    let id = id_of(pending, "claim")?;
    if !w.consistent(k) {
        return invalid("the account the claim carries is not the one its hashes bind: not a claim an account wrote");
    }
    let s = w.settlement(pending.value, pending.daa as i64);
    let mut d = start(rate);
    d.contract(pending, OWhich::C(CWhich::PendingX), w.redeem(k), "finalize", vec![], "pending claim", w.finalize_age(pending.daa as i64));
    let mut last: Option<(i64, [u8; 32], &str)> = None;
    match &s {
        XSettlement::Close { a, b } => {
            if *a > 0 {
                d.payout(*a, &w.a_key, "the owner");
            }
            if *b > 0 {
                last = Some((*b, w.b_key, "the counterparty"));
            }
        }
        XSettlement::Forfeit { fee, winner, rest } => {
            if *fee > 0 {
                d.payout(*fee, &w.proposer, "proposer's fee");
            }
            last = Some((*rest, *winner, "the winner"));
        }
        XSettlement::Restore { account, value } => {
            d.obj(OObj::C(CObj::Cross(*account)), *value as u64, Some((0, id)), "restored account");
        }
    }
    d.fund(funder, change_to);
    if let Some((v, key, label)) = last {
        d.last(v, &key, label, change_to);
    }
    Ok((d.finish()?, s))
}

/// A cure by the loser of a pending liquidation of an account (its key signs and funds): out 0 the cured account.
pub fn x_cure(pending: &OLive<XPending>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Cross)> {
    let k = pinned::xkit();
    let w = &pending.s;
    let id = id_of(pending, "claim")?;
    if !w.consistent(k) {
        return invalid("the account the claim carries is not the one its hashes bind: not a claim an account wrote");
    }
    let (c, v) = w.cure(pending.value).ok_or_else(|| Error::Invalid("no cure: the claim is a close".into()))?;
    let loser = if w.side == SIDE_A { w.a_key } else { w.b_key };
    let mut d = start(rate);
    d.contract(pending, OWhich::C(CWhich::PendingX), w.redeem(k), "cure", vec![Arg::Sig(loser)], "pending claim", 0);
    d.obj(OObj::C(CObj::Cross(c)), v as u64, Some((0, id)), "cured account");
    d.fund(funder, change_to);
    Ok((d.finish()?, c))
}

/// `addMargin` on an account: the owner of `side` signs, after quietDaa.
pub fn x_add_margin(account: &OLive<Cross>, side: i64, delta: i64, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Cross)> {
    let k = pinned::xkit();
    let c = &account.s;
    let id = id_of(account, "account")?;
    let next = c.added(side, delta).ok_or_else(|| Error::Invalid("the margin added must be positive, of side a or b, and within range".into()))?;
    let key = if side == SIDE_A { c.a_key } else { c.b_key };
    let mut d = start(rate);
    d.contract(account, OWhich::C(CWhich::Cross), c.redeem(k), "addMargin", vec![Arg::Sig(key), iv(side), iv(delta)], "account", c.t.quiet_daa as u64);
    d.obj(OObj::C(CObj::Cross(next)), (account.value as i64 + delta).max(next.floor()) as u64, Some((0, id)), "account");
    d.fund(funder, change_to);
    Ok((d.finish()?, next))
}

/// `keepAlive` on an account with a signed price of leg `leg`'s market (1 or 2): in 0 the account, 1 the epoch table.
pub fn x_keep_alive(account: &OLive<Cross>, leg: usize, epoch_table: &OLive<EpochTable>, a: &Attestation, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<(OBuilt, Cross)> {
    let k = pinned::xkit();
    let c = &account.s;
    let id = id_of(account, "account")?;
    if !(1..=2).contains(&leg) || c.legs[leg - 1].size == 0 {
        return invalid("the account has no such leg");
    }
    let l = &c.legs[leg - 1];
    if epoch_table.id.map(|h| h.as_bytes()) != Some(l.committee) || a.market != l.market {
        return invalid("the epoch table or the attestation is not of the leg's committee and market");
    }
    if epoch_table.s.spread_bps * SPREAD_MARGIN > c.t.g_bps {
        return invalid("the committee's spread leaves the account's tolerance g no margin");
    }
    let next = c.kept_alive(a.t0).ok_or_else(|| Error::Invalid(format!("a signed price at DAA {} does not move the lease ({}) by a quarter of a term: the first signed price that does is DAA {}", a.t0, c.expiry, c.keep_alive_from())))?;
    let mut d = start(rate);
    d.lock(a.t0 as u64);
    d.contract(account, OWhich::C(CWhich::Cross), c.redeem(k), "keepAlive", vec![iv(leg as i64), iv(1), iv(a.p), iv(a.t0)], "account", 0);
    let rid = attest_input(&mut d, epoch_table, a, a.t0 as u64)?;
    d.obj(OObj::C(CObj::Cross(next)), account.value, Some((0, id)), "account");
    d.obj(OObj::C(CObj::EpochTable(epoch_table.s)), epoch_table.value, Some((1, rid)), "epoch table");
    d.fund(funder, change_to);
    Ok((d.finish()?, next))
}

/// A lapse settle of an account (anyone): out 0 the owner, change, the counterparty last.
pub fn x_lapse_settle(account: &OLive<Cross>, funder: &[KeyUtxo], change_to: &[u8; 32], rate: u64) -> Result<OBuilt> {
    let k = pinned::xkit();
    let c = &account.s;
    let (a, b) = c.lapse_out(account.value);
    let mut d = start(rate);
    d.lock(c.lapse_from());
    d.contract(account, OWhich::C(CWhich::Cross), c.redeem(k), "lapseSettle", vec![], "account", 0);
    if a > 0 {
        d.payout(a, &c.a_key, "the owner");
    }
    d.fund(funder, change_to);
    if b > 0 {
        d.last(b, &c.b_key, "the counterparty", change_to);
    }
    d.finish()
}

/// A cooperative close of an account: both parties sign; the owner is paid `a_pay`, the counterparty the rest less
/// the fee (the account funds it).
pub fn x_coop_close(account: &OLive<Cross>, a_pay: u64, rate: u64) -> Result<OBuilt> {
    let k = pinned::xkit();
    let c = &account.s;
    if a_pay > account.value {
        return invalid("the owner cannot be paid more than the account holds");
    }
    let mut d = start(rate);
    d.contract(account, OWhich::C(CWhich::Cross), c.redeem(k), "coopClose", vec![Arg::Sig(c.a_key), Arg::Sig(c.b_key)], "account", 0);
    if a_pay > 0 {
        d.out(a_pay.max(VIABLE_OUTPUT_MIN), crate::tx::p2pk_spk(&c.a_key), None, "the owner", None);
    }
    let ch = d.out(0, crate::tx::p2pk_spk(&c.b_key), None, "the counterparty (the rest, less the fee)", None);
    d.change = Some(ch);
    d.finish()
}
