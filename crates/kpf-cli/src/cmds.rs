//! The commands. Everything is offline unless `--node` is given; nothing is submitted without `--submit`.

use std::path::{Path, PathBuf};

use kaspa_consensus_core::Hash;
use kpf_protocol::build::*;
use kpf_protocol::error::{from_hex, to_hex, Error};
use kpf_protocol::network::{p2pk_address, p2sh_address, Network, TESTNET_ONLY};
use kpf_protocol::payoff::*;
use kpf_protocol::state::*;
use kpf_protocol::tx::*;
use kpf_protocol::verify::verify;
use serde_json::{json, Value};

use crate::args::*;
use crate::files::*;
use crate::keys;
use crate::node::{Node, WsTransport};
use crate::report;

pub(crate) type R<T> = Result<T, String>;

pub(crate) fn e<T: ToString>(x: T) -> String {
    x.to_string()
}

pub(crate) fn network(a: &Args) -> R<Network> {
    Network::parse(a.get("network").unwrap_or("testnet-10")).map_err(e)
}

pub(crate) fn fee_opts(a: &Args) -> R<Opts> {
    let rate = match a.get("fee-rate") {
        None => MIN_FEE_RATE,
        Some(s) => s.parse::<u64>().map_err(|_| format!("--fee-rate `{s}` is not an integer (sompi per gram)"))?,
    };
    if rate < MIN_FEE_RATE {
        return Err(format!("--fee-rate {rate} is below the relay minimum {MIN_FEE_RATE} sompi per gram"));
    }
    Ok(Opts { fee_rate: rate, fee_mode: if a.has("priority") { FeeMode::Priority } else { FeeMode::Relay } })
}

pub(crate) fn side(s: &str) -> R<i64> {
    match s.to_ascii_lowercase().as_str() {
        "long" | "1" => Ok(LONG),
        "short" | "2" => Ok(SHORT),
        _ => Err(format!("side must be long or short, not `{s}`")),
    }
}

pub(crate) fn int(a: &Args, name: &str) -> R<i64> {
    let s = a.need(name)?;
    s.replace('_', "").parse::<i64>().map_err(|_| format!("--{name} `{s}` is not an integer"))
}

pub(crate) fn int_or(a: &Args, name: &str, d: i64) -> R<i64> {
    if a.has(name) {
        int(a, name)
    } else {
        Ok(d)
    }
}

/// A price or rate (KAS per whole token, or `<n>s` sompi per whole token).
pub(crate) fn price(a: &Args, name: &str) -> R<i64> {
    kas(a.need(name)?).map_err(|m| format!("--{name}: {m}"))
}

pub(crate) fn price_or(a: &Args, name: &str, d: i64) -> R<i64> {
    if a.has(name) {
        price(a, name)
    } else {
        Ok(d)
    }
}

/// An amount of tokens (decimal whole tokens, or `<n>s` base units) of a market with `scale` base units per token.
pub(crate) fn tokens(a: &Args, name: &str, scale: i64) -> R<i64> {
    parse_amount(a.need(name)?, scale).map_err(|m| format!("--{name}: {m}"))
}

pub(crate) fn units_of(scale: i64) -> String {
    format!("tokens of {scale} base units")
}

// ---------------------------------------------------------------- node and funding

pub(crate) fn connect(a: &Args) -> R<Option<Node<WsTransport>>> {
    match a.get("node") {
        None => Ok(None),
        Some(url) => {
            let mut n = Node::new(WsTransport::connect(url)?);
            n.check_network()?;
            Ok(Some(n))
        }
    }
}

pub(crate) fn refresh(node: &mut Node<WsTransport>, o: &mut ObjFile, net: Network) -> R<()> {
    let list = node.utxos(&[p2sh_address(&o.redeem, net)])?;
    let u = list
        .iter()
        .find(|u| u.op == o.op)
        .ok_or_else(|| format!("UTXO {}:{} is not at the node (spent, or not accepted yet)", o.op.transaction_id, o.op.index))?;
    o.amount = u.amount;
    o.daa = Some(u.daa);
    if let Some(c) = u.covenant_id {
        o.covenant_id = c;
    }
    Ok(())
}

pub(crate) fn load_obj(a: &Args, flag: &str, node: &mut Option<Node<WsTransport>>, net: Network) -> R<ObjFile> {
    let mut o = ObjFile::load(a.need(flag)?)?;
    if let Some(n) = node {
        refresh(n, &mut o, net)?;
    }
    Ok(o)
}

pub(crate) struct Funder {
    pub(crate) key: [u8; 32],
    pub(crate) keys: kpf_protocol::tx::KeySet,
}

/// The funder key file plus every `--key` file, as one key set.
pub(crate) fn funder_and_keys(a: &Args, need_funder: bool) -> R<Option<Funder>> {
    let mut paths: Vec<&str> = vec![];
    let funder_pub = match a.get("funder") {
        Some(f) => {
            paths.push(f);
            Some(keys::load(Path::new(f))?.public)
        }
        None if need_funder => return Err("--funder <key file> is required (it pays the network fee and signs the funding inputs)".into()),
        None => None,
    };
    paths.extend(a.all("key"));
    let ks = keys::keyset(&paths)?;
    Ok(Some(Funder { key: funder_pub.unwrap_or([0; 32]), keys: ks }))
}

/// Builds with funding: the UTXOs of a `--funding` file exactly as listed, else (with `--node`) the largest UTXOs of
/// the funder's address, adding more until the builder is satisfied.
pub(crate) fn build_funded(
    a: &Args,
    net: Network,
    node: &mut Option<Node<WsTransport>>,
    funder: &[u8; 32],
    build: &dyn Fn(&Funding) -> kpf_protocol::Result<Built>,
) -> R<Built> {
    if let Some(f) = a.get("funding") {
        let utxos = funding_utxos(&parse_utxos(&read_json(f)?)?, funder);
        if utxos.is_empty() {
            return Err(format!("{f} lists no plain UTXO of the funder {}", p2pk_address(funder, net)));
        }
        return build(&Funding { utxos, change_key: *funder }).map_err(e);
    }
    let Some(n) = node else {
        return Err("no funding: give --funding <utxos.json> or --node <ws-url> to fetch the funder's UTXOs".into());
    };
    let mut utxos = funding_utxos(&n.utxos(&[p2pk_address(funder, net)])?, funder);
    if utxos.is_empty() {
        return Err(format!("the funder {} has no spendable UTXO at the node", p2pk_address(funder, net)));
    }
    utxos.sort_by_key(|u| std::cmp::Reverse(u.amount));
    let mut last = None;
    for k in 1..=utxos.len().min(30) {
        match build(&Funding { utxos: utxos[..k].to_vec(), change_key: *funder }) {
            Ok(b) => return Ok(b),
            Err(Error::Insufficient { need, have }) => last = Some(Error::Insufficient { need, have }),
            Err(other) => return Err(e(other)),
        }
    }
    Err(e(last.expect("at least one UTXO")))
}

