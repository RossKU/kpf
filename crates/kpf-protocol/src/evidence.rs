//! Transaction builders of the two-step evidence paths (DESIGN 3.3, 3.6, 4.4, 4.5d): `KpfPair.propose` (P5 unilateral
//! close, P6 liquidation) on up to 8 pairs acting on one evidence fill, `KpfPending.challenge` on up to 8 pending parts
//! acting on one counter-evidence fill (the two may share one block), and the evidence-free `KpfPending.cure` and
//! `KpfPending.finalize`.
//!
//! # What an evidence transaction contains
//!
//! Besides the pairs and pending parts, the transaction holds the fill of a live KOB `KobAsk` or `KobBid` (the evidence,
//! DESIGN 3.3), its counterparty and the KCC-20 token inputs and outputs the token program requires, so that every
//! input, KOB's and the token's included, passes its own script. KOB's own fill builder (`kob_protocol::build::build_batch`)
//! lives in a crate-private draft that takes only KOB's own order and key UTXOs, not a foreign pair input, so this module
//! lays the KOB side out itself from KOB's published artifacts ([`crate::artifacts::ask_base`] and friends).
//!
//! The counterparty is the executor (the funder), which needs the fewest inputs KOB's rules allow:
//!
//! - **ask evidence** (a long's liquidation, a short's close, the challenge of a long's close or of a short's
//!   liquidation): the executor *buys* the ask's tokens with its own KAS. Inputs `KobAsk` + its custody (the leader of the
//!   token transfer); the maker gets the KAS at the ask's own index, the executor receives the tokens as a P2PK holder.
//! - **bid evidence** (the other four): the executor *sells* tokens into the bid. Inputs `KobBid` + one P2PK token UTXO
//!   of the executor holding at least the fill (the leader, signed by the executor); the bid's maker gets the tokens at
//!   the bid's own index, the executor the KAS.
//!
//! # Layout (DESIGN 4.4; the covenants accept any order with the same rules)
//!
//! ```text
//! inputs   [0, B)        the block: pairs (`propose`) and pending parts (`challenge`), first pushes n_i, q = 0
//!          B             the evidence order (settle / fill; its first push is n_ev)
//!          B + 1         its token input: the ask's custody, or the executor's tokens sold to the bid
//!          B + 2 ..      the funding inputs (they pay the bonds, a partial proposal's carriers and the fee)
//! outputs  i             block member i: the pending part of a proposal, or the restored pair of a challenge
//!          B             ask: the maker's KAS; bid: the delivery of the tokens to the bid's maker
//!          B + 1         ask: the executor's tokens; bid: the executor's KAS
//!          then          pair remainders, the order's continuation, a custody rest or token change, the change
//!          M - 1 - B     a terminal bid's refund
//! ```
//!
//! Every party output holds the least value its covenant accepts; the executor's change takes the rest, less the fee.
//! The bond of a successful challenge is such a rest: it lands in the challenger's change.

use kaspa_consensus_core::Hash;
use silverscript_abi::ArtifactValue;

use crate::artifacts::pending_template;
use crate::build::{Funding, Opts};
use crate::error::{invalid, kas_str, Error, Result};
use crate::kob::*;
use crate::payoff::*;
use crate::pending::*;
use crate::state::*;
use crate::tx::*;

/// Most block members one evidence fill can serve (the block of DESIGN 4.4 is scanned over at most 8 inputs).
pub const MAX_BLOCK: usize = 8;

/// Storage mass (KIP-9) is `C * p^2 / value` per output, `p` the storage plurality (2 for a covenant output), and a
/// block takes 500,000 grams in all: a 0.02 KAS plain output alone uses all of it. The outputs this module chooses the
/// value of are therefore generous where a covenant leaves the choice open (the executor pays the difference).
/// A filler or an output of the executor's own: 1 KAS (10,000 grams).
pub const FILLER_VALUE: u64 = 100_000_000;
/// A token output the transaction creates (the executor's tokens, a delivery, a token change): 0.2 KAS (200,000 grams).
pub const TOKEN_OUT_VALUE: u64 = 20_000_000;
/// The least a plain payout to another party is made, 0.1 KAS (100,000 grams), even where its covenant asks for less.
pub const PARTY_FLOOR: u64 = 10_000_000;

/// What one pair of a proposal block claims (DESIGN 4.5d).
#[derive(Clone, Debug)]
pub enum Act {
    /// P5, kind 1: the owner of `side` (signature) proposes to close `n` at the adverse bound.
    Close { side: i64 },
    /// P6, kind 2: the loser `side` is liquidatable; the bond and the fee go to `proposer`.
    Liq { side: i64, proposer: [u8; 32] },
}

/// One pair of a proposal block: the part `n` of `pair` and its claim.
#[derive(Clone, Debug)]
pub struct Leg {
    pub pair: LivePair,
    pub n: i64,
    /// What the proposer gives up from its own payout to a guarantor that fast-settles the claim (DESIGN 4.5f):
    /// `0 ..= bond - 0.02 KAS`, written into the pending part.
    pub offer: i64,
    pub act: Act,
}

/// One member of an evidence block: a pair that proposes, a pending part that is challenged, or a guarantor's lock that
/// is disputed (DESIGN 4.5f; a lock is judged by the same counter-evidence a challenge of its claim would have needed).
#[derive(Clone, Debug)]
pub enum Member {
    Propose(Leg),
    Challenge(LivePending),
    Dispute(LiveLock),
}

/// The evidence order and the executor's side of its fill.
#[derive(Clone, Debug)]
pub struct EvidenceUse {
    pub order: Evidence,
    /// Base units the evidence fill takes. `None`: the least that covers `k * sum(n)` and KOB's own minimum-fill rules.
    pub fill: Option<i64>,
    /// Bid evidence: the executor's token UTXO sold into the bid (P2PK owner, the market's extension commitment, at
    /// least the fill). Ask evidence needs none.
    pub seller: Option<LiveToken>,
    /// The DAA score now, when known (the node's, or `--daa`): the evidence must still be live then.
    pub now: Option<u64>,
    /// Accept an order that is dead (soft-expired) at the DAA the transaction lands. For sweeping tests only: a dead
    /// order is a quote nobody can trade at any more (DESIGN 4.5c, review 3 F1).
    pub allow_dead: bool,
}

/// Where the KOB side put things, for the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FillFacts {
    pub fill: i64,
    pub quote: i64,
    /// KAS the maker of the evidence order receives (ask) or the executor receives (bid).
    pub kas: u64,
}

