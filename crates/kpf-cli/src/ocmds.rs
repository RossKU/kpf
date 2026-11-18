//! The commands of oracle-only markets (docs/DESIGN-ORACLE-IMPL.md, third slice; runbook: docs/TESTNET.md). Offline
//! unless `--node` is given, nothing submitted without `--submit`, every transaction verified locally first, as the
//! book-market commands.

use std::path::{Path, PathBuf};

use kaspa_consensus_core::Hash;
use kpf_protocol::bond::{Bond, BondParams};
use kpf_protocol::error::{from_hex, to_hex};
use kpf_protocol::judge::{Js, JudgeParams, NO, YES};
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
use crate::node::{Node, WsTransport};

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
    s += &format!("A contest is judged by aged bonded KAS: the side committing at least {} times the other's wins after rounds of {}; stake counts only if it was bonded {} before the claim. If colluders control that stake, false prices stand.\n", m.judge.mu_bps as f64 / 1e4, h(m.judge.round), h(m.judge.age));
    s += &format!("An underwriter may underwrite up to {} times its bond. Its bond is never paid to you in this version: a refuted claim costs its proposer the claim's bond ({} % of the cover, at least {} KAS), {} % of it to the disputer and the rest to the wronged party.\n", m.bond.rho_bps as f64 / 1e4, p.sigma_bps as f64 / 100.0, k(p.bond_min), p.beta_bps as f64 / 100.0);
    s += &format!("A side is liquidated (it forfeits its whole margin to the other side) when a vouched price, moved {} % in its favour, is past its trigger at {} % maintenance. A close by a party is paid at the vouched price moved {} % against the closer.\n", p.g_bps as f64 / 100.0, p.mmr_bps as f64 / 100.0, p.g_bps as f64 / 100.0);
    s += &format!("The cover runs for a term of {} and is renewed by the underwriter at the agreed premium, taken half from each side's margin. If the underwriter stops, the pair can be closed or transferred without any price; once its cover has lapsed ({} after expiry) and it is {} idle, anyone settles it at its entry price.\n", h(m.bond.term), h(m.bond.tail), h(p.idle_daa));
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
fn stake_of_path(path: &str) -> R<OLive<Js>> {
    let f = ObjFile::load(path)?;
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
    if !a.has("no-save") {
        let dir = PathBuf::from(a.get("out-dir").unwrap_or("kpf-out"));
        let tag: String = sg.tx.id().to_string().chars().take(8).collect();
        for (i, obj, _) in &sg.created {
            let p = dir.join(format!("{tag}-out{i}-{}.json", obj.kind().replace(' ', "-")));
            write_json(&p, &obj_json(&sg, *i, obj, net))?;
            println!("written: {}", p.display());
        }
        let p = dir.join(format!("{tag}-tx.json"));
        write_json(&p, &json!({ "format": "kpf-tx/1", "title": title, "network": net.name(), "txid": sg.tx.id().to_string(), "transaction": rpc_json(&sg.tx) }))?;
        println!("written: {}", p.display());
    }
    if a.has("submit") {
        let n = node.as_mut().ok_or("--submit needs --node <ws-url>")?;
        println!("SUBMITTED: node accepted {}", n.submit(&rpc_json(&sg.tx))?);
    } else {
        println!("not submitted (add --submit --node <ws-url> to send it)");
    }
    Ok(())
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

/// `oracle-order`: an order of one side, escrowing its margin, its tip, its half of the first premium and a carrier.
pub fn order_cmd(a: &Args) -> R<()> {
    only(a, &["market", "side", "size", "limit", "rate", "expiry", "owner", "min-counter-rate", "min-fill", "tip", "active-from", "underwriter", "prem-max", "escrow"])?;
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
        uw_accept: match a.get("underwriter") {
            Some(h) => hex32(h)?,
            None => [0; 32],
        },
        prem_max: price_or(a, "prem-max", 0)?,
    };
    let need = o.fill_spend(size, o.prem_max) as u64 + 100_000_000;
    let esc = if a.has("escrow") { kas(a.need("escrow")?)? as u64 } else { need };
    let b = create_order(&o, esc, &fu, &fk, rate(a)?).map_err(e)?;
    println!("escrow {} KAS (a full fill takes at most {} KAS of it)", fmt_kas(esc as i128), fmt_kas(o.fill_spend(size, o.prem_max) as i128));
    finish(a, "oracle-order", &b, &keys, net, &mut node)
}

/// `oracle-open`: two orders and the underwriter's slot open a pair (the underwriter's key signs the bind).
pub fn open_cmd(a: &Args) -> R<()> {
    only(a, &["long", "short", "slot", "n", "x", "cover-rate", "premium", "expiry"])?;
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
    let (b, p) = open(&lo, &so, &sl, &terms, &fu, &fk, rate(a)?).map_err(e)?;
    println!("pair [{} .. {}] KAS of {} units, cover {} KAS until DAA {xe}", fmt_kas(p.bl as i128), fmt_kas(p.bs as i128), fmt_units(p.size as i128, sc as i128), fmt_kas(p.cover_of(p.size) as i128));
    finish(a, "oracle-open", &b, &keys, net, &mut node)
}

