//! The commands of oracle-only markets (docs/DESIGN-ORACLE-IMPL.md, third slice; runbook: docs/TESTNET.md). Offline
//! unless `--node` is given, nothing submitted without `--submit`, every transaction verified locally first, as the
//! book-market commands.

use std::path::{Path, PathBuf};

use kaspa_consensus_core::Hash;
use kpf_protocol::bond::{Bond, BondParams};
use kpf_protocol::error::{from_hex, to_hex};
use kpf_protocol::judge::{Js, JudgeParams, NO, STATUS_FREE, STATUS_UNBONDING, YES};
use kpf_protocol::network::{p2pk_address, p2sh_address, Network};
use kpf_protocol::oracle::*;
use kpf_protocol::oracle_build::*;
use kpf_protocol::state::{KIND_CLOSE, KIND_LIQ, LONG, SHORT};
use kpf_protocol::tx::{rpc_json, KeySet, KeyUtxo, MIN_FEE_RATE};
use kpf_protocol::verify::verify;
use serde_json::{json, Value};

use crate::args::*;
use crate::cmds::{connect, e, int, int_or, network, price, price_or, side, R};
use crate::files::*;
use crate::keys;
use crate::node::{Node, Transport, WsTransport};

const DAA_PER_SECOND: i64 = 10;

fn hex32(s: &str) -> R<[u8; 32]> {
    kpf_protocol::error::hex32(s).map_err(e)
}

// ---------------------------------------------------------------- market files

/// A market file: the block (309 bytes, hex), its id, and the parameters in plain form.
pub fn market_json(m: &OMarket) -> Value {
    let (b, j, p) = (&m.bond, &m.judge, &m.p);
    json!({
        "format": "kpf-oracle-market/1",
        "id": to_hex(&m.id()),
        "block": to_hex(&m.bytes()),
        "feedHash": to_hex(&m.feed_hash),
        "bond": { "rhoBps": b.rho_bps, "stepDaa": b.step, "termDaa": b.term, "tailDaa": b.tail, "rateDaa": b.rate, "unbondDaa": b.unbond },
        "judge": { "muBps": j.mu_bps, "ageDaa": j.age, "unbondDaa": j.unbond, "roundDaa": j.round, "burnBps": j.burn_bps, "minStake": j.min_stake },
        "pair": {
            "scale": p.scale, "priceMax": p.price_max, "mmrBps": p.mmr_bps, "gBps": p.g_bps, "liqFeeBps": p.liq_fee_bps,
            "keeperFeeMin": p.keeper_fee_min, "minSize": p.min_size, "carrier": p.carrier, "idleDaa": p.idle_daa,
            "challengeDaa": p.challenge_daa, "lateDaa": p.late_daa, "quietDaa": p.quiet_daa, "sigmaBps": p.sigma_bps,
            "betaBps": p.beta_bps, "bondMin": p.bond_min,
        },
    })
}

/// The market of a market file: decoded from its block (the id must be the hash of the body), and the plain
/// parameters, if given, must say the same.
pub fn market_from(path: &str) -> R<OMarket> {
    let v = read_json(path)?;
    let v = v.get("market").unwrap_or(&v);
    let block = from_hex(v.get("block").and_then(Value::as_str).ok_or(format!("{path}: no `block`"))?).map_err(e)?;
    let m = OMarket::decode(&block).map_err(|x| format!("{path}: {x}"))?;
    if let Some(id) = v.get("id").and_then(Value::as_str) {
        if hex32(id)? != m.id() {
            return Err(format!("{path}: the `id` is not the block's id (blake3 of its body)"));
        }
    }
    if v.get("pair").is_some() && market_json(&m)["pair"] != v["pair"] {
        return Err(format!("{path}: the plain parameters differ from the block's: the block is what the contracts read"));
    }
    Ok(m)
}

fn opt(a: &Args, name: &str, d: i64) -> R<i64> {
    int_or(a, name, d)
}

/// `oracle-market`: a market block from a feed definition (testnet defaults, each overridable), with its id and the
/// worst-case figures of an example pair.
pub fn market_cmd(a: &Args) -> R<()> {
    a.only(&["feed", "out", "rho-bps", "step-daa", "term-daa", "tail-daa", "rate-daa", "slot-unbond-daa", "mu-bps", "stake-age-daa", "stake-unbond-daa", "round-daa", "burn-bps", "min-stake", "scale", "price-max", "mmr-bps", "g-bps", "liq-fee-bps", "keeper-fee-min", "min-size", "carrier", "idle-daa", "window-daa", "late-daa", "quiet-daa", "sigma-bps", "beta-bps", "bond-min", "json"])?;
    let feed = std::fs::read(a.need("feed")?).map_err(|x| format!("--feed: {x}"))?;
    let d = OMarket::testnet(feed_hash_of(&feed));
    let m = OMarket {
        bond: BondParams {
            rho_bps: opt(a, "rho-bps", d.bond.rho_bps)?,
            step: opt(a, "step-daa", d.bond.step)?,
            term: opt(a, "term-daa", d.bond.term)?,
            tail: opt(a, "tail-daa", d.bond.tail)?,
            rate: opt(a, "rate-daa", d.bond.rate)?,
            unbond: opt(a, "slot-unbond-daa", d.bond.unbond)?,
        },
        judge: JudgeParams {
            mu_bps: opt(a, "mu-bps", d.judge.mu_bps)?,
            age: opt(a, "stake-age-daa", d.judge.age)?,
            unbond: opt(a, "stake-unbond-daa", d.judge.unbond)?,
            round: opt(a, "round-daa", d.judge.round)?,
            burn_bps: opt(a, "burn-bps", d.judge.burn_bps)?,
            min_stake: if a.has("min-stake") { kas(a.need("min-stake")?)? } else { d.judge.min_stake },
        },
        feed_hash: d.feed_hash,
        p: OParams {
            scale: opt(a, "scale", d.p.scale)?,
            price_max: price_or(a, "price-max", d.p.price_max)?,
            mmr_bps: opt(a, "mmr-bps", d.p.mmr_bps)?,
            g_bps: opt(a, "g-bps", d.p.g_bps)?,
            liq_fee_bps: opt(a, "liq-fee-bps", d.p.liq_fee_bps)?,
            keeper_fee_min: if a.has("keeper-fee-min") { kas(a.need("keeper-fee-min")?)? } else { d.p.keeper_fee_min },
            min_size: opt(a, "min-size", d.p.min_size)?,
            carrier: if a.has("carrier") { kas(a.need("carrier")?)? } else { d.p.carrier },
            idle_daa: opt(a, "idle-daa", d.p.idle_daa)?,
            challenge_daa: opt(a, "window-daa", d.p.challenge_daa)?,
            late_daa: opt(a, "late-daa", d.p.late_daa)?,
            quiet_daa: opt(a, "quiet-daa", d.p.quiet_daa)?,
            sigma_bps: opt(a, "sigma-bps", d.p.sigma_bps)?,
            beta_bps: opt(a, "beta-bps", d.p.beta_bps)?,
            bond_min: if a.has("bond-min") { kas(a.need("bond-min")?)? } else { d.p.bond_min },
        },
    };
    m.check().map_err(|x| format!("the parameters fail the contracts' bounds: {x}"))?;
    let out = PathBuf::from(a.get("out").unwrap_or("kpf-out/oracle-market.json"));
    write_json(&out, &market_json(&m))?;
    println!("market id   {}", to_hex(&m.id()));
    println!("feed hash   {}", to_hex(&m.feed_hash));
    println!("written     {}", out.display());
    print!("{}", words(&m, None));
    Ok(())
}