fn side_name(side: i64) -> &'static str {
    if side == LONG {
        "long"
    } else {
        "short"
    }
}

/// `KpfPair.propose(nb, qb, kind, s, side, ev, tk, fb, pk, k, offer, pPre, pSuf)`.
#[allow(clippy::too_many_arguments)]
pub fn propose_args(n: i64, q: i64, kind: i64, s: Arg, side: i64, ev: usize, tk: i64, fb: i64, pk: &[u8; 32], k: i64, offer: i64) -> Vec<Arg> {
    let t = pending_template();
    vec![bv(&sm8(n)), bv(&sm8(q)), iv(kind), s, iv(side), iv(ev as i64), iv(tk), iv(fb), bv(pk), iv(k), iv(offer), bv(&t.prefix), bv(&t.suffix)]
}

/// `KpfPending.challenge(nb, qb, ev, tk)`.
pub fn challenge_args(n: i64, q: i64, ev: usize, tk: i64) -> Vec<Arg> {
    vec![bv(&sm8(n)), bv(&sm8(q)), iv(ev as i64), iv(tk)]
}

/// `KpfLock.dispute(nb, qb, ev, tk)` (DESIGN 4.5f).
pub fn dispute_args(n: i64, q: i64, ev: usize, tk: i64) -> Vec<Arg> {
    challenge_args(n, q, ev, tk)
}

/// `KpfPending.guarantee(j, gk, lPre, lSuf)` (DESIGN 4.5f): the lock at output `j`, owned by `gk`.
pub fn guarantee_args(j: usize, gk: &[u8; 32]) -> Vec<Arg> {
    let t = crate::artifacts::lock_template();
    vec![iv(j as i64), bv(gk), bv(&t.prefix), bv(&t.suffix)]
}

/// The least fill that covers `kBps * sum / 10^4` (the evidence block rule of DESIGN 4.4, exact as in the contracts).
fn fill_for(k_bps: i64, sum: i64) -> i64 {
    (sum / BPS) * k_bps + ((sum % BPS) * k_bps + BPS - 1) / BPS
}

// ---------------------------------------------------------------- planning the KOB fill

struct AskPlan {
    fill: i64,
    all_in: i64,
    rest: i64,
}

fn plan_ask(o: &LiveAsk, need: i64, want: Option<i64>) -> Result<AskPlan> {
    let a = &o.a;
    let cap = a.amount_left;
    if need > cap {
        return invalid(format!(
            "the ask sells {cap} base units, fewer than the {need} the evidence must fill (k x the sum of the parts); act on a smaller part or find a larger order"
        ));
    }
    let end_only = a.tif != 0;
    let rate = a.price - a.tip;
    let mut fill = match want {
        Some(w) => w,
        None if end_only => cap,
        None => need.max(a.min_fill).min(cap),
    };
    if want.is_none() && fill < cap && rate > 0 {
        // A partial fill pays the maker alone: its proceeds must clear the plain-output dust bound.
        let dust_n = ((PLAIN_DUST as i128 * a.scale as i128 + rate as i128 - 1) / rate as i128).min(cap as i128) as i64;
        fill = fill.max(dust_n);
    }
    if fill < need {
        return invalid(format!("the evidence fill {fill} is below the {need} the block needs (k x the sum of the parts)"));
    }
    if fill <= 0 || fill > cap {
        return invalid(format!("the evidence fill {fill} must be in 1..={cap} (what the ask sells)"));
    }
    if !(fill >= a.min_fill || fill == cap) {
        return invalid(format!("the evidence fill {fill} is below the ask's minimum fill {} and does not take all that is left", a.min_fill));
    }
    if a.max_fill > 0 && fill > a.max_fill {
        return invalid(format!("the ask caps a fill at {} base units (maxFill) and the block needs {fill}", a.max_fill));
    }
    if end_only && fill != cap {
        return invalid("an IOC or FOK ask ends with its fill: the evidence fill must take everything it sells");
    }
    if rate < 0 {
        return invalid("the ask's price is below its tip");
    }
    let all_in = q(fill, rate, a.scale, a.scale - 1);
    if fill < cap && all_in < PLAIN_DUST {
        return invalid(format!("a partial fill of this ask pays its maker {all_in} sompi, below the 0.02 KAS minimum output; fill it entirely or fill more"));
    }
    Ok(AskPlan { fill, all_in, rest: cap - fill })
}

struct BidPlan {
    fill: i64,
    used: i64,
    all_in: i64,
    /// The order continues (GTC with buying power left): its continuation value.
    cont: Option<u64>,
    /// A terminal fill returns this much to the maker as its own output (>= REFUND_MIN), else 0.
    refund: u64,
}

fn plan_bid(o: &LiveBid, need: i64, want: Option<i64>) -> Result<BidPlan> {
    let b = &o.b;
    let bp = b.buying_power(o.value);
    if need > bp {
        return invalid(format!(
            "the bid can buy {bp} base units with its KAS, fewer than the {need} the evidence must fill (k x the sum of the parts); act on a smaller part or find a larger order"
        ));
    }
    let top = if b.max_fill > 0 { bp.min(b.max_fill) } else { bp };
    let cands: Vec<i64> = match want {
        Some(w) => vec![w],
        None => vec![need.max(1), need.max(b.min_fill), top],
    };
    let mut why = String::new();
    for n in cands {
        let mut bad = |m: String| {
            if why.is_empty() {
                why = m;
            }
        };
        if n <= 0 || n > bp || n < need {
            bad(format!("the evidence fill {n} must be in {need}..={bp} (what the block needs, what the bid can buy)"));
            continue;
        }
        if b.max_fill > 0 && n > b.max_fill {
            bad(format!("the bid caps a fill at {} base units (maxFill)", b.max_fill));
            continue;
        }
        let used = b.used(n);
        let left = o.value as i64 - used;
        let can = b.can_continue(left);
        if n < b.min_fill && can {
            bad(format!("the fill {n} is below the bid's minimum fill {} and would leave it able to continue", b.min_fill));
            continue;
        }
        if b.tif == 2 && can {
            bad("an FOK bid needs a fill that uses up its buying power".into());
            continue;
        }
        let all_in = b.all_in(n);
        if b.tif == 0 && can {
            let cont = left - b.delivery_carrier;
            if cont < COVENANT_OUTPUT_MIN as i64 {
                bad(format!("this fill would leave the bid a continuation of {cont} sompi, below the 0.08 KAS minimum covenant output"));
                continue;
            }
            return Ok(BidPlan { fill: n, used, all_in, cont: Some(cont as u64), refund: 0 });
        }
        let rem = o.value as i64 - all_in - b.delivery_carrier;
        return Ok(BidPlan { fill: n, used, all_in, cont: None, refund: if rem >= REFUND_MIN { rem as u64 } else { 0 } });
    }
    invalid(why)
}

