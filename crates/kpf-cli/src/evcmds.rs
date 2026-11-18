//! The commands of the two-step price-evidence paths (DESIGN 4.5d): `propose-close` (P5), `propose-liquidate` (P6),
//! `challenge`, `cure`, `finalize`, `watch` (pending claims against the user's pairs), `evidence` (the check of an order
//! against a pair or a pending part) and `kob-file` (an order file from a redeem script). Everything is offline unless
//! `--node` is given; nothing is submitted without `--submit`.
//!
//! An evidence transaction contains a real fill of a KOB order and moves tokens: the executor (the funder) BUYS the
//! evidence ask's tokens with its own KAS, or SELLS its own tokens into the evidence bid. See `docs/TESTNET.md`.

use std::path::Path;

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kpf_protocol::error::{from_hex, hex32, to_hex};
use kpf_protocol::evidence::*;
use kpf_protocol::kob::*;
use kpf_protocol::network::Network;
use kpf_protocol::state::*;
use kpf_protocol::tx::*;
use serde_json::{json, Value};

use crate::args::*;
use crate::cmds::*;
use crate::files::*;
use crate::keys;
use crate::node::{Node, WsTransport};

const EV_FLAGS: &[&str] = &["evidence", "seller", "evidence-fill", "daa", "allow-dead-evidence"];

fn kas(v: i64) -> String {
    fmt_kas(v as i128)
}

/// The node's virtual DAA score.
fn node_daa(node: &mut Node<WsTransport>) -> R<u64> {
    let p = node.t_call("getBlockDagInfo")?;
    let v = p.get("virtualDaaScore").ok_or("the node did not report a virtualDaaScore")?;
    match v {
        Value::Number(n) => n.as_u64().ok_or_else(|| "virtualDaaScore is not an integer".to_string()),
        Value::String(s) => s.parse().map_err(|_| "virtualDaaScore is not an integer".to_string()),
        _ => Err("virtualDaaScore is not an integer".into()),
    }
}

/// The DAA score the checks run at: the node's, else `--daa`, else unknown.
pub(crate) fn now_daa(a: &Args, node: &mut Option<Node<WsTransport>>) -> R<Option<u64>> {
    if let Some(n) = node {
        return node_daa(n).map(Some);
    }
    if a.has("daa") {
        return Ok(Some(int(a, "daa")? as u64));
    }
    Ok(None)
}

/// The pair's evidence clock and hand-over limit in words (DESIGN 4.5c).
pub(crate) fn print_clock(name: &str, l: &LivePair, now: Option<u64>) {
    let s = l.clock();
    let from = if l.p.since != 0 { format!("since = {}", l.p.since) } else { format!("since = 0: the DAA of this UTXO, {}", l.daa) };
    let hand = l.handover_from();
    let wait = match now {
        Some(n) if n >= hand => format!("allowed now (DAA {n})"),
        Some(n) => format!("{} DAA from now (DAA {n})", hand - n),
        None => "give --daa or --node to see how long".into(),
    };
    println!(
        "{name}: evidence clock starts at DAA {s} ({from}); evidence counts only for its rest from then on ({} DAA). A hand-over (transfer, close-versus-close) is allowed from DAA {hand}: {wait}",
        l.p.m.min_rest_daa
    );
}

fn load_kob(a: &Args, flag: &str, node: &mut Option<Node<WsTransport>>, net: Network) -> R<KobFile> {
    let mut f = KobFile::load(a.need(flag)?)?;
    if let Some(n) = node {
        refresh(n, &mut f.obj, net)?;
        if let Some(c) = &mut f.custody {
            refresh(n, c, net)?;
        }
    }
    Ok(f)
}

fn evidence_use(a: &Args, node: &mut Option<Node<WsTransport>>, net: Network, scale: i64, now: Option<u64>) -> R<EvidenceUse> {
    let order = load_kob(a, "evidence", node, net)?.evidence()?;
    let seller = if a.has("seller") { Some(load_kob(a, "seller", node, net)?.live_token()?) } else { None };
    let fill = if a.has("evidence-fill") { Some(tokens(a, "evidence-fill", scale)?) } else { None };
    let allow_dead = a.has("allow-dead-evidence");
    if allow_dead {
        println!("WARNING: --allow-dead-evidence: a soft-expired order is accepted as evidence. Use it only to sweep a dead order in a test; a dead order is a past quote nobody can trade at (DESIGN 4.5c)");
    }
    Ok(EvidenceUse { order, fill, seller, now, allow_dead })
}

