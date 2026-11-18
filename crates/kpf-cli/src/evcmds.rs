//! The commands of the price-evidence paths: `close` (P5), `liquidate` (P6), `liquidate-transfer` (P7), `evidence` (the
//! check of an order) and `kob-file` (an order file from a redeem script). Everything is offline unless `--node` is
//! given; nothing is submitted without `--submit`.
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

// ---------------------------------------------------------------- close (P5)

pub fn close_cmd(a: &Args) -> R<()> {
    let mut own = vec!["pair", "side", "n"];
    own.extend(EV_FLAGS);
    only_with(a, &own)?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load_obj(a, "pair", &mut node, net)?.live_pair()?;
    let sd = side(a.need("side")?)?;
    let n = tokens(a, "n", l.p.m.scale)?;
    let now = now_daa(a, &mut node)?;
    let eu = evidence_use(a, &mut node, net, l.p.m.scale, now)?;
    print_clock("pair", &l, now);
    print_report(&check_evidence_on(&eu.order, &l.p.m, Some(l.clock()), now), &l.p.m, now);
    let fk = funder_and_keys(a, true)?.expect("funder");
    let owner = if sd == LONG { l.p.long_key } else { l.p.short_key };
    println!("unilateral close of {} by the {}: the owner's key {} must sign (--key)", short_part(&l, n), if sd == LONG { "long" } else { "short" }, to_hex(&owner));
    let opts = fee_opts(a)?;
    let facts = std::cell::RefCell::new(None);
    let leg = Leg { pair: l.clone(), n, act: Act::Close { side: sd } };
    let b = build_funded(a, net, &mut node, &fk.key, &|f| {
        let (b, fa) = evidence_tx_facts(std::slice::from_ref(&leg), &eu, f, &opts)?;
        *facts.borrow_mut() = Some(fa);
        Ok(b)
    })?;
    if let Some(f) = facts.borrow().as_ref() {
        describe_fill(f, &eu.order, &l.p.m);
    }
    lock_guard(a, &b, now)?;
    finish(a, "unilateral close (price evidence)", b, &fk.keys, net, &mut node)
}

// ---------------------------------------------------------------- liquidate (P6)

pub fn liquidate_cmd(a: &Args) -> R<()> {
    let mut own = vec!["pair", "n"];
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
    let loser = if eu.order.is_ask() { "long" } else { "short" };
    for (p, n) in &parts {
        println!("liquidate the {loser} of {} (the {loser} receives nothing; the winner receives the part less the fee)", short_part(p, *n));
    }
    let opts = fee_opts(a)?;
    let facts = std::cell::RefCell::new(None);
    let legs: Vec<Leg> = parts.iter().map(|(p, n)| Leg { pair: p.clone(), n: *n, act: Act::Liq { side: if eu.order.is_ask() { LONG } else { SHORT } } }).collect();
    let b = build_funded(a, net, &mut node, &fk.key, &|f| {
        let (b, fa) = evidence_tx_facts(&legs, &eu, f, &opts)?;
        *facts.borrow_mut() = Some(fa);
        Ok(b)
    })?;
    if let Some(f) = facts.borrow().as_ref() {
        describe_fill(f, &eu.order, &m);
    }
    lock_guard(a, &b, now)?;
    finish(a, if parts.len() == 1 { "liquidation (price evidence)" } else { "liquidation of a block (price evidence)" }, b, &fk.keys, net, &mut node)
}

// ---------------------------------------------------------------- liquidate-transfer (P7)

pub fn liquidate_transfer_cmd(a: &Args) -> R<()> {
    let mut own = vec!["pair", "n", "x", "k", "entrant-order"];
    own.extend(EV_FLAGS);
    only_with(a, &own)?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load_obj(a, "pair", &mut node, net)?.live_pair()?;
    let entrant = load_obj(a, "entrant-order", &mut node, net)?.live_open()?;
    let m = l.p.m.clone();
    let n = if a.has("n") { tokens(a, "n", m.scale)? } else { l.p.size };
    let now = now_daa(a, &mut node)?;
    let eu = evidence_use(a, &mut node, net, m.scale, now)?;
    print_clock("pair", &l, now);
    print_report(&check_evidence_on(&eu.order, &m, Some(l.clock()), now), &m, now);
    let sd = if eu.order.is_ask() { LONG } else { SHORT };
    let x = if a.has("x") { price(a, "x")? } else { p7_default_x(&l.p, sd, eu.order.quote(), &entrant.o).map_err(e)? };
    let k = int_or(a, "k", 0)?;
    let fk = funder_and_keys(a, true)?.expect("funder");
    println!(
        "liquidate by transfer: the {} of {} is sold at {} KAS to the {} order of {}; the {} keeps its position and receives the entrant's payment less the fee",
        if sd == LONG { "long" } else { "short" },
        short_part(&l, n),
        kas(x),
        if sd == LONG { "long" } else { "short" },
        to_hex(&entrant.o.owner),
        if sd == LONG { "short" } else { "long" },
    );
    let opts = fee_opts(a)?;
    let facts = std::cell::RefCell::new(None);
    let leg = Leg { pair: l.clone(), n, act: Act::LiqTransfer { side: sd, x, entrant: entrant.clone(), k } };
    let b = build_funded(a, net, &mut node, &fk.key, &|f| {
        let (b, fa) = evidence_tx_facts(std::slice::from_ref(&leg), &eu, f, &opts)?;
        *facts.borrow_mut() = Some(fa);
        Ok(b)
    })?;
    if let Some(f) = facts.borrow().as_ref() {
        describe_fill(f, &eu.order, &m);
    }
    lock_guard(a, &b, now)?;
    finish(a, "liquidation by transfer (price evidence)", b, &fk.keys, net, &mut node)
}

// ---------------------------------------------------------------- evidence (the check)

pub fn evidence_cmd(a: &Args) -> R<()> {
    a.only(&["order", "pair", "daa", "node", "json", "allow-dead-evidence"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let file = load_kob(a, "order", &mut node, net)?;
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