// ---------------------------------------------------------------- the builder

/// An output under assembly: its value, what it is, and its label.
struct Pend {
    value: u64,
    kind: Kind,
    label: String,
}

enum Kind {
    Plain([u8; 32]),
    Pair(Pair, (u16, Hash)),
    Kob(KobObj, Option<(u16, Hash)>),
    Pending(Pending, (u16, Hash)),
    /// The burn output of a dispute (DESIGN 4.5f).
    Burn,
}

fn emit(d: &mut Draft, p: Pend) -> usize {
    match p.kind {
        Kind::Plain(k) => d.add_plain(p.value, &k, p.label),
        Kind::Pair(pr, b) => d.add_pair_out(&pr, p.value, Some(b), p.label),
        Kind::Kob(o, b) => d.add_kob_out(o, p.value, b, p.label),
        Kind::Pending(w, b) => d.add_pending_out(&w, p.value, Some(b), p.label),
        Kind::Burn => d.add_out(p.value, crate::lock::burn_spk(), None, p.label),
    }
}

fn filler(key: &[u8; 32], what: &str) -> Pend {
    Pend { value: FILLER_VALUE, kind: Kind::Plain(*key), label: format!("filler ({what})") }
}

fn plain(value: u64, key: &[u8; 32], label: impl Into<String>) -> Pend {
    let value = value.max(PARTY_FLOOR);
    Pend { value, kind: Kind::Plain(*key), label: label.into() }
}


