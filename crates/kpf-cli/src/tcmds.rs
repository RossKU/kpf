//! The commands of the thin family, the main one (docs/DESIGN-COMMITTEE.md 8a): a library of scripts, positions, and
//! declared claims. Offline unless `--node` is given, nothing submitted without `--submit`, every transaction verified
//! locally first, as every other command.
//!
//! The chain holds an object's hash, so its file holds its state (`thin`) and its library (`library`); a command reads
//! both and refuses a file whose state is not the one its script carries. A copy of a library's script is a file too
//! (`--script`): after a transaction, the copy it used is the file of its output 1.

use kaspa_consensus_core::tx::{Transaction, TransactionOutpoint};
use kaspa_consensus_core::Hash;
use kpf_protocol::cross::{SIDE_A, SIDE_B};
use kpf_protocol::error::{from_hex, hex32, to_hex};
use kpf_protocol::network::{p2pk_address, p2sh_address, spk_address, Network};
use kpf_protocol::oracle_build::{OBuilt, OLive, OPartial};
use kpf_protocol::thin::{Position, LiqState, Order, Rest, TObj, Template};
use kpf_protocol::thin_artifacts as pinned;
use kpf_protocol::thin_build::{self as tb, LibScript, Payer};
use kpf_protocol::tx::{p2pk_key, KeySet};
use serde_json::Value;

use crate::args::{fmt_kas, Args};
use crate::cmds::{connect, e, int_or, network, price, with_node_facts, R};
use crate::files::{funding_utxos, parse_utxos, read_json, write_json, ObjFile};
use crate::node::{Node, WsTransport};
use crate::ocmds::{finish_signed, funding, now, only, out_dir, rate};

pub(crate) type N = Option<Node<WsTransport>>;

/// An object of the family read from its file.
pub(crate) struct Loaded {
    pub(crate) f: ObjFile,
    pub(crate) lib: [u8; 32],
    pub(crate) obj: TObj,
}

pub(crate) fn load(a: &Args, flag: &str, node: &mut N, net: Network) -> R<Loaded> {
    load_path(a.need(flag)?, node, net)
}

/// The object of the file `path`, held to its script (and, with a node, to what the node lists).
pub(crate) fn load_path(path: &str, node: &mut N, net: Network) -> R<Loaded> {
    let v = read_json(path)?;
    let f = with_node_facts(ObjFile::from_json(&v).map_err(|x| format!("{path}: {x}"))?, node, net)?;
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).ok_or_else(|| format!("{path}: no `{k}`: not the file of an object of the thin family"));
    let obj = TObj::decode(&from_hex(text("thin")?).map_err(e)?).map_err(|x| format!("{path}: {x}"))?;
    let lib = hex32(text("library")?).map_err(e)?;
    if pinned::kit().read(&f.redeem) != Some((lib, obj.hash())) {
        return Err(format!("{path}: the state in the file is not the one its script carries"));
    }
    // No script ties an object's value to its state: an object that holds less than its margins and its bonds pays
    // whoever claims first, up to what it holds (review T1b, round 2).
    if (f.amount as i64) < obj.least_value() {
        return Err(format!("{path}: REFUSED: the object holds {} KAS and its state says at least {}", fmt_kas(f.amount as i128), fmt_kas(obj.least_value() as i128)));
    }
    Ok(Loaded { f, lib, obj })
}

pub(crate) fn live<T>(l: &Loaded, s: T, need_daa: bool) -> R<OLive<T>> {
    let daa = if need_daa { l.f.known_daa()? } else { l.f.daa.unwrap_or(0) };
    Ok(OLive { s, value: l.f.amount, id: (l.f.covenant_id != Hash::default()).then_some(l.f.covenant_id), op: l.f.op, daa })
}

pub(crate) fn position_of(a: &Args, node: &mut N, net: Network) -> R<(Loaded, Position)> {
    let l = load(a, "position", node, net)?;
    match l.obj {
        TObj::Position(x) => Ok((l, x)),
        o => Err(format!("--position is a {}, not a position", o.kind())),
    }
}

/// The copy of `name` given as `--script`, which must be of the library `lib`.
pub(crate) fn script_of(a: &Args, lib: &[u8; 32], name: &'static str, node: &mut N, net: Network) -> R<LibScript> {
    let Some(path) = a.get("script") else {
        // No file: a live copy is found at the node (every copy of a script is at one address, under the library's
        // id). Another machine has no file of the copy a transaction of somebody else's wrote last.
        let redeem = pinned::kit().script(name).ok_or_else(|| format!("no script {name}"))?.to_vec();
        let f = found(node, net, redeem, lib, &format!("copy of the script {name}"), "script")?;
        println!("script {name}: the copy at {}:{}, found at the node", f.op.transaction_id, f.op.index);
        return Ok(OLive { s: name, value: f.amount, id: Some(f.covenant_id), op: f.op, daa: f.daa.unwrap_or(0) });
    };
    let f = with_node_facts(ObjFile::from_json(&read_json(path)?).map_err(|x| format!("{path}: {x}"))?, node, net)?;
    if Some(&f.redeem[..]) != pinned::kit().script(name) {
        return Err(format!("--script is not a copy of {name} (this command's script)"));
    }
    if f.covenant_id.as_bytes() != *lib {
        return Err("--script is a copy in another library than the object's".into());
    }
    Ok(OLive { s: name, value: f.amount, id: Some(f.covenant_id), op: f.op, daa: f.daa.unwrap_or(0) })
}

/// A UTXO with the script `redeem` under the covenant id `id`, read from the node: one of those the node lists, taken
/// by the clock (two users that look at the same moment seldom take the same one; whoever loses asks again).
pub(crate) fn found(node: &mut N, net: Network, redeem: Vec<u8>, id: &[u8; 32], what: &str, flag: &str) -> R<ObjFile> {
    let Some(n) = node else { return Err(format!("give --{flag} FILE, or --node URL to find a {what} there")) };
    let list: Vec<_> = n.utxos(&[p2sh_address(&redeem, net)])?.into_iter().filter(|u| u.covenant_id.map(|c| c.as_bytes()) == Some(*id)).collect();
    if list.is_empty() {
        return Err(format!("no {what} under the id {} is at the node this moment (each is in a transaction not yet in a block, or none was made): ask again", to_hex(id)));
    }
    let pick = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() as usize).unwrap_or(0) % list.len();
    let u = &list[pick];
    Ok(ObjFile { op: u.op, amount: u.amount, daa: Some(u.daa), covenant_id: Hash::from_bytes(*id), redeem })
}

/// What a transaction wrote that a later command takes as a file.
#[derive(Clone, Copy)]
pub(crate) enum Made {
    Obj(usize, TObj),
    /// An order, whose file carries its template too (a match is shown it).
    OrderOf(usize, Order, Template),
    Script(usize, &'static str),
}

pub(crate) fn file_of(id: kaspa_consensus_core::tx::TransactionId, daa: Option<u64>, tx: &Transaction, i: usize, redeem: Vec<u8>, net: Network) -> Value {
    let o = &tx.outputs[i];
    ObjFile { op: TransactionOutpoint { transaction_id: id, index: i as u32 }, amount: o.value, daa, covenant_id: o.covenant.map(|c| c.covenant_id).unwrap_or_default(), redeem }.to_json(net)
}

/// The files of what `tx` made, in `dir`.
pub(crate) fn write_made(tx: &Transaction, made: &[Made], lib: &[u8; 32], net: Network, dir: &std::path::Path) -> R<()> {
    write_made_as(tx.id(), None, tx, made, lib, net, dir, true).map(|_| ())
}

/// As `write_made`, for a transaction read from the chain: its id as the node gives it (what a node shows of an
/// accepted transaction is not all the id is made of) and the DAA score it was accepted at. Returns the files.
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_made_as(id: kaspa_consensus_core::tx::TransactionId, daa: Option<u64>, tx: &Transaction, made: &[Made], lib: &[u8; 32], net: Network, dir: &std::path::Path, say: bool) -> R<Vec<std::path::PathBuf>> {
    let tag: String = id.to_string().chars().take(8).collect();
    let mut files = vec![];
    for m in made {
        let (i, name, v) = match m {
            Made::Obj(i, o) => {
                let mut v = file_of(id, daa, tx, *i, o.script(lib), net);
                v["kind"] = Value::from(o.kind());
                v["thin"] = Value::from(to_hex(&o.encode()));
                v["library"] = Value::from(to_hex(lib));
                v["state"] = Value::from(format!("{o:?}"));
                (*i, o.kind().replace(' ', "-"), v)
            }
            Made::OrderOf(i, o, t) => {
                let obj = TObj::Order(*o);
                let mut v = file_of(id, daa, tx, *i, obj.script(lib), net);
                v["kind"] = Value::from("order");
                v["thin"] = Value::from(to_hex(&obj.encode()));
                v["library"] = Value::from(to_hex(lib));
                v["template"] = Value::from(to_hex(&t.bytes()));
                v["state"] = Value::from(format!("{o:?}"));
                (*i, "order".to_string(), v)
            }
            Made::Script(i, n) => {
                let mut v = file_of(id, daa, tx, *i, pinned::kit().script(n).expect("a script").to_vec(), net);
                v["kind"] = Value::from("library script");
                v["script"] = Value::from(*n);
                v["library"] = Value::from(to_hex(lib));
                (*i, format!("script-{n}"), v)
            }
        };
        let p = dir.join(format!("{tag}-out{i}-{name}.json"));
        write_json(&p, &v)?;
        if say {
            println!("written: {}", p.display());
        }
        files.push(p);
    }
    Ok(files)
}

/// The keys of an object's parties.
pub(crate) fn parties(o: &TObj) -> Vec<[u8; 32]> {
    let of = |a: &Position| vec![a.x.a_key, a.x.b_key];
    match o {
        TObj::Position(a) => of(a),
        TObj::Claim(c) => of(&c.position),
        TObj::Disputed(d) => [of(&d.claim.position), vec![d.disputer]].concat(),
        TObj::Liq(l) => [of(&l.liq.position), vec![l.liq.liquidator]].concat(),
        TObj::ExitClaim(q) => of(&q.position),
        TObj::Change(c) => [of(&c.position), vec![c.claimer], c.disputer.into_iter().collect()].concat(),
        TObj::Order(o) => vec![o.owner],
        TObj::Anchor => vec![],
    }
}

use tb::{swap_position as sw_position, swap_funds as sw_funds, Swap as Sw};

pub const THIN_PARTIAL: &str = "kpf-thin-partial/1";

/// `done` for a transaction that two users sign, which `--partial FILE` writes signed by the keys at hand only: the
/// other signs it on its own machine (`cosign`). `mk` builds the transaction with the keys it is given.
///
/// (What a script that waits for a signature costs to run is measured on a twin: `thin_build::sign_apart`.) `spent` are the states of the objects the transaction spends: with those it makes they
/// go into the file, where the co-signer's tool reads them and holds each to the script of its input or output.
fn done_by(a: &Args, title: &str, mk: &dyn Fn(Sw) -> R<OBuilt>, keys: &KeySet, net: Network, node: &mut N, lib: &[u8; 32], made: &[Made], spent: &[TObj]) -> R<()> {
    let b = mk(&|k| k)?;
    let Some(path) = a.get("partial") else { return done(a, title, &b, keys, net, node, lib, made) };
    if a.has("submit") {
        return Err("--partial writes a transaction for its co-signer: it is not submitted here (drop --submit)".into());
    }
    let (part, size) = tb::sign_apart(&|f| mk(f).map_err(kpf_protocol::Error::Invalid), keys).map_err(e)?;
    println!("== {title} (partly signed)");
    println!("txid {}  bytes {size} when whole", part.tx.id());
    for (i, l) in part.input_labels.iter().enumerate() {
        let left = part.open.iter().find(|(j, _)| *j == i).map(|(_, k)| format!("   LEFT TO SIGN: {}", p2pk_address(k, net))).unwrap_or_default();
        println!("  in  {i}: {l} {} KAS{left}", fmt_kas(part.entries[i].amount as i128));
        for (_, _, k) in part.open_sigs.iter().filter(|(j, _, _)| *j == i) {
            println!("         WAITS FOR THE SIGNATURE OF {}", p2pk_address(k, net));
        }
    }
    for (i, l) in part.output_labels.iter().enumerate() {
        println!("  out {i}: {l} {} KAS", fmt_kas(part.tx.outputs[i].value as i128));
    }
    let mut states: Vec<TObj> = spent.to_vec();
    let mut templates: Vec<Template> = vec![];
    for m in made {
        match m {
            Made::Obj(_, o) => states.push(*o),
            Made::OrderOf(_, o, t) => {
                states.push(TObj::Order(*o));
                templates.push(*t);
            }
            Made::Script(..) => {}
        }
    }
    let v = serde_json::json!({
        "format": THIN_PARTIAL,
        "title": title,
        "network": net.name(),
        "library": to_hex(lib),
        "partial": serde_json::to_value(&part).map_err(e)?,
        "states": states.iter().map(|o| to_hex(&o.encode())).collect::<Vec<_>>(),
        "templates": templates.iter().map(|t| to_hex(&t.bytes())).collect::<Vec<_>>(),
    });
    write_json(std::path::Path::new(path), &v)?;
    println!("written: {path}");
    if !a.has("no-save") {
        // What it makes, for this machine: files of UTXOs that exist once the last signer has sent the transaction.
        write_made(&part.tx, made, lib, net, &out_dir(a, true))?;
        println!("the files above are of what the transaction makes: they hold once the co-signer has sent it (`kpf status --object FILE --node URL`)");
    }
    println!("the signatures of the keys given here (--key) are in it, and hold only for this transaction. Give the file to whoever must still sign, who reads what it does and signs: `kpf cosign --tx {path} --lib GENESIS-tx.json --key KEY --node URL --submit`");
    println!("NOTE: whoever holds the file with the last signature sends it when it likes, for as long as every input is unspent: a transaction has no last day. To take your signature back, spend one of its inputs before it is sent (whichever lands first stands): a coin of yours among them, or the position itself (`pos-add-margin` writes it anew; an anchor is not yours to move, so for an opening it is your coin)");
    Ok(())
}

/// Signs, verifies, prints, saves and (with `--submit`) sends, then writes the files of what the transaction made:
/// under `dry-run/` unless the node accepted it.
pub(crate) fn done(a: &Args, title: &str, b: &OBuilt, keys: &KeySet, net: Network, node: &mut N, lib: &[u8; 32], made: &[Made]) -> R<()> {
    if a.has("partial") {
        return Err(format!("{title} is not written partly signed: `--partial` is for what two users sign (pos-open, pos-close --mutual, pos-join, pos-leave)"));
    }
    let sg = b.sign(keys).map_err(e)?;
    let submit = a.has("submit");
    let r = finish_signed(a, title, &sg, net, node, submit);
    if !a.has("no-save") {
        write_made(&sg.tx, made, lib, net, &out_dir(a, !(submit && r.is_ok())))?;
    }
    r
}

pub(crate) fn side_of(s: &str) -> R<i64> {
    match s.to_ascii_lowercase().as_str() {
        "a" | "owner" | "1" => Ok(SIDE_A),
        "b" | "counterparty" | "2" => Ok(SIDE_B),
        _ => Err(format!("side must be a (the position's owner) or b (its counterparty), not `{s}`")),
    }
}

pub(crate) fn word(side: i64) -> &'static str {
    if side == SIDE_A {
        "a (the owner)"
    } else {
        "b (the counterparty)"
    }
}