/// The evidence report in words.
fn print_report(r: &EvidenceReport, m: &Market, daa: Option<u64>) {
    let sc = m.scale as i128;
    println!(
        "evidence: {} at {} KAS per token; it can fill {} tokens; one fill of all of it covers parts totalling {} tokens (k = {} bps)",
        r.kind,
        kas(r.quote),
        fmt_units(r.capacity as i128, sc),
        fmt_units(r.covers as i128, sc),
        m.k_bps
    );
    println!(
        "          rested since DAA {}{}; counts as evidence from DAA {} (rest {} DAA); conforming matchers fill it until DAA {}{}",
        r.exposed,
        r.clock.map(|c| format!(" on the pair's clock (clock start DAA {c})")).unwrap_or_default(),
        r.rest_end,
        m.min_rest_daa,
        r.live_until,
        daa.map(|d| format!("; DAA now {d}")).unwrap_or_default()
    );
    for p in &r.problems {
        println!("  CANNOT SERVE: {p}");
    }
    for w in &r.waiting {
        println!("  NOT YET: {w}");
    }
    for d in &r.dead {
        println!("  DEAD: {d}");
    }
    for n in &r.notes {
        println!("  note: {n}");
    }
    if r.usable() {
        println!("  usable as evidence{}", if daa.is_some() { " now" } else { " (give --daa or --node to check the time)" });
    }
}

/// What the builders refuse before the engine does, for the time: the lock time proves a DAA the chain may not have
/// reached.
pub(crate) fn lock_guard(a: &Args, built: &Built, now: Option<u64>) -> R<()> {
    let lock = built.tx.lock_time;
    println!("lock time {lock}: the transaction proves DAA {lock} (the latest of the bounds it needs: the evidence's rest on each pair's clock, the hand-over limit S + 4R, an order's activeFrom); it is valid from that DAA on");
    if let Some(n) = now {
        if lock > n {
            let msg = format!("the lock time {lock} is ahead of the DAA now ({n}) by {} DAA: a node refuses the transaction until then", lock - n);
            if a.has("submit") {
                return Err(msg);
            }
            println!("WARNING: {msg}");
        }
    }
    Ok(())
}

fn short_part(p: &LivePair, n: i64) -> String {
    format!("{} of {} tokens", fmt_units(n as i128, p.p.m.scale as i128), fmt_units(p.p.size as i128, p.p.m.scale as i128))
}

fn load_pairs(a: &Args, node: &mut Option<Node<WsTransport>>, net: Network) -> R<Vec<LivePair>> {
    let files = a.all("pair");
    if files.is_empty() {
        return Err("--pair <file> is required".into());
    }
    files
        .iter()
        .map(|f| {
            let mut o = ObjFile::load(f)?;
            if let Some(n) = node {
                refresh(n, &mut o, net)?;
            }
            o.live_pair()
        })
        .collect()
}

/// `--n` repeated, one per pair; the whole pair when absent.
fn parts_of(a: &Args, pairs: &[LivePair]) -> R<Vec<(LivePair, i64)>> {
    let ns = a.all("n");
    if !ns.is_empty() && ns.len() != pairs.len() {
        return Err(format!("{} --pair but {} --n: give one --n per pair, or none for the whole pairs", pairs.len(), ns.len()));
    }
    pairs
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let n = if ns.is_empty() { p.p.size } else { parse_amount(ns[i], p.p.m.scale).map_err(|m| format!("--n: {m}"))? };
            Ok((p.clone(), n))
        })
        .collect()
}

fn describe_fill(f: &FillFacts, ev: &Evidence, m: &Market) {
    let sc = m.scale as i128;
    match ev {
        Evidence::Ask { .. } => println!(
            "evidence fill: the executor BUYS {} tokens at the ask {} KAS, paying {} KAS (it receives the tokens as a holder UTXO)",
            fmt_units(f.fill as i128, sc),
            kas(f.quote),
            kas(f.kas as i64)
        ),
        Evidence::Bid { .. } => println!(
            "evidence fill: the executor SELLS {} tokens into the bid {} KAS, receiving {} KAS",
            fmt_units(f.fill as i128, sc),
            kas(f.quote),
            kas(f.kas as i64)
        ),
    }
}

// ---------------------------------------------------------------- the two-step paths (DESIGN 4.5d)

fn load_pending(a: &Args, flag: &str, node: &mut Option<Node<WsTransport>>, net: Network) -> R<LivePending> {
    load_obj(a, flag, node, net)?.live_pending()
}