// ---------------------------------------------------------------- finishing a transaction

pub(crate) fn finish(a: &Args, title: &str, built: Built, keys: &kpf_protocol::tx::KeySet, net: Network, node: &mut Option<Node<WsTransport>>) -> R<()> {
    let s = built.sign(keys).map_err(e)?;
    let v = verify(&s.tx, &s.entries);
    print!("{}", report::text(title, &s, &v, net, built.fee_rate));
    if !v.is_ok() {
        return Err("verification failed: nothing written, nothing submitted".into());
    }
    let out_dir = PathBuf::from(a.get("out-dir").unwrap_or("kpf-out"));
    let tag: String = s.txid().to_string().chars().take(8).collect();
    let mut created_files = vec![];
    if !a.has("no-save") {
        let made = s.created_utxos();
        for (op, entry, obj) in &made {
            let of = ObjFile { op: *op, amount: entry.amount, daa: None, covenant_id: entry.covenant_id.unwrap_or_default(), redeem: obj.redeem() };
            // An ask continued by an evidence fill comes with the custody that holds its remaining tokens.
            let custody = match obj {
                Obj::Kob(kpf_protocol::kob::KobObj::Ask(_)) => made.iter().find_map(|(cop, ce, co)| match co {
                    Obj::Kob(kpf_protocol::kob::KobObj::Token(t)) if t.scheme == kpf_protocol::kob::SCHEME_COVID && Some(Hash::from_bytes(t.owner)) == entry.covenant_id => {
                        Some(ObjFile { op: *cop, amount: ce.amount, daa: None, covenant_id: ce.covenant_id.unwrap_or_default(), redeem: co.redeem() })
                    }
                    _ => None,
                }),
                _ => None,
            };
            let path = out_dir.join(format!("{tag}-out{}-{}.json", op.index, obj.kind()));
            write_json(&path, &KobFile { obj: of, custody }.to_json(net))?;
            created_files.push(path.display().to_string());
        }
        let doc = report::json_doc(title, &s, &v, net, &created_files);
        let path = out_dir.join(format!("{tag}-tx.json"));
        write_json(&path, &doc)?;
        println!("written: {}", path.display());
        for f in &created_files {
            println!("written: {f}   (new object; its DAA is filled in by `kpf status --node ...` or when you pass --node to the next command)");
        }
    }
    if !a.has("brief") {
        println!("\ntransaction (JSON, RpcTransaction):\n{}", serde_json::to_string_pretty(&rpc_json(&s.tx)).expect("json"));
        println!("\ntransaction (hex, Borsh encoding of the consensus transaction):\n{}", tx_hex(&s.tx));
    }
    if a.has("submit") {
        let n = node.as_mut().ok_or("--submit needs --node <ws-url>")?;
        let id = n.submit(&rpc_json(&s.tx))?;
        println!("\nSUBMITTED: node accepted {id}");
    } else {
        println!("\nnot submitted (add --submit --node <ws-url> to send it)");
    }
    Ok(())
}

// ---------------------------------------------------------------- keygen, address

pub fn keygen(a: &Args) -> R<()> {
    a.only(&["out", "name", "show-secret", "allow-in-repo"])?;
    let net = network(a)?;
    let path = match a.get("out") {
        Some(p) => PathBuf::from(p),
        None => keys::default_key_path(a.get("name").unwrap_or("default"))?,
    };
    let k = keys::generate(&path, net, a.has("allow-in-repo"))?;
    println!("key file    {}", path.display());
    println!("public key  {}", to_hex(&k.public));
    println!("address     {}", p2pk_address(&k.public, net));
    if a.has("show-secret") {
        println!("secret key  {}", to_hex(&k.secret));
    } else {
        println!("The secret key is in the file only. Back it up; whoever has the file controls the funds.");
    }
    println!("permissions: {}", k.permissions);
    println!("{TESTNET_ONLY}: fund this address with testnet-10 coins only.");
    Ok(())
}