pub(crate) fn show(o: &TObj, lib: &[u8; 32], landed: Option<u64>, net: Network) {
    if let TObj::Anchor = o {
        println!("anchor of a registry of the library {}: the way in for an order (order-place) or a position (pos-open)", to_hex(lib));
        return;
    }
    let position = match o {
        TObj::Position(a) => a,
        TObj::Claim(c) => &c.position,
        TObj::Disputed(d) => &d.claim.position,
        TObj::Liq(l) => &l.liq.position,
        TObj::ExitClaim(q) => &q.position,
        TObj::Change(c) => &c.position,
        TObj::Anchor => return,
        TObj::Order(o) => {
            println!("order of library {}", to_hex(lib));
            println!("state hash {}", to_hex(&o.hash()));
            println!("{} {} units left at a limit of {} KAS, by {}", if o.dir == kpf_protocol::state::LONG { "long" } else { "short" }, o.size, fmt_kas(o.limit as i128), p2pk_address(&o.owner, net));
            println!("margin {} KAS and a matcher's tip of {} KAS for what is left; what a match leaves rests; from DAA {} anyone returns it to its owner", fmt_kas(o.margin as i128), fmt_kas(o.tip as i128), o.expiry);
            println!("a match takes at least {} units of it, or all that is left; it asks {} KAS of margin of the other side", o.min_fill, fmt_kas(o.min_counter as i128));
            println!("template {}", to_hex(&o.tpl));
            return;
        }
    };
    println!("{} of library {}", o.kind(), to_hex(lib));
    println!("state hash {}", to_hex(&o.hash()));
    crate::ccmds::cross_state(&position.x, None, None, net);
    if position.a_min_counter > 0 || position.b_min_counter > 0 {
        println!("asked of a counterparty's margin: a asks {} KAS, b asks {} KAS", fmt_kas(position.a_min_counter as i128), fmt_kas(position.b_min_counter as i128));
    }
    println!("claims: a dispute period of {} DAA, a bond of {} KAS ({} basis points of the pot); an exit claim waits {} DAA", position.dispute_daa, fmt_kas(position.bond() as i128), position.bond_bps, position.exit_daa());
    if let TObj::ExitClaim(q) = o {
        println!("side {} states: the committee signed no price of one of the position's markets since DAA {}", word(q.side), q.since);
        println!("ended by a signed price of each market signed at DAA {} or after (`pos-refute`); else settled at the entries once it is {} DAA old: a {} KAS, b the rest", q.since, position.exit_daa(), fmt_kas((position.x.a_margin + if q.side == SIDE_A { q.bond } else { 0 }) as i128));
        if let Some(d) = landed {
            println!("landed at DAA {d}: {}", if q.of_the_past(d) { "it speaks of the past" } else { "BEFORE its own `since`: ended by anyone with `pos-late`" });
        }
        return;
    }
    if let TObj::Liq(l) = o {
        let q = &l.liq;
        match l.state {
            LiqState::Claimed => {}
            LiqState::Disputed(d) => println!("disputed by {}: decided by whoever shows the committee's prices for its time (`pos-prove` if it is within the rule there, `pos-refute` if not: the loser's bond is shared by the winner and the committee); unanswered after the dispute period it is void, with both bonds back (`pos-lapse`)", p2pk_address(&d, net)),
            LiqState::Proved => println!("proved by its liquidator: settled once it is a dispute period old again, unless the side cures"),
        }
        println!("liquidation of side {} stated by {}: a fee of {} KAS, a cure adds {} KAS, at the prices {} / {} of the time at DAA {}", word(q.side), p2pk_address(&q.liquidator, net), fmt_kas(q.fee as i128), fmt_kas(q.add as i128), q.a[0], q.a[1], q.t0);
        match position.liquidation(q.side, q.t0, q.a, q.liquidator) {
            _ if q.within(q.a) => println!("CHECK: the liquidation is within the rule at these prices, the liquidator's own. A dispute is decided at the prices the committee signs for that time"),
            Some(r) => println!("CHECK: FALSE. The rule gives a fee of at most {} KAS and a cure of at most {} KAS at these prices: unless the committee's prices differ, a dispute refutes this claim", fmt_kas(r.fee as i128), fmt_kas(r.add as i128)),
            None => println!("CHECK: FALSE. The rule does not liquidate this side at these prices: unless the committee's prices differ, a dispute refutes this claim"),
        }
        if let (Some(d), LiqState::Claimed) = (landed, l.state) {
            println!("landed at DAA {d}: {}", if q.in_time(d) { "in time" } else { "LATE (neither settled nor disputed: ended with `pos-late`)" });
        }
        return;
    }
    if let TObj::Change(c) = o {
        println!("stated by {} for the time at DAA {}: a change of kind {} with a bond of {} KAS; the position becomes:", p2pk_address(&c.claimer, net), c.t0, c.kind, fmt_kas(c.bond as i128));
        for out in c.outs.list() {
            println!("  {} KAS {}", fmt_kas(out.value as i128), if out.key { format!("to {}", p2pk_address(&out.to, net)) } else { format!("in the position, with the state {}", to_hex(&out.to)) });
        }
        match c.disputer {
            Some(d) => println!("disputed by {}: decided by whoever shows the committee's prices for its time in a citation (`take-answer`); unanswered after the dispute period it lapses, and both bonds return", p2pk_address(&d, net)),
            None => println!("nothing is computed on the chain: applied after the dispute period of {} DAA (`change-settle`) unless it is disputed (`change-dispute`) or answered at once with a citation (`take-answer`)", c.position.dispute_daa),
        }
        if let Some(d) = landed {
            println!("landed at DAA {d}: {}", if c.in_time(d) { "in time" } else { "OUT OF ITS TIME (ended with `change-late`)" });
        }
        return;
    }
    let claim = match o {
        TObj::Position(_) | TObj::Liq(_) | TObj::ExitClaim(_) | TObj::Change(_) | TObj::Order(_) | TObj::Anchor => return,
        TObj::Claim(c) => c,
        TObj::Disputed(d) => {
            println!("disputed by {}: decided by whoever shows the committee's prices for its time (`pos-prove` if they give at least the amount, `pos-refute` if not: the loser's bond is shared by the winner and the committee); unanswered after the dispute period it is void, with both bonds back (`pos-lapse`)", p2pk_address(&d.disputer, net));
            &d.claim
        }
    };
    println!("stated by side {}: owed {} KAS at the prices {} / {} of the time at DAA {}", word(claim.side), fmt_kas(claim.amount as i128), claim.a[0], claim.a[1], claim.t0);
    match position.owed(claim.side, claim.a) {
        Some(v) if v == claim.amount => println!("CHECK: the amount is what the rule gives at these prices, the claimer's own, counted the tolerance worse for it. A dispute is decided at the prices the committee signs for that time"),
        Some(v) if claim.amount < v => println!("CHECK: the amount is {} KAS less than the rule gives at these prices, the claimer's own", fmt_kas((v - claim.amount) as i128)),
        Some(v) => println!("CHECK: FALSE. The rule gives {} KAS at these prices: unless the committee's prices are better for the claimer, a dispute refutes this claim", fmt_kas(v as i128)),
        None => println!("CHECK: FALSE. The rule allows this side no close at these prices: unless the committee's prices differ, a dispute refutes this claim"),
    }
    if let Some(d) = landed {
        println!("landed at DAA {d}: {}", if claim.in_time(d) { "in time" } else { "LATE (neither settled nor disputed: ended with `pos-late`)" });
    }
}

/// The signed price and the prices a claim states: the committee's, when the epoch tables and the members' signed prices are given
/// (`--epoch-table1 --signed-price1 ...`), so that the claim can be proved if disputed; else as typed (`--t0 --p1 --p2`), with a
/// warning.
pub(crate) fn stated_price(a: &Args, x: &Position, node: &mut N, net: Network) -> R<(i64, [i64; 2])> {
    if a.has("epoch-table1") {
        let t1 = crate::ccmds::x_att(a, 1, &x.x, node, net)?;
        // An epoch table serves its epoch's signed prices until one epoch after the epoch's end, when anyone may retire it: the
        // claim's proof (as late as lateDaa and two dispute periods after the signed price) must come before that (review T1).
        let lives = |t: &kpf_protocol::committee::EpochTable| t.until + (t.until - t.from);
        if t1.1.t0 + x.proof_span() > lives(&t1.0.s) {
            return Err(format!("REFUSED: a dispute of this claim could come as late as DAA {} and the epoch table of its time can be retired from DAA {}: the claim could not be proved. State it at a signed price of a later epoch, or use a position whose dispute period is shorter (lateDaa and two dispute periods within an epoch)", t1.1.t0 + x.proof_span(), lives(&t1.0.s)));
        }
        let mut prices = [t1.1.p, 0];
        if x.x.legs[1].size > 0 {
            let t2 = crate::ccmds::x_att(a, 2, &x.x, node, net)?;
            if t2.1.t0 != t1.1.t0 {
                return Err(format!("REFUSED: leg 1 is attested for the time {} and leg 2 for {}: a claim states one time", t1.1.t0, t2.1.t0));
            }
            prices[1] = t2.1.p;
        }
        return Ok((t1.1.t0, prices));
    }
    println!("stated at prices of the funder's own (--t0 --p1 --p2). If the claim is disputed, the prices the committee signs for that time decide it");
    let t0 = a.need("t0")?.parse::<i64>().map_err(|_| "--t0 is the signed price's DAA, an integer".to_string())?;
    Ok((t0, [price(a, "p1")?, if x.x.legs[1].size > 0 { price(a, "p2")? } else { 0 }]))
}

/// The library `--lib` names: the file of its genesis transaction (`lib-create` writes it: `<txid>-tx.json`), which
/// is read and checked (`thin_build::lib_check`: every output under the id is one of the family's scripts), or a
/// bare id, which is taken only with `--lib-unchecked` (review T1b, F4: a position in a library with one script
/// more than the family's is anybody's).
pub(crate) fn lib_of(a: &Args) -> R<[u8; 32]> {
    let v = a.need("lib")?;
    if v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()) {
        if !a.has("lib-unchecked") {
            return Err("REFUSED: --lib is a bare id, and a library is safe only if its genesis wrote nothing but the family's scripts. Give the file of its genesis transaction (--lib <txid>-tx.json), which is checked, or --lib-unchecked if you checked it yourself".into());
        }
        println!("WARNING: the library's genesis was NOT checked (--lib-unchecked)");
        return hex32(v).map_err(e);
    }
    let file = read_json(v)?;
    let tx = kpf_protocol::tx::tx_from_rpc_json(file.get("transaction").ok_or_else(|| format!("{v}: no `transaction`: not the file of a transaction"))?).map_err(|x| format!("{v}: {x}"))?;
    let id = tb::lib_check(&tx).map_err(|x| format!("{v}: {x}"))?;
    println!("library {}: its genesis writes the family's scripts and nothing else under its id", to_hex(&id));
    Ok(id)
}

