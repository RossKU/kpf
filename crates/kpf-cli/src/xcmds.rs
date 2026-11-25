//! The executor's commands: a folder of a registry's objects kept from the chain (`registry-sync`), and the loop that acts on
//! it (`exec-run`): matches orders that cross, puts positions together, takes a user out of two that cancel, ends what
//! is late, and (asked to) disputes what the rule does not give.
//!
//! Load on the node. A round asks the node three small things: its DAG info, its sink's blue score, and one list of
//! UTXOs (the library's scripts, the registry's anchors, the objects in the folder, the sender's coins). Every
//! transaction of the family spends a copy of a script of the library and writes it again, so while the copies and
//! the anchors stand where the folder's last reading left them nothing happened in the registry and nothing more is
//! asked. When one moved, the transaction that moved it is named by where the copy stands now, and the one before it
//! by what that one spent: the transactions of the registry since the last reading are a chain walked back from the
//! present, and only they are fetched (each from the blocks its accepting chain block merged). To find those blocks
//! the node is asked which chain block accepted which id since the last reading: ids only, about 70 bytes a
//! transaction of the chain.
//!
//! KOB's indexer (`vendor/kob`, `kobd`) reads every transaction of the chain whole and without pause
//! (`getVirtualChainFromBlockV2` at verbosity High), next to a node of its own. Live run 11 read the chain that way,
//! on a change only, and still asked the node for 4 MB a reading and 25 to 85 MB for a user who had been away a
//! minute or five, on a day the chain carried a hundred transactions a second: what a node somebody else runs should
//! not be asked for a registry nobody trades in.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use kaspa_consensus_core::tx::TransactionOutpoint;
use kpf_protocol::committee::{Attestation, SignedPrice, EpochTable};
use kpf_protocol::committee_build::CObj;
use kpf_protocol::cross::{SIDE_A, SIDE_B};
use kpf_protocol::error::{from_hex, to_hex};
use kpf_protocol::network::{p2pk_address, p2sh_address, Network};
use kpf_protocol::oracle_build::{OBuilt, OLive, OObj};
use kpf_protocol::state::{LONG, SHORT};
use kpf_protocol::thin::{folded, match_orders, merged, novated, replaced, Position, LiqState, Order, Rest, TObj, Template};
use kpf_protocol::thin_artifacts::{self as pinned, SCRIPTS};
use kpf_protocol::thin_build::{self as tb, script_spk, LibScript, Payer};
use kpf_protocol::thin_read::{call_of, read_tx, slot_of, Known};
use kpf_protocol::tx::{p2pk_spk, KeySet, KeyUtxo};
use serde_json::{json, Value};

use crate::args::{fmt_kas, Args};
use crate::cmds::{connect, e, int_or, network, R};
use crate::files::{funding_utxos, parse_utxos, read_json, write_json, ListedUtxo, ObjFile};
use crate::node::{chain_tx, ChainTx, Cursor};
use crate::ocmds::{finish_signed, out_dir, rate};
use crate::tcmds::{lib_of, load_path, tx_file, write_made, write_made_as, Made, N};

/// How far back `registry-sync` and `exec-run` read without being told to (`--catch-up`): half an hour of blue score.
const BEHIND_MAX: u64 = 18_000;

/// An object of the registry, in its file.
#[derive(Clone)]
struct Item {
    path: PathBuf,
    f: ObjFile,
    obj: TObj,
}

/// The folder of a registry: the files of its objects that are thought live, those known spent (`spent/`, kept for what
/// they tell of the positions that followed), and the templates learnt (`templates/`).
struct Registry {
    dir: PathBuf,
    lib: [u8; 32],
    id: [u8; 32],
    net: Network,
    items: Vec<Item>,
    old: Vec<TObj>,
    tpls: Vec<Template>,
    /// `--only`: the files written are those of the objects this key is a party of.
    only: Option<[u8; 32]>,
    /// The library's fee pool, if its genesis wrote one: where the oracle fees go.
    fee_pool: Option<tb::FeePoolOf>,
}

fn position_in(o: &TObj) -> Option<Position> {
    match o {
        TObj::Position(a) => Some(*a),
        TObj::Claim(c) => Some(c.position),
        TObj::Disputed(d) => Some(d.claim.position),
        TObj::Liq(l) => Some(l.liq.position),
        TObj::ExitClaim(q) => Some(q.position),
        TObj::Change(c) => Some(c.position),
        _ => None,
    }
}

impl Known for Registry {
    fn object(&self, state: &[u8; 32]) -> Option<TObj> {
        self.items.iter().map(|i| &i.obj).chain(self.old.iter()).find(|o| o.hash() == *state).copied()
    }
    fn fixed(&self, h: &[u8; 32]) -> Option<Vec<u8>> {
        self.items.iter().map(|i| &i.obj).chain(self.old.iter()).filter_map(position_in).find(|a| a.fixed_hash() == *h).map(|a| a.fixed())
    }
    fn template(&self, h: &[u8; 32]) -> Option<Template> {
        self.tpls.iter().find(|t| t.hash() == *h).copied()
    }
}

/// The files of objects of the library `lib` in `dir` (not below it), with the template a file carries.
fn objects_in(dir: &Path, lib: &[u8; 32], net: Network) -> Vec<(Item, Option<Template>)> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    let mut paths: Vec<PathBuf> = rd.flatten().map(|f| f.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    paths.sort();
    for p in paths {
        let Some(s) = p.to_str() else { continue };
        let Ok(v) = read_json(s) else { continue };
        if v.get("thin").is_none() {
            continue;
        }
        let Ok(l) = load_path(s, &mut None, net) else { continue };
        if l.lib != *lib || matches!(l.obj, TObj::Anchor) {
            continue;
        }
        let tpl = v.get("template").and_then(Value::as_str).and_then(|h| from_hex(h).ok()).and_then(|b| Template::from_bytes(&b).ok());
        out.push((Item { path: p, f: l.f, obj: l.obj }, tpl));
    }
    out
}

impl Registry {
    fn open(a: &Args, net: Network) -> R<Registry> {
        let lib = lib_of(a)?;
        let path = a.need("registry")?;
        let id = tb::registry_check(&tx_file(path)?, &lib).map_err(|x| format!("{path}: {x}"))?;
        let dir = PathBuf::from(a.need("dir")?);
        std::fs::create_dir_all(dir.join("spent")).map_err(e)?;
        std::fs::create_dir_all(dir.join("templates")).map_err(e)?;
        let only = match a.get("only") {
            Some(k) => Some(crate::keys::party(k, net)?),
            None => None,
        };
        std::fs::create_dir_all(dir.join("gone")).map_err(e)?;
        let mut b = Registry { dir, lib, id, net, items: vec![], old: vec![], tpls: vec![], only, fee_pool: crate::tcmds::fee_pool_terms(a)? };
        // The templates given: a market and the three values of the declared form.
        for m in a.all("market1") {
            let (t, market, committee, scale, price_max) = crate::ccmds::market_parts(m)?;
            b.learn(Template { t, market, committee, scale, price_max, exit: int_or(a, "exit-daa", 864_000)?, dispute_daa: int_or(a, "dispute-daa", 600)?, bond_bps: int_or(a, "bond-bps", 1_000)? })?;
        }
        b.reload();
        Ok(b)
    }

    /// Reads the folder again (a command of this machine may have written into it).
    fn reload(&mut self) {
        self.items.clear();
        for (it, tpl) in objects_in(&self.dir, &self.lib, self.net) {
            // What a file says of its registry the node is asked before anything is built on it.
            if it.f.covenant_id.as_bytes() == self.id {
                self.tpls.extend(tpl.filter(|t| !self.tpls.contains(t)));
                self.items.push(it);
            }
        }
        self.old = objects_in(&self.dir.join("spent"), &self.lib, self.net).into_iter().map(|(i, _)| i.obj).collect();
        if let Ok(rd) = std::fs::read_dir(self.dir.join("templates")) {
            for f in rd.flatten() {
                let t = f.path().to_str().and_then(|p| read_json(p).ok()).and_then(|v| v.get("template").and_then(Value::as_str).and_then(|h| from_hex(h).ok())).and_then(|b| Template::from_bytes(&b).ok());
                self.tpls.extend(t.filter(|t| !self.tpls.contains(t)));
            }
        }
        // A position of one leg shows its template whole.
        let of: Vec<Template> = self.items.iter().map(|i| &i.obj).chain(self.old.iter()).filter_map(position_in).filter(|a| a.x.legs[1].size == 0).map(|a| Template::of(&a)).collect();
        for t in of {
            if !self.tpls.contains(&t) {
                self.tpls.push(t);
            }
        }
    }

    fn learn(&mut self, t: Template) -> R<()> {
        if !self.tpls.contains(&t) {
            let h = to_hex(&t.hash());
            write_json(&self.dir.join("templates").join(format!("{}.json", &h[..16])), &json!({ "format": "kpf-template/1", "hash": h, "template": to_hex(&t.bytes()) }))?;
            self.tpls.push(t);
        }
        Ok(())
    }

    /// The file of an object goes to `spent/`.
    fn retire(&mut self, op: &TransactionOutpoint) {
        if let Some(k) = self.items.iter().position(|i| i.f.op == *op) {
            let it = self.items.remove(k);
            if let Some(name) = it.path.file_name() {
                let _ = std::fs::rename(&it.path, self.dir.join("spent").join(name));
            }
            self.old.push(it.obj);
        }
    }

    /// A transaction the chain accepted: if it is of the library, the files of what it made in this registry are written
    /// and those of what it spent retired. Returns a line for the log.
    fn apply(&mut self, t: &ChainTx) -> R<Option<String>> {
        let Some(r) = read_tx(&self.lib, &t.tx, &t.amounts, self) else { return Ok(None) };
        if let Some(tpl) = r.template {
            self.learn(tpl)?;
        }
        let mut ours = false;
        for (i, _, obj) in &r.spent {
            let op = t.tx.inputs[*i].previous_outpoint;
            ours |= self.items.iter().any(|x| x.f.op == op);
            self.retire(&op);
            // What was rebuilt of an object the folder never held still tells the fixed part of what follows it.
            self.old.extend(obj.filter(|o| !matches!(o, TObj::Anchor)));
        }
        let mut made = vec![];
        for (i, o, id) in &r.made {
            if *id != self.id || matches!(o, TObj::Anchor) {
                continue;
            }
            if self.only.is_some_and(|k| !crate::tcmds::parties(o).contains(&k)) {
                // Somebody else's: no file, but what it tells of the positions that follow is kept for this run.
                self.old.push(*o);
                continue;
            }
            made.push(match o {
                TObj::Order(ord) => match self.template(&ord.tpl) {
                    Some(tpl) => Made::OrderOf(*i, *ord, tpl),
                    None => Made::Obj(*i, *o),
                },
                _ => Made::Obj(*i, *o),
            });
        }
        let under_registry = t.tx.outputs.iter().enumerate().any(|(i, o)| o.covenant.map(|c| c.covenant_id.as_bytes()) == Some(self.id) && r.unread.contains(&i));
        if made.is_empty() && !ours && !under_registry {
            return Ok(None);
        }
        let files = write_made_as(t.id, Some(t.daa), &t.tx, &made, &self.lib, self.net, &self.dir, false)?;
        for p in &files {
            if let Some(s) = p.to_str() {
                if let Ok(l) = load_path(s, &mut None, self.net) {
                    self.items.retain(|x| x.f.op != l.f.op);
                    self.items.push(Item { path: p.clone(), f: l.f, obj: l.obj });
                }
            }
        }
        let kinds: Vec<String> = made
            .iter()
            .map(|m| match m {
                Made::Obj(_, o) => o.kind().to_string(),
                Made::OrderOf(..) => "order".to_string(),
                Made::Script(..) => String::new(),
            })
            .collect();
        let blind = if under_registry { " (an object it made was NOT read: a position whose state nobody published; whoever holds its file has it)" } else { "" };
        Ok(Some(format!("  {} {}.{}: made [{}]{blind}", &t.id.to_string()[..8], r.script, r.entry, kinds.join(", "))))
    }
}

fn place_file(dir: &Path) -> PathBuf {
    dir.join("cursor.json")
}

/// Where the folder's reading of the chain stands: a place in the chain, and where the library's copies and the
/// registry's anchors stood then.
#[derive(Clone)]
struct Place {
    cursor: Cursor,
    stood: HashSet<TransactionOutpoint>,
}

fn place_read(dir: &Path) -> Option<Place> {
    let v = read_json(place_file(dir).to_str()?).ok()?;
    let stood = v.get("stood")?.as_array()?.iter().map(|o| crate::files::outpoint_from(o).ok()).collect::<Option<HashSet<_>>>()?;
    Some(Place { cursor: Cursor { hash: v.get("hash")?.as_str()?.to_string(), blue: v.get("blue")?.as_u64()?, daa: v.get("daa")?.as_u64()? }, stood })
}

fn place_write(dir: &Path, p: &Place) -> R<()> {
    let mut stood: Vec<String> = p.stood.iter().map(|o| format!("{}:{}", o.transaction_id, o.index)).collect();
    stood.sort();
    let stood: Vec<Value> = stood.iter().map(|x| x.rsplit_once(':').map(|(t, i)| json!({ "txid": t, "index": i.parse::<u32>().unwrap_or(0) })).unwrap_or(Value::Null)).collect();
    write_json(&place_file(dir), &json!({ "format": "kpf-cursor/2", "hash": p.cursor.hash, "blue": p.cursor.blue, "daa": p.cursor.daa, "stood": stood }))
}

/// What a reading of the chain cost and found.
#[derive(Default)]
struct Reading {
    calls: usize,
    bytes: usize,
    /// The ids of the chain's transactions the node listed, and the transactions of the library fetched.
    ids: usize,
    fetched: usize,
    /// Transactions of the library that the span read did not hold (the reading began after them).
    lost: usize,
    lines: Vec<String>,
}

impl Reading {
    fn say(&self) -> String {
        let lost = if self.lost > 0 { format!("; WARNING: {} transactions of the library are older than the span read: what they made is known from files only", self.lost) } else { String::new() };
        format!("{} calls, {} bytes: the ids of {} transactions of the chain, {} of the library fetched, {} of this registry{lost}", self.calls, self.bytes, self.ids, self.fetched, self.lines.len())
    }
}

/// What the node lists this moment: the library's copies, where they and the registry's anchors stand, and the list.
struct Seen {
    list: Vec<ListedUtxo>,
    copies: BTreeMap<&'static str, Vec<LibScript>>,
    stand: HashSet<TransactionOutpoint>,
    fee_pool: Option<tb::FeePool>,
}

