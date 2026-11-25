//! The commands of price notes, their citation, and stated changes of a position (docs/ARCHITECTURE-CLEARING.md 3
//! and 4a). As every other command: offline unless `--node` is given, nothing submitted without `--submit`, every
//! transaction verified locally first.
//!
//! A note is the committee's price for a market and a time, written once under the library's id (`note-write`). A
//! citation holds, against the commitment of the chain block that accepted that writing, that it was written
//! (`note-cite`, alone; `take-answer`, beside the answer of a take). A take is stated (`pos-take`), applied after the
//! position's dispute period (`change-settle`), disputed (`change-dispute`), answered with citations (`take-answer`,
//! disputed or not), or ended (`change-lapse`, `change-late`).

use kaspa_consensus_core::tx::Transaction;
use kaspa_consensus_core::Hash;
use kpf_protocol::cite;
use kpf_protocol::error::{hex32, to_hex};
use kpf_protocol::network::{p2pk_address, Network};
use kpf_protocol::oracle_build::OLive;
use kpf_protocol::thin::{Note, TObj, Take};
use kpf_protocol::thin_artifacts as pinned;
use kpf_protocol::thin_build::{self as tb, Cited, LibScript, Payer};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::args::{fmt_kas, Args};
use crate::cmds::{connect, e, int_or, network, price, R};
use crate::files::{read_json, write_json};
use crate::node::{Node, WsTransport};
use crate::ocmds::{finish_signed, funding, only, out_dir, rate};
use crate::tcmds::{done, found, live, load, position_of, script_of, show, side_of, stated_price, Made, N};

fn num(v: Option<&Value>) -> Option<u64> {
    v.and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok())))
}