/// A transaction read from the file a command wrote (`<txid>-tx.json`).
pub(crate) fn tx_file(path: &str) -> R<kaspa_consensus_core::tx::Transaction> {
    let file = read_json(path)?;
    kpf_protocol::tx::tx_from_rpc_json(file.get("transaction").ok_or_else(|| format!("{path}: no `transaction`: not the file of a transaction"))?).map_err(|x| format!("{path}: {x}"))
}

/// The anchor `--anchor` of the library `lib`: the way into its registry. With `--registry` (the file of the registry's genesis
/// transaction, which is read and checked: it wrote anchors and nothing else) the anchor must be of that registry;
/// without it the command is refused unless `--registry-unchecked` (review T1b, F14). An order or a position in a registry whose genesis wrote anything but
/// anchors is among objects nobody signed, which the scripts take for signed.
pub(crate) fn anchor_of(a: &Args, lib: &[u8; 32], node: &mut N, net: Network) -> R<tb::Anchor> {
    if !a.has("anchor") {
        // No file: an anchor of the checked registry is found at the node (they are all at one address, under the
        // registry's id).
        let path = a.get("registry").ok_or("give --anchor FILE, or --registry REGISTRY-tx.json and --node URL to find an anchor of that registry at the node")?;
        let id = tb::registry_check(&tx_file(path)?, lib).map_err(|x| format!("{path}: {x}"))?;
        println!("registry {}: its genesis writes anchors of this library and nothing else under its id", to_hex(&id));
        let f = found(node, net, TObj::Anchor.script(lib), &id, "anchor of the registry", "anchor")?;
        if (f.amount as i64) < TObj::Anchor.least_value() {
            return Err("the anchor found holds less than an anchor does".into());
        }
        println!("anchor: the one at {}:{}, found at the node", f.op.transaction_id, f.op.index);
        return Ok(OLive { s: (), value: f.amount, id: Some(f.covenant_id), op: f.op, daa: f.daa.unwrap_or(0) });
    }
    let l = load(a, "anchor", node, net)?;
    if !matches!(l.obj, TObj::Anchor) {
        return Err(format!("--anchor is a {}, not an anchor", l.obj.kind()));
    }
    if l.lib != *lib {
        return Err("--anchor is of another library".into());
    }
    match a.get("registry") {
        Some(path) => {
            let id = tb::registry_check(&tx_file(path)?, lib).map_err(|x| format!("{path}: {x}"))?;
            if l.f.covenant_id.as_bytes() != id {
                return Err("REFUSED: --anchor is not of the registry --registry begins".into());
            }
            println!("registry {}: its genesis writes anchors of this library and nothing else under its id", to_hex(&id));
        }
        // Review T1b, F14: as the library's (F4). A registry whose genesis wrote a position beside its anchors holds an
        // position nobody signed, which anyone merges into a real one.
        None if a.has("registry-unchecked") => println!("WARNING: the registry of this anchor ({}) was NOT checked (--registry-unchecked)", l.f.covenant_id),
        None => return Err("REFUSED: the anchor's registry is not checked. Give the file of the registry's genesis transaction (--registry <txid>-tx.json: `registry-create` writes it), which is read and held to anchors and nothing else; or --registry-unchecked, if you checked it yourself".into()),
    }
    live(&l, (), false)
}

/// `lib-check`: reads the genesis transaction of a library and says whether a position can be opened in it; with
/// `--registry`, the genesis of a registry of it as well.
pub fn lib_check_cmd(a: &Args) -> R<()> {
    a.only(&["lib", "lib-unchecked", "registry"])?;
    let lib = lib_of(a)?;
    match fee_pool_terms(a)? {
        Some(of) => println!("oracle fees: every operation leaves {} KAS in the copy of the script it used; they gather in the library's fee pool for the committee {} (a copy keeps {} KAS)", fmt_kas(kpf_protocol::thin::ORACLE_FEE as i128), to_hex(&of.committee), fmt_kas(of.floor as i128)),
        None if !a.has("lib-unchecked") => println!("NOTE: this library has no fee pool: what its operations leave in the copies ({} KAS each) stays there", fmt_kas(kpf_protocol::thin::ORACLE_FEE as i128)),
        None => {}
    }
    if let Some(path) = a.get("registry") {
        let id = tb::registry_check(&tx_file(path)?, &lib).map_err(|x| format!("{path}: {x}"))?;
        println!("registry {}: its genesis writes anchors of this library and nothing else under its id", to_hex(&id));
    }
    Ok(())
}