fn sort_out(registry: &Registry, list: Vec<ListedUtxo>) -> Seen {
    let (lib_id, registry_id) = (kaspa_consensus_core::Hash::from_bytes(registry.lib), kaspa_consensus_core::Hash::from_bytes(registry.id));
    let names: Vec<(&'static str, kaspa_consensus_core::tx::ScriptPublicKey)> = SCRIPTS.iter().filter_map(|n| script_spk(n).ok().map(|s| (*n, s))).collect();
    let anchor = tb::object_spk(&registry.lib, &TObj::Anchor);
    let mut copies: BTreeMap<&'static str, Vec<LibScript>> = BTreeMap::new();
    let mut stand = HashSet::new();
    for u in &list {
        let Some(spk) = &u.spk else { continue };
        if u.covenant_id == Some(lib_id) {
            if let Some((name, _)) = names.iter().find(|(_, s)| s == spk) {
                copies.entry(name).or_default().push(OLive { s: name, value: u.amount, id: Some(lib_id), op: u.op, daa: u.daa });
                stand.insert(u.op);
            }
        } else if u.covenant_id == Some(registry_id) && *spk == anchor {
            stand.insert(u.op);
        }
    }
    let fee_pool = crate::tcmds::fee_pool_among(&list, &registry.lib, registry.fee_pool.as_ref()).1;
    Seen { list, copies, stand, fee_pool }
}

/// One list of UTXOs: the library's scripts, the registry's anchors, the folder's objects, and `more` addresses.
fn look(node: &mut N, registry: &Registry, more: &[String]) -> R<Seen> {
    let n = node.as_mut().ok_or("--node URL is needed")?;
    let mut addrs: Vec<String> = SCRIPTS.iter().map(|s| p2sh_address(pinned::kit().script(s).expect("a script"), registry.net)).collect();
    addrs.push(p2sh_address(&TObj::Anchor.script(&registry.lib), registry.net));
    addrs.extend(registry.fee_pool.as_ref().map(|of| p2sh_address(&of.script(), registry.net)));
    addrs.extend(more.iter().cloned());
    addrs.extend(registry.items.iter().map(|i| p2sh_address(&i.f.redeem, registry.net)));
    addrs.sort();
    addrs.dedup();
    Ok(sort_out(registry, n.utxos(&addrs)?))
}

fn num(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Reads into the registry what moved the library's copies and the registry's anchors from where they `stood` to where they
/// `stand`: the transactions of the registry since the place `from`, walked back from the present. A copy that stands at
/// a new outpoint names the transaction that wrote it; the copy that one spent names the one before; the walk ends
/// at an outpoint that stood. The node is asked for the ids the chain accepted since `from` (to learn the block of
/// each), then for the blocks of those transactions only.
fn follow(node: &mut N, registry: &mut Registry, from: &Cursor, stood: &HashSet<TransactionOutpoint>, stand: &HashSet<TransactionOutpoint>, out: &mut Reading) -> R<()> {
    let n = node.as_mut().ok_or("reading the chain needs --node URL")?;
    // Which chain block accepted which transaction, in the chain's order.
    let mut place: HashMap<String, (usize, String)> = HashMap::new();
    let (mut at, mut k) = (from.hash.clone(), 0usize);
    for _ in 0..200 {
        let step = n.chain_ids(&at)?;
        out.calls += 1;
        out.bytes += step.bytes;
        for (acc, ids) in step.groups {
            for id in ids {
                place.entry(id).or_insert((k, acc.clone()));
                k += 1;
            }
        }
        match step.tip {
            // The node answers a batch of chain blocks at a time: a full one is followed by another.
            Some(t) if step.added >= 1_000 => at = t,
            _ => break,
        }
    }
    out.ids = k;
    // The walk goes through outpoints: each is held, once its transaction is fetched, to being a copy of a script
    // under the library's id or an anchor under the registry's. An input that only looks like one (the same script under
    // another id, which anyone can make, with a long history of its own) is followed one transaction and no further.
    let mut queue: Vec<TransactionOutpoint> = stand.difference(stood).copied().collect();
    let mut done = HashSet::new();
    let scripts: Vec<kaspa_consensus_core::tx::ScriptPublicKey> = SCRIPTS.iter().filter_map(|x| script_spk(x).ok()).collect();
    let anchor_spk = tb::object_spk(&registry.lib, &TObj::Anchor);
    let true_one = |t: &ChainTx, index: u32| {
        t.tx.outputs.get(index as usize).is_some_and(|o| {
            let id = o.covenant.map(|c| c.covenant_id.as_bytes());
            (id == Some(registry.lib) && scripts.contains(&o.script_public_key)) || (id == Some(registry.id) && o.script_public_key == anchor_spk)
        })
    };
    let mut got: BTreeMap<usize, ChainTx> = BTreeMap::new();
    let mut heads: HashMap<String, (u64, Vec<String>)> = HashMap::new();
    let mut bodies: HashMap<String, Vec<Value>> = HashMap::new();
    let anchor = TObj::Anchor.hash();
    while let Some(op) = queue.pop() {
        let id = op.transaction_id;
        if !done.insert(op) {
            continue;
        }
        let want = id.to_string();
        let Some((k, acc)) = place.get(&want).cloned() else {
            out.lost += 1;
            continue;
        };
        if !heads.contains_key(&acc) {
            let (b, size) = n.block(&acc, false)?;
            out.calls += 1;
            out.bytes += size;
            let daa = num(b.get("header").and_then(|h| h.get("daaScore"))).ok_or("a chain block without its DAA score")?;
            let list = |key: &str| b.get("verboseData").and_then(|v| v.get(key)).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect::<Vec<_>>()).unwrap_or_default();
            heads.insert(acc.clone(), (daa, [list("mergeSetBluesHashes"), list("mergeSetRedsHashes")].concat()));
        }
        let (daa, merged) = heads[&acc].clone();
        let mut found = None;
        for h in &merged {
            if !bodies.contains_key(h) {
                let (b, size) = n.block(h, true)?;
                out.calls += 1;
                out.bytes += size;
                bodies.insert(h.clone(), b.get("transactions").and_then(Value::as_array).cloned().unwrap_or_default());
            }
            if let Some(t) = bodies[h].iter().find(|t| t.get("verboseData").and_then(|v| v.get("transactionId")).and_then(Value::as_str) == Some(want.as_str())) {
                found = Some(chain_tx(t, daa)?);
                break;
            }
        }
        // Held to what was asked for: a transaction is its id.
        let Some(t) = found.filter(|t| t.tx.id() == id) else {
            out.lost += 1;
            continue;
        };
        out.fetched += 1;
        if !true_one(&t, op.index) {
            continue;
        }
        for i in &t.tx.inputs {
            let of_the_registry = call_of(&i.signature_script).is_some() || slot_of(&i.signature_script).is_some_and(|(l, st)| l == registry.lib && st == anchor);
            if of_the_registry && !stood.contains(&i.previous_outpoint) {
                queue.push(i.previous_outpoint);
            }
        }
        got.insert(k, t);
    }
    // In the chain's order. What an input held: what the folder knows of it, or what an earlier one of these wrote.
    let mut values: HashMap<TransactionOutpoint, u64> = registry.items.iter().map(|i| (i.f.op, i.f.amount)).collect();
    for t in got.values_mut() {
        for (j, i) in t.tx.inputs.iter().enumerate() {
            if t.amounts[j].is_none() {
                t.amounts[j] = values.get(&i.previous_outpoint).copied();
            }
        }
        for (j, o) in t.tx.outputs.iter().enumerate() {
            values.insert(TransactionOutpoint { transaction_id: t.id, index: j as u32 }, o.value);
        }
        out.lines.extend(registry.apply(t)?);
    }
    Ok(())
}

/// The present as a place in the chain.
fn present(node: &mut N) -> R<(u64, Cursor)> {
    let n = node.as_mut().ok_or("--node URL is needed")?;
    let (daa, sink) = n.dag()?;
    let blue = n.sink_blue()?;
    Ok((daa, Cursor { hash: sink, blue, daa }))
}

/// Where the folder's reading stands. None: it has none, or was told to forget it (`--from-now`); it begins this
/// moment. Refused: one further back than the tool reads unasked.
fn standing(a: &Args, node: &mut N, registry: &Registry) -> R<Option<Place>> {
    let Some(p) = place_read(&registry.dir).filter(|_| !a.has("from-now")) else { return Ok(None) };
    let (_, now) = present(node)?;
    let behind = now.blue.saturating_sub(p.cursor.blue);
    if behind > BEHIND_MAX * 72 {
        return Err(format!("the folder's reading of the chain stopped {} hours ago: a node keeps about a day and a half. Go on from this moment with --from-now (what entered the registry meanwhile is known from files only)", behind / 36_000));
    }
    if behind > BEHIND_MAX && !a.has("catch-up") {
        return Err(format!(
            "the folder's reading of the chain stopped {} minutes ago (DAA {}). Reading on asks the node which transactions the chain accepted since (their ids, about 70 bytes each, of every user of the chain): say so with --catch-up, or go on from this moment with --from-now (what entered the registry meanwhile is then known from files only)",
            behind / 600,
            p.cursor.daa
        ));
    }
    Ok(Some(p))
}

fn begins(registry: &Registry, daa: u64) {
    println!("the folder's reading of the chain begins this moment (DAA {daa}): what entered the registry before it is known from files only (those of this machine's commands, and those given: put them in {})", registry.dir.display());
}

const SYNC: &[&str] = &["dir", "lib", "lib-unchecked", "registry", "node", "from-now", "catch-up", "market1", "dispute-daa", "bond-bps", "exit-daa", "only"];

/// `registry-sync`: the folder `--dir` of the registry `--registry` is brought to the present. If no copy of a script and no
/// anchor moved since the folder's last reading, nothing happened in the registry and nothing is read (one list asked of
/// the node). Else the transactions that moved them are fetched, the files of the orders and positions they made are
/// written (`--only KEY`: those this key is a party of), and those of what they spent go to `spent/`. An order
/// somebody matched, a position somebody merged or took a user out of: the file of what it became is here after
/// this, with nobody sending it.
pub fn registry_sync_cmd(a: &Args) -> R<()> {
    a.only(SYNC)?;
    let net = network(a)?;
    let mut node = connect(a)?;
    if node.is_none() {
        return Err("registry-sync reads the chain: give --node URL".into());
    }
    let mut registry = Registry::open(a, net)?;
    let before = standing(a, &mut node, &registry)?;
    // The place first, then the list: what the list shows is then never older than the place.
    let (daa, now) = present(&mut node)?;
    let seen = look(&mut node, &registry, &[])?;
    match &before {
        None => begins(&registry, daa),
        Some(p) if p.stood == seen.stand => println!("nothing moved in the registry since DAA {} (every copy of a script and every anchor stands where it stood): nothing read", p.cursor.daa),
        Some(p) => {
            let mut r = Reading::default();
            follow(&mut node, &mut registry, &p.cursor, &p.stood, &seen.stand, &mut r)?;
            for l in &r.lines {
                println!("{l}");
            }
            println!("read to DAA {daa}: {}", r.say());
        }
    }
    place_write(&registry.dir, &Place { cursor: now, stood: seen.stand })?;
    println!("{}: {} objects thought live, {} templates known", registry.dir.display(), registry.items.len(), registry.tpls.len());
    Ok(())
}

// ---------------------------------------------------------------- the loop

/// What the loop does, by name (`--do`).
const DEFAULT_DO: &[&str] = &["match", "novate", "merge", "late", "lapse", "sweep"];
const ALL_DO: &[&str] = &["match", "novate", "merge", "late", "lapse", "sweep", "dispute", "settle", "expire", "watch", "take", "answer"];


/// A round: what is live at the node this moment, and what this round already used of it.
struct Round<'a> {
    a: &'a Args,
    net: Network,
    lib: [u8; 32],
    keys: &'a KeySet,
    fk: [u8; 32],
    rate: u64,
    coins: Vec<KeyUtxo>,
    copies: BTreeMap<&'static str, Vec<LibScript>>,
    used: HashSet<TransactionOutpoint>,
    left: usize,
    did: usize,
    min_gain: i64,
    /// What this round sent.
    sent: Vec<Sent>,
    /// What was built and left alone, in this round or an earlier one: not built again (review T1b, F21).
    alone: HashSet<(&'static str, Vec<TransactionOutpoint>)>,
    refused: Vec<TransactionOutpoint>,
}

impl Round<'_> {
    fn free(&self, ops: &[TransactionOutpoint]) -> bool {
        self.left > 0 && ops.iter().all(|o| !self.used.contains(o))
    }

    /// Builds with a copy of `script` and a coin, signs, and sends if the sender is left at least `--min-gain` richer
    /// (`gainful`: else whatever it costs, for what was asked by name). True: sent (or, without `--submit`, shown).
    fn send(&mut self, node: &mut N, title: &str, script: &'static str, objs: &[TransactionOutpoint], gainful: bool, mk: &dyn Fn(&LibScript, Payer) -> R<(OBuilt, Vec<Made>)>) -> bool {
        // (Remembered by the script too: a fold that does not pay says nothing of a novation of the same two.)
        if !self.free(objs) || self.alone.contains(&(script, objs.to_vec())) {
            return false;
        }
        let Some(copy) = self.copies.get(script).and_then(|v| v.iter().find(|c| !self.used.contains(&c.op))).cloned() else {
            println!("  {title}: no copy of {script} is free this round (one transaction a copy a block: a library with more copies serves more)");
            return false;
        };
        let Some(coin) = self.coins.iter().find(|c| !self.used.contains(&c.outpoint)).cloned() else {
            println!("  {title}: no coin of the sender is free this round (a transaction takes one: keep several)");
            self.left = 0;
            return false;
        };
        let built = mk(&copy, Payer { funds: std::slice::from_ref(&coin), change_to: self.fk, rate: self.rate }).and_then(|(b, made)| b.sign(self.keys).map(|sg| (sg, made)).map_err(e));
        let (sg, mut made) = match built {
            Ok(x) => x,
            Err(x) => {
                println!("  {title}: not built: {x}");
                return false;
            }
        };
        let mine = p2pk_spk(&self.fk);
        let back: u64 = sg.tx.outputs.iter().filter(|o| o.script_public_key == mine && o.covenant.is_none()).map(|o| o.value).sum();
        let gain = back as i64 - coin.amount as i64;
        if gainful && gain < self.min_gain {
            println!("  {title}: left alone: it would leave the sender {} KAS (its share less the fee of {} KAS; --min-gain {})", fmt_kas(gain as i128), fmt_kas(sg.fee as i128), fmt_kas(self.min_gain as i128));
            self.alone.insert((script, objs.to_vec()));
            return false;
        }
        let submit = self.a.has("submit");
        let again = script_spk(script).ok();
        let at = sg.tx.outputs.iter().position(|o| Some(&o.script_public_key) == again.as_ref() && o.covenant.is_some());
        if let Some(i) = at {
            made.push(Made::Script(i, script));
        }
        println!("  {title}: the sender is left {} KAS", fmt_kas(gain as i128));
        self.used.extend(objs.iter().copied());
        self.used.insert(copy.op);
        self.used.insert(coin.outpoint);
        match finish_signed(self.a, title, &sg, self.net, node, submit) {
            Ok(()) => {
                if !self.a.has("no-save") {
                    if let Err(x) = write_made(&sg.tx, &made, &self.lib, self.net, &out_dir(self.a, !submit)) {
                        println!("  {title}: its files were not written: {x}");
                    }
                }
                if let (true, Some(i)) = (submit, at) {
                    self.sent.push(Sent { old: copy.op, new: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, objs: objs.to_vec(), until: 0 });
                    // A second copy this transaction moved (the one a sweep emptied) stands where it wrote it too.
                    for m in &made {
                        if let Made::Script(k, name) = m {
                            let was = self.copies.get(name).and_then(|v| v.iter().find(|c| objs.contains(&c.op))).map(|c| c.op);
                            if let (true, Some(old)) = (*k != i, was) {
                                self.sent.push(Sent { old, new: TransactionOutpoint { transaction_id: sg.tx.id(), index: *k as u32 }, objs: vec![], until: 0 });
                            }
                        }
                    }
                }
                self.left -= 1;
                self.did += 1;
                true
            }
            Err(x) => {
                // Another executor was first, or the object moved: left alone for some rounds.
                println!("  {title}: {x}");
                self.refused.extend(objs.iter().copied());
                false
            }
        }
    }
}

/// A transaction this machine sent: the copy of a script it spent and the one it wrote in its place, the objects
/// it spent, and the round it is waited for until.
struct Sent {
    old: TransactionOutpoint,
    new: TransactionOutpoint,
    objs: Vec<TransactionOutpoint>,
    until: i64,
}

fn at<T>(it: &Item, u: &ListedUtxo, s: T) -> OLive<T> {
    OLive { s, value: u.amount, id: u.covenant_id, op: it.f.op, daa: u.daa }
}

/// The price two orders that cross are matched at: the limit of the one that stood first (the other came to it).
fn match_price(long: &Order, long_daa: u64, short: &Order, short_daa: u64) -> i64 {
    if long_daa <= short_daa {
        long.limit
    } else {
        short.limit
    }
}