fn load_pendings(a: &Args, node: &mut Option<Node<WsTransport>>, net: Network) -> R<Vec<LivePending>> {
    let files = a.all("pending");
    if files.is_empty() {
        return Err("--pending <file> is required (a KpfPending object, written by the proposal)".into());
    }
    files
        .iter()
        .map(|f| {
            let mut o = ObjFile::load(f)?;
            if let Some(n) = node {
                refresh(n, &mut o, net)?;
            }
            o.live_pending()
        })
        .collect()
}

fn side_word(s: i64) -> &'static str {
    if s == LONG {
        "long"
    } else {
        "short"
    }
}

fn describe_claim(w: &LivePending) -> String {
    let m = &w.w.m;
    let part = format!("{} tokens", fmt_units(w.w.n as i128, m.scale as i128));
    if w.w.kind == KIND_CLOSE {
        format!("the {} closes {part} at {} KAS", side_word(w.w.side), kas(w.w.quote))
    } else {
        format!("the {} of {part} is liquidatable (evidence quote {} KAS)", side_word(w.w.side), kas(w.w.quote))
    }
}

/// The time left before a pending part can be finalized, in words.
fn finalize_words(w: &LivePending, now: Option<u64>) -> String {
    let from = w.finalize_from();
    match now {
        Some(n) if n >= from => format!("finalize is possible now (from DAA {from}); a challenge or a cure is still accepted until a finalize lands"),
        Some(n) => format!("finalize from DAA {from}: {} DAA from now (DAA {n})", from - n),
        None => format!("finalize from DAA {from} (give --daa or --node for the time left)"),
    }
}

/// The proposal's bond and the outputs a proposal writes, before signing.
fn print_proposal(l: &LivePair, n: i64, kind: i64, side: i64, quote: i64, proposer: &[u8; 32]) {
    let po = kpf_protocol::pending::propose_out(&l.clocked(), l.value, n, kind, side, quote, *proposer);
    let w = &po.pending;
    println!(
        "propose (DESIGN 4.5d): nothing settles now. {} tokens move into a pending part with a bond of {} KAS (refund {} KAS, funded by the executor); after {} DAA anyone may finalize it, and until then anyone may challenge it",
        fmt_units(n as i128, l.p.m.scale as i128),
        kas(w.bond),
        kas(w.refund),
        l.p.m.challenge_daa
    );
    if let Some(b) = w.contradiction_bound() {
        println!(
            "  it is refuted by a fresh {} {} {} KAS that rests {} DAA from the proposal (the bond then goes to the challenger and the part returns with a fresh clock)",
            if w.counter_is_ask() { "ask" } else { "bid" },
            if w.counter_is_ask() { "<=" } else { ">=" },
            kas(b),
            l.p.m.min_rest_daa
        );
    }
    if kind == KIND_LIQ {
        println!(
            "  at finalize the winner keeps both sides of the part (forfeit and keep) and the proposer {} receives the bond and the fee {} KAS; the loser may cure before then by moving its bound to {} KAS",
            to_hex(proposer),
            kas(w.fee),
            if w.can_cure() { kas(w.cure_bound) } else { "(no cure: the bound would pass priceMax)".into() }
        );
    }
}

pub fn propose_close_cmd(a: &Args) -> R<()> {
    let mut own = vec!["pair", "side", "n"];
    own.extend(EV_FLAGS);
    only_with(a, &own)?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load_obj(a, "pair", &mut node, net)?.live_pair()?;
    let sd = side(a.need("side")?)?;
    let n = if a.has("n") { tokens(a, "n", l.p.m.scale)? } else { l.p.size };
    let now = now_daa(a, &mut node)?;
    let eu = evidence_use(a, &mut node, net, l.p.m.scale, now)?;
    print_clock("pair", &l, now);
    print_report(&check_evidence_on(&eu.order, &l.p.m, Some(l.clock()), now), &l.p.m, now);
    let fk = funder_and_keys(a, true)?.expect("funder");
    let owner = if sd == LONG { l.p.long_key } else { l.p.short_key };
    println!("unilateral close of {} by the {}: the owner's key {} must sign (--key)", short_part(&l, n), side_word(sd), to_hex(&owner));
    print_proposal(&l, n, KIND_CLOSE, sd, eu.order.quote(), &owner);
    let opts = fee_opts(a)?;
    let facts = std::cell::RefCell::new(None);
    let m = Member::Propose(Leg { pair: l.clone(), n, act: Act::Close { side: sd } });
    let b = build_funded(a, net, &mut node, &fk.key, &|f| {
        let (b, fa) = evidence_tx_facts(std::slice::from_ref(&m), &eu, f, &opts)?;
        *facts.borrow_mut() = Some(fa);
        Ok(b)
    })?;
    if let Some(f) = facts.borrow().as_ref() {
        describe_fill(f, &eu.order, &l.p.m);
    }
    lock_guard(a, &b, now)?;
    finish(a, "proposal of a unilateral close (price evidence, DESIGN 4.5d)", b, &fk.keys, net, &mut node)
}