/// The transaction of one evidence fill and the block that acts on it: proposals of pairs ([`Act`]) and challenges of
/// pending parts, in any mix the covenants accept. The executor is `funding.change_key`: it receives the tokens an ask
/// evidence sells it, or the KAS a bid evidence pays it, funds the bonds (and a partial proposal's carriers), receives
/// the bond of every pending part it refutes, and pays the network fee.
pub fn evidence_tx(members: &[Member], eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<Built> {
    let (built, _) = evidence_tx_facts(members, eu, funding, opts)?;
    Ok(built)
}

/// [`evidence_tx`], also returning what the KOB side does.
pub fn evidence_tx_facts(members: &[Member], eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<(Built, FillFacts)> {
    evidence_tx_inner(members, eu, funding, opts, true)
}

/// Builds the transaction even for an order [`check_evidence`] rejects or a claim the checks refuse, so that tests can
/// show that the contracts refuse what the check calls a problem. Never use it to send anything.
#[doc(hidden)]
pub fn evidence_tx_unchecked(members: &[Member], eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<Built> {
    evidence_tx_inner(members, eu, funding, opts, false).map(|(b, _)| b)
}

fn valid_part(p: &Pair, n: i64) -> bool {
    n > 0 && n <= p.size && n >= p.m.min_size && (p.size == n || p.size - n >= p.m.min_size)
}

/// Why a proposal of `act` on the part `n` of `p` at the evidence quote is refused before anything is built.
pub fn check_claim(p: &Pair, n: i64, act: &Act, is_ask: bool, quote: i64, ev_name: &str) -> Result<()> {
    p.validate()?;
    if !valid_part(p, n) {
        return invalid(format!("n = {n} is not a valid part of size {} (a part and a remainder are each 0 or at least minSize {})", p.size, p.m.min_size));
    }
    match act {
        Act::Close { side } => {
            if *side != LONG && *side != SHORT {
                return invalid("side must be long or short");
            }
            if (*side == LONG) == is_ask {
                return invalid(format!(
                    "the {} closes at a resting {} (a long at a bid, a short at an ask), but the evidence is {} {ev_name}",
                    side_name(*side),
                    if *side == LONG { "bid" } else { "ask" },
                    if is_ask { "an" } else { "a" },
                ));
            }
        }
        Act::Liq { side, .. } => {
            if *side != LONG && *side != SHORT {
                return invalid("side must be long or short");
            }
            if p.long_key == p.short_key {
                return invalid(
                    "both sides of this pair have one owner (a forfeited part, or a side bought by the other owner): nobody is owed a liquidation, and KpfPair.propose refuses one (DESIGN 4.5e)",
                );
            }
            if (*side == LONG) != is_ask {
                return invalid(format!(
                    "a {} is liquidated by {} (a long by an ask, a short by a bid), but the evidence is {} {ev_name}",
                    side_name(*side),
                    if *side == LONG { "an ask" } else { "a bid" },
                    if is_ask { "an" } else { "a" },
                ));
            }
            if quote < 0 || quote > p.m.price_max {
                return invalid(format!("the evidence quote {quote} is outside 0..=priceMax {}: it cannot liquidate", p.m.price_max));
            }
            let trig = if *side == LONG { long_liquidatable(p, quote) } else { short_liquidatable(p, quote) };
            if !trig {
                return invalid(format!(
                    "the {} is not liquidatable at the {ev_name} {} KAS (its trigger is {} KAS: {})",
                    side_name(*side),
                    kas_str(quote as i128),
                    kas_str(trigger_of(p, *side) as i128),
                    if *side == LONG { "a long needs an ask below it" } else { "a short needs a bid above it" }
                ));
            }
        }
    }
    Ok(())
}

/// Whether `lw` is the pending part a genuine proposal of the pinned `KpfPair` writes (review 4, F4; DESIGN 4.5e): its
/// restore, cure and forfeit hashes match the pair its own state describes, and it holds at least the part's range and
/// the refund, `q(n, bs - bl) + refund`, which every genuine part holds (`KpfPair.propose`: `v + B` with `v >= q + C`
/// when `n = s`, `q + C + B` with `refund = B + C` when `n < s`). A look-alike that fails either is refused: acting on it
/// would make the user's funding pay what the part does not hold. Its market block must also pass the bounds every order
/// checks (`Market::validate` = `KpfOpen.checkMarket` = `KpfPending.checkMarket`; review 5 F2, DESIGN 4.5g): a genuine
/// part's market is its pair's, which an order checked, and on a look-alike with impossible parameters (a negative
/// mmrBps, a gBps above 10^4) every statement about what refutes or disputes the claim would be wrong.
pub fn check_pending(lw: &LivePending) -> Result<()> {
    let w = &lw.w;
    if let Err(e) = w.m.validate() {
        return invalid(format!(
            "the pending part's market block fails the bounds every order checks ({e}): no real pair has it, so it is a look-alike, and what refutes or disputes its claim cannot be judged"
        ));
    }
    if !w.hashes_match() {
        return invalid(
            "the pending part's restore, cure or forfeit hash does not match the pair its own state describes: it was not proposed by the pinned KpfPair",
        );
    }
    let least = w.least_value();
    if (lw.value as i128) < least as i128 {
        return invalid(format!(
            "the pending part holds {} KAS, less than the {} KAS every genuine part of this claim holds (its range q(n, bs - bl) and the refund): it is a look-alike, not a proposal of a real pair; acting on it would make your funding pay the difference",
            kas_str(lw.value as i128),
            kas_str(least as i128)
        ));
    }
    Ok(())
}

/// [`check_pending`], and that `lw` was cut from the user's pair `pair` (an object file of the pair, live or already spent
/// by the proposal): the same covenant id (a pending part shares its pair's id) and the same two keys.
pub fn check_pending_of(lw: &LivePending, pair_id: Hash, pair: &Pair) -> Result<()> {
    check_pending(lw)?;
    if lw.id != pair_id {
        return invalid(format!(
            "the pending part's covenant id {} is not the pair's {}: it was not cut from this pair (a part and its pair share one id)",
            lw.id, pair_id
        ));
    }
    if lw.w.long_key != pair.long_key || lw.w.short_key != pair.short_key {
        return invalid("the pending part names other owners than the pair: it was not cut from this pair");
    }
    Ok(())
}

/// Why a challenge of `w` at the counter quote is refused before anything is built.
pub fn check_challenge(w: &Pending, is_ask: bool, quote: i64, ev_name: &str) -> Result<()> {
    if w.counter_is_ask() != is_ask {
        return invalid(format!(
            "this claim is refuted by {} (a long's close at a bid and a short's liquidation by an ask, the others by a bid), but the evidence is {} {ev_name}",
            if w.counter_is_ask() { "an ask" } else { "a bid" },
            if is_ask { "an" } else { "a" }
        ));
    }
    if !w.contradicts(quote) {
        let bound = w.contradiction_bound().map(|b| format!("{} {} KAS", if w.counter_is_ask() { "at most" } else { "at least" }, kas_str(b as i128)));
        return invalid(format!(
            "the {ev_name} {} KAS does not contradict the claim by the margin g = {} bps: a challenge needs {} {}",
            kas_str(quote as i128),
            w.m.g_bps,
            if w.counter_is_ask() { "an ask" } else { "a bid" },
            bound.unwrap_or_else(|| "a quote no order can have".into())
        ));
    }
    if w.restore_pair().is_none() {
        return invalid("the pending part's restore hash does not match the pair its own state describes: it was not proposed by the pinned KpfPair");
    }
    Ok(())
}

/// Whether `ll` is a lock `KpfPending.guarantee` can have written: status 1, a market that passes the bounds, a cover the
/// lock covers. A lock is created only in the transaction that spends its pending part, so whether it is the lock of a
/// real claim can be told only from that transaction (`kpf watch` checks it from object files); a look-alike is the KAS of
/// whoever made it and can only pay its own victim or owner.
pub fn check_lock(ll: &LiveLock) -> Result<()> {
    let l = &ll.l;
    if l.status != STATUS_LOCK {
        return invalid(format!("the lock has status {}, not 1 (a lock): it was not written by KpfPending.guarantee", l.status));
    }
    l.m.validate()?;
    if l.cover < 0 || (ll.value as i128) < crate::lock::ratio_up(l.cover, l.m.lock_bps).max(crate::lock::LOCK_FLOOR) as i128 {
        return invalid(format!(
            "the lock holds {} KAS, less than lambda x its cover ({} KAS): it was not written by a guarantee",
            kas_str(ll.value as i128),
            kas_str(l.cover as i128)
        ));
    }
    Ok(())
}

/// Why a dispute of `ll` at the counter quote of `ev` is refused before anything is built: the counter-evidence must be of
/// the opposite kind, contradict the claim by g, and have finished its rest (on the clock P) by the window's end
/// `F + disputeDaa - minRestDaa`.
pub fn check_dispute(ll: &LiveLock, ev: &Evidence, rest_end: u64) -> Result<()> {
    let l = &ll.l;
    let (is_ask, quote) = (ev.is_ask(), ev.quote());
    if l.counter_is_ask() != is_ask {
        return invalid(format!(
            "this claim is refuted by {}, but the evidence is {} {}",
            if l.counter_is_ask() { "an ask" } else { "a bid" },
            if is_ask { "an" } else { "a" },
            ev.name()
        ));
    }
    if !l.contradicts(quote) {
        let bound = l.contradiction_bound().map(|b| format!("{} {} KAS", if l.counter_is_ask() { "at most" } else { "at least" }, kas_str(b as i128)));
        return invalid(format!(
            "the {} {} KAS does not contradict the claim by the margin g = {} bps: a dispute needs {} {}",
            ev.name(),
            kas_str(quote as i128),
            l.m.g_bps,
            if l.counter_is_ask() { "an ask" } else { "a bid" },
            bound.unwrap_or_else(|| "a quote no order can have".into())
        ));
    }
    let deadline = ll.rest_deadline();
    if rest_end as i128 > deadline as i128 {
        return invalid(format!(
            "the counter order finishes its rest at DAA {rest_end}, after the dispute window of this lock ends for evidence (DAA {deadline} = F + disputeDaa - minRestDaa): the lock is no longer liable for it"
        ));
    }
    Ok(())
}

fn evidence_tx_inner(members: &[Member], eu: &EvidenceUse, funding: &Funding, opts: &Opts, checked: bool) -> Result<(Built, FillFacts)> {
    if members.is_empty() {
        return invalid("nothing to do: no pair or pending part given");
    }
    if members.len() > MAX_BLOCK {
        return invalid(format!("an evidence fill serves at most {MAX_BLOCK} block members (the block scan is unrolled to 8); {} given", members.len()));
    }
    let m = match &members[0] {
        Member::Propose(g) => g.pair.p.m.clone(),
        Member::Challenge(w) => w.w.m.clone(),
        Member::Dispute(l) => l.l.m.clone(),
    };
    supports_token_program(&m).map_err(Error::Invalid)?;
    let ev = &eu.order;
    let quote = ev.quote();
    let is_ask = ev.is_ask();
    let exec = funding.change_key;
    // The evidence against the market (time-free: the lock time is derived below).
    let rep = check_evidence(ev, &m, None);
    if checked && !rep.problems.is_empty() {
        return invalid(format!("this order cannot serve as evidence: {}", rep.problems.join("; ")));
    }

    // The members.
    let mut seen = std::collections::BTreeSet::new();
    let (mut sum, mut any_pair, mut any_pending) = (0i64, false, false);
    let mut lock = rep.rest_end;
    for (j, g) in members.iter().enumerate() {
        let (mm, id, n, clock) = match g {
            Member::Propose(l) => (&l.pair.p.m, Some(l.pair.id), l.n, l.pair.clock()),
            // The counter-evidence clock is the pending UTXO's own DAA score P (DESIGN 4.5d).
            Member::Challenge(w) => (&w.w.m, Some(w.id), w.w.n, w.daa as i64),
            // A lock's clock is the P its claim had (DESIGN 4.5f); a lock has no covenant id.
            Member::Dispute(l) => (&l.l.m, None, l.l.n, l.l.clock),
        };
        if *mm != m {
            return invalid(format!("block member {j} is of another market than member 0"));
        }
        if id.is_some_and(|id| !seen.insert(id)) {
            return invalid(format!(
                "block member {j} has the covenant id of an earlier member (the same pair twice, or a pending part and a remainder of one split, which share an id): every KpfPair and KpfPending must be the only input of its covenant id"
            ));
        }
        let on = check_evidence_on(ev, &m, Some(clock), eu.now);
        if checked && !on.problems.is_empty() {
            return invalid(format!("block member {j}: this order cannot serve as evidence on its clock (DAA {clock}): {}", on.problems.join("; ")));
        }
        lock = lock.max(on.rest_end);
        match g {
            Member::Propose(l) => {
                any_pair = true;
                if checked {
                    check_claim(&l.pair.p, l.n, &l.act, is_ask, quote, ev.name()).map_err(|e| Error::Invalid(format!("pair {j}: {e}")))?;
                    let most = bond_of(&l.pair.p, l.n) - PLAIN_DUST;
                    if l.offer < 0 || l.offer > most {
                        return invalid(format!(
                            "pair {j}: the offer to a guarantor must be in 0..={} KAS (the bond less 0.02 KAS: the proposer is still paid at least that), not {} KAS",
                            kas_str(most as i128),
                            kas_str(l.offer as i128)
                        ));
                    }
                }
            }
            Member::Challenge(w) => {
                any_pending = true;
                if checked {
                    check_pending(w).map_err(|e| Error::Invalid(format!("pending part {j}: {e}")))?;
                    check_challenge(&w.w, is_ask, quote, ev.name()).map_err(|e| Error::Invalid(format!("pending part {j}: {e}")))?;
                }
            }
            Member::Dispute(l) => {
                any_pending = true;
                if checked {
                    check_lock(l).map_err(|e| Error::Invalid(format!("lock {j}: {e}")))?;
                    check_dispute(l, ev, on.rest_end).map_err(|e| Error::Invalid(format!("lock {j}: {e}")))?;
                }
            }
        }
        sum = sum.checked_add(n).ok_or_else(|| Error::Invalid("the parts add up beyond 63 bits".into()))?;
    }
    if lock as i64 >= LOCK_TIME_THRESHOLD {
        return invalid("the required lock time is above the lock-time threshold");
    }
    // A dead order (soft-expired at the DAA the transaction can land) is refused by default (DESIGN 4.5c).
    let at = lock.max(eu.now.unwrap_or(0));
    if checked && !eu.allow_dead && at as i64 > ev.live_until() {
        let r = check_evidence_on(ev, &m, None, Some(at));
        return invalid(r.dead.join("; "));
    }
    let mut need = 0;
    if any_pair {
        need = need.max(fill_for(m.k_bps, sum));
    }
    if any_pending {
        need = need.max(fill_for(m.kc_bps, sum));
    }

    // The KOB side.
    let b_cnt = members.len();
    let ev_i = b_cnt;
    let tk_i = b_cnt + 1;
    let n_in = b_cnt + 2 + funding.utxos.len();
    let idx0 = b_cnt + 2; // the first output after the positional ones
    let mut extras: Vec<Pend> = vec![];
    let mut tail: Vec<Option<Pend>> = vec![]; // by input index i: lands at output M - 1 - i
    let set_tail = |tail: &mut Vec<Option<Pend>>, i: usize, p: Pend| {
        while tail.len() <= i {
            tail.push(None);
        }
        tail[i] = Some(p);
    };
    let token_cov = m.token_cov_id;
    let tk_arg = if is_ask { tk_i as i64 } else { -1 };

    let mut pos_members: Vec<Pend> = vec![];
    let mut member_ins: Vec<(usize, Vec<Arg>, String)> = vec![];
    for (j, g) in members.iter().enumerate() {
        match g {
            Member::Propose(l) => {
                // The outputs that do not change hands keep the clock S (DESIGN 4.5c); propose_out is given the clocked pair.
                let lp = l.pair.clocked();
                let (kind, side, s, pk) = match &l.act {
                    Act::Close { side } => (KIND_CLOSE, *side, Arg::Sig(if *side == LONG { lp.long_key } else { lp.short_key }), [0u8; 32]),
                    Act::Liq { side, proposer } => (KIND_LIQ, *side, bv(&[]), *proposer),
                };
                let po = propose_out(&lp, l.pair.value, l.n, kind, side, quote, pk, ev.live_until(), l.offer);
                member_ins.push((
                    j,
                    propose_args(l.n, 0, kind, s, side, ev_i, tk_arg, po.fb, &pk, 0, l.offer),
                    format!("pair {j} (propose {} of the {})", if kind == KIND_CLOSE { "close" } else { "liquidation" }, side_name(side)),
                ));
                pos_members.push(Pend {
                    value: po.pending_value as u64,
                    kind: Kind::Pending(po.pending, (j as u16, l.pair.id)),
                    label: format!("pair {j}: pending part (bond {} KAS)", kas_str(bond_of(&lp, l.n) as i128)),
                });
                if let Some((r, rv)) = po.rest {
                    extras.push(Pend { value: rv as u64, kind: Kind::Pair(r, (j as u16, l.pair.id)), label: format!("pair {j} remainder") });
                }
            }
            Member::Challenge(w) => {
                let r = w.w.restore_pair().ok_or_else(|| Error::Invalid(format!("pending part {j}: its restore hash does not match its own state")))?;
                member_ins.push((j, challenge_args(w.w.n, 0, ev_i, tk_arg), format!("pending part {j} (challenge)")));
                pos_members.push(Pend {
                    value: w.w.restore_value(w.value).max(0) as u64,
                    kind: Kind::Pair(r, (j as u16, w.id)),
                    label: format!("pending part {j}: restored pair (the bond {} KAS goes to the challenger)", kas_str(w.w.bond as i128)),
                });
            }
            Member::Dispute(l) => {
                // DESIGN 4.5f: the victim at this member's own index, the burn at the reverse slot M - 1 - j, the reward
                // (what no output must hold) in the executor's change.
                member_ins.push((j, dispute_args(l.l.n, 0, ev_i, tk_arg), format!("lock {j} (dispute)")));
                let pay = l.l.victim_pay();
                pos_members.push(if pay > 0 {
                    plain(pay as u64, &l.l.victim, format!("lock {j}: restitution to the wronged party"))
                } else {
                    filler(&exec, "a dispute with no restitution")
                });
                let burn = l.l.burn(l.value);
                if burn > 0 {
                    // A burn of a small lock is raised to the party floor (storage mass, KIP-9): the executor pays the difference.
                    set_tail(&mut tail, j, Pend { value: (burn as u64).max(PARTY_FLOOR), kind: Kind::Burn, label: format!("lock {j}: burned (the lock less the restitution and the reward)") });
                }
            }
        }
    }
    // The evidence fill.
    let mut ev_pos: Vec<Pend> = vec![];
    let (ev_input, tk_input);
    let facts;
    match ev {
        Evidence::Ask { order, custody } => {
            let plan = plan_ask(order, need, eu.fill)?;
            let a = &order.a;
            let ends = plan.rest == 0;
            let rest_at = idx0 + extras.len() + usize::from(!ends) * 0; // the order's continuation comes first
            let tok_out = if ends { tk_i } else { rest_at + 1 };
            ev_input = (
                KobWhich::Ask,
                order.entry(),
                order.op,
                order.a.redeem(),
                "settle",
                vec![bv(&sm8(plan.fill)), iv(tk_i as i64), iv(tok_out as i64), iv(a.active_from)],
                "evidence ask (settle)".to_string(),
            );
            let bought = TokenState::p2pk(plan.fill, exec, custody.t.ext);
            let mut next = vec![bought.value()];
            if !ends {
                let rest_state = TokenState { amount: plan.rest, owner: order.id.as_bytes(), scheme: SCHEME_COVID, borrow: 0, guard: custody.t.guard, ext: custody.t.ext };
                next.push(rest_state.value());
            }
            tk_input = (
                KobWhich::Token,
                custody.entry(),
                custody.op,
                custody.t.redeem(),
                "transfer",
                vec![Arg::V(ArtifactValue::Array(next)), bv(&[0x00])],
                "evidence ask custody (token transfer)".to_string(),
            );
            let maker_value = if ends { plan.all_in as u64 + order.value + custody.value } else { plan.all_in as u64 };
            ev_pos.push(plain(maker_value, &a.maker, "evidence ask: maker paid"));
            ev_pos.push(Pend {
                value: TOKEN_OUT_VALUE,
                kind: Kind::Kob(KobObj::Token(bought), Some((tk_i as u16, token_cov))),
                label: "tokens bought (executor)".into(),
            });
            if !ends {
                let mut cont = a.clone();
                cont.amount_left = plan.rest;
                extras.push(Pend { value: order.value, kind: Kind::Kob(KobObj::Ask(cont), Some((ev_i as u16, order.id))), label: "evidence ask continuation".into() });
                let rest_state = TokenState { amount: plan.rest, owner: order.id.as_bytes(), scheme: SCHEME_COVID, borrow: 0, guard: custody.t.guard, ext: custody.t.ext };
                extras.push(Pend { value: custody.value, kind: Kind::Kob(KobObj::Token(rest_state), Some((tk_i as u16, token_cov))), label: "evidence ask custody rest".into() });
            }
            facts = FillFacts { fill: plan.fill, quote, kas: plan.all_in as u64 };
        }
        Evidence::Bid { order } => {
            let plan = plan_bid(order, need, eu.fill)?;
            let b = &order.b;
            let seller = eu.seller.as_ref().ok_or_else(|| {
                Error::Invalid(
                    "bid evidence needs tokens to sell into the bid: pass a P2PK-owned token UTXO of the market's token holding at least the fill (the executor's own tokens)".into(),
                )
            })?;
            if seller.id != token_cov {
                return invalid("the seller's token UTXO is not of the market's token (its covenant id differs)");
            }
            if seller.t.scheme != SCHEME_P2PK || seller.t.ext != m.ext {
                return invalid("the seller's token UTXO must be owned by a P2PK key and carry the market's extension commitment");
            }
            if seller.t.amount < plan.fill {
                return invalid(format!("the seller's token UTXO holds {} base units, the evidence fill needs {}", seller.t.amount, plan.fill));
            }
            ev_input = (
                KobWhich::Bid,
                order.entry(),
                order.op,
                order.b.redeem(),
                "fill",
                vec![bv(&sm8(plan.fill)), iv(tk_i as i64), iv(b.active_from)],
                "evidence bid (fill)".to_string(),
            );
            let delivered = TokenState::p2pk(plan.fill, b.maker, b.ext);
            let mut next = vec![delivered.value()];
            let change_state = (seller.t.amount > plan.fill).then(|| TokenState::p2pk(seller.t.amount - plan.fill, seller.t.owner, seller.t.ext));
            if let Some(c) = &change_state {
                next.push(c.value());
            }
            tk_input = (
                KobWhich::Token,
                seller.entry(),
                seller.op,
                seller.t.redeem(),
                "transfer",
                vec![Arg::V(ArtifactValue::Array(next)), Arg::LeaderSig(seller.t.owner)],
                "evidence bid: executor's tokens (token transfer)".to_string(),
            );
            let delivery_value = match plan.cont {
                Some(_) => (b.delivery_carrier + plan.used - plan.all_in).max(0) as u64,
                None if plan.refund > 0 => b.delivery_carrier.max(0) as u64,
                None => (order.value as i64 - plan.all_in).max(0) as u64,
            };
            ev_pos.push(Pend {
                value: delivery_value.max(TOKEN_OUT_VALUE),
                kind: Kind::Kob(KobObj::Token(delivered), Some((tk_i as u16, token_cov))),
                label: "evidence bid: tokens delivered to its maker".into(),
            });
            ev_pos.push(plain((plan.all_in as u64).max(FILLER_VALUE), &exec, "evidence bid: KAS to the executor"));
            if let Some(cv) = plan.cont {
                extras.push(Pend { value: cv, kind: Kind::Kob(KobObj::Bid(b.clone()), Some((ev_i as u16, order.id))), label: "evidence bid continuation".into() });
            }
            let change_value = seller.value.max(TOKEN_OUT_VALUE);
            if let Some(c) = change_state {
                extras.push(Pend { value: change_value, kind: Kind::Kob(KobObj::Token(c), Some((tk_i as u16, token_cov))), label: "executor's token change".into() });
            }
            if plan.refund > 0 {
                set_tail(&mut tail, ev_i, plain(plan.refund, &b.maker, "evidence bid: rest of the escrow to its maker"));
            }
            facts = FillFacts { fill: plan.fill, quote, kas: plan.all_in as u64 };
        }
    }

    // Assemble: inputs in index order, then outputs in layout order.
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    d.lock = lock;
    for (j, args, label) in member_ins {
        match &members[j] {
            Member::Propose(l) => d.add_pair_in(&l.pair, "propose", args, label),
            Member::Challenge(w) => d.add_pending_in(w, "challenge", args, label),
            Member::Dispute(l) => d.add_lock_in(l, "dispute", args, label),
        };
    }
    let (w, e, op, redeem, name, args, label) = ev_input;
    let ev_idx = d.add_kob_in(op, e, w, redeem, name, args, label);
    debug_assert_eq!(ev_idx, ev_i);
    if ev.interval() > 0 {
        d.ins[ev_idx].sequence = ev.interval() as u64;
    }
    let (w, e, op, redeem, name, args, label) = tk_input;
    let tk_idx = d.add_kob_in(op, e, w, redeem, name, args, label);
    debug_assert_eq!(tk_idx, tk_i);
    let first_fund = d.fund_inputs(funding)?;
    debug_assert_eq!(first_fund, n_in - funding.utxos.len());

    for p in pos_members {
        emit(&mut d, p);
    }
    for p in ev_pos {
        emit(&mut d, p);
    }
    debug_assert_eq!(d.outs.len(), idx0);
    for p in extras {
        emit(&mut d, p);
    }
    d.add_change(&funding.change_key);
    // The reverse slots must lie at or above the input count (DESIGN 4.4): pad before the tail if needed.
    while d.outs.len() < n_in && !tail.is_empty() {
        d.add_plain(FILLER_VALUE, &exec, "filler (keeps the reverse slots clear of the inputs)");
    }
    for i in (0..tail.len()).rev() {
        let p = tail[i].take().unwrap_or_else(|| filler(&exec, "reverse slot"));
        emit(&mut d, p);
    }
    let built = d.finish()?;
    Ok((built, facts))
}

// ---------------------------------------------------------------- the paths

/// Propose a P5 unilateral close (kind 1): the owner of `side` of `l` (signature) closes `n` at the adverse bound, a long
/// at a resting bid and a short at a resting ask. The executor funds the bond (and the carrier of a part); the closer is
/// paid at finalize, unless the claim is refuted.
pub fn propose_close(l: &LivePair, side: i64, n: i64, eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<Built> {
    evidence_tx(&[Member::Propose(Leg { pair: l.clone(), n, offer: 0, act: Act::Close { side } })], eu, funding, opts)
}

/// Propose a P6 liquidation (kind 2) of up to 8 pairs, whole or in parts, on one evidence: an ask liquidates longs, a
/// bid shorts. At finalize the winner keeps the loser's side (forfeit and keep) and `proposer` gets the bond and the fee.
pub fn propose_liquidate(parts: &[(LivePair, i64)], proposer: &[u8; 32], eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<Built> {
    let side = if eu.order.is_ask() { LONG } else { SHORT };
    let m: Vec<Member> = parts.iter().map(|(p, n)| Member::Propose(Leg { pair: p.clone(), n: *n, offer: 0, act: Act::Liq { side, proposer: *proposer } })).collect();
    evidence_tx(&m, eu, funding, opts)
}

/// Challenge up to 8 pending parts on one counter-evidence fill: each returns to its restore state and its bond goes to
/// the executor (the challenger).
pub fn challenge(parts: &[LivePending], eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<Built> {
    let m: Vec<Member> = parts.iter().map(|w| Member::Challenge(w.clone())).collect();
    evidence_tx(&m, eu, funding, opts)
}

/// Cure (`KpfPending.cure`): the loser's owner moves its bound to the cure bound, funding the added margin (and the
/// carrier of a part), and the proposer is repaid its refund. Valid at any time before a finalize lands.
pub fn cure(lw: &LivePending, funding: &Funding, opts: &Opts) -> Result<Built> {
    let w = &lw.w;
    if w.kind != KIND_LIQ {
        return invalid("only a pending liquidation can be cured; a pending close is refuted by a challenge");
    }
    let c = w.cure_pair().ok_or_else(|| {
        Error::Invalid(if w.can_cure() {
            "the pending part's cure hash does not match the pair its own state describes".into()
        } else {
            "this claim has no cure: the short's cure bound would exceed priceMax".into()
        })
    })?;
    check_pending(lw)?;
    let loser = if w.side == LONG { w.long_key } else { w.short_key };
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    d.add_pending_in(lw, "cure", vec![Arg::Sig(loser)], format!("pending part (cure by the {})", side_name(w.side)));
    d.fund_inputs(funding)?;
    d.add_plain(w.refund as u64, &w.proposer_key, "proposer repaid (bond and carrier)");
    d.add_pair_out(&c, w.cure_value(lw.value) as u64, Some((0, lw.id)), format!("cured pair (the {}'s bound moved to {} KAS)", side_name(w.side), kas_str(w.cure_bound as i128)));
    d.add_change(&funding.change_key);
    d.finish()
}

/// Finalize (`KpfPending.finalize`): anyone, once the pending UTXO is `challengeDaa` old. A close pays the closer its
/// payoff at the clamped quote plus the refund and the other party the rest; a liquidation forfeits the loser's side to
/// the winner (both sides then the winner's) and pays the proposer the bond and the fee.
pub fn finalize(lw: &LivePending, funding: &Funding, opts: &Opts) -> Result<Built> {
    let w = &lw.w;
    check_pending(lw)?;
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    let i = d.add_pending_in(lw, "finalize", vec![], "pending part (finalize)");
    d.ins[i].sequence = w.m.challenge_daa.max(0) as u64;
    d.fund_inputs(funding)?;
    if w.kind == KIND_CLOSE {
        let (lp, sp) = w.finalize_close(lw.value);
        if lp > 0 {
            d.add_plain(lp as u64, &w.long_key, if w.side == LONG { "long paid (closer, with the refund)" } else { "long paid" });
        }
        d.add_change(&funding.change_key);
        if sp > 0 {
            while d.outs.len() < d.ins.len() {
                d.add_plain(DUST_OUTPUT_MIN, &funding.change_key, "filler (keeps the reverse payout slot clear of the inputs)");
            }
            d.add_plain(sp as u64, &w.short_key, if w.side == SHORT { "short paid (closer, with the refund)" } else { "short paid" });
        }
    } else {
        let f = w.forfeit_pair().ok_or_else(|| Error::Invalid("the pending part's forfeit hash does not match the pair its own state describes".into()))?;
        let (fv, pay) = w.finalize_forfeit(lw.value);
        d.add_plain(pay as u64, &w.proposer_key, "proposer paid (bond and fee)");
        d.add_pair_out(&f, fv as u64, Some((0, lw.id)), format!("forfeited part (both sides the {}'s)", if w.side == LONG { "short" } else { "long" }));
        d.add_change(&funding.change_key);
    }
    d.finish()
}

// ---------------------------------------------------------------- guaranteed fast settlement (DESIGN 4.5f)

/// Why a guarantee of `lw` by `owner` is refused before anything is built.
pub fn check_guarantee(lw: &LivePending, owner: &[u8; 32]) -> Result<()> {
    check_pending(lw)?;
    let w = &lw.w;
    secp256k1::XOnlyPublicKey::from_slice(owner).map_err(|_| Error::Invalid("the guarantor key is not a valid x-only public key".into()))?;
    if !w.guaranteeable_at(lw.daa) {
        return invalid(format!(
            "the pending part was accepted at DAA {} after its proposal's evidence order stopped being fillable by conforming matchers (DAA {}): it can only take the slow path (challenge window, then finalize)",
            lw.daa, w.visible_until
        ));
    }
    if w.kind == KIND_LIQ && w.forfeit_pair().is_none() {
        return invalid("the pending part's forfeit hash does not match the pair its own state describes");
    }
    Ok(())
}

/// Guarantee (`KpfPending.guarantee`, DESIGN 4.5f): settle the pending claim `lw` at once, exactly as finalize would
/// after the window, with a lock of at least `lambda x cover + offer` owned by `owner`, funded by `funding` (the
/// guarantor's KAS; the proposer's offer rides in the lock). A liquidation needs the pending part to be `graceDaa` old
/// (the loser's cure grace). The lock is disputable for `disputeDaa` with the counter-evidence a challenge would have
/// needed; a successful dispute pays the wronged party from it, otherwise it returns to `owner` by [`release`].
pub fn guarantee(lw: &LivePending, owner: &[u8; 32], funding: &Funding, opts: &Opts) -> Result<Built> {
    check_guarantee(lw, owner)?;
    let w = &lw.w;
    let lock = w.lock_of(*owner, lw.id, lw.daa, lw.value);
    let lock_value = w.lock_min(lw.value) as u64;
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    let pi = d.add_pending_in(lw, "guarantee", vec![], "pending part (guarantee)");
    if w.kind == KIND_LIQ {
        d.ins[pi].sequence = w.m.grace_daa.max(0) as u64;
    }
    d.fund_inputs(funding)?;
    let j;
    if w.kind == KIND_CLOSE {
        let (lp, sp) = w.guarantee_close(lw.value);
        if lp > 0 {
            d.add_plain(lp as u64, &w.long_key, if w.side == LONG { "long paid (closer, with the refund, less the offer)" } else { "long paid" });
        } else {
            d.add_plain(FILLER_VALUE, &funding.change_key, "filler (the long is owed nothing)");
        }
        j = d.add_lock_out(&lock, lock_value, format!("lock of the guarantor ({} KAS, disputable for {} DAA)", kas_str(lock_value as i128), w.m.dispute_daa));
        d.add_change(&funding.change_key);
        if sp > 0 {
            while d.outs.len() < d.ins.len() {
                d.add_plain(DUST_OUTPUT_MIN, &funding.change_key, "filler (keeps the reverse payout slot clear of the inputs)");
            }
            d.add_plain(sp as u64, &w.short_key, if w.side == SHORT { "short paid (closer, with the refund, less the offer)" } else { "short paid" });
        }
    } else {
        let f = w.forfeit_pair().ok_or_else(|| Error::Invalid("the pending part's forfeit hash does not match the pair its own state describes".into()))?;
        let (fv, pay) = w.guarantee_forfeit(lw.value);
        d.add_plain(pay as u64, &w.proposer_key, "proposer paid (bond and fee, less the offer)");
        d.add_pair_out(&f, fv as u64, Some((0, lw.id)), format!("forfeited part (both sides the {}'s)", if w.side == LONG { "short" } else { "long" }));
        j = d.add_lock_out(&lock, lock_value, format!("lock of the guarantor ({} KAS, disputable for {} DAA)", kas_str(lock_value as i128), w.m.dispute_daa));
        d.add_change(&funding.change_key);
    }
    if let Plan::Pending { args, .. } = &mut d.ins[pi].plan {
        *args = guarantee_args(j, owner);
    }
    d.finish()
}

/// Dispute up to 8 locks (with pending parts to challenge and proposals in the same block, if wanted) on one
/// counter-evidence fill (`KpfLock.dispute`, DESIGN 4.5f, 4.5g): each pays its wronged party the whole cover (whatever
/// the counter quote), burns what is neither restitution nor reward, and leaves the reward to the executor (the disputer).
pub fn dispute(locks: &[LiveLock], eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<Built> {
    let m: Vec<Member> = locks.iter().map(|l| Member::Dispute(l.clone())).collect();
    evidence_tx(&m, eu, funding, opts)
}

/// Release (`KpfLock.release`, DESIGN 4.5f): anyone, once the lock is `disputeDaa` old; its whole value goes to its
/// owner, the fee comes from `funding`.
pub fn release(ll: &LiveLock, funding: &Funding, opts: &Opts) -> Result<Built> {
    let l = &ll.l;
    let mut d = Draft::new(opts.fee_rate, opts.fee_mode);
    let i = d.add_lock_in(ll, "release", vec![], "lock (release)");
    d.ins[i].sequence = l.m.dispute_daa.max(0) as u64;
    d.fund_inputs(funding)?;
    d.add_plain(ll.value, &l.owner, "the guarantor's lock returned");
    d.add_change(&funding.change_key);
    d.finish()
}