// ---------------------------------------------------------------- claims

/// `vouch`: the underwriter vouches a price and proposes a claim (a liquidation; or a close, which the closing party
/// signs too). `--vouch` is the underwriter's own UTXO (a funding file of one P2PK UTXO of its key).
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
    finish(a, "vouch: a price claim", &b, &keys, net, &mut node)
}

/// `contest`: open a dispute on a pending claim with an aged judge stake on NO.
pub fn contest_cmd(a: &Args) -> R<()> {
    only(a, &["pending", "stake", "base"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let w = pending_of(a, "pending", &mut node, net)?;
    let st = stake_of_path(a.need("stake")?)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let t = now(a, &mut node)?;
    let until = w.s.contest_until(w.daa);
    if t > until {
        println!("WARNING: the contest window ended at DAA {until}: a contest landing now is ignored and the claim settles as if uncontested");
    }
    let base = if a.has("base") { kas(a.need("base")?)? as u64 } else { 50_000_000 };
    let (b, _, _) = contest(&w, &st, base, &fu, &fk, rate(a)?).map_err(e)?;
    finish(a, "contest", &b, &keys, net, &mut node)
}

/// `deposit`: stakes join the trailing side of a dispute (`--side yes|no`, up to 7 `--stake` files).
pub fn deposit_cmd(a: &Args) -> R<()> {
    only(a, &["dispute", "side", "stake"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let d = stake_of_path(a.need("dispute")?)?;
    let sd = match a.need("side")? {
        "yes" => YES,
        "no" => NO,
        s => return Err(format!("--side yes|no, not `{s}`")),
    };
    let stakes = a.all("stake").into_iter().map(stake_of_path).collect::<R<Vec<_>>>()?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, n) = deposit(&d, sd, &stakes, &fu, &fk, rate(a)?).map_err(e)?;
    println!("after the deposit: {} leads, YES {} KAS, NO {} KAS", if n.side == YES { "YES" } else { "NO" }, fmt_kas(n.yes as i128), fmt_kas(n.no as i128));
    finish(a, "deposit", &b, &keys, net, &mut node)
}

/// `resolve`: once the round's window has closed, the dispute resolves and its verdict reaches the contested part.
pub fn resolve_cmd(a: &Args) -> R<()> {
    only(a, &["dispute", "pending"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let d = stake_of_path(a.need("dispute")?)?;
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
    only(a, &["pair", "slot"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let mut pairs = vec![];
    for p in a.all("pair") {
        let f = ObjFile::load(p)?;
        match OObj::decode(&f.redeem).map_err(e)? {
            OObj::Pair(x) => pairs.push(live_of(&f, x)?),
            o => return Err(format!("{p} is a {}", o.kind())),
        }
    }
    let sl = slot_of(a, "slot", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let t = now(a, &mut node)?;
    finish(a, "renew", &renew(&pairs, &sl, t, &fu, &fk, rate(a)?).map_err(e)?.0, &keys, net, &mut node)
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
    let pool = stake_of_path(a.need("pool")?)?;
    let rs = a.all("receipt").into_iter().map(stake_of_path).collect::<R<Vec<_>>>()?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let b = if rs.is_empty() { pool_close(&pool, &fk, rate(a)?).map_err(e)? } else { pool_settle(&pool, &rs, &fu, &fk, rate(a)?).map_err(e)?.0 };
    finish(a, if rs.is_empty() { "close the pool" } else { "settle receipts" }, &b, &keys, net, &mut node)
}

// ---------------------------------------------------------------- watch

/// `oracle-watch`: for the key `--me`, every claim against it and the time left to contest, its pairs' underwriter
/// cover expiry and what follows it, and the disputes it has stake in, from object files (`--object`, `--scan DIR`).
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
        let Ok(of) = ObjFile::load(&f) else { continue };
        let Ok(obj) = OObj::decode(&of.redeem) else { continue };
        let daa = of.daa.map(|d| d as i64);
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
                let msg = if t < x - step {
                    format!("your underwriter's cover runs until DAA {x} ({} DAA left); renewal from DAA {}", x - t, x - step)
                } else if t < x {
                    format!("COVER EXPIRES at DAA {x} ({} DAA left): the underwriter may renew now; claims landing after DAA {} are void", x - t, x - p.m.p.challenge_daa)
                } else if t < lapse {
                    format!("THE COVER HAS EXPIRED (DAA {x}): no claim is valid; the underwriter may still renew; lapse settle at the entry price from DAA {lapse} once the pair is idle {} DAA", p.m.p.idle_daa)
                } else {
                    format!("THE COVER HAS LAPSED: anyone may settle the pair at its entry price once it is idle {} DAA (kpf lapse-settle --pair {f}); a price-free exit (transfer, mutual or cooperative close) is open meanwhile", p.m.p.idle_daa)
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