pub fn propose_liquidate_cmd(a: &Args) -> R<()> {
    let mut own = vec!["pair", "n", "proposer"];
    own.extend(EV_FLAGS);
    only_with(a, &own)?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let pairs = load_pairs(a, &mut node, net)?;
    let parts = parts_of(a, &pairs)?;
    let m = pairs[0].p.m.clone();
    let now = now_daa(a, &mut node)?;
    let eu = evidence_use(a, &mut node, net, m.scale, now)?;
    for (i, (p, _)) in parts.iter().enumerate() {
        print_clock(&format!("pair {i}"), p, now);
        print_report(&check_evidence_on(&eu.order, &m, Some(p.clock()), now), &m, now);
    }
    let fk = funder_and_keys(a, true)?.expect("funder");
    let proposer = match a.get("proposer") {
        Some(p) => keys::party(p, net)?,
        None => fk.key,
    };
    let sd = if eu.order.is_ask() { LONG } else { SHORT };
    for (p, n) in &parts {
        println!("propose the liquidation of the {} of {}", side_word(sd), short_part(p, *n));
        print_proposal(p, *n, KIND_LIQ, sd, eu.order.quote(), &proposer);
    }
    let opts = fee_opts(a)?;
    let facts = std::cell::RefCell::new(None);
    let members: Vec<Member> = parts.iter().map(|(p, n)| Member::Propose(Leg { pair: p.clone(), n: *n, act: Act::Liq { side: sd, proposer } })).collect();
    let b = build_funded(a, net, &mut node, &fk.key, &|f| {
        let (b, fa) = evidence_tx_facts(&members, &eu, f, &opts)?;
        *facts.borrow_mut() = Some(fa);
        Ok(b)
    })?;
    if let Some(f) = facts.borrow().as_ref() {
        describe_fill(f, &eu.order, &m);
    }
    lock_guard(a, &b, now)?;
    finish(a, if parts.len() == 1 { "proposal of a liquidation (price evidence, DESIGN 4.5d)" } else { "proposal of a block of liquidations (price evidence, DESIGN 4.5d)" }, b, &fk.keys, net, &mut node)
}

pub fn challenge_cmd(a: &Args) -> R<()> {
    let mut own = vec!["pending"];
    own.extend(EV_FLAGS);
    only_with(a, &own)?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let ws = load_pendings(a, &mut node, net)?;
    let m = ws[0].w.m.clone();
    let now = now_daa(a, &mut node)?;
    let eu = evidence_use(a, &mut node, net, m.scale, now)?;
    for (i, w) in ws.iter().enumerate() {
        println!("pending part {i}: {}; its proposal's clock P is DAA {}: counter-evidence counts only for its rest from then on", describe_claim(w), w.daa);
        print_report(&check_evidence_on(&eu.order, &m, Some(w.daa as i64), now), &m, now);
        println!("  {}", finalize_words(w, now));
    }
    let fk = funder_and_keys(a, true)?.expect("funder");
    let bonds: i64 = ws.iter().map(|w| w.w.bond).sum();
    println!("challenge: each part returns to the pair it was cut from (fresh clock); the bonds, {} KAS, go to the executor (the challenger)", kas(bonds));
    let opts = fee_opts(a)?;
    let facts = std::cell::RefCell::new(None);
    let members: Vec<Member> = ws.iter().map(|w| Member::Challenge(w.clone())).collect();
    let b = build_funded(a, net, &mut node, &fk.key, &|f| {
        let (b, fa) = evidence_tx_facts(&members, &eu, f, &opts)?;
        *facts.borrow_mut() = Some(fa);
        Ok(b)
    })?;
    if let Some(f) = facts.borrow().as_ref() {
        describe_fill(f, &eu.order, &m);
    }
    lock_guard(a, &b, now)?;
    finish(a, "challenge of a pending claim (counter-evidence, DESIGN 4.5d)", b, &fk.keys, net, &mut node)
}

