//! Transaction builders of the three price-evidence paths of `KpfPair.onEvidence` (DESIGN 3.3, 3.6, 4.4, 4.5 T4, T6,
//! T7, 4.5b): P5 unilateral close, P6 liquidation (close both), P7 liquidation by transfer, alone or as a block of up to
//! 8 pairs acting on one evidence fill.
//!
//! # What the transaction contains
//!
//! Besides the pairs, the transaction holds the fill of a live KOB `KobAsk` or `KobBid` (the evidence, DESIGN 3.3), its
//! counterparty and the KCC-20 token inputs and outputs the token program requires, so that every input, KOB's and the
//! token's included, passes its own script. KOB's own fill builder (`kob_protocol::build::build_batch`) lives in a
//! crate-private draft that takes only KOB's own order and key UTXOs, not a foreign pair input, so this module lays the
//! KOB side out itself from KOB's published artifacts ([`crate::artifacts::ask_base`] and friends).
//!
//! The counterparty is the executor (the funder), which needs the fewest inputs KOB's rules allow:
//!
//! - **ask evidence** (a long is liquidated, a short closes): the executor *buys* the ask's tokens with its own KAS. Inputs
//!   `KobAsk` + its custody (the leader of the token transfer); the maker gets the KAS at the ask's own index, the executor
//!   receives the tokens as a P2PK holder UTXO.
//! - **bid evidence** (a short is liquidated, a long closes): the executor *sells* tokens into the bid. Inputs `KobBid` +
//!   one P2PK token UTXO of the executor holding at least the fill (the leader, signed by the executor); the bid's maker
//!   gets the tokens at the bid's own index, the executor the KAS. Without such a token UTXO the path is closed to that
//!   executor: a resting ask to fill against the bid would add three more inputs and the ask's own custody rules.
//!
//! # Layout (DESIGN 4.4; the covenants accept any order with the same rules)
//!
//! ```text
//! inputs   [0, B)        the block of pairs, one `onEvidence` each (first pushes n_i, q = 0)
//!          B             the evidence order (settle / fill; its first push is n_ev)
//!          B + 1         its token input: the ask's custody, or the executor's tokens sold to the bid
//!          B + 2 ..      the `KpfOpen` entrants of P7, then the funding inputs
//! outputs  one positional output per input up to the entrants:
//!          i             pair i: the long's payout (P5, P6) or the winner's payout (P7), else a filler
//!          B             ask: the maker's KAS; bid: the delivery of the tokens to the bid's maker
//!          B + 1         ask: the executor's tokens; bid: the executor's KAS
//!          B + 2 + k     the entrant's continuation or refund
//!          then          pair remainders and parts, the order's continuation, a custody rest or token change, the
//!                        executor's change
//!          M - 1 - i     pair i: the short's payout (P5, P6); a terminal bid's refund at M - 1 - B
//! ```
//!
//! Every party output holds the least value its covenant accepts; the executor's change takes the rest, less the fee.

use kaspa_consensus_core::Hash;
use silverscript_abi::ArtifactValue;

use crate::build::{take_args, Funding, Opts};
use crate::error::{invalid, Error, Result};
use crate::kob::*;
use crate::payoff::*;
use crate::state::*;
use crate::tx::*;

/// Most pairs one evidence fill can serve (the block of DESIGN 4.4 is scanned over at most 8 inputs).
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

/// What one pair of the block does.
#[derive(Clone, Debug)]
pub enum Act {
    /// P5: the owner of `side` (signature) closes `n` at the adverse bound.
    Close { side: i64 },
    /// P6: the loser `side` is liquidated; the winner takes the part less the fee.
    Liq { side: i64 },
    /// P7: the loser `side` is sold to the `KpfOpen` `entrant` at `x`; the part is ordinal `k` of the continuations.
    LiqTransfer { side: i64, x: i64, entrant: LiveOpen, k: i64 },
}