/// The parameters and the worst cases in plain words.
fn words(m: &OMarket, pair: Option<(i64, i64, i64, i64, i64, i64)>) -> String {
    let h = |d: i64| format!("{:.2} h", d as f64 / DAA_PER_SECOND as f64 / 3600.0);
    let k = |s: i64| fmt_kas(s as i128);
    let p = &m.p;
    let mut s = String::new();
    s += &format!("This market prices an underlying in KAS (sompi per whole unit of {} base units). It is not approved, listed or verified by anyone: anyone can create a market.\n", p.scale);
    s += &format!("Prices come only from each pair's underwriter: whoever vouches a price can move margin from one party to the other. A false price stands unless someone contests it within {} ({} DAA) of the claim landing.\n", h(p.challenge_daa), p.challenge_daa);
    let j = &m.judge;
    s += &format!(
        "A contest is judged by aged bonded KAS (the judge): each stake is at least {} KAS; the side committing at least {} times the other's takes the lead, and the leader wins once a round of {} passes without a change; stake counts only if it was bonded {} before the claim; a stake takes {} to leave; {} % of the losers' stake is burned and the rest goes to the winners. If colluders control that stake, false prices stand.\n",
        k(j.min_stake),
        j.mu_bps as f64 / 1e4,
        h(j.round),
        h(j.age),
        h(j.unbond),
        j.burn_bps as f64 / 100.0
    );
    s += &format!("A new market has no judge for its first {}: no stake can count before it is that old, so a claim made earlier cannot be contested.\n", h(j.age));
    s += &format!("A contest that lands after the window is ignored for the verdict, but the part still waits for the dispute (at least {}): a late contest delays a claim for the price of its fees.\n", h(j.round));
    s += &format!("An underwriter may underwrite up to {} times its bond. Its bond is never paid to you in this version: a refuted claim costs its proposer the claim's bond ({} % of the cover, at least {} KAS), {} % of it to the disputer and the rest to the wronged party.\n", m.bond.rho_bps as f64 / 1e4, p.sigma_bps as f64 / 100.0, k(p.bond_min), p.beta_bps as f64 / 100.0);
    s += &format!("A side is liquidated (it forfeits its whole margin to the other side) when a vouched price, moved {} % in its favour, is past its trigger at {} % maintenance. A close by a party is paid at the vouched price moved {} % against the closer.\n", p.g_bps as f64 / 100.0, p.mmr_bps as f64 / 100.0, p.g_bps as f64 / 100.0);
    s += &format!("The cover runs for a term of {} and is renewed by the underwriter at the agreed premium, taken half from each side's margin. If the underwriter stops, the pair can be closed or transferred without any price; once {} has passed after its cover's expiry (the lapse after {}, then {} more by the calendar; amending the pair does not postpone it), anyone settles it at its entry price.\n", h(m.bond.term), h(m.bond.tail + p.idle_daa), h(m.bond.tail), h(p.idle_daa));
    s += &format!("Until a party acts on the pair after its cover's expiry, the underwriter can still renew it late, even once the settle is due: each late renewal charges a full premium for a term that has partly or wholly passed and moves the settle date one term ({}) later; a renewal landing more than a term after the expiry it renews is the last. Any party entry after the expiry (an amend is enough) makes the pair unrenewable and fixes the settle date. A pair handed over, restored or cured after its expiry is never renewed.\n", h(m.bond.term));
    s += "Each side may add margin, and be cured, up to its own half of the cover rate from the entry price (the short up to half above it, rounded down, the long the rest below it), whatever the other side or a newcomer does; a side opened beyond its half keeps its bound but cannot widen it.\n";
    s += &format!("A claim must land within {} of the time it prices, and at least the contest window before the cover expires, or it is void. Each party entry waits {} after the last change of the pair.\n", h(p.late_daa), h(p.quiet_daa));
    s += "The winner of a pair bears every move beyond the loser's margin (gaps, a silent underwriter). The underwriter is never a party to its own pairs by key; a second key of the same owner cannot be detected.\n";
    if let Some((size, x, rl, rs, cr, prem)) = pair {
        let w = m.worst_case(size, rl, rs, cr, prem);
        s += &format!(
            "For a pair of {} units at {} KAS: the long's margin {} KAS and the short's {} KAS are each what an undisputed false claim can take; a claim's bond is {} KAS ({} to a disputer, {} to the wronged party if refuted); the premium is {} KAS a term.\n",
            fmt_units(size as i128, p.scale as i128),
            k(x),
            k(w.long_margin),
            k(w.short_margin),
            k(w.bond),
            k(w.disputer_share),
            k(w.defender_share),
            k(w.term_premium)
        );
        for x in payout_warnings(m, size, rl, rs, cr, prem) {
            s += &format!("WARNING: {x}\n");
        }
    } else if m.p.bond_min.max(kpf_protocol::state::PLAIN_DUST) < VIABLE_OUTPUT_MIN as i64 {
        s += &format!("WARNING: a claim's bond may be as small as {} KAS on a small pair: a contract pays it (at a cure or a settlement) in an output of its own, and an output below about {} KAS cannot be mined (KIP-9 storage mass); the tools top it up from the funder\n", k(m.p.bond_min.max(kpf_protocol::state::PLAIN_DUST)), k(VIABLE_OUTPUT_MIN as i64));
    }
    for w in market_warnings(m, DAA_PER_SECOND) {
        s += &format!("WARNING: {w}\n");
    }
    s
}

/// `oracle-check`: what a wallet checks before showing a market: the feed definition's hash is the block's, the id is
/// the block's, the bounds hold, the text renders plainly; then the parameters and the worst case in words. Refuses a
/// market that fails the bounds or whose feed text does not match.
pub fn check_cmd(a: &Args) -> R<()> {
    a.only(&["market", "feed", "size", "x", "long-rate", "short-rate", "cover-rate", "premium"])?;
    let m = market_from(a.need("market")?)?;
    let feed = std::fs::read(a.need("feed")?).map_err(|x| format!("--feed: {x}"))?;
    let fc = check_feed(&feed);
    if fc.hash != m.feed_hash {
        return Err(format!("REFUSED: the feed definition's hash {} is not the market's feed hash {}: this market is not about this text", to_hex(&fc.hash), to_hex(&m.feed_hash)));
    }
    m.check().map_err(|x| format!("REFUSED: the market fails the contracts' bounds ({x}); no order should be placed in it"))?;
    println!("market id {} (blake3 of the block's body: checked)", to_hex(&m.id()));
    println!("feed definition: hash matches");
    for (i, ch, why) in &fc.suspicious {
        println!("WARNING: byte {i} of the feed definition is {ch:?}: {why}; the text may not read as it renders");
    }
    for k in &fc.missing {
        println!("WARNING: the feed definition has no `{k}:` line");
    }
    let pair = if a.has("size") {
        let sc = m.p.scale;
        Some((parse_amount(a.need("size")?, sc)?, price(a, "x")?, price(a, "long-rate")?, price(a, "short-rate")?, price(a, "cover-rate")?, price_or(a, "premium", 0)?))
    } else {
        None
    };
    print!("{}", words(&m, pair));
    Ok(())
}

// ---------------------------------------------------------------- objects and funding

fn obj_json(sg: &OSigned, i: usize, obj: &OObj, net: Network) -> Value {
    let o = &sg.tx.outputs[i];
    let redeem = obj.redeem();
    let f = ObjFile { op: kaspa_consensus_core::tx::TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, amount: o.value, daa: None, covenant_id: o.covenant.map(|c| c.covenant_id).unwrap_or_default(), redeem: redeem.clone() };
    let mut v = f.to_json(net);
    v["kind"] = json!(obj.kind());
    v["state"] = json!(format!("{obj:?}"));
    v["address"] = json!(p2sh_address(&redeem, net));
    v
}

fn live_of<T>(f: &ObjFile, s: T) -> R<OLive<T>> {
    Ok(OLive { s, value: f.amount, id: (f.covenant_id != Hash::default()).then_some(f.covenant_id), op: f.op, daa: f.known_daa()? })
}

fn load(a: &Args, flag: &str, node: &mut Option<Node<WsTransport>>, net: Network) -> R<(ObjFile, OObj)> {
    let o = crate::cmds::load_obj(a, flag, node, net)?;
    let obj = OObj::decode(&o.redeem).map_err(|x| format!("--{flag}: {x}"))?;
    Ok((o, obj))
}

fn pair_of(a: &Args, flag: &str, node: &mut Option<Node<WsTransport>>, net: Network) -> R<OLive<OPair>> {
    match load(a, flag, node, net)? {
        (f, OObj::Pair(p)) => live_of(&f, p),
        (_, o) => Err(format!("--{flag} is a {}, not an oracle pair", o.kind())),
    }
}
fn pending_of(a: &Args, flag: &str, node: &mut Option<Node<WsTransport>>, net: Network) -> R<OLive<OPending>> {
    match load(a, flag, node, net)? {
        (f, OObj::Pending(w)) => live_of(&f, w),
        (_, o) => Err(format!("--{flag} is a {}, not an oracle pending part", o.kind())),
    }
}
fn slot_of(a: &Args, flag: &str, node: &mut Option<Node<WsTransport>>, net: Network) -> R<OLive<Bond>> {
    match load(a, flag, node, net)? {
        (f, OObj::Slot(b)) => live_of(&f, b),
        (_, o) => Err(format!("--{flag} is a {}, not an underwriting slot", o.kind())),
    }
}
/// A stake file (a free or unbonding stake, a receipt, a dispute, a pool): with `--node` its UTXO, value and DAA score
/// are read from the node (review 7; defect 3 of the first live run), without it the file is used as it is.
fn stake_at(path: &str, node: &mut Option<Node<WsTransport>>, net: Network) -> R<OLive<Js>> {
    stake_from(path, node, net)
}

fn stake_from<T: Transport>(path: &str, node: &mut Option<Node<T>>, net: Network) -> R<OLive<Js>> {
    let f = crate::cmds::with_node_facts(ObjFile::load(path)?, node, net)?;
    match OObj::decode(&f.redeem).map_err(e)? {
        OObj::Stake(s) => live_of(&f, s),
        o => Err(format!("{path} is a {}, not a judge stake", o.kind())),
    }
}
fn order_of(a: &Args, flag: &str, node: &mut Option<Node<WsTransport>>, net: Network) -> R<OLive<OOpen>> {
    match load(a, flag, node, net)? {
        (f, OObj::Order(o)) => live_of(&f, o),
        (_, o) => Err(format!("--{flag} is a {}, not an oracle order", o.kind())),
    }
}

/// The funder's key, every key file, and the funding UTXOs (a `--funding` file or the node).
fn funding(a: &Args, net: Network, node: &mut Option<Node<WsTransport>>) -> R<([u8; 32], KeySet, Vec<KeyUtxo>)> {
    let f = crate::cmds::funder_and_keys(a, true)?.expect("funder");
    let utxos = if let Some(p) = a.get("funding") {
        funding_utxos(&parse_utxos(&read_json(p)?)?, &f.key)
    } else if let Some(n) = node {
        let mut u = funding_utxos(&n.utxos(&[p2pk_address(&f.key, net)])?, &f.key);
        u.sort_by_key(|x| std::cmp::Reverse(x.amount));
        u.truncate(4);
        u
    } else {
        return Err("no funding: give --funding <utxos.json> or --node <ws-url>".into());
    };
    if utxos.is_empty() {
        return Err(format!("no plain UTXO of the funder {}", p2pk_address(&f.key, net)));
    }
    Ok((f.key, f.keys, utxos))
}

fn rate(a: &Args) -> R<u64> {
    match a.get("fee-rate") {
        None => Ok(MIN_FEE_RATE),
        Some(s) => s.parse::<u64>().map_err(|_| format!("--fee-rate `{s}` is not an integer")),
    }
}