pub fn cure_cmd(a: &Args) -> R<()> {
    let mut own = vec!["pending"];
    own.push("daa");
    only_with(a, &own)?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let w = load_pending(a, "pending", &mut node, net)?;
    let now = now_daa(a, &mut node)?;
    let loser = if w.w.side == LONG { w.w.long_key } else { w.w.short_key };
    println!("pending part: {}; {}", describe_claim(&w), finalize_words(&w, now));
    println!(
        "cure: the {} (key {}, --key) moves its bound to {} KAS, adds {} KAS of margin and repays the proposer its refund {} KAS",
        side_word(w.w.side),
        to_hex(&loser),
        kas(w.w.cure_bound),
        kas(w.w.cure_add()),
        kas(w.w.refund)
    );
    let fk = funder_and_keys(a, true)?.expect("funder");
    let opts = fee_opts(a)?;
    let b = build_funded(a, net, &mut node, &fk.key, &|f| kpf_protocol::evidence::cure(&w, f, &opts))?;
    finish(a, "cure of a pending liquidation (DESIGN 4.5d)", b, &fk.keys, net, &mut node)
}

pub fn finalize_cmd(a: &Args) -> R<()> {
    let own = vec!["pending", "daa"];
    only_with(a, &own)?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let w = load_pending(a, "pending", &mut node, net)?;
    let now = now_daa(a, &mut node)?;
    println!("pending part: {}; {}", describe_claim(&w), finalize_words(&w, now));
    if let Some(n) = now {
        if n < w.finalize_from() {
            let msg = format!("this part can be finalized only from DAA {} (its UTXO is {} DAA old, the window is {})", w.finalize_from(), n.saturating_sub(w.daa), w.w.m.challenge_daa);
            if a.has("submit") {
                return Err(msg);
            }
            println!("WARNING: {msg}");
        }
    }
    if w.w.kind == KIND_CLOSE {
        let (lp, sp) = w.w.finalize_close(w.value);
        println!("finalize: the long receives {} KAS, the short {} KAS (the closer's payout includes its refund)", kas(lp), kas(sp));
    } else {
        let (fv, pay) = w.w.finalize_forfeit(w.value);
        println!(
            "finalize: forfeit and keep: the {} owns both sides of the part ({} KAS); the proposer {} receives {} KAS (bond and fee)",
            side_word(3 - w.w.side),
            kas(fv),
            to_hex(&w.w.proposer_key),
            kas(pay)
        );
    }
    let fk = funder_and_keys(a, true)?.expect("funder");
    let opts = fee_opts(a)?;
    let b = build_funded(a, net, &mut node, &fk.key, &|f| kpf_protocol::evidence::finalize(&w, f, &opts))?;
    finish(a, "finalize of a pending claim (DESIGN 4.5d)", b, &fk.keys, net, &mut node)
}

// ---------------------------------------------------------------- watch

/// Every object file of a directory (the out-dir of `kpf` builders, or files a watcher collects).
fn scan_dir(dir: &str) -> R<Vec<ObjFile>> {
    let mut v = vec![];
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{dir}: {e}"))?;
    let mut paths: Vec<_> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    paths.sort();
    for p in paths {
        if let Ok(o) = ObjFile::load(&p.display().to_string()) {
            v.push(o);
        }
    }
    Ok(v)
}