const RUN: &[&str] = &[
    "dir", "lib", "lib-unchecked", "registry", "node", "from-now", "catch-up", "market1", "dispute-daa", "bond-bps", "exit-daa", "funder", "funding", "key", "fee-rate", "out-dir", "no-save", "submit", "daa", "every", "rounds", "once", "do",
    "max-tx", "min-gain", "expiry-guard", "only", "signed-prices", "epoch-table", "epoch-tables", "silence", "watch-gain", "max-exposure", "price", "price-file", "price-cmd", "dispute-margin-bps", "take-max", "take-spread-bps", "member-key",
];

/// `exec-run`: the loop of an executor over the registry `--registry`, its folder `--dir`. Each round (`--every` seconds, 10
/// at least) it asks the node what is live, reads the chain if the registry moved, and sends what `--do` names and the
/// node would accept, `--max-tx` transactions a round at most, each paid by a coin of `--funder`:
///   match    two orders that cross, for all the smaller has, at the limit of the one that stood first
///   novate   a user out of two positions that cancel each other
///   merge    two positions of the same two users into one (or one folded into the other's leg)
///   late     a claim that landed late or before its time, ended
///   lapse    a claim this key disputed and nobody proved, ended
///   sweep    the oracle fees that gathered in a copy of a script, moved to the library's fee pool (one a round)
///   dispute  (asked by name) a claim or a liquidation whose amount is not the rule's at its own prices
///   settle   (asked by name) a claim nobody disputed, a dispute period old
///   expire   (asked by name) an order past its expiry, returned to its owner
///   watchtower    (asked by name, with `--signed-prices DIR` and `--epoch-table FILE` or `--epoch-tables DIR`) for the positions `--funder`
///            is a party of: when the newest price a quorum signed is `--silence` DAA old (a third of the time a
///            claim may land after its time, unless given) and the position is worth more to this key at that price
///            than its margin (by more than `--watch-gain` KAS), the position is claimed at that price; a claim of
///            this key that is disputed is proved with those signatures; a claim of this key is settled after its
///            dispute period. A committee that stops signing while it is losing does not get out at the entries
/// With an epoch table given, a round says what margin stands under each committee in this folder's sight beside the committee's
/// stake, and `--max-exposure R` leaves unmatched the orders that would take it above R times the stake.
/// The first four and the sweep are sent only when they leave the sender `--min-gain` KAS richer after the fee (default 0).
/// Without `--submit` nothing is sent: a round shows what it would do. Without `--node` (`--daa N --funding FILE`)
/// one round runs on the folder as it is.
pub fn exec_run_cmd(a: &Args) -> R<()> {
    a.only(RUN)?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let mut registry = Registry::open(a, net)?;
    if a.has("out-dir") && Path::new(a.need("out-dir")?) != registry.dir {
        return Err("exec-run writes into its folder: drop --out-dir (it is --dir)".into());
    }
    let wanted: Vec<&str> = match a.get("do") {
        None => DEFAULT_DO.to_vec(),
        // (All but the watchtower, which is for a user's own positions and reads the committee's prices.)
        Some("all") => ALL_DO.iter().copied().filter(|x| *x != "watch" || a.has("signed-prices")).collect(),
        Some(s) => s.split(',').map(|x| x.trim()).collect(),
    };
    if let Some(x) = wanted.iter().find(|x| !ALL_DO.contains(x)) {
        return Err(format!("--do `{x}`: one of {}, or `all`", ALL_DO.join(", ")));
    }
    let f = crate::cmds::funder_and_keys(a, true)?.expect("funder");
    let every = int_or(a, "every", 20)?.max(10) as u64;
    let live = node.is_some();
    let rounds = if a.has("once") || !live { 1 } else { int_or(a, "rounds", 0)? };
    // The folder is this command's out-dir: what it sends writes its files where the next round reads them.
    let mut flags = a.clone();
    flags.set("out-dir", registry.dir.to_str().ok_or("--dir is not a path this tool can write")?);
    // (What the watchtower and the limit need is asked for before the first round.)
    Watchtower::open(a)?;
    let c = Cfg {
        a: &flags,
        net,
        f: &f,
        wanted,
        max_tx: int_or(a, "max-tx", 4)?.max(1) as usize,
        guard: int_or(a, "expiry-guard", 100)?,
        min_gain: if a.has("min-gain") { crate::cmds::price(a, "min-gain")? } else { 0 },
        silence: if a.has("silence") { Some(int_or(a, "silence", 0)?.max(1)) } else { None },
        watch_gain: if a.has("watch-gain") { crate::cmds::price(a, "watch-gain")? } else { 0 },
        max_exposure: match a.get("max-exposure") {
            Some(x) => Some(x.parse::<f64>().ok().filter(|v| *v > 0.0).ok_or("--max-exposure is a number above 0: the margin under a committee over the committee's stake")?),
            None => None,
        },
        every_daa: every as i64 * 10,
        margin_bps: match a.get("dispute-margin-bps") {
            Some(x) => Some(x.parse::<i64>().ok().filter(|v| *v >= 0).ok_or("--dispute-margin-bps is a whole number of basis points, 0 or more")?),
            None => None,
        },
        take_max: match a.get("take-max") {
            Some(_) => Some(crate::cmds::price(a, "take-max")?).filter(|v| *v > 0),
            None if a.get("do").is_some_and(|d| d.split(',').any(|x| x.trim() == "take")) => return Err("--do take puts this executor's own margin behind orders nobody matched: give --take-max KAS (the most of its own margin it has in positions and orders at a time), and its own price of the market (--price-file, --price-cmd or --price, with one --market1)".into()),
            None => None,
        },
        take_spread_bps: int_or(a, "take-spread-bps", 100)?.max(1),
        live,
        me: p2pk_address(&f.key, net),
    };
    println!("executor {}: registry {} in {}; doing {}; a round every {every} s: the node is asked three small things a round, and for the registry's transactions only when a copy of a script or an anchor moved", c.me, to_hex(&registry.id), registry.dir.display(), c.wanted.join(", "));

    let mut st = Loop { marks: vec![], stood: None, waiting: vec![], avoid: HashMap::new(), alone: HashSet::new(), under: String::new(), told: HashSet::new(), round: 0 };
    if live {
        match standing(c.a, &mut node, &registry)? {
            Some(p) => {
                st.marks.push(p.cursor);
                st.stood = Some(p.stood);
            }
            None => begins(&registry, present(&mut node)?.0),
        }
    }
    let mut fails = 0u32;
    loop {
        match turn(&mut st, &c, &mut node, &mut registry) {
            Ok(()) => fails = 0,
            // A node that fails once is asked again, each time later: the loop is left running for days.
            Err(x) if live => {
                fails = (fails + 1).min(4);
                println!("round {}: {x}. Asked again in {} s", st.round, every << fails);
                match connect(c.a) {
                    Ok(n) => node = n,
                    Err(y) => println!("round {}: {y}", st.round),
                }
            }
            Err(x) => return Err(x),
        }
        if rounds > 0 && st.round >= rounds {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(every << fails));
    }
    Ok(())
}

/// What the watchtower knows of the committee: the folder the members' signed prices arrive in (`--signed-prices`: the files a
/// member's runner writes, `price-<t0>-<key>.json`) and the epoch tables it was given (`--epoch-table FILE`, `--epoch-tables DIR`, read
/// again every round: a committee rolls). An epoch table's member table says which signatures are a quorum, and a proof spends a
/// live epoch table of the same epoch.
struct Watchtower {
    dir: Option<PathBuf>,
    /// Every signed price this machine has read, under a name of its own (signed price, member, price): a later file of the
    /// same name does not take a price away (review T1b, F27).
    kept: Option<PathBuf>,
    epoch_tables: Vec<(ObjFile, EpochTable)>,
    /// The double prices already said.
    told: std::cell::RefCell<HashSet<([u8; 32], i64)>>,
}

/// One leg's signed prices of a signed price: the epoch table that covers the signed price, every valid signed price by member (a member may
/// have signed several), and the prices a quorum stands within the spread of, the lowest first.
struct Signed {
    k: usize,
    by_member: Vec<(usize, SignedPrice)>,
    /// The prices a claim of this machine's own is made at: those a member signed, where a quorum stands near one.
    prices: Vec<i64>,
    /// Every price found that a quorum stands near, the ends of the band among them (review T1b, F38: an epoch table
    /// attests any of them, so a claim is proved or refuted at any).
    all: Vec<i64>,
}

/// Why a signed price gives the watchtower no price.
enum NoPrice {
    /// No epoch table given covers the signed price.
    EpochTable,
    Quorum,
}

impl Watchtower {
    fn open(a: &Args) -> R<Option<Watchtower>> {
        // A file named by `--epoch-table` is an epoch table or the command is refused; a folder (`--epoch-tables`) is filled by
        // other hands while the loop runs, and a file in it that does not read as an epoch table (half written, or not
        // one) is passed over and said (review T1b, F31: one such file stopped every action of every round).
        let mut paths: Vec<(String, bool)> = a.all("epoch-table").iter().map(|s| (s.to_string(), true)).collect();
        if let Some(d) = a.get("epoch-tables") {
            for f in std::fs::read_dir(d).map_err(|x| format!("--epoch-tables {d}: {x}"))?.flatten() {
                if f.path().to_string_lossy().ends_with("-epoch-table.json") {
                    paths.push((f.path().display().to_string(), false));
                }
            }
        }
        let mut epoch_tables = vec![];
        for (p, named) in &paths {
            let read = ObjFile::load(p).and_then(|f| match OObj::decode(&f.redeem) {
                Ok(OObj::C(CObj::EpochTable(t))) => Ok((f, t)),
                _ => Err(format!("{p}: not an epoch table of a committee")),
            });
            match read {
                Ok(x) => epoch_tables.push(x),
                Err(x) if *named => return Err(x),
                Err(x) => println!("  (passed over among the epoch tables: {x})"),
            }
        }
        let dir = a.get("signed-prices").map(PathBuf::from);
        let asked = a.get("do").is_some_and(|d| d.split(',').any(|x| x.trim() == "watch"));
        if (asked || dir.is_some()) && (dir.is_none() || epoch_tables.is_empty()) {
            return Err("the watchtower reads the committee's signed prices: give --signed-prices DIR (where the members' signed prices arrive) and --epoch-table FILE or --epoch-tables DIR (an epoch table of the committee: its member table says which signatures are a quorum)".into());
        }
        if a.has("max-exposure") && epoch_tables.is_empty() {
            return Err("--max-exposure is measured against the committee's stake: give --epoch-table FILE or --epoch-tables DIR (an epoch table carries the sum of the members' weights)".into());
        }
        let kept = a.get("dir").map(|d| Path::new(d).join("prices-kept"));
        Ok((dir.is_some() || !epoch_tables.is_empty()).then_some(Watchtower { dir, kept, epoch_tables, told: Default::default() }))
    }