/// `lib-status`: what of a library (and, with `--registry`, of a registry of it) is at the node this moment: the live copies
/// of each script and the anchors. One query.
pub fn lib_status_cmd(a: &Args) -> R<()> {
    a.only(&["lib", "lib-unchecked", "registry", "node", "network"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lib = lib_of(a)?;
    let registry = match a.get("registry") {
        Some(path) => Some(tb::registry_check(&tx_file(path)?, &lib).map_err(|x| format!("{path}: {x}"))?),
        None => None,
    };
    let Some(n) = &mut node else { return Err("lib-status reads the node: give --node URL".into()) };
    let of = fee_pool_terms(a)?;
    let mut addrs = fee_pool_addrs(of.as_ref(), net);
    let anchor = tb::object_spk(&lib, &TObj::Anchor);
    addrs.push(p2sh_address(&TObj::Anchor.script(&lib), net));
    let list = n.utxos(&addrs)?;
    let mut none = vec![];
    for name in pinned::SCRIPTS {
        let spk = tb::script_spk(name).map_err(e)?;
        let of: Vec<_> = list.iter().filter(|u| u.spk.as_ref() == Some(&spk) && u.covenant_id.map(|c| c.as_bytes()) == Some(lib)).collect();
        println!("{name:<16} {} live, {} KAS", of.len(), fmt_kas(of.iter().map(|u| u.amount as i128).sum()));
        if of.is_empty() {
            none.push(name);
        }
    }
    if let Some(id) = registry {
        let of: Vec<_> = list.iter().filter(|u| u.spk.as_ref() == Some(&anchor) && u.covenant_id.map(|c| c.as_bytes()) == Some(id)).collect();
        println!("anchors of the registry {}: {} live", to_hex(&id), of.len());
    }
    let (copies, fee_pool) = fee_pool_among(&list, &lib, of.as_ref());
    match &of {
        Some(of) => {
            let above: i128 = copies.iter().map(|c| (c.value as i128 - of.floor as i128).max(0)).sum();
            let ready = copies.iter().filter(|c| !pinned::COLLECTORS.contains(&c.s) && kpf_protocol::thin::swept(c.value as i64, of.floor).is_some()).count();
            println!("oracle fees (for the committee {}): {} KAS in the copies above the {} KAS each keeps; {ready} of them hold enough to sweep (fee-sweep)", to_hex(&of.committee), fmt_kas(above), fmt_kas(of.floor as i128));
            match &fee_pool {
                Some(p) => println!("fee pool: {} KAS above its floor (fee-pay shares it among the committee's members)", fmt_kas(p.value as i128 - of.floor as i128)),
                None => println!("fee pool: not at the node this moment (in a transaction not yet in a block)"),
            }
        }
        None => println!("oracle fees: no fee pool is known of this library (its genesis names none, or --lib is a bare id): what operations leave in the copies stays there"),
    }
    if !none.is_empty() {
        println!("NOTE: no live copy this moment of {} (in a transaction not yet in a block, or the library was made without it)", none.join(", "));
    }
    Ok(())
}

/// `registry-create`: a registry of the library `--lib`: `--copies` anchors under one covenant id. Every order and position
/// that enters through them is under that id.
pub fn registry_create_cmd(a: &Args) -> R<()> {
    only(a, &["lib", "lib-unchecked", "copies", "value"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lib = lib_of(a)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let copies = int_or(a, "copies", 4)? as usize;
    let value = if a.has("value") { price(a, "value")? as u64 } else { 100_000_000 };
    let (b, registry) = tb::registry_genesis(&lib, copies, value, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("registry {}: {copies} anchors of the library {}, {} KAS each (an anchor goes on as it is)", to_hex(&registry), to_hex(&lib), fmt_kas(value as i128));
    debug_assert_eq!(tb::registry_check(&b.tx, &lib).ok(), Some(registry));
    let made: Vec<Made> = tb::registry_anchors(&b.tx, &lib, 0).iter().map(|x| Made::Obj(x.op.index as usize, TObj::Anchor)).collect();
    done(a, "registry-create", &b, &keys, net, &mut node, &lib, &made)
}

/// `lib-create`: a library, `--copies` of each script of the family under one covenant id.
pub fn lib_create_cmd(a: &Args) -> R<()> {
    only(a, &["copies", "value", "committee"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let copies = int_or(a, "copies", 2)? as usize;
    let value = if a.has("value") { price(a, "value")? as u64 } else { 1_000_000_000 };
    let committee = match a.get("committee") {
        Some(r) => Some(hex32(r).map_err(|x| format!("--committee: {x} (the id of the committee the library's oracle fees are for)"))?),
        None => None,
    };
    let (b, lib) = tb::lib_genesis_for(copies, value, committee, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    match committee {
        Some(r) => println!("oracle fees: every operation leaves {} KAS in the copy of the script it used; they gather in the library's fee pool for the committee {} (fee-sweep, fee-pay). A copy keeps {} KAS", fmt_kas(kpf_protocol::thin::ORACLE_FEE as i128), to_hex(&r), fmt_kas(value as i128)),
        None => println!("NOTE: no --committee: this library has no fee pool, and what its operations leave in the copies ({} KAS each) stays there for good", fmt_kas(kpf_protocol::thin::ORACLE_FEE as i128)),
    }
    println!("library {}: {copies} copies of {} scripts, {} KAS each (they stay there: a script goes on as it is)", to_hex(&lib), pinned::SCRIPTS.len(), fmt_kas(value as i128));
    let made: Vec<Made> = tb::lib_scripts(&b.tx, 0).iter().map(|s| Made::Script(s.op.index as usize, s.s)).collect();
    debug_assert_eq!(tb::lib_check(&b.tx).ok(), Some(lib));
    done(a, "lib-create", &b, &keys, net, &mut node, &lib, &made)
}

/// What the genesis `--lib` names says of its fee pool (none: a bare id, or a library without one).
pub(crate) fn fee_pool_terms(a: &Args) -> R<Option<tb::FeePoolOf>> {
    let v = a.need("lib")?;
    if v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    Ok(tb::genesis_fee_pool(&tx_file(v)?))
}

/// The addresses of a library's scripts, and of its fee pool.
pub(crate) fn fee_pool_addrs(of: Option<&tb::FeePoolOf>, net: Network) -> Vec<String> {
    let mut v: Vec<String> = pinned::SCRIPTS.iter().map(|s| p2sh_address(pinned::kit().script(s).expect("a script"), net)).collect();
    v.extend(of.map(|of| p2sh_address(&of.script(), net)));
    v
}

/// The library's copies and its fee pool among the UTXOs a node listed.
pub(crate) fn fee_pool_among(list: &[crate::files::ListedUtxo], lib: &[u8; 32], of: Option<&tb::FeePoolOf>) -> (Vec<LibScript>, Option<tb::FeePool>) {
    let id = Hash::from_bytes(*lib);
    let mut copies = vec![];
    for name in pinned::SCRIPTS {
        let Ok(spk) = tb::script_spk(name) else { continue };
        copies.extend(list.iter().filter(|u| u.spk.as_ref() == Some(&spk) && u.covenant_id == Some(id)).map(|u| OLive { s: name, value: u.amount, id: Some(id), op: u.op, daa: u.daa }));
    }
    let fee_pool = of.and_then(|of| {
        let spk = tb::fee_pool_spk(of);
        list.iter().find(|u| u.spk.as_ref() == Some(&spk) && u.covenant_id == Some(id)).map(|u| OLive { s: *of, value: u.amount, id: Some(id), op: u.op, daa: u.daa })
    });
    (copies, fee_pool)
}

/// The copies and the fee pool of the library at the node this moment: one query.
fn fee_pool_now(a: &Args, node: &mut N, net: Network, lib: &[u8; 32]) -> R<(Vec<LibScript>, tb::FeePool)> {
    let of = fee_pool_terms(a)?.ok_or("this library has no fee pool: its genesis names no committee, and what its operations leave in the copies stays there (or --lib is a bare id: give the file of the genesis transaction)")?;
    let n = node.as_mut().ok_or("the copies and the fee pool are read at the node: give --node URL")?;
    let list = n.utxos(&fee_pool_addrs(Some(&of), net))?;
    let (copies, fee_pool) = fee_pool_among(&list, lib, Some(&of));
    let fee_pool = fee_pool.ok_or("the library's fee pool is not at the node this moment (in a transaction not yet in a block): ask again")?;
    Ok((copies, fee_pool))
}

/// `fee-sweep`: what gathered in a copy of a script of the library (the one that holds the most, or a copy of
/// `--copy NAME`) moves to the library's fee pool; the sender keeps one part in twenty. Anyone.
pub fn fee_sweep_cmd(a: &Args) -> R<()> {
    only(a, &["lib", "lib-unchecked", "copy"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lib = lib_of(a)?;
    let (copies, fee_pool) = fee_pool_now(a, &mut node, net, &lib)?;
    let sweeper = copies.iter().find(|c| c.s == "KpfTSweep").ok_or("no copy of KpfTSweep is at the node this moment: ask again")?;
    let want = a.get("copy");
    if let Some(w) = want {
        if !pinned::SCRIPTS.contains(&w) || pinned::COLLECTORS.contains(&w) {
            return Err(format!("--copy {w}: a script of the library that takes oracle fees (the two that collect take none)"));
        }
    }
    let best = copies.iter().filter(|c| !pinned::COLLECTORS.contains(&c.s) && want.is_none_or(|w| w == c.s)).max_by_key(|c| c.value).ok_or("no copy of that script is at the node this moment: ask again")?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, moved, to_fee_pool) = tb::sweep(&lib, &fee_pool, sweeper, best, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("swept: {} KAS out of a copy of {}: {} KAS to the fee pool (which then holds {} KAS above its floor), {} KAS the sender's before the fee", fmt_kas(moved as i128), best.s, fmt_kas(to_fee_pool as i128), fmt_kas(fee_pool.value as i128 + to_fee_pool as i128 - fee_pool.s.floor as i128), fmt_kas((moved - to_fee_pool) as i128));
    done(a, "fee-sweep", &b, &keys, net, &mut node, &lib, &[Made::Script(1, "KpfTSweep"), Made::Script(2, best.s)])
}

/// `fee-pay`: the library's fee pool is paid to the members of its committee, each by its weight; the sender keeps one part
/// in twenty. `--epoch-table` is an epoch table of the committee and `--signed-price` (once a member) the signed prices of any one time of
/// its dispute period a quorum signed: the epoch table's member table is who is paid. Anyone.
pub fn fee_pay_cmd(a: &Args) -> R<()> {
    only(a, &["lib", "lib-unchecked", "epoch-table", "signed-price", "t0"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lib = lib_of(a)?;
    let (copies, fee_pool) = fee_pool_now(a, &mut node, net, &lib)?;
    let payer_script = copies.iter().find(|c| c.s == "KpfTPay").ok_or("no copy of KpfTPay is at the node this moment: ask again")?;
    let epoch_table = crate::ccmds::epoch_table_of(a, "epoch-table", &mut node, net)?;
    let present = now(a, &mut node)?;
    if (present as i64) < epoch_table.s.until {
        return Err(format!("the epoch table is of the epoch [{}, {}), which has not ended (the present is DAA {present}): the fee pool is paid to the members of an epoch that has ended. Give an epoch table of the epoch before, or pay after DAA {}", epoch_table.s.from, epoch_table.s.until, epoch_table.s.until));
    }
    let files = a.all("signed-price");
    if files.is_empty() {
        return Err("give the members' signed prices of one time: --signed-price FILE (once a member)".into());
    }
    let raw: Vec<_> = files.iter().map(|f| crate::ccmds::price_raw(f)).collect::<R<_>>()?;
    let market = raw[0].2;
    if raw.iter().any(|t| t.1 != fee_pool.s.committee || t.2 != market) {
        return Err("REFUSED: the signed prices are not all of the committee this library's oracle fees are for and of one market".into());
    }
    let signed_prices: Vec<kpf_protocol::committee::SignedPrice> = raw.iter().map(|t| t.0).collect();
    let t0 = int_or(a, "t0", signed_prices[0].t0)?;
    let att = kpf_protocol::committee::Attestation::collect(&epoch_table.s, &fee_pool.s.committee, &market, t0, &signed_prices).map_err(|x| format!("no quorum for that time: {x} (the epoch table serves signed prices [{}, {}))", epoch_table.s.from, epoch_table.s.until))?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    // One coin pays: the payment's transaction holds the fee pool, the script, the epoch table and a coin, and no more.
    let coin = fu.iter().max_by_key(|u| u.amount).cloned().ok_or("the funder has no coin")?;
    let (b, shares) = tb::pay(&lib, &fee_pool, payer_script, (&epoch_table, &att), Payer { funds: std::slice::from_ref(&coin), change_to: fk, rate: rate(a)? }).map_err(e)?;
    let paid: i64 = shares.iter().map(|x| x.1).sum();
    for (key, share) in &shares {
        println!("member {}: {} KAS", p2pk_address(key, net), fmt_kas(*share as i128));
    }
    let pot = (fee_pool.value as i64 - fee_pool.s.floor).min(kpf_protocol::thin::ORACLE_FEE_POT_MAX);
    println!("paid: {} KAS to {} members by their weights; {} KAS the sender's before the fee; {} KAS stay in the fee pool", fmt_kas(paid as i128), shares.len(), fmt_kas((pot / kpf_protocol::thin::ORACLE_FEE_CUT) as i128), fmt_kas((fee_pool.value as i64 - fee_pool.s.floor - paid - pot / kpf_protocol::thin::ORACLE_FEE_CUT) as i128));
    done(a, "fee-pay", &b, &keys, net, &mut node, &lib, &[Made::Script(1, "KpfTPay")])
}

/// `pos-open`: a position between `--a` and `--b` in the library `--lib`, through `--anchor` (an anchor of its registry)
/// and `--script` (a copy of `KpfTOpen`); both parties sign (`--key`). It is with the flags of `x-open` for its legs and
/// margins. The funder pays A's margin, the carrier and the fee; `--b-funding` (the counterparty's listed UTXOs, its
/// key among `--key`) pays B's margin, else the funder pays that too. The position's state is published in the
/// transaction unless `--private`.
pub fn open_cmd(a: &Args) -> R<()> {
    only(a, &["market1", "size1", "dir1", "x1", "market2", "size2", "dir2", "x2", "a", "b", "a-margin", "b-margin", "lib", "lib-unchecked", "dispute-daa", "bond-bps", "exit-daa", "b-funding", "private", "a-min-counter-margin", "b-min-counter-margin", "anchor", "script", "registry", "registry-unchecked"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lib = lib_of(a)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let t = now(a, &mut node)? as i64;
    let mut x = crate::ccmds::cross_from_args(a, net, t)?;
    // The thin family has no lease: the field holds the time an exit claim waits (a day by default).
    x.expiry = int_or(a, "exit-daa", 864_000)?;
    // What each party asks of whoever takes the other's place later: by default what the other posts now (review
    // T1b, F12: a position that asks nothing has its counterparty replaced by one without margin).
    let min_counter = |n: &str, other: i64| -> R<i64> { if a.has(n) { price(a, n) } else { Ok(other) } };
    let position = Position { x, dispute_daa: int_or(a, "dispute-daa", 600)?, bond_bps: int_or(a, "bond-bps", 1_000)?, a_min_counter: min_counter("a-min-counter-margin", x.b_margin)?, b_min_counter: min_counter("b-min-counter-margin", x.a_margin)? };
    let bf = match a.get("b-funding") {
        Some(p) => funding_utxos(&parse_utxos(&read_json(p)?)?, &x.b_key),
        None => vec![],
    };
    if a.has("b-funding") && bf.is_empty() {
        return Err(format!("--b-funding lists no plain UTXO of the counterparty {}", p2pk_address(&x.b_key, net)));
    }
    let anchor = anchor_of(a, &lib, &mut node, net)?;
    let s = script_of(a, &lib, "KpfTOpen", &mut node, net)?;
    let r = rate(a)?;
    let mk = |f: Sw| tb::open(&lib, &anchor, &s, &sw_position(position, f), &sw_funds(&bf, f), &f(x.b_key), !a.has("private"), Payer { funds: &sw_funds(&fu, f), change_to: f(fk), rate: r }).map_err(e);
    show(&TObj::Position(position), &lib, None, net);
    println!("{}", if a.has("private") { "the state is NOT published: only who holds this file can test a claim on the position" } else { "the state is published in the transaction: anybody can test a claim on the position" });
    done_by(a, "pos-open", &mk, &keys, net, &mut node, &lib, &[Made::Obj(0, TObj::Position(position)), Made::Script(1, "KpfTOpen"), Made::Obj(2, TObj::Anchor)], &[TObj::Anchor])
}

/// `pos-show`: a position, a claim or a disputed claim, and for a claim whether what it states is the rule's.
pub fn show_cmd(a: &Args) -> R<()> {
    a.only(&["object", "node", "network", "lib", "lib-unchecked", "registry"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load(a, "object", &mut node, net)?;
    show(&l.obj, &l.lib, l.f.daa, net);
    println!("holds {} KAS", fmt_kas(l.f.amount as i128));
    // A file that came from somebody else (the position a match made of your order, say): is it of the library and
    // the registry you checked, and is it there. With a node the UTXO, its value and its covenant id are the node's.
    if node.is_some() {
        println!("at the node: {}:{}, under the covenant id {}", l.f.op.transaction_id, l.f.op.index, l.f.covenant_id);
    }
    if a.has("lib") && lib_of(a)? != l.lib {
        return Err("REFUSED: the object is not of the library --lib names".into());
    }
    if let Some(path) = a.get("registry") {
        let id = tb::registry_check(&tx_file(path)?, &l.lib).map_err(|x| format!("{path}: {x}"))?;
        if l.f.covenant_id.as_bytes() != id {
            return Err(format!("REFUSED: the object is under the covenant id {}, not in the registry {} that --registry begins: nothing holds what is beside that registry to the family's rules", l.f.covenant_id, to_hex(&id)));
        }
        println!("registry {}: the object is in the registry --registry begins{}", to_hex(&id), if node.is_some() { " (its covenant id read from the node)" } else { " BY THE FILE (no --node: not compared with the chain)" });
    }
    Ok(())
}

/// `pos-add-margin`: the owner of `--side` adds `--amount` to its margin.
pub fn add_cmd(a: &Args) -> R<()> {
    only(a, &["position", "script", "side", "amount"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = position_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTAdd", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, next) = tb::add(&l.lib, &live(&l, x, false)?, &s, side_of(a.need("side")?)?, price(a, "amount")?, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("margin: a {} KAS, b {} KAS", fmt_kas(next.x.a_margin as i128), fmt_kas(next.x.b_margin as i128));
    done(a, "pos-add-margin", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Position(next)), Made::Script(1, "KpfTAdd")])
}

/// `pos-close`: `--side` states what it is owed at the prices `--p1` `--p2` of the time `--t0` and posts the bond.
/// The amount is what the rule gives at the prices stated, counted the tolerance worse for the claimer; `--amount` states
/// another (more than the committee's prices give, and a dispute refutes it).
pub fn close_cmd(a: &Args) -> R<()> {
    only(a, &["position", "script", "side", "t0", "p1", "p2", "amount", "epoch-table1", "signed-price1", "epoch-table2", "signed-price2"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = position_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTClose", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (t0, prices) = stated_price(a, &x, &mut node, net)?;
    let amount = if a.has("amount") { Some(price(a, "amount")?) } else { None };
    let (b, claim) = tb::close(&l.lib, &live(&l, x, false)?, &s, side_of(a.need("side")?)?, t0, prices, amount, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&TObj::Claim(claim), &l.lib, None, net);
    println!("settled as stated once it is {} DAA old, unless somebody disputes it", x.dispute_daa);
    done(a, "pos-close", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Claim(claim)), Made::Script(1, "KpfTClose")])
}

/// `pos-settle`: anyone settles a claim or a liquidation nobody disputed (or a proved liquidation) that landed in
/// time, once it is a dispute period old.
pub fn settle_cmd(a: &Args) -> R<()> {
    only(a, &["claim", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load(a, "claim", &mut node, net)?;
    match l.obj {
        TObj::Claim(c) => {
            let s = script_of(a, &l.lib, "KpfTSettle", &mut node, net)?;
            let (fk, keys, fu) = funding(a, net, &mut node)?;
            let b = tb::settle(&l.lib, &live(&l, c, true)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
            let (pa, pb) = c.settle_pay(l.f.amount as i64);
            println!("pays a {} KAS, b {} KAS", fmt_kas(pa as i128), fmt_kas(pb as i128));
            done(a, "pos-settle", &b, &keys, net, &mut node, &l.lib, &[Made::Script(1, "KpfTSettle")])
        }
        TObj::Liq(q) => {
            let s = script_of(a, &l.lib, "KpfTLiqSettle", &mut node, net)?;
            let (fk, keys, fu) = funding(a, net, &mut node)?;
            let b = tb::liq_settle(&l.lib, &live(&l, q, q.state == LiqState::Claimed)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
            let (mine, rest) = q.liq.settle_pay(l.f.amount as i64);
            println!("pays the liquidator {} KAS, the other party {} KAS", fmt_kas(mine as i128), fmt_kas(rest as i128));
            done(a, "pos-settle", &b, &keys, net, &mut node, &l.lib, &[Made::Script(1, "KpfTLiqSettle")])
        }
        TObj::ExitClaim(q) => {
            let s = script_of(a, &l.lib, "KpfTExitEnd", &mut node, net)?;
            let (fk, keys, fu) = funding(a, net, &mut node)?;
            let b = tb::exit_settle(&l.lib, &live(&l, q, true)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
            let (pa, pb) = q.settle_pay(l.f.amount as i64);
            println!("settled at the entries: pays a {} KAS, b {} KAS", fmt_kas(pa as i128), fmt_kas(pb as i128));
            done(a, "pos-settle", &b, &keys, net, &mut node, &l.lib, &[Made::Script(1, "KpfTExitEnd")])
        }
        TObj::Disputed(_) => Err("--claim is disputed: it is decided with the committee's prices (`pos-prove`, `pos-refute`) or ended with `pos-lapse`".into()),
        o => Err(format!("--claim is a {}, not a claim", o.kind())),
    }
}

/// `pos-late`: the funder ends a claim or a liquidation that landed later than lateDaa after its time, or before
/// it (and an exit claim of a time to come): the position again, the claim's bond shared by the funder and the
/// party the claim was made against.
pub fn late_cmd(a: &Args) -> R<()> {
    only(a, &["claim", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load(a, "claim", &mut node, net)?;
    let name = match l.obj {
        TObj::Claim(_) => "KpfTLate",
        TObj::Liq(_) => "KpfTLiq",
        TObj::ExitClaim(_) => "KpfTExit",
        o => return Err(format!("--claim is a {}, not a claim nobody disputed", o.kind())),
    };
    let s = script_of(a, &l.lib, name, &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let payer = Payer { funds: &fu, change_to: fk, rate: rate(a)? };
    let (b, back) = match l.obj {
        TObj::Claim(c) => tb::late(&l.lib, &live(&l, c, true)?, &s, fk, payer).map_err(e)?,
        TObj::Liq(q) => tb::liq_late(&l.lib, &live(&l, q, true)?, &s, fk, payer).map_err(e)?,
        TObj::ExitClaim(q) => tb::exit_early(&l.lib, &live(&l, q, true)?, &s, fk, payer).map_err(e)?,
        _ => unreachable!("named above"),
    };
    done(a, "pos-late", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Position(back)), Made::Script(1, name)])
}

/// `pos-dispute`: the funder (or `--disputer`) asks for the proof of a claim or a liquidation and posts a bond as
/// large as its own.
pub fn dispute_cmd(a: &Args) -> R<()> {
    only(a, &["claim", "script", "disputer"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load(a, "claim", &mut node, net)?;
    let (name, truth, bond, dispute_daa) = match l.obj {
        TObj::Claim(c) => ("KpfTDispute", c.is_true(), c.bond, c.position.dispute_daa),
        TObj::Liq(q) => ("KpfTLiqDispute", q.liq.is_true(), q.liq.bond, q.liq.position.dispute_daa),
        TObj::Disputed(_) => return Err("--claim is disputed already".into()),
        o => return Err(format!("--claim is a {}, not a claim", o.kind())),
    };
    let s = script_of(a, &l.lib, name, &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let who = match a.get("disputer") {
        Some(k) => crate::keys::party(k, net)?,
        None => fk,
    };
    if truth {
        println!("WARNING: what this claim states is what the rule gives at its prices. If the committee's prices for that time are no worse for the claimer, it is proved and this bond of {} KAS is lost", fmt_kas(bond as i128));
    }
    let payer = Payer { funds: &fu, change_to: fk, rate: rate(a)? };
    let (b, next) = match l.obj {
        TObj::Claim(c) => {
            let (b, d) = tb::dispute(&l.lib, &live(&l, c, true)?, &s, who, payer).map_err(e)?;
            (b, TObj::Disputed(d))
        }
        TObj::Liq(q) => {
            let (b, d) = tb::liq_dispute(&l.lib, &live(&l, q, true)?, &s, who, payer).map_err(e)?;
            (b, TObj::Liq(d))
        }
        _ => unreachable!("named above"),
    };
    println!("disputed by {} with a bond of {} KAS; ended with `pos-lapse` once it is {dispute_daa} DAA old, unless proved", p2pk_address(&who, net), fmt_kas(bond as i128));
    done(a, "pos-dispute", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, next), Made::Script(1, name)])
}

/// `pos-lapse`: anyone ends a disputed claim or liquidation nobody proved within the dispute period.
pub fn lapse_cmd(a: &Args) -> R<()> {
    only(a, &["claim", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load(a, "claim", &mut node, net)?;
    let (name, pay) = match l.obj {
        TObj::Disputed(d) => ("KpfTDispute", d.lapse_pay(l.f.amount as i64)),
        TObj::Liq(q) if matches!(q.state, LiqState::Disputed(_)) => ("KpfTLiqDispute", q.lapse_pay(l.f.amount as i64)),
        o => return Err(format!("--claim is a {}, not a disputed claim", o.kind())),
    };
    let s = script_of(a, &l.lib, name, &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let payer = Payer { funds: &fu, change_to: fk, rate: rate(a)? };
    let (b, back) = match l.obj {
        TObj::Disputed(d) => tb::lapse(&l.lib, &live(&l, d, false)?, &s, payer).map_err(e)?,
        TObj::Liq(q) => tb::liq_lapse(&l.lib, &live(&l, q, false)?, &s, payer).map_err(e)?,
        _ => unreachable!("named above"),
    };
    if let Some((_, to_d, to_o)) = pay {
        if matches!(l.obj, TObj::Liq(_)) {
            println!("the position again; nobody showed the committee's prices for the liquidation or against it, and nothing is decided: the disputer has its bond of {} KAS back and the liquidator its {} KAS", fmt_kas(to_d as i128), fmt_kas(to_o as i128));
        } else {
            println!("the position again; nobody showed the committee's price for the claim or against it, and nothing is decided: the disputer has its bond of {} KAS back and the claimer its {} KAS", fmt_kas(to_d as i128), fmt_kas(to_o as i128));
        }
    }
    done(a, "pos-lapse", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Position(back)), Made::Script(1, name)])
}

/// `pos-refute` of a disputed claim: anyone answers a dispute against the claim, with the committee's signed prices for the claim's
/// time (`--epoch-table1 --signed-price1 ...`, and the second leg's): a price the claim states is further than twice
/// the position's tolerance from the committee's. The position again; the disputer has its bond back, the funder
/// half of the claimer's, and the other half stays in the library's script.
fn refute_disputed(a: &Args) -> R<()> {
    only(a, &["claim", "script", "epoch-table1", "signed-price1", "epoch-table2", "signed-price2", "t0"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load(a, "claim", &mut node, net)?;
    let (x, name) = match l.obj {
        TObj::Disputed(d) => (d.claim.position, "KpfTProve"),
        TObj::Liq(q) if matches!(q.state, LiqState::Disputed(_)) => (q.liq.position, "KpfTLiqProve"),
        o => return Err(format!("--claim is a {}, not a disputed claim", o.kind())),
    };
    let s = script_of(a, &l.lib, name, &mut node, net)?;
    let t1 = crate::ccmds::x_att(a, 1, &x.x, &mut node, net)?;
    let t2 = if x.x.legs[1].size > 0 { Some(crate::ccmds::x_att(a, 2, &x.x, &mut node, net)?) } else { None };
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let second = t2.as_ref().map(|t| (&t.0, &t.1));
    let payer = Payer { funds: &fu, change_to: fk, rate: rate(a)? };
    let ((b, back), pay) = match l.obj {
        TObj::Disputed(d) => (tb::refute(&l.lib, &live(&l, d, false)?, &s, (&t1.0, &t1.1), second, fk, payer).map_err(e)?, d.refute_pay(l.f.amount as i64)),
        TObj::Liq(q) => (tb::liq_refute(&l.lib, &live(&l, q, false)?, &s, (&t1.0, &t1.1), second, fk, payer).map_err(e)?, q.refute_pay(l.f.amount as i64)),
        _ => unreachable!("named above"),
    };
    if let Some((_, to_d, to_r, kept)) = pay {
        println!("refuted: the position again; the disputer has its bond of {} KAS back, the funder {} KAS (half of the bond of who claimed), and {} KAS stay in the library's script: the committee's", fmt_kas(to_d as i128), fmt_kas(to_r as i128), fmt_kas(kept as i128));
    }
    done(a, "pos-refute", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Position(back)), Made::Script(1, name)])
}

/// `pos-liquidate`: the funder (or `--liquidator`) states that `--side` is below its maintenance at the prices
/// `--p1` `--p2` of the time `--t0`, and posts the bond. The fee and the cure are what the rule gives; `--fee` and
/// `--add` state others (outside the rule at the committee's prices, and a dispute refutes them).
pub fn liquidate_cmd(a: &Args) -> R<()> {
    only(a, &["position", "script", "side", "t0", "p1", "p2", "fee", "add", "liquidator", "epoch-table1", "signed-price1", "epoch-table2", "signed-price2"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = position_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTLiq", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let who = match a.get("liquidator") {
        Some(k) => crate::keys::party(k, net)?,
        None => fk,
    };
    let (t0, prices) = stated_price(a, &x, &mut node, net)?;
    let stated = match (a.has("fee"), a.has("add")) {
        // Stated from prices of the liquidator's own, the fee and the cure are those that stay within the rule
        // wherever the committee's prices are no further than the position's tolerance from them.
        (false, false) if !a.has("epoch-table1") => {
            let own = x.liquidation_own(side_of(a.need("side")?)?, t0, prices, who).ok_or("REFUSED: the rule does not liquidate this side even at these prices moved by the position's tolerance toward it. Stated from a price of the liquidator's own, the liquidation would be refuted wherever the committee's price is a little better for the side (--fee and --add state it all the same)")?;
            Some((own.fee, own.add))
        }
        (false, false) => None,
        (true, true) => Some((price(a, "fee")?, price(a, "add")?)),
        _ => return Err("--fee and --add are stated together, or neither (the rule's)".into()),
    };
    let (b, q) = tb::liquidate(&l.lib, &live(&l, x, false)?, &s, side_of(a.need("side")?)?, t0, prices, stated, who, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&TObj::Liq(q), &l.lib, None, net);
    println!("settled once it is {} DAA old, unless the side cures it or somebody disputes it", x.dispute_daa);
    done(a, "pos-liquidate", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Liq(q)), Made::Script(1, "KpfTLiq")])
}

/// `pos-cure`: the side a liquidation names adds what it asks and has its position again. Its key signs. A
/// liquidation nobody disputed is cured once it is half a dispute period old, a proved one at any time, a disputed one not
/// at all (its liquidator proves it first, or it lapses).
pub fn cure_cmd(a: &Args) -> R<()> {
    only(a, &["claim", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load(a, "claim", &mut node, net)?;
    let TObj::Liq(q) = l.obj else { return Err(format!("--claim is a {}, not a liquidation", l.obj.kind())) };
    let s = script_of(a, &l.lib, "KpfTCure", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, next) = tb::cure(&l.lib, &live(&l, q, false)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("cured with {} KAS: margin a {} KAS, b {} KAS", fmt_kas(q.liq.add as i128), fmt_kas(next.x.a_margin as i128), fmt_kas(next.x.b_margin as i128));
    done(a, "pos-cure", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Position(next)), Made::Script(1, "KpfTCure")])
}

/// `pos-close --mutual`: the two parties end the position, `--to-a` to A and the rest (or `--to-b`) to B. Both keys sign.
pub fn close_both_cmd(a: &Args) -> R<()> {
    only(a, &["position", "script", "to-a", "to-b", "mutual"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = position_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTBoth", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let to_a = price(a, "to-a")?;
    let to_b = if a.has("to-b") { price(a, "to-b")? } else { l.f.amount as i64 - to_a };
    let (lx, r) = (live(&l, x, false)?, rate(a)?);
    let mk = |f: Sw| tb::both_close(&l.lib, &OLive { s: sw_position(x, f), ..lx }, &s, to_a, to_b, Payer { funds: &sw_funds(&fu, f), change_to: f(fk), rate: r }).map_err(e);
    println!("pays a {} KAS, b {} KAS", fmt_kas(to_a as i128), fmt_kas(to_b as i128));
    done_by(a, "pos-close --mutual", &mk, &keys, net, &mut node, &l.lib, &[Made::Script(1, "KpfTBoth")], &[TObj::Position(x)])
}

/// `pos-prove`: the claimer of a disputed claim, or the liquidator of a disputed liquidation, answers with the
/// committee's signatures: an epoch table of each leg's committee (`--epoch-table1`, `--epoch-table2`) and the members' signed prices of
/// the claim's signed price (`--signed-price1 FILE` once a member, `--signed-price2 ...`). A proved claim is paid at once (its claimer the
/// amount and both bonds); a proved liquidation waits a dispute period again and its liquidator has the disputer's bond.
pub fn prove_cmd(a: &Args) -> R<()> {
    only(a, &["claim", "script", "epoch-table1", "signed-price1", "epoch-table2", "signed-price2", "t0"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load(a, "claim", &mut node, net)?;
    let (position, name) = match l.obj {
        TObj::Disputed(d) => (d.claim.position, "KpfTProve"),
        TObj::Liq(q) if matches!(q.state, LiqState::Disputed(_)) => (q.liq.position, "KpfTLiqProve"),
        o => return Err(format!("--claim is a {}, not a disputed claim", o.kind())),
    };
    let s = script_of(a, &l.lib, name, &mut node, net)?;
    let t1 = crate::ccmds::x_att(a, 1, &position.x, &mut node, net)?;
    let t2 = if position.x.legs[1].size > 0 { Some(crate::ccmds::x_att(a, 2, &position.x, &mut node, net)?) } else { None };
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let payer = Payer { funds: &fu, change_to: fk, rate: rate(a)? };
    let second = t2.as_ref().map(|t| (&t.0, &t.1));
    match l.obj {
        TObj::Disputed(d) => {
            let b = tb::prove(&l.lib, &live(&l, d, false)?, &s, (&t1.0, &t1.1), second, payer).map_err(e)?;
            let (pa, pb, kept) = d.prove_pay(l.f.amount as i64);
            println!("proved: pays a {} KAS, b {} KAS (the claimer has its bond and half of the disputer's; {} KAS, the other half, stays in the library's script: the committee's)", fmt_kas(pa as i128), fmt_kas(pb as i128), fmt_kas(kept as i128));
            done(a, "pos-prove", &b, &keys, net, &mut node, &l.lib, &[Made::Script(1, name)])
        }
        TObj::Liq(q) => {
            let (b, next) = tb::liq_prove(&l.lib, &live(&l, q, false)?, &s, (&t1.0, &t1.1), second, payer).map_err(e)?;
            let (to_l, kept) = q.prove_pay();
            println!("proved: the liquidator has half of the disputer's bond, {} KAS ({} KAS, the other half, stays in the library's script: the committee's); settled once it is {} DAA old, unless the side cures", fmt_kas(to_l as i128), fmt_kas(kept as i128), position.dispute_daa);
            done(a, "pos-prove", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Liq(next)), Made::Script(1, name)])
        }
        _ => unreachable!("named above"),
    }
}

/// `pos-exit`: the owner of `--side` states that the committee signed no price of one of the position's markets since
/// `--since` (default: now), and posts the bond. The way out of a market whose committee stopped: settled at the
/// entries after the position's exit time, unless somebody shows the prices (`pos-refute`).
pub fn exit_cmd(a: &Args) -> R<()> {
    only(a, &["position", "script", "side", "since"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = position_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTExit", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let since = match a.get("since") {
        Some(v) => v.parse::<i64>().map_err(|_| "--since is a DAA, an integer".to_string())?,
        None => now(a, &mut node)? as i64,
    };
    let (b, q) = tb::exit(&l.lib, &live(&l, x, false)?, &s, side_of(a.need("side")?)?, since, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&TObj::ExitClaim(q), &l.lib, None, net);
    done(a, "pos-exit", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::ExitClaim(q)), Made::Script(1, "KpfTExit")])
}

/// `pos-refute`: the funder (or `--refuter`) ends an exit claim with the prices it denies: an epoch table of each leg's
/// committee and the members' signed prices of a signed price at or after the claim's `since` (`--epoch-table1 --signed-price1 ...`,
/// `--epoch-table2 --signed-price2 ...`, `--t0` if the files hold several signed prices).
pub fn refute_cmd(a: &Args) -> R<()> {
    only(a, &["claim", "script", "epoch-table1", "signed-price1", "epoch-table2", "signed-price2", "t0", "refuter"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let l = load(a, "claim", &mut node, net)?;
    // (A disputed claim is refuted by the committee's price for its time: the same command, the other script.)
    if matches!(l.obj, TObj::Disputed(_)) || matches!(l.obj, TObj::Liq(q) if matches!(q.state, LiqState::Disputed(_))) {
        return refute_disputed(a);
    }
    let TObj::ExitClaim(q) = l.obj else { return Err(format!("--claim is a {}, not an exit claim or a disputed claim", l.obj.kind())) };
    let s = script_of(a, &l.lib, "KpfTExitRefute", &mut node, net)?;
    let t1 = crate::ccmds::x_att(a, 1, &q.position.x, &mut node, net)?;
    let t2 = if q.position.x.legs[1].size > 0 { Some(crate::ccmds::x_att(a, 2, &q.position.x, &mut node, net)?) } else { None };
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let who = match a.get("refuter") {
        Some(k) => crate::keys::party(k, net)?,
        None => fk,
    };
    let (b, back) = tb::exit_refute(&l.lib, &live(&l, q, false)?, &s, (&t1.0, &t1.1), t2.as_ref().map(|t| (&t.0, &t.1)), who, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    if let Some((_, to_e, to_o)) = q.end_pay(l.f.amount as i64) {
        println!("ended: the committee signed after DAA {}. The position again; {} KAS to {}, {} KAS to the other party", q.since, fmt_kas(to_e as i128), p2pk_address(&who, net), fmt_kas(to_o as i128));
    }
    done(a, "pos-refute", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Position(back)), Made::Script(1, "KpfTExitRefute")])
}

/// An order read from its file, with the template the file carries (checked against the order's hash of it).
fn order_of(a: &Args, flag: &str, node: &mut N, net: Network) -> R<(Loaded, Order, Template)> {
    let l = load(a, flag, node, net)?;
    let TObj::Order(o) = l.obj else { return Err(format!("--{flag} is a {}, not an order", l.obj.kind())) };
    let path = a.need(flag)?;
    let text = read_json(path)?;
    let hex = text.get("template").and_then(|x| x.as_str()).ok_or_else(|| format!("{path}: no `template`"))?;
    let t = Template::from_bytes(&from_hex(hex).map_err(e)?).map_err(|x| format!("{path}: {x}"))?;
    if t.hash() != o.tpl {
        return Err(format!("{path}: the template in the file is not the one the order carries"));
    }
    Ok((l, o, t))
}

/// `order-place`: an order of the funder (or `--owner`) in the library `--lib`: long or short `--size` units of the
/// market `--market1` at `--limit` or better, with `--margin` and a `--tip` for whoever matches it, until `--expiry`.
/// The position it would become takes the market's terms and `--dispute-daa`, `--bond-bps`, `--exit-daa`; an order matches
/// only one of the same market and the same values. What a match leaves rests, unless `--no-rest`; a match takes at
/// least `--min-fill` units of it (or all that is left; by default a tenth of it, or all of an order that does not rest),
/// and it asks `--min-counter-margin` of margin of the other side (by default as much as its own).
pub fn order_place_cmd(a: &Args) -> R<()> {
    only(a, &["lib", "lib-unchecked", "market1", "side", "size", "limit", "margin", "tip", "expiry", "owner", "dispute-daa", "bond-bps", "exit-daa", "no-rest", "min-fill", "min-counter-margin", "anchor", "script", "registry", "registry-unchecked"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let lib = lib_of(a)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (t, market, committee, scale, price_max) = crate::ccmds::market_parts(a.need("market1")?)?;
    let tpl = Template { t, market, committee, scale, price_max, exit: int_or(a, "exit-daa", 864_000)?, dispute_daa: int_or(a, "dispute-daa", 600)?, bond_bps: int_or(a, "bond-bps", 1_000)? };
    let owner = match a.get("owner") {
        Some(k) => crate::keys::party(k, net)?,
        None => fk,
    };
    let size = crate::args::parse_amount(a.need("size")?, scale)?;
    // `--no-rest`: taken whole or not at all (its least fill is its size).
    let rest = !a.has("no-rest");
    let margin = price(a, "margin")?;
    let o = Order {
        tpl: tpl.hash(),
        owner,
        dir: crate::cmds::side(a.need("side")?)?,
        size,
        limit: price(a, "limit")?,
        margin,
        tip: if a.has("tip") { price(a, "tip")? } else { 0 },
        expiry: a.need("expiry")?.parse::<i64>().map_err(|_| "--expiry is a DAA, an integer".to_string())?,
        // Review T1b, F8: without a least fill an order is cut up, or ended, by a fill of one unit. By default an
        // order that does not rest is taken whole and one that rests a tenth at a time.
        min_fill: if a.has("min-fill") { crate::args::parse_amount(a.need("min-fill")?, scale)? } else if rest { (size / 10).max(1) } else { size },
        // Review T1b, F6: what the order asks the other side to post. By default as much as its owner posts.
        min_counter: if a.has("min-counter-margin") { price(a, "min-counter-margin")? } else { margin },
    };
    let anchor = anchor_of(a, &lib, &mut node, net)?;
    let s = script_of(a, &lib, "KpfTPlace", &mut node, net)?;
    let b = tb::order_place(&lib, &anchor, &s, &o, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&TObj::Order(o), &lib, None, net);
    if o.tip == 0 {
        println!("NOTE: the order carries no tip. Whoever matches it pays the match's fee (about 0.0072 KAS) and keeps the two orders' tips: an executor that looks for a gain (`exec-run`) leaves alone a match whose tips do not cover it");
    }
    // Review T1b, F13: anyone merges the positions of the same two users, and takes a user out of two that cancel.
    println!("NOTE: the margin of this order is not the most it can lose. Its position is one pot with your other positions of this market against the same counterparty once anyone merges them, and with an opposite position of yours once anyone takes you out of the two");
    done(a, "order-place", &b, &keys, net, &mut node, &lib, &[Made::OrderOf(0, o, tpl), Made::Script(1, "KpfTPlace"), Made::Obj(2, TObj::Anchor)])
}

/// `order-match`: the funder matches the long order `--long` and the short order `--short` for `--size` units at
/// `--price`: a position between their owners; the two tips' shares stay in the funder's change. With `--into
/// POSITION --leaver a|b` the match is made into that position: its party a or b leaves by its own order (a by the
/// short one, b by the long one) and the other order's owner takes its place, in one transaction.
pub fn order_match_cmd(a: &Args) -> R<()> {
    only(a, &["long", "short", "script", "size", "price", "past-expiry", "into", "leaver"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (ll, lo, tpl) = order_of(a, "long", &mut node, net)?;
    let (sl, so, t2) = order_of(a, "short", &mut node, net)?;
    if t2 != tpl || sl.lib != ll.lib {
        return Err("the two orders are not of one template and one library: they do not mean the same position".into());
    }
    // Review T1b, F7: no script can refuse an order past its expiry (a script cannot show that a DAA has not come), so
    // the tool does, when it knows the time.
    if !a.has("past-expiry") && (a.has("daa") || node.is_some()) {
        let t = now(a, &mut node)? as i64;
        for (o, which) in [(&lo, "long"), (&so, "short")] {
            if t >= o.expiry {
                return Err(format!("REFUSED: the {which} order ended at DAA {} (now {t}): its owner expects it back, not matched. `order-expire` returns it", o.expiry));
            }
        }
    }
    if a.has("into") || a.has("leaver") {
        let side = side_of(a.need("leaver")?)?;
        let xl = load(a, "into", &mut node, net)?;
        let TObj::Position(x) = xl.obj else { return Err(format!("--into is a {}, not a position", xl.obj.kind())) };
        if xl.lib != ll.lib || Template::of(&x) != tpl {
            return Err("--into is not a position of the orders' library and template".into());
        }
        let s = script_of(a, &ll.lib, "KpfTReplace", &mut node, net)?;
        let (fk, keys, fu) = funding(a, net, &mut node)?;
        let n = crate::args::parse_amount(a.need("size")?, tpl.scale)?;
        let (b, r) = tb::replace(&ll.lib, &live(&xl, x, false)?, &live(&ll, lo, false)?, &live(&sl, so, false)?, &s, n, price(a, "price")?, side, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
        println!("side {} leaves the position by its order: it is paid {} KAS (its margin in the position, its order's, and {} KAS the price moved its way)", word(side), fmt_kas(r.to_leaver as i128), fmt_kas(r.gain as i128));
        match r.z {
            Some((z, _)) => show(&TObj::Position(z), &ll.lib, None, net),
            None => println!("the newcomer is the one that stays: nothing is written of these units, and it is paid {} KAS", fmt_kas(r.to_stayer as i128)),
        }
        println!("the tips {} KAS (the sender's: they are in its change)", fmt_kas(r.tips as i128));
        // The objects under the registry's id are outputs 0, 1, 2 and 4 in their order; the script is output 3.
        let mut made = vec![Made::Script(3, "KpfTReplace")];
        let mut k = 0usize;
        let mut next = || {
            k += 1;
            if k <= 3 { k - 1 } else { 4 }
        };
        for (o, _) in [r.rest, r.z].into_iter().flatten() {
            made.push(Made::Obj(next(), TObj::Position(o)));
        }
        for o in [r.long, r.short] {
            if let Rest::Order(o, _) = o {
                made.push(Made::OrderOf(next(), o, tpl));
            }
        }
        return done(a, "order-match", &b, &keys, net, &mut node, &ll.lib, &made);
    }
    let s = script_of(a, &ll.lib, "KpfTMatch", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let n = crate::args::parse_amount(a.need("size")?, tpl.scale)?;
    let (b, m) = tb::order_match(&ll.lib, &tpl, &live(&ll, lo, false)?, &live(&sl, so, false)?, &s, n, price(a, "price")?, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&TObj::Position(m.position), &ll.lib, None, net);
    let say = |r: &Rest| match r {
        Rest::Nothing => "used up".to_string(),
        Rest::Order(o, v) => format!("{} units rest, holding {} KAS", o.size, fmt_kas(*v as i128)),
    };
    println!("the long order: {}; the short order: {}; the tips {} KAS (the sender's: they are in its change)", say(&m.long), say(&m.short), fmt_kas(m.tips as i128));
    let mut made = vec![Made::Obj(0, TObj::Position(m.position)), Made::Script(2, "KpfTMatch")];
    let long_rests = matches!(m.long, Rest::Order(..));
    if let Rest::Order(o, _) = m.long {
        made.push(Made::OrderOf(1, o, tpl));
    }
    if let Rest::Order(o, _) = m.short {
        made.push(Made::OrderOf(if long_rests { 3 } else { 1 }, o, tpl));
    }
    done(a, "order-match", &b, &keys, net, &mut node, &ll.lib, &made)
}

/// `order-cancel`: the owner takes its order back (its key signs). `order-expire`: anyone returns an order to its
/// owner from its expiry on.
pub fn order_cancel_cmd(a: &Args) -> R<()> {
    order_end(a, true)
}

pub fn order_expire_cmd(a: &Args) -> R<()> {
    order_end(a, false)
}

fn order_end(a: &Args, signed: bool) -> R<()> {
    only(a, &["order", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, o, _) = order_of(a, "order", &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTCancel", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let payer = Payer { funds: &fu, change_to: fk, rate: rate(a)? };
    let b = if signed { tb::order_cancel(&l.lib, &live(&l, o, false)?, &s, payer) } else { tb::order_expire(&l.lib, &live(&l, o, false)?, &s, payer) }.map_err(e)?;
    println!("{} KAS back to {}", fmt_kas(l.f.amount as i128), p2pk_address(&o.owner, net));
    done(a, if signed { "order-cancel" } else { "order-expire" }, &b, &keys, net, &mut node, &l.lib, &[Made::Script(1, "KpfTCancel")])
}

/// `pos-merge`: two positions of the same two parties (`--position`, `--with`: the same market and terms, the same
/// sides, one registry) become one. Anyone sends it, nobody signs, and whoever sends it keeps the library's merge share of what the two held (half from each
/// party's margin); at every price each party is owed what the two positions owed it, to the rounding of a quote.
pub fn merge_cmd(a: &Args) -> R<()> {
    only(a, &["position", "with", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l1, x1) = position_of(a, &mut node, net)?;
    let l2 = load(a, "with", &mut node, net)?;
    let TObj::Position(x2) = l2.obj else { return Err(format!("--with is a {}, not a position", l2.obj.kind())) };
    if l1.lib != l2.lib {
        return Err("the two positions are in two libraries".into());
    }
    let s = script_of(a, &l1.lib, "KpfTMerge", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, out) = tb::merge(&l1.lib, &live(&l1, x1, false)?, &live(&l2, x2, false)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&TObj::Position(out), &l1.lib, None, net);
    println!("merged: one position of the two; the sender keeps {} KAS of what they held", fmt_kas(kpf_protocol::thin::MERGE_TIP as i128));
    done(a, "pos-merge", &b, &keys, net, &mut node, &l1.lib, &[Made::Obj(0, TObj::Position(out)), Made::Script(2, "KpfTMerge")])
}

/// `pos-novate`: a user leaves two positions that cancel each other (`--position`, `--with`, in either order: in one it
/// is long, its A; in the other short, its B; the same market and terms). The two it faced face each other from the
/// second position's entry, for the units both positions have; the user is paid its two margins and what the price
/// moved between the two entries, which the margin of the one that stays short gives up (or takes). Anyone sends it and
/// nobody signs (the two positions are of one registry: the user is in each by its own signature); no price is shown: each
/// who stays is owed at every price what it was.
pub fn novate_cmd(a: &Args) -> R<()> {
    only(a, &["position", "with", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l1, x1) = position_of(a, &mut node, net)?;
    let l2 = load(a, "with", &mut node, net)?;
    let TObj::Position(x2) = l2.obj else { return Err(format!("--with is a {}, not a position", l2.obj.kind())) };
    if l1.lib != l2.lib {
        return Err("the two positions are in two libraries".into());
    }
    let s = script_of(a, &l1.lib, "KpfTNovate", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (lx, x, ly, y) = if x2.x.b_key == x1.x.a_key { (&l1, x1, &l2, x2) } else { (&l2, x2, &l1, x1) };
    let (b, nv, made) = tb::novate(&l1.lib, &live(lx, x, false)?, &live(ly, y, false)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    let moved = if nv.transfer >= 0 { format!("{} KAS of it from", fmt_kas(nv.transfer as i128)) } else { format!("after {} KAS to", fmt_kas(-nv.transfer as i128)) };
    println!("the user that leaves is paid {} KAS ({moved} the margin of the one that stays short); the sender keeps {} KAS", fmt_kas(nv.to_leaver as i128), fmt_kas(kpf_protocol::thin::NOVATE_TIP as i128));
    match nv.z {
        Some((z, _)) => show(&TObj::Position(z), &l1.lib, None, net),
        None => println!("the two that stay are one user: it is paid {} KAS, and no position is written for these units", fmt_kas(nv.to_stayer as i128)),
    }
    for (r, which) in [(nv.rest_x, "first"), (nv.rest_y, "second")] {
        if let Some((r, v)) = r {
            println!("what is left of the {which} position goes on: {} units, holding {} KAS", r.x.legs[0].size, fmt_kas(v as i128));
        }
    }
    let mut m: Vec<Made> = made.iter().map(|(i, position)| Made::Obj(*i, TObj::Position(*position))).collect();
    m.push(Made::Script(2, "KpfTNovate"));
    done(a, "pos-novate", &b, &keys, net, &mut node, &l1.lib, &m)
}

/// `pos-join`: two positions of the same two users in two markets (`--position`, `--with`: the same terms and registry,
/// one leg each) become one position of two legs: one pot of margin for both markets, closed and liquidated on the
/// whole. Both users sign (`--key`: a position of two legs is not merged, joined again, or left through a sale;
/// it ends by a close of both markets or by both signatures). No price is shown: each leg keeps its size and entry.
pub fn join_cmd(a: &Args) -> R<()> {
    only(a, &["position", "with", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l1, x1) = position_of(a, &mut node, net)?;
    let l2 = load(a, "with", &mut node, net)?;
    let TObj::Position(x2) = l2.obj else { return Err(format!("--with is a {}, not a position", l2.obj.kind())) };
    if l1.lib != l2.lib {
        return Err("the two positions are in two libraries".into());
    }
    let s = script_of(a, &l1.lib, "KpfTJoin", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (v1, v2, r) = (live(&l1, x1, false)?, live(&l2, x2, false)?, rate(a)?);
    let mk = |f: Sw| tb::join(&l1.lib, &OLive { s: sw_position(x1, f), ..v1 }, &OLive { s: sw_position(x2, f), ..v2 }, &s, Payer { funds: &sw_funds(&fu, f), change_to: f(fk), rate: r }).map(|x| x.0).map_err(e);
    let out = kpf_protocol::thin::joined(&x1, &x2).map_err(e)?;
    show(&TObj::Position(out), &l1.lib, None, net);
    println!("joined: one position of two legs; the sender keeps {} KAS of what the two held", fmt_kas(kpf_protocol::thin::MERGE_TIP as i128));
    println!("NOTE: a position of two legs is not merged, joined again, or left one market at a time through a sale: it ends by a close of both markets at the committee's prices, or by both signatures");
    done_by(a, "pos-join", &mk, &keys, net, &mut node, &l1.lib, &[Made::Obj(0, TObj::Position(out)), Made::Script(2, "KpfTJoin")], &[TObj::Position(x1), TObj::Position(x2)])
}

/// The two positions `--position` and `--with` of one library.
fn two(a: &Args, node: &mut N, net: Network) -> R<(Loaded, Position, Loaded, Position)> {
    let (l1, x1) = position_of(a, node, net)?;
    let l2 = load(a, "with", node, net)?;
    let TObj::Position(x2) = l2.obj else { return Err(format!("--with is a {}, not a position", l2.obj.kind())) };
    if l1.lib != l2.lib {
        return Err("the two positions are in two libraries".into());
    }
    Ok((l1, x1, l2, x2))
}

/// `pos-turn`: the same position written the other way round: `--mode users` (its two users change places, and each
/// leg's direction turns) or `--mode legs` (its two legs change places). Nothing is owed to anybody that was not. It
/// is for the commands that read a position one way: `pos-leave` takes the first leg of the position whose A sells,
/// `pos-fold` folds into the first leg. Anyone, once the position is 100 DAA old.
pub fn turn_cmd(a: &Args) -> R<()> {
    only(a, &["position", "script", "mode"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = position_of(a, &mut node, net)?;
    let users = match a.need("mode")? {
        "users" => true,
        "legs" => false,
        _ => return Err("--mode is `users` or `legs`".into()),
    };
    let s = script_of(a, &l.lib, "KpfTTurn", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, out) = tb::turn(&l.lib, &live(&l, x, false)?, &s, users, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&TObj::Position(out), &l.lib, None, net);
    println!("turned: the same position, its {} in each other's places", if users { "users" } else { "legs" });
    done(a, "pos-turn", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Position(out)), Made::Script(1, "KpfTTurn")])
}

/// `pos-fold`: the position `--with` (one leg, the same two users, the market of `--position`'s first leg) is folded
/// into that leg: added when it is the same way, set against it when it is the other. `--position` is of one leg or of
/// two. Anyone sends it, nobody signs.
pub fn fold_cmd(a: &Args) -> R<()> {
    only(a, &["position", "with", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l1, x1, l2, x2) = two(a, &mut node, net)?;
    let s = script_of(a, &l1.lib, "KpfTFold", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, out) = tb::fold(&l1.lib, &live(&l1, x1, false)?, &live(&l2, x2, false)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&TObj::Position(out), &l1.lib, None, net);
    println!("folded: the second position is in the first's first leg; the sender keeps {} KAS of what the two held", fmt_kas(kpf_protocol::thin::MERGE_TIP as i128));
    done(a, "pos-fold", &b, &keys, net, &mut node, &l1.lib, &[Made::Obj(0, TObj::Position(out)), Made::Script(2, "KpfTFold")])
}

/// `pos-leave`: the first leg of `--position` (two legs; its A is the user that sells) goes to the third user of
/// `--with` (one leg, of that leg's market, in which the user holds the other way what it holds in the leg). The
/// third user and the position's other user face each other from `--with`'s entry. The user is paid its margin in
/// `--with`; its margin in the position stays there, with what it made on the leg, behind the leg it keeps. The
/// position's two users sign (`--key`; the third user does not): a leg that leaves takes its gains and losses out of
/// the pot they were set against, and no rule without a price holds both whole (review T1b, F16). No price is shown.
pub fn leave_cmd(a: &Args) -> R<()> {
    only(a, &["position", "with", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l1, x1, l2, x2) = two(a, &mut node, net)?;
    let s = script_of(a, &l1.lib, "KpfTLeave", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (v1, v2, r) = (live(&l1, x1, false)?, live(&l2, x2, false)?, rate(a)?);
    let mk = |f: Sw| tb::leave(&l1.lib, &OLive { s: sw_position(x1, f), ..v1 }, &OLive { s: sw_position(x2, f), ..v2 }, &s, Payer { funds: &sw_funds(&fu, f), change_to: f(fk), rate: r }).map(|x| x.0).map_err(e);
    let lv = kpf_protocol::thin::left(&x1, v1.value as i64, &x2, v2.value as i64, true).map_err(e)?;
    let moved = if lv.gain >= 0 { format!("{} KAS to its margin there from the other's", fmt_kas(lv.gain as i128)) } else { format!("{} KAS from its margin there to the other's", fmt_kas(-lv.gain as i128)) };
    println!("the leg leaves the position: {} units; the user that sold is paid {} KAS (its margin in the second position, less the sender's {} KAS), and {moved}", lv.n, fmt_kas(lv.to_leaver as i128), fmt_kas(kpf_protocol::thin::NOVATE_TIP as i128));
    println!("-- the position after");
    show(&TObj::Position(lv.x_after.0), &l1.lib, None, net);
    println!("-- the position of the two that face each other");
    show(&TObj::Position(lv.z.0), &l1.lib, None, net);
    let mut m = vec![Made::Obj(0, TObj::Position(lv.x_after.0)), Made::Obj(1, TObj::Position(lv.z.0)), Made::Script(2, "KpfTLeave")];
    if let Some((r, v)) = lv.rest_y {
        println!("what is left of the second position goes on: {} units, holding {} KAS", r.x.legs[0].size, fmt_kas(v as i128));
        m.push(Made::Obj(3, TObj::Position(r)));
    }
    done_by(a, "pos-leave", &mk, &keys, net, &mut node, &l1.lib, &m, &[TObj::Position(x1), TObj::Position(x2)])
}

/// `cosign` for a transaction of the thin family that `--partial` wrote: whoever must still sign reads what it does,
/// signs, and (when nothing is left open) sends it. What is printed is read from the transaction: every input and
/// output is a plain coin, a script of the library, or an object whose state the file gives and whose script that
/// state makes; anything else is refused. The inputs are looked up at the node (one query): a signature covers the
/// inputs' outpoints and the outputs and not what the file says the inputs ARE, so a file that showed another position
/// at an outpoint would have a signature for what that outpoint really holds.
pub fn cosign_cmd(a: &Args, v: &Value) -> R<()> {
    a.only(&["tx", "key", "node", "network", "submit", "out-dir", "no-save", "lib", "lib-unchecked", "registry", "registry-unchecked", "inputs-unchecked", "out", "daa"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let path = a.need("tx")?;
    if v["network"].as_str() != Some(net.name()) {
        return Err(format!("{path} is for the network {}, not {}", v["network"], net.name()));
    }
    let lib = lib_of(a)?;
    if v["library"].as_str() != Some(to_hex(&lib).as_str()) {
        return Err(format!("REFUSED: {path} is of the library {}, not the one --lib names. Nothing was signed", v["library"]));
    }
    let part: OPartial = serde_json::from_value(v["partial"].clone()).map_err(|x| format!("{path}: {x}"))?;
    part.check_signed().map_err(|x| format!("REFUSED: {x}"))?;
    let keys = crate::keys::keyset(&a.all("key"))?;
    let hexes = |k: &str| -> R<Vec<Vec<u8>>> { v[k].as_array().map(|l| l.iter().map(|x| from_hex(x.as_str().unwrap_or("")).map_err(e)).collect()).unwrap_or(Ok(vec![])) };
    let pool = hexes("states")?.iter().map(|b| TObj::decode(b).map_err(|x| format!("{path}: {x}"))).collect::<R<Vec<TObj>>>()?;
    let tpls = hexes("templates")?.iter().map(|b| Template::from_bytes(b).map_err(|x| format!("{path}: {x}"))).collect::<R<Vec<Template>>>()?;
    let script_at = |spk: &kaspa_consensus_core::tx::ScriptPublicKey| pinned::SCRIPTS.iter().copied().find(|n| tb::script_spk(n).ok().as_ref() == Some(spk));
    let object_at = |spk: &kaspa_consensus_core::tx::ScriptPublicKey| pool.iter().copied().find(|o| tb::object_spk(&lib, o) == *spk);
    let yours = |o: &TObj| {
        let mine: Vec<String> = parties(o).iter().filter(|k| keys.has(k)).map(|k| p2pk_address(k, net)).collect();
        match o {
            TObj::Anchor => {}
            _ if mine.is_empty() => println!("       NONE OF THIS OBJECT'S KEYS IS YOURS"),
            _ => println!("       yours in it: {}", mine.join(", ")),
        }
    };
    println!("== cosign (the writer calls it {:?}; what follows is read from the transaction)", v["title"].as_str().unwrap_or(""));
    if part.tx.inputs.len() != part.entries.len() {
        return Err("REFUSED: the file's inputs and their UTXO entries do not match".into());
    }
    let (mut put_in, mut back) = (0u64, 0u64);
    let (mut registry, mut enters): (Option<Hash>, bool) = (None, false);
    let (mut in_labels, mut out_labels, mut looked) = (vec![], vec![], vec![]);
    for (i, en) in part.entries.iter().enumerate() {
        let spk = &en.script_public_key;
        if let Some(k) = p2pk_key(spk) {
            if en.covenant_id.is_some() {
                return Err(format!("REFUSED: input {i} is a plain coin the file puts under a covenant id. Nothing was signed"));
            }
            if part.open_sigs.iter().any(|(j, _, _)| *j == i) {
                return Err(format!("REFUSED: input {i} is a plain coin, and the file lists it among the signatures a script waits for. Nothing was signed"));
            }
            let (open, mine) = (part.open.iter().any(|(j, _)| *j == i), keys.has(&k));
            if mine {
                put_in += en.amount;
            }
            println!("  in  {i}: {} KAS of {}{}", fmt_kas(en.amount as i128), p2pk_address(&k, net), match (open, mine) { (true, true) => "   YOU SIGN THIS", (true, false) => "   left to its owner", (false, true) => "   (yours)", _ => "" });
            in_labels.push(if mine { "yours".to_string() } else { "another's coin".to_string() });
            // Another's coin is looked up as well: an outpoint the file calls a plain coin is one.
            looked.push(i);
            continue;
        }
        if part.open.iter().any(|(j, _)| *j == i) {
            return Err(format!("REFUSED: input {i} is left to a key and is not a plain coin of it. Nothing was signed"));
        }
        looked.push(i);
        if let Some(name) = script_at(spk) {
            if en.covenant_id.map(|c| c.as_bytes()) != Some(lib) {
                return Err(format!("REFUSED: input {i} has the script {name} and is not under the library's id. Nothing was signed"));
            }
            println!("  in  {i}: the script {name} of the library, holding {} KAS", fmt_kas(en.amount as i128));
            for (_, _, k) in part.open_sigs.iter().filter(|(j, _, _)| *j == i) {
                println!("         it waits for the signature of {}{}", p2pk_address(k, net), if keys.has(k) { "   YOU SIGN THIS" } else { "" });
            }
            in_labels.push(format!("script {name}"));
            continue;
        }
        let Some(o) = object_at(spk) else {
            return Err(format!("REFUSED: input {i} is neither a plain coin, a script of the library, nor an object whose state the file gives. Nothing was signed"));
        };
        if part.open_sigs.iter().any(|(j, _, _)| *j == i) {
            return Err(format!("REFUSED: input {i} is an object, and the file says it waits for a signature: an object's script takes none. Nothing was signed"));
        }
        let Some(id) = en.covenant_id else { return Err(format!("REFUSED: input {i} is a {} under no covenant id. Nothing was signed", o.kind())) };
        if registry.is_some_and(|b| b != id) {
            return Err(format!("REFUSED: input {i} is of another registry than the objects before it. Nothing was signed"));
        }
        registry = Some(id);
        enters |= matches!(o, TObj::Anchor);
        println!("  in  {i}: {} KAS in an object of the registry {}:", fmt_kas(en.amount as i128), id);
        show(&o, &lib, None, net);
        yours(&o);
        in_labels.push(o.kind().to_string());
    }
    // The registry: what enters one (an anchor among the inputs) enters a registry the co-signer checked, as the writer did.
    match (a.get("registry"), registry) {
        (Some(p), Some(id)) => {
            if tb::registry_check(&tx_file(p)?, &lib).map_err(|x| format!("{p}: {x}"))? != id.as_bytes() {
                return Err("REFUSED: the objects of this transaction are not of the registry --registry begins. Nothing was signed".into());
            }
            println!("registry {id}: its genesis writes anchors of this library and nothing else under its id");
        }
        (None, Some(id)) if enters && a.has("registry-unchecked") => println!("WARNING: the registry {id} that this transaction enters was NOT checked (--registry-unchecked)"),
        (None, Some(_)) if enters => return Err("REFUSED: this transaction enters a registry, and the registry is not checked: give --registry REGISTRY-tx.json (or --registry-unchecked, if you checked it yourself). Nothing was signed".into()),
        _ => {}
    }
    // The inputs at the node.
    match &mut node {
        Some(n) => {
            let mut addrs: Vec<String> = looked.iter().filter_map(|i| spk_address(&part.entries[*i].script_public_key, net)).collect();
            addrs.sort();
            addrs.dedup();
            let list = n.utxos(&addrs)?;
            for i in &looked {
                let (op, en) = (part.tx.inputs[*i].previous_outpoint, &part.entries[*i]);
                // At its own address: the node's entry carries the script, and one without a script is not taken.
                let Some(u) = list.iter().find(|u| u.op == op && u.spk.as_ref() == Some(&en.script_public_key)) else {
                    return Err(format!("REFUSED: input {i} ({}:{}) is not at the node with the script the file gives it (spent, not yet in a block, or something else than the file says). Nothing was signed", op.transaction_id, op.index));
                };
                if u.amount != en.amount || u.covenant_id != en.covenant_id {
                    return Err(format!("REFUSED: input {i} is not at the node what the file says it is (its value or its covenant id). Nothing was signed"));
                }
            }
            println!("at the node: every input is there as the file says");
        }
        None if a.has("inputs-unchecked") => println!("WARNING: the inputs were NOT looked up (--inputs-unchecked). A signature holds for the outpoints, whatever they hold: if one is not the object shown above, you sign for what it is"),
        None => return Err("REFUSED: the inputs must be looked up at the node (--node URL): a signature holds for the outpoints, whatever they hold. Or --inputs-unchecked, if you know each outpoint. Nothing was signed".into()),
    }
    let mut made: Vec<Made> = vec![];
    for (i, o) in part.tx.outputs.iter().enumerate() {
        let spk = &o.script_public_key;
        if let Some(k) = p2pk_key(spk) {
            if o.covenant.is_some() {
                return Err(format!("REFUSED: output {i} is a plain output under a covenant id. Nothing was signed"));
            }
            if keys.has(&k) {
                back += o.value;
            }
            println!("  out {i}: {} KAS to {}{}", fmt_kas(o.value as i128), p2pk_address(&k, net), if keys.has(&k) { "   (you)" } else { "" });
            out_labels.push(if keys.has(&k) { "to you".to_string() } else { "to another address".to_string() });
            continue;
        }
        if let Some(name) = script_at(spk) {
            if part.bound(i).map(|c| c.as_bytes()) != Some(lib) {
                return Err(format!("REFUSED: output {i} has the script {name} and does not go on under the library's id. Nothing was signed"));
            }
            println!("  out {i}: the script {name} goes on, holding {} KAS", fmt_kas(o.value as i128));
            made.push(Made::Script(i, name));
            out_labels.push(format!("script {name}"));
            continue;
        }
        let Some(obj) = object_at(spk) else {
            return Err(format!("REFUSED: output {i} ({} KAS) is neither a plain output, a script of the library, nor an object whose state the file gives: what is put there may never come out. Nothing was signed", fmt_kas(o.value as i128)));
        };
        if part.bound(i).is_none() || part.bound(i) != registry {
            return Err(format!("REFUSED: output {i} is a {} that is not of the registry of the objects spent: no script of the family spends it. Nothing was signed", obj.kind()));
        }
        if (o.value as i64) < obj.least_value() {
            return Err(format!("REFUSED: output {i} is a {} that holds {} KAS and whose state says at least {}. Nothing was signed", obj.kind(), fmt_kas(o.value as i128), fmt_kas(obj.least_value() as i128)));
        }
        println!("  out {i}: {} KAS in an object of the registry:", fmt_kas(o.value as i128));
        show(&obj, &lib, None, net);
        yours(&obj);
        if (o.value as i64) > obj.least_value() && !matches!(obj, TObj::Anchor) {
            println!("       it holds {} KAS more than its state says (its carrier): a close at a price pays that to the party the close is declared against", fmt_kas((o.value as i64 - obj.least_value()) as i128));
        }
        made.push(match obj {
            TObj::Order(ord) => match tpls.iter().find(|t| t.hash() == ord.tpl) {
                Some(t) => Made::OrderOf(i, ord, *t),
                None => Made::Obj(i, obj),
            },
            _ => Made::Obj(i, obj),
        });
        out_labels.push(obj.kind().to_string());
    }
    println!("in plain coins you put in {} KAS and are paid {} KAS; what you hold in the objects is shown above, before and after", fmt_kas(put_in as i128), fmt_kas(back as i128));
    let mine = part.open.iter().any(|(_, k)| keys.has(k)) || part.open_sigs.iter().any(|(_, _, k)| keys.has(k));
    if !mine {
        return Err("nothing in this transaction waits for a key given with --key: nothing was signed".into());
    }
    let more = part.sign_more(&keys).map_err(e)?;
    if !more.open.is_empty() || !more.open_sigs.is_empty() {
        let left: Vec<String> = more.open.iter().map(|(_, k)| *k).chain(more.open_sigs.iter().map(|(_, _, k)| *k)).map(|k| p2pk_address(&k, net)).collect();
        if a.has("submit") {
            return Err(format!("signatures are still missing ({}): not sent. Drop --submit and give --out FILE for the file to pass on", left.join(", ")));
        }
        let out = a.get("out").ok_or_else(|| format!("signatures are still missing ({}): give --out FILE for the file to pass on", left.join(", ")))?;
        let mut w = v.clone();
        w["partial"] = serde_json::to_value(&more).map_err(e)?;
        write_json(std::path::Path::new(out), &w)?;
        println!("signed, and still to be signed by {}: written to {out}", left.join(", "));
        println!("NOTE: whoever holds that file with the last signature sends it when it likes, for as long as every input is unspent. To take your signature back, spend one of its inputs before it is sent (whichever lands first stands): a coin of yours among them, or the position itself (`pos-add-margin` writes it anew; an anchor is not yours to move, so for an opening it is your coin)");
        return Ok(());
    }
    let mut sg = more.finished().map_err(e)?;
    (sg.input_labels, sg.output_labels) = (in_labels, out_labels);
    let submit = a.has("submit");
    let r = finish_signed(a, "cosign", &sg, net, &mut node, submit);
    if !a.has("no-save") && r.is_ok() {
        write_made(&sg.tx, &made, &lib, net, &out_dir(a, !submit))?;
    }
    r
}