/// What leads from the transaction `txid` to the commitment of the chain block that accepted it, read at the node:
/// the chain is followed from the block `from` (one that was the sink before the transaction was sent), the block's
/// header, its merged blocks' transactions and the native lane's proofs are read, and the commitment is recomputed
/// from the transaction up. Refused unless it is the header's. Returns the transaction as the block has it, the
/// witness, and the commitment.
pub(crate) fn witness_of(n: &mut Node<WsTransport>, txid: &str, from: &str) -> R<(Transaction, cite::Witness, Hash)> {
    let h32 = |s: &str| hex32(s).map(Hash::from_bytes).map_err(e);
    let step = n.chain_ids(from)?;
    let (block, ids) = step.groups.iter().find(|(_, ids)| ids.iter().any(|i| *i == txid)).cloned().ok_or(format!("no chain block after {from} has accepted {txid} yet ({} blocks read): ask again in a few seconds", step.groups.len()))?;
    let (b, _) = n.block(&block, false)?;
    let head = b.get("header").ok_or("a block without a header")?;
    let hn = |k: &str| num(head.get(k)).ok_or(format!("a header without {k}"));
    let (daa, blue) = (hn("daaScore")?, hn("blueScore")?);
    let commit = h32(head.get("acceptedIdMerkleRoot").and_then(Value::as_str).ok_or("no acceptedIdMerkleRoot")?)?;
    let vd = b.get("verboseData").ok_or("a block without verbose data")?;
    let parent = vd.get("selectedParentHash").and_then(Value::as_str).ok_or("no selected parent")?.to_string();
    let list = |k: &str| vd.get(k).and_then(Value::as_array).map(|x| x.iter().filter_map(|h| h.as_str().map(str::to_owned)).collect::<Vec<_>>()).unwrap_or_default();
    let merged = [list("mergeSetBluesHashes"), list("mergeSetRedsHashes")].concat();
    // (The context of a block's lanes carries the timestamp of its selected parent, with its own DAA and blue scores.)
    let (pb, _) = n.block(&parent, false)?;
    let parent_timestamp = num(pb.get("header").and_then(|h| h.get("timestamp"))).ok_or("the selected parent's header has no timestamp")?;
    let mut seen: HashMap<String, (Transaction, String)> = HashMap::new();
    for h in &merged {
        let (mb, _) = n.block(h, true)?;
        for t in mb.get("transactions").and_then(Value::as_array).cloned().unwrap_or_default() {
            let subnet = t.get("subnetworkId").and_then(Value::as_str).unwrap_or("").to_string();
            if let Ok(c) = crate::node::chain_tx(&t, daa) {
                seen.insert(c.id.to_string(), (c.tx, subnet));
            }
        }
    }
    let mut lane: Vec<(Hash, u16, u32)> = vec![];
    let mut at = None;
    for (i, id) in ids.iter().enumerate() {
        let (t, subnet) = seen.get(id).ok_or(format!("the accepted transaction {id} is in none of the merged blocks read"))?;
        if subnet.chars().all(|c| c == '0') && !subnet.is_empty() {
            if *id == txid {
                at = Some(lane.len());
            }
            lane.push((h32(id)?, t.version, i as u32));
        }
    }
    let at = at.ok_or("the transaction is not of the native lane")?;
    let cited = seen[txid].0.clone();
    if cited.id().to_string() != txid {
        return Err(format!("the transaction as the block shows it has the id {}, not {txid}: it cannot be cited from what the node says", cited.id()));
    }
    let key = cite::native_lane_key();
    let mut proof = |block: &str| -> R<(Option<(Hash, u64)>, Vec<u8>, Hash, Hash, Hash)> {
        let r = n.raw("getSeqCommitLaneProof", json!({ "blockHash": block, "laneKey": key.to_string() }))?;
        let g = |k: &str| r.get(k).and_then(Value::as_str).ok_or(format!("the lane proof has no {k}")).and_then(|x| hex32(x).map(Hash::from_bytes).map_err(e));
        let lane = match r.get("lane") {
            Some(l) if !l.is_null() => Some((g_of(l, "tip")?, num(l.get("blueScore")).ok_or("a lane without a blue score")?)),
            _ => None,
        };
        let bytes = r.get("smtProof").and_then(Value::as_array).map(|x| x.iter().filter_map(|b| b.as_u64().map(|b| b as u8)).collect()).unwrap_or_default();
        Ok((lane, bytes, g("payloadAndCtxDigest")?, g("parentSeqCommit")?, g("inactivityShortcut")?))
    };
    let (here, smt, payload_and_ctx, parent_commitment, inactivity_shortcut) = proof(&block)?;
    let (_, lane_blue) = here.ok_or("the node says the native lane is not active at the block")?;
    let (before, ..) = proof(&parent)?;
    let parent_ref = before.map(|x| x.0).ok_or("the node says the native lane is not active at the block's selected parent")?;
    let (_, activity_path) = cite::merkle(&cite::activity_leaves(&lane), at);
    let w = cite::Witness { block: h32(&block)?, merge_idx: lane[at].2, activity_path, lane_key: key, parent_ref, context: cite::context(parent_timestamp, daa, blue), blue_score: lane_blue, lanes_path: cite::lanes_path(&smt, &key).map_err(e)?, inactivity_shortcut, payload_and_ctx, parent_commitment };
    if w.commitment(&cited.id(), cited.version) != commit {
        return Err("the citation does not lead to the commitment in the block's header: the form of the proof is not this node's".into());
    }
    println!("cited: {txid} was accepted by the chain block {block} ({} steps in the lane's activity, {} in the lanes' tree, {} bytes of proof)", w.activity_path.len(), w.lanes_path.len(), w.bytes());
    Ok((cited, w, commit))
}

fn g_of(v: &Value, k: &str) -> R<Hash> {
    v.get(k).and_then(Value::as_str).ok_or(format!("no {k}")).and_then(|x| hex32(x).map(Hash::from_bytes).map_err(e))
}

/// A note's file (`note-write` writes it): the note, its library, the transaction that wrote it and a chain block
/// from which that transaction is looked for.
struct NoteFile {
    note: Note,
    lib: [u8; 32],
    txid: String,
    from: String,
}

fn note_file(path: &str) -> R<NoteFile> {
    let v = read_json(path)?;
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned).ok_or_else(|| format!("{path}: no `{k}`: not the file of a price note"));
    let n = |k: &str| v.get(k).and_then(Value::as_i64).ok_or_else(|| format!("{path}: no `{k}`"));
    let h = |k: &str| text(k).and_then(|x| hex32(&x).map_err(e));
    Ok(NoteFile { note: Note { committee: h("committee")?, market: h("market")?, t0: n("t0")?, price: n("price")?, spread_bps: n("spreadBps")?, owner: h("owner")? }, lib: h("library")?, txid: text("txid")?, from: text("from")? })
}

/// The citation of the note of the file `path`, and the chain block's commitment it leads to.
pub(crate) fn cited_of(path: &str, n: &mut Node<WsTransport>) -> R<(Cited, Hash)> {
    let f = note_file(path)?;
    let (tx, w, commit) = witness_of(n, &f.txid, &f.from)?;
    Ok((Cited::of(&tx, &f.lib, f.note, w).map_err(e)?, commit))
}