    /// Kept prices of signed prices long past (200,000 DAA: far beyond the time any claim of theirs can still be disputed
    /// and proved in) are removed: the folder does not grow for ever.
    fn prune(&self, now: i64) {
        if let Some(rd) = self.kept.as_ref().and_then(|k| std::fs::read_dir(k).ok()) {
            for f in rd.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                if name.strip_prefix("price-").and_then(|r| r.split('-').next()).and_then(|n| n.parse::<i64>().ok()).is_some_and(|t0| t0 < now - 200_000) {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
    }

    /// The stake of a committee: the sum of its members' weights in the newest epoch table given.
    fn stake(&self, committee: &[u8; 32]) -> Option<i64> {
        self.epoch_tables.iter().filter(|(f, _)| f.covenant_id.as_bytes() == *committee).max_by_key(|(_, t)| t.until).map(|(_, t)| t.total)
    }

    /// The signed price files (those that arrived and those kept) by signed price, the newest signed price first: a file is named by its
    /// signed price.
    fn by_price(&self) -> Vec<(i64, Vec<PathBuf>)> {
        let mut m: BTreeMap<i64, Vec<PathBuf>> = BTreeMap::new();
        for d in [&self.dir, &self.kept].into_iter().flatten() {
            if let Ok(rd) = std::fs::read_dir(d) {
                for f in rd.flatten() {
                    let name = f.file_name().to_string_lossy().into_owned();
                    if let Some(t0) = name.strip_prefix("price-").and_then(|r| r.split('-').next()).and_then(|n| n.parse::<i64>().ok()) {
                        m.entry(t0).or_default().push(f.path());
                    }
                }
            }
        }
        m.into_iter().rev().collect()
    }

    /// The signed prices of the time `t0` among `files`, each with its committee and market, its signature verified. Each
    /// is kept under a name of its own.
    fn raw(&self, t0: i64, files: &[PathBuf]) -> Vec<(SignedPrice, [u8; 32], [u8; 32])> {
        let mut v: Vec<(SignedPrice, [u8; 32], [u8; 32])> = vec![];
        for f in files {
            // (A signed price's file is a few hundred bytes: a larger one is not read.)
            if std::fs::metadata(f).map(|m| m.len() > 8_192).unwrap_or(true) {
                continue;
            }
            let Ok(t) = crate::ccmds::price_raw(&f.display().to_string()) else { continue };
            if t.0.t0 != t0 || !kpf_protocol::committee::price_verifies(&t.0, &t.1, &t.2) || v.contains(&t) {
                continue;
            }
            if let Some(k) = self.kept.as_ref().filter(|k| f.parent() != Some(k.as_path())) {
                // Named by all that is signed (review T1b, F30: with four bytes of the market's id in the name, a
                // member's price for another market took the name of its price for this one).
                let all = [&t.1[..], &t.2[..], &t.0.key[..], &t0.to_le_bytes()[..], &t.0.price.to_le_bytes()[..]].concat();
                let to = k.join(format!("price-{t0}-{}.json", to_hex(&kpf_protocol::committee::blake2b(&all))));
                if !to.exists() {
                    let _ = std::fs::create_dir_all(k);
                    let _ = std::fs::copy(f, &to);
                }
            }
            v.push(t);
        }
        v
    }

    /// The attestation of exactly `p` by the epoch table `t`: of each member its signed price nearest `p` within the spread;
    /// none if those members are no quorum.
    fn at_price(t: &EpochTable, committee: &[u8; 32], market: &[u8; 32], t0: i64, p: i64, by_member: &[(usize, SignedPrice)]) -> Option<Attestation> {
        let mut chosen: Vec<(usize, SignedPrice)> = vec![];
        for i in 0..kpf_protocol::committee::MEMBERS {
            if let Some((_, s)) = by_member.iter().filter(|(j, s)| *j == i && t.near(s.price, p)).min_by_key(|(_, s)| (s.price as i128 - p as i128).abs()) {
                chosen.push((i, *s));
            }
        }
        let a = Attestation::of(t, market, t0, p, &chosen);
        a.check(t, committee, t0).ok().map(|_| a)
    }

    /// The signed prices of one leg at the time `t0`, by member, under the epoch table that covers the signed price. A member that
    /// signed two prices for the time is said (once): the two are proof against it.
    fn members(&self, l: &kpf_protocol::cross::XLeg, t0: i64, raw: &[(SignedPrice, [u8; 32], [u8; 32])]) -> std::result::Result<(usize, Vec<(usize, SignedPrice)>), NoPrice> {
        let (k, t) = self.epoch_tables.iter().enumerate().find(|(_, (f, t))| f.covenant_id.as_bytes() == l.committee && t.covers(t0)).map(|(k, (_, t))| (k, t)).ok_or(NoPrice::EpochTable)?;
        let mut by_member: Vec<(usize, SignedPrice)> = vec![];
        for (i, member) in t.members.iter().enumerate().filter(|(_, s)| s.taken()) {
            for (s, r, m) in raw {
                if *r == l.committee && *m == l.market && s.key == member.key && s.price >= 0 && s.price <= kpf_protocol::committee::PRICE_LIMIT && !by_member.iter().any(|(j, o)| *j == i && o.price == s.price) {
                    by_member.push((i, *s));
                }
            }
            let of: Vec<i64> = by_member.iter().filter(|(j, _)| *j == i).map(|(_, s)| s.price).collect();
            if of.len() > 1 && self.told.borrow_mut().insert((member.key, t0)) {
                println!("  watchtower: the member {} signed {} prices for the time {t0} ({} KAS and {} KAS): two prices for one time are proof against it (member-forfeit); the files are kept in {}", &to_hex(&member.key)[..16], of.len(), fmt_kas(of[0] as i128), fmt_kas(of[1] as i128), self.kept.as_ref().map(|k| k.display().to_string()).unwrap_or_default());
            }
        }
        Ok((k, by_member))
    }

    /// One leg at a signed price: its signed prices and the prices a quorum stands within the spread of (each signed price
    /// and the two ends of the band it allows, as `Attestation::collect` tries them).
    fn leg(&self, l: &kpf_protocol::cross::XLeg, t0: i64, raw: &[(SignedPrice, [u8; 32], [u8; 32])]) -> std::result::Result<Signed, NoPrice> {
        let (k, by_member) = self.members(l, t0, raw)?;
        let t = &self.epoch_tables[k].1;
        let (b, sp) = (kpf_protocol::state::BPS as i128, t.spread_bps.clamp(0, kpf_protocol::state::BPS - 1) as i128);
        let mut cand: Vec<i64> = vec![];
        for (_, s) in &by_member {
            let x = s.price as i128;
            for c in [x, (x * b + b + sp - 1) / (b + sp), x * b / (b - sp)] {
                if c >= 0 && c <= kpf_protocol::committee::PRICE_LIMIT as i128 {
                    cand.push(c as i64);
                }
            }
        }
        cand.sort();
        cand.dedup();
        let near: Vec<i64> = cand.into_iter().filter(|p| Watchtower::at_price(t, &l.committee, &l.market, t0, *p, &by_member).is_some()).collect();
        if near.is_empty() {
            return Err(NoPrice::Quorum);
        }
        // A price a member signed, where a quorum stands near one: the ends of the band are for the case in which the
        // members' prices are a quorum only around a price between them.
        let signed: Vec<i64> = near.iter().copied().filter(|p| by_member.iter().any(|(_, s)| s.price == *p)).collect();
        // (And the prices midway between two of them, where a quorum stands near those too.)
        let mut all = near.clone();
        all.extend(near.windows(2).map(|w| w[0] + (w[1] - w[0]) / 2).filter(|p| Watchtower::at_price(t, &l.committee, &l.market, t0, *p, &by_member).is_some()));
        all.sort();
        all.dedup();
        let prices = if signed.is_empty() { near } else { signed };
        Ok(Signed { k, by_member, prices, all })
    }

    /// What the time `t0` gives the side `side` of the position: of the prices members signed that a quorum stands near,
    /// those that give it most (whoever claims chooses among them), the attestations of exactly those prices, and
    /// the claim.
    #[allow(clippy::type_complexity)]
    fn best(&self, x: &Position, side: i64, t0: i64, files: &[PathBuf]) -> std::result::Result<Option<([i64; 2], Vec<(usize, Attestation)>, kpf_protocol::thin::Claim)>, NoPrice> {
        let raw = self.raw(t0, files);
        let legs: Vec<&kpf_protocol::cross::XLeg> = x.x.legs.iter().filter(|l| l.size > 0).collect();
        let mut signed = vec![];
        for l in &legs {
            signed.push(self.leg(l, t0, &raw)?);
        }
        let ends = |s: &Signed| vec![s.prices[0], s.prices[s.prices.len() - 1]];
        let mut best: Option<([i64; 2], kpf_protocol::thin::Claim)> = None;
        for p1 in ends(&signed[0]) {
            for p2 in signed.get(1).map_or(vec![0], ends) {
                if let Some(cl) = x.true_claim(side, t0, [p1, p2]) {
                    if best.as_ref().is_none_or(|b| cl.amount > b.1.amount) {
                        best = Some(([p1, p2], cl));
                    }
                }
            }
        }
        Ok(best.and_then(|(prices, cl)| {
            let atts: Option<Vec<(usize, Attestation)>> = signed.iter().zip(&legs).enumerate().map(|(n, (s, l))| Watchtower::at_price(&self.epoch_tables[s.k].1, &l.committee, &l.market, t0, prices[n], &s.by_member).map(|a| (s.k, a))).collect();
            atts.map(|a| (prices, a, cl))
        }))
    }

    /// The newest signed price, not after `now` and not older than `late` (a claim lands within that of its time), for which
    /// a quorum signed a price of every market of the position, with what it gives the side. Every signed price of that time
    /// is looked at, however many no quorum signed (review T1b, F25); a signed price of a time to come says nothing of the
    /// present. Also: how many signed prices of that time are in the folder, and how many of them no epoch table given covers.
    #[allow(clippy::type_complexity)]
    fn last(&self, x: &Position, side: i64, now: i64, late: i64) -> (Option<(i64, [i64; 2], kpf_protocol::thin::Claim)>, usize, usize) {
        let (mut there, mut uncovered) = (0, 0);
        for (t0, files) in self.by_price().into_iter().filter(|(t0, _)| *t0 <= now && *t0 >= now - late) {
            there += 1;
            match self.best(x, side, t0, &files) {
                Ok(Some((prices, _, cl))) => return (Some((t0, prices, cl)), there, uncovered),
                Err(NoPrice::EpochTable) => uncovered += 1,
                _ => {}
            }
        }
        (None, there, uncovered)
    }

    /// The attestations of exactly the prices a claim of the time `t0` states, from every signed price of the time
    /// this machine has (review T1b, F26: not the price the files in the folder would be collected at now).
    /// The committee's answer for the time `t0`, from the signed prices kept here: the attestation of a price for
    /// each leg, among the prices a quorum stands near (the members' prices differ within the spread, and any price
    /// near enough to a quorum of them is the committee's, the ends of that band too), that `decides` the way asked. `Ok(None)`: a quorum signed, and no price of theirs
    /// decides that way. `Err`: no quorum's prices of that time are here.
    fn answer(&self, x: &Position, t0: i64, decides: &dyn Fn([i64; 2]) -> bool) -> std::result::Result<Option<Vec<(usize, Attestation)>>, String> {
        let files = self.by_price().into_iter().find(|(t, _)| *t == t0).map(|(_, f)| f).unwrap_or_default();
        let raw = self.raw(t0, &files);
        let legs: Vec<&kpf_protocol::cross::XLeg> = x.x.legs.iter().filter(|l| l.size > 0).collect();
        let mut signed = vec![];
        for l in &legs {
            signed.push(self.leg(l, t0, &raw).map_err(|why| match why {
                NoPrice::EpochTable => format!("no epoch table given covers its time {t0}"),
                NoPrice::Quorum => format!("the signed prices of its time {t0} this machine has are no quorum"),
            })?);
        }
        for p1 in &signed[0].all {
            for p2 in signed.get(1).map_or(vec![0], |s| s.all.clone()) {
                let prices = [*p1, p2];
                if !decides(prices) {
                    continue;
                }
                let atts: Option<Vec<(usize, Attestation)>> = signed.iter().zip(&legs).enumerate().map(|(n, (s, l))| Watchtower::at_price(&self.epoch_tables[s.k].1, &l.committee, &l.market, t0, prices[n], &s.by_member).map(|a| (s.k, a))).collect();
                if atts.is_some() {
                    return Ok(atts);
                }
            }
        }
        Ok(None)
    }
    /// The epoch tables on the chain that the attestations of an answer are shown with.
    fn tables(&self, node: &mut N, net: Network, atts: &[(usize, Attestation)]) -> R<Vec<OLive<EpochTable>>> {
        let mut epoch_tables: Vec<OLive<EpochTable>> = vec![];
        for (k, _) in atts {
            let taken: Vec<TransactionOutpoint> = epoch_tables.iter().map(|t| t.op).collect();
            epoch_tables.push(self.live(node, net, *k, &taken)?);
        }
        Ok(epoch_tables)
    }
    fn live(&self, node: &mut N, net: Network, k: usize, not: &[TransactionOutpoint]) -> R<OLive<EpochTable>> {
        let (f, t) = &self.epoch_tables[k];
        let Some(n) = node.as_mut() else {
            return Ok(OLive { s: *t, value: f.amount, id: Some(f.covenant_id), op: f.op, daa: f.daa.unwrap_or(0) });
        };
        let u = n.utxos(&[p2sh_address(&f.redeem, net)])?.into_iter().find(|u| u.covenant_id == Some(f.covenant_id) && !not.contains(&u.op)).ok_or("no epoch table of that price's epoch is at the node this moment (each is in a transaction not yet in a block, or they were retired)")?;
        Ok(OLive { s: *t, value: u.amount, id: Some(f.covenant_id), op: u.op, daa: u.daa })
    }
}

/// What a run of the loop is told once.
struct Cfg<'a> {
    a: &'a Args,
    net: Network,
    f: &'a crate::cmds::Funder,
    wanted: Vec<&'a str>,
    max_tx: usize,
    guard: i64,
    min_gain: i64,
    silence: Option<i64>,
    watch_gain: i64,
    max_exposure: Option<f64>,
    /// A round's length in DAA, and the margin a claim is judged with by this executor's own price (basis points).
    every_daa: i64,
    margin_bps: Option<i64>,
    /// `take`: the most of its own margin this executor has in positions and orders of its own, and how far from its
    /// own price an order must be for it to answer (basis points).
    take_max: Option<i64>,
    take_spread_bps: i64,
    live: bool,
    me: String,
}

/// What the loop keeps from round to round.
struct Loop {
    /// Where the chain was at the last two rounds in which nothing moved (a reading starts at the older).
    marks: Vec<Cursor>,
    /// Where the library's copies and the registry's anchors stood when the folder was last brought to the present.
    stood: Option<HashSet<TransactionOutpoint>>,
    waiting: Vec<Sent>,
    avoid: HashMap<TransactionOutpoint, i64>,
    /// Sets of objects whose transaction would not have left the sender its gain.
    alone: HashSet<(&'static str, Vec<TransactionOutpoint>)>,
    /// What was last said of the margins under the committees (said again when it changes).
    under: String,
    /// The members and signed prices whose two prices the watchtower already named.
    told: HashSet<([u8; 32], i64)>,
    round: i64,
}

/// One round.
fn turn(st: &mut Loop, c: &Cfg, node: &mut N, registry: &mut Registry) -> R<()> {
    st.round += 1;
    let (a, net, f, guard, max_tx, min_gain, live, round) = (c.a, c.net, c.f, c.guard, c.max_tx, c.min_gain, c.live, st.round);
    let wanted = &c.wanted;
    registry.reload();
    // What a round asks: the present, and one list (the place first: the list is then never older than it).
    let (now, mut seen) = if live {
        let (daa, mark) = present(node)?;
        let mut seen = look(node, registry, std::slice::from_ref(&c.me))?;
        if st.marks.is_empty() {
            st.marks.push(mark.clone());
        }
        // Did the registry move by another hand? What this machine sent is expected: a copy it spent stands where it
        // wrote it.
        let mut expect = st.stood.clone().unwrap_or_else(|| seen.stand.clone());
        let mut still = vec![];
        for w in st.waiting.drain(..) {
            if seen.stand.contains(&w.new) {
                // It landed: what it spent is spent.
                expect.remove(&w.old);
                expect.insert(w.new);
                for o in &w.objs {
                    registry.retire(o);
                }
            } else if seen.stand.contains(&w.old) && round <= w.until {
                still.push(w);
            }
            // Else the copy went to another transaction, or this one never landed: the comparison below sees a
            // copy that moved, and the chain is read.
        }
        st.waiting = still;
        if expect != seen.stand {
            let mut r = Reading::default();
            follow(node, registry, &st.marks[0], &expect, &seen.stand, &mut r)?;
            for l in &r.lines {
                println!("{l}");
            }
            println!("round {round}, DAA {daa}: the registry moved: {}", r.say());
            // What the reading wrote is looked up in this same round: a claim is disputed within its dispute period.
            st.stood = Some(std::mem::take(&mut seen.stand));
            seen = look(node, registry, std::slice::from_ref(&c.me))?;
            st.waiting.clear();
            st.marks = vec![mark];
        } else {
            st.stood = Some(expect);
            st.marks.push(mark);
            if st.marks.len() > 2 {
                st.marks.remove(0);
            }
        }
        // Nothing unread is older than the older mark: a reading after a restart starts there.
        place_write(&registry.dir, &Place { cursor: st.marks[0].clone(), stood: st.stood.clone().unwrap_or_default() })?;
        (daa, seen)
    } else {
        // Without a node: the folder as it is, the coins of the funding file, and one copy of each script taken as
        // standing (a trial builds on it).
        let mut l: Vec<ListedUtxo> = registry.items.iter().map(|i| ListedUtxo { op: i.f.op, amount: i.f.amount, daa: i.f.daa.unwrap_or(0), spk: Some(i.f.spk()), covenant_id: Some(i.f.covenant_id) }).collect();
        l.extend(parse_utxos(&read_json(a.need("funding").map_err(|_| "without --node give --funding FILE (the sender's coins) and --daa N".to_string())?)?)?);
        let mut seen = sort_out(registry, l);
        let lib_id = kaspa_consensus_core::Hash::from_bytes(registry.lib);
        for n in SCRIPTS {
            let code = pinned::kit().script(n).expect("a script");
            let op = TransactionOutpoint { transaction_id: kaspa_consensus_core::Hash::from_bytes(kpf_protocol::committee::blake2b(code)), index: 1 };
            seen.copies.entry(n).or_default().push(OLive { s: n, value: tb::SCRIPT_VALUE_MIN, id: Some(lib_id), op, daa: 0 });
        }
        (crate::ocmds::now(a, node)?, seen)
    };
    let registry_id = kaspa_consensus_core::Hash::from_bytes(registry.id);
    let (list, copies) = (&seen.list, std::mem::take(&mut seen.copies));
    let (avoid, waiting) = (&mut st.avoid, &mut st.waiting);
    avoid.retain(|_, until| *until > round);

        // What is live of the folder, by what the node lists at each object's own script.
        let listed: HashMap<TransactionOutpoint, &ListedUtxo> = list.iter().map(|u| (u.op, u)).collect();
        // A file whose object the node has not listed for ten minutes is of nothing live (a transaction that never
        // landed, or one spent while the chain was not read): out of the way, in `gone/`.
        if live {
            for i in &registry.items {
                let old = std::fs::metadata(&i.path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() > 600);
                if old && !listed.contains_key(&i.f.op) {
                    if let Some(name) = i.path.file_name() {
                        let _ = std::fs::rename(&i.path, registry.dir.join("gone").join(name));
                    }
                }
            }
        }
        let alive: Vec<(&Item, &ListedUtxo)> = registry
            .items
            .iter()
            .filter_map(|i| listed.get(&i.f.op).map(|u| (i, *u)))
            .filter(|(i, u)| u.covenant_id == Some(registry_id) && u.spk.as_ref() == Some(&i.f.spk()) && (u.amount as i64) >= i.obj.least_value() && !avoid.contains_key(&i.f.op))
            .collect();
        // The sender's plain coins: what the node lists at its key under no covenant id (a funding file's: as listed).
        let mut coins = funding_utxos(list, &f.key);
        coins.retain(|c| !registry.items.iter().any(|i| i.f.op == c.outpoint));
        coins.sort_by_key(|c| std::cmp::Reverse(c.amount));
        let mut r = Round { a, net, lib: registry.lib, keys: &f.keys, fk: f.key, rate: rate(a)?, coins, copies, used: HashSet::new(), left: max_tx, did: 0, min_gain, sent: vec![], refused: vec![], alone: std::mem::take(&mut st.alone) };
        let does = |x: &str| wanted.contains(&x);
        let lib = registry.lib;
        // The epoch tables given, read again: a committee rolls while the loop runs (review T1b, F28).
        let watch = Watchtower::open(a)?;
        if let Some(g) = &watch {
            *g.told.borrow_mut() = std::mem::take(&mut st.told);
            g.prune(now as i64);
        }
        // What stands under each committee, in this folder's sight: the margins of the positions of its markets, beside
        // the committee's stake (the sum of its members' weights in the newest epoch table given).
        let mut under: BTreeMap<[u8; 32], (usize, i64)> = BTreeMap::new();
        for (i, _) in &alive {
            if let Some(x) = position_in(&i.obj) {
                let mut of: Vec<[u8; 32]> = x.x.legs.iter().filter(|l| l.size > 0).map(|l| l.committee).collect();
                of.dedup();
                for r in of {
                    let e = under.entry(r).or_default();
                    e.0 += 1;
                    e.1 += x.x.a_margin + x.x.b_margin;
                }
            }
        }
        let stake_of = |r: &[u8; 32]| watch.as_ref().and_then(|g| g.stake(r));
        let said: Vec<String> = under
            .iter()
            .map(|(r, (n, m))| match stake_of(r) {
                Some(s) => format!("  under the committee {}: {n} positions, {} KAS of margin in this folder's sight; the committee's stake is {} KAS ({:.2} of it)", &to_hex(r)[..8], fmt_kas(*m as i128), fmt_kas(s as i128), *m as f64 / s.max(1) as f64),
                None => format!("  under the committee {}: {n} positions, {} KAS of margin in this folder's sight (its stake is not known here: give --epoch-table)", &to_hex(r)[..8], fmt_kas(*m as i128)),
            })
            .collect();
        if said.join("\n") != st.under {
            st.under = said.join("\n");
            for l in &said {
                println!("{l}");
            }
        }
        // Whether `more` margin under the committee would take it above what this executor allows.
        // (With what this round already matched: review T1b, F29.)
        let mut added: BTreeMap<[u8; 32], i64> = BTreeMap::new();
        let above = |added: &BTreeMap<[u8; 32], i64>, r: &[u8; 32], more: i64| match (c.max_exposure, stake_of(r)) {
            (Some(max), Some(s)) => (under.get(r).map_or(0, |x| x.1) + added.get(r).copied().unwrap_or(0) + more) as f64 > max * s as f64,
            _ => false,
        };

        // Orders that cross, the best of each side first.
        if does("match") {
            let orders: Vec<(&Item, &ListedUtxo, Order)> = alive.iter().filter_map(|(i, u)| if let TObj::Order(o) = i.obj { Some((*i, *u, o)) } else { None }).filter(|(_, _, o)| (now as i64) + guard < o.expiry).collect();
            let mut longs: Vec<_> = orders.iter().filter(|x| x.2.dir == LONG).collect();
            let mut shorts: Vec<_> = orders.iter().filter(|x| x.2.dir == SHORT).collect();
            longs.sort_by_key(|x| (std::cmp::Reverse(x.2.limit), x.1.daa));
            shorts.sort_by_key(|x| (x.2.limit, x.1.daa));
            let held: Vec<(&Item, &ListedUtxo, Position)> = alive.iter().filter_map(|(i, u)| if let TObj::Position(x) = i.obj { Some((*i, *u, x)) } else { None }).collect();
            for (li, lu, lo) in longs {
                let Some(tpl) = registry.template(&lo.tpl) else { continue };
                for (si, su, so) in &shorts {
                    if so.tpl != lo.tpl || lo.limit < so.limit || lo.owner == so.owner || !r.free(&[li.f.op, si.f.op]) {
                        continue;
                    }
                    // A match into a position: the owner of one of the two orders is the other side of a position
                    // already (a short order of its long, or a long order of its short). It leaves by its order and
                    // the other order's owner takes its place, in one transaction (KpfTReplace) where a match and a
                    // novation were two.
                    let px = match_price(lo, lu.daa, so, su.daa);
                    let extra = (lu.amount as i64 - lo.value()).max(0) + (su.amount as i64 - so.value()).max(0);
                    let mut into = false;
                    for (xi, xu, xp) in &held {
                        let side = if xp.x.a_key == so.owner {
                            SIDE_A
                        } else if xp.x.b_key == lo.owner {
                            SIDE_B
                        } else {
                            continue;
                        };
                        if Template::of(xp).hash() != lo.tpl || !r.free(&[xi.f.op, li.f.op, si.f.op]) {
                            continue;
                        }
                        let n = lo.size.min(so.size).min(xp.x.legs[0].size);
                        match replaced(xp, xu.amount as i64, lo, lu.amount as i64, so, su.amount as i64, n, px, side, true) {
                            Ok(rp) if rp.tips + extra > min_gain => {}
                            _ => continue,
                        }
                        let (lx, l, s) = (at(xi, xu, *xp), at(li, lu, *lo), at(si, su, *so));
                        let title = format!("match {} units at {} into the position {} ({} with {}): its side {} leaves", crate::args::fmt_units(n as i128, tpl.scale as i128), fmt_kas(px as i128), &xi.f.op.transaction_id.to_string()[..8], &li.f.op.transaction_id.to_string()[..8], &si.f.op.transaction_id.to_string()[..8], if side == SIDE_A { "a" } else { "b" });
                        into = r.send(node, &title, "KpfTReplace", &[xi.f.op, li.f.op, si.f.op], true, &|c, p| {
                            let (b, rp) = tb::replace(&lib, &lx, &l, &s, c, n, px, side, p).map_err(e)?;
                            // The objects under the registry's id are outputs 0, 1, 2 and 4 in their order.
                            let mut made = vec![];
                            let mut k = 0usize;
                            let mut next = || {
                                k += 1;
                                if k <= 3 { k - 1 } else { 4 }
                            };
                            for (o, _) in [rp.rest, rp.z].into_iter().flatten() {
                                made.push(Made::Obj(next(), TObj::Position(o)));
                            }
                            for o in [rp.long, rp.short] {
                                if let Rest::Order(o, _) = o {
                                    made.push(Made::OrderOf(next(), o, tpl));
                                }
                            }
                            Ok((b, made))
                        });
                        if into {
                            break;
                        }
                    }
                    if into {
                        break;
                    }
                    if above(&added, &tpl.committee, lo.margin + so.margin) {
                        println!("  orders {} and {} cross and are left unmatched: their margins would stand above --max-exposure of the committee's stake", &li.f.op.transaction_id.to_string()[..8], &si.f.op.transaction_id.to_string()[..8]);
                        continue;
                    }
                    let (n, x) = (lo.size.min(so.size), match_price(lo, lu.daa, so, su.daa));
                    // A match leaves its sender the two tips' shares less the fee: one whose tips are below the gain
                    // asked is not built (review T1b, F21: orders without a tip cost every round a transaction each).
                    match match_orders(&tpl, lo, lu.amount as i64, so, su.amount as i64, n, x) {
                        // (An order that is used up leaves its sender what it held beyond its margin and tip, too.)
                        Ok(m) if m.tips + (lu.amount as i64 - lo.value()).max(0) + (su.amount as i64 - so.value()).max(0) > min_gain => {}
                        _ => continue,
                    }
                    let (l, s) = (at(li, lu, *lo), at(si, su, *so));
                    let title = format!("match {} units at {} ({} with {})", crate::args::fmt_units(n as i128, tpl.scale as i128), fmt_kas(x as i128), &li.f.op.transaction_id.to_string()[..8], &si.f.op.transaction_id.to_string()[..8]);
                    let sent = r.send(node, &title, "KpfTMatch", &[li.f.op, si.f.op], true, &|c, p| {
                        let (b, m) = tb::order_match(&lib, &tpl, &l, &s, c, n, x, p).map_err(e)?;
                        let mut made = vec![Made::Obj(0, TObj::Position(m.position))];
                        let long_rests = matches!(m.long, Rest::Order(..));
                        if let Rest::Order(o, _) = m.long {
                            made.push(Made::OrderOf(1, o, tpl));
                        }
                        if let Rest::Order(o, _) = m.short {
                            made.push(Made::OrderOf(if long_rests { 3 } else { 1 }, o, tpl));
                        }
                        Ok((b, made))
                    });
                    if sent {
                        *added.entry(tpl.committee).or_default() += lo.margin + so.margin;
                        break;
                    }
                }
            }
        }
        // Positions: a user out of two that cancel; two of the same two users into one.
        let positions: Vec<(&Item, &ListedUtxo, Position)> = alive.iter().filter_map(|(i, u)| if let TObj::Position(x) = i.obj { Some((*i, *u, x)) } else { None }).collect();
        if does("novate") {
            for (xi, xu, x) in &positions {
                for (yi, yu, y) in &positions {
                    if xi.f.op == yi.f.op || !r.free(&[xi.f.op, yi.f.op]) || novated(x, xu.amount as i64, y, yu.amount as i64, true).is_err() {
                        continue;
                    }
                    let (lx, ly) = (at(xi, xu, *x), at(yi, yu, *y));
                    let title = format!("novate ({} and {})", &xi.f.op.transaction_id.to_string()[..8], &yi.f.op.transaction_id.to_string()[..8]);
                    r.send(node, &title, "KpfTNovate", &[xi.f.op, yi.f.op], true, &|c, p| {
                        let (b, _, made) = tb::novate(&lib, &lx, &ly, c, p).map_err(e)?;
                        Ok((b, made.iter().map(|(i, position)| Made::Obj(*i, TObj::Position(*position))).collect()))
                    });
                }
            }
        }
        if does("merge") {
            for (k, (xi, xu, x)) in positions.iter().enumerate() {
                for (yi, yu, y) in positions.iter().skip(k + 1) {
                    if !r.free(&[xi.f.op, yi.f.op]) {
                        continue;
                    }
                    let (lx, ly) = (at(xi, xu, *x), at(yi, yu, *y));
                    let pair = format!("({} and {})", &xi.f.op.transaction_id.to_string()[..8], &yi.f.op.transaction_id.to_string()[..8]);
                    if let Ok(out) = merged(x, y) {
                        r.send(node, &format!("merge {pair}"), "KpfTMerge", &[xi.f.op, yi.f.op], true, &|c, p| tb::merge(&lib, &lx, &ly, c, p).map(|(b, _)| (b, vec![Made::Obj(0, TObj::Position(out))])).map_err(e));
                    } else if let Ok(out) = folded(x, y) {
                        r.send(node, &format!("fold {pair}"), "KpfTFold", &[xi.f.op, yi.f.op], true, &|c, p| tb::fold(&lib, &lx, &ly, c, p).map(|(b, _)| (b, vec![Made::Obj(0, TObj::Position(out))])).map_err(e));
                    } else if let Ok(out) = folded(y, x) {
                        r.send(node, &format!("fold {pair}"), "KpfTFold", &[xi.f.op, yi.f.op], true, &|c, p| tb::fold(&lib, &ly, &lx, c, p).map(|(b, _)| (b, vec![Made::Obj(0, TObj::Position(out))])).map_err(e));
                    }
                }
            }
        }
        // This executor's own price, read every round and kept with the round's time in the folder (`own-prices`): what
        // it judges a claim by where the committee's signed prices of the claim's time are not at hand. It is the
        // price of one market, the one `--market1` names (review T1b, F35: one price judged the claims of every
        // market), and a claim is judged by the readings around its own time (F36), never by the price of now.
        let own_file = registry.dir.join("own-prices");
        let own_market: Option<([u8; 32], [u8; 32])> = if ["price", "price-file", "price-cmd"].iter().any(|k| c.a.has(k)) {
            match c.a.all("market1").as_slice() {
                [m] => crate::ccmds::market_parts(m).ok().map(|(_, market, committee, _, _)| (market, committee)),
                _ => {
                    println!("  this executor's own price is the price of one market: give exactly one --market1 with it. No claim is disputed by it");
                    None
                }
            }
        } else {
            None
        };
        let own_tag = own_market.map(|(m, _)| to_hex(&m)[..16].to_string()).unwrap_or_default();
        if own_market.is_some() {
            // A price file nobody rewrote for three rounds says nothing of now.
            let stale = c.a.get("price-file").and_then(|f| std::fs::metadata(f).ok()).and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|e| c.live && e.as_secs() as i64 * 10 > 3 * c.every_daa);
            match crate::ccmds::feed_prices(c.a) {
                Ok((p, lo, hi)) if lo > 0 && !stale => {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&own_file) {
                        let _ = writeln!(f, "{own_tag} {now} {p} {lo} {hi}");
                    }
                }
                Ok(_) => println!("  no price of this executor's own this round (the price is 0, or its file was not rewritten for three rounds)"),
                Err(x) => println!("  no price of this executor's own this round ({x})"),
            }
        }
        // A reading: its time, the price then, and the least and the most since the reading before it (the price
        // itself where the source says no more). Each line names its market, and a line of another market is not
        // read (review T1b, F40: a folder whose `--market1` was changed judged the new market by the old one's price).
        let own_all: Vec<String> = if own_market.is_some() { std::fs::read_to_string(&own_file).unwrap_or_default().lines().map(str::to_owned).collect() } else { vec![] };
        let own_read: Vec<(i64, i64, i64, i64)> = own_all
            .iter()
            .filter_map(|l| {
                let w: Vec<&str> = l.split(' ').collect();
                if w.len() != 5 || w[0] != own_tag {
                    return None;
                }
                Some((w[1].parse().ok()?, w[2].parse().ok()?, w[3].parse().ok()?, w[4].parse().ok()?))
            })
            .filter(|(_, p, lo, hi)| *lo > 0 && lo <= p && p <= hi)
            .collect();
        // (The file does not grow for ever: the newest thousand readings are kept.)
        if own_all.len() > 2_000 {
            let kept: String = own_all[own_all.len() - 1_000..].iter().map(|l| format!("{l}\n")).collect();
            let _ = std::fs::write(&own_file, kept);
        }
        // Taking what is left (`--do take`, with `--take-max`): where an order of somebody else rests at a price better
        // for its counterparty than this executor's own price of now by the spread (`--take-spread-bps`), the
        // executor places the order the other way at that price, for what the limit on its own margin leaves room for.
        // The next round matches the two as any two orders: into a position this executor is the other side of
        // already (KpfTReplace: it leaves, and the order's owner takes its place), or into a new one between them. So
        // what it took from one user it passes on to the next that comes the other way; and where its order lets it
        // out of what it holds, it asks no spread. One order a round. It is this executor's own choice and its own
        // money: no script knows of it, and another executor leaves the same order alone or answers it first.
        if let (true, Some(max), Some((market, committee))) = (does("take"), c.take_max, own_market) {
            let fk = f.key;
            let part = |v: i64, n: i64, size: i64| (v / size) * n + ((v % size) * n) / size;
            // This round's reading: the price, and the least and the most since the reading before (review T1b, F44:
            // an order is the spread better than its own price only if it is so against what the source saw since).
            let fair = own_read.last().filter(|r| r.0 == now as i64).map(|r| (r.1, r.2, r.3));
            let own_orders: Vec<Order> = alive.iter().filter_map(|(i, _)| if let TObj::Order(o) = i.obj { Some(o) } else { None }).filter(|o| o.owner == fk).collect();
            // Its own margin: in its orders, and in every position it is a party of, claimed or not, with the bonds
            // it posted there (review T1b, F42: a position under a claim was not counted). And what it holds that an
            // order can let it out of: a position that is not claimed, by the order's own template (F43: a position
            // of other terms is not left by this order; the match would make a second one).
            let mut held = own_orders.iter().map(|o| o.margin).sum::<i64>();
            let mut units: HashMap<[u8; 32], (i64, i64)> = HashMap::new();
            for (i, _) in &alive {
                let (x, plain, bond) = match i.obj {
                    TObj::Position(x) => (x, true, 0),
                    TObj::Claim(k) => (k.position, false, if k.position.key_of(k.side) == Some(fk) { k.bond } else { 0 }),
                    TObj::Disputed(d) => (d.claim.position, false, if d.claim.position.key_of(d.claim.side) == Some(fk) { d.claim.bond } else { 0 } + if d.disputer == fk { d.claim.bond } else { 0 }),
                    TObj::Liq(q) => (q.liq.position, false, if q.liq.liquidator == fk { q.liq.bond } else { 0 } + if q.state == LiqState::Disputed(fk) { q.liq.bond } else { 0 }),
                    TObj::ExitClaim(q) => (q.position, false, if q.position.key_of(q.side) == Some(fk) { q.bond } else { 0 }),
                    TObj::Change(k) => (k.position, false, if k.claimer == fk { k.bond } else { 0 } + if k.disputer == Some(fk) { k.bond } else { 0 }),
                    _ => continue,
                };
                held += bond;
                let (long, short) = (x.x.a_key == fk, x.x.b_key == fk);
                held += if long { x.x.a_margin } else { 0 } + if short { x.x.b_margin } else { 0 };
                if plain && x.x.legs[1].size == 0 && (long != short) {
                    let e = units.entry(Template::of(&x).hash()).or_default();
                    if long {
                        e.0 += x.x.legs[0].size;
                    } else {
                        e.1 += x.x.legs[0].size;
                    }
                }
            }
            // The templates this executor was started with (`--market1` with `--dispute-daa`, `--bond-bps`,
            // `--exit-daa`): it puts its own margin only into positions of terms its operator chose (review T1b, F49:
            // the folder learns the template of every match the chain shows, and an order of a template with a
            // dispute period or an exit time of one DAA was answered like any other).
            let started: Vec<[u8; 32]> = c
                .a
                .all("market1")
                .iter()
                .filter_map(|m| crate::ccmds::market_parts(m).ok())
                .filter_map(|(t, market, committee, scale, price_max)| Some(Template { t, market, committee, scale, price_max, exit: int_or(c.a, "exit-daa", 864_000).ok()?, dispute_daa: int_or(c.a, "dispute-daa", 600).ok()?, bond_bps: int_or(c.a, "bond-bps", 1_000).ok()? }.hash()))
                .collect();
            let mut placed = false;
            if let Some((_, lo, hi)) = fair {
                let bps = kpf_protocol::state::BPS;
                let (above, below) = (hi + (hi / bps) * c.take_spread_bps + 1, lo - (lo / bps) * c.take_spread_bps - 1);
                for (i, _) in &alive {
                    let TObj::Order(o) = i.obj else { continue };
                    let Some(tpl) = registry.template(&o.tpl) else { continue };
                    if o.owner == fk || !started.contains(&o.tpl) || tpl.market != market || tpl.committee != committee || (now as i64) + guard + 3 * c.every_daa >= o.expiry || !r.free(&[i.f.op]) {
                        continue;
                    }
                    // The order the other way; one of its own a direction at a time.
                    let dir = if o.dir == LONG { SHORT } else { LONG };
                    if own_orders.iter().any(|m| m.dir == dir) {
                        continue;
                    }
                    // What it holds the order would let it out of: a long order takes its place in what it is long in.
                    let (long_units, short_units) = units.get(&o.tpl).copied().unwrap_or((0, 0));
                    let out_of = if o.dir == LONG { long_units } else { short_units };
                    let good = match (o.dir == LONG, out_of > 0) {
                        (true, true) => o.limit >= hi,
                        (true, false) => o.limit >= above,
                        (false, true) => o.limit <= lo,
                        (false, false) => o.limit <= below,
                    };
                    // An order that asks of its counterparty more than it posts itself would set this executor's
                    // margin, up to its cap, against a small one: it is left.
                    if o.min_counter > o.margin {
                        continue;
                    }
                    if !good {
                        continue;
                    }
                    let full = o.margin.max(o.min_counter).max(1);
                    let n = if out_of > 0 {
                        o.size.min(out_of)
                    } else if full <= max - held {
                        o.size
                    } else {
                        ((o.size as i128) * (max - held).max(0) as i128 / full as i128) as i64
                    };
                    if n <= 0 || (n < o.min_fill && n != o.size) {
                        if out_of == 0 {
                            println!("  take: the order {} is left: this executor's own margin stands at {} KAS of the {} KAS of --take-max", &i.f.op.transaction_id.to_string()[..8], fmt_kas(held as i128), fmt_kas(max as i128));
                        }
                        continue;
                    }
                    // A match must leave whoever sends it the tips' share: an order without one is matched by nobody.
                    if part(o.tip, n, o.size) <= min_gain {
                        continue;
                    }
                    let margin = part(o.margin, n, o.size).max(part(o.min_counter, n, o.size)).max(1);
                    let mine = Order { tpl: o.tpl, owner: fk, dir, size: n, limit: o.limit, margin, tip: 0, expiry: now as i64 + guard + 3 * c.every_daa, min_fill: n, min_counter: part(o.margin, n, o.size) };
                    if mine.check().is_err() {
                        continue;
                    }
                    let title = format!("take: an order of its own, {} {} units at {} with {} KAS of margin, against the order {} ({})", if dir == LONG { "long" } else { "short" }, crate::args::fmt_units(n as i128, tpl.scale as i128), fmt_kas(o.limit as i128), fmt_kas(margin as i128), &i.f.op.transaction_id.to_string()[..8], if out_of > 0 { "it lets this executor out of what it holds" } else { "the price is the spread better than its own" });
                    match crate::tcmds::anchor_of(c.a, &lib, node, net) {
                        Ok(anchor) => {
                            placed = r.send(node, &title, "KpfTPlace", &[anchor.op], false, &|s, pay| tb::order_place(&lib, &anchor, s, &mine, pay).map(|b| (b, vec![Made::OrderOf(0, mine, tpl), Made::Obj(2, TObj::Anchor)])).map_err(e));
                        }
                        Err(x) => println!("  take: no anchor to place an order through ({x})"),
                    }
                    if placed {
                        break;
                    }
                }
            } else {
                println!("  take: no price of this executor's own this round: it answers no order");
            }
        }
        // A member answers (`--do answer`, with `--member-key`): for every disputed claim, disputed liquidation and
        // exit claim of its market it signs, once, its own price of the time asked for, into the folder the signed prices are read
        // from (`--signed-prices`). Nobody is sent a price before a dispute; a dispute is what asks. The price is this
        // machine's reading nearest the claim's time, no further from it than a round: a member that did not watch
        // that time signs nothing (a price it did not see is one a charge can call false). The key's journal refuses
        // a second price for a time it signed.
        if does("answer") {
            match (c.a.get("member-key"), own_market, watch.as_ref().and_then(|g| g.dir.clone())) {
                (Some(kpath), Some((market, committee)), Some(dir)) => {
                    // The time a price is asked for: a disputed claim's own; for an exit claim (which a price signed at
                    // or after its `since` ends: review T1b, F46, nothing answered one) the first multiple of 300 DAA
                    // at or after the claim landed, so that members that never talk sign for one time.
                    // (An exit claim is asked every such multiple up to now, the newest ten of them, for as long as it
                    // stands: review T1b, F47, a member that could not sign the first was asked no other, and the
                    // claimer chooses where the first falls. The last of a claim's times is the one a refusal speaks of.)
                    let asked: Vec<(i64, String, bool)> = alive
                        .iter()
                        .flat_map(|(i, u)| {
                            let id8 = i.f.op.transaction_id.to_string()[..8].to_string();
                            let (x, times) = match i.obj {
                                TObj::Disputed(d) => (d.claim.position, vec![d.claim.t0]),
                                // A disputed take: its answer is a note of the committee's price for its time.
                                TObj::Change(k) if k.disputer.is_some() => (k.position, vec![k.t0]),
                                TObj::Liq(q) if matches!(q.state, LiqState::Disputed(_)) => (q.liq.position, vec![q.liq.t0]),
                                TObj::ExitClaim(q) => {
                                    // (Once the folder holds a quorum's prices of a time at or after `since`, the claim is
                                    // ended by them and no further time is signed: review T1b, F50.)
                                    if watch.as_ref().is_some_and(|g| g.by_price().into_iter().any(|(t, _)| t >= q.since && matches!(g.answer(&q.position, t, &|_| true), Ok(Some(_))))) {
                                        return vec![];
                                    }
                                    let first = ((u.daa as i64).max(q.since) + 299) / 300 * 300;
                                    let last = (now as i64) / 300 * 300;
                                    (q.position, (0..10).map(|k| last - 300 * k).filter(|t| *t >= first).rev().collect())
                                }
                                _ => return vec![],
                            };
                            if !x.x.legs.iter().any(|l| l.size > 0 && l.market == market && l.committee == committee) {
                                return vec![];
                            }
                            let n = times.len();
                            times.into_iter().enumerate().map(|(k, t)| (t, id8.clone(), k + 1 == n)).collect()
                        })
                        .collect();
                    // The committee's spread: what the members' prices may differ by and still be one price.
                    let spread = watch.as_ref().and_then(|g| g.epoch_tables.iter().find(|(f, _)| f.covenant_id.as_bytes() == committee).map(|(_, t)| t.spread_bps));
                    if !asked.is_empty() {
                        let signer = crate::keys::load(Path::new(kpath)).and_then(|k| secp256k1::Keypair::from_seckey_slice(&secp256k1::Secp256k1::new(), &k.secret).map_err(e));
                        let cm = c.a.all("market1").first().ok_or("no --market1".to_string()).and_then(|m| crate::ccmds::cmarket_from(m)).map(|x| x.0);
                        match (signer, cm) {
                            (Ok(kp), Ok(cm)) => {
                                let key8 = to_hex(&kp.x_only_public_key().0.serialize())[..8].to_string();
                                for (t0, id8, say) in asked {
                                    let out = dir.join(format!("price-{t0}-{key8}.json"));
                                    if out.exists() {
                                        continue;
                                    }
                                    // A reading each side of the time, no further than a round from it; and what
                                    // they say of the time between (their prices, and the lows and highs since)
                                    // no further apart than the committee's spread (review T1b, F45: the nearest
                                    // reading was signed for a time the source itself said the price moved at).
                                    let before = own_read.iter().filter(|r| r.0 <= t0 && t0 - r.0 <= c.every_daa).max_by_key(|r| r.0);
                                    let after = own_read.iter().filter(|r| r.0 >= t0 && r.0 - t0 <= c.every_daa).min_by_key(|r| r.0);
                                    let (Some(b), Some(a), Some(sp)) = (before, after, spread) else {
                                        if !say {
                                        } else if before.is_none() || after.is_none() {
                                            println!("  answer: the claim {id8} asks for DAA {t0}, and this member has no reading of its own price within a round of it on both sides: it signs nothing (yet)");
                                        } else {
                                            println!("  answer: no epoch table of the committee is given (--epoch-table, --epoch-tables): its spread is not known, and nothing is signed");
                                        }
                                        continue;
                                    };
                                    let later = own_read.iter().filter(|r| r.0 > b.0 && r.0 <= a.0);
                                    let (lo, hi) = (later.clone().map(|r| r.2).chain([b.1]).min().expect("a reading"), later.map(|r| r.3).chain([b.1]).max().expect("a reading"));
                                    if (hi - lo) as i128 * kpf_protocol::state::BPS as i128 > lo as i128 * sp as i128 {
                                        if !say {
                                            continue;
                                        }
                                        println!("  answer: the claim {id8} asks for DAA {t0}, and this member's readings around it run from {} to {} KAS, further apart than the committee's spread: it signs nothing", fmt_kas(lo as i128), fmt_kas(hi as i128));
                                        continue;
                                    }
                                    let near = if t0 - b.0 <= a.0 - t0 { b } else { a };
                                    match crate::ccmds::sign_and_write(kpath, &kp, &cm, t0, near.1, Some(out), false) {
                                        Ok((t, out)) => println!("  answer: signed {} KAS for DAA {t0}, the time the claim {id8} asks for (its reading of DAA {})   {}", fmt_kas(t.price as i128), near.0, out.display()),
                                        Err(x) => println!("  answer: not signed for DAA {t0} ({x})"),
                                    }
                                }
                            }
                            (Err(x), _) | (_, Err(x)) => println!("  answer: {x}"),
                        }
                    }
                }
                (None, _, _) if c.a.get("do").is_some_and(|d| d.split(',').any(|x| x.trim() == "answer")) => println!("  answer: a member signs with its key: give --member-key FILE"),
                (Some(_), _, _) => println!("  answer: a member signs its own price of one market into the folder the signed prices are read from: give --price-file, --price-cmd or --price with one --market1, and --signed-prices DIR"),
                _ => {}
            }
        }
        // Claims.
        for (i, u) in &alive {
            let (op, age, fk) = (i.f.op, now.saturating_sub(u.daa) as i64, f.key);
            let back = |x: Position| vec![Made::Obj(0, TObj::Position(x))];
            let id8 = &op.transaction_id.to_string()[..8];
            // The watchtower: this key's own positions and claims, with the committee's signed prices at hand.
            if let (true, Some(g)) = (does("watch"), &watch) {
                match i.obj {
                    TObj::Position(x) if x.x.a_key == fk || x.x.b_key == fk => {
                        let side = if x.x.a_key == fk { SIDE_A } else { SIDE_B };
                        let mine = if side == SIDE_A { x.x.a_margin } else { x.x.b_margin };
                        let late = x.x.t.late_daa;
                        // (A fifth of the time a claim may land in is left for it to land.)
                        let wait = c.silence.unwrap_or(late / 3);
                        if wait + late / 5 > late {
                            println!("  watchtower: position {id8}: with --silence {wait} it is never claimed: a claim lands within {late} DAA of its time, and a fifth of that is kept for it to land");
                        }
                        match g.last(&x, side, now as i64, late) {
                            (Some((t0, prices, cl)), _, _) => {
                                let silent = now as i64 - t0;
                                if silent >= wait && cl.amount - mine > c.watch_gain {
                                    if silent + late / 5 <= late {
                                        let l = at(i, u, x);
                                        let title = format!("watchtower: no price signed for {silent} DAA: position {id8} claimed at the last one ({} KAS at DAA {t0}), {} KAS for a margin of {} KAS", fmt_kas(prices[0] as i128), fmt_kas(cl.amount as i128), fmt_kas(mine as i128));
                                        r.send(node, &title, "KpfTClose", &[op], false, &|s, p| tb::close(&lib, &l, s, side, t0, prices, None, p).map(|(b, cl)| (b, vec![Made::Obj(0, TObj::Claim(cl))])).map_err(e));
                                    } else {
                                        println!("  watchtower: position {id8}: the last price signed is {silent} DAA old, and a claim lands within {late} DAA of its time: it cannot be claimed at that price any more (what is left is the way out at the entries: pos-exit)");
                                    }
                                }
                            }
                            (None, there, uncovered) => {
                                let why = if uncovered > 0 { format!(": {uncovered} of them are of a time no epoch table given covers (the committee rolled: give the new epoch's epoch table, or a --epoch-tables folder that receives them)") } else { String::new() };
                                println!("  watchtower: position {id8}: no price a quorum signed in the last {late} DAA (the time a claim may land in) is in the signed prices' folder, of {there} signed prices of that time there{why}. Nothing to claim by");
                            }
                        }
                    }
                    TObj::Disputed(d) if d.claim.position.key_of(d.claim.side) == Some(fk) => match g.answer(&d.claim.position, d.claim.t0, &|c| d.claim.within(c)) {
                        Ok(Some(atts)) => match g.tables(node, net, &atts) {
                            Ok(epoch_tables) => {
                                let l = at(i, u, d);
                                let t1 = (&epoch_tables[0], &atts[0].1);
                                let t2 = epoch_tables.get(1).map(|t| (t, &atts[1].1));
                                r.send(node, &format!("watchtower: disputed claim {id8} proved with the committee's signatures"), "KpfTProve", &[op], false, &|s, p| tb::prove(&lib, &l, s, t1, t2, p).map(|b| (b, vec![])).map_err(e));
                            }
                            Err(x) => println!("  watchtower: disputed claim {id8}: {x}"),
                        },
                        Ok(None) => println!("  watchtower: disputed claim {id8}: the prices the committee signed for its time give this side less than it states: it cannot be proved, and whoever shows them refutes it"),
                        Err(why) => println!("  watchtower: disputed claim {id8}: {why}: it cannot be proved from here, and lapses"),
                    },
                    TObj::Liq(q) if q.liq.liquidator == fk && matches!(q.state, LiqState::Disputed(_)) => match g.answer(&q.liq.position, q.liq.t0, &|c| q.liq.within(c)) {
                        Ok(Some(atts)) => match g.tables(node, net, &atts) {
                            Ok(epoch_tables) => {
                                let l = at(i, u, q);
                                let t1 = (&epoch_tables[0], &atts[0].1);
                                let t2 = epoch_tables.get(1).map(|t| (t, &atts[1].1));
                                r.send(node, &format!("watchtower: disputed liquidation {id8} proved with the committee's signatures"), "KpfTLiqProve", &[op], false, &|s, p| tb::liq_prove(&lib, &l, s, t1, t2, p).map(|(b, n)| (b, vec![Made::Obj(0, TObj::Liq(n))])).map_err(e));
                            }
                            Err(x) => println!("  watchtower: disputed liquidation {id8}: {x}"),
                        },
                        Ok(None) => println!("  watchtower: disputed liquidation {id8}: it is not within the rule at the prices the committee signed for its time: it cannot be proved, and whoever shows them refutes it"),
                        Err(why) => println!("  watchtower: disputed liquidation {id8}: {why}: it cannot be proved from here, and lapses"),
                    },
                    TObj::Claim(cl) if cl.position.key_of(cl.side) == Some(fk) && cl.is_true() && cl.in_time(u.daa) && age >= cl.position.dispute_daa => {
                        let l = at(i, u, cl);
                        r.send(node, &format!("watchtower: claim {id8} settled"), "KpfTSettle", &[op], false, &|s, p| tb::settle(&lib, &l, s, p).map(|b| (b, vec![])).map_err(e));
                    }
                    _ => {}
                }
            }
            // What decides a claim is the committee's prices for its time; the prices a claim states decide nothing.
            // (review T1b, F33) Without the members' signed prices of that time at hand, a claim is judged by this
            // executor's own price of the market (`--price-file`, `--price-cmd`, `--price`) or not at all: a claim of
            // one leg, of that market, of a time it has readings around, within the rule at no price nearer than the
            // margin to those readings (`--dispute-margin-bps`, twice the position's tolerance unless said). An
            // executor without a price disputes nothing: a claim that states a low price and a true amount took the
            // bond of one that judged it by the price stated.
            let by_own = |x: &Position, t0: i64, within: &dyn Fn([i64; 2]) -> bool| -> bool {
                let Some((market, committee)) = own_market else { return false };
                let l = &x.x.legs[0];
                if x.x.legs[1].size > 0 || l.market != market || l.committee != committee {
                    return false;
                }
                // The readings on both sides of the claim's time, each no further than three rounds from it: the
                // price then was among them, the margin counted. A claim of a time this executor did not watch is
                // not judged.
                let reach = 3 * c.every_daa;
                let before = own_read.iter().filter(|r| r.0 <= t0 && t0 - r.0 <= reach).max_by_key(|r| r.0);
                let after = own_read.iter().filter(|r| r.0 >= t0 && r.0 - t0 <= reach).min_by_key(|r| r.0);
                let (Some(b), Some(a)) = (before, after) else { return false };
                // The price at the reading before, and everything the readings after it up to the one after the
                // claim's time say of the time since: their prices, and their lows and highs where the source gives
                // them (review T1b, F39: a price that moved and came back between two readings is seen only by a
                // source that says so; one that gives a single number is covered by the margin alone).
                let later = own_read.iter().filter(|r| r.0 > b.0 && r.0 <= a.0);
                let lo = later.clone().map(|r| r.2).chain([b.1]).min().expect("a reading") as i128;
                let hi = later.map(|r| r.3).chain([b.1]).max().expect("a reading") as i128;
                let (bps, g) = (kpf_protocol::state::BPS as i128, x.x.t.g_bps.max(1) as i128);
                // At least the tolerance; and tried in steps of a quarter of the tolerance whatever the margin
                // (review T1b, F37: wider steps stepped over the band a liquidation is within).
                let m = (c.margin_bps.unwrap_or(2 * x.x.t.g_bps) as i128).max(g);
                let (from, to) = (lo * (bps - m) / bps, hi * (bps + m) / bps + 1);
                let step = (lo * g / (4 * bps)).max(1);
                if (to - from) / step > 4_000 {
                    return false;
                }
                let mut p = from;
                while p <= to {
                    if within([p as i64, 0]) {
                        return false;
                    }
                    p += step;
                }
                !within([to as i64, 0])
            };
            // With the members' signed prices of the claim's time at hand (`--signed-prices`), a claim is judged by
            // them: `Some(atts)` where they refute it. (review T1b, F34) The members' prices differ within the
            // spread and any price a quorum stands near is the committee's: a claim one of them proves is not
            // disputed, whatever another refutes, since its claimer has the proof in hand before the dispute lands.
            let refuting = |x: &Position, t0: i64, within: &dyn Fn([i64; 2]) -> bool| -> (bool, Option<Vec<(usize, Attestation)>>, &'static str) {
                match watch.as_ref().map(|g| (g.answer(x, t0, &|c| !within(c)), g)) {
                    Some((Ok(Some(atts)), g)) => (!matches!(g.answer(x, t0, within), Ok(Some(_))), Some(atts), "not within the rule at the prices the committee signed for its time"),
                    Some((Ok(None), _)) => (false, None, ""),
                    _ => (by_own(x, t0, within), None, "not within the rule at any price near this executor's own"),
                }
            };
            match i.obj {
                TObj::Claim(c) => {
                    let l = at(i, u, c);
                    let (bad, _, why) = refuting(&c.position, c.t0, &|p| c.within(p));
                    if !c.in_time(u.daa) {
                        if does("late") {
                            r.send(node, &format!("late claim {id8} ended"), "KpfTLate", &[op], true, &|s, p| tb::late(&lib, &l, s, fk, p).map(|(b, x)| (b, back(x))).map_err(e));
                        }
                    } else if bad && age + guard < c.position.dispute_daa {
                        if does("dispute") {
                            r.send(node, &format!("claim {id8} disputed ({why})"), "KpfTDispute", &[op], false, &|s, p| tb::dispute(&lib, &l, s, fk, p).map(|(b, d)| (b, vec![Made::Obj(0, TObj::Disputed(d))])).map_err(e));
                        }
                    } else if !bad && age >= c.position.dispute_daa && does("settle") {
                        r.send(node, &format!("claim {id8} settled"), "KpfTSettle", &[op], false, &|s, p| tb::settle(&lib, &l, s, p).map(|b| (b, vec![])).map_err(e));
                    }
                }
                TObj::Disputed(d) => {
                    let l = at(i, u, d);
                    // Whoever shows the committee's prices against a claim has half of its bond: this key, here.
                    let refuted = match (does("dispute"), refuting(&d.claim.position, d.claim.t0, &|p| d.claim.within(p)).1, &watch) {
                        (true, Some(atts), Some(g)) => match g.tables(node, net, &atts) {
                            Ok(epoch_tables) => {
                                let t1 = (&epoch_tables[0], &atts[0].1);
                                let t2 = epoch_tables.get(1).map(|t| (t, &atts[1].1));
                                r.send(node, &format!("disputed claim {id8} refuted with the committee's signatures"), "KpfTProve", &[op], true, &|s, p| tb::refute(&lib, &l, s, t1, t2, fk, p).map(|(b, x)| (b, back(x))).map_err(e))
                            }
                            Err(x) => {
                                println!("  disputed claim {id8}: {x}");
                                false
                            }
                        },
                        _ => false,
                    };
                    if !refuted && d.disputer == fk && age >= d.claim.position.dispute_daa && does("lapse") {
                        r.send(node, &format!("disputed claim {id8} ended: nobody showed the committee's prices"), "KpfTDispute", &[op], false, &|s, p| tb::lapse(&lib, &l, s, p).map(|(b, x)| (b, back(x))).map_err(e));
                    }
                }
                TObj::Liq(q) => {
                    let l = at(i, u, q);
                    let w = q.liq.position.dispute_daa;
                    let (bad, by_price, why) = refuting(&q.liq.position, q.liq.t0, &|p| q.liq.within(p));
                    match q.state {
                        LiqState::Claimed if !q.liq.in_time(u.daa) => {
                            if does("late") {
                                r.send(node, &format!("late liquidation {id8} ended"), "KpfTLiq", &[op], true, &|s, p| tb::liq_late(&lib, &l, s, fk, p).map(|(b, x)| (b, back(x))).map_err(e));
                            }
                        }
                        LiqState::Claimed if bad && age + guard < w => {
                            if does("dispute") {
                                r.send(node, &format!("liquidation {id8} disputed ({why})"), "KpfTLiqDispute", &[op], false, &|s, p| tb::liq_dispute(&lib, &l, s, fk, p).map(|(b, d)| (b, vec![Made::Obj(0, TObj::Liq(d))])).map_err(e));
                            }
                        }
                        LiqState::Disputed(k) => {
                            let refuted = match (does("dispute"), by_price, &watch) {
                                (true, Some(atts), Some(g)) => match g.tables(node, net, &atts) {
                                    Ok(epoch_tables) => {
                                        let t1 = (&epoch_tables[0], &atts[0].1);
                                        let t2 = epoch_tables.get(1).map(|t| (t, &atts[1].1));
                                        r.send(node, &format!("disputed liquidation {id8} refuted with the committee's signatures"), "KpfTLiqProve", &[op], true, &|s, p| tb::liq_refute(&lib, &l, s, t1, t2, fk, p).map(|(b, x)| (b, back(x))).map_err(e))
                                    }
                                    Err(x) => {
                                        println!("  disputed liquidation {id8}: {x}");
                                        false
                                    }
                                },
                                _ => false,
                            };
                            if !refuted && k == fk && age >= w && does("lapse") {
                                r.send(node, &format!("disputed liquidation {id8} ended: nobody showed the committee's prices"), "KpfTLiqDispute", &[op], false, &|s, p| tb::liq_lapse(&lib, &l, s, p).map(|(b, x)| (b, back(x))).map_err(e));
                            }
                        }
                        LiqState::Claimed | LiqState::Proved if (!bad || q.state == LiqState::Proved) && age >= w && does("settle") => {
                            r.send(node, &format!("liquidation {id8} settled"), "KpfTLiqSettle", &[op], false, &|s, p| tb::liq_settle(&lib, &l, s, p).map(|b| (b, vec![])).map_err(e));
                        }
                        _ => {}
                    }
                }
                TObj::ExitClaim(q) => {
                    let l = at(i, u, q);
                    // An exit claim says the committee signed nothing since a time: a price a quorum signed at or after
                    // it ends the claim, and whoever shows it has half of the claimer's bond (review T1b, F46: with a
                    // committee that signs when asked nothing asked it, and nothing ended the claim).
                    let shown = match (does("dispute") || (does("watch") && (q.position.x.a_key == fk || q.position.x.b_key == fk)), &watch) {
                        // (Also a claim that landed before its own `since`: the script asks only that the price is of a
                        // time at or after it. Review T1b, F48.)
                        (true, Some(g)) => g.by_price().into_iter().map(|(t, _)| t).filter(|t| *t >= q.since).find_map(|t| g.answer(&q.position, t, &|_| true).ok().flatten()),
                        _ => None,
                    };
                    let ended = match (shown, &watch) {
                        (Some(atts), Some(g)) => match g.tables(node, net, &atts) {
                            Ok(epoch_tables) => {
                                let t1 = (&epoch_tables[0], &atts[0].1);
                                let t2 = epoch_tables.get(1).map(|t| (t, &atts[1].1));
                                r.send(node, &format!("exit claim {id8} ended: the committee signed after DAA {}", q.since), "KpfTExitRefute", &[op], true, &|s, p| tb::exit_refute(&lib, &l, s, t1, t2, fk, p).map(|(b, x)| (b, back(x))).map_err(e))
                            }
                            Err(x) => {
                                println!("  exit claim {id8}: {x}");
                                false
                            }
                        },
                        _ => false,
                    };
                    if ended {
                    } else if !q.of_the_past(u.daa) {
                        if does("late") {
                            r.send(node, &format!("exit claim {id8} of a time to come ended"), "KpfTExit", &[op], true, &|s, p| tb::exit_early(&lib, &l, s, fk, p).map(|(b, x)| (b, back(x))).map_err(e));
                        }
                    } else if age >= q.position.exit_daa() && does("settle") {
                        r.send(node, &format!("exit claim {id8} settled"), "KpfTExitEnd", &[op], false, &|s, p| tb::exit_settle(&lib, &l, s, p).map(|b| (b, vec![])).map_err(e));
                    }
                }
                TObj::Order(o) => {
                    if (now as i64) > o.expiry && does("expire") {
                        let l = at(i, u, o);
                        r.send(node, &format!("order {id8} returned after its expiry"), "KpfTCancel", &[op], false, &|s, p| tb::order_expire(&lib, &l, s, p).map(|b| (b, vec![])).map_err(e));
                    }
                }
                // A stated change of a position (a take): judged as a claim is, by the committee's signed prices of
                // its time where they are at hand and else by this executor's own. One that is not within the rule
                // is disputed; one nobody disputed is applied after the dispute period; one that landed out of its
                // time is ended. A disputed one is answered with a citation of a price note (`note-write`, then
                // `take-answer`): this loop does not write notes yet, and ends its own dispute when nobody answered.
                TObj::Change(k) => {
                    let l = at(i, u, k);
                    let held = k.held;
                    let x = k.position;
                    let within = |p: [i64; 2]| x.take_within(&k.take, &k.claimer, &k.outs, held, p);
                    match k.disputer {
                        None => {
                            let (bad, _, why) = refuting(&x, k.t0, &within);
                            if !k.in_time(u.daa) {
                                if does("late") {
                                    r.send(node, &format!("statement {id8} out of its time ended"), "KpfTChangeLate", &[op], true, &|s, p| tb::change_late(&lib, &l, s, fk, p).map(|(b, x)| (b, back(x))).map_err(e));
                                }
                            } else if bad && age + guard < x.dispute_daa {
                                if does("dispute") {
                                    r.send(node, &format!("take {id8} disputed ({why})"), "KpfTChangeDispute", &[op], false, &|s, p| tb::change_dispute(&lib, &l, s, fk, p).map(|(b, d)| (b, vec![Made::Obj(0, TObj::Change(d))])).map_err(e));
                                }
                            } else if !bad && age >= x.dispute_daa && does("settle") {
                                let made = x.taken(&k.take).filter(|n| k.becomes(n)).map(back).unwrap_or_default();
                                r.send(node, &format!("take {id8} applied"), "KpfTChangeSettle", &[op], false, &|s, p| tb::change_settle(&lib, &l, s, p).map(|b| (b, made.clone())).map_err(e));
                            }
                        }
                        Some(d) => {
                            if d == fk && age >= x.dispute_daa && does("lapse") {
                                r.send(node, &format!("disputed take {id8} ended: nobody showed the committee's prices"), "KpfTChangeDispute", &[op], false, &|s, p| tb::change_lapse(&lib, &l, s, p).map(|(b, x)| (b, back(x))).map_err(e));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // What gathered in the copies, to the library's fee pool: its sender keeps one part in twenty. One a round
        // (each spends the fee pool), the copy that holds the most first.
        if does("sweep") {
            if let Some(fee_pool) = &seen.fee_pool {
                let mut full: Vec<LibScript> = r.copies.values().flatten().filter(|c| !pinned::COLLECTORS.contains(&c.s) && !r.used.contains(&c.op)).filter(|c| kpf_protocol::thin::swept(c.value as i64, fee_pool.s.floor).is_some_and(|(moved, _)| moved / kpf_protocol::thin::ORACLE_FEE_CUT > min_gain)).cloned().collect();
                full.sort_by_key(|c| std::cmp::Reverse(c.value));
                // (The fullest first; if its sweep does not pay its fee, the next: a copy of a shorter script costs
                // less to sweep.)
                for copy in &full {
                    let title = format!("{} KAS of oracle fees swept out of a copy of {}", fmt_kas((copy.value as i64 - fee_pool.s.floor) as i128), copy.s);
                    if r.send(node, &title, "KpfTSweep", &[fee_pool.op, copy.op], true, &|s, p| tb::sweep(&lib, fee_pool, s, copy, p).map(|(b, _, _)| (b, vec![Made::Script(2, copy.s)])).map_err(e)) || r.left == 0 {
                        break;
                    }
                }
            }
        }
        if let Some(g) = &watch {
            st.told = g.told.take();
        }
        let did = r.did;
        let mut alone = std::mem::take(&mut r.alone);
        alone.retain(|(_, objs)| objs.iter().all(|o| listed.contains_key(o)));
        println!("round {round}, DAA {now}: {} objects live of {} in the folder, {} coins; {did} sent{}", alive.len(), registry.items.len(), r.coins.len(), if a.has("submit") { "" } else { " (a trial: nothing is sent without --submit)" });
        // What was refused is left alone for a while: another executor was first, or the folder is behind.
        for op in r.refused.drain(..) {
            avoid.insert(op, round + 3);
        }
        for mut s in r.sent.drain(..) {
            s.until = round + 2;
            waiting.push(s);
        }
        st.alone = alone;
    Ok(())
}

/// `coins`: the funder's coins as `--count` coins of `--each` KAS and the change. `exec-run` sends one transaction a
/// coin a round.
pub fn coins_cmd(a: &Args) -> R<()> {
    crate::ocmds::only(a, &["count", "each"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (fk, keys, fu) = crate::ocmds::funding(a, net, &mut node)?;
    let each = crate::cmds::price(a, "each")?;
    let b = tb::split(int_or(a, "count", 4)? as usize, each as u64, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    let sg = b.sign(&keys).map_err(e)?;
    finish_signed(a, "coins", &sg, net, &mut node, a.has("submit"))
}


/// `send`: `--amount` KAS from the funder's key to `--to` (an address, a key or a key file), with `--text` as the
/// `probe-seq-commit` (research, `docs/RESEARCH-UNIFIED-MARGIN.md`): is `OpChainblockSeqCommit` (KIP-21) live on the
/// network? Without `--spend`: 1 KAS to a script that passes only when it is shown the sequencing commitment of a
/// chain block (`OpChainblockSeqCommit OpEqual`). With `--spend TXID`: that output spent, the script shown the
/// commitment this node's header gives for a block `--back` selected parents behind the sink (default 10). A node
/// `node-call` (research): one call of the node by its name, with `--params JSON`, printed as it answers (cut to
/// `--cut` characters, default 2,000).
pub fn node_call_cmd(a: &Args) -> R<()> {
    crate::ocmds::only(a, &["method", "params", "cut"])?;
    let mut node = connect(a)?;
    let n = node.as_mut().ok_or("--node URL")?;
    let params: Value = serde_json::from_str(a.get("params").unwrap_or("{}")).map_err(|x| format!("--params: {x}"))?;
    let r = n.raw(a.need("method")?, params)?;
    let text = r.to_string();
    let cut = int_or(a, "cut", 2_000)?.max(1) as usize;
    println!("{} bytes: {}", text.len(), text.chars().take(cut).collect::<String>());
    Ok(())
}

/// `probe-cite` (research, `docs/RESEARCH-UNIFIED-MARGIN.md`): does a citation hold against a real block? The funder
/// sends itself a small payment, and once a chain block has accepted it the command reads what the node says of that
/// block (its header, its merged blocks' transactions, the native lane's proof) and recomputes the block's
/// sequencing commitment from the transaction up, as a citing script would (`kpf_protocol::cite`). It prints where the
/// computation agrees with the node. With `--tx ID --from BLOCK` nothing is sent: a transaction accepted after the
/// chain block `BLOCK` is looked for.
pub fn probe_cite_cmd(a: &Args) -> R<()> {
    use kaspa_consensus_core::Hash;
    use kpf_protocol::cite;
    crate::ocmds::only(a, &["tx", "from", "wait"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let h32 = |s: &str| crate::ocmds::hex32(s).map(Hash::from_bytes);
    let (txid, from) = match (a.get("tx"), a.get("from")) {
        (Some(t), Some(f)) => (t.to_string(), f.to_string()),
        _ => {
            let (_, sink) = node.as_mut().ok_or("--node URL")?.dag()?;
            let (fk, keys, fu) = crate::ocmds::funding(a, net, &mut node)?;
            let b = tb::send(&fk, 20_000_000, b"cite me".to_vec(), Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
            let sg = b.sign(&keys).map_err(e)?;
            let id = sg.tx.id().to_string();
            finish_signed(a, "probe: a payment to cite", &sg, net, &mut node, true)?;
            let secs = int_or(a, "wait", 15)?.clamp(5, 120) as u64;
            println!("waiting {secs} s for a chain block to accept it");
            std::thread::sleep(std::time::Duration::from_secs(secs));
            (id, sink)
        }
    };
    let n = node.as_mut().ok_or("--node URL")?;
    // 1. The chain block that accepted it, and everything that block accepted, in its order.
    let step = n.chain_ids(&from)?;
    let (block, ids) = step.groups.iter().find(|(_, ids)| ids.iter().any(|i| *i == txid)).cloned().ok_or(format!("no chain block after {from} has accepted {txid} yet ({} blocks read)", step.groups.len()))?;
    println!("accepted by the chain block {block}, the {} of {} transactions it accepted ({} bytes read of the chain's ids)", ids.iter().position(|i| *i == txid).unwrap_or(0) + 1, ids.len(), step.bytes);
    // 2. The block: its context, its commitment, its merged blocks.
    let (b, _) = n.block(&block, false)?;
    let head = b.get("header").ok_or("a block without a header")?;
    let hn = |k: &str| num(head.get(k)).ok_or(format!("a header without {k}"));
    let (timestamp, daa, blue) = (hn("timestamp")?, hn("daaScore")?, hn("blueScore")?);
    let commit = h32(head.get("acceptedIdMerkleRoot").and_then(Value::as_str).ok_or("no acceptedIdMerkleRoot")?)?;
    let vd = b.get("verboseData").ok_or("a block without verbose data")?;
    let parent = vd.get("selectedParentHash").and_then(Value::as_str).ok_or("no selected parent")?.to_string();
    let list = |k: &str| vd.get(k).and_then(Value::as_array).map(|x| x.iter().filter_map(|h| h.as_str().map(str::to_owned)).collect::<Vec<_>>()).unwrap_or_default();
    let merged = [list("mergeSetBluesHashes"), list("mergeSetRedsHashes")].concat();
    // (The context of a block's lanes carries the timestamp of its selected parent, with its own DAA and blue scores.)
    let (pb, _) = n.block(&parent, false)?;
    let parent_timestamp = num(pb.get("header").and_then(|h| h.get("timestamp"))).ok_or("the selected parent's header has no timestamp")?;
    let _ = timestamp;
    // 3. The transactions of the merged blocks: the version and the lane of each accepted one.
    let mut seen: HashMap<String, (kaspa_consensus_core::tx::Transaction, String)> = HashMap::new();
    let mut read = 0usize;
    for h in &merged {
        let (mb, size) = n.block(h, true)?;
        read += size;
        for t in mb.get("transactions").and_then(Value::as_array).cloned().unwrap_or_default() {
            let subnet = t.get("subnetworkId").and_then(Value::as_str).unwrap_or("").to_string();
            if let Ok(c) = crate::node::chain_tx(&t, daa) {
                seen.insert(c.id.to_string(), (c.tx, subnet));
            }
        }
    }
    println!("{} merged blocks read ({read} bytes), {} transactions in them", merged.len(), seen.len());
    let mut lane: Vec<(Hash, u16, u32)> = vec![];
    let mut at = None;
    for (i, id) in ids.iter().enumerate() {
        let (t, subnet) = seen.get(id).ok_or(format!("the accepted transaction {id} is in none of the merged blocks read"))?;
        if subnet.chars().all(|c| c == '0') && !subnet.is_empty() {
            if *id == txid {
                at = Some(lane.len());
            }
            lane.push((t.id(), t.version, i as u32));
        }
    }
    let at = at.ok_or("the transaction is not of the native lane")?;
    let cited = &seen[&txid].0;
    println!("the native lane has {} of the {} accepted; the transaction is of version {} with {} inputs, {} bytes without its signature scripts and payload", lane.len(), ids.len(), cited.version, cited.inputs.len(), if cited.version >= 1 { kaspa_consensus_core::hashing::tx::transaction_v1_rest_preimage(cited).len() } else { 0 });
    // 4. The lane's proofs at the block and at its selected parent.
    let key = cite::native_lane_key();
    let mut proof = |block: &str| -> R<(Option<(Hash, u64)>, Vec<u8>, Hash, Hash, Hash)> {
        let r = n.raw("getSeqCommitLaneProof", serde_json::json!({ "blockHash": block, "laneKey": key.to_string() }))?;
        let g = |k: &str| r.get(k).and_then(Value::as_str).ok_or(format!("the lane proof has no {k}")).and_then(|x| h32(x));
        let lane = match r.get("lane") {
            Some(l) if !l.is_null() => Some((h32(l.get("tip").and_then(Value::as_str).ok_or("a lane without a tip")?)?, num(l.get("blueScore")).ok_or("a lane without a blue score")?)),
            _ => None,
        };
        let bytes = r.get("smtProof").and_then(Value::as_array).map(|x| x.iter().filter_map(|b| b.as_u64().map(|b| b as u8)).collect()).unwrap_or_default();
        Ok((lane, bytes, g("payloadAndCtxDigest")?, g("parentSeqCommit")?, g("inactivityShortcut")?))
    };
    let (here, smt, payload_and_ctx, parent_commitment, inactivity_shortcut) = proof(&block)?;
    let (tip, lane_blue) = here.ok_or("the node says the native lane is not active at the block")?;
    let (before, ..) = proof(&parent)?;
    let parent_ref = before.map(|x| x.0).ok_or("the node says the native lane is not active at the block's selected parent")?;
    // 5. The witness, and the commitment recomputed.
    let leaves = cite::activity_leaves(&lane);
    let (_, activity_path) = cite::merkle(&leaves, at);
    let w = cite::Witness { block: h32(&block)?, merge_idx: lane[at].2, activity_path, lane_key: key, parent_ref, context: cite::context(parent_timestamp, daa, blue), blue_score: lane_blue, lanes_path: cite::lanes_path(&smt, &key).map_err(e)?, inactivity_shortcut, payload_and_ctx, parent_commitment };
    let (id, version) = (cited.id(), cited.version);
    let my_tip = w.tip(&id, version);
    println!("the lane's tip: the node says {tip}; recomputed {my_tip}: {}", if my_tip == tip { "AGREE" } else { "DIFFER" });
    let mine = w.commitment(&id, version);
    println!("the block's sequencing commitment: its header says {commit}; recomputed from the transaction {mine}: {}", if mine == commit { "AGREE" } else { "DIFFER" });
    println!("the proof: {} steps in the lane's activity, {} in the lanes' tree, {} bytes beside the transaction's rest (the lane's leaf is of blue score {lane_blue}, the block's {blue})", w.activity_path.len(), w.lanes_path.len(), w.bytes());
    if mine != commit {
        return Err("the citation does not hold against this block: the form of the proof is not the node's".into());
    }
    Ok(())
}

/// that takes the spend runs the opcode and agrees on the value; one on which the opcode is not active refuses it.
pub fn probe_seq_cmd(a: &Args) -> R<()> {
    crate::ocmds::only(a, &["spend", "back"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let redeem = vec![0xd4u8, 0x87];
    let (fk, keys, fu) = crate::ocmds::funding(a, net, &mut node)?;
    let Some(txid) = a.get("spend") else {
        let b = kpf_protocol::oracle_build::pay_script(&redeem, 100_000_000, &fu, &fk, rate(a)?).map_err(e)?;
        let sg = b.sign(&keys).map_err(e)?;
        println!("then, once it is accepted: probe-seq-commit --spend {} --funder ... --node ... --submit", sg.tx.id());
        return finish_signed(a, "probe: 1 KAS to a script that asks for a chain block's sequencing commitment", &sg, net, &mut node, a.has("submit"));
    };
    let n = node.as_mut().ok_or("--node URL: the block and its commitment are read in the node's headers")?;
    let (_, sink) = n.dag()?;
    let mut hash = sink;
    for _ in 0..int_or(a, "back", 10)?.max(1) {
        let (b, _) = n.block(&hash, false)?;
        hash = b.get("verboseData").and_then(|v| v.get("selectedParentHash")).and_then(Value::as_str).ok_or("a block without its selected parent")?.to_string();
    }
    let (b, _) = n.block(&hash, false)?;
    let commit = b.get("header").and_then(|h| h.get("acceptedIdMerkleRoot")).and_then(Value::as_str).ok_or("a header without acceptedIdMerkleRoot")?.to_string();
    let (block, commit) = (crate::ocmds::hex32(&hash)?, crate::ocmds::hex32(&commit)?);
    println!("chain block {}: the header's acceptedIdMerkleRoot (the sequencing commitment, if KIP-21 is live) is {}", to_hex(&block), to_hex(&commit));
    kpf_protocol::verify::set_chain_view(vec![(kaspa_consensus_core::Hash::from_bytes(block), kaspa_consensus_core::Hash::from_bytes(commit))]);
    let sig: Vec<u8> = [vec![0x20], commit.to_vec(), vec![0x20], block.to_vec(), vec![0x02], redeem.clone()].concat();
    let op = TransactionOutpoint { transaction_id: kaspa_consensus_core::Hash::from_bytes(crate::ocmds::hex32(txid)?), index: 0 };
    let sg = kpf_protocol::oracle_build::raw_spend(op, 100_000_000, &redeem, sig, &fk, rate(a)?).map_err(e)?;
    finish_signed(a, "probe: the script is shown the chain block's sequencing commitment (OpChainblockSeqCommit)", &sg, net, &mut node, a.has("submit"))
}

/// transaction's payload: a note that whoever reads the transaction reads. Plain KAS; nothing of the protocol.
pub fn send_cmd(a: &Args) -> R<()> {
    crate::ocmds::only(a, &["to", "amount", "text"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let to = crate::keys::party(a.need("to")?, net)?;
    let (fk, keys, fu) = crate::ocmds::funding(a, net, &mut node)?;
    let amount = crate::cmds::price(a, "amount")?;
    let text = a.get("text").unwrap_or("");
    let b = tb::send(&to, amount as u64, text.as_bytes().to_vec(), Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("send: {} KAS to {}{}", fmt_kas(amount as i128), p2pk_address(&to, net), if text.is_empty() { String::new() } else { format!(", with a note of {} bytes in the payload (public: anyone reads it)", text.len()) });
    let sg = b.sign(&keys).map_err(e)?;
    finish_signed(a, "send", &sg, net, &mut node, a.has("submit"))
}