/// `watch`: for the user's pairs, every pending claim against them (a KpfPending of the pair's covenant id and keys),
/// how long before it can be finalized, whether one of the given resting orders refutes it now, and the command to do so.
pub fn watch_cmd(a: &Args) -> R<()> {
    a.only(&["pair", "pending", "scan", "order", "seller", "node", "daa", "funder", "json"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let now = now_daa(a, &mut node)?;
    let pair_files = a.all("pair");
    if pair_files.is_empty() {
        return Err("--pair <file> is required (one per pair to watch)".into());
    }
    // The pairs: their covenant id and keys (a pair spent by a proposal is gone from the node, but its file still names it).
    let mut pairs: Vec<(String, ObjFile, Pair, bool)> = vec![];
    for f in &pair_files {
        let mut o = ObjFile::load(f)?;
        let p = match o.decoded()? {
            Decoded::Pair(p) => p,
            _ => return Err(format!("{f} is not a pair")),
        };
        let live = match &mut node {
            Some(n) => refresh(n, &mut o, net).is_ok(),
            None => true,
        };
        pairs.push((f.to_string(), o, p, live));
    }
    // The candidate pending parts: given files and every pending object of the scanned directories.
    let mut cands: Vec<(String, ObjFile)> = vec![];
    for f in a.all("pending") {
        cands.push((f.to_string(), ObjFile::load(f)?));
    }
    for d in a.all("scan") {
        for o in scan_dir(d)? {
            if matches!(o.decoded(), Ok(Decoded::Pending(_))) {
                cands.push((format!("{d}/{}:{}", o.op.transaction_id, o.op.index), o));
            }
        }
    }
    let orders: Vec<(String, kpf_protocol::kob::Evidence)> = a
        .all("order")
        .iter()
        .map(|f| {
            let mut k = KobFile::load(f)?;
            if let Some(n) = &mut node {
                refresh(n, &mut k.obj, net)?;
                if let Some(c) = &mut k.custody {
                    refresh(n, c, net)?;
                }
            }
            Ok((f.to_string(), k.evidence()?))
        })
        .collect::<R<_>>()?;
    let node_flag = a.get("node").map(|u| format!(" --node {u}")).unwrap_or_default();
    let funder = a.get("funder").unwrap_or("FUNDER.json");
    let mut report: Vec<Value> = vec![];
    let mut found = 0;
    for (pf, o, p, live) in &pairs {
        let mut entry = json!({"pair": pf, "covenantId": o.covenant_id.to_string(), "live": live, "pending": []});
        if !a.has("json") {
            println!(
                "pair {pf}: {} tokens, range {} .. {} KAS{}",
                fmt_units(p.size as i128, p.m.scale as i128),
                kas(p.bl),
                kas(p.bs),
                if *live { "" } else { "  (its UTXO is no longer at the node: spent, perhaps by a proposal)" }
            );
        }
        for (wf, wo) in &cands {
            let Ok(Decoded::Pending(w)) = wo.decoded() else { continue };
            if wo.covenant_id != o.covenant_id || w.long_key != p.long_key || w.short_key != p.short_key {
                continue;
            }
            let mut wo = wo.clone();
            let still = match &mut node {
                Some(n) => refresh(n, &mut wo, net).is_ok(),
                None => wo.daa.is_some(),
            };
            if !still {
                if !a.has("json") {
                    println!("  {wf}: a pending part of this pair that is no longer live (challenged, cured or finalized), or whose DAA is unknown");
                }
                continue;
            }
            let lw = wo.live_pending()?;
            found += 1;
            let threatened = if lw.w.kind == KIND_CLOSE { 3 - lw.w.side } else { lw.w.side };
            let mut refuters = vec![];
            for (of, ev) in &orders {
                let kind_ok = ev.is_ask() == lw.w.counter_is_ask();
                let contra = lw.w.contradicts(ev.quote());
                let rep = check_evidence_on(ev, &lw.w.m, Some(lw.daa as i64), now);
                let ok = kind_ok && contra && rep.problems.is_empty() && rep.dead.is_empty();
                refuters.push((of.clone(), ok, kind_ok, contra, rep));
            }
            let bound = lw.w.contradiction_bound();
            if a.has("json") {
                entry["pending"].as_array_mut().expect("array").push(json!({
                    "file": wf, "claim": describe_claim(&lw), "threatenedSide": side_word(threatened), "proposalDaa": lw.daa,
                    "finalizeFrom": lw.finalize_from(), "bond": lw.w.bond, "refutationBound": bound, "counterKind": if lw.w.counter_is_ask() { "ask" } else { "bid" },
                    "cureBound": lw.w.can_cure().then_some(lw.w.cure_bound),
                    "refutingOrders": refuters.iter().filter(|r| r.1).map(|r| json!({"order": r.0, "restEnd": r.4.rest_end})).collect::<Vec<_>>(),
                }));
                continue;
            }
            println!("  PENDING {wf}: {}; it threatens the {}", describe_claim(&lw), side_word(threatened));
            println!("    {}", finalize_words(&lw, now));
            match bound {
                Some(b) => println!(
                    "    refuted by a {} {} {} KAS resting {} DAA from DAA {} (the proposal's clock) and fillable {} DAA after; the challenger receives the bond {} KAS",
                    if lw.w.counter_is_ask() { "ask" } else { "bid" },
                    if lw.w.counter_is_ask() { "<=" } else { ">=" },
                    kas(b),
                    lw.w.m.min_rest_daa,
                    lw.daa,
                    lw.w.m.min_evidence_life,
                    kas(lw.w.bond)
                ),
                None => println!("    no quote refutes it"),
            }
            if orders.is_empty() {
                println!("    no resting orders given (--order ORDER.json): none checked");
            }
            for (of, ok, kind_ok, contra, rep) in &refuters {
                if *ok {
                    let wait = match now {
                        Some(n) if n < rep.rest_end => format!(" once it has rested (from DAA {}, {} DAA from now)", rep.rest_end, rep.rest_end - n),
                        _ => String::new(),
                    };
                    let seller = if lw.w.counter_is_ask() { String::new() } else { format!(" --seller {}", a.get("seller").unwrap_or("YOUR-TOKENS.json")) };
                    println!("    REFUTES IT: {of}{wait}. Run:");
                    println!("      kpf challenge --pending {wf} --evidence {of}{seller} --funder {funder}{node_flag} --submit");
                } else {
                    let why = if !kind_ok {
                        format!("it is {}; this claim needs {}", if lw.w.counter_is_ask() { "a bid" } else { "an ask" }, if lw.w.counter_is_ask() { "an ask" } else { "a bid" })
                    } else if !contra {
                        "its quote does not contradict the claim by the margin g".to_string()
                    } else {
                        rep.problems.iter().chain(rep.dead.iter()).cloned().collect::<Vec<_>>().join("; ")
                    };
                    println!("    {of}: does not refute it: {why}");
                }
            }
            if lw.w.can_cure() {
                let lk = if lw.w.side == LONG { "LONG-KEY.json" } else { "SHORT-KEY.json" };
                println!(
                    "    or the {} cures it, adding {} KAS of margin (bound to {} KAS): kpf cure --pending {wf} --key {lk} --funder {funder}{node_flag} --submit",
                    side_word(lw.w.side),
                    kas(lw.w.cure_add()),
                    kas(lw.w.cure_bound)
                );
            }
            println!("    after the window anyone may end it: kpf finalize --pending {wf} --funder {funder}{node_flag} --submit");
        }
        report.push(entry);
    }
    if a.has("json") {
        println!("{}", serde_json::to_string_pretty(&json!({"daa": now, "pairs": report})).expect("json"));
    } else if found == 0 {
        println!(
            "no pending claim found against these pairs among {} pending object(s) given. A node lists UTXOs by address only, not by covenant id, so a pending part is found from its object file: pass --pending FILE or --scan DIR (where proposals are written)",
            cands.len()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------- evidence (the check)

pub fn evidence_cmd(a: &Args) -> R<()> {
    a.only(&["order", "pair", "pending", "daa", "node", "json", "allow-dead-evidence"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let file = load_kob(a, "order", &mut node, net)?;
    if a.has("pending") {
        return counter_evidence(a, &file, &mut node, net);
    }
    let l = load_obj(a, "pair", &mut node, net)?.live_pair()?;
    let now = now_daa(a, &mut node)?;
    let m = &l.p.m;
    let ev = file.evidence()?;
    let rep = check_evidence_on(&ev, m, Some(l.clock()), now);
    let mut problems = rep.problems.clone();
    if let Err(w) = supports_token_program(m) {
        problems.push(w);
    }
    let verdicts = pair_verdicts(&rep, &l.p);
    if a.has("json") {
        let v: Vec<Value> = verdicts.iter().map(|v| json!({"path": v.path, "ok": v.ok, "why": v.why})).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "kind": rep.kind, "quote": rep.quote, "capacity": rep.capacity, "covers": rep.covers, "exposed": rep.exposed, "restEnd": rep.rest_end,
                "clockStart": l.clock(), "since": l.p.since, "handoverFrom": l.handover_from(), "liveUntil": rep.live_until, "dead": rep.dead,
                "usable": problems.is_empty() && rep.waiting.is_empty() && rep.dead.is_empty(), "problems": problems, "waiting": rep.waiting, "notes": rep.notes, "pair": v,
            }))
            .expect("json")
        );
    } else {
        let mut r2 = rep.clone();
        r2.problems = problems.clone();
        print_clock("pair", &l, now);
        print_report(&r2, m, now);
        println!("what it lets this pair do ({} tokens, range {} .. {} KAS):", fmt_units(l.p.size as i128, m.scale as i128), kas(l.p.bl), kas(l.p.bs));
        for v in &verdicts {
            println!("  {:<36} {}  {}", v.path, if v.ok { "yes" } else { "no " }, v.why);
        }
        println!(
            "the executor funds the evidence fill itself: for an ask it pays the ask's price for every token it takes (and keeps the tokens); for a bid it sells tokens of its own (--seller). See docs/TESTNET.md."
        );
    }
    let dead_ok = rep.dead.is_empty() || a.has("allow-dead-evidence");
    if problems.is_empty() && rep.waiting.is_empty() && dead_ok {
        Ok(())
    } else if problems.is_empty() && rep.waiting.is_empty() {
        Err("this order is dead (soft-expired): it is refused as evidence (pass --allow-dead-evidence only to sweep it in a test)".into())
    } else {
        Err("this order cannot serve as evidence for this pair now".into())
    }
}

/// `evidence --pending`: can this order refute the pending claim (DESIGN 4.5d): the opposite kind, a quote beyond the
/// claim by the margin g, and rested on the proposal's clock P with the evidence life after it.
fn counter_evidence(a: &Args, file: &KobFile, node: &mut Option<Node<WsTransport>>, net: Network) -> R<()> {
    let w = load_pending(a, "pending", node, net)?;
    let now = now_daa(a, node)?;
    let m = &w.w.m;
    let ev = file.evidence()?;
    let rep = check_evidence_on(&ev, m, Some(w.daa as i64), now);
    let kind_ok = ev.is_ask() == w.w.counter_is_ask();
    let contra = w.w.contradicts(ev.quote());
    let bound = w.w.contradiction_bound();
    let refutes = kind_ok && contra && rep.usable();
    if a.has("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "claim": describe_claim(&w), "counterKind": if w.w.counter_is_ask() { "ask" } else { "bid" }, "refutationBound": bound,
                "kind": rep.kind, "quote": rep.quote, "contradicts": kind_ok && contra, "restEnd": rep.rest_end, "finalizeFrom": w.finalize_from(),
                "problems": rep.problems, "waiting": rep.waiting, "dead": rep.dead, "refutes": refutes,
            }))
            .expect("json")
        );
    } else {
        println!("pending part: {}; proposal clock P = DAA {}; {}", describe_claim(&w), w.daa, finalize_words(&w, now));
        print_report(&rep, m, now);
        match (kind_ok, contra, bound) {
            (false, _, _) => println!("  does not refute it: the claim needs {}", if w.w.counter_is_ask() { "an ask" } else { "a bid" }),
            (true, false, Some(b)) => println!("  does not refute it: a quote {} {} KAS is needed (margin g {} bps)", if w.w.counter_is_ask() { "<=" } else { ">=" }, kas(b), m.g_bps),
            (true, false, None) => println!("  does not refute it: no quote does"),
            (true, true, _) if refutes => println!("  REFUTES the claim now: kpf challenge --pending <file> --evidence <this order>"),
            (true, true, _) => println!("  its quote contradicts the claim; it refutes it once the problems above are gone"),
        }
    }
    if refutes {
        Ok(())
    } else {
        Err("this order does not refute the pending claim now".into())
    }
}