pub fn address(a: &Args) -> R<()> {
    a.only(&["key", "pubkey", "redeem", "object", "json"])?;
    let net = network(a)?;
    let mut out = vec![];
    if let Some(k) = a.get("key") {
        let k = keys::load(Path::new(k))?;
        out.push(("p2pk", p2pk_address(&k.public, net), to_hex(&k.public)));
    }
    if let Some(p) = a.get("pubkey") {
        let k = keys::party(p, net)?;
        out.push(("p2pk", p2pk_address(&k, net), to_hex(&k)));
    }
    if let Some(r) = a.get("redeem") {
        let rs = from_hex(r).map_err(e)?;
        out.push(("p2sh", p2sh_address(&rs, net), format!("{} byte redeem script", rs.len())));
    }
    if let Some(o) = a.get("object") {
        let o = ObjFile::load(o)?;
        out.push(("p2sh", p2sh_address(&o.redeem, net), format!("{} byte redeem script", o.redeem.len())));
    }
    if out.is_empty() {
        return Err("give --key <file>, --pubkey <hex|address|file>, --redeem <hex> or --object <file>".into());
    }
    for (k, addr, what) in &out {
        if a.has("json") {
            println!("{}", json!({"kind": k, "address": addr, "detail": what, "network": net.name()}));
        } else {
            println!("{addr}   ({k}; {what}; {})", net.name());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- inspect, quote

fn load_decoded(a: &Args) -> R<(Decoded, Vec<u8>, Option<ObjFile>)> {
    if let Some(f) = a.get("object").or_else(|| a.get("pair")).or_else(|| a.get("order")) {
        let o = ObjFile::load(f)?;
        return Ok((o.decoded()?, o.redeem.clone(), Some(o)));
    }
    let hex = if let Some(r) = a.get("redeem") {
        r.to_string()
    } else if let Some(p) = a.positional.first() {
        if Path::new(p).is_file() {
            let o = ObjFile::load(p)?;
            return Ok((o.decoded()?, o.redeem.clone(), Some(o)));
        }
        p.clone()
    } else {
        return Err("give an object file, a redeem script in hex (positional or --redeem)".into());
    };
    let rs = from_hex(&hex).map_err(e)?;
    Ok((decode_redeem(&rs).map_err(e)?, rs, None))
}

pub fn inspect(a: &Args) -> R<()> {
    a.only(&["object", "pair", "order", "redeem", "json"])?;
    let net = network(a)?;
    let (d, rs, obj) = load_decoded(a)?;
    let (kind, state, tpl) = match &d {
        Decoded::Pair(p) => ("pair", p.to_json(), kpf_protocol::artifacts::pair_template().hash),
        Decoded::Open(o) => ("order", o.to_json(), kpf_protocol::artifacts::open_template().hash),
        Decoded::Pending(w) => ("pending", w.to_json(), kpf_protocol::artifacts::pending_template().hash),
        Decoded::Lock(l) => ("lock", l.to_json(), kpf_protocol::artifacts::lock_template().hash),
    };
    let mut doc = json!({
        "kind": kind, "network": net.name(), "address": p2sh_address(&rs, net), "redeemBytes": rs.len(),
        "templateHash": to_hex(&tpl), "state": state,
    });
    if let Some(o) = &obj {
        doc["utxo"] = json!({"outpoint": outpoint_json(&o.op), "amount": o.amount, "daa": o.daa, "covenantId": o.covenant_id.to_string()});
    }
    if a.has("json") {
        println!("{}", serde_json::to_string_pretty(&doc).expect("json"));
        return Ok(());
    }
    println!("{kind}   {}   template {}", p2sh_address(&rs, net), &to_hex(&tpl)[..16]);
    match &d {
        Decoded::Pair(p) => {
            let sc = p.m.scale;
            println!("size {} tokens ({} base units, scale {sc})", fmt_units(p.size as i128, sc as i128), p.size);
            println!("range  bl {} .. bs {} KAS per token   entry {}   floor value {} KAS", fmt_kas(p.bl as i128), fmt_kas(p.bs as i128), fmt_kas(p.entry as i128), fmt_kas(p.floor() as i128));
            println!("long   {}   listing ask {} (min part {}, tip {})", to_hex(&p.long_key), fmt_kas(p.long_ask as i128), p.long_min, fmt_kas(p.long_tip as i128));
            println!("short  {}   listing bid {} (min part {}, tip {})", to_hex(&p.short_key), fmt_kas(p.short_bid as i128), p.short_min, fmt_kas(p.short_tip as i128));
            println!("tags   long {}  short {}", short_hash(&p.tag_l), short_hash(&p.tag_s));
            match (p.since, obj.as_ref().and_then(|o| o.daa)) {
                (0, Some(d)) => println!("evidence clock  since 0: starts at this UTXO's DAA {d}; a hand-over is allowed from DAA {}", d as i64 + 4 * p.m.min_rest_daa),
                (0, None) => println!("evidence clock  since 0: starts at this UTXO's DAA (unknown here)"),
                (s, _) => println!("evidence clock  since {s}; a hand-over is allowed from DAA {}", s + 4 * p.m.min_rest_daa),
            }
            if !p.range_ok() {
                println!("WARNING: the range is narrower than both maintenance bands (DESIGN 4.5c): neither side can be liquidated");
            }
            println!("idle settle after {} DAA ({:.1} days)", p.m.idle_daa, p.m.idle_daa as f64 / 864_000.0);
        }
        Decoded::Open(o) => {
            let sc = o.m.scale;
            println!(
                "{} order of {} tokens left, limit {} KAS, own margin rate {} KAS, accepts counter rate >= {} KAS, min fill {} tokens, tip {} KAS",
                if o.side == LONG { "long" } else { "short" },
                fmt_units(o.size_left as i128, sc as i128),
                fmt_kas(o.limit as i128),
                fmt_kas(o.my_rate as i128),
                fmt_kas(o.min_counter_rate as i128),
                fmt_units(o.min_fill as i128, sc as i128),
                fmt_kas(o.tip as i128)
            );
            println!("owner {}   active from DAA {}   expires DAA {}   escrow {} KAS", to_hex(&o.owner), o.active_from, o.expiry_daa, fmt_kas(o.escrow() as i128));
        }
        Decoded::Pending(w) => {
            let sc = w.m.scale;
            let side = if w.side == LONG { "long" } else { "short" };
            let claim = if w.kind == KIND_CLOSE { format!("the {side} closes at {} KAS", fmt_kas(w.quote as i128)) } else { format!("the {side} is liquidatable (evidence quote {} KAS)", fmt_kas(w.quote as i128)) };
            println!("PENDING claim (DESIGN 4.5d): {claim} on {} tokens of the range {} .. {} KAS", fmt_units(w.n as i128, sc as i128), fmt_kas(w.bl as i128), fmt_kas(w.bs as i128));
            println!("bond {} KAS, refund {} KAS, fee {} KAS, proposer {}", fmt_kas(w.bond as i128), fmt_kas(w.refund as i128), fmt_kas(w.fee as i128), to_hex(&w.proposer_key));
            if let Err(e) = w.m.validate() {
                println!("WARNING: the market block fails the bounds every order checks ({e}): no real pair has it (a look-alike); nothing below about what refutes it can be trusted, and no guarantee of it is possible");
            }
            match w.contradiction_bound() {
                Some(b) => println!("refuted by a fresh {} {} {} KAS (margin g {} bps) resting {} DAA from this UTXO's DAA and fillable {} DAA more", if w.counter_is_ask() { "ask" } else { "bid" }, if w.counter_is_ask() { "<=" } else { ">=" }, fmt_kas(b as i128), w.m.g_bps, w.m.min_rest_daa, w.m.min_evidence_life),
                None => println!("no quote in 0..=priceMax refutes it"),
            }
            if w.can_cure() {
                println!("cure: the {side} may move its bound to {} KAS (adding margin) before a finalize", fmt_kas(w.cure_bound as i128));
            }
            match obj.as_ref().and_then(|o| o.daa) {
                Some(d) => println!("finalize from DAA {} (this UTXO's DAA {d} + the window {})", w.finalize_from(d), w.m.challenge_daa),
                None => println!("finalize once this UTXO is {} DAA old", w.m.challenge_daa),
            }
            if !w.hashes_match() {
                println!("WARNING: the pair states this part may become do not match the state it describes: not written by the pinned KpfPair (a look-alike)");
            }
        }
        Decoded::Lock(l) => {
            let sc = l.m.scale;
            let side = if l.side == LONG { "long" } else { "short" };
            let claim = if l.kind == KIND_CLOSE { format!("the {side} closed at {} KAS", fmt_kas(l.quote as i128)) } else { format!("the {side} was liquidated (evidence quote {} KAS)", fmt_kas(l.quote as i128)) };
            println!("LOCK of a guaranteed fast settlement (DESIGN 4.5f): {claim} on {} tokens of the range {} .. {} KAS", fmt_units(l.n as i128, sc as i128), fmt_kas(l.bl as i128), fmt_kas(l.bs as i128));
            println!("guarantor {}   wronged party if refuted {}   cover {} KAS   claim id {}   clock P {}", to_hex(&l.owner), to_hex(&l.victim), fmt_kas(l.cover as i128), short_hash(&l.claim_id), l.clock);
            if let Err(e) = l.m.validate() {
                println!("WARNING: the market block fails the bounds every order checks ({e}): no guarantee wrote this lock (a look-alike)");
            }
            match l.contradiction_bound() {
                Some(b) => println!("disputed by a fresh {} {} {} KAS resting {} DAA from P and done resting by F + {} DAA", if l.counter_is_ask() { "ask" } else { "bid" }, if l.counter_is_ask() { "<=" } else { ">=" }, fmt_kas(b as i128), l.m.min_rest_daa, l.m.dispute_daa - l.m.min_rest_daa),
                None => println!("no quote in 0..=priceMax disputes it"),
            }
            match obj.as_ref().and_then(|o| o.daa) {
                Some(d) => println!("release from DAA {} (this UTXO's DAA F = {d} + the window {})", l.release_from(d), l.m.dispute_daa),
                None => println!("release once this UTXO is {} DAA old", l.m.dispute_daa),
            }
        }
    }
    let m = match &d {
        Decoded::Pair(p) => &p.m,
        Decoded::Open(o) => &o.m,
        Decoded::Pending(w) => &w.m,
        Decoded::Lock(l) => &l.m,
    };
    println!(
        "market token {}  template {}  maintenance {} bps  evidence k {} bps  rest {} DAA  liquidation fee {} bps (min {} KAS)  carrier {} KAS",
        short_hash(&m.token_cov_id),
        short_hash(&m.token_tpl_hash),
        m.mmr_bps,
        m.k_bps,
        m.min_rest_daa,
        m.liq_fee_bps,
        fmt_kas(m.keeper_fee_min as i128),
        fmt_kas(m.carrier as i128)
    );
    println!(
        "two-step evidence (DESIGN 4.5d): bond {} bps of the part's range (at least {} KAS), counter-evidence k_c {} bps, margin g {} bps, window {} DAA, evidence life {} DAA",
        m.bond_bps,
        fmt_kas(m.bond_min as i128),
        m.kc_bps,
        m.g_bps,
        m.challenge_daa,
        m.min_evidence_life
    );
    println!(
        "guaranteed fast settlement (DESIGN 4.5f): lock ratio lambda {} bps, dispute window {} DAA after the settlement, cure grace of a liquidation {} DAA",
        m.lock_bps, m.dispute_daa, m.grace_daa
    );
    if let Some(o) = &obj {
        println!("utxo {}:{}  {} KAS  DAA {}  covenant id {}", o.op.transaction_id, o.op.index, fmt_kas(o.amount as i128), daa_str(o.daa), short_hash(&o.covenant_id));
    }
    Ok(())
}

pub(crate) fn daa_str(d: Option<u64>) -> String {
    d.map(|x| x.to_string()).unwrap_or_else(|| "unknown".into())
}

pub(crate) fn short_hash(h: &Hash) -> String {
    to_hex(&h.as_bytes()[..6])
}

fn price_list(a: &Args, p: &Pair) -> R<Vec<i64>> {
    if let Some(l) = a.get("prices") {
        return l.split(',').map(|s| kas(s.trim()).map_err(|m| format!("--prices: {m}"))).collect();
    }
    let from = price_or(a, "from", p.bl - (p.bs - p.bl) / 4)?.max(0);
    let to = price_or(a, "to", p.bs + (p.bs - p.bl) / 4)?;
    let steps = int_or(a, "steps", 12)?.clamp(1, 200);
    if to <= from {
        return Err("--to must be above --from".into());
    }
    let mut v: Vec<i64> = (0..=steps).map(|i| from + (to - from) * i / steps).collect();
    for k in [p.bl, p.entry, p.bs] {
        if k >= from && k <= to && !v.contains(&k) {
            v.push(k);
        }
    }
    v.sort_unstable();
    Ok(v)
}


/// What a liquidation of `n` of a pair gives each side (DESIGN 4.5b): the loser `side` forfeits, the winner takes the
/// part's whole value less the keeper fee.
struct LiqInfo {
    side: i64,
    /// The extreme evidence quote that still liquidates: the highest ask (long), the lowest bid (short).
    limit: i64,
    trigger: i64,
    /// The keeper fee at the extreme quote, and its cap (the maintenance slice).
    fee: i64,
    slice: i64,
    /// What the winner receives at the extreme quote, and at least (its payoff at the trigger price).
    winner: i64,
    winner_min: i64,
}

fn liq_info(p: &Pair, part: &Pair, n: i64, side: i64) -> LiqInfo {
    let limit = if side == LONG {
        ((p.bl as i128 * 10_000 - 1) / (10_000 - p.m.mmr_bps as i128)) as i64
    } else {
        ((p.bs as i128 * 10_000) / (10_000 + p.m.mmr_bps as i128) + 1) as i64
    };
    let value = if n == p.size { part.floor() } else { part.floor() - p.m.carrier };
    let fee = liq_fee(part, n, limit, side);
    LiqInfo { side, limit, trigger: trigger_of(p, side), fee, slice: slice_of(part, n, side), winner: (value - fee).max(0), winner_min: winner_at_trigger(part, n, side) }
}
pub fn quote(a: &Args) -> R<()> {
    a.only(&["object", "pair", "redeem", "prices", "from", "to", "steps", "n", "json"])?;
    let (d, _rs, _) = load_decoded(a)?;
    let Decoded::Pair(p) = d else {
        return Err("quote needs a pair (an order has no payoff table); use `kpf inspect` for an order".into());
    };
    let sc = p.m.scale;
    let n = if a.has("n") { tokens(a, "n", sc)? } else { p.size };
    let part = p.with_size(n);
    let prices = price_list(a, &p)?;
    let t_long = trigger_of(&p, LONG);
    let t_short = trigger_of(&p, SHORT);
    let a_max = (p.bl as i128 * 10_000 - 1) / (10_000 - p.m.mmr_bps as i128);
    let b_min = (p.bs as i128 * 10_000) / (10_000 + p.m.mmr_bps as i128) + 1;
    if let Err(e) = p.m.validate() {
        eprintln!("WARNING: the market block fails the bounds every order checks ({e}): no real pair has it (a look-alike); its refutation and guarantee figures below cannot be trusted");
    }
    let rows = quote_table(&part, &prices);
    let (liq_long, liq_short) = (liq_info(&p, &part, n, LONG), liq_info(&p, &part, n, SHORT));
    // Two-step evidence (DESIGN 4.5d): what a claim on this part costs and what refutes it.
    let bond = kpf_protocol::pending::bond_of(&p, n);
    let refund = kpf_protocol::pending::refund_of(&p, n);
    let claim = |kind: i64, side: i64, q: i64| kpf_protocol::pending::pending_of(&p.clocked(0), n, kind, side, q, [0; 32]);
    let refute_long = claim(KIND_LIQ, LONG, liq_long.limit).contradiction_bound();
    let refute_short = claim(KIND_LIQ, SHORT, liq_short.limit).contradiction_bound();
    let cure_long = kpf_protocol::pending::cure_bound_of(&p.m, LONG, liq_long.limit);
    let cure_short = kpf_protocol::pending::cure_bound_of(&p.m, SHORT, liq_short.limit);
    // Guaranteed fast settlement (DESIGN 4.5f): what a guarantor locks for a claim on this part and what it earns.
    let lam = p.m.lock_bps;
    let liq_cover = part.range_of(n);
    let ec = p.entry.clamp(p.bl, p.bs);
    let close_long_cover = kpf_protocol::payoff::q(n, ec - p.bl, sc, 0);
    let close_short_cover = kpf_protocol::payoff::q(n, p.bs - ec, sc, 0);
    let lock_for = |cover: i64| kpf_protocol::lock::ratio_up(cover, lam).max(kpf_protocol::lock::LOCK_FLOOR);
    let reward_for = |cover: i64| kpf_protocol::lock::ratio_down(cover, lam - kpf_protocol::state::BPS);
    let max_offer = (bond - kpf_protocol::state::PLAIN_DUST).max(0);
    if a.has("json") {
        let r: Vec<Value> = rows
            .iter()
            .map(|r| {
                json!({"price": r.price, "long": r.long, "short": r.short, "longLiquidatable": r.long_liquidatable, "shortLiquidatable": r.short_liquidatable,
                       "longLiqFee": r.long_liquidatable.then(|| liq_fee(&part, n, r.price, LONG)), "shortLiqFee": r.short_liquidatable.then(|| liq_fee(&part, n, r.price, SHORT))})
            })
            .collect();
        let liq: Vec<Value> = [liq_long, liq_short]
            .iter()
            .map(|i| json!({"loser": if i.side == LONG { "long" } else { "short" }, "extremeQuote": i.limit, "trigger": i.trigger, "feeAtExtremeQuote": i.fee, "feeCap": i.slice, "winnerReceives": i.winner, "winnerReceivesAtLeast": i.winner_min, "loserReceives": 0}))
            .collect();
        let two_step = json!({
            "bond": bond, "refund": refund, "challengeDaa": p.m.challenge_daa, "gBps": p.m.g_bps, "minEvidenceLife": p.m.min_evidence_life,
            "liquidationOfTheLongRefutedByABidAtLeast": refute_long, "liquidationOfTheShortRefutedByAnAskAtMost": refute_short,
            "cureBoundLongAtExtremeQuote": cure_long, "cureBoundShortAtExtremeQuote": cure_short,
        });
        let guarantee = json!({
            "lockBps": lam, "disputeDaa": p.m.dispute_daa, "graceDaa": p.m.grace_daa,
            "liquidation": {"cover": liq_cover, "lock": lock_for(liq_cover), "disputerReward": reward_for(liq_cover)},
            "closeOfTheLongAtEntry": {"cover": close_long_cover, "lock": lock_for(close_long_cover), "disputerReward": reward_for(close_long_cover)},
            "closeOfTheShortAtEntry": {"cover": close_short_cover, "lock": lock_for(close_short_cover), "disputerReward": reward_for(close_short_cover)},
            "guarantorEarnsTheOfferUpTo": max_offer, "offerIsAddedToTheLock": true,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"size": n, "rows": r, "triggerLong": t_long, "triggerShort": t_short, "floorValue": part.floor(), "liquidation": liq, "twoStep": two_step, "guarantee": guarantee}))
                .expect("json")
        );
        return Ok(());
    }
    println!(
        "pair of {} tokens: range {} .. {} KAS per token, entry {}; pair value {} KAS (range {} + carrier {})",
        fmt_units(n as i128, sc as i128),
        fmt_kas(p.bl as i128),
        fmt_kas(p.bs as i128),
        fmt_kas(p.entry as i128),
        fmt_kas(part.floor() as i128),
        fmt_kas((part.floor() - p.m.carrier) as i128),
        fmt_kas(p.m.carrier as i128)
    );
    println!("payouts exclude the carrier. A long is liquidatable by an evidence ask <= {} KAS, a short by an evidence bid >= {} KAS.", fmt_kas(a_max), fmt_kas(b_min));
    println!("trigger price T: long {} KAS, short {} KAS (the winner receives at least its payoff at T)", fmt_kas(t_long as i128), fmt_kas(t_short as i128));
    println!("liquidation (the loser forfeits everything, DESIGN 4.5b), for {} tokens:", fmt_units(n as i128, sc as i128));
    for i in [&liq_long, &liq_short] {
        println!(
            "  {:<5} liquidated by an evidence {} {} {} KAS (trigger price {} KAS): the {} receives 0; the {} receives {} KAS at that quote (the part's value less the fee {} KAS, which is at most {} KAS) and at least {} KAS",
            if i.side == LONG { "long" } else { "short" },
            if i.side == LONG { "ask" } else { "bid" },
            if i.side == LONG { "<=" } else { ">=" },
            fmt_kas(i.limit as i128),
            fmt_kas(i.trigger as i128),
            if i.side == LONG { "long" } else { "short" },
            if i.side == LONG { "short" } else { "long" },
            fmt_kas(i.winner as i128),
            fmt_kas(i.fee as i128),
            fmt_kas(i.slice as i128),
            fmt_kas(i.winner_min as i128)
        );
    }
    println!("{:>16}  {:>20}  {:>20}  {:>12}  {:>14}", "price (KAS/token)", "long payout (KAS)", "short payout (KAS)", "liquidatable", "keeper fee (KAS)");
    for r in &rows {
        let (liq, fee) = if r.long_liquidatable {
            ("long", fmt_kas(liq_fee(&part, n, r.price, LONG) as i128))
        } else if r.short_liquidatable {
            ("short", fmt_kas(liq_fee(&part, n, r.price, SHORT) as i128))
        } else {
            ("-", "-".to_string())
        };
        println!("{:>16}  {:>20}  {:>20}  {:>12}  {:>14}", fmt_kas(r.price as i128), fmt_kas(r.long as i128), fmt_kas(r.short as i128), liq, fee);
    }
    println!("Liquidation fee: max(keeperFeeMin, q(n, quote * liqFeeBps / 10^4)) capped at the maintenance slice; the loser receives nothing (DESIGN 4.5b).");
    let opt = |v: Option<i64>| v.map(|x| format!("{} KAS", fmt_kas(x as i128))).unwrap_or_else(|| "none".into());
    println!(
        "Two-step (DESIGN 4.5d): a close or a liquidation of {} tokens is first proposed with a bond of {} KAS (refund {} KAS) and settles at finalize, {} DAA later at the earliest; a liquidation finalizes as forfeit and keep (the winner holds both sides) and pays the fee and the bond to the proposer.",
        fmt_units(n as i128, sc as i128),
        fmt_kas(bond as i128),
        fmt_kas(refund as i128),
        p.m.challenge_daa
    );
    println!(
        "  a claim at the extreme quote is refuted (margin g {} bps, counter order resting {} DAA from the proposal and fillable {} DAA after): the long's liquidation by a bid >= {}, the short's by an ask <= {}; a close at quote c by an opposite quote beyond c by g",
        p.m.g_bps,
        p.m.min_rest_daa,
        p.m.min_evidence_life,
        opt(refute_long),
        opt(refute_short)
    );
    println!("  the threatened owner may cure instead: the long to bl {}, the short to bs {} (at the extreme quotes)", opt(cure_long), opt(cure_short));
    println!(
        "Guaranteed fast settlement (DESIGN 4.5f, lambda {} bps): a guarantor may settle a pending claim at once by locking KAS for {} DAA after the settlement (a liquidation once the claim is {} DAA old: the loser's cure grace). It locks lambda times the value the settlement moves away from the party it could wrong, plus the proposer's offer:",
        lam, p.m.dispute_daa, p.m.grace_daa
    );
    println!("  a liquidation of {} tokens covers the part's range {} KAS: lock {} KAS", fmt_units(n as i128, sc as i128), fmt_kas(liq_cover as i128), fmt_kas(lock_for(liq_cover) as i128));
    println!(
        "  a close at the entry {} KAS covers the closer's payoff: the long's {} KAS (lock {} KAS), the short's {} KAS (lock {} KAS)",
        fmt_kas(p.entry as i128),
        fmt_kas(close_long_cover as i128),
        fmt_kas(lock_for(close_long_cover) as i128),
        fmt_kas(close_short_cover as i128),
        fmt_kas(lock_for(close_short_cover) as i128)
    );
    println!(
        "  the guarantor earns the proposer's offer (0 to {} KAS, set at the proposal, returned with the lock after the window) and risks the whole lock: if the book refutes the claim within the window, the wronged party is paid the whole cover from it, a disputer keeps (lambda - 1) x cover ({} KAS for the liquidation) and the rest (the offer and rounding) is burned",
        fmt_kas(max_offer as i128),
        fmt_kas(reward_for(liq_cover) as i128)
    );
    Ok(())
}

// ---------------------------------------------------------------- market

pub fn market(a: &Args) -> R<()> {
    a.only(&["example", "check", "idle-daa", "carrier", "scale", "min-size"])?;
    if let Some(f) = a.get("check") {
        let m = market_from_file(f)?;
        println!("{f}: a valid market (scale {}, priceMax {}, carrier {} KAS)", m.scale, m.price_max, fmt_kas(m.carrier as i128));
        return Ok(());
    }
    let mut m = kpf_protocol::fixtures::market();
    if a.has("idle-daa") {
        m.idle_daa = int(a, "idle-daa")?;
    }
    if a.has("scale") {
        m.scale = int(a, "scale")?;
    }
    if a.has("carrier") {
        m.carrier = kas(a.need("carrier")?).map_err(|x| format!("--carrier: {x}"))?;
    }
    if a.has("min-size") {
        m.min_size = int(a, "min-size")?;
    }
    m.validate().map_err(e)?;
    println!("{}", serde_json::to_string_pretty(&m.to_json()).expect("json"));
    eprintln!("This is the DESIGN 5 worked example with placeholder token identity (tokenCovId, tokenTplHash, extensionCommitment): replace them with the real token's before use.");
    Ok(())
}

// ---------------------------------------------------------------- builders

const FEE_FLAGS: &[&str] = &["fee-rate", "priority", "out-dir", "submit", "node", "brief", "no-save", "funding", "funder", "key"];

pub(crate) fn only_with(a: &Args, own: &[&str]) -> R<()> {
    let mut all: Vec<&str> = own.to_vec();
    all.extend(FEE_FLAGS);
    a.only(&all)
}

pub fn create_order_cmd(a: &Args) -> R<()> {
    only_with(a, &["market", "side", "owner", "size", "limit", "rate", "min-counter-rate", "min-fill", "tip", "active-from", "expiry"])?;
    let net = network(a)?;
    let m = market_from_file(a.need("market")?)?;
    let sc = m.scale;
    let fk = funder_and_keys(a, true)?.expect("funder");
    let owner = match a.get("owner") {
        Some(o) => keys::party(o, net)?,
        None => fk.key,
    };
    let o = Open {
        m: m.clone(),
        side: side(a.need("side")?)?,
        owner,
        size_left: tokens(a, "size", sc)?,
        limit: price(a, "limit")?,
        my_rate: price(a, "rate")?,
        min_counter_rate: price_or(a, "min-counter-rate", 0)?,
        min_fill: if a.has("min-fill") { tokens(a, "min-fill", sc)? } else { m.min_size },
        tip: price_or(a, "tip", 0)?,
        active_from: int_or(a, "active-from", 0)?,
        expiry_daa: int(a, "expiry").map_err(|_| "--expiry <DAA score> is required (the order is refundable from then; see `kpf utxos`/`status` for the current DAA)".to_string())?,
    };
    o.validate().map_err(e)?;
    println!("order: {} {} ({}), escrow {} KAS", if o.side == LONG { "long" } else { "short" }, fmt_units(o.size_left as i128, sc as i128), units_of(sc), fmt_kas(o.escrow() as i128));
    order_ticket(&o);
    let mut node = connect(a)?;
    let opts = fee_opts(a)?;
    let b = build_funded(a, net, &mut node, &fk.key, &|f| create_order(&o, f, &opts))?;
    finish(a, "create order", b, &fk.keys, net, &mut node)
}

pub fn cancel_cmd(a: &Args) -> R<()> {
    only_with(a, &["order", "to"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lo = load_obj(a, "order", &mut node, net)?.live_open()?;
    let fk = funder_and_keys(a, false)?.expect("keys");
    let dest = match a.get("to") {
        Some(t) => keys::party(t, net)?,
        None => lo.o.owner,
    };
    let b = cancel_order(&lo, &dest, &fee_opts(a)?).map_err(e)?;
    finish(a, "cancel order", b, &fk.keys, net, &mut node)
}

pub fn refund_cmd(a: &Args) -> R<()> {
    only_with(a, &["order"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lo = load_obj(a, "order", &mut node, net)?.live_open()?;
    let fk = funder_and_keys(a, true)?.expect("funder");
    let opts = fee_opts(a)?;
    println!("refund is due at DAA {} (lock time of the transaction)", (lo.o.expiry_daa as u64).min(lo.daa + MAX_IDLE));
    let b = build_funded(a, net, &mut node, &fk.key, &|f| refund_order(&lo, f, &opts))?;
    finish(a, "refund order", b, &fk.keys, net, &mut node)
}

pub fn open_cmd(a: &Args) -> R<()> {
    only_with(a, &["long", "short", "n", "x"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lo = load_obj(a, "long", &mut node, net)?.live_open()?;
    let so = load_obj(a, "short", &mut node, net)?.live_open()?;
    let sc = lo.o.m.scale;
    let (n, x) = (tokens(a, "n", sc)?, price(a, "x")?);
    let fk = funder_and_keys(a, true)?.expect("funder");
    let opts = fee_opts(a)?;
    let b = build_funded(a, net, &mut node, &fk.key, &|f| open_orders(&lo, &so, n, x, f, &opts))?;
    finish(a, "open pair (order x order)", b, &fk.keys, net, &mut node)
}

pub fn open_taker_cmd(a: &Args) -> R<()> {
    only_with(a, &["order", "n", "x", "rate"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let o = load_obj(a, "order", &mut node, net)?.live_open()?;
    let sc = o.o.m.scale;
    let (n, x, rate) = (tokens(a, "n", sc)?, price(a, "x")?, price(a, "rate")?);
    let fk = funder_and_keys(a, true)?.expect("funder");
    let opts = fee_opts(a)?;
    let taker = fk.key;
    let b = build_funded(a, net, &mut node, &fk.key, &|f| open_taker(&o, n, x, rate, &taker, f, &opts))?;
    finish(a, "open pair (order x signing taker)", b, &fk.keys, net, &mut node)
}

pub fn amend_cmd(a: &Args) -> R<()> {
    only_with(a, &["pair", "side", "price", "min-part", "tip", "unlist"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load_obj(a, "pair", &mut node, net)?.live_pair()?;
    let sd = side(a.need("side")?)?;
    let w = if a.has("unlist") {
        (0, 0, 0)
    } else {
        (price(a, "price")?, if a.has("min-part") { tokens(a, "min-part", l.p.m.scale)? } else { 0 }, price_or(a, "tip", 0)?)
    };
    let fk = funder_and_keys(a, true)?.expect("funder");
    let opts = fee_opts(a)?;
    let b = build_funded(a, net, &mut node, &fk.key, &|f| amend(&l, sd, w, f, &opts))?;
    finish(a, if a.has("unlist") { "amend: unlist" } else { "amend: list" }, b, &fk.keys, net, &mut node)
}

pub fn add_margin_cmd(a: &Args) -> R<()> {
    only_with(a, &["pair", "side", "delta"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load_obj(a, "pair", &mut node, net)?.live_pair()?;
    let (sd, delta) = (side(a.need("side")?)?, price(a, "delta")?);
    let fk = funder_and_keys(a, true)?.expect("funder");
    let opts = fee_opts(a)?;
    println!("add margin cost: {} KAS (delta {} KAS per token over {} tokens, rounded up)", fmt_kas(margin_cost(&l.p, delta) as i128), fmt_kas(delta as i128), fmt_units(l.p.size as i128, l.p.m.scale as i128));
    let b = build_funded(a, net, &mut node, &fk.key, &|f| add_margin(&l, sd, delta, f, &opts))?;
    finish(a, "add margin", b, &fk.keys, net, &mut node)
}

pub fn transfer_cmd(a: &Args) -> R<()> {
    only_with(a, &["pair", "side", "n", "x", "entrant-order", "rate", "k", "daa"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load_obj(a, "pair", &mut node, net)?.live_pair()?;
    let now = crate::evcmds::now_daa(a, &mut node)?;
    crate::evcmds::print_clock("pair", &l, now);
    println!("the part that changes hands starts a fresh evidence clock (since = 0); the remainder keeps DAA {}", l.clock());
    let sd = side(a.need("side")?)?;
    let (n, x) = (tokens(a, "n", l.p.m.scale)?, price(a, "x")?);
    let k = int_or(a, "k", 0)?;
    let fk = funder_and_keys(a, true)?.expect("funder");
    let ent = if a.has("entrant-order") {
        Entrant::Open(load_obj(a, "entrant-order", &mut node, net)?.live_open()?)
    } else {
        Entrant::Direct { key: fk.key, rate: price(a, "rate").map_err(|_| "give --entrant-order <order file>, or --rate <margin rate> for a signing taker (the funder)".to_string())? }
    };
    let opts = fee_opts(a)?;
    let b = build_funded(a, net, &mut node, &fk.key, &|f| transfer(&l, sd, n, x, &ent, k, f, &opts))?;
    crate::evcmds::lock_guard(a, &b, now)?;
    finish(a, if matches!(ent, Entrant::Open(_)) { "transfer (order takes the listing)" } else { "transfer (signing taker)" }, b, &fk.keys, net, &mut node)
}

pub fn close_vs_close_cmd(a: &Args) -> R<()> {
    only_with(a, &["host", "donor", "n", "x", "e3", "daa"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let h = load_obj(a, "host", &mut node, net)?.live_pair()?;
    let d = load_obj(a, "donor", &mut node, net)?.live_pair()?;
    let now = crate::evcmds::now_daa(a, &mut node)?;
    crate::evcmds::print_clock("host", &h, now);
    crate::evcmds::print_clock("donor", &d, now);
    let (n, x) = (tokens(a, "n", h.p.m.scale)?, price(a, "x")?);
    let e3 = price_or(a, "e3", h.p.entry)?;
    let fk = funder_and_keys(a, true)?.expect("funder");
    let opts = fee_opts(a)?;
    let c = CloseSpec { n, x, e3 };
    let b = build_funded(a, net, &mut node, &fk.key, &|f| close_vs_close(&h, &d, &c, f, &opts))?;
    crate::evcmds::lock_guard(a, &b, now)?;
    finish(a, "close versus close", b, &fk.keys, net, &mut node)
}

pub fn mutual_close_cmd(a: &Args) -> R<()> {
    only_with(a, &["pair", "n", "x"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load_obj(a, "pair", &mut node, net)?.live_pair()?;
    let (n, x) = (tokens(a, "n", l.p.m.scale)?, price(a, "x")?);
    let fk = funder_and_keys(a, true)?.expect("funder");
    let opts = fee_opts(a)?;
    let b = build_funded(a, net, &mut node, &fk.key, &|f| mutual_close(&l, n, x, f, &opts))?;
    finish(a, "mutual close", b, &fk.keys, net, &mut node)
}

pub fn coop_close_cmd(a: &Args) -> R<()> {
    only_with(a, &["pair", "long-pay", "fee-from"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load_obj(a, "pair", &mut node, net)?.live_pair()?;
    let long_pay = kas(a.need("long-pay")?).map_err(|m| format!("--long-pay: {m}"))?;
    if long_pay < 0 {
        return Err("--long-pay must not be negative".into());
    }
    let ff = match a.get("fee-from").unwrap_or("short") {
        "long" => FeeFrom::Long,
        "short" => FeeFrom::Short,
        o => return Err(format!("--fee-from must be long or short, not `{o}`")),
    };
    let fk = funder_and_keys(a, false)?.expect("keys");
    let opts = fee_opts(a)?;
    println!("both owners' keys are needed: long {} and short {}", to_hex(&l.p.long_key), to_hex(&l.p.short_key));
    let b = if a.has("funder") {
        build_funded(a, net, &mut node, &fk.key, &|f| coop_close(&l, long_pay as u64, ff, Some(f), &opts))?
    } else {
        coop_close(&l, long_pay as u64, ff, None, &opts).map_err(e)?
    };
    finish(a, "cooperative close", b, &fk.keys, net, &mut node)
}

pub fn idle_settle_cmd(a: &Args) -> R<()> {
    only_with(a, &["pair"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load_obj(a, "pair", &mut node, net)?.live_pair()?;
    let fk = funder_and_keys(a, true)?.expect("funder");
    let opts = fee_opts(a)?;
    println!("idle settle is valid {} DAA after the pair's UTXO was created (DAA {}), i.e. from DAA {}", l.p.m.idle_daa, l.daa, l.daa + l.p.m.idle_daa as u64);
    let b = build_funded(a, net, &mut node, &fk.key, &|f| idle_settle(&l, f, &opts))?;
    finish(a, "idle settle", b, &fk.keys, net, &mut node)
}

// ---------------------------------------------------------------- verify, submit, utxos, status

fn load_tx_file(path: &str) -> R<(kaspa_consensus_core::tx::Transaction, Vec<kaspa_consensus_core::tx::UtxoEntry>, Value)> {
    let v = read_json(path)?;
    let tx = tx_from_rpc_json(v.get("rpc").ok_or_else(|| format!("{path}: no `rpc` transaction"))?).map_err(e)?;
    let entries = v
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{path}: no `entries`"))?
        .iter()
        .map(|x| entry_from_json(x).map_err(e))
        .collect::<R<Vec<_>>>()?;
    Ok((tx, entries, v))
}

pub fn verify_cmd(a: &Args) -> R<()> {
    a.only(&["tx"])?;
    let (tx, entries, _) = load_tx_file(a.need("tx")?)?;
    if entries.len() != tx.inputs.len() {
        return Err("the file has a different number of entries than inputs".into());
    }
    let v = verify(&tx, &entries);
    for (i, r) in v.inputs.iter().enumerate() {
        match r {
            Ok(u) => println!("input {i}: ok ({u} script units, budget {})", v.budgets[i]),
            Err(m) => println!("input {i}: FAILED {m}"),
        }
    }
    for t in &v.tx_errors {
        println!("transaction: {t}");
    }
    if v.is_ok() {
        println!("txid {}: valid (fee {} KAS)", tx.id(), fmt_kas(v.fee as i128));
        Ok(())
    } else {
        Err("the transaction does not verify".into())
    }
}

pub fn submit_cmd(a: &Args) -> R<()> {
    a.only(&["tx", "node"])?;
    let (tx, entries, _) = load_tx_file(a.need("tx")?)?;
    let v = verify(&tx, &entries);
    if !v.is_ok() {
        return Err(format!("the transaction does not verify locally, not submitting: {}", v.errors().join("; ")));
    }
    let mut node = connect(a)?.ok_or("--node <ws-url> is required")?;
    let id = node.submit(&rpc_json(&tx))?;
    println!("SUBMITTED: node accepted {id}");
    Ok(())
}

pub fn utxos_cmd(a: &Args) -> R<()> {
    a.only(&["node", "key", "address", "out", "json"])?;
    let net = network(a)?;
    let mut node = connect(a)?.ok_or("--node <ws-url> is required")?;
    let addr = match (a.get("key"), a.get("address")) {
        (Some(k), _) => p2pk_address(&keys::load(Path::new(k))?.public, net),
        (None, Some(ad)) => {
            kpf_protocol::network::parse_address(ad, net).map_err(e)?;
            ad.to_string()
        }
        _ => return Err("give --key <file> or --address <kaspatest:...>".into()),
    };
    let list = node.utxos(&[addr.clone()])?;
    let total: u64 = list.iter().map(|u| u.amount).sum();
    println!("{addr}: {} UTXOs, {} KAS", list.len(), fmt_kas(total as i128));
    let arr: Vec<Value> = list
        .iter()
        .map(|u| json!({"txid": u.op.transaction_id.to_string(), "index": u.op.index, "amount": u.amount, "daa": u.daa, "covenantId": u.covenant_id.map(|c| c.to_string()), "scriptPublicKey": u.spk.as_ref().map(spk_hex)}))
        .collect();
    for u in &list {
        println!("  {}:{}  {} KAS  DAA {}", u.op.transaction_id, u.op.index, fmt_kas(u.amount as i128), u.daa);
    }
    if let Some(p) = a.get("out") {
        write_json(Path::new(p), &Value::Array(arr))?;
        println!("written: {p} (usable as --funding)");
    }
    Ok(())
}

pub fn status_cmd(a: &Args) -> R<()> {
    a.only(&["object", "node", "update"])?;
    let net = network(a)?;
    let mut node = connect(a)?.ok_or("--node <ws-url> is required")?;
    let mut count = 0;
    for f in a.all("object") {
        let mut o = ObjFile::load(f)?;
        match refresh(&mut node, &mut o, net) {
            Ok(()) => {
                println!("{f}: LIVE  {} KAS  DAA {}  covenant id {}", fmt_kas(o.amount as i128), daa_str(o.daa), short_hash(&o.covenant_id));
                if a.has("update") {
                    write_json(Path::new(f), &o.to_json(net))?;
                    println!("  updated {f}");
                }
            }
            Err(m) => println!("{f}: NOT LIVE ({m})"),
        }
        count += 1;
    }
    if count == 0 {
        return Err("give --object <file> (repeatable)".into());
    }
    Ok(())
}

pub fn daa_cmd(a: &Args) -> R<()> {
    a.only(&["node"])?;
    let mut node = connect(a)?.ok_or("--node <ws-url> is required")?;
    let p = node.t_call("getBlockDagInfo")?;
    println!("network {}   virtual DAA score {}", p.get("network").and_then(Value::as_str).unwrap_or("?"), p.get("virtualDaaScore").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default());
    Ok(())
}

/// What an order ticket must say (review 3 F4, DESIGN 4.5c): a liquidation forfeits the whole margin, where the order's
/// own side is liquidatable at its limit, and whether the order can be filled at its limit at all.
pub(crate) fn order_ticket(o: &Open) {
    let m = &o.m;
    let at_limit = if o.side == LONG { o.limit - o.my_rate } else { o.limit + o.my_rate };
    let trig = if o.side == LONG {
        (at_limit as i128 * BPS as i128 / (BPS - m.mmr_bps) as i128) as i64
    } else {
        ((at_limit as i128 * BPS as i128 + (BPS + m.mmr_bps) as i128 - 1) / (BPS + m.mmr_bps) as i128) as i64
    };
    println!(
        "NOTE: a liquidation forfeits the whole margin (DESIGN 4.5b). Filled at its limit {} KAS this {} gets the bound {} KAS and is liquidatable once a resting {} {} {} KAS has rested {} DAA on the pair's clock. Keep the limit within your margin rate less maintenance of the market.",
        fmt_kas(o.limit as i128),
        if o.side == LONG { "long" } else { "short" },
        fmt_kas(at_limit as i128),
        if o.side == LONG { "ask" } else { "bid" },
        if o.side == LONG { "below" } else { "above" },
        fmt_kas(trig as i128),
        m.min_rest_daa
    );
    if !healthy_at(m, o.side, at_limit, o.limit) {
        println!(
            "WARNING: the margin rate {} KAS is below the {} bps maintenance of the limit {} KAS: KpfOpen refuses a fill at the limit (a side may not be born liquidatable, DESIGN 4.5c); it fills only where its rate covers the maintenance",
            fmt_kas(o.my_rate as i128),
            m.mmr_bps,
            fmt_kas(o.limit as i128)
        );
    }
    if o.active_from > 0 {
        println!("the order is fillable from DAA {} (activeFrom; the builders set the lock time to it)", o.active_from);
    }
}