/// The present as the transaction may prove it: `--daa N`, else the node's virtual DAA score.
fn now(a: &Args, node: &mut Option<Node<WsTransport>>) -> R<u64> {
    if a.has("daa") {
        return Ok(int(a, "daa")? as u64);
    }
    match node {
        Some(n) => n.check_network(),
        None => Err("give --daa N (the DAA the lock time proves) or --node <ws-url>".into()),
    }
}

/// The first DAA score at which consensus accepts `tx` (rusty-kaspa `check_sequence_lock`: an input with a relative
/// lock r is spendable once the DAA score exceeds `its UTXO's DAA + r - 1`; `check_tx_is_finalized`: a DAA lock time t
/// once it exceeds t), with what sets it; None when nothing locks it. Inputs with sequence 0 lock nothing beyond the
/// UTXO's existence and are skipped.
pub fn usable_from(tx: &kaspa_consensus_core::tx::Transaction, entries: &[kaspa_consensus_core::tx::UtxoEntry]) -> Option<(u64, String)> {
    const DISABLED: u64 = 1 << 63;
    const MASK: u64 = 0x0000_0000_ffff_ffff;
    let mut best: Option<(u64, String)> = None;
    let mut take = |from: u64, why: String| {
        if best.as_ref().is_none_or(|(b, _)| from > *b) {
            best = Some((from, why));
        }
    };
    for (i, (inp, e)) in tx.inputs.iter().zip(entries).enumerate() {
        if inp.sequence & DISABLED != 0 {
            continue;
        }
        let r = inp.sequence & MASK;
        if r > 0 {
            take(e.block_daa_score + r, format!("input {i} must be {r} DAA old: its UTXO is at DAA {}", e.block_daa_score));
        }
    }
    let all_max = tx.inputs.iter().all(|i| i.sequence == u64::MAX);
    if tx.lock_time > 0 && (tx.lock_time as i64) < kpf_protocol::state::LOCK_TIME_THRESHOLD && !all_max {
        take(tx.lock_time + 1, format!("lock time DAA {}", tx.lock_time));
    }
    best
}

/// What `finish` does with a transaction usable from DAA `from`: with the node's DAA `now`, a submission before `from`
/// is refused ("usable from DAA X (now Y, about Z s)"), a dry run says it is not yet usable; without a node the line is
/// information only.
pub fn lock_verdict(from: u64, why: &str, now: Option<u64>, submit: bool) -> R<String> {
    match now {
        Some(now) => {
            let line = format!("usable from DAA {from} (now {now}, about {:.1} s; {why})", from.saturating_sub(now) as f64 / DAA_PER_SECOND as f64);
            if now >= from {
                Ok(line)
            } else if submit {
                Err(format!("not submitted, the node would refuse it: {line}; nothing written"))
            } else {
                Ok(format!("NOT YET USABLE: {line}"))
            }
        }
        None => Ok(format!("usable from DAA {from} ({why}; no --node, so not compared with the present)")),
    }
}

/// Signs, verifies, prints, writes the transaction and the objects it creates, submits if asked.
fn finish(a: &Args, title: &str, b: &OBuilt, keys: &KeySet, net: Network, node: &mut Option<Node<WsTransport>>) -> R<()> {
    let sg = b.sign(keys).map_err(e)?;
    let v = verify(&sg.tx, &sg.entries);
    println!("== {title}");
    println!("txid {}  fee {} KAS  compute mass {}  storage mass {}  bytes {}", sg.tx.id(), fmt_kas(sg.fee as i128), sg.mass.compute, sg.mass.storage, sg.mass.size);
    for (i, l) in sg.input_labels.iter().enumerate() {
        println!("  in  {i}: {l} ({} units)", sg.units[i]);
    }
    for (i, l) in sg.output_labels.iter().enumerate() {
        println!("  out {i}: {l} {} KAS", fmt_kas(sg.tx.outputs[i].value as i128));
    }
    if !v.is_ok() {
        for x in v.errors() {
            println!("  FAILED: {x}");
        }
        return Err("verification failed: nothing written, nothing submitted".into());
    }
    // Relative locks (CSV, an input's sequence) and the lock time (CLTV): the node refuses the transaction before the
    // DAA they name ("sequence locks conditions was not met", live run 1). With --node it is compared with the node's
    // DAA score and a submission is refused here; offline the same line is printed for information.
    if let Some((from, why)) = usable_from(&sg.tx, &sg.entries) {
        let now = match node.as_mut() {
            Some(n) => Some(n.check_network()?),
            None => None,
        };
        println!("{}", lock_verdict(from, &why, now, a.has("submit"))?);
    }
    let save = |dry: bool| -> R<()> {
        let dir = out_dir(a, dry);
        let tag: String = sg.tx.id().to_string().chars().take(8).collect();
        for (i, obj, _) in &sg.created {
            let p = dir.join(format!("{tag}-out{i}-{}.json", obj.kind().replace(' ', "-")));
            let mut v = obj_json(&sg, *i, obj, net);
            if dry {
                v["dryRun"] = json!(true);
            }
            write_json(&p, &v)?;
            println!("written: {}", p.display());
        }
        let p = dir.join(format!("{tag}-tx.json"));
        write_json(&p, &json!({ "format": "kpf-tx/1", "title": title, "network": net.name(), "txid": sg.tx.id().to_string(), "dryRun": dry, "transaction": rpc_json(&sg.tx) }))?;
        println!("written: {}", p.display());
        Ok(())
    };
    if a.has("submit") {
        let n = node.as_mut().ok_or("--submit needs --node <ws-url>")?;
        // Files at the top level of the out-dir are those of submitted transactions only: a refused submission leaves its
        // files under `dry-run/`.
        match n.submit(&rpc_json(&sg.tx)) {
            Ok(id) => {
                println!("SUBMITTED: node accepted {id}");
                if !a.has("no-save") {
                    save(false)?;
                }
            }
            Err(x) => {
                if !a.has("no-save") {
                    save(true)?;
                }
                return Err(format!("the node refused the transaction (its files, if any, are under {}): {x}", out_dir(a, true).display()));
            }
        }
    } else {
        if !a.has("no-save") {
            save(true)?;
        }
        println!("not submitted: a dry run (its files are under {}, apart from the live ones; add --submit --node <ws-url> to send it, which writes the files again at {})", out_dir(a, true).display(), out_dir(a, false).display());
    }
    Ok(())
}

/// The subfolder of `--out-dir` that holds the files of a dry run (a transaction not submitted), so that the top level
/// holds only the files of transactions the node accepted.
pub const DRY_RUN_DIR: &str = "dry-run";

fn out_dir(a: &Args, dry: bool) -> PathBuf {
    let base = PathBuf::from(a.get("out-dir").unwrap_or("kpf-out"));
    if dry {
        base.join(DRY_RUN_DIR)
    } else {
        base
    }
}

const BUILD: &[&str] = &["funder", "funding", "key", "node", "fee-rate", "out-dir", "no-save", "submit", "daa"];

fn only(a: &Args, extra: &[&str]) -> R<()> {
    let mut v: Vec<&str> = BUILD.to_vec();
    v.extend_from_slice(extra);
    a.only(&v)
}

// ---------------------------------------------------------------- the slot and the stakes