// ---------------------------------------------------------------- kob-file

pub fn kob_file_cmd(a: &Args) -> R<()> {
    a.only(&["redeem", "outpoint", "amount", "daa", "covenant-id", "custody", "out", "node"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let redeem = from_hex(a.need("redeem")?).map_err(e)?;
    let op = a.need("outpoint")?;
    let (txid, idx) = op.split_once(':').ok_or("--outpoint is TXID:INDEX")?;
    let op = TransactionOutpoint { transaction_id: TransactionId::from_bytes(hex32(txid).map_err(e)?), index: idx.parse().map_err(|_| "the outpoint index is not an integer")? };
    let obj = KobObj::decode(&redeem).map_err(e)?;
    let mut o = ObjFile {
        op,
        amount: if a.has("amount") { int(a, "amount")? as u64 } else { 0 },
        daa: if a.has("daa") { Some(int(a, "daa")? as u64) } else { None },
        covenant_id: match a.get("covenant-id") {
            Some(h) => kaspa_consensus_core::Hash::from_bytes(hex32(h).map_err(e)?),
            None => Default::default(),
        },
        redeem,
    };
    if let Some(n) = &mut node {
        refresh(n, &mut o, net)?;
    } else if !a.has("amount") || !a.has("daa") {
        return Err("give --amount (sompi), --daa and --covenant-id of the UTXO, or --node to read them from the node (the DAA is not assumed: the evidence rest depends on it)".into());
    }
    let custody = match a.get("custody") {
        Some(c) => Some(ObjFile::load(c)?),
        None => None,
    };
    if matches!(obj, KobObj::Ask(_)) && custody.is_none() {
        println!("note: an ask needs its custody UTXO (the token UTXO owned by its covenant id): add it with --custody <file>");
    }
    let doc = KobFile { obj: o, custody }.to_json(net);
    match a.get("out") {
        Some(p) => {
            write_json(Path::new(p), &doc)?;
            println!("written: {p} ({})", obj.kind());
        }
        None => println!("{}", serde_json::to_string_pretty(&doc).expect("json")),
    }
    Ok(())
}