/// A copy of a script of the library found at the node (a citing copy is never given as a file: it is nobody's).
pub(crate) fn copy_at(lib: &[u8; 32], name: &'static str, node: &mut N, net: Network, skip: &[kaspa_consensus_core::tx::TransactionOutpoint]) -> R<LibScript> {
    let redeem = pinned::kit().script(name).ok_or_else(|| format!("no script {name}"))?.to_vec();
    for _ in 0..8 {
        let f = found(node, net, redeem.clone(), lib, &format!("copy of the script {name}"), "node")?;
        if !skip.contains(&f.op) {
            return Ok(OLive { s: name, value: f.amount, id: Some(f.covenant_id), op: f.op, daa: f.daa.unwrap_or(0) });
        }
    }
    Err(format!("the library has no second copy of {name} at the node: a position of two legs is answered with two citations"))
}

/// `note-write`: the committee's price for the market of `--leg` (1 or 2) of `--position` at a time, written once as
/// a note under the library's id, from the members' signed prices (`--epoch-tableN`, `--signed-priceN` of that leg,
/// as a proof takes them). What the note holds returns to the funder (`note-reclaim`). Writes the note's file.
pub fn note_write_cmd(a: &Args) -> R<()> {
    only(a, &["position", "change", "leg", "script", "t0", "epoch-table1", "signed-price1", "epoch-table2", "signed-price2"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    // The market is a leg's: of a position, or of the position a stated change is of.
    let (lib, x) = if a.has("change") {
        let (l, c) = change_of(a, &mut node, net)?;
        (l.lib, c.position)
    } else {
        let (l, x) = position_of(a, &mut node, net)?;
        (l.lib, x)
    };
    let leg = int_or(a, "leg", 1)?;
    if leg != 1 && !(leg == 2 && x.x.legs[1].size > 0) {
        return Err("--leg is 1, or 2 for a position of two legs".into());
    }
    let t = crate::ccmds::x_att(a, leg as usize, &x.x, &mut node, net)?;
    let s = script_of(a, &lib, "KpfTNote", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, note) = tb::note_write(&lib, &s, (&t.0, &t.1), fk, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    // A chain block that stands before the note's transaction: the citation is looked for from it. Thirty selected
    // parents behind the sink, which the chain no longer leaves.
    let from = match node.as_mut() {
        Some(n) => {
            let mut h = n.dag()?.1;
            for _ in 0..30 {
                let (b, _) = n.block(&h, false)?;
                h = b.get("verboseData").and_then(|v| v.get("selectedParentHash")).and_then(Value::as_str).ok_or("a block without its selected parent")?.to_string();
            }
            h
        }
        None => String::new(),
    };
    let sg = b.sign(&keys).map_err(e)?;
    println!("note: the committee's price of the market {} at DAA {} is {} KAS (spread {} basis points); what it holds, {} KAS, returns to {}", to_hex(&note.market), note.t0, fmt_kas(note.price as i128), note.spread_bps, fmt_kas(kpf_protocol::thin::NOTE_VALUE as i128), p2pk_address(&note.owner, net));
    let submit = a.has("submit");
    let r = finish_signed(a, "note-write", &sg, net, &mut node, submit);
    if !a.has("no-save") {
        let dir = out_dir(a, !(submit && r.is_ok()));
        let tag: String = sg.tx.id().to_string().chars().take(8).collect();
        let v = json!({ "kind": "price note", "library": to_hex(&lib), "txid": sg.tx.id().to_string(), "from": from, "committee": to_hex(&note.committee), "market": to_hex(&note.market), "t0": note.t0, "price": note.price, "spreadBps": note.spread_bps, "owner": to_hex(&note.owner) });
        let p = dir.join(format!("{tag}-note.json"));
        write_json(&p, &v)?;
        println!("written: {}", p.display());
        crate::tcmds::write_made(&sg.tx, &[Made::Script(1, "KpfTNote")], &lib, net, &dir)?;
    }
    r
}

/// `note-cite`: a citation of `--note` and nothing else: the script recomputes the commitment of the chain block
/// that accepted the note's writing and holds it to the engine's. What a node charges for one, and whether it agrees.
pub fn note_cite_cmd(a: &Args) -> R<()> {
    only(a, &["note"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let f = note_file(a.need("note")?)?;
    let (c, commit) = cited_of(a.need("note")?, node.as_mut().ok_or("--node URL: a citation is read at the node")?)?;
    kpf_protocol::verify::set_chain_view(vec![(c.witness.block, commit)]);
    let s = copy_at(&f.lib, "KpfTCite", &mut node, net, &[])?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let b = tb::cite(&f.lib, &s, &c, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("cites: the committee's price of the market {} at DAA {} is {} KAS", to_hex(&f.note.market), f.note.t0, fmt_kas(f.note.price as i128));
    done(a, "note-cite", &b, &keys, net, &mut node, &f.lib, &[Made::Script(0, "KpfTCite")])
}

/// `note-reclaim`: what `--note` holds returns to its writer's key (a citation is of the note's writing, not of its
/// output).
pub fn note_reclaim_cmd(a: &Args) -> R<()> {
    only(a, &["note"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let f = note_file(a.need("note")?)?;
    let o = found(&mut node, net, pinned::kit().note(&f.note.hash(), &f.note.owner), &f.lib, "price note", "node")?;
    let live = OLive { s: f.note, value: o.amount, id: Some(o.covenant_id), op: o.op, daa: o.daa.unwrap_or(0) };
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let b = tb::note_reclaim(&live, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("returns {} KAS to {}", fmt_kas(o.amount as i128), p2pk_address(&f.note.owner, net));
    done(a, "note-reclaim", &b, &keys, net, &mut node, &f.lib, &[])
}

/// The take the flags state: `--side`, the prices (as a close states them) and `--amount` (default: the most the
/// side takes whatever the committee's prices within the tolerance of those stated, with `--keep` KAS more left in).
fn take_of(a: &Args, x: &kpf_protocol::thin::Position, node: &mut N, net: Network) -> R<(i64, Take)> {
    let side = side_of(a.need("side")?)?;
    let (t0, prices) = stated_price(a, x, node, net)?;
    let amount = if a.has("amount") { price(a, "amount")? } else { (x.take_safe(side, prices) - if a.has("keep") { price(a, "keep")? } else { 0 }).max(0) };
    Ok((t0, Take { side, a: prices, amount }))
}

/// `pos-take`: the owner of `--side` states that the position's entries move to the prices `--p1` `--p2` of the time
/// `--t0` (its own; or the members' signed prices, as a close takes them), so that what it gained since is margin of
/// its own, and that it takes `--amount` out (default: all it has above its floor there). It posts the bond; nothing
/// is computed on the chain.
pub fn take_cmd(a: &Args) -> R<()> {
    only(a, &["position", "script", "side", "t0", "p1", "p2", "amount", "keep", "epoch-table1", "signed-price1", "epoch-table2", "signed-price2"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, x) = position_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTChange", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (t0, tk) = take_of(a, &x, &mut node, net)?;
    let (b, c) = tb::take(&l.lib, &live(&l, x, false)?, &s, tk, t0, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    let after = x.taken(&tk).ok_or("the take cannot be applied at its prices")?;
    println!("stated: side {} takes {} KAS at the prices {} / {} of DAA {t0}; the position goes on from those prices with a {} KAS, b {} KAS", crate::tcmds::word(tk.side), fmt_kas(tk.amount as i128), fmt_kas(tk.a[0] as i128), fmt_kas(tk.a[1] as i128), fmt_kas(after.x.a_margin as i128), fmt_kas(after.x.b_margin as i128));
    show(&TObj::Change(c), &l.lib, None, net);
    done(a, "pos-take", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Change(c)), Made::Script(1, "KpfTChange")])
}

fn change_of(a: &Args, node: &mut N, net: Network) -> R<(crate::tcmds::Loaded, kpf_protocol::thin::Change)> {
    let l = load(a, "change", node, net)?;
    match l.obj {
        TObj::Change(c) => Ok((l, c)),
        o => Err(format!("--change is a {}, not a stated change", o.kind())),
    }
}

/// `change-settle`: anyone applies a stated change nobody disputed, once it is a dispute period old.
pub fn change_settle_cmd(a: &Args) -> R<()> {
    only(a, &["change", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, c) = change_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTChangeSettle", &mut node, net)?;
    let after = c.position.taken(&c.take).filter(|p| c.becomes(p));
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let b = tb::change_settle(&l.lib, &live(&l, c, true)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("applied as stated: the bond of {} KAS returns to the claimer", fmt_kas(c.bond as i128));
    let mut made = vec![Made::Script(1, "KpfTChangeSettle")];
    if let Some(p) = after {
        made.push(Made::Obj(0, TObj::Position(p)));
    }
    done(a, "change-settle", &b, &keys, net, &mut node, &l.lib, &made)
}

/// `change-dispute`: the funder asks for the proof of a stated change, with a bond as large as the claimer's.
pub fn change_dispute_cmd(a: &Args) -> R<()> {
    only(a, &["change", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, c) = change_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTChangeDispute", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, next) = tb::change_dispute(&l.lib, &live(&l, c, true)?, &s, fk, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    show(&TObj::Change(next), &l.lib, None, net);
    done(a, "change-dispute", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Change(next)), Made::Script(1, "KpfTChangeDispute")])
}

/// `change-lapse`: a disputed change nobody answered within the dispute period: the position again, both bonds back.
pub fn change_lapse_cmd(a: &Args) -> R<()> {
    only(a, &["change", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, c) = change_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTChangeDispute", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, again) = tb::change_lapse(&l.lib, &live(&l, c, true)?, &s, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("lapsed: the position again, and both bonds return");
    done(a, "change-lapse", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Position(again)), Made::Script(1, "KpfTChangeDispute")])
}

/// `change-late`: the funder ends a statement that landed outside the position's lateness of its time, and has its
/// bond.
pub fn change_late_cmd(a: &Args) -> R<()> {
    only(a, &["change", "script"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, c) = change_of(a, &mut node, net)?;
    let s = script_of(a, &l.lib, "KpfTChangeLate", &mut node, net)?;
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, again) = tb::change_late(&l.lib, &live(&l, c, true)?, &s, fk, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    println!("ended as late: the position again; the bond of {} KAS to the funder", fmt_kas(c.bond as i128));
    done(a, "change-late", &b, &keys, net, &mut node, &l.lib, &[Made::Obj(0, TObj::Position(again)), Made::Script(1, "KpfTChangeLate")])
}

/// `take-answer`: a stated take (`--change`) is answered by the committee's prices for its time, read in the
/// citations of `--note1` (and `--note2` for a second leg). The
/// verdict is the rule's: within, the take is applied and the claimer has its amount and its bond (and half of a
/// disputer's); not within, the position again, and half of the claimer's bond to the funder. A statement nobody
/// disputed is answered the same way: at once.
pub fn take_answer_cmd(a: &Args) -> R<()> {
    only(a, &["change", "script", "note1", "note2"])?;
    let net = network(a)?;
    let mut node = connect(a)?;
    let (l, c) = change_of(a, &mut node, net)?;
    let tk = c.take;
    let two = c.position.x.legs[1].size > 0;
    let mut cites = vec![];
    let mut view = vec![];
    for flag in if two { vec!["note1", "note2"] } else { vec!["note1"] } {
        let (cited, commit) = cited_of(a.need(flag)?, node.as_mut().ok_or("--node URL: a citation is read at the node")?)?;
        view.push((cited.witness.block, commit));
        cites.push(cited);
    }
    kpf_protocol::verify::set_chain_view(view);
    let s = script_of(a, &l.lib, "KpfTTake", &mut node, net)?;
    let first = copy_at(&l.lib, "KpfTCite", &mut node, net, &[])?;
    let copies = if two { vec![first.clone(), copy_at(&l.lib, "KpfTCite", &mut node, net, &[first.op])?] } else { vec![first] };
    let pairs: Vec<(&LibScript, &Cited)> = copies.iter().zip(&cites).collect();
    let (fk, keys, fu) = funding(a, net, &mut node)?;
    let (b, within, after) = tb::take_answer(&l.lib, &live(&l, c, true)?, &s, &pairs, fk, Payer { funds: &fu, change_to: fk, rate: rate(a)? }).map_err(e)?;
    let prices: Vec<String> = cites.iter().map(|x| fmt_kas(x.note.price as i128)).collect();
    if within {
        println!("answered: WITHIN the rule at the committee's prices {} of DAA {}: the take is applied, and the claimer has its {} KAS and its bond", prices.join(" / "), c.t0, fmt_kas(tk.amount as i128));
    } else {
        println!("answered: NOT WITHIN the rule at the committee's prices {} of DAA {}: the position again; half of the claimer's bond, {} KAS, to the funder", prices.join(" / "), c.t0, fmt_kas((c.bond / 2) as i128));
    }
    let mut made = vec![Made::Obj(0, TObj::Position(after)), Made::Script(1, "KpfTTake"), Made::Script(2, "KpfTCite")];
    if two {
        made.push(Made::Script(3, "KpfTCite"));
    }
    done(a, "take-answer", &b, &keys, net, &mut node, &l.lib, &made)
}