/// `bond`: a new underwriting slot of `--owner` (default the funder) worth `--value` KAS in the market.
pub fn bond_cmd(a: &Args) -> R<()> {
    only(a, &["market", "value", "owner"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let m = market_from(a.need("market")?)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let owner = match a.get("owner") {
        Some(o) => keys::party(o, net)?,
        None => fk,
    };
    let daa = now(a, &mut node)?;
    let b = create_slot(&m, owner, kas(a.need("value")?)? as u64, daa, &fu, &fk, rate(a)?).map_err(e)?;
    println!("A slot of {} KAS can count up to {} KAS of cover (rho {}).", a.need("value")?, fmt_kas(capacity(kas(a.need("value")?)?, m.bond.rho_bps) as i128), m.bond.rho_bps as f64 / 1e4);
    finish(a, "bond: a new underwriting slot", &b, &keys, net, &mut node)
}

/// `unbond`: the slot's owner unbonds once every bucket's liability has ended (the ring advanced to empty).
pub fn unbond_cmd(a: &Args) -> R<()> {
    only(a, &["slot"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let sl = slot_of(a, "slot", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let b = &sl.s;
    let last = (0..kpf_protocol::bond::RING).rev().find(|&i| b.ring[i] != 0).map(|i| i as i64 + 1).unwrap_or(0);
    let free_at = ((b.j0 + last) * b.m.p.step + b.m.p.tail).max(0) as u64;
    let t = now(a, &mut node)?;
    if t < free_at && last > 0 {
        return Err(format!("the slot still counts cover until DAA {free_at} (the last expiry plus the tail): unbond from then"));
    }
    let (built, _) = slot_unbond(&sl, last, t.max(free_at), &fu, &fk, rate(a)?).map_err(e)?;
    finish(a, "unbond the slot", &built, &keys, net, &mut node)
}

/// `withdraw-bond`: an unbonding slot `U` old, its whole value to `--to` (default the owner).
pub fn withdraw_bond_cmd(a: &Args) -> R<()> {
    only(a, &["slot", "to"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let sl = slot_of(a, "slot", &mut node, net)?;
    let keys = crate::cmds::funder_and_keys(a, false)?.expect("keys").keys;
    let to = match a.get("to") {
        Some(t) => keys::party(t, net)?,
        None => sl.s.owner,
    };
    finish(a, "withdraw the slot", &slot_withdraw(&sl, &to, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

/// `stake`: a free judge stake of `--owner` (default the funder) worth `--value` KAS.
pub fn stake_cmd(a: &Args) -> R<()> {
    only(a, &["market", "value", "owner"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let m = market_from(a.need("market")?)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let owner = match a.get("owner") {
        Some(o) => keys::party(o, net)?,
        None => fk,
    };
    println!("This stake counts in a dispute only on claims accepted {} DAA or more after it is.", m.judge.age);
    finish(a, "stake: a judge stake", &create_stake(&m, owner, kas(a.need("value")?)? as u64, &fu, &fk, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

// ---------------------------------------------------------------- orders and the open

/// What an order risks beyond its margin (review 9, R9-F1): the consequence of `--any-slot`, and what a fill of an order
/// that rested longer than one term can cost (the cover's expiry may then lie in the past).
pub fn order_risk_lines(o: &OOpen) -> Vec<String> {
    let sc = o.m.p.scale;
    let half = o.premium_half(o.prem_max);
    let prem = kpf_protocol::payoff::q(o.size_left, half, sc, 0);
    let worst = kpf_protocol::payoff::q(o.size_left, o.tip + half, sc, 0) + o.m.p.carrier;
    let mut v = vec![];
    if o.uw_accept == [0; 32] {
        v.push(format!(
            "--any-slot: any underwriting slot of this market can fill this order and choose the cover rate (at least the range), the cover's expiry and the premium up to --prem-max {} KAS per unit and term; your half of the first premium (up to {} KAS on a full fill) goes to that underwriter.",
            fmt_kas(o.prem_max as i128),
            fmt_kas(prem as i128)
        ));
    }
    v.push(format!(
        "An order resting longer than one term ({} DAA) can be filled with a cover expiry already in the past (a contract proves only that the expiry is a term after the order was accepted). Such a pair is never renewed and settles at its entry price once its lapse time has passed: you get your margin back and lose your tip, the carrier and your half of the first premium (at most {} KAS on a full fill). Cancel or replace an order that is not filled within a term.",
        o.m.bond.term,
        fmt_kas(worst as i128)
    ));
    v
}

/// `oracle-order`: an order of one side, escrowing its margin, its tip, its half of the first premium and a carrier.
pub fn order_cmd(a: &Args) -> R<()> {
    only(a, &["market", "side", "size", "limit", "rate", "expiry", "owner", "min-counter-rate", "min-fill", "tip", "active-from", "slot", "underwriter", "any-slot", "prem-max", "escrow"])?;
    // Review 9 (R9-F1): accepting any slot is a choice the user makes explicitly, with its consequence printed.
    let slot_arg = a.get("slot").or_else(|| a.get("underwriter"));
    let uw_accept = match (slot_arg, a.has("any-slot")) {
        (Some(_), true) => return Err("give either --slot SLOT_ID or --any-slot, not both".into()),
        (Some(h), false) => hex32(h)?,
        (None, true) => [0; 32],
        (None, false) => {
            return Err("oracle-order needs --slot SLOT_ID (the one underwriting slot this order accepts) or --any-slot (any slot of the market may fill it and set its cover's terms within your --prem-max)".into())
        }
    };
    let net = network(a)?;
    let mut node = connect(a)?;
    let m = market_from(a.need("market")?)?;
    m.check().map_err(|x| format!("REFUSED: the market fails the contracts' bounds: {x}"))?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let sd = side(a.need("side")?)?;
    let sc = m.p.scale;
    let size = parse_amount(a.need("size")?, sc)?;
    let o = OOpen {
        m,
        side: sd,
        owner: match a.get("owner") {
            Some(o) => keys::party(o, net)?,
            None => fk,
        },
        size_left: size,
        limit: price(a, "limit")?,
        my_rate: price(a, "rate")?,
        min_counter_rate: price_or(a, "min-counter-rate", 0)?,
        min_fill: if a.has("min-fill") { parse_amount(a.need("min-fill")?, sc)? } else { m.p.min_size },
        tip: price_or(a, "tip", 0)?,
        active_from: int_or(a, "active-from", 0)?,
        expiry_daa: int(a, "expiry")?,
        uw_accept,
        prem_max: price_or(a, "prem-max", 0)?,
    };
    let need = o.fill_spend(size, o.prem_max) as u64 + 100_000_000;
    let esc = if a.has("escrow") { kas(a.need("escrow")?)? as u64 } else { need };
    let b = create_order(&o, esc, &fu, &fk, rate(a)?).map_err(e)?;
    println!("escrow {} KAS (a full fill takes at most {} KAS of it)", fmt_kas(esc as i128), fmt_kas(o.fill_spend(size, o.prem_max) as i128));
    for l in order_risk_lines(&o) {
        println!("{l}");
    }
    finish(a, "oracle-order", &b, &keys, net, &mut node)
}

/// `--uw-utxo FILE`: one plain UTXO of the slot owner (a funding file; the first entry of that key) that joins the
/// premium in one output, for a premium too small for an output of its own when the funder is not the underwriter.
fn uw_utxo(a: &Args, owner: &[u8; 32]) -> R<Option<KeyUtxo>> {
    let Some(p) = a.get("uw-utxo") else { return Ok(None) };
    let v = parse_utxos(&read_json(p)?)?;
    let u = funding_utxos(&v, owner).into_iter().next().ok_or("--uw-utxo lists no plain UTXO of the slot owner's key")?;
    Ok(Some(u))
}

/// `oracle-open`: two orders and the underwriter's slot open a pair (the underwriter's key signs the bind).
pub fn open_cmd(a: &Args) -> R<()> {
    only(a, &["long", "short", "slot", "n", "x", "cover-rate", "premium", "expiry", "uw-utxo"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (lo, so, sl) = (order_of(a, "long", &mut node, net)?, order_of(a, "short", &mut node, net)?, slot_of(a, "slot", &mut node, net)?);
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let t = now(a, &mut node)?;
    let sc = lo.s.m.p.scale;
    let step = lo.s.m.bond.step;
    let earliest = lo.daa.max(so.daa) as i64 + lo.s.m.bond.term;
    let xe = if a.has("expiry") { int(a, "expiry")? } else { (earliest + step - 1) / step * step };
    let terms = OpenTerms { n: parse_amount(a.need("n")?, sc)?, x: price(a, "x")?, cover_rate: price(a, "cover-rate")?, premium: price_or(a, "premium", 0)?, expiry: xe, lock: t };
    let extra = uw_utxo(a, &sl.s.owner)?;
    let fu: Vec<KeyUtxo> = fu.into_iter().filter(|u| extra.as_ref().is_none_or(|x| x.outpoint != u.outpoint)).collect();
    let (b, p) = open_with(&lo, &so, &sl, &terms, &fu, &fk, rate(a)?, extra.as_ref()).map_err(e)?;
    println!("pair [{} .. {}] KAS of {} units, cover {} KAS until DAA {xe}", fmt_kas(p.bl as i128), fmt_kas(p.bs as i128), fmt_units(p.size as i128, sc as i128), fmt_kas(p.cover_of(p.size) as i128));
    finish(a, "oracle-open", &b, &keys, net, &mut node)
}

// ---------------------------------------------------------------- claims

/// `vouch`: the underwriter vouches a price and proposes a claim (a liquidation; or a close, which the closing party
/// signs too). `--vouch` is the underwriter's own UTXO (a funding file of one P2PK UTXO of its key). A liquidation's
/// proposer is the underwriter: the vouch UTXO funds what the claim adds to the contract outputs (the bond, and the
/// carrier of the new part when `--n` is below the pair's size) and what is left of it goes back to the underwriter. A
/// close's bond is the closer's: the vouch value goes back in full and the funder (expected to be the closer) pays the
/// bond (and that carrier). The funder always pays the network fee.
pub fn vouch_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "kind", "side", "price", "t0", "n", "vouch"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let kind = match a.need("kind")? {
        "liquidate" | "liquidation" => KIND_LIQ,
        "close" => KIND_CLOSE,
        k => return Err(format!("--kind liquidate|close, not `{k}`")),
    };
    let sd = side(a.need("side")?)?;
    let v = parse_utxos(&read_json(a.need("vouch")?)?)?;
    let vu = funding_utxos(&v, &lp.s.b.uw_key);
    let vouch = vu.first().ok_or("--vouch lists no plain UTXO of the pair's underwriter key")?;
    // The vouch input is the underwriter's (see `propose`: it funds a liquidation's bond, the rest goes back to it): it never funds the fee, so a funder that
    // is also the underwriter must not list it twice.
    let fu: Vec<KeyUtxo> = fu.into_iter().filter(|u| u.outpoint != vouch.outpoint).collect();
    if fu.is_empty() {
        return Err("no funding left once the vouch UTXO is set aside: the funder pays the network fee (and, for a close, the bond), so give another plain UTXO of the funder".into());
    }
    let t = now(a, &mut node)?;
    let t0 = int_or(a, "t0", t as i64)?;
    let n = if a.has("n") { parse_amount(a.need("n")?, lp.s.m.p.scale)? } else { lp.s.size };
    let c = ClaimTerms { kind, side: sd, n, price: price(a, "price")?, t0 };
    let (b, w) = propose(&lp, &c, vouch, &fu, &fk, rate(a)?).map_err(e)?;
    println!(
        "claim: {} of the {} at {} KAS (t0 {t0}); bond {} KAS. It may be contested until P + {} (P: the DAA it lands at) and is void if it lands after DAA {} or more than {} DAA after t0.",
        if kind == KIND_LIQ { "liquidation" } else { "close" },
        if sd == LONG { "long" } else { "short" },
        fmt_kas(c.price as i128),
        fmt_kas(w.bond as i128),
        w.m.p.challenge_daa - 1,
        w.b.expiry - w.m.p.challenge_daa,
        w.m.p.late_daa
    );
    let adds = funded_by_proposer(&lp, &w);
    let what = if n < lp.s.size { format!("the claim's bond {} KAS and the carrier of the new part {} KAS", fmt_kas(w.bond as i128), fmt_kas((adds - w.bond) as i128)) } else { format!("the claim's bond {} KAS", fmt_kas(w.bond as i128)) };
    if kind == KIND_LIQ {
        let back = if n < lp.s.size {
            format!("a cure refunds the {} KAS to the underwriter; a valid settlement pays it the bond and the keeper fee and leaves the carrier in the forfeited part (the winner's); a void or refuted claim leaves the carrier in the restored part and pays the bond to the defender (and the disputer)", fmt_kas(adds as i128))
        } else {
            "a cure refunds the bond to the underwriter, a valid settlement pays it the bond and the keeper fee, a void or refuted claim pays the bond to the defender (and the disputer)".to_string()
        };
        println!("The vouch input ({} KAS) funds {what} and the rest goes back to the underwriter ({back}); the funder pays only the network fee.", fmt_kas(vouch.amount as i128));
    } else {
        println!("The vouch input's {} KAS go back to the underwriter in full. The closer posts {what}: the funder is expected to be the closer, and pays it and the fee.", fmt_kas(vouch.amount as i128));
    }
    finish(a, "vouch: a price claim", &b, &keys, net, &mut node)
}

/// A transaction body file: a `kpf-tx/1` file this tool wrote (`transaction`), a node's `rpc` object, or the bare
/// `RpcTransaction` JSON. Its id binds what the trace reads of it but the signature scripts.
fn tx_body(path: &str) -> R<kaspa_consensus_core::tx::Transaction> {
    let v = read_json(path)?;
    let t = v.get("transaction").or_else(|| v.get("rpc")).unwrap_or(&v);
    kpf_protocol::tx::tx_from_rpc_json(t).map_err(|x| format!("{path}: {x}"))
}

/// Review 7 (R7-F2): the wallet's trace before a stake is committed to a claim (`contest`) or a dispute (`deposit`).
/// With `--node` the trace is required (`--claim-tx`, and `--opening` for a dispute) and a failing trace refuses; without
/// it the command refuses unless `--accept-untraced` acknowledges what was not checked, which it prints.
/// DESIGN: the trace reads the transaction bodies from files (the node API this tool uses looks UTXOs up by address,
/// not transactions by id); a body is bound to the UTXO by its id (outpoints, outputs, covenant bindings), so any
/// source will do, and the node confirms that the UTXO itself is live.
fn untraced_or_refuse(a: &Args, node: bool, what: &str, traced: bool) -> R<()> {
    if traced && node {
        return Ok(());
    }
    if node {
        return Err(format!("{what}: with --node the claim must be traced: give --claim-tx (the claim's birth transaction){}", if what == "deposit" { " and --opening (the dispute's opening transaction)" } else { "" }));
    }
    if !a.has("accept-untraced") {
        return Err(format!(
            "{what}: refused without --node: the {} cannot be checked. A dispute or claim written by a genesis keeps every stake committed to it. Use --node with the trace files, or --accept-untraced to commit anyway",
            if what == "deposit" { "dispute's UTXO" } else { "claim's UTXO" }
        ));
    }
    println!("WARNING: {what} without a node: NOT CHECKED that the {} is live at a node with this script, covenant id and value", if what == "deposit" { "dispute" } else { "claim" });
    if !traced {
        println!("WARNING: {what} untraced: NOT CHECKED that the claim's id was born alone in a KpfPairO.propose{}; a forged one keeps every stake committed to it", if what == "deposit" { " and that the dispute was opened on it by KpfStake.open" } else { "" });
    }
    Ok(())
}

/// `contest`: open a dispute on a pending claim with an aged judge stake on NO.
pub fn contest_cmd(a: &Args) -> R<()> {
    only(a, &["pending", "stake", "base", "claim-tx", "accept-untraced"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let w = pending_of(a, "pending", &mut node, net)?;
    let st = stake_at(a.need("stake")?, &mut node, net)?;
    let trace = match a.get("claim-tx") {
        Some(p) => {
            let id = w.id.ok_or("the pending part has no covenant id")?;
            let raw = ObjFile::load(a.need("pending")?)?.redeem;
            let t = trace_claim(id, &tx_body(p)?, &raw).map_err(|x| format!("the claim's trace failed: {x}"))?;
            println!("traced: the claim was born alone in {} as the pending part of the pair {}:{} (KpfPairO.propose); {}", t.birth, t.pair_in.transaction_id, t.pair_in.index, TRACE_LIMITS);
            Some(t)
        }
        None => None,
    };
    untraced_or_refuse(a, node.is_some(), "contest", trace.is_some())?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let t = now(a, &mut node)?;
    let until = w.s.contest_until(w.daa);
    if t > until {
        println!("WARNING: the contest window ended at DAA {until}: a contest landing now is ignored and the claim settles as if uncontested");
    }
    let base = if a.has("base") { kas(a.need("base")?)? as u64 } else { 50_000_000 };
    let trust = match &trace {
        Some(t) => Trust::Traced(t),
        None => Trust::Untraced,
    };
    let (b, _, _) = contest(&w, &st, base, &fu, &fk, rate(a)?, &trust).map_err(e)?;
    finish(a, "contest", &b, &keys, net, &mut node)
}

/// `deposit`: stakes join the trailing side of a dispute (`--side yes|no`, up to 7 `--stake` files).
pub fn deposit_cmd(a: &Args) -> R<()> {
    only(a, &["dispute", "side", "stake", "opening", "claim-tx", "accept-untraced"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    // Review 7 (R7-F2): the dispute is read through the node when one is given (its UTXO, value and id confirmed).
    let d = match load(a, "dispute", &mut node, net)? {
        (f, OObj::Stake(s)) => live_of(&f, s)?,
        (_, o) => return Err(format!("--dispute is a {}, not a judge stake", o.kind())),
    };
    let sd = match a.need("side")? {
        "yes" => YES,
        "no" => NO,
        s => return Err(format!("--side yes|no, not `{s}`")),
    };
    let trace = match (a.get("opening"), a.get("claim-tx")) {
        (Some(o), Some(c)) => {
            let t = trace_dispute(&d, &tx_body(o)?, &tx_body(c)?).map_err(|x| format!("the dispute's trace failed: {x}"))?;
            println!("traced: the dispute was opened on the claim born alone in {} as the pending part of the pair {}:{} (KpfPairO.propose); {}", t.birth, t.pair_in.transaction_id, t.pair_in.index, TRACE_LIMITS);
            Some(t)
        }
        (None, None) => None,
        _ => return Err("the trace needs both --opening and --claim-tx".into()),
    };
    untraced_or_refuse(a, node.is_some(), "deposit", trace.is_some())?;
    let stakes = a.all("stake").into_iter().map(|p| stake_at(p, &mut node, net)).collect::<R<Vec<_>>>()?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let trust = match &trace {
        Some(t) => Trust::Traced(t),
        None => Trust::Untraced,
    };
    let (b, n) = deposit(&d, sd, &stakes, &fu, &fk, rate(a)?, &trust).map_err(e)?;
    println!("after the deposit: {} leads, YES {} KAS, NO {} KAS", if n.side == YES { "YES" } else { "NO" }, fmt_kas(n.yes as i128), fmt_kas(n.no as i128));
    finish(a, "deposit", &b, &keys, net, &mut node)
}

/// `resolve`: once the round's window has closed, the dispute resolves and its verdict reaches the contested part.
pub fn resolve_cmd(a: &Args) -> R<()> {
    only(a, &["dispute", "pending"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let d = stake_at(a.need("dispute")?, &mut node, net)?;
    let w = pending_of(a, "pending", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, w2, _) = decide(&d, &w, &fu, &fk, rate(a)?).map_err(e)?;
    println!("verdict delivered: the claim is {}", if w2.status == UPHELD { "upheld (or the contest was late)" } else { "refuted" });
    finish(a, "resolve", &b, &keys, net, &mut node)
}

/// `oracle-cure`: the loser of a pending liquidation adds the margin that clears it and repays the underwriter.
pub fn cure_cmd(a: &Args) -> R<()> {
    only(a, &["pending"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let w = pending_of(a, "pending", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    finish(a, "oracle-cure", &cure(&w, &fu, &fk, rate(a)?).map_err(e)?.0, &keys, net, &mut node)
}

/// `oracle-finalize`: a pending part W old, or a decided one at once: settle, or restore a refuted or void claim.
pub fn finalize_cmd(a: &Args) -> R<()> {
    only(a, &["pending"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let w = pending_of(a, "pending", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, s) = finalize(&w, &fu, &fk, rate(a)?).map_err(e)?;
    println!("settlement: {s:?}");
    finish(a, "oracle-finalize", &b, &keys, net, &mut node)
}

/// `renew`: the underwriter renews up to 7 pairs of its slot in one bind (its key signs).
pub fn renew_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "slot", "uw-utxo"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let mut pairs = vec![];
    for p in a.all("pair") {
        let f = crate::cmds::with_node_facts(ObjFile::load(p)?, &mut node, net)?;
        match OObj::decode(&f.redeem).map_err(e)? {
            OObj::Pair(x) => pairs.push(live_of(&f, x)?),
            o => return Err(format!("{p} is a {}", o.kind())),
        }
    }
    let sl = slot_of(a, "slot", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let t = now(a, &mut node)?;
    let extra = uw_utxo(a, &sl.s.owner)?;
    let fu: Vec<KeyUtxo> = fu.into_iter().filter(|u| extra.as_ref().is_none_or(|x| x.outpoint != u.outpoint)).collect();
    finish(a, "renew", &renew_with(&pairs, &sl, t, &fu, &fk, rate(a)?, extra.as_ref()).map_err(e)?.0, &keys, net, &mut node)
}

/// `lapse-settle`: a pair whose cover lapsed and that is idle settles at its entry price (anyone).
pub fn lapse_cmd(a: &Args) -> R<()> {
    only(a, &["pair"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    finish(a, "lapse-settle", &lapse_settle(&lp, &fu, &fk, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

/// `settle-pool`: a resolved dispute's pool pays its winners and forfeits its losers (up to 7 receipts), then (with
/// `--close`) closes.
pub fn settle_pool_cmd(a: &Args) -> R<()> {
    only(a, &["pool", "receipt"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let pool = stake_at(a.need("pool")?, &mut node, net)?;
    let rs = a.all("receipt").into_iter().map(|p| stake_at(p, &mut node, net)).collect::<R<Vec<_>>>()?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let b = if rs.is_empty() { pool_close(&pool, &fk, rate(a)?).map_err(e)? } else { pool_settle(&pool, &rs, &fu, &fk, rate(a)?).map_err(e)?.0 };
    finish(a, if rs.is_empty() { "close the pool" } else { "settle receipts" }, &b, &keys, net, &mut node)
}

// ---------------------------------------------------------------- the judge stake's exit

/// `stake-unbond`: the owner's free stake (a stake returned by `settle-pool` is one) starts leaving; it counts nowhere
/// from then on. `--key` is the owner's.
pub fn stake_unbond_cmd(a: &Args) -> R<()> {
    only(a, &["stake"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let st = stake_at(a.need("stake")?, &mut node, net)?;
    if st.s.status != STATUS_FREE {
        return Err(format!("--stake is {}: only a free stake unbonds", stake_state_words(&st.s)));
    }
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, _) = stake_unbond(&st, &fu, &fk, rate(a)?).map_err(e)?;
    println!("The stake of {} KAS stops counting now and can be withdrawn {} DAA after the unbonding transaction is accepted (kpf stake-withdraw).", fmt_kas(st.value as i128), st.s.m.p.unbond);
    finish(a, "stake-unbond: the stake starts leaving", &b, &keys, net, &mut node)
}

/// `stake-withdraw`: an unbonding stake that is `unbond` DAA old pays its whole value, less the fee, to `--to` (default
/// the owner). `--key` is the owner's; no funder is needed.
pub fn stake_withdraw_cmd(a: &Args) -> R<()> {
    only(a, &["stake", "to"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let st = stake_at(a.need("stake")?, &mut node, net)?;
    if st.s.status != STATUS_UNBONDING {
        return Err(format!("--stake is {}: only an unbonding stake withdraws (run kpf stake-unbond first)", stake_state_words(&st.s)));
    }
    let keys = crate::cmds::funder_and_keys(a, false)?.expect("keys").keys;
    let to = match a.get("to") {
        Some(t) => keys::party(t, net)?,
        None => st.s.owner,
    };
    println!("withdrawable from DAA {} (the stake's UTXO DAA {} + the unbonding delay {})", st.daa + st.s.m.p.unbond as u64, st.daa, st.s.m.p.unbond);
    finish(a, "stake-withdraw: the stake leaves", &stake_withdraw(&st, &to, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

fn stake_state_words(s: &Js) -> &'static str {
    use kpf_protocol::judge::*;
    match s.status {
        STATUS_FREE => "a free stake",
        STATUS_UNBONDING => "an unbonding stake",
        STATUS_RECEIPT => "a receipt (committed stake leaves only through its pool's settlement)",
        STATUS_DISPUTE => "a dispute",
        STATUS_RESOLVED => "a resolved pool",
        _ => "a stake in an unknown state",
    }
}

// ---------------------------------------------------------------- the pair's price-free exits

/// `oracle-amend`: a party lists its side for a hand-over (`--price P`, the ask of a long or the bid of a short) or
/// unlists it. `--key` is that side's owner; the pair's UTXO must be quietDaa old.
pub fn oracle_amend_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "side", "price", "min-part", "tip", "unlist"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let sd = side(a.need("side")?)?;
    let (ask, min_part, tip) = if a.has("unlist") {
        (0, 0, 0)
    } else {
        (price(a, "price")?, if a.has("min-part") { parse_amount(a.need("min-part")?, lp.s.m.p.scale)? } else { 0 }, price_or(a, "tip", 0)?)
    };
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, _) = amend(&lp, sd, ask, min_part, tip, &fu, &fk, rate(a)?).map_err(e)?;
    println!("The pair's UTXO must be {} DAA old (quietDaa); it is DAA {}, so from DAA {}.", lp.s.m.p.quiet_daa, lp.daa, lp.daa as i64 + lp.s.m.p.quiet_daa);
    finish(a, if a.has("unlist") { "oracle-amend: unlist" } else { "oracle-amend: list" }, &b, &keys, net, &mut node)
}

/// `oracle-transfer`: the whole of a listed side changes hands at `--x` to `--entrant` (default the funder), who posts
/// the bound `--nb` and funds the new pair. The leaver is paid out of the pair at `x`. `--key` is not needed: no party
/// signs a transfer (the listing is the leaver's consent).
pub fn oracle_transfer_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "side", "x", "nb", "entrant"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let sd = side(a.need("side")?)?;
    let x = price(a, "x")?;
    let nb = price(a, "nb")?;
    let p = &lp.s;
    if sd == LONG && !(p.long_ask > 0 && x >= p.long_ask) {
        return Err(format!("the long is not listed at or below {} KAS (ask {}): the contract takes a long only at or above its ask", fmt_kas(x as i128), fmt_kas(p.long_ask as i128)));
    }
    if sd != LONG && !(p.short_bid > 0 && x <= p.short_bid) {
        return Err(format!("the short is not listed at or above {} KAS (bid {}): the contract takes a short only at or below its bid", fmt_kas(x as i128), fmt_kas(p.short_bid as i128)));
    }
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let entrant = match a.get("entrant") {
        Some(k) => keys::party(k, net)?,
        None => fk,
    };
    let (b, part) = transfer(&lp, sd, x, nb, entrant, &fu, &fk, rate(a)?).map_err(e)?;
    println!("new pair [{} .. {}] KAS of {} units; the entrant is {}", fmt_kas(part.bl as i128), fmt_kas(part.bs as i128), fmt_units(part.size as i128, part.m.p.scale as i128), to_hex(&entrant));
    finish(a, "oracle-transfer: a side changes hands", &b, &keys, net, &mut node)
}

/// `oracle-mutual-close`: both sides are listed and the listings cross (long ask <= `--x` <= short bid): anyone closes
/// the whole pair at `--x`.
pub fn oracle_mutual_close_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "x"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let x = price(a, "x")?;
    let p = &lp.s;
    if !(p.long_ask > 0 && p.short_bid > 0 && x >= p.long_ask && x <= p.short_bid) {
        return Err(format!("the listings do not cross at {} KAS (long ask {}, short bid {}; 0 is unlisted)", fmt_kas(x as i128), fmt_kas(p.long_ask as i128), fmt_kas(p.short_bid as i128)));
    }
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    finish(a, "oracle-mutual-close", &mutual_close(&lp, x, &fu, &fk, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

/// `oracle-coop-close`: both owners sign; the long gets `--long-pay` KAS, the short the rest less the fee.
/// `--key` twice: the long's and the short's.
pub fn oracle_coop_close_cmd(a: &Args) -> R<()> {
    only(a, &["pair", "long-pay"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lp = pair_of(a, "pair", &mut node, net)?;
    let long_pay = kas(a.need("long-pay")?).map_err(|m| format!("--long-pay: {m}"))?;
    if long_pay < 0 {
        return Err("--long-pay must not be negative".into());
    }
    let keys = crate::cmds::funder_and_keys(a, false)?.expect("keys").keys;
    println!("both owners' keys are needed: long {} and short {}", to_hex(&lp.s.long_key), to_hex(&lp.s.short_key));
    finish(a, "oracle-coop-close", &coop_close(&lp, long_pay as u64, rate(a)?).map_err(e)?, &keys, net, &mut node)
}

// ---------------------------------------------------------------- inspect

/// `inspect` of an oracle-market object: what it is, its state in words, and (for a file) its UTXO.
pub fn inspect_oracle(a: &Args, o: &OObj, rs: &[u8], file: Option<&ObjFile>, net: Network) -> R<()> {
    let k = kpf_protocol::oracle_artifacts::kit();
    let tpl = match o {
        OObj::Order(_) => k.open.hash,
        OObj::Pair(_) => k.pair.hash,
        OObj::Pending(_) => k.pending.hash,
        OObj::Slot(_) => k.bond.hash,
        OObj::Stake(_) => k.stake.hash,
    };
    let market = match o {
        OObj::Order(x) => x.m.id(),
        OObj::Pair(x) => x.m.id(),
        OObj::Pending(x) => x.m.id(),
        OObj::Slot(x) => x.m.id,
        OObj::Stake(x) => x.m.id,
    };
    let mut doc = json!({
        "kind": o.kind(), "family": "oracle", "network": net.name(), "address": p2sh_address(rs, net), "redeemBytes": rs.len(),
        "templateHash": to_hex(&tpl), "marketId": to_hex(&market), "state": format!("{o:?}"),
    });
    if let Some(f) = file {
        doc["utxo"] = json!({"outpoint": outpoint_json(&f.op), "amount": f.amount, "daa": f.daa, "covenantId": f.covenant_id.to_string()});
    }
    if a.has("json") {
        println!("{}", serde_json::to_string_pretty(&doc).expect("json"));
        return Ok(());
    }
    let side_word = |s: i64| if s == LONG { "long" } else { "short" };
    println!("{}   {}   template {}   market {}", o.kind(), p2sh_address(rs, net), &to_hex(&tpl)[..16], &to_hex(&market)[..16]);
    match o {
        OObj::Pair(p) => {
            let sc = p.m.p.scale as i128;
            println!("size {} units ({} base units)   range bl {} .. bs {} KAS   entry {}   floor value {} KAS", fmt_units(p.size as i128, sc), p.size, fmt_kas(p.bl as i128), fmt_kas(p.bs as i128), fmt_kas(p.entry as i128), fmt_kas(p.floor() as i128));
            println!("long   {}   listing ask {} (min part {}, tip {})", to_hex(&p.long_key), fmt_kas(p.long_ask as i128), p.long_min, fmt_kas(p.long_tip as i128));
            println!("short  {}   listing bid {} (min part {}, tip {})", to_hex(&p.short_key), fmt_kas(p.short_bid as i128), p.short_min, fmt_kas(p.short_tip as i128));
            println!("underwriter {}   slot {}   cover rate {} KAS   premium {} KAS a term", to_hex(&p.b.uw_key), to_hex(&p.b.uw_id), fmt_kas(p.b.cover_rate as i128), fmt_kas(p.b.premium as i128));
            println!("cover expires at DAA {}; lapse settle from DAA {}; claims must land {} DAA before the expiry", p.b.expiry, p.lapse_from(), p.m.p.challenge_daa);
        }
        OObj::Pending(w) => {
            let status = match w.status {
                PENDING => "pending: contestable until the window ends",
                CONTESTED => "contested: its dispute decides",
                UPHELD => "upheld: finalize at once",
                REFUTED => "refuted: finalize at once (restores the part)",
                _ => "in an unknown state",
            };
            println!(
                "{} claim ({status}): the {} of {} units of the range {} .. {} KAS at the vouched price {} KAS (t0 {})",
                if w.kind == KIND_LIQ { "liquidation" } else { "close" },
                side_word(w.side),
                fmt_units(w.n as i128, w.m.p.scale as i128),
                fmt_kas(w.bl as i128),
                fmt_kas(w.bs as i128),
                fmt_kas(w.quote as i128),
                w.t0
            );
            println!("bond {} KAS, refund {} KAS, keeper fee {} KAS; underwriter {}; long {} short {}", fmt_kas(w.bond as i128), fmt_kas(w.refund as i128), fmt_kas(w.fee as i128), to_hex(&w.b.uw_key), to_hex(&w.long_key), to_hex(&w.short_key));
            if w.status != PENDING {
                println!("contested at P = {}; disputer {}", w.clock, to_hex(&w.disputer));
            }
            println!("contest window {} DAA from the UTXO's DAA", w.m.p.challenge_daa);
        }
        OObj::Order(x) => {
            let sc = x.m.p.scale as i128;
            println!(
                "{} order of {} units left, limit {} KAS, own margin rate {} KAS, accepts counter rate >= {} KAS, min fill {} units, tip {} KAS, premium at most {} KAS",
                side_word(x.side),
                fmt_units(x.size_left as i128, sc),
                fmt_kas(x.limit as i128),
                fmt_kas(x.my_rate as i128),
                fmt_kas(x.min_counter_rate as i128),
                fmt_units(x.min_fill as i128, sc),
                fmt_kas(x.tip as i128),
                fmt_kas(x.prem_max as i128)
            );
            println!("owner {}   active from DAA {}   expires DAA {}   slot accepted {}", to_hex(&x.owner), x.active_from, x.expiry_daa, if x.uw_accept == [0; 32] { "any".to_string() } else { to_hex(&x.uw_accept) });
        }
        OObj::Slot(b) => {
            let sum: i64 = b.ring.iter().sum();
            println!("{} slot of {}   cover counted {} KAS", if b.status == kpf_protocol::bond::STATUS_UNBONDING { "unbonding" } else { "active" }, to_hex(&b.owner), fmt_kas(sum as i128));
            if let Some(f) = file {
                println!("capacity {} KAS (rho {})   unbonding delay {} DAA", fmt_kas(capacity(f.amount as i64, b.m.p.rho_bps) as i128), b.m.p.rho_bps as f64 / 1e4, b.m.p.unbond);
            }
        }
        OObj::Stake(s) => {
            println!("{}   owner {}", stake_state_words(s), to_hex(&s.owner));
            match s.status {
                kpf_protocol::judge::STATUS_RECEIPT => println!("receipt: {} KAS on {} (round {}, anchor {})", fmt_kas(s.amt as i128), if s.side == YES { "YES" } else { "NO" }, s.round, s.anchor),
                kpf_protocol::judge::STATUS_DISPUTE | kpf_protocol::judge::STATUS_RESOLVED => println!("{} leads (YES {} KAS, NO {} KAS), anchor {}, lead changes {}", if s.side == YES { "YES" } else { "NO" }, fmt_kas(s.yes as i128), fmt_kas(s.no as i128), s.anchor, s.round),
                _ => println!("unbonding delay {} DAA, minimum stake {} KAS, stake counts if {} DAA older than the claim", s.m.p.unbond, fmt_kas(s.m.p.min_stake as i128), s.m.p.age),
            }
        }
    }
    if let Some(f) = file {
        println!("utxo {}:{}  {} KAS  DAA {}  covenant id {}", f.op.transaction_id, f.op.index, fmt_kas(f.amount as i128), crate::cmds::daa_str(f.daa), crate::cmds::short_hash(&f.covenant_id));
    }
    Ok(())
}

// ---------------------------------------------------------------- watch

/// `oracle-watch`: for the key `--me`, every claim against it and the time left to contest, its pairs' underwriter
/// cover expiry and what follows it, and the disputes it has stake in, from object files (`--object`, `--scan DIR`).
/// With `--node` every object's DAA score and value are read from the node; without it a DAA read from a file is shown
/// as unchecked (review 6).
pub fn watch_cmd(a: &Args) -> R<()> {
    a.only(&["me", "object", "scan", "daa", "node", "json"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let me = keys::party(a.need("me")?, net)?;
    let t = now(a, &mut node)? as i64;
    let mut files: Vec<String> = a.all("object").into_iter().map(String::from).collect();
    if let Some(d) = a.get("scan") {
        for x in std::fs::read_dir(d).map_err(|x| format!("{d}: {x}"))? {
            let p = x.map_err(e)?.path();
            if p.extension().is_some_and(|x| x == "json") {
                files.push(p.display().to_string());
            }
        }
    }
    let mut rows = vec![];
    for f in files {
        let Ok(mut of) = ObjFile::load(&f) else { continue };
        let Ok(obj) = OObj::decode(&of.redeem) else { continue };
        // Review 6 (note 3): the DAA score decides whether a claim is valid and when its window ends, so it is read from
        // the node whenever one is given; a file's own `daa` is marked as unchecked.
        let unchecked = match node.as_mut() {
            Some(n) => {
                if let Err(x) = crate::cmds::refresh(n, &mut of, net) {
                    rows.push((f.clone(), format!("NOT AT THE NODE: {x}; nothing in this file is current")));
                    continue;
                }
                ""
            }
            None if of.daa.is_some() => " [DAA from the object file, not checked against a node: pass --node]",
            None => "",
        };
        let daa = of.daa.map(|d| d as i64);
        let before = rows.len();
        match obj {
            OObj::Pending(w) => {
                let mine = w.long_key == me || w.short_key == me;
                if !mine && w.b.uw_key != me && w.disputer != me {
                    continue;
                }
                let against = w.defender() == me;
                let what = if w.kind == KIND_LIQ { "liquidation" } else { "close" };
                let msg = match (w.status, daa) {
                    (PENDING, Some(p)) => {
                        let until = p + w.m.p.challenge_daa - 1;
                        let valid = w.valid_at(p);
                        format!(
                            "{} {what} of the {} at {} KAS{}; {}; finalizable from DAA {}",
                            if against { "A CLAIM AGAINST YOU:" } else { "a claim on your pair:" },
                            if w.side == LONG { "long" } else { "short" },
                            fmt_kas(w.quote as i128),
                            if valid { "" } else { " (void: it will restore and pay you the bond)" },
                            if t <= until { format!("contest until DAA {until} ({} DAA left): kpf contest --pending {f} --stake <your aged stake>", until - t) } else { "the contest window has ended".into() },
                            p + w.m.p.challenge_daa
                        )
                    }
                    (PENDING, None) => format!("a pending {what}: its DAA is unknown (run kpf status --update)"),
                    (CONTESTED, Some(c)) if c >= w.clock + w.m.p.challenge_daa => format!(
                        "a contested {what}, but the contest landed at DAA {c}, after the window (it ended at DAA {}): the verdict will be ignored and the claim settles as if uncontested once its dispute resolves (kpf resolve)",
                        w.clock + w.m.p.challenge_daa - 1
                    ),
                    (CONTESTED, _) => format!("a contested {what}: resolves when its dispute's round closes (kpf resolve)"),
                    (UPHELD, _) => format!("an upheld {what}: finalize at once (kpf oracle-finalize --pending {f})"),
                    _ => format!("a refuted {what}: finalize at once to restore the part and pay the bond (kpf oracle-finalize --pending {f})"),
                };
                rows.push((f.clone(), msg));
            }
            OObj::Pair(p) => {
                if p.long_key != me && p.short_key != me && p.b.uw_key != me {
                    continue;
                }
                let x = p.b.expiry;
                let step = p.m.bond.step;
                let lapse = x + p.m.bond.tail;
                let settle_from = p.lapse_from();
                let msg = if t < x - step {
                    format!("your underwriter's cover runs until DAA {x} ({} DAA left); renewal from DAA {}", x - t, x - step)
                } else if t < x {
                    format!("COVER EXPIRES at DAA {x} ({} DAA left): the underwriter may renew now; claims landing after DAA {} are void", x - t, x - p.m.p.challenge_daa)
                } else if t < lapse {
                    format!("THE COVER HAS EXPIRED (DAA {x}): no claim is valid; the underwriter may still renew; the cover lapses at DAA {lapse}; lapse settle at the entry price from DAA {settle_from}")
                } else if (t as u64) < settle_from {
                    format!("THE COVER HAS LAPSED (DAA {lapse}): anyone may settle the pair at its entry price from DAA {settle_from} (kpf lapse-settle --pair {f}), whatever its parties do meanwhile; a price-free exit (transfer, mutual or cooperative close) is open meanwhile")
                } else {
                    format!("THE COVER HAS LAPSED: anyone may settle the pair at its entry price now (kpf lapse-settle --pair {f}); a price-free exit (transfer, mutual or cooperative close) is open meanwhile")
                };
                rows.push((f.clone(), msg));
            }
            OObj::Stake(s) if s.owner == me || s.status == kpf_protocol::judge::STATUS_DISPUTE => {
                let msg = match s.status {
                    6 => format!("your stake of {} KAS is committed to a dispute on {} (anchor {}): it is paid or forfeited when the dispute's pool settles", fmt_kas(s.amt as i128), if s.side == YES { "YES" } else { "NO" }, s.anchor),
                    7 => {
                        let st = if s.start == -1 { daa.unwrap_or(0) } else { s.start };
                        format!("a dispute: {} leads (YES {} KAS, NO {} KAS); it resolves from DAA {}", if s.side == YES { "YES" } else { "NO" }, fmt_kas(s.yes as i128), fmt_kas(s.no as i128), st + s.m.p.round)
                    }
                    8 => "a resolved pool: settle the receipts (kpf settle-pool)".into(),
                    4 => format!("your free stake of {} KAS", fmt_kas(of.amount as i128)),
                    _ => format!("your unbonding stake of {} KAS", fmt_kas(of.amount as i128)),
                };
                rows.push((f.clone(), msg));
            }
            OObj::Slot(b) if b.owner == me => {
                let sum: i64 = b.ring.iter().sum();
                rows.push((f.clone(), format!("your slot: {} KAS of cover counted of a capacity of {} KAS", fmt_kas(sum as i128), fmt_kas(capacity(of.amount as i64, b.m.p.rho_bps) as i128))));
            }
            _ => {}
        }
        if rows.len() > before {
            if let Some(r) = rows.last_mut() {
                r.1 += unchecked;
            }
        }
    }
    if a.has("json") {
        println!("{}", serde_json::to_string_pretty(&json!(rows.iter().map(|(f, m)| json!({"file": f, "status": m})).collect::<Vec<_>>())).expect("json"));
    } else if rows.is_empty() {
        println!("nothing of {} among the objects given", to_hex(&me));
    } else {
        for (f, m) in rows {
            println!("{f}\n  {m}");
        }
    }
    let _ = (Path::new("."), SHORT);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Args {
        Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    /// Review 7 (R7-F2): with a node the trace is required (no flag excuses it); without one an explicit flag is.
    #[test]
    fn the_trace_gate_of_contest_and_deposit() {
        let none = args(&["deposit"]);
        let ack = args(&["deposit", "--accept-untraced"]);
        let e1 = untraced_or_refuse(&none, true, "deposit", false).unwrap_err();
        assert!(e1.contains("--claim-tx") && e1.contains("--opening"), "{e1}");
        assert!(untraced_or_refuse(&ack, true, "deposit", false).is_err(), "with a node the flag does not excuse a missing trace");
        assert!(untraced_or_refuse(&none, true, "deposit", true).is_ok());
        assert!(untraced_or_refuse(&none, false, "deposit", true).is_err(), "without a node the UTXO is unchecked: the flag is needed");
        assert!(untraced_or_refuse(&none, false, "contest", false).unwrap_err().contains("--accept-untraced"));
        assert!(untraced_or_refuse(&ack, false, "contest", false).is_ok());
    }

    /// Live run 1 (three node refusals "sequence locks conditions was not met"): a relative lock r on an input of DAA d
    /// is usable from d + r (consensus: spendable once the DAA exceeds d + r - 1), a DAA lock time t from t + 1; the
    /// latest one wins; with a node a submission before it is refused locally, a dry run says it is not yet usable,
    /// offline the line is information.
    #[test]
    fn relative_locks_are_compared_with_the_node_before_submitting() {
        use kaspa_consensus_core::tx::{ScriptPublicKey, Transaction, TransactionInput, TransactionOutpoint, UtxoEntry};
        let op = |i: u32| TransactionOutpoint { transaction_id: kaspa_consensus_core::tx::TransactionId::from_bytes([1; 32]), index: i };
        let inp = |i: u32, seq: u64| TransactionInput::new(op(i), vec![], seq, 1);
        let ent = |daa: u64| UtxoEntry::new(1, ScriptPublicKey::from_vec(0, vec![]), daa, false, None);
        let tx = |ins: Vec<TransactionInput>, lock: u64| Transaction::new(0, ins, vec![], lock, kaspa_consensus_core::subnets::SUBNETWORK_ID_NATIVE, 0, vec![]);
        // Nothing locked: a funder (sequence 0) and no lock time.
        assert_eq!(usable_from(&tx(vec![inp(0, 0)], 0), &[ent(100)]), None);
        // The slot of an open, rateDaa = 50, its UTXO at DAA 1,000: usable from 1,050.
        let t = tx(vec![inp(0, 0), inp(1, 50)], 0);
        let (from, why) = usable_from(&t, &[ent(7), ent(1_000)]).unwrap();
        assert_eq!(from, 1_050);
        assert!(why.contains("input 1 must be 50 DAA old"), "{why}");
        // A lock time later than the relative lock wins; one disabled sequence is ignored.
        let t = tx(vec![inp(0, 50), inp(1, (1 << 63) | 9_999)], 2_000);
        assert_eq!(usable_from(&t, &[ent(1_000), ent(0)]).unwrap().0, 2_001);
        // The verdicts.
        let e = lock_verdict(1_050, "x", Some(1_000), true).unwrap_err();
        assert!(e.contains("usable from DAA 1050 (now 1000, about 5.0 s"), "{e}");
        assert!(lock_verdict(1_050, "x", Some(1_000), false).unwrap().starts_with("NOT YET USABLE: usable from DAA 1050"));
        assert!(lock_verdict(1_050, "x", Some(1_050), true).unwrap().starts_with("usable from DAA 1050 (now 1050"));
        assert!(lock_verdict(1_050, "x", None, true).unwrap().contains("no --node"));
    }

    /// A node that knows one UTXO and counts the queries it is asked.
    struct Mock {
        txid: String,
        daa: u64,
        asked: usize,
    }
    impl Transport for Mock {
        fn call(&mut self, method: &str, _params: Value) -> Result<Value, String> {
            assert_eq!(method, "getUtxosByAddresses", "one small query per object, nothing else");
            self.asked += 1;
            Ok(json!({"entries": [{"outpoint": {"transactionId": self.txid, "index": 1},
                "utxoEntry": {"amount": 1_000_000_000u64, "blockDaaScore": self.daa, "covenantId": null}}]}))
        }
    }

    /// Defect 3 of the first live run: a command given a node refreshes an object file that has no DAA score (or a
    /// stale one) from the node, one query per object; without a node the refusal names the fix.
    #[test]
    fn a_node_fills_in_a_missing_or_stale_daa_and_offline_refuses_with_the_fix() {
        let net = Network::Testnet10;
        let m = OMarket::testnet([7; 32]);
        let st = Js::free([9; 32], m.judge_market());
        let txid = "ab".repeat(32);
        let dir = std::env::temp_dir().join(format!("kpf-daa-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, daa: Option<u64>| {
            let f = ObjFile { op: kaspa_consensus_core::tx::TransactionOutpoint { transaction_id: kaspa_consensus_core::tx::TransactionId::from_bytes(hex32(&txid).unwrap()), index: 1 }, amount: 1_000_000_000, daa, covenant_id: Hash::default(), redeem: OObj::Stake(st.clone()).redeem() };
            let p = dir.join(name);
            write_json(&p, &f.to_json(net)).unwrap();
            p.display().to_string()
        };
        let fresh = write("no-daa.json", None);
        let stale = write("stale.json", Some(5));
        // Offline: refused, and the message says what fixes it.
        let none: &mut Option<Node<Mock>> = &mut None;
        let err = stake_from(&fresh, none, net).unwrap_err();
        assert!(err.contains("has no `daa`") && err.contains("--node"), "{err}");
        // Offline with a DAA in the file: used as it is.
        assert_eq!(stake_from(&stale, none, net).unwrap().daa, 5);
        // With a node: filled in, and a stale score replaced; one query per object.
        let mut node = Some(Node::new(Mock { txid: txid.clone(), daa: 777, asked: 0 }));
        assert_eq!(stake_from(&fresh, &mut node, net).unwrap().daa, 777);
        assert_eq!(stake_from(&stale, &mut node, net).unwrap().daa, 777);
        assert_eq!(node.as_ref().unwrap().t.asked, 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Defect 7: a dry run writes under `dry-run/`; only a submitted transaction writes at the top of the out-dir.
    #[test]
    fn dry_runs_have_their_own_folder() {
        let a = args(&["x", "--out-dir", "somewhere"]);
        assert_eq!(out_dir(&a, true), PathBuf::from("somewhere").join("dry-run"));
        assert_eq!(out_dir(&a, false), PathBuf::from("somewhere"));
        assert_eq!(out_dir(&args(&["x"]), true), PathBuf::from("kpf-out").join("dry-run"));
    }
}