/// One pair of the block: the part `n` of `pair` and what is done with it.
#[derive(Clone, Debug)]
pub struct Leg {
    pub pair: LivePair,
    pub n: i64,
    pub act: Act,
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

fn zero_args() -> (Arg, Arg) {
    (bv(&[0u8; 32]), bv(&[]))
}

/// `KpfPair.onEvidence(nb, qb, path, s, side, ev, tk, x, nbd, ek, et, k)`.
#[allow(clippy::too_many_arguments)]
fn ev_args(n: i64, q: i64, path: i64, s: Arg, side: i64, ev: usize, tk: i64, x: i64, nbd: i64, ek: &[u8; 32], et: Hash, k: i64) -> Vec<Arg> {
    vec![bv(&sm8(n)), bv(&sm8(q)), iv(path), s, iv(side), iv(ev as i64), iv(tk), iv(x), iv(nbd), bv(ek), bv(&et.as_bytes()), iv(k)]
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
    Open(Open, (u16, Hash)),
}

fn emit(d: &mut Draft, p: Pend) -> usize {
    match p.kind {
        Kind::Plain(k) => d.add_plain(p.value, &k, p.label),
        Kind::Pair(pr, b) => d.add_pair_out(&pr, p.value, Some(b), p.label),
        Kind::Kob(o, b) => d.add_kob_out(o, p.value, b, p.label),
        Kind::Open(o, b) => d.add_open_out(&o, p.value, Some(b), p.label),
    }
}

fn filler(key: &[u8; 32], what: &str) -> Pend {
    Pend { value: FILLER_VALUE, kind: Kind::Plain(*key), label: format!("filler ({what})") }
}

fn plain(value: u64, key: &[u8; 32], label: impl Into<String>) -> Pend {
    let value = value.max(PARTY_FLOOR);
    Pend { value, kind: Kind::Plain(*key), label: label.into() }
}

/// The transaction of one evidence fill and the block of pairs that act on it (P5, P6, P7 in any mix the covenants
/// accept: see [`Act`]). The executor is `funding.change_key`: it receives the tokens an ask evidence sells it, or the
/// KAS a bid evidence pays it, and pays the network fee.
pub fn evidence_tx(legs: &[Leg], eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<Built> {
    let (built, _) = evidence_tx_facts(legs, eu, funding, opts)?;
    Ok(built)
}

/// [`evidence_tx`], also returning what the KOB side does.
pub fn evidence_tx_facts(legs: &[Leg], eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<(Built, FillFacts)> {
    evidence_tx_inner(legs, eu, funding, opts, true)
}

/// Builds the transaction even for an order [`check_evidence`] rejects, so that tests can show that the contracts refuse
/// what the check calls a problem. Never use it to send anything.
#[doc(hidden)]
pub fn evidence_tx_unchecked(legs: &[Leg], eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<Built> {
    evidence_tx_inner(legs, eu, funding, opts, false).map(|(b, _)| b)
}

fn evidence_tx_inner(legs: &[Leg], eu: &EvidenceUse, funding: &Funding, opts: &Opts, checked: bool) -> Result<(Built, FillFacts)> {
    if legs.is_empty() {
        return invalid("nothing to do: no pair given");
    }
    if legs.len() > MAX_BLOCK {
        return invalid(format!("an evidence fill serves at most {MAX_BLOCK} pairs (the block scan of KpfPair is unrolled to 8); {} given", legs.len()));
    }
    let m = legs[0].pair.p.m.clone();
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

    // The legs.
    let mut seen = std::collections::BTreeSet::new();
    let mut sum = 0i64;
    let mut lock = rep.rest_end;
    for (j, g) in legs.iter().enumerate() {
        let p = &g.pair.p;
        p.validate()?;
        // The evidence clock of this pair (DESIGN 4.5c): the rest counts from max(exposed, S), and the lock time must
        // prove its end.
        let s = g.pair.clock();
        let on = check_evidence_on(ev, &m, Some(s), eu.now);
        if checked && !on.problems.is_empty() {
            return invalid(format!("pair {j}: this order cannot serve as evidence on the pair's clock (DAA {s}): {}", on.problems.join("; ")));
        }
        lock = lock.max(on.rest_end);
        if p.m != m {
            return invalid(format!("pair {j} is of another market than pair 0"));
        }
        if !seen.insert(g.pair.id) {
            return invalid(format!(
                "pair {j} has the covenant id of an earlier pair of the block (the same pair twice, or the pieces of one split, which share an id): every KpfPair must be the only input of its covenant id, so one piece of an id acts per transaction"
            ));
        }
        let ok_part = g.n > 0 && g.n <= p.size && g.n >= m.min_size && (p.size == g.n || p.size - g.n >= m.min_size);
        if !ok_part {
            return invalid(format!(
                "pair {j}: n = {} is not a valid part of its size {} (a part and a remainder are each 0 or at least minSize {})",
                g.n, p.size, m.min_size
            ));
        }
        sum = sum.checked_add(g.n).ok_or_else(|| Error::Invalid("the parts add up beyond 63 bits".into()))?;
        match &g.act {
            Act::Close { side } => {
                if *side != LONG && *side != SHORT {
                    return invalid("side must be long or short");
                }
                if (*side == LONG) == is_ask {
                    return invalid(format!(
                        "pair {j}: the {} closes at a resting {} (a long at a bid, a short at an ask), but the evidence is {} {}",
                        side_name(*side),
                        if *side == LONG { "bid" } else { "ask" },
                        if is_ask { "an" } else { "a" },
                        ev.name()
                    ));
                }
            }
            Act::Liq { side } | Act::LiqTransfer { side, .. } => {
                if *side != LONG && *side != SHORT {
                    return invalid("side must be long or short");
                }
                if (*side == LONG) != is_ask {
                    return invalid(format!(
                        "pair {j}: a {} is liquidated by {} (a long by an ask, a short by a bid), but the evidence is {} {}",
                        side_name(*side),
                        if *side == LONG { "an ask" } else { "a bid" },
                        if is_ask { "an" } else { "a" },
                        ev.name()
                    ));
                }
                if quote < 0 || quote > m.price_max {
                    return invalid(format!("the evidence quote {quote} is outside 0..=priceMax {}: it cannot liquidate", m.price_max));
                }
                let trig = if *side == LONG { long_liquidatable(p, quote) } else { short_liquidatable(p, quote) };
                if !trig {
                    return invalid(format!(
                        "pair {j}: the {} is not liquidatable at the {} {} KAS (its trigger is {} KAS: {})",
                        side_name(*side),
                        ev.name(),
                        crate::error::kas_str(quote as i128),
                        crate::error::kas_str(trigger_of(p, *side) as i128),
                        if *side == LONG { "a long needs an ask below it" } else { "a short needs a bid above it" }
                    ));
                }
                if let Act::LiqTransfer { x, entrant, k, .. } = &g.act {
                    if *k != 0 && *k != 1 {
                        return invalid("k must be 0 or 1");
                    }
                    if entrant.o.side != *side || entrant.o.m != m {
                        return invalid(format!("pair {j}: the entrant order must be a {} order of the same market", side_name(*side)));
                    }
                    if *x < 0 || *x > m.price_max {
                        return invalid(format!("pair {j}: x = {x} is outside 0..=priceMax"));
                    }
                    if *side == LONG {
                        if *x < quote {
                            return invalid(format!("pair {j}: a long is sold at x >= the evidence ask: x = {x} is below {quote}"));
                        }
                    } else if *x > quote {
                        return invalid(format!("pair {j}: a short is sold at x <= the evidence bid: x = {x} is above {quote}"));
                    }
                    let nb = if *side == LONG { *x - entrant.o.my_rate } else { *x + entrant.o.my_rate };
                    if *side == LONG {
                        if !(nb >= 0 && nb <= p.bl) {
                            return invalid(format!("pair {j}: the entrant's bound {nb} would raise bl above {} (no-downgrade for the winner)", p.bl));
                        }
                    } else if !(nb >= p.bs && nb <= m.price_max) {
                        return invalid(format!("pair {j}: the entrant's bound {nb} would lower bs below {} (no-downgrade for the winner)", p.bs));
                    }
                    // Health at entry (DESIGN 4.5c, review 3 F3): the entrant must not be liquidatable at the very quote of
                    // this transaction (KpfPair checks it at the quote, which implies the check at x).
                    if !healthy_at(&m, *side, nb, quote) {
                        return invalid(format!(
                            "pair {j}: the entrant would enter liquidatable: with its bound {} KAS it is liquidatable at the evidence {} {} KAS itself; sell at a price x {} {} KAS (or choose an entrant with more margin)",
                            crate::error::kas_str(nb as i128),
                            ev.name(),
                            crate::error::kas_str(quote as i128),
                            if *side == LONG { "at most" } else { "at least" },
                            crate::error::kas_str(entrant_health_bound(&m, *side, quote, entrant.o.my_rate) as i128)
                        ));
                    }
                    // The entrant's own rules (`KpfOpen.take`): its limit, its minimum counter rate and the entry price it
                    // accepts (a long: entry >= x; a short: entry <= x).
                    let eo = &entrant.o;
                    let why = if *side == LONG {
                        if *x > eo.limit {
                            Some(format!("x = {x} is above the entrant long order's limit {}", eo.limit))
                        } else if p.entry < *x {
                            Some(format!("the pair's entry price {} is below x = {x}: a long order accepts an entry at or above its fill price", p.entry))
                        } else if p.bs - *x < eo.min_counter_rate {
                            Some(format!("the stayer's margin rate {} is below the entrant's minCounterRate {}", p.bs - *x, eo.min_counter_rate))
                        } else {
                            None
                        }
                    } else if *x < eo.limit {
                        Some(format!("x = {x} is below the entrant short order's limit {}", eo.limit))
                    } else if p.entry > *x {
                        Some(format!("the pair's entry price {} is above x = {x}: a short order accepts an entry at or below its fill price", p.entry))
                    } else if *x - p.bl < eo.min_counter_rate {
                        Some(format!("the stayer's margin rate {} is below the entrant's minCounterRate {}", *x - p.bl, eo.min_counter_rate))
                    } else {
                        None
                    };
                    if let Some(w) = why {
                        return invalid(format!("pair {j}: {w}"));
                    }
                    check_entrant_amount(&entrant.o, g.n)?;
                    lock = lock.max(entrant.o.active_from.max(0) as u64);
                }
            }
        }
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
    let need = needed_fill(&m, sum);

    // The KOB side.
    let b_cnt = legs.len();
    let ev_i = b_cnt;
    let tk_i = b_cnt + 1;
    let n_opens = legs.iter().filter(|g| matches!(g.act, Act::LiqTransfer { .. })).count();
    let n_in = b_cnt + 2 + n_opens + funding.utxos.len();
    let idx0 = b_cnt + 2 + n_opens; // the first output after the positional ones
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

    let mut pos_pairs: Vec<Pend> = vec![];
    let mut open_ins: Vec<(LiveOpen, Vec<Arg>)> = vec![];
    let mut open_pos: Vec<Pend> = vec![];
    let mut pair_ins: Vec<(usize, Vec<Arg>, String)> = vec![];
    let mut oi = b_cnt + 2;
    let (z32, nosig) = zero_args();
    let _ = z32;
    for (j, g) in legs.iter().enumerate() {
        let l = &g.pair;
        // The outputs that do not change hands keep the clock S (DESIGN 4.5c).
        let lpc = l.clocked();
        let lp = &lpc;
        let z = [0u8; 32];
        match &g.act {
            Act::Close { side } => {
                let owner = if *side == LONG { lp.long_key } else { lp.short_key };
                let (l_pay, s_pay, rest) = close_at(lp, l.value, g.n, clamp_price(lp, quote), *side, 0);
                pair_ins.push((j, ev_args(g.n, 0, 5, Arg::Sig(owner), *side, ev_i, tk_arg, 0, 0, &z, ZERO, 0), format!("pair {j} (unilateral close, {})", side_name(*side))));
                pos_pairs.push(if l_pay > 0 { plain(l_pay as u64, &lp.long_key, format!("long paid (pair {j})")) } else { filler(&exec, "pair slot") });
                if s_pay > 0 {
                    set_tail(&mut tail, j, plain(s_pay as u64, &lp.short_key, format!("short paid (pair {j})")));
                }
                if let Some((r, rv)) = rest {
                    extras.push(Pend { value: rv as u64, kind: Kind::Pair(r, (j as u16, l.id)), label: format!("pair {j} remainder") });
                }
            }
            Act::Liq { side } => {
                let fee = liq_fee(lp, g.n, quote, *side);
                let (l_pay, s_pay, rest) = close_at(lp, l.value, g.n, loser_price(lp, *side), *side, fee);
                pair_ins.push((j, ev_args(g.n, 0, 6, nosig.clone(), *side, ev_i, tk_arg, 0, 0, &z, ZERO, 0), format!("pair {j} (liquidation, {} loses)", side_name(*side))));
                pos_pairs.push(if l_pay > 0 { plain(l_pay as u64, &lp.long_key, format!("long paid (pair {j})")) } else { filler(&exec, "pair slot") });
                if s_pay > 0 {
                    set_tail(&mut tail, j, plain(s_pay as u64, &lp.short_key, format!("short paid (pair {j})")));
                }
                if let Some((r, rv)) = rest {
                    extras.push(Pend { value: rv as u64, kind: Kind::Pair(r, (j as u16, l.id)), label: format!("pair {j} remainder") });
                }
            }
            Act::LiqTransfer { side, x, entrant, k } => {
                let nb = if *side == LONG { *x - entrant.o.my_rate } else { *x + entrant.o.my_rate };
                let t = TransferSpec { side: *side, n: g.n, x: *x, nb, ek: entrant.o.owner, et: entrant.id, k: *k };
                let to = hand_over_out(lp, l.value, &t, p7_pay(lp, g.n, *side, *x, quote));
                pair_ins.push((
                    j,
                    ev_args(g.n, 0, 7, nosig.clone(), *side, ev_i, tk_arg, *x, nb, &t.ek, t.et, *k),
                    format!("pair {j} (liquidation by transfer, {} sold)", side_name(*side)),
                ));
                let winner = winner_key(lp, *side);
                pos_pairs.push(if to.pay > 0 { plain(to.pay as u64, &winner, format!("winner paid (pair {j})")) } else { filler(&exec, "pair slot") });
                let part_o = Pend { value: to.part_value as u64, kind: Kind::Pair(to.part.clone(), (j as u16, l.id)), label: format!("pair {j} part (entrant)") };
                let part_at;
                match &to.rest {
                    None => {
                        part_at = idx0 + extras.len();
                        extras.push(part_o);
                    }
                    Some((r, rv)) => {
                        let rest_o = Pend { value: *rv as u64, kind: Kind::Pair(r.clone(), (j as u16, l.id)), label: format!("pair {j} remainder") };
                        if *k == 0 {
                            part_at = idx0 + extras.len();
                            extras.push(part_o);
                            extras.push(rest_o);
                        } else {
                            extras.push(rest_o);
                            part_at = idx0 + extras.len();
                            extras.push(part_o);
                        }
                    }
                }
                open_ins.push((entrant.clone(), take_args(&to.part, *side, part_at as i64, g.n, *x)));
                let (rest, cont) = order_rest(&entrant.o, entrant.value, g.n);
                open_pos.push(match cont {
                    Some(c) => Pend { value: rest, kind: Kind::Open(c, (oi as u16, entrant.id)), label: format!("entrant order continuation (pair {j})") },
                    None => plain(rest, &entrant.o.owner, format!("entrant order refund (pair {j})")),
                });
                oi += 1;
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
    for (j, args, label) in pair_ins {
        d.add_pair_in(&legs[j].pair, "onEvidence", args, label);
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
    for (lo, args) in &open_ins {
        d.add_open_in(lo, "take", args.clone(), "entrant order (take)");
    }
    let first_fund = d.fund_inputs(funding)?;
    debug_assert_eq!(first_fund, n_in - funding.utxos.len());

    for p in pos_pairs {
        emit(&mut d, p);
    }
    for p in ev_pos {
        emit(&mut d, p);
    }
    for p in open_pos {
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

/// The checks of `KpfOpen.take` on the amount of a P7 entrant.
fn check_entrant_amount(o: &Open, n: i64) -> Result<()> {
    if !(n > 0 && n <= o.size_left && n >= o.m.min_size && (n >= o.min_fill || n == o.size_left)) {
        return invalid(format!("the entrant order cannot take n = {n}: it has {} left, minFill {}, minSize {}", o.size_left, o.min_fill, o.m.min_size));
    }
    let left = o.size_left - n;
    if left != 0 && left < o.m.min_size {
        return invalid(format!("taking {n} would leave the entrant order {left}, below minSize {}", o.m.min_size));
    }
    Ok(())
}

// ---------------------------------------------------------------- the three paths

/// P5 unilateral close: the owner of `side` of `l` (signature) closes `n` at the adverse bound, a long at a resting bid
/// and a short at a resting ask.
pub fn close_on_evidence(l: &LivePair, side: i64, n: i64, eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<Built> {
    let leg = Leg { pair: l.clone(), n, act: Act::Close { side } };
    evidence_tx(&[leg], eu, funding, opts)
}

/// P6 liquidation (close both) of up to 8 pairs, whole or in parts, on one evidence: an ask liquidates longs, a bid
/// shorts. The loser gets 0, the winner the part less the keeper fee.
pub fn liquidate(parts: &[(LivePair, i64)], eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<Built> {
    let side = if eu.order.is_ask() { LONG } else { SHORT };
    let legs: Vec<Leg> = parts.iter().map(|(p, n)| Leg { pair: p.clone(), n: *n, act: Act::Liq { side } }).collect();
    evidence_tx(&legs, eu, funding, opts)
}

/// P7 liquidation by transfer: the loser's part `n` of `l` is sold at `x` to the resting `KpfOpen` `entrant` of its
/// side; the winner keeps its position and is paid the entrant's payment less the fee.
pub fn liquidate_transfer(l: &LivePair, n: i64, x: i64, entrant: &LiveOpen, k: i64, eu: &EvidenceUse, funding: &Funding, opts: &Opts) -> Result<Built> {
    let side = if eu.order.is_ask() { LONG } else { SHORT };
    let leg = Leg { pair: l.clone(), n, act: Act::LiqTransfer { side, x, entrant: entrant.clone(), k } };
    evidence_tx(&[leg], eu, funding, opts)
}

/// The price bound at which a P7 entrant of `side` with margin rate `my_rate` is healthy at the evidence `quote`
/// (DESIGN 4.5c, review 3 F3): a long entrant (bound `x - my_rate`) needs `x <= floor(quote * (10^4 - mmrBps) / 10^4) +
/// my_rate`; a short entrant (bound `x + my_rate`) needs `x >= ceil(quote * (10^4 + mmrBps) / 10^4) - my_rate`.
pub fn entrant_health_bound(m: &Market, side: i64, quote: i64, my_rate: i64) -> i64 {
    let (q, mm, b) = (quote as i128, m.mmr_bps as i128, BPS as i128);
    let v = if side == LONG { q * (b - mm) / b + my_rate as i128 } else { (q * (b + mm) + b - 1) / b - my_rate as i128 };
    v.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// The price `x` at which the loser's part is best sold to `entrant` for the winner (who receives the entrant's payment
/// less the fee): the highest `x` for a long loser, the lowest for a short loser, within what the evidence quote, the
/// no-downgrade rule, the entrant's health at the quote (DESIGN 4.5c) and the entrant's own limit, entry and
/// counter-rate rules allow. When no price keeps the entrant healthy at the quote, it refuses: such an entrant would
/// enter liquidatable by the very quote of the transaction (review 3 F3).
pub fn p7_default_x(p: &Pair, side: i64, quote: i64, entrant: &Open) -> Result<i64> {
    let health = entrant_health_bound(&p.m, side, quote, entrant.my_rate);
    if side == LONG {
        let hi = [entrant.limit, p.bl + entrant.my_rate, p.bs - entrant.min_counter_rate, p.entry, p.m.price_max, health].into_iter().min().expect("six bounds");
        if hi < quote {
            if health < quote {
                return invalid(format!(
                    "no price works: this entrant (margin rate {} KAS) would be liquidatable at the evidence ask {} KAS itself at any x >= the ask; it would forfeit its whole margin at the next liquidation. Choose an entrant with a margin rate of at least {} KAS",
                    crate::error::kas_str(entrant.my_rate as i128),
                    crate::error::kas_str(quote as i128),
                    crate::error::kas_str((quote - (quote as i128 * (BPS - p.m.mmr_bps) as i128 / BPS as i128) as i64) as i128)
                ));
            }
            return invalid(format!(
                "no price works: a long is sold at x >= the evidence ask {quote}, but this entrant allows at most {hi} (its limit {}, no-downgrade {}, counter rate, the pair's entry {}, health at the ask {health})",
                entrant.limit,
                p.bl + entrant.my_rate,
                p.entry
            ));
        }
        Ok(hi)
    } else {
        let lo = [entrant.limit, p.bs - entrant.my_rate, p.bl + entrant.min_counter_rate, p.entry, 0, health].into_iter().max().expect("six bounds");
        if lo > quote {
            if health > quote {
                return invalid(format!(
                    "no price works: this entrant (margin rate {} KAS) would be liquidatable at the evidence bid {} KAS itself at any x <= the bid; it would forfeit its whole margin at the next liquidation. Choose an entrant with a margin rate of at least {} KAS",
                    crate::error::kas_str(entrant.my_rate as i128),
                    crate::error::kas_str(quote as i128),
                    crate::error::kas_str((((quote as i128 * (BPS + p.m.mmr_bps) as i128 + BPS as i128 - 1) / BPS as i128) as i64 - quote) as i128)
                ));
            }
            return invalid(format!(
                "no price works: a short is sold at x <= the evidence bid {quote}, but this entrant needs at least {lo} (its limit {}, no-downgrade {}, counter rate, the pair's entry {}, health at the bid {health})",
                entrant.limit,
                p.bs - entrant.my_rate,
                p.entry
            ));
        }
        Ok(lo)
    }
}
